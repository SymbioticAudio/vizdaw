import Foundation
import Observation
import VizDAWCore

/// What the Track Synth sheet sets up (spec §6.7).
public struct TrackSynthDraft: Identifiable, Equatable, Sendable {
    public var id: UInt64 { lane }
    public let seq: UInt64
    /// The MIDI lane that plays the synth.
    public let lane: UInt64
    /// The synth: the lane's external MIDI destination.
    public let dest: UInt64
    public let synthName: String
    /// The first return channel, from 1.
    public var channel = 1
    public var stereo = false
    public var armMidi = false
    public var muteAfter = false
    /// The return lane is set up (the sheet then offers Measure and Done).
    public var tracked = false

    public init(seq: UInt64, lane: UInt64, dest: UInt64, synthName: String) {
        self.seq = seq
        self.lane = lane
        self.dest = dest
        self.synthName = synthName
    }
}

/// What New MIDI Destination adds (plan 5d-1 Ruling 25).
public struct MidiDestinationDraft: Identifiable, Equatable, Sendable {
    public var id: UInt64 { lane }
    public let seq: UInt64
    /// The lane that then plays to it.
    public let lane: UInt64
    public var name: String
    public var port: String
    /// From 1.
    public var channel = 1
    /// The system's MIDI destinations when the sheet opened.
    public let ports: [String]
}

/// What the destination editor edits (plan release-fixes Task 8): an
/// external MIDI destination's return latency, typed as text.
public struct EditDestinationDraft: Equatable, Sendable {
    public var dest: UInt64
    /// The sheet's title.
    public let name: String
    public var latencyText: String

    public init(dest: UInt64, name: String, latencyText: String) {
        self.dest = dest
        self.name = name
        self.latencyText = latencyText
    }
}

/// The lane menu's Track Synth… and New MIDI Destination… sheets (spec
/// §6.7, §6.9; plan 5d-2 Task 8). Under the control channel no sheet
/// opens: the status line names the command to use instead.
@Observable
@MainActor
public final class SynthSheets {
    @ObservationIgnored weak var document: DocumentModel?
    public var trackSynth: TrackSynthDraft?
    public var newDestination: MidiDestinationDraft?
    /// The destination editor's draft (plan release-fixes Task 8).
    public var editDestination: EditDestinationDraft?
    /// The Measure running for the sheet.
    public private(set) var measuring: MeasureJob?
    /// The last Measure's outcome, or why a sheet's action failed.
    public private(set) var message: String?
    /// Why New MIDI Destination's Add failed (its own: Measure's result
    /// never shows there; Task 8 review M3).
    public private(set) var destinationMessage: String?

    /// The MIDI destinations New MIDI Destination offers (tests replace it).
    public static var ports: () -> [String] = { midiDestinationPorts() }
    public static let trackSynthOnChannel = "Track Synth opens a sheet: use lane.track_synth over the control channel."
    public static let newDestinationOnChannel =
        "New MIDI Destination opens a sheet: use destination.add_midi over the control channel."

    public static let editDestinationOnChannel = "Use destination.set_return_latency or destination.measure_return."

    init(document: DocumentModel) { self.document = document }

    /// The external MIDI destination `lane` plays (Track Synth needs one).
    public func synth(of lane: LaneRow) -> DestRow? {
        guard let m = document, let d = lane.midiDest else { return nil }
        return SynthCommands.externalDestinations(m).first { $0.id == d }
    }

    // MARK: Track Synth

    public func beginTrackSynth(seq: UInt64, lane: LaneRow, channelOn: Bool = MediaModel.channelIsOn()) {
        guard let m = document else { return }
        guard !channelOn else {
            m.editor.status = Self.trackSynthOnChannel
            return
        }
        guard let d = synth(of: lane) else {
            m.editor.status = "Track Synth needs a track that plays an external MIDI destination."
            return
        }
        message = nil
        trackSynth = TrackSynthDraft(seq: seq, lane: lane.id, dest: d.id, synthName: d.name)
    }

