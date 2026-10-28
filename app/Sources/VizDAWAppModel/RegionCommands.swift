import Foundation
import VizDAWCore

/// The Region menu (spec §10.2; plan 5b Task 8): one implementation for
/// the menu items and the control channel's `command.run`, acting on the
/// selected events (the core ignores those that are not regions).
@MainActor
public enum RegionCommands {
    public static let normalize = "region.normalize"
    public static let fadeIn = "region.fade_in_to_cursor"
    public static let fadeOut = "region.fade_out_from_cursor"
    public static let removeFades = "region.remove_fades"

    public static let ids: Set<String> = [normalize, fadeIn, fadeOut, removeFades]

    /// The Normalize panel's starting level (Ruling 18).
    public static let defaultPeakDb = -0.3

    static let noSelection = "Select audio regions first."

    public static func enabled(_ id: String, _ m: DocumentModel) -> Bool {
        ids.contains(id) && !m.editor.selection.events.isEmpty && m.editor.gesture == nil
    }

    public static func isOn(_ id: String, _ m: DocumentModel) -> Bool { false }

    /// Runs `id` on the selection. Normalize uses the default level; the
    /// menu item asks for one first and calls `normalize(peakDb:_:)`.
    @discardableResult
    public static func run(_ id: String, on: Bool? = nil, _ m: DocumentModel) -> KeyOutcome {
        guard ids.contains(id) else { return .unavailable("VizDAW has no handler for \(id).") }
        if id == normalize { return Self.normalize(peakDb: defaultPeakDb, m) }
        let events = m.editor.selection.events
        guard !events.isEmpty else { return .unavailable(noSelection) }
        guard m.editor.gesture == nil else { return .unavailable(EditorModel.gestureOpenMessage) }
        let cmd: EditCommand =
            id == removeFades
            ? .audio(edit: .removeFades(events: events))
            : .audio(edit: .fadeTo(events: events, at: m.transport.tick, fadeIn: id == fadeIn))
        return m.editor.edit(cmd) ? .done : .refused(m.editor.status ?? "The region command was refused.")
    }

    /// Normalize… to `peakDb` (−96…0 dBFS).
    @discardableResult
    public static func normalize(peakDb: Double, _ m: DocumentModel) -> KeyOutcome {
        let events = m.editor.selection.events
        guard !events.isEmpty else { return .unavailable(noSelection) }
        guard m.editor.gesture == nil else { return .unavailable(EditorModel.gestureOpenMessage) }
        guard peakDb.isFinite, (-96...0).contains(peakDb) else {
            return .refused("The peak must be a level from −96 to 0 dB.")
        }
        do {
            m.apply(try m.session.normalizeRegions(events: events, peakDb: Float(peakDb)))
            m.editor.refreshSelection()
            return .done
        } catch VizError.Busy(let message) {
            m.editor.status = message
            return .busy(message)
        } catch {
            m.editor.report(error)
            return .refused(m.editor.status ?? "Normalize was refused.")
        }
    }

    /// The level a text names ("-0.3", "−1", "0"): nil unless it is a
    /// finite number from −96 to 0.
    public static func parsePeak(_ text: String) -> Double? {
        let t = text.trimmingCharacters(in: .whitespaces).replacingOccurrences(of: "−", with: "-")
        guard let v = Double(t), v.isFinite, (-96...0).contains(v) else { return nil }
        return v
    }
}

extension RegionCommands: MenuCommands {}
