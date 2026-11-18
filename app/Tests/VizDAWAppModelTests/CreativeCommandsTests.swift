import Foundation
import Testing
import VizDAWCore
@testable import VizDAWAppModel

/// Plan 8e Ruling 1's creative command, `automation.write`: one
/// `runCreative` call with the arguments as given, its snapshot applied,
/// answering its title. The gesture is checked first; nothing opens a sheet.
@MainActor
@Suite struct CreativeCommandsTests {
    func demo() throws -> (DocumentModel, FakeSession) {
        let f = mixerFake()
        return (try DocumentModel(session: f), f)
    }

    func failure(_ o: ControlOutcome) -> ControlFailure? {
        if case .failed(let f) = o { return f }
        return nil
    }

    func args(_ a: [ControlArg]) -> [String] { a.map { "\($0.name)=\($0.value)" } }

    static let given: [ControlArg] = [
        ControlArg(name: "seq", value: .int(value: 1)),
        ControlArg(name: "target", value: .text(value: "fader:1")),
        ControlArg(name: "start", value: .int(value: 0)),
        ControlArg(name: "end", value: .int(value: 61_440)),
        ControlArg(name: "points", value: .text(value: "0:0 61440:-144")),
    ]

    @Test func creativeEditsRunThroughOneCall() throws {
        #expect(CreativeCommands.ids == [CreativeCommands.writeAutomation])
        #expect(ControlCommands.handled.contains(CreativeCommands.writeAutomation))
        let (m, f) = try demo()
        let doc = UUID()
        let o = CreativeCommands.run(CreativeCommands.writeAutomation, args: Self.given, docID: doc, m)
        guard case .done(.ran(let command, let d, _, _, let status)) = o else {
            Issue.record("not ran: \(o)")
            return
        }
        #expect(command == CreativeCommands.writeAutomation)
        #expect(d == doc.uuidString)
        #expect(status == "Write Automation")
        #expect(f.creativeRuns.map(\.id) == [CreativeCommands.writeAutomation])
        #expect(f.creativeRuns.map { args($0.args) } == [args(Self.given)])
        #expect(m.snapshot.version == f.snap.version, "the snapshot is applied")
        #expect(m.alert == nil)
    }

    /// The session's refusal is `failed` in its words; its Busy is `busy`.
    @Test func aRefusedCreativeEditIsFailedOrBusy() throws {
        let (m, f) = try demo()
        f.mixerError = .Model(message: "This song has no fader:9.")
        let refused = try #require(
            failure(CreativeCommands.run(CreativeCommands.writeAutomation, args: Self.given, docID: UUID(), m)))
        #expect(refused.code == .failed)
        #expect(refused.message == "This song has no fader:9.")
        f.mixerError = .Busy(message: "Finish the edit first.")
        let busy = try #require(
            failure(CreativeCommands.run(CreativeCommands.writeAutomation, args: Self.given, docID: UUID(), m)))
        #expect(busy.code == .busy)
        #expect(busy.message == "Finish the edit first.")
        #expect(m.alert == nil)
    }

    /// With an editor gesture open it is unavailable, and the session is
    /// not called (8d's guard order).
    @Test func aGestureStopsTheCreativeEdit() throws {
        let (m, f) = try demo()
        #expect(m.editor.beginGesture("drag", lanes: [], origin: .other))
        let o = try #require(
            failure(CreativeCommands.run(CreativeCommands.writeAutomation, args: Self.given, docID: UUID(), m)))
        #expect(o.code == .unavailable)
        #expect(o.message == EditorModel.gestureOpenMessage)
        #expect(f.creativeRuns.isEmpty)
    }

    /// Hostile arguments reach the core unchanged (it checks them), and an
    /// unknown id is refused here.
    @Test func hostileArgumentsGoToTheCoreAsGiven() throws {
        let (m, f) = try demo()
        let hostile: [ControlArg] = [
            ControlArg(name: "seq", value: .text(value: "one")),
            ControlArg(name: "target", value: .text(value: String(repeating: "x", count: 70_000))),
            ControlArg(name: "points", value: .text(value: "0:\u{0}")),
            ControlArg(name: "extra", value: .flag(value: true)),
        ]
        f.mixerError = .Model(message: "seq is a sequence ID.")
        let o = try #require(
            failure(CreativeCommands.run(CreativeCommands.writeAutomation, args: hostile, docID: UUID(), m)))
        #expect(o.code == .failed)
        #expect(f.creativeRuns.map { args($0.args) } == [args(hostile)])
        let unknown = try #require(failure(CreativeCommands.run("midi.add_notes", args: [], docID: UUID(), m)))
        #expect(unknown.code == .unknownId)
        #expect(f.creativeRuns.count == 1)
        #expect(m.alert == nil)
    }
}
