import Foundation
import VizDAWCore

/// Tracking a hardware synth from the control channel (plan 5c-2;
/// sub-project 5 spec §6.7, §12.1): `lane.track_synth` sets up the lane
/// recording a synth's return, `destination.set_return_latency` types a
/// synth's return latency and `destination.measure_return` measures it.
/// The Track Synth sheet, the lane menu item and the destination editor
/// are plan 5d's; they call the same session methods (Measure off the
/// main thread: it takes about three seconds).
///
/// Errors: a parameter of the wrong shape, and a `dest` that names no
/// external MIDI destination, are `invalid_params` and never reach the
/// core; the core's `Busy` (recording or playing, another measurement, an
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

    /// Whether `dest` names one of `m`'s external MIDI destinations.
    static func isExternal(_ dest: UInt64, _ m: DocumentModel) -> Bool {
        externalDestinations(m).contains { $0.id == dest }
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
            guard isExternal(dest, m) else { return notExternal(id, dest) }
            do {
                m.apply(try m.session.setReturnLatency(dest: dest, ms: ms))
            } catch {
                return .failed(.thrown(error, "Set Return Latency Failed"))
            }
        case measureReturn:
            guard let dest = Self.id(args, "dest") else {
                return .failed(.of(.invalidParams, "destination.measure_return: `dest` names a destination"))
            }
            guard isExternal(dest, m) else { return notExternal(id, dest) }
            // Blocks the main thread for about 2.7 s, 10 s at most (plan
            // 5c-2 Ruling 9 accepts it for the control channel; 5d's sheet
            // calls it off the main thread).
            do {
                let r = try m.session.measureReturn(dest: dest)
                m.apply(r.snapshot)
                return .done(
                    .measured(doc: doc.uuidString, dest: dest, returnLatencyMs: r.returnLatencyMs, onsets: r.onsets))
            } catch {
                return .failed(.thrown(error, "Measure Failed"))
            }
        default:
            return .failed(.of(.unknownId, "VizDAW has no handler for \(id)."))
        }
        return .done(ControlCommands.ran(id, doc, m))
    }
}
