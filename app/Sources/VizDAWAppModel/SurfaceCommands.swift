import Foundation
import VizDAWCore

/// The core's process-wide surfaces, command maps and learn state (plan 5f-1
/// Rulings 9–11, 16). The app passes `LiveSurfaceCore` (the free functions);
/// tests pass a fake, so no Swift test creates a CoreMIDI client or writes
/// surfaces.json (pre-flight M25).
public protocol SurfaceCore: AnyObject {
    func list() -> [SurfaceRow]
    func ports() -> SurfacePortsRow
    func add(_ k: SurfaceKindRow, _ i: String, _ o: String, _ p: UInt8?) throws -> UInt32
    func remove(_ id: UInt32) throws
    /// `state.get surfaces` without a document.
    func state() -> SurfaceStateRow
    func commandMaps() -> [CommandMapRow]
    func removeCommandMap(_ command: String) throws
    func learnCommand(_ id: String) throws
    func cancelLearn()
    func learnState() -> LearnStateRow
}

/// Lists nothing and refuses every change ("Surfaces are not available here.").
public final class NoSurfaceCore: SurfaceCore {
    public static let refusal = "Surfaces are not available here."

    public init() {}

    public func list() -> [SurfaceRow] { [] }
    public func ports() -> SurfacePortsRow { SurfacePortsRow(sources: [], destinations: []) }
    public func add(_ k: SurfaceKindRow, _ i: String, _ o: String, _ p: UInt8?) throws -> UInt32 {
        throw VizError.Model(message: Self.refusal)
    }
    public func remove(_ id: UInt32) throws { throw VizError.Model(message: Self.refusal) }
    public func state() -> SurfaceStateRow { SurfaceStateRow(surfaces: [], feedback: [], learning: nil, problems: []) }
    public func commandMaps() -> [CommandMapRow] { [] }
    public func removeCommandMap(_ command: String) throws { throw VizError.Model(message: Self.refusal) }
    public func learnCommand(_ id: String) throws { throw VizError.Model(message: Self.refusal) }
    public func cancelLearn() {}
    public func learnState() -> LearnStateRow { LearnStateRow(waiting: nil, last: nil) }
}

/// `surface.*` from the control channel (plan 5f-1 Task 9, Ruling 16):
/// `surface.add` and `surface.remove` change the app-wide surfaces (no
/// document needed); `surface.inject` feeds bytes to a surface as if from
/// its input; `surface.learn` and `surface.unlearn` start a MIDI learn or
/// remove learned maps, for exactly one of `target` (an automatable
/// control of the project, spelt as `automation.gesture` spells it) and
/// `command` (a key-bindable registry ID).
///
/// Errors: every argument is checked here before the core is called, and
/// is `invalid_params` naming the parameter. `bytes` is refused only when
/// it is not hex, is empty or holds more than 1024 bytes: any hex goes to
/// the core, which ignores and counts malformed MIDI (spec §12.2,
/// pre-flight I8). The core's refusals are the call's `failed` (its `Busy`
/// is `busy`). Nothing here opens a dialog.
@MainActor
public enum SurfaceCommands {
    public static let add = "surface.add"
    public static let remove = "surface.remove"
    public static let inject = "surface.inject"
    public static let learn = "surface.learn"
    public static let unlearn = "surface.unlearn"
    public static let ids: Set<String> = [add, remove, inject, learn, unlearn]

    /// `s` as a unit type, spelt as the registry's Choice does; nil for
    /// anything else.
    nonisolated public static func parseKind(_ s: String) -> SurfaceKindRow? {
        switch s {
        case "mcu": .mcu
        case "mcu_xt": .mcuXt
        case "hui": .hui
        default: nil
        }
    }

    static func invalid(_ message: String) -> ControlOutcome { .failed(.of(.invalidParams, message)) }

    /// The `surface` argument as a surface ID.
    static func surfaceID(_ args: [ControlArg]) -> UInt32? { args.int("surface").flatMap { UInt32(exactly: $0) } }

