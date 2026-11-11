import Foundation
import Testing
import VizDAWCore
@testable import VizDAWAppModel

/// `lane.set_mpe` and `destination.set_mpe` (plan 5g-2 Task 7): arguments
/// checked before the core is called; the core's refusals are failures.
@MainActor
@Suite(.serialized) struct MpeCommandsTests {
    func doc() throws -> (UUID, DocumentModel, FakeSession) {
        let f = demoFake()
        let s = DocumentStore(makeEmpty: { FakeSession([]) }, makeDemo: { f }, open: { _ in demoFake() })
        let id = try #require(s.newDemo())
        return (id, try #require(s.model(id)), f)
    }

    func text(_ n: String, _ v: String) -> ControlArg { ControlArg(name: n, value: .text(value: v)) }
    func int(_ n: String, _ v: Int64) -> ControlArg { ControlArg(name: n, value: .int(value: v)) }

    func code(_ o: ControlOutcome) -> ControlErrorCode? {
        if case .failed(let f) = o { return f.code }
        return nil
    }

    @Test func mpeCommandsCheckTheirArguments() throws {
        let (id, m, f) = try doc()
        let lane = { (a: [ControlArg]) in MpeCommands.run(MpeCommands.setLane, args: [int("seq", 3), int("lane", 31)] + a, doc: id, m) }
        let dest = { (a: [ControlArg]) in MpeCommands.run(MpeCommands.setDestination, args: [int("dest", 4)] + a, doc: id, m) }
        #expect(code(lane([text("zone", "lower")])) == nil)
        #expect(f.edits.last == .lane(edit: .setMpe(seq: 3, lane: 31, mpe: MpeRow(zone: .lower, members: 15, bendRange: 48))))
        #expect(code(lane([text("zone", "upper"), int("members", 7), int("bend_range", 24)])) == nil)
        #expect(f.edits.last == .lane(edit: .setMpe(seq: 3, lane: 31, mpe: MpeRow(zone: .upper, members: 7, bendRange: 24))))
        #expect(code(lane([text("zone", "off")])) == nil)
        #expect(f.edits.last == .lane(edit: .setMpe(seq: 3, lane: 31, mpe: nil)))
        #expect(code(dest([text("zone", "upper"), int("members", 2)])) == nil)
        #expect(f.mpeCalls.last?.0 == 4 && f.mpeCalls.last?.1 == MpeRow(zone: .upper, members: 2, bendRange: 48))
        let edits = f.edits.count, calls = f.mpeCalls.count
        for bad in [
            [text("zone", "middle")], [], [text("zone", "lower"), int("members", 0)],
            [text("zone", "lower"), int("members", 16)], [text("zone", "lower"), int("bend_range", 97)],
            [text("zone", "off"), int("members", 3)], [text("zone", "off"), int("bend_range", 48)],
        ] as [[ControlArg]] {
            #expect(code(lane(bad)) == .invalidParams, "\(bad)")
            #expect(code(dest(bad)) == .invalidParams, "\(bad)")
        }
        #expect(code(MpeCommands.run(MpeCommands.setLane, args: [int("seq", 3), text("zone", "lower")], doc: id, m)) == .invalidParams)
        #expect(code(MpeCommands.run(MpeCommands.setDestination, args: [text("zone", "lower")], doc: id, m)) == .invalidParams)
        #expect(f.edits.count == edits && f.mpeCalls.count == calls, "nothing reached the core")
        f.editError = .Model(message: "unknown lane")
        #expect(code(lane([text("zone", "lower")])) == .failed)
        f.editError = nil
    }
}
