import Foundation
import Testing
@testable import VizDAWAppModel
import VizDAWCore

/// The transport keys, modes, locators and key-binding files (plan 4f
/// Task 7), through `ControlCommands.run` as keys and the channel run
/// them.
@MainActor
@Suite struct TransportCommandsTests {
    let host = FakeControlHost()

    func demo() throws -> (DocumentStore, UUID, DocumentModel, FakeSession) {
        let f = demoFake()
        let s = DocumentStore(makeEmpty: { FakeSession([]) }, makeDemo: { f }, open: { _ in demoFake() })
        let id = try #require(s.newDemo())
        return (s, id, try #require(s.model(id)), f)
    }

    func run(_ id: String, _ args: [ControlArg], _ s: DocumentStore, _ doc: UUID?) -> ControlOutcome {
        ControlCommands.run(id, args: args, doc: doc.flatMap { d in s.model(d).map { (d, $0) } }, store: s, host: host)
    }

    func run(_ id: String, _ s: DocumentStore, _ doc: UUID?) -> ControlOutcome { run(id, [], s, doc) }

    func failure(_ o: ControlOutcome) -> ControlFailure? {
        if case .failed(let f) = o { return f }
        return nil
    }

    let on = [ControlArg(name: "on", value: .flag(value: true))]

    @Test func stopGoesBackToWherePlayingStarted() throws {
        let (s, id, m, f) = try demo()
        #expect(failure(run("transport.stop", s, id))?.message == "Nothing is playing.")
        f.state.tick = 30_000
        m.poll()
        _ = run("transport.play_stop", s, id)
        #expect(m.playStart == 30_000 && f.state.playing)
        f.state.tick = 90_000
        #expect(failure(run("transport.stop", s, id)) == nil)
        #expect(!f.state.playing && f.state.tick == 30_000)
    }

    @Test func playSelectionPlaysItsSpanOnceOrOnwards() throws {
        let (s, id, m, f) = try demo()
        #expect(failure(run("transport.play_selection", s, id))?.code == .unavailable, "nothing selected")
        f.span = TickRange(start: 1_000, end: 5_000)
        _ = run("transport.play_selection", s, id)
        #expect(f.playsFrom.last.map { [$0.seq, UInt64($0.from), UInt64($0.to ?? -1)] } == [3, 1_000, 5_000])
        #expect(m.playStart == 1_000)
        _ = run("transport.play_from_selection", s, id)
        #expect(f.playsFrom.last?.to == nil && f.playsFrom.last?.from == 1_000)
        f.playError = .Model(message: "the end must be after the start")
        #expect(failure(run("transport.play_selection", s, id)) == .of(.failed, "the end must be after the start"))
    }

    @Test func playFromEditInNeedsARange() throws {
        let (s, id, m, f) = try demo()
        #expect(failure(run("transport.play_from_edit_in", s, id))?.code == .unavailable)
        #expect(!TransportCommands.enabled("transport.play_from_edit_in", m))
        m.editor.setSelection(SelectionRow(timeRange: TickRange(start: 7_680, end: 61_440), lanes: [], events: []))
        #expect(TransportCommands.enabled("transport.play_from_edit_in", m))
        _ = run("transport.play_from_edit_in", s, id)
        #expect(f.playsFrom.last?.from == 7_680 && f.playsFrom.last?.to == nil)
    }

    @Test func markerKeysLocateToTheNearestMarker() throws {
        let (s, id, _, f) = try demo()
        f.markerTicks = [61_440, 122_880]
        _ = run("transport.next_marker", s, id)
        #expect(f.state.tick == 61_440)
        _ = run("transport.next_marker", s, id)
        #expect(f.state.tick == 122_880)
        #expect(failure(run("transport.next_marker", s, id))?.message == "No marker after the position.")
        _ = run("transport.previous_marker", s, id)
        #expect(f.state.tick == 61_440)
        #expect(failure(run("transport.previous_marker", s, id))?.code == .unavailable)
    }

    @Test func locatorsAreSetJumpedToAndCleared() throws {
        let (s, id, m, f) = try demo()
        #expect(failure(run("locator.go_3", s, id))?.message == "Locator 3 is not set.")
        f.state.tick = 12_345
        _ = run("locator.set_3", s, id)
        #expect(m.editor.layout.transport.locator(3) == 12_345)
        f.state.tick = 0
        _ = run("locator.go_3", s, id)
        #expect(f.state.tick == 12_345)
        _ = run("locator.clear_3", s, id)
        #expect(m.editor.layout.transport.locator(3) == nil)
        #expect(TransportCommands.locator("locator.clear_9")! == ("clear", 9))
        #expect(TransportCommands.locator("transport.stop") == nil)
    }

    /// Item 6: a locator the marks refuse is a refusal, locators need no
    /// sequence in the menu either, and Stop reads the engine first.
    @Test func locatorRefusalsMenusAndStopMatchTheirRun() throws {
        let (s, id, m, f) = try demo()
        f.state.tick = 1 << 41
        #expect(failure(run("locator.set_2", s, id))?.code == .failed, "past the tick limit")
        #expect(m.editor.layout.transport.locator(2) == nil)
        let empty = try #require(s.newEmpty())
        let e = try #require(s.model(empty))
        #expect(e.selectedSeq == nil)
        for cmd in ["locator.set_1", "locator.clear_1", "locator.go_1"] {
            #expect(TransportCommands.enabled(cmd, e), "\(cmd) runs without a sequence")
        }
        #expect(failure(run("locator.set_1", s, empty)) == nil)
        f.state.tick = 0
        m.poll()
        f.state.playing = true
        #expect(failure(run("transport.stop", s, id)) == nil, "not the stale cache")
        #expect(!f.state.playing)
    }

    @Test func modesToggleOrTakeTheirFlag() throws {
        let (s, id, m, _) = try demo()
        for (cmd, value) in [
            ("transport.overdub", { m.editor.layout.transport.overdub }),
            ("transport.punch", { m.editor.layout.transport.punch }),
        ] {
            _ = run(cmd, on, s, id)
            #expect(value() && TransportCommands.isOn(cmd, m), "\(cmd) on")
            _ = run(cmd, s, id)
            #expect(!value(), "\(cmd) toggled off")
        }
        #expect(m.editor.layout.snap.enabled)
        _ = run("editors.snap", s, id)
        #expect(!m.editor.layout.snap.enabled && !TransportCommands.isOn("editors.snap", m))
    }

    @Test func recordingCommandsWaitForSubProjectFive() throws {
        let (s, id, m, _) = try demo()
        for cmd in ["transport.record", "transport.scrub", "record.keep_pass", "record.discard_pass"] {
            #expect(failure(run(cmd, s, id)) == .of(.unavailable, TransportCommands.notYet), "\(cmd)")
            #expect(!TransportCommands.enabled(cmd, m))
        }
    }

    @Test func recordArmMovesAndARefusalShows() throws {
        let (s, id, m, f) = try demo()
        _ = run("transport.arm_next", s, id)
        #expect(f.armMoves.map(\.forward) == [true])
        f.armError = .Model(message: "there is no lane after the armed one")
        #expect(failure(run("transport.arm_next", s, id)) == .of(.failed, "there is no lane after the armed one"))
        #expect(m.editor.status == "there is no lane after the armed one")
    }

    @Test func keyBindingFilesNeedAPathAndNeedNoDocument() throws {
        let (s, _, _, _) = try demo()
        #expect(failure(run("keys.save", s, nil))?.dialog == "Save")
        #expect(failure(run("keys.load", s, nil))?.dialog == "Open")
        let path = FileManager.default.temporaryDirectory.appendingPathComponent("k-\(UUID().uuidString).vizkeys").path
        defer { try? FileManager.default.removeItem(atPath: path) }
        try host.keys.bind("transport.stop", keys: ["X"])
        let p = [ControlArg(name: "path", value: .text(value: path))]
        guard case .done = run("keys.save", p, s, nil) else {
            Issue.record("not saved")
            return
        }
        _ = run("keys.reset", s, nil)
        #expect(host.keys.row("transport.stop")?.keys == ["S"])
        _ = run("keys.load", p, s, nil)
        #expect(host.keys.row("transport.stop")?.keys == ["X"])
        let bad = [ControlArg(name: "path", value: .text(value: "/nonexistent/k.vizkeys"))]
        #expect(failure(run("keys.load", bad, s, nil))?.message.hasPrefix("Load Key Bindings Failed: ") == true)
    }

    @Test func stopReturnsToWhereTheEngineActuallyStarted() throws {
        let (s, id, m, f) = try demo()
        f.state.tick = 61_440  // stopped at the sequence's end: Play restarts at bar 1
        m.poll()
        _ = run("transport.play_stop", s, id)
        #expect(m.playStart == 0)
        _ = run("transport.stop", s, id)
        #expect(f.state.tick == 0)
        f.state.seq = 1  // a stale cursor in another sequence is ignored too
        f.state.tick = 30_000
        m.poll()
        _ = run("transport.play_stop", s, id)
        #expect(m.playStart == 0)
    }

    @Test func bareKeyCommandsWaitForTextFields() throws {
        let (s, id, m, f) = try demo()
        m.beginRename(3)
        for cmd in ["transport.overdub", "editors.snap", "locator.set_3", "locator.go_1", "transport.next_marker",
                    "transport.play_selection", "transport.record"] {
            #expect(failure(run(cmd, s, id)) == .of(.unavailable, "A text field has the keyboard."), "\(cmd)")
            #expect(!TransportCommands.enabled(cmd, m), "\(cmd)")
        }
        #expect(!m.editor.layout.transport.overdub && m.editor.layout.snap.enabled)
        #expect(m.editor.layout.transport.locator(3) == nil && f.playsFrom.isEmpty)
        m.cancelRename()
        #expect(failure(run("locator.set_3", s, id)) == nil)
    }

    @Test func busyIsBusyAndAnOpenDragBlocksArming() throws {
        let (s, id, m, f) = try demo()
        f.span = TickRange(start: 1_000, end: 5_000)
        f.playError = .Busy(message: "The core is busy.")
        #expect(failure(run("transport.play_selection", s, id)) == .of(.busy, "The core is busy."))
        f.playError = nil
        f.armError = .Busy(message: "The core is busy.")
        #expect(failure(run("transport.arm_next", s, id)) == .of(.busy, "The core is busy."))
        f.armError = nil
        #expect(m.editor.beginGesture("Move", lanes: [LaneKey(3, 30)], origin: .other))
        let moves = f.armMoves.count
        #expect(failure(run("transport.arm_next", s, id)) == .of(.unavailable, EditorModel.gestureOpenMessage))
        #expect(f.armMoves.count == moves)
    }
}
