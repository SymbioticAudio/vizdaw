import Foundation
import VizDAWCore

/// `lane.set_input_fx` (plan 5g-1 Task 7; spec §12.1): replaces a track's
/// MIDI input effect chain from JSON, as one undoable lane edit.
///
/// Errors: `seq` or `lane` that is not an id, a missing `chain`, or a chain
/// the core's `inputFxFromJson` refuses (malformed, a wrong type, an unknown
/// effect, a value out of range, too many effects) is `invalid_params` and
/// never reaches the session. The session's refusal (an unknown track, a
/// track that takes no MIDI) is `failed`; its `Busy` is `busy`. An open
/// editor drag is `unavailable`. Nothing here opens a dialog or a window.
@MainActor
public enum InputFxCommands {
    public static let setInputFx = "lane.set_input_fx"
    public static let ids: Set<String> = [setInputFx]

    static func id(_ args: [ControlArg], _ name: String) -> UInt64? {
        args.int(name).flatMap { UInt64(exactly: $0) }
    }

    /// Runs `id` (one of `ids`) on document `doc`.
    public static func run(_ id: String, args: [ControlArg], doc: UUID, _ m: DocumentModel) -> ControlOutcome {
        guard m.editor.gesture == nil else { return ControlCommands.unavailable(EditorModel.gestureOpenMessage) }
        guard id == setInputFx else { return .failed(.of(.unknownId, "VizDAW has no handler for \(id).")) }
        guard let seq = Self.id(args, "seq"), let lane = Self.id(args, "lane") else {
            return .failed(.of(.invalidParams, "lane.set_input_fx: `seq` and `lane` name the track"))
        }
        guard let text = args.text("chain") else {
            return .failed(.of(.invalidParams, "lane.set_input_fx: `chain` is the effect chain as JSON"))
        }
        let chain: [InputFxRow]
        do {
            chain = try inputFxFromJson(text: text)
        } catch {
            return .failed(.of(.invalidParams, "lane.set_input_fx: \(AlertItem(error, title: "").message)"))
        }
        do {
            m.apply(try m.session.edit(cmd: .lane(edit: .setInputFx(seq: seq, lane: lane, chain: chain)), filter: nil))
        } catch {
            return .failed(.thrown(error, "Set Input Effects Failed"))
        }
        return .done(ControlCommands.ran(id, doc, m))
    }
}
