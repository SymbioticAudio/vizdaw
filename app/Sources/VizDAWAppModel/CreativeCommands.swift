import Foundation
import VizDAWCore

/// Plan 8e Ruling 1's creative command, `automation.write`: automation as
/// real points, run through one Session call (Task 5). On the Co-Producer
/// socket it runs only through `co_producer.apply`, after its take
/// (`CoProducerCommands`). Co-Producer writes no notes (owner,
/// 2026-11-16), so there is no note command here.
///
/// It is one `runCreative` call with the arguments as given (the core
/// checks them), its snapshot applied, answering `status` with its title.
/// An open editor gesture is checked first, as 8d's mixer edits do.
/// Nothing here opens a dialog or a sheet.
@MainActor
public enum CreativeCommands {
    public static let writeAutomation = "automation.write"
    /// The registry's `CREATIVE_IDS`.
    public static let ids: Set<String> = [writeAutomation]
    /// The registry's titles: each command's status.
    public static let titles: [String: String] = [writeAutomation: "Write Automation"]

    /// Runs `id` (one of `ids`) on document `docID`: the gesture first,
    /// then the one session call. A refusal is `failed` in the session's
    /// words, its Busy `busy`.
    public static func run(_ id: String, args: [ControlArg], docID: UUID, _ m: DocumentModel) -> ControlOutcome {
        guard ids.contains(id) else { return .failed(.of(.unknownId, "VizDAW has no handler for \(id).")) }
        guard m.editor.gesture == nil else { return ControlCommands.unavailable(EditorModel.gestureOpenMessage) }
        do {
            m.apply(try m.session.runCreative(id: id, args: args))
        } catch {
            return .failed(CoProducerCommands.failure(error))
        }
        m.editor.status = titles[id]
        return .done(ControlCommands.ran(id, docID, m))
    }
}
