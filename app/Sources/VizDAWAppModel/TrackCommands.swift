import Foundation
import VizDAWCore

/// The Track menu's commands (plan tracks Ruling 5): New Audio, Instrument
/// and MIDI Track, Duplicate, Delete and Rename. One implementation for the
/// menu items, the keys and the control channel's `command.run`. None opens
/// a dialog: Rename from the menu starts the header's rename field, and over
/// the channel it takes `name`.
@MainActor
public enum TrackCommands {
    public static let newAudio = "track.new_audio"
    public static let newInstrument = "track.new_instrument"
    public static let newMidi = "track.new_midi"
    public static let duplicate = "track.duplicate"
    public static let delete = "track.delete"
    public static let rename = "track.rename"
    /// Make Mine (plan 8b Ruling 1): the composer takes over a Co-Producer
    /// track, its strip, and that strip's sends and plug-ins. Never
    /// Co-Producer's own call.
    public static let makeMine = "track.make_mine"
    public static let ids: Set<String> = [newAudio, newInstrument, newMidi, duplicate, delete, rename, makeMine]

    /// Set Track Output (release-fixes Task 6): an audio track's output
    /// path, or none. Key-only; the header's output menu runs it too.
    public static let setOutput = "lane.set_output"
    /// The output menu's first item, and the status after choosing it.
    public static let noOutput = "No Output"
    /// Why a MIDI or instrument track has no output to set.
    public static let notAudio = "Only an audio track has an output."
    /// Why the output waits for Stop.
    public static let recordingOutput = "Stop recording to change a track's output."
    /// A channel click on an audio track's output: the menu tracks modally,
    /// so the channel opens none.
    public static let outputOnChannel =
        "The output menu opens on a click: use lane.set_output over the control channel."

    static let noSequence = "Select a sequence first."
    static let noTrack = "Select a track in the arrangement first."
    /// Rename Track needs the arrangement's header on screen (Ruling 5, I11).
    public static let showArrange = "Show Arrange to rename a track."
    /// The core's own Busy text while recording (`RECORDING_LANES`): the
    /// same words whether Swift or the core refuses.
    public static let recordingBusy = "Stop recording to change which tracks record."
    /// A channel click on the "+ Add Track" row (Ruling 5): `NSMenu.popUp`
    /// tracks modally, so the channel opens no menu.
    public static let addTrackOnChannel =
        "Add Track opens a menu: use track.new_audio, track.new_instrument or track.new_midi over the control channel."

    /// The kind a New id makes.
    public static func kind(of id: String) -> TrackKindRow? {
        switch id {
        case newAudio: .audio
        case newInstrument: .instrument
        case newMidi: .midi
        default: nil
        }
    }

    /// Whether `id` can run on `m` now (its menu item is enabled): the New
    /// items need a selected sequence, the others also a selected track.
    /// Make Mine needs the selection's first track to be Co-Producer's.
    public static func enabled(_ id: String, _ m: DocumentModel) -> Bool {
        guard ids.contains(id), m.selectedSeq != nil else { return false }
        if id == makeMine { return m.selectedLane?.coProducer == true }
        return kind(of: id) != nil || m.selectedLane != nil
    }

    /// The menu and key entry: acts on the selected sequence and track.
    @discardableResult
    public static func perform(_ id: String, _ m: DocumentModel) -> KeyOutcome {
        guard ids.contains(id) else { return .unavailable("There is no command \(id).") }
        guard let seq = m.selectedSeq else { return .unavailable(noSequence) }
        if kind(of: id) == nil, m.selectedLane == nil { return .unavailable(noTrack) }
        return execute(id, seq: seq, lane: m.selectedLane?.id, after: nil, name: nil, m)
    }

    /// A menu or header-menu item: `perform`, and what it cannot do shows
    /// in the status line (a refusal already does).
    public static func performFromMenu(_ id: String, _ m: DocumentModel) {
        if case .unavailable(let why) = perform(id, m) { m.editor.status = why }
    }

    /// The track header's Make Mine, for the track it was opened on; what
    /// it cannot do shows in the status line.
    public static func makeMineFromMenu(seq: UInt64, lane: UInt64, _ m: DocumentModel) {
        if case .unavailable(let why) = execute(makeMine, seq: seq, lane: lane, after: nil, name: nil, m) {
            m.editor.status = why
        }
    }