    /// Sets up the return lane (one undo step "Track Synth"); the sheet
    /// stays open for Measure.
    public func commitTrackSynth() {
        guard let m = document, var d = trackSynth, !d.tracked else { return }
        do {
            m.apply(
                try m.session.trackSynth(
                    seq: d.seq, lane: d.lane, first: UInt16(clamping: max(d.channel, 1) - 1), stereo: d.stereo,
                    armMidi: d.armMidi, muteAfter: d.muteAfter))
            d.tracked = true
            trackSynth = d
            message = "\(d.synthName) audio records the return. Measure its latency with the synth connected."
        } catch {
            message = AlertItem(error, title: "").message
        }
    }

    /// The synth's return latency now, in ms.
    public func returnLatency(_ dest: UInt64) -> Float? {
        guard let m = document else { return nil }
        return SynthCommands.externalDestinations(m).first { $0.id == dest }?.returnLatencyMs
    }

    /// Measure (spec §6.7) off the main thread; one at a time.
    public func measure(_ dest: UInt64) {
        guard let m = document, measuring == nil else { return }
        message = "Measuring…"
        let job = MeasureJob(session: m.session, dest: dest)
        job.onDone = { [weak self] r in self?.measured(r) }
        measuring = job
    }

    private func measured(_ r: Result<MeasureRow, any Error>) {
        let dest = measuring?.dest
        measuring = nil
        guard let m = document else { return }
        switch r {
        case .success(let row):
            SynthCommands.applyMeasured(row, m)
            // The destination editor's field shows what was measured.
            if let d = editDestination, d.dest == dest { editDestination?.latencyText = Self.msText(row.returnLatencyMs) }
            message = String(format: "Return latency %.1f ms (%u of 5 notes heard).", row.returnLatencyMs, row.onsets)
        case .failure(let e):
            message = AlertItem(e, title: "").message
        }
    }

    public func closeTrackSynth() {
        trackSynth = nil
        if measuring == nil { message = nil }
    }

    // MARK: New MIDI Destination

    public func beginNewDestination(seq: UInt64, lane: LaneRow, channelOn: Bool = MediaModel.channelIsOn()) {
        guard let m = document else { return }
        guard !channelOn else {
            m.editor.status = Self.newDestinationOnChannel
            return
        }
        let ports = Self.ports()
        destinationMessage = nil
        newDestination = MidiDestinationDraft(
            seq: seq, lane: lane.id, name: ports.first ?? "Synth", port: ports.first ?? "", ports: ports)
    }

    /// Whether Add can run: a name and a port. The name's length counts
    /// Unicode scalars, as the core does (Task 8 review M4).
    public var canAddDestination: Bool {
        guard let d = newDestination else { return false }
        let name = d.name.trimmingCharacters(in: .whitespacesAndNewlines)
        return !name.isEmpty && name.unicodeScalars.count <= 255 && !d.port.isEmpty && (1...16).contains(d.channel)
    }

    /// Adds the destination ("Add Destination") and points the lane at it
    /// ("Set Lane Destination"): two undo steps (plan 5d-2 Ruling 7).
    public func commitNewDestination() {
        guard let m = document, let d = newDestination, canAddDestination else { return }
        let name = d.name.trimmingCharacters(in: .whitespacesAndNewlines)
        do {
            let before = (try? m.session.destinations()).map { Set($0.map(\.id)) }
            m.apply(try m.session.addMidiDestination(name: name, port: d.port, channel: UInt8(d.channel - 1)))
            let after = try? m.session.destinations()
            let added = before.flatMap { b in after?.last { !b.contains($0.id) } }
            if let added {
                m.editor.edit(.lane(edit: .setDest(seq: d.seq, lane: d.lane, dest: added.id)))
            } else {
                // Added, but not found again (Task 8 review M5): said.
                m.editor.status = "Added \(name); choose it in the track's destination menu."
            }
            newDestination = nil
        } catch {
            destinationMessage = AlertItem(error, title: "").message
        }
    }

