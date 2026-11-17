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
}
