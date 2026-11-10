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
/// is `failed`. `destination.set_midi_mix` (plan 5g-1b) takes `volume`/`pan`
/// as integers 0 to 127 or null (the channel sends null as the text
/// `none`: unset) and `mute` as a flag; an absent key keeps the current
/// value. Nothing here opens a dialog.
@MainActor
public enum SynthCommands {
    public static let trackSynth = "lane.track_synth"
    public static let setReturnLatency = "destination.set_return_latency"
    public static let measureReturn = "destination.measure_return"
    /// A MIDI destination's strip mix (plan 5g-1b Task 4).
    public static let setMidiMix = "destination.set_midi_mix"
    public static let ids: Set<String> = [trackSynth, setReturnLatency, setMidiMix, measureReturn]

    /// What the channel sends for a null volume or pan: unset.
    static let unsetText = "none"

    /// `args`' `name` as a MIDI controller value: `.absent` when the key is
    /// not given, `.value(nil)` for null (unset), else an integer 0 to 127;
    /// nil for any other value (the wrong type, or out of range).
    enum Controller { case absent, value(UInt8?) }
    static func controller(_ args: [ControlArg], _ name: String) -> Controller? {
        let given = args.filter { $0.name == name }
        guard let a = given.first else { return .absent }
        guard given.count == 1 else { return nil }
        switch a.value {
        case .int(let v): return (0...127).contains(v) ? .value(UInt8(v)) : nil
        case .text(let t): return t == unsetText ? .value(nil) : nil
        case .flag: return nil
        }
    }

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

    /// Applies the snapshot of Measure's step. No stale snapshot (M5): an
    /// edit made while Measure ran wins.
    static func applyMeasured(_ row: MeasureRow, _ m: DocumentModel) {
        if row.snapshot.version >= m.snapshot.version { m.apply(row.snapshot) } else { m.refreshAfterJob() }
    }

    /// The answer of a Measure that ended: its milliseconds (its step was
    /// applied when it ended), or its failure (the core's `Busy` is
    /// `busy`); nil while it runs.
    public static func answer(_ job: MeasureJob, doc: UUID, _ m: DocumentModel) -> ControlOutcome? {
        switch job.result {
        case nil: return nil
        case .success(let row):
            // The step was applied when the job ended (`run`'s `onDone`).
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
                return .failed(.of(.invalidParams, "lane.track_synth: `seq` and `lane` name the MIDI track"))
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
        case setMidiMix:
            let usage = "destination.set_midi_mix: `dest` names an external MIDI destination; `volume` and `pan` are 0 to 127 or null, `mute` is true or false; give at least one"
            guard let dest = Self.id(args, "dest"), let volume = controller(args, "volume"),
                let pan = controller(args, "pan")
            else { return .failed(.of(.invalidParams, usage)) }
            var mute: Bool?
            if let a = args.first(where: { $0.name == "mute" }) {
                guard case .flag(let v) = a.value else { return .failed(.of(.invalidParams, usage)) }
                mute = v
            }
            if case .absent = volume, case .absent = pan, mute == nil {
                return .failed(.of(.invalidParams, usage))
            }
            // Absent keeps what is set; the list is the same one the check reads.
            guard let all = try? m.session.destinations() else {
                return .failed(.of(.failed, "Set MIDI Mix Failed: the destinations cannot be read"))
            }
            guard let row = all.first(where: { $0.external && $0.id == dest }) else { return notExternal(id, dest) }
            var mix = row.midiMix
            if case .value(let v) = volume { mix.volume = v }
            if case .value(let v) = pan { mix.pan = v }
            if let mute { mix.mute = mute }
            do {
                m.apply(try m.session.setMidiMix(dest: dest, mix: mix))
            } catch {
                return .failed(.thrown(error, "Set MIDI Mix Failed"))
            }
        case measureReturn:
            guard let dest = Self.id(args, "dest") else {
                return .failed(.of(.invalidParams, "destination.measure_return: `dest` names a destination"))
            }
            if isExternal(dest, m) == false { return notExternal(id, dest) }
            // Off the main thread (plan 5d-2 Ruling 6): the channel answers
            // when the job ends, and its pump never blocks.
            let job = MeasureJob(session: m.session, dest: dest)
            // The step reaches the model when the job ends, whether or not
            // a client still waits for the answer (Task 8 review M1).
            job.onDone = { [weak m] r in
                if case .success(let row) = r, let m { applyMeasured(row, m) }
            }
            return .measure(doc: doc, job: job)
        default:
            return .failed(.of(.unknownId, "VizDAW has no handler for \(id)."))
        }
        return .done(ControlCommands.ran(id, doc, m))
    }
}
