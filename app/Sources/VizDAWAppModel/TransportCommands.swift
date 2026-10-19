import Foundation
import VizDAWCore

/// What a command came to: the Drum menu's three outcomes, shared by
/// every command that runs from keys, menus and the channel alike.
public typealias KeyOutcome = DrumOutcome

/// A group of commands with menu items (plan 4f): whether one is enabled,
/// its check mark, and running it (`on` sets a toggle; nil flips it).
@MainActor
public protocol MenuCommands {
    static func enabled(_ id: String, _ m: DocumentModel) -> Bool
    static func isOn(_ id: String, _ m: DocumentModel) -> Bool
    static func run(_ id: String, on: Bool?, _ m: DocumentModel) -> KeyOutcome
}

extension TransportCommands: MenuCommands {}
extension DrumCommands: MenuCommands {}

/// The transport keys, modes and locators (editors spec §4f, plan 4f
/// Rulings 7–10): one implementation for the menu items, the keys and
/// the control channel's `command.run`. None opens a dialog.
@MainActor
public enum TransportCommands {
    /// Recording, scrubbing and loop passes are sub-project 5's; their
    /// commands exist so keys can be bound now (editors spec §1).
    public static let notYet = "Recording and scrubbing arrive with sub-project 5."
    static let noSequence = "Select a sequence first."

    public static let ids: Set<String> = Set(
        [
            "transport.stop", "transport.play_selection", "transport.play_from_selection",
            "transport.play_from_edit_in", "transport.record", "transport.scrub",
            "transport.previous_marker", "transport.next_marker", "transport.overdub", "transport.punch",
            "transport.arm_previous", "transport.arm_next", "record.keep_pass", "record.discard_pass",
            "editors.snap",
        ]
            + (0..<TransportMarks.locatorCount).flatMap { n in
                ["go", "set", "clear"].map { "locator.\($0)_\(n)" }
            })

    /// A locator command's operation and number.
    static func locator(_ id: String) -> (op: String, n: Int)? {
        guard id.hasPrefix("locator."), let us = id.lastIndex(of: "_"),
            let n = Int(id[id.index(after: us)...])
        else { return nil }
        let op = id[id.index(id.startIndex, offsetBy: 8)..<us]
        return (String(op), n)
    }

    /// Whether `id` can run on `m` now (its menu item is enabled).
    public static func enabled(_ id: String, _ m: DocumentModel) -> Bool {
        if typesText(id) && !m.transportKeysEnabled { return false }
        return switch id {
        case "transport.record", "transport.scrub", "record.keep_pass", "record.discard_pass": false
        case "transport.stop": m.isPlaying
        case "transport.overdub", "transport.punch", "editors.snap": true
        case "transport.play_from_edit_in": m.selectedSeq != nil && m.editor.selection.timeRange != nil
        // Locators need no sequence, as in `run`.
        case _ where locator(id) != nil: true
        default: m.selectedSeq != nil
        }
    }

    static let textFieldHasKeyboard = "A text field has the keyboard."

    /// Commands with a bare (or ⌥/⇧-only) default key wait while a text
    /// field has the keyboard; the pass commands (no key) and the arm
    /// arrows do not.
    static func typesText(_ id: String) -> Bool {
        !["record.keep_pass", "record.discard_pass", "transport.arm_previous", "transport.arm_next"].contains(id)
    }

    /// A toggle's state (its menu item's check mark).
    public static func isOn(_ id: String, _ m: DocumentModel) -> Bool {
        switch id {
        case "transport.overdub": m.editor.layout.transport.overdub
        case "transport.punch": m.editor.layout.transport.punch
        case "editors.snap": m.editor.layout.snap.enabled
        default: false
        }
    }

