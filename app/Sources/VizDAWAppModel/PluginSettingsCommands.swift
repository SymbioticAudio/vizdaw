import Foundation
import VizDAWCore

/// `plugins.new_channel_eq` and `plugins.install_multiq` (plan 6g Task 4,
/// Ruling 10): app-wide, so they need no document, and they open nothing.
/// An install over something in the way is `dialog_required` "Replace"
/// unless `replace` is set; a build without Multi-Q is `unavailable`.
/// `plugins.default_eq` (plan 6g-2 Ruling 7) takes `multi-q`, `clap:<id>` or
/// `au:<type/subtype/manufacturer>` of an installed effect.
@MainActor
public enum PluginSettingsCommands {
    public static let newChannelEq = "plugins.new_channel_eq"
    public static let installMultiq = "plugins.install_multiq"
    public static let defaultEq = "plugins.default_eq"
    public static let ids: Set<String> = [newChannelEq, installMultiq, defaultEq]

    public static func run(_ id: String, args: [ControlArg], settings: PluginSettings) -> ControlOutcome {
        switch id {
        case newChannelEq:
            guard let on = args.flag("on") else {
                return .failed(.of(.invalidParams, "\(id): `on` is required"))
            }
            settings.setNewChannelEq(on)
            return ran(id, "new-channel EQ \(on ? "on" : "off")")
        case installMultiq:
            guard settings.bundledEqName != nil, settings.bundledAuPresent else {
                return .failed(.of(.unavailable, PluginSettings.notInThisBuild))
            }
            do {
                let path = try settings.installQuietly(replace: args.flag("replace") ?? false)
                return ran(id, path)
            } catch VizError.Exists {
                return .failed(.dialog("Replace", "replace"))
            } catch VizError.Model(let message) where message == PluginSettings.notInThisBuild {
                return .failed(.of(.unavailable, message))
            } catch {
                return .failed(.thrown(error, "Install Failed"))
            }
        case defaultEq:
            return setDefaultEq(id, args.text("plugin"), settings)
        default:
            return .failed(.of(.unknownId, "VizDAW has no handler for \(id)."))
        }
    }

    /// `plugin` is `multi-q`, or a prefixed ID matched against the installed
    /// effects (read again first); anything else changes nothing.
    private static func setDefaultEq(_ id: String, _ plugin: String?, _ settings: PluginSettings) -> ControlOutcome {
        guard let plugin else {
            return .failed(.of(.invalidParams, "\(id): `plugin` is required"))
        }
        if plugin == "multi-q" {
            settings.setDefaultEq(nil)
            return ran(id, "default EQ Multi-Q")
        }
        let parts = plugin.split(separator: ":", maxSplits: 1, omittingEmptySubsequences: false)
        let format: PluginFormatTag? =
            switch parts.count == 2 ? String(parts[0]) : "" {
            case "au": .au
            case "clap": .clap
            default: nil
            }
        guard let format, parts.count == 2, !parts[1].isEmpty, !plugin.contains("\u{0}") else {
            return .failed(
                .of(.invalidParams, "\(id): `plugin` is multi-q, clap:<id> or au:<type/subtype/manufacturer>"))
        }
        let pluginId = String(parts[1])
        settings.refreshEqChoices()
        guard let row = settings.eqChoices.first(where: { $0.format == format && $0.id == pluginId }) else {
            return .failed(.of(.unknownId, "No installed effect is \(plugin)."))
        }
        settings.setDefaultEq(row)
        return ran(id, "default EQ \(row.name)")
    }

    private static func ran(_ id: String, _ status: String) -> ControlOutcome {
        .done(.ran(command: id, doc: nil, window: nil, undoLabel: nil, status: status))
    }
}
