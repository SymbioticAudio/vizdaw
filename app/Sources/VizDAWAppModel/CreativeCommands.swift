import Foundation
import VizDAWCore

/// Plan 8e Ruling 1's creative command, `automation.write`: automation as
/// real points, run through one Session call (Task 5). On the Co-Producer
/// socket it runs only through `co_producer.apply`, after its take
/// (`CoProducerCommands`). Co-Producer writes no notes (owner,
/// 2026-11-16), so there is no note command here.
///
/// Until Task 5 it answers `unavailable` "Not ready yet." and changes
/// nothing. Nothing here opens a dialog.
@MainActor
public enum CreativeCommands {
    public static let writeAutomation = "automation.write"
    /// The registry's `CREATIVE_IDS`.
    public static let ids: Set<String> = [writeAutomation]
    /// What a creative command answers until it is built.
    public static let notReady = "Not ready yet."

    /// Runs `id` (one of `ids`) on document `docID`.
    public static func run(_ id: String, args: [ControlArg], docID: UUID, _ m: DocumentModel) -> ControlOutcome {
        .failed(.of(.unavailable, notReady))
    }
}
