import Foundation
import VizDAWCore

/// `ui.theme` (plan 7b Ruling 11): sets the app's theme, a built-in one or
/// a user theme (R-UT). App-wide, so it needs no document, and it opens
/// nothing.
@MainActor
public enum AppearanceCommands {
    public static let ids: Set<String> = ["ui.theme"]

    public static func run(_ args: [ControlArg], prefs: AppearancePreferences) -> ControlOutcome {
        guard let id = args.text("theme") else {
            return .failed(.of(.invalidParams, "ui.theme: `theme` names the theme"))
        }
        guard prefs.choose(id) else { return .failed(.of(.invalidParams, "\(id.prefix(80)) is not a theme.")) }
        return .done(.ran(command: "ui.theme", doc: nil, window: nil, undoLabel: nil, status: "theme \(id)"))
    }
}