    /// `doc` nil: `surface.add` and `surface.remove` still run; the others are `no_document`.
    public static func run(
        _ id: String, args: [ControlArg], doc: (UUID, DocumentModel)?, surfaces: any SurfaceCore
    ) -> ControlOutcome {
        switch id {
        case add: return runAdd(args, surfaces)
        case remove:
            guard let s = surfaceID(args) else { return invalid("surface.remove: `surface` is a surface ID") }
            do {
                try surfaces.remove(s)
            } catch {
                return .failed(.thrown(error, "Remove Surface Failed"))
            }
            return .done(ControlCommands.ran(remove, nil))
        case inject, learn, unlearn: break
        default: return .failed(.of(.unknownId, "VizDAW has no handler for \(id)."))
        }
        // Check every argument before looking for a document.
        let checked: Checked
        switch check(id, args) {
        case .ok(let c): checked = c
        case .invalid(let message): return invalid(message)
        }
        guard let (docID, m) = doc else { return .failed(.of(.noDocument, "No document is open.")) }
        // A result's `status` is this command's own (plan 4f Ruling 2).
        m.editor.status = nil
        do {
            switch checked {
            case .inject(let s, let bytes):
                try m.session.injectSurface(surface: s, bytes: bytes)
            case .learnTarget(let t):
                try m.session.learnTarget(target: t)
            case .learnCommand(let c):
                // The learn completes in this window's session.
                try m.session.focusSurfaces()
                try surfaces.learnCommand(c)
            case .unlearnTarget(let t):
                guard m.editor.gesture == nil else { return ControlCommands.unavailable(EditorModel.gestureOpenMessage) }
                m.apply(try m.session.unlearnTarget(target: t))
            case .unlearnCommand(let c):
                try surfaces.removeCommandMap(c)
            }
        } catch {
            let title =
                switch id {
                case inject: "Surface Injection Failed"
                case learn: "MIDI Learn Failed"
                default: "Remove MIDI Learn Failed"
                }
            return .failed(.thrown(error, title))
        }
        return .done(ControlCommands.ran(id, docID, m))
    }

    /// A per-document call whose arguments were checked.
    enum Checked {
        case inject(UInt32, Data)
        case learnTarget(AutomationTargetRow)
        case learnCommand(String)
        case unlearnTarget(AutomationTargetRow)
        case unlearnCommand(String)
    }

    /// Checked arguments, or why they are `invalid_params`.
    enum Check {
        case ok(Checked)
        case invalid(String)
    }

    static func check(_ id: String, _ args: [ControlArg]) -> Check {
        func bad(_ message: String) -> Check { .invalid(message) }
        if id == inject {
            guard let s = surfaceID(args) else { return bad("surface.inject: `surface` is a surface ID") }
            guard let bytes = ControlLanes.hexBytes(args.text("bytes") ?? "") else {
                return bad("surface.inject: `bytes` are 1 to 1024 hex bytes such as \"90 68 7f\"")
            }
            return .ok(.inject(s, bytes))
        }
        let target = args.text("target")
        let command = args.text("command")
        let given = { (name: String) in args.contains { $0.name == name } }
        guard given("target") != given("command") else {
            return bad("\(id): give exactly one of `target` and `command`")
        }
        if given("target") {
            guard let text = target, let t = parseAutomationTarget(text: text) else {
                return bad(
                    "\(id): `target` is fader:<strip>, pan:<strip>, mute:<strip>, solo:<strip>, "
                        + "send:<strip>:<n> (n from 1) or param:<slot>:<param>")
            }
            return .ok(id == learn ? .learnTarget(t) : .unlearnTarget(t))
        }
        guard let c = command, !c.isEmpty else { return bad("\(id): `command` is a registry command ID") }
        return .ok(id == learn ? .learnCommand(c) : .unlearnCommand(c))
    }

    /// `surface.add`: answers the new surface's ID in `status` ("surface 3").
    static func runAdd(_ args: [ControlArg], _ surfaces: any SurfaceCore) -> ControlOutcome {
        guard let kind = args.text("type").flatMap(parseKind) else {
            return invalid("surface.add: `type` is mcu, mcu_xt or hui")
        }
        // The ports go as given: the core matches CoreMIDI names and refuses
        // a blank one (`failed`, as the e2e expects).
        guard let input = args.text("input"), let output = args.text("output") else {
            return invalid("surface.add: `input` and `output` name the MIDI ports")
        }
        var position: UInt8?
        if args.contains(where: { $0.name == "position" }) {
            guard let p = args.int("position"), (0...3).contains(p) else {
                return invalid("surface.add: `position` is 0 to 3")
            }
            position = UInt8(p)
        }
        let id: UInt32
        do {
            id = try surfaces.add(kind, input, output, position)
        } catch {
            return .failed(.thrown(error, "Add Surface Failed"))
        }
        return .done(.ran(command: add, doc: nil, window: nil, undoLabel: nil, status: "surface \(id)"))
    }
}