    public func closeNewDestination() {
        newDestination = nil
        destinationMessage = nil
    }

    // MARK: Built-in Synth sounds (plan release-fixes Task 8)

    /// The Built-in Synth's sounds, in the window's order.
    public static let sounds: [BuiltinSound] = [.piano, .bass, .pad, .lead, .drums]

    /// What the window and the status line call `s`.
    public static func title(_ s: BuiltinSound) -> String {
        switch s {
        case .piano: "Piano"
        case .bass: "Bass"
        case .pad: "Pad"
        case .lead: "Lead"
        case .drums: "Drums"
        }
    }

    public static let notBuiltinSynth = "Only a Built-in Synth has these sounds."

    /// Whether `row` is a Built-in Synth: a MIDI endpoint that is not
    /// external (its sound may be none of the five, from a file).
    public static func isBuiltinSynth(_ row: DestRow) -> Bool { row.plugin == nil && !row.external }

    /// The sound Built-in Synth destination `dest` plays; nil for any other
    /// destination (or when the list cannot be read).
    public func sound(of dest: UInt64) -> BuiltinSound? {
        guard let m = document else { return nil }
        return ((try? m.session.destinations()) ?? []).first { $0.id == dest }?.builtinSound
    }

    /// Sets Built-in Synth `dest`'s sound: one undo step "Set Built-in Synth
    /// Sound", heard by every track that plays it (Decision 3).
    @discardableResult
    public func setSound(_ s: BuiltinSound, dest: UInt64) -> KeyOutcome {
        guard let m = document else { return .unavailable(Self.notBuiltinSynth) }
        guard let row = ((try? m.session.destinations()) ?? []).first(where: { $0.id == dest }),
            Self.isBuiltinSynth(row)
        else { return .unavailable(Self.notBuiltinSynth) }
        do {
            try m.session.setBuiltinSound(dest: dest, sound: s)
            m.apply(try m.session.snapshot())
            return .done
        } catch VizError.Busy(let message) {
            m.editor.status = message
            return .busy(message)
        } catch {
            m.editor.report(error)
            return .refused(m.editor.status ?? "The sound was not set.")
        }
    }

    // MARK: Destination editor (plan release-fixes Task 8)

    /// `ms` as the field shows it ("12", "12.5").
    static func msText(_ ms: Float) -> String { String(format: "%g", ms) }

    /// Opens the destination editor on external destination `dest`; any
    /// other destination opens nothing. Under the control channel no sheet
    /// opens: the status line names the commands to use instead.
    public func beginEditDestination(_ dest: UInt64, channelOn: Bool = MediaModel.channelIsOn()) {
        guard let m = document else { return }
        guard !channelOn else {
            m.editor.status = Self.editDestinationOnChannel
            return
        }
        guard let row = SynthCommands.externalDestinations(m).first(where: { $0.id == dest }) else { return }
        if measuring == nil { message = nil }
        editDestination = EditDestinationDraft(dest: dest, name: row.name, latencyText: Self.msText(row.returnLatencyMs))
    }

    /// Commits the field (Return or Done): parsed with
    /// `SynthCommands.parseMs`, 0 to 500 ms, one undo step "Set Return
    /// Latency" (none when unchanged). False, with no call, for any other
    /// text, or when the core refused it (the message says why).
    @discardableResult
    public func commitLatency() -> Bool {
        guard let m = document, let d = editDestination else { return false }
        guard let ms = SynthCommands.parseMs(d.latencyText) else {
            message = "The return latency is 0 to 500 ms."
            return false
        }
        if returnLatency(d.dest) == ms { return true }
        do {
            m.apply(try m.session.setReturnLatency(dest: d.dest, ms: ms))
            if measuring == nil { message = nil }
            return true
        } catch {
            message = AlertItem(error, title: "").message
            return false
        }
    }

    public func closeEditDestination() {
        editDestination = nil
        if measuring == nil { message = nil }
    }
}
