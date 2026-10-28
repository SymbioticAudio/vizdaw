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

/// The lane menu's Track Synth… and New MIDI Destination… sheets (spec
/// §6.7, §6.9; plan 5d-2 Task 8). Under the control channel no sheet
/// opens: the status line names the command to use instead.
@Observable
@MainActor
public final class SynthSheets {
    @ObservationIgnored weak var document: DocumentModel?
    public var trackSynth: TrackSynthDraft?
    public var newDestination: MidiDestinationDraft?
    /// The Measure running for the sheet.
    public private(set) var measuring: MeasureJob?
    /// The last Measure's outcome, or why a sheet's action failed.
    public private(set) var message: String?

    /// The MIDI destinations New MIDI Destination offers (tests replace it).
    public static var ports: () -> [String] = { midiDestinationPorts() }
    public static let trackSynthOnChannel = "Track Synth opens a sheet: use lane.track_synth over the control channel."
    public static let newDestinationOnChannel =
        "New MIDI Destination opens a sheet: use destination.add_midi over the control channel."

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
            m.editor.status = "Track Synth needs a lane that plays an external MIDI destination."
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
        measuring = nil
        guard let m = document else { return }
        switch r {
        case .success(let row):
            // No stale snapshot (M5): an edit made while Measure ran wins.
            if row.snapshot.version >= m.snapshot.version { m.apply(row.snapshot) } else { m.refreshAfterJob() }
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
        message = nil
        newDestination = MidiDestinationDraft(
            seq: seq, lane: lane.id, name: ports.first ?? "Synth", port: ports.first ?? "", ports: ports)
    }

    /// Whether Add can run: a name and a port.
    public var canAddDestination: Bool {
        guard let d = newDestination else { return false }
        let name = d.name.trimmingCharacters(in: .whitespacesAndNewlines)
        return !name.isEmpty && name.count <= 255 && !d.port.isEmpty && (1...16).contains(d.channel)
    }

    /// Adds the destination ("Add Destination") and points the lane at it
    /// ("Set Lane Destination"): two undo steps (plan 5d-2 Ruling 7).
    public func commitNewDestination() {
        guard let m = document, let d = newDestination, canAddDestination else { return }
        let name = d.name.trimmingCharacters(in: .whitespacesAndNewlines)
        do {
            let before = Set(((try? m.session.destinations()) ?? []).map(\.id))
            m.apply(try m.session.addMidiDestination(name: name, port: d.port, channel: UInt8(d.channel - 1)))
            let added = ((try? m.session.destinations()) ?? []).last { !before.contains($0.id) }
            if let added {
                m.editor.edit(.lane(edit: .setDest(seq: d.seq, lane: d.lane, dest: added.id)))
            }
            newDestination = nil
        } catch {
            message = AlertItem(error, title: "").message
        }
    }

    public func closeNewDestination() {
        newDestination = nil
        message = nil
    }
}
