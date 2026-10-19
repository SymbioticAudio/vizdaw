import Foundation
import VizDAWCore

/// What the user chose after a crash (spec §10): "Open with Plugins Off /
/// Open Normally / Cancel", plus "Disable ‹plugin›" when one was loading.
public enum CrashChoice: Equatable, Sendable {
    case pluginsOff
    case normally
    case cancel
    /// Disable the plugins that were loading, then open normally.
    case disable
}

/// The questions the store asks before opening a project.
@MainActor
public protocol RecoveryQuestions: AnyObject {
    /// VizDAW quit unexpectedly while `project` was open (`crashed`), or
    /// Shift was held while opening it.
    func afterCrash(project: String, loading: [PluginRefRow], crashed: Bool) -> CrashChoice
    /// A newer autosave of `project` exists: restore it?
    func restoreAutosave(project: String) -> Bool
}

/// How the store checks and opens projects (the app gives the real ones;
/// tests give fakes).
@MainActor
public struct Recovery {
    public var check: (String) throws -> RecoveryInfo
    public var openWith: (String, OpenOptions) throws -> any SessionProtocol
    /// Disables plugins in the Plugin Manager; false when that was not
    /// possible (the project then opens with plugins off).
    public var disable: ([PluginRefRow]) -> Bool
    public var shiftHeld: () -> Bool
    public var questions: any RecoveryQuestions

    public init(
        check: @escaping (String) throws -> RecoveryInfo,
        openWith: @escaping (String, OpenOptions) throws -> any SessionProtocol,
        disable: @escaping ([PluginRefRow]) -> Bool,
        shiftHeld: @escaping () -> Bool,
        questions: any RecoveryQuestions
    ) {
        self.check = check
        self.openWith = openWith
        self.disable = disable
        self.shiftHeld = shiftHeld
        self.questions = questions
    }

    /// The options to open `path` with, or nil when the user cancelled.
    public func options(for path: String) -> OpenOptions? {
        var options = OpenOptions(pluginsOff: false, autosave: nil)
        guard let info = try? check(path) else { return options }
        let name = ((path as NSString).lastPathComponent as NSString).deletingPathExtension
        if info.crashed || shiftHeld() {
            switch questions.afterCrash(project: name, loading: info.loading, crashed: info.crashed) {
            case .cancel: return nil
            case .pluginsOff: options.pluginsOff = true
            case .normally: break
            case .disable:
                // Never open normally with the crashing plugin still enabled.
                if !disable(info.loading) { options.pluginsOff = true }
            }
        }
        if let a = info.autosave, questions.restoreAutosave(project: name) {
            options.autosave = a
        }
        return options
    }
}
