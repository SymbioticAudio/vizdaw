import Foundation
import VizDAWCore

/// Whether a command button can run now (groups-vcas-transport Task 12):
/// as its menu item is enabled.
@MainActor
public enum CommandAvailability {
    /// The commands that need no document: `ControlCommands.run` answers
    /// them before it looks for one.
    public static var appWide: Set<String> {
        Set([
            "file.new", "file.create_demo", "file.create_demo_audio", "file.open", "window.plugin_manager",
            "keys.save", "keys.load", "keys.reset", AudioCommands.set, AutomationCommands.suspend,
            TransportCustomize.command,
        ])
        .union(AppearanceCommands.ids).union(TransportLayoutCommands.ids).union(PluginSettingsCommands.ids)
        .union(SurfaceCommands.ids).union(ControlCommands.coProducer)
    }

    /// The family's `enabled` (Transport, Drum, View, Automation, Step,
    /// Track, Region); any other command is enabled with a document; with
    /// none, only an app-wide command is.
    public static func enabled(_ id: String, doc: DocumentModel?) -> Bool {
        guard let m = doc else { return appWide.contains(id) }
        if TransportCommands.ids.contains(id) { return TransportCommands.enabled(id, m) }
        if DrumCommands.ids.contains(id) { return DrumCommands.enabled(id, m) }
        if ViewCommands.ids.contains(id) { return ViewCommands.enabled(id, m) }
        if AutomationCommands.ids.contains(id) { return AutomationCommands.enabled(id, m) }
        if StepCommands.ids.contains(id) { return StepCommands.enabled(id, m) }
        if TrackCommands.ids.contains(id) { return TrackCommands.enabled(id, m) }
        if RegionCommands.ids.contains(id) { return RegionCommands.enabled(id, m) }
        return true
    }
}

/// The icons a command button shows beside its label; most have none.
public enum CommandGlyphs {
    public static func icon(_ id: String) -> AppIcon? {
        switch id {
        case "window.mixer": .mixer
        case "editors.piano_roll": .pianoRoll
        case "editors.list": .list
        case "editors.drum_grid": .drumGrid
        case "transport.loop": .loop
        case "transport.record": .record
        default: nil
        }
    }
}

/// The app's main menu, as a command button falls back to it (Ruling 16).
@MainActor public protocol MenuPerformer {
    /// Performs the item titled `title` of the top-level menu `menu`;
    /// false when there is none, or it is disabled.
    func perform(menu: String, title: String) -> Bool
}

/// What a command button shows (Ruling 16): its label (the command's
/// title unless given), its icon, its tooltip and whether it can run. A
/// stale id (no registry row) is greyed with `staleTooltip`, never dropped.
public struct CommandButtonFace: Equatable, Sendable {
    public let label: String
    public let icon: AppIcon?
    public let tooltip: String
    public let accessibilityLabel: String
    public let enabled: Bool
    public let stale: Bool

    @MainActor
    public static func make(id: String, label: String?, row: BindingRow?, doc: DocumentModel?) -> CommandButtonFace {
        guard let row else {
            return CommandButtonFace(
                label: label ?? id, icon: nil, tooltip: CommandButtonAction.staleTooltip,
                accessibilityLabel: label ?? id, enabled: false, stale: true)
        }
        return CommandButtonFace(
            label: label ?? row.title, icon: CommandGlyphs.icon(id), tooltip: row.title,
            accessibilityLabel: row.title, enabled: CommandAvailability.enabled(id, doc: doc), stale: false)
    }
}

/// A command button's click (Ruling 16; spec §5: a user click, so its
/// command may fall through to its menu item, dialog and all).
@MainActor
public enum CommandButtonAction {
    /// A stale id's tooltip (pre-flight F18).
    public static let staleTooltip = "This command is no longer in VizDAW"

    /// The status line when neither the command nor its menu item ran.
    public static func cannotRun(_ title: String) -> String { "\(title) can't run from a button right now." }

    /// Ruling 16: `KeyDispatch.command`; on `.pass` (unavailable, or it
    /// needs its dialog), `menu.perform(row.menu, row.title)`; when that
    /// is false, the status line says so. A stale id does nothing.
    public static func run(
        _ id: String, doc: (id: UUID, model: DocumentModel)?, store: DocumentStore,
        host: any ControlHost, menu: any MenuPerformer
    ) {
        guard let row = host.keys.row(id) else {
            doc?.model.editor.status = cannotRun(id)
            return
        }
        switch KeyDispatch.command(id, doc: doc, store: store, host: host) {
        case .handled: break
        case .opened(let d): host.showDocument(d)
        case .pass:
            if !menu.perform(menu: row.menu, title: row.title) { doc?.model.editor.status = cannotRun(row.title) }
        }
    }
}

/// `ui.customize_transport` (Task 12): the View menu's sheet. Over the
/// channel it is only `dialog_required`; no sheet opens.
@MainActor
public enum TransportCustomize {
    public static let command = "ui.customize_transport"
    public static let title = "Customize Transport Bar…"

    public static func run() -> ControlOutcome {
        .failed(
            .of(
                .dialogRequired, "this would open the \"Customize Transport Bar\" dialog, which no parameter answers",
                dialog: "Customize Transport Bar"))
    }
}
