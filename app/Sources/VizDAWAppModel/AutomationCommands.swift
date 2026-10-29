import Foundation
import VizDAWCore

/// Automation from the control channel (plan 5e-1 Task 10; sub-project 5
/// spec §7, Rulings 17 and 20): `automation.set_mode` sets strips' modes
/// as one undo step, `automation.gesture` touches, moves and releases a
/// target exactly as the mixer does, `automation.suspend` is the app-wide
/// switch.
///
/// Errors: every argument is checked here before the core is called (a
/// missing or wrongly typed one, a mode, target or value not spelt as the
/// registry says) and is `invalid_params` naming the parameter. The core's
/// refusals (an unknown strip, send, slot or parameter, a strip that went)
/// are the call's `failed`; its `Busy` (a static touch while an edit is
/// open) is `busy`. Nothing here opens a dialog.
///
/// Gestures outlive the call that began them (each `vizctl` call is its
/// own connection): a touch lasts until a release of the same target from
/// any client, or until its window closes (Ruling 20). A release is never
/// refused for anything its touch passed; a release of a target that is
/// not touched, or that no longer exists, does nothing and answers `ran`.
/// `state.get automation` lists the touched targets, so a client that lost
/// its connection mid-gesture can find and release them.
@MainActor
public enum AutomationCommands {
    public static let setMode = "automation.set_mode"
    public static let gesture = "automation.gesture"
    public static let suspend = "automation.suspend"
    public static let ids: Set<String> = [setMode, gesture, suspend]

    /// The longest `strips` text read (the registry's string limit).
    nonisolated static let maxStripsText = 4096
    /// The longest `value` text read: more digits than a double holds.
    nonisolated static let maxValueText = 32

    /// `text` as a mode, spelt as the registry's Choice does; nil for
    /// anything else.
    public nonisolated static func mode(_ text: String?) -> AutomationModeRow? {
        switch text {
        case "off": return .off
        case "read": return .read
        case "touch": return .touch
        case "latch": return .latch
        case "write": return .write
        case "trim": return .trim
        default: return nil
        }
    }

    /// `text` as strip IDs: `"all"` (every strip in `all`, in its order)
    /// or IDs separated by commas, ASCII digits only; a repeated ID is
    /// kept once at its first place (M21). Nil for anything else (an
    /// empty item, a sign, a space, an overflow, more than 4096 bytes).
    public nonisolated static func strips(_ text: String, all: [UInt64]) -> [UInt64]? {
        if text == "all" { return all }
        guard !text.isEmpty, text.utf8.count <= maxStripsText else { return nil }
        var out: [UInt64] = []
        var seen = Set<UInt64>()
        for item in text.split(separator: ",", omittingEmptySubsequences: false) {
            guard !item.isEmpty, item.utf8.allSatisfy({ (0x30...0x39).contains($0) }),
                let id = UInt64(item)
            else { return nil }
            if seen.insert(id).inserted { out.append(id) }
        }
        return out
    }

    /// `text` as a normalised value: a plain decimal, ASCII digits with at
    /// most one `.` and nothing else (no sign, exponent, hex or spaces),
    /// at most 32 characters, from 0 to 1. Nil for anything else.
    public nonisolated static func value(_ text: String?) -> Double? {
        guard let t = text, !t.isEmpty, t.utf8.count <= maxValueText else { return nil }
        var dots = 0
        var digits = 0
        for b in t.utf8 {
            if b == 0x2E {
                dots += 1
            } else if (0x30...0x39).contains(b) {
                digits += 1
            } else {
                return nil
            }
        }
        guard dots <= 1, digits > 0, let v = Double(t), v.isFinite, (0...1).contains(v) else { return nil }
        return v
    }

    static func invalid(_ message: String) -> ControlOutcome { .failed(.of(.invalidParams, message)) }

    /// Whether an argument `name` was given (of any type).
    static func given(_ args: [ControlArg], _ name: String) -> Bool { args.contains { $0.name == name } }

    /// `automation.suspend`: sets the app-wide switch (`on`; absent
    /// toggles).
    public static func runSuspend(_ args: [ControlArg]) -> ControlOutcome {
        let on = args.flag("on")
        if on == nil && given(args, "on") { return invalid("automation.suspend: `on` is true or false") }
        setAutomationSuspended(on: on ?? !automationSuspended())
        return .done(ControlCommands.ran(suspend, nil))
    }

    /// Runs `id` (`setMode` or `gesture`) on document `doc`.
    public static func run(_ id: String, args: [ControlArg], doc: UUID, _ m: DocumentModel) -> ControlOutcome {
        switch id {
        case setMode: return runSetMode(args, doc: doc, m)
        case gesture: return runGesture(args, doc: doc, m)
        default: return .failed(.of(.unknownId, "VizDAW has no handler for \(id)."))
        }
    }

    static func runSetMode(_ args: [ControlArg], doc: UUID, _ m: DocumentModel) -> ControlOutcome {
        guard let mode = mode(args.text("mode")) else {
            return invalid("automation.set_mode: `mode` is off, read, touch, latch, write or trim")
        }
        guard let text = args.text("strips") else {
            return invalid("automation.set_mode: `strips` is all or strip IDs such as \"1,2\"")
        }
        guard m.editor.gesture == nil else { return ControlCommands.unavailable(EditorModel.gestureOpenMessage) }
        do {
            let all = text == "all" ? try m.session.mixer().strips.map(\.id) : []
            guard let ids = strips(text, all: all) else {
                return invalid("automation.set_mode: `strips` is all or strip IDs such as \"1,2\"")
            }
            m.apply(try m.session.setAutomationMode(strips: ids, mode: mode))
            return .done(ControlCommands.ran(setMode, doc, m))
        } catch {
            return .failed(.thrown(error, "Automation Failed"))
        }
    }

    static func runGesture(_ args: [ControlArg], doc: UUID, _ m: DocumentModel) -> ControlOutcome {
        guard let text = args.text("target"), let target = parseAutomationTarget(text: text) else {
            return invalid(
                "automation.gesture: `target` is fader:<strip>, pan:<strip>, mute:<strip>, solo:<strip>, "
                    + "send:<strip>:<n> (n from 1) or param:<slot>:<param>")
        }
        let hasValue = given(args, "value")
        let v = value(args.text("value"))
        if hasValue && v == nil { return invalid("automation.gesture: `value` is a number from 0 to 1 such as \"0.5\"") }
        let phase: GesturePhaseRow
        switch args.text("phase") {
        case "touch": phase = .touch(value: v)
        case "value":
            guard let v else { return invalid("automation.gesture: `value` is needed for value (0 to 1)") }
            phase = .value(value: v)
        case "release":
            guard !hasValue else { return invalid("automation.gesture: release takes no `value`") }
            phase = .release
        default: return invalid("automation.gesture: `phase` is touch, value or release")
        }
        do {
            try m.session.gesture(target: target, phase: phase)
        } catch {
            return .failed(.thrown(error, "Automation Failed"))
        }
        // A static gesture changes the document (its edit, Ruling 14).
        if let s = try? m.session.snapshot(), s.version != m.snapshot.version { m.apply(s) }
        return .done(ControlCommands.ran(gesture, doc, m))
    }
}
