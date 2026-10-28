import Foundation
import VizDAWCore

/// Tracking a hardware synth from the control channel (plan 5c-2;
/// sub-project 5 spec §6.7, §12.1): `lane.track_synth` sets up the lane
/// recording a synth's return, `destination.set_return_latency` types a
/// synth's return latency and `destination.measure_return` measures it.
/// The Track Synth sheet and the lane menu (plan 5d-2 Task 8) call the
/// same session methods; Measure runs off the main thread for both (it
/// takes about three seconds).
///
/// Errors: a parameter of the wrong shape, and a `dest` (or
/// `lane.set_input`'s `return_of`) that names no external MIDI
/// destination, are `invalid_params` and never reach the core. Only
/// destination ids are checked here: an unknown `seq` or `lane`, or a
/// lane that plays no external synth, is the core's refusal, `failed`
/// (Task 10 review M3); the core's `Busy` (recording or playing, another measurement, an
/// open edit, an edit opened while Measure ran with the value it measured)
/// is `busy` with its message unchanged; any other refusal (no lane
/// records the return, no signal, the measurement timed out or stopped)
/// is `failed`. Nothing here opens a dialog.
@MainActor
public enum SynthCommands {
    public static let trackSynth = "lane.track_synth"
    public static let setReturnLatency = "destination.set_return_latency"
    public static let measureReturn = "destination.measure_return"
    public static let ids: Set<String> = [trackSynth, setReturnLatency, measureReturn]

    /// The largest return latency (spec §4.5a: 0–500 ms).
    public static let maxReturnLatencyMs: Float = 500

    /// `text` as milliseconds: a plain decimal number from 0 to 500
    /// ("12.5"); nil for anything else.
    public static func parseMs(_ text: String) -> Float? {
        let t = text.trimmingCharacters(in: .whitespaces)
        guard !t.isEmpty, t.count <= 16, t.allSatisfy({ $0.isASCII && ($0.isNumber || $0 == ".") }),
            let v = Float(t), v.isFinite, (0...maxReturnLatencyMs).contains(v)
        else { return nil }
        return v
    }

    /// The external MIDI destinations (spec §4.5a): what Track Synth plays
    /// and Measure measures.
    public static func externalDestinations(_ m: DocumentModel) -> [DestRow] {
        ((try? m.session.destinations()) ?? []).filter(\.external)
    }

    /// Whether `dest` names one of `m`'s external MIDI destinations; nil
    /// when the list cannot be read (the core, which checks again, then
    /// decides: Task 10 review M2).
    static func isExternal(_ dest: UInt64, _ m: DocumentModel) -> Bool? {
        guard let all = try? m.session.destinations() else { return nil }
        return all.contains { $0.external && $0.id == dest }
    }

    /// The time, from its start, a call of `id` may block for, in ms:
    /// Measure 10 s at most (the core's `MEASURE_WAIT`) plus a margin; nil
    /// for the others. The channel extends the call's timeout to it before
    /// running it, so a call queued behind others is not cut off (Task 10
    /// review M6).
    public static func extraTimeMs(_ id: String) -> UInt64? {
        id == measureReturn ? 15_000 : nil
    }

    /// Whether the channel runs a call of `id` now. A call that blocks
    /// (Measure) runs only when `extend` gave it `extraTimeMs(_:)` (true:
    /// its client still waits); one whose client was already answered
    /// `timeout` while it was queued is skipped, so it never measures and
    /// adds a step nobody asked for (final review finding 3). Other calls
    /// always run.
    public static func shouldRun(_ id: String, extend: (UInt64) -> Bool) -> Bool {
        guard let extra = extraTimeMs(id) else { return true }
        return extend(extra)
    }

    /// The answer of a Measure that ended: its milliseconds, with the step
    /// applied, or its failure (the core's `Busy` is `busy`); nil while it
    /// runs.
    public static func answer(_ job: MeasureJob, doc: UUID, _ m: DocumentModel) -> ControlOutcome? {
        switch job.result {
        case nil: return nil
        case .success(let row):
            // No stale snapshot (M5): an edit made while Measure ran wins.
            if row.snapshot.version >= m.snapshot.version { m.apply(row.snapshot) } else { m.refreshAfterJob() }
            return .done(
                .measured(doc: doc.uuidString, dest: job.dest, returnLatencyMs: row.returnLatencyMs, onsets: row.onsets))
        case .failure(let error):
            return .failed(.thrown(error, "Measure Failed"))
        }
    }

    static func id(_ args: [ControlArg], _ name: String) -> UInt64? {
        args.int(name).flatMap { UInt64(exactly: $0) }
    }

    static func notExternal(_ command: String, _ dest: UInt64) -> ControlOutcome {
        .failed(.of(.invalidParams, "\(command): destination \(dest) is not an external MIDI destination"))
    }

    /// Runs `id` (one of `ids`) on document `doc`.
    public static func run(_ id: String, args: [ControlArg], doc: UUID, _ m: DocumentModel) -> ControlOutcome {
        guard m.editor.gesture == nil else { return ControlCommands.unavailable(EditorModel.gestureOpenMessage) }
        switch id {
        case trackSynth:
            guard let seq = Self.id(args, "seq"), let lane = Self.id(args, "lane") else {
                return .failed(.of(.invalidParams, "lane.track_synth: `seq` and `lane` name the MIDI lane"))
            }
            let channel = args.int("channel") ?? 1
            guard (1...256).contains(channel) else {
                return .failed(.of(.invalidParams, "lane.track_synth: `channel` is 1 to 256"))
            }
            do {
                m.apply(
                    try m.session.trackSynth(
                        seq: seq, lane: lane, first: UInt16(channel - 1), stereo: args.flag("stereo") ?? false,
                        armMidi: args.flag("arm_midi") ?? false, muteAfter: args.flag("mute_after") ?? false))
            } catch {
                return .failed(.thrown(error, "Track Synth Failed"))
            }
        case setReturnLatency:
            guard let dest = Self.id(args, "dest"), let ms = args.text("ms").flatMap(parseMs) else {
                return .failed(
                    .of(
                        .invalidParams,
                        "destination.set_return_latency: `dest` names a destination and `ms` is 0 to 500 (\"12.5\")"))
            }
            if isExternal(dest, m) == false { return notExternal(id, dest) }
            do {
                m.apply(try m.session.setReturnLatency(dest: dest, ms: ms))
            } catch {
                return .failed(.thrown(error, "Set Return Latency Failed"))
            }
        case measureReturn:
            guard let dest = Self.id(args, "dest") else {
                return .failed(.of(.invalidParams, "destination.measure_return: `dest` names a destination"))
            }
            if isExternal(dest, m) == false { return notExternal(id, dest) }
            // Off the main thread (plan 5d-2 Ruling 6): the channel answers
            // when the job ends, and its pump never blocks.
            return .measure(doc: doc, job: MeasureJob(session: m.session, dest: dest))
        default:
            return .failed(.of(.unknownId, "VizDAW has no handler for \(id)."))
        }
        return .done(ControlCommands.ran(id, doc, m))
    }
}
