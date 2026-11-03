import Foundation
import Testing
import VizDAWCore
@testable import VizDAWAppModel

/// The process-wide surfaces as the tests see them (plan 5f-1 Task 9): no
/// CoreMIDI client, no surfaces.json (pre-flight M25).
final class FakeSurfaceCore: SurfaceCore, @unchecked Sendable {
    var rows: [SurfaceRow] = []
    var addError: VizError?
    var removed: [UInt32] = []
    var stateRow = SurfaceStateRow(surfaces: [], feedback: [], learning: nil, problems: [])
    var maps: [CommandMapRow] = []
    var removedCommands: [String] = []
    var learnCommands: [String] = []
    var learnError: VizError?
    var cancels = 0
    var learnRow = LearnStateRow(waiting: nil, last: nil)
    func list() -> [SurfaceRow] { rows }
    func ports() -> SurfacePortsRow { SurfacePortsRow(sources: ["In A"], destinations: ["Out A"]) }
    func add(_ k: SurfaceKindRow, _ i: String, _ o: String, _ p: UInt8?) throws -> UInt32 {
        if let e = addError { throw e }
        let id = UInt32(rows.count)
        rows.append(SurfaceRow(id: id, kind: k, input: i, output: o, position: p ?? 0, connected: true, online: false, ignored: 0))
        return id
    }
    func remove(_ id: UInt32) throws {
        removed.append(id)
        rows.removeAll { $0.id == id }
    }
    func state() -> SurfaceStateRow { stateRow }
    func commandMaps() -> [CommandMapRow] { maps }
    func removeCommandMap(_ command: String) throws { removedCommands.append(command) }
    func learnCommand(_ id: String) throws {
        if let e = learnError { throw e }
        learnCommands.append(id)
    }
    func cancelLearn() { cancels += 1 }
    func learnState() -> LearnStateRow { learnRow }
}

/// `surface.*` from the channel (plan 5f-1 Task 9).
@MainActor
@Suite struct SurfaceCommandsTests {
    func arg(_ name: String, _ v: String) -> ControlArg { ControlArg(name: name, value: .text(value: v)) }
    func int(_ name: String, _ v: Int64) -> ControlArg { ControlArg(name: name, value: .int(value: v)) }

    @Test func surfaceCommandsCheckTheirArguments() throws {
        let f = demoFake()
        let m = try DocumentModel(session: f)
        let c = FakeSurfaceCore()
        for (id, args) in [
            ("surface.inject", [int("surface", 1), arg("bytes", "9g")]),
            ("surface.inject", [int("surface", 1), arg("bytes", "")]),
            ("surface.learn", []),
            ("surface.learn", [arg("target", "fader:1"), arg("command", "edit.undo")]),
            ("surface.learn", [arg("target", "fader")]),
            ("surface.unlearn", []),
            ("surface.unlearn", [arg("target", "fader:1"), arg("command", "edit.undo")]),
        ] {
            guard case .failed(let e) = SurfaceCommands.run(id, args: args, doc: (UUID(), m), surfaces: c) else {
                Issue.record("\(id) \(args) ran"); continue
            }
            #expect(e.code == .invalidParams, "\(id)")
        }
        #expect(f.surfaceInjections.isEmpty && f.learnTargets.isEmpty && f.unlearnTargets.isEmpty)
        #expect(c.learnCommands.isEmpty && c.removedCommands.isEmpty)
    }

    @Test func injectAndLearnReachTheSession() throws {
        let f = demoFake()
        let m = try DocumentModel(session: f)
        let c = FakeSurfaceCore()
        _ = SurfaceCommands.run(
            "surface.inject", args: [int("surface", 2), arg("bytes", "90 68 7f e0 00 40")], doc: (UUID(), m), surfaces: c)
        #expect(f.surfaceInjections.map(\.0) == [2])
        #expect(f.surfaceInjections.first?.1 == Data([0x90, 0x68, 0x7F, 0xE0, 0x00, 0x40]))
        _ = SurfaceCommands.run("surface.inject", args: [int("surface", 2), arg("bytes", "90 68")], doc: (UUID(), m), surfaces: c)
        #expect(f.surfaceInjections.count == 2, "a cut message is the core's to count")
        _ = SurfaceCommands.run("surface.learn", args: [arg("target", "pan:3")], doc: (UUID(), m), surfaces: c)
        #expect(f.learnTargets == [.stripPan(strip: 3)])
        _ = SurfaceCommands.run("surface.learn", args: [arg("command", "edit.undo")], doc: (UUID(), m), surfaces: c)
        #expect(c.learnCommands == ["edit.undo"])
        _ = SurfaceCommands.run("surface.unlearn", args: [arg("target", "pan:3")], doc: (UUID(), m), surfaces: c)
        #expect(f.unlearnTargets == [.stripPan(strip: 3)])
        _ = SurfaceCommands.run("surface.unlearn", args: [arg("command", "edit.undo")], doc: (UUID(), m), surfaces: c)
        #expect(c.removedCommands == ["edit.undo"])
        c.learnError = .Model(message: "“automation.gesture” cannot be learned")
        guard case .failed(let e) = SurfaceCommands.run(
            "surface.learn", args: [arg("command", "automation.gesture")], doc: (UUID(), m), surfaces: c)
        else { Issue.record("a core refusal ran"); return }
        #expect(e.code == .failed, "a core refusal is the call's failure")
    }

    /// Pre-flight M25 and M29: the process-wide commands go through the
    /// host's `SurfaceCore`, with or without a document.
    @Test func addAndRemoveGoThroughTheHostWithoutADocument() throws {
        let c = FakeSurfaceCore()
        let o = SurfaceCommands.run(
            "surface.add", args: [arg("type", "mcu_xt"), arg("input", "In A"), arg("output", "Out A"), int("position", 2)],
            doc: nil, surfaces: c)
        guard case .done(let r) = o else { Issue.record("\(o)"); return }
        #expect(c.rows.map(\.kind) == [.mcuXt] && c.rows.first?.position == 2)
        #expect("\(r)".contains("surface 0"))
        _ = SurfaceCommands.run("surface.remove", args: [int("surface", 0)], doc: nil, surfaces: c)
        #expect(c.removed == [0])
        guard case .failed(let e) = SurfaceCommands.run(
            "surface.inject", args: [int("surface", 0), arg("bytes", "90 68 7f")], doc: nil, surfaces: c)
        else { Issue.record("inject ran without a document"); return }
        #expect(e.code == .noDocument)
        #expect(SurfaceCommands.parseKind("hui") == .hui && SurfaceCommands.parseKind("mackie") == nil)
    }
}