    /// Runs `id` for `seq` (and `lane`, `after`, `name` where it takes them).
    static func execute(
        _ id: String, seq: UInt64, lane: UInt64?, after: UInt64?, name: String?, _ m: DocumentModel
    ) -> KeyOutcome {
        if let refused = waitRefusal(m) { return refused }
        if let kind = kind(of: id) { return m.newTrack(kind, seq: seq, after: after) }
        guard let lane else { return .unavailable(noTrack) }
        switch id {
        case duplicate:
            let before = Set(m.lanes.map(\.id))
            do {
                m.apply(try m.session.duplicateTrack(seq: seq, lane: lane))
                m.editor.refreshSelection()
                // The copy is selected, so the next Rename or Delete acts on it.
                if seq == m.selectedSeq, let copy = m.lanes.first(where: { !before.contains($0.id) }) {
                    m.editor.setSelection(SelectionRow(timeRange: nil, lanes: [LaneKey(seq, copy.id).ref], events: []))
                }
                return .done
            } catch VizError.Busy(let message) {
                m.editor.status = message
                return .busy(message)
            } catch {
                m.editor.report(error)
                return .refused(m.editor.status ?? "Duplicate Track was refused.")
            }
        case delete:
            return m.editor.edit(.lane(edit: .remove(seq: seq, lane: lane)))
                ? .done : .refused(m.editor.status ?? "Delete Track was refused.")
        case makeMine:
            do {
                m.apply(try m.session.makeMineTrack(seq: seq, lane: lane))
                m.editor.refreshSelection()
                return .done
            } catch VizError.Busy(let message) {
                m.editor.status = message
                return .busy(message)
            } catch {
                m.editor.report(error)
                return .refused(m.editor.status ?? "Make Mine was refused.")
            }
        case rename:
            if let name {
                return m.editor.edit(.lane(edit: .rename(seq: seq, lane: lane, name: name)))
                    ? .done : .refused(m.editor.status ?? "Rename Track was refused.")
            }
            guard let begin = m.beginTrackRename else { return .unavailable(showArrange) }
            return begin(seq, lane) ? .done : .unavailable(showArrange)
        default:
            return .unavailable("There is no command \(id).")
        }
    }

    /// Why an edit cannot start now: an open drag, or a recording. The core
    /// refuses New and Duplicate while recording; Delete and Rename it does
    /// not, so all five (and the take commands) are refused here, before
    /// any call. A recording's refusal shows in the status line.
    static func waitRefusal(_ m: DocumentModel) -> KeyOutcome? {
        guard m.editor.gesture == nil else { return .unavailable(EditorModel.gestureOpenMessage) }
        m.record.refresh(quiet: true)
        if m.record.isRecording {
            m.editor.status = recordingBusy
            return .busy(recordingBusy)
        }
        return nil
    }

    /// The channel's whole-number argument `name`: absent, a value, or bad.
    static func number(_ args: [ControlArg], _ name: String) -> (value: UInt64?, bad: Bool) {
        guard args.contains(where: { $0.name == name }) else { return (nil, false) }
        guard let v = args.int(name).flatMap({ UInt64(exactly: $0) }) else { return (nil, true) }
        return (v, false)
    }

    /// `invalid_params` for a bad whole-number argument of `id`.
    static func badNumber(_ id: String, _ name: String) -> ControlOutcome {
        .failed(.of(.invalidParams, "\(id): `\(name)` is a whole number from 0"))
    }

    /// The track a channel call acts on: sequence `seq` (else the shown
    /// one) and track `lane` (else the selection's first track, in the
    /// shown sequence), with the sequence's tracks; or the refusal
    /// `track.rename` gives for an unknown sequence or track.
    static func target(
        seq seqArg: UInt64?, lane laneArg: UInt64?, _ m: DocumentModel
    ) -> (seq: UInt64, row: LaneRow?, rows: [LaneRow], refusal: ControlOutcome?) {
        guard let seq = seqArg ?? m.selectedSeq else { return (0, nil, [], ControlCommands.unavailable(noSequence)) }
        guard m.snapshot.sequences.contains(where: { $0.id == seq }) else {
            return (seq, nil, [], .failed(.of(.failed, "There is no sequence \(seq).")))
        }
        let rows = seq == m.selectedSeq ? m.lanes : ((try? m.session.lanes(seq: seq)) ?? [])
        if let l = laneArg {
            guard let row = rows.first(where: { $0.id == l }) else { return (seq, nil, rows, missing(l)) }
            return (seq, row, rows, nil)
        }
        let selected = seq == m.selectedSeq ? m.selectedLane?.id : nil
        return (seq, rows.first { $0.id == selected }, rows, nil)
    }

    /// "There is no track `lane` in this sequence."
    static func missing(_ lane: UInt64) -> ControlOutcome {
        .failed(.of(.failed, "There is no track \(lane) in this sequence."))
    }