    /// Runs `id` on `m`; `on` sets a toggle (nil flips it).
    @discardableResult
    public static func run(_ id: String, on: Bool? = nil, _ m: DocumentModel) -> KeyOutcome {
        if typesText(id) && !m.transportKeysEnabled { return .unavailable(textFieldHasKeyboard) }
        switch id {
        case "transport.record", "transport.scrub", "record.keep_pass", "record.discard_pass":
            return .unavailable(notYet)
        case "transport.overdub":
            m.editor.updateLayout { $0.transport.overdub = on ?? !$0.transport.overdub }
            return .done
        case "transport.punch":
            m.editor.updateLayout { $0.transport.punch = on ?? !$0.transport.punch }
            return .done
        case "editors.snap":
            m.editor.updateLayout { $0.snap.enabled = on ?? !$0.snap.enabled }
            return .done
        case "transport.stop":
            m.poll()
            guard m.isPlaying else { return .unavailable("Nothing is playing.") }
            return attempt { try m.stopAndReturn() }
        default: break
        }
        if let (op, n) = locator(id) { return runLocator(op, n, m) }
        guard let seq = m.selectedSeq else { return .unavailable(noSequence) }
        switch id {
        case "transport.play_selection", "transport.play_from_selection":
            let span: TickRange?
            do {
                span = try m.session.selectionSpan(seq: seq)
            } catch {
                return refused(error)
            }
            guard let span else { return .unavailable("Select something in the shown sequence first.") }
            let only = id == "transport.play_selection"
            return attempt { try m.startPlaying(from: span.start, to: only ? span.end : nil) }
        case "transport.play_from_edit_in":
            guard let r = m.editor.selection.timeRange else {
                return .unavailable("Set an edit-in point (select a time range) first.")
            }
            return attempt { try m.startPlaying(from: r.start) }
        case "transport.previous_marker", "transport.next_marker":
            let forward = id == "transport.next_marker"
            m.poll()
            let target: Int64?
            do {
                target = try m.session.markerStep(seq: seq, tick: m.transport.tick, forward: forward)
            } catch {
                return refused(error)
            }
            guard let t = target else {
                return .unavailable(forward ? "No marker after the position." : "No marker before the position.")
            }
            return attempt { try m.moveTo(t) }
        case "transport.arm_previous", "transport.arm_next":
            let forward = id == "transport.arm_next"
            guard m.editor.gesture == nil else { return .unavailable(EditorModel.gestureOpenMessage) }
            var busy: String?
            let moved = m.editor.change {
                do {
                    return try m.session.moveRecordArm(seq: seq, forward: forward)
                } catch VizError.Busy(let message) {
                    busy = message
                    throw VizError.Busy(message: message)
                }
            }
            if let busy { return .busy(busy) }
            return moved ? .done : .refused(m.editor.status ?? "Record arm did not move.")
        default:
            return .unavailable("VizDAW has no handler for \(id).")
        }
    }

    private static func runLocator(_ op: String, _ n: Int, _ m: DocumentModel) -> KeyOutcome {
        switch op {
        case "go":
            guard let t = m.editor.layout.transport.locator(n) else {
                return .unavailable("Locator \(n) is not set.")
            }
            return attempt { try m.moveTo(t) }
        case "set":
            m.poll()
            var marks = m.editor.layout.transport
            guard marks.setLocator(n, m.transport.tick) else {
                return .refused("Locator \(n) cannot be set at this position.")
            }
            m.editor.updateLayout { $0.transport = marks }
            return .done
        default:
            var marks = m.editor.layout.transport
            guard marks.setLocator(n, nil) else { return .refused("There is no locator \(n).") }
            m.editor.updateLayout { $0.transport = marks }
            return .done
        }
    }

    private static func attempt(_ body: () throws -> Void) -> KeyOutcome {
        do {
            try body()
            return .done
        } catch {
            return refused(error)
        }
    }

    private static func refused(_ error: any Error) -> KeyOutcome {
        if case VizError.Busy(let message) = error { return .busy(message) }
        return .refused(AlertItem(error, title: "").message)
    }
}
