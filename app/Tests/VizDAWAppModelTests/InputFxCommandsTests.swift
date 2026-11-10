import Foundation
import Testing
import VizDAWCore
@testable import VizDAWAppModel

/// `lane.set_input_fx` (plan 5g-1 Task 7): arguments checked before the
/// core is called; the core's refusals are the call's failures; nothing
/// opens a dialog.
@MainActor
@Suite(.serialized) struct InputFxCommandsTests {
    func doc() throws -> (UUID, DocumentModel, FakeSession) {
        let f = demoFake()
        let s = DocumentStore(makeEmpty: { FakeSession([]) }, makeDemo: { f }, open: { _ in demoFake() })
        let id = try #require(s.newDemo())
        return (id, try #require(s.model(id)), f)
    }

    func arg(_ name: String, _ v: String) -> ControlArg { ControlArg(name: name, value: .text(value: v)) }
    func int(_ name: String, _ v: Int64) -> ControlArg { ControlArg(name: name, value: .int(value: v)) }

    func code(_ o: ControlOutcome) -> ControlErrorCode? {
        if case .failed(let f) = o { return f.code }
        return nil
    }

    @Test func inputFxCommandsCheckTheirArguments() throws {
        let (id, m, f) = try doc()
        let run = { (args: [ControlArg]) in InputFxCommands.run(InputFxCommands.setInputFx, args: args, doc: id, m) }
        let chain = #"[{"kind": {"transpose": {"semitones": 12}}}]"#
        #expect(code(run([int("seq", 3), int("lane", 31), arg("chain", chain)])) == nil)
        #expect(
            f.edits.last
                == .lane(edit: .setInputFx(seq: 3, lane: 31, chain: [InputFxRow(kind: .transpose(semitones: 12), bypass: false)])))
        #expect(code(run([int("seq", 3), int("lane", 31), arg("chain", "[]")])) == nil)
        #expect(f.edits.last == .lane(edit: .setInputFx(seq: 3, lane: 31, chain: [])))
        let before = f.edits.count
        for bad in ["", "{", "[1]", #"[{"kind": {"flanger": {}}}]"#, #"[{"kind": {"transpose": {"semitones": 300}}}]"#] {
            #expect(code(run([int("seq", 3), int("lane", 31), arg("chain", bad)])) == .invalidParams, "\(bad)")
        }
        #expect(code(run([int("lane", 31), arg("chain", "[]")])) == .invalidParams)
        #expect(code(run([int("seq", 3), arg("chain", "[]")])) == .invalidParams)
        #expect(code(run([int("seq", 3), int("lane", 31)])) == .invalidParams)
        #expect(code(run([int("seq", -3), int("lane", 31), arg("chain", "[]")])) == .invalidParams)
        #expect(f.edits.count == before, "nothing reached the core")
        f.editError = .Model(message: "out of range: arpeggiator.octaves")
        #expect(code(run([int("seq", 3), int("lane", 31), arg("chain", "[]")])) == .failed)
        f.editError = .Busy(message: "Finish the open edit first.")
        #expect(code(run([int("seq", 3), int("lane", 31), arg("chain", "[]")])) == .busy)
        f.editError = nil
    }

    @Test func aDragKeepsTheChannelOff() throws {
        let (id, m, f) = try doc()
        #expect(m.editor.beginGesture("Move", lanes: [], origin: .other))
        let before = f.edits.count
        let o = InputFxCommands.run(
            InputFxCommands.setInputFx, args: [int("seq", 3), int("lane", 31), arg("chain", "[]")], doc: id, m)
        #expect(code(o) == .unavailable)
        #expect(f.edits.count == before)
    }
}