    /// The channel's entry. `seq` defaults to the shown sequence and `lane`
    /// to the selection's first track; ids that name nothing are refused in
    /// track wording, and a blank `name` is the core's refusal, passed on.
    public static func run(_ id: String, args: [ControlArg], doc: UUID, _ m: DocumentModel) -> ControlOutcome {
        if id == setOutput { return runSetOutput(args, doc: doc, m) }
        let seqArg = number(args, "seq"), laneArg = number(args, "lane"), afterArg = number(args, "after")
        if seqArg.bad { return badNumber(id, "seq") }
        if laneArg.bad { return badNumber(id, "lane") }
        if afterArg.bad { return badNumber(id, "after") }
        var name: String?
        if args.contains(where: { $0.name == "name" }) {
            guard let n = args.text("name") else { return .failed(.of(.invalidParams, "\(id): `name` is text")) }
            name = n
        }
        let t = target(seq: seqArg.value, lane: laneArg.value, m)
        if let refusal = t.refusal { return refusal }
        if let a = afterArg.value, !t.rows.contains(where: { $0.id == a }) { return missing(a) }
        // A New command ignores the track; the others act on it.
        let lane = t.row?.id
        if lane == nil, kind(of: id) == nil { return ControlCommands.unavailable(noTrack) }
        let seq = t.seq
        if id == rename, name == nil { return .failed(.dialog("Rename Track", "name")) }
        let outcome = execute(id, seq: seq, lane: lane, after: afterArg.value, name: name, m)
        if let f = ControlCommands.failure(outcome) { return f }
        return .done(ControlCommands.ran(id, doc, m))
    }

    /// Sets track `lane`'s output to `output` (nil: none) as one undo step,
    /// Set Track Output; the status names the output, or "No Output".
    /// Refused during an open drag or a recording, and for an output the
    /// document does not have, with nothing sent.
    @discardableResult
    public static func setOutput(seq: UInt64, lane: UInt64, output: UInt64?, _ m: DocumentModel) -> KeyOutcome {
        if let refused = outputWait(m) { return refused }
        var name = noOutput
        if let output {
            let path = pathName(output, m)
            if let refused = path.refused { return refused }
            guard let found = path.name else { return .refused(unknownOutput(output)) }
            name = found
        }
        guard m.editor.edit(.lane(edit: .setOutput(seq: seq, lane: lane, output: output))) else {
            return .refused(m.editor.status ?? "Set Track Output was refused.")
        }
        m.editor.status = name
        return .done
    }

    /// Why the output cannot change now: an open drag, or a recording
    /// (which shows in the status line).
    static func outputWait(_ m: DocumentModel) -> KeyOutcome? {
        guard m.editor.gesture == nil else { return .unavailable(EditorModel.gestureOpenMessage) }
        m.record.refresh(quiet: true)
        guard !m.record.isRecording else {
            m.editor.status = recordingOutput
            return .busy(recordingOutput)
        }
        return nil
    }

    /// Output path `id`'s name (nil when the document has none such), or
    /// why the paths could not be read.
    static func pathName(_ id: UInt64, _ m: DocumentModel) -> (name: String?, refused: KeyOutcome?) {
        do {
            return (try m.session.outputPaths().first { $0.id == id }?.name, nil)
        } catch VizError.Busy(let message) {
            m.editor.status = message
            return (nil, .busy(message))
        } catch {
            m.editor.report(error)
            return (nil, .refused(m.editor.status ?? "Set Track Output was refused."))
        }
    }

    /// "There is no output `n`."
    static func unknownOutput(_ n: UInt64) -> String { "There is no output \(n)." }

    /// The channel's `lane.set_output`: `seq` and `lane` default as the
    /// Track commands' do; `output` is an id or null (the text "none").
    public static func runSetOutput(_ args: [ControlArg], doc: UUID, _ m: DocumentModel) -> ControlOutcome {
        let id = setOutput
        let seqArg = number(args, "seq"), laneArg = number(args, "lane")
        if seqArg.bad { return badNumber(id, "seq") }
        if laneArg.bad { return badNumber(id, "lane") }
        let given = args.filter { $0.name == "output" }
        let output: UInt64?
        switch (given.count, given.first?.value) {
        case (1, .int(let v)?) where v >= 0: output = UInt64(v)
        case (1, .text(let t)?) where t == SynthCommands.unsetText: output = nil
        default: return .failed(.of(.invalidParams, "\(id): `output` is an output id from 0, or null for none"))
        }
        let t = target(seq: seqArg.value, lane: laneArg.value, m)
        if let refusal = t.refusal { return refusal }
        guard let row = t.row else { return ControlCommands.unavailable(noTrack) }
        guard row.kind == .audio else { return .failed(.of(.failed, notAudio)) }
        if let refused = outputWait(m), let f = ControlCommands.failure(refused) { return f }
        if let output {
            let path = pathName(output, m)
            if let refused = path.refused, let f = ControlCommands.failure(refused) { return f }
            if path.name == nil { return .failed(.of(.unknownId, unknownOutput(output))) }
        }
        let outcome = setOutput(seq: t.seq, lane: row.id, output: output, m)
        if let f = ControlCommands.failure(outcome) { return f }
        return .done(ControlCommands.ran(id, doc, m))
    }
}
