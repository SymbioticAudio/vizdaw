import Foundation
import VizDAWCore

/// `plugins.new_channel_eq` and `plugins.install_multiq` (plan 6g Task 4,
/// Ruling 10): app-wide, so they need no document, and they open nothing.
/// An install over something in the way is `dialog_required` "Replace"
/// unless `replace` is set; a build without Multi-Q is `unavailable`.
@MainActor
public enum PluginSettingsCommands {
    public static let newChannelEq = "plugins.new_channel_eq"
    public static let installMultiq = "plugins.install_multiq"
    public static let ids: Set<String> = [newChannelEq, installMultiq]

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
        default:
            return .failed(.of(.unknownId, "VizDAW has no handler for \(id)."))
        }
    }

    private static func ran(_ id: String, _ status: String) -> ControlOutcome {
        .done(.ran(command: id, doc: nil, window: nil, undoLabel: nil, status: status))
    }
}
