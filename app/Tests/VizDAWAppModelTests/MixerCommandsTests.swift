import Foundation
import Testing
import VizDAWCore
@testable import VizDAWAppModel

/// `mixer.new_bus` (plan 6g-2 Task 3, Ruling 7): with `strip`, a bus and a
/// send to it from the strip in one step; without, a bus alone. The name
/// defaults to the next free "Bus N". Nothing here opens a dialog.
@MainActor
@Suite struct MixerCommandsTests {
    func arg(_ name: String, _ v: String) -> ControlArg { ControlArg(name: name, value: .text(value: v)) }
    func arg(_ name: String, _ v: Int64) -> ControlArg { ControlArg(name: name, value: .int(value: v)) }

    /// Synth (1), Bus 1 (2) and Master (0).
    func demo() throws -> (DocumentModel, FakeSession) {
        let f = mixerFake()
        return (try DocumentModel(session: f), f)
    }

    func status(_ o: ControlOutcome) -> String? {
        if case .done(.ran(_, _, _, _, let status)) = o { return status }
        return nil
    }

    func failure(_ o: ControlOutcome) -> ControlFailure? {
        if case .failed(let f) = o { return f }
        return nil
    }

    @Test func newBusCommandRuns() throws {
        #expect(MixerCommands.ids == [MixerCommands.newBus])
        #expect(ControlCommands.handled.contains(MixerCommands.newBus))
        let (m, f) = try demo()
        let doc = UUID()
        let r = MixerCommands.run(MixerCommands.newBus, args: [arg("strip", 1)], docID: doc, m)
        #expect(status(r) == "Bus 2", "\(r)")
        #expect(f.calls == ["newBusFrom 1 Bus 2"])

        f.calls = []
        let alone = MixerCommands.run(MixerCommands.newBus, args: [], docID: doc, m)
        #expect(status(alone) == "Bus 2", "\(alone)")
        #expect(f.calls == ["addBus Bus 2"])

        f.calls = []
        let named = MixerCommands.run(
            MixerCommands.newBus, args: [arg("strip", 1), arg("name", "Drums")], docID: doc, m)
        #expect(status(named) == "Drums", "\(named)")
        #expect(f.calls == ["newBusFrom 1 Drums"])
        #expect(m.alert == nil)
    }

    @Test func newBusCommandRefusesBadInput() throws {
        let (m, f) = try demo()
        let doc = UUID()
        let cases: [([ControlArg], ControlErrorCode, String?)] = [
            ([arg("strip", 1), arg("name", "a\u{0}b")], .invalidParams, nil),
            ([arg("name", "  ")], .invalidParams, nil),
            ([arg("strip", 0)], .invalidParams, "Master has no sends."),
            ([arg("strip", 999)], .unknownId, nil),
            ([arg("strip", -1)], .invalidParams, nil),
        ]
        for (args, code, message) in cases {
            let r = MixerCommands.run(MixerCommands.newBus, args: args, docID: doc, m)
            let fail = try #require(failure(r), "\(args)")
            #expect(fail.code == code, "\(args): \(fail.message)")
            if let message { #expect(fail.message == message, "\(args)") }
        }
        #expect(f.calls.isEmpty)
        #expect(m.alert == nil)
    }
}
