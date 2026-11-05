import Foundation
import Testing
@testable import VizDAWAppModel
import VizDAWCore

/// The transport bar's rewind and fast forward held to wind (plan 7d
/// Task B2, Ruling 3): a click steps one bar, ⇧-click goes to a marker,
/// a hold winds faster the longer it is held, and a held key is one hold.
@MainActor
@Suite struct TransportShuttleTests {
    func demo() throws -> (DocumentModel, FakeSession) {
        let f = demoFake()
        let m = try DocumentModel(session: f)
        m.select(1)
        return (m, f)
    }

    @Test func aClickStepsOneBar() throws {
        let (m, f) = try demo()
        m.shuttle.press(forward: true, shift: false, at: 0)
        #expect(m.shuttle.holding)
        m.shuttle.release(at: 0.2)
        #expect(f.stepBars == [true])
        #expect(f.winds.isEmpty)
        #expect(m.transport.tick == 61_440)
        #expect(!m.shuttle.holding)
        #expect(f.holdEnds == 0, "a click is one locate, with no hold to end")
    }

    @Test func aShiftClickGoesToTheMarker() throws {
        let (m, f) = try demo()
        f.markerTicks = [30_720]
        m.shuttle.press(forward: true, shift: true, at: 0)
        m.shuttle.tick(at: 0.5)
        m.shuttle.tick(at: 1.0)
        m.shuttle.release(at: 1.1)
        #expect(f.winds.isEmpty, "a ⇧-press never winds")
        #expect(f.stepBars.isEmpty)
        #expect(m.transport.tick == 30_720)
        m.shuttle.press(forward: true, shift: true, at: 2)
        m.shuttle.release(at: 2.1)
        #expect(m.editor.status == "No marker after the position.")
        #expect(m.transport.tick == 30_720)
    }

    @Test func aHoldWindsFasterTheLongerItIsHeld() throws {
        let (m, f) = try demo()
        m.shuttle.press(forward: true, shift: false, at: 0)
        for t in [0.1, 0.2, 0.3] { m.shuttle.tick(at: t) }
        #expect(f.winds.isEmpty, "nothing below 0.4 s")
        m.shuttle.tick(at: 0.5)
        #expect(f.winds == [0.4])
        m.shuttle.tick(at: 1.5)
        #expect(abs((f.winds.last ?? 0) - 4.4) < 1e-9, "0.9 × 4 + 0.1 × 8")
        m.shuttle.tick(at: 3.5)
        #expect(abs((f.winds.last ?? 0) - 26.4) < 1e-9, "0.9 × 8 + 1.0 × 16 + 0.1 × 32")
        #expect(f.winds.count == 3)
        #expect(f.stepBars.isEmpty)
        #expect(m.shuttle.holding)
    }

    @Test func aReleaseAfterWindingStepsNoBar() throws {
        let (m, f) = try demo()
        f.state.tick = 200_000
        m.poll()
        m.shuttle.press(forward: false, shift: false, at: 0)
        m.shuttle.tick(at: 0.5)
        m.shuttle.release(at: 0.55)
        #expect(f.winds == [-0.4, -0.2], "the last part, 0.05 × 4")
        #expect(f.stepBars.isEmpty)
        #expect(f.holdEnds == 1)
        #expect(!m.shuttle.holding)
    }

    @Test func cancelStopsWithoutAMove() throws {
        let (m, f) = try demo()
        m.shuttle.press(forward: true, shift: false, at: 0)
        m.shuttle.tick(at: 0.5)
        m.shuttle.cancel()
        m.shuttle.tick(at: 1.0)
        m.shuttle.release(at: 1.1)
        #expect(f.winds == [0.4])
        #expect(f.stepBars.isEmpty)
        #expect(f.holdEnds == 1, "the open pass lands where the wind got to")
        #expect(!m.shuttle.holding)
        // A release outside the button with no wind yet moves nothing.
        m.shuttle.press(forward: true, shift: false, at: 2)
        m.shuttle.release(at: 2.2, inside: false)
        #expect(f.winds == [0.4] && f.stepBars.isEmpty && f.locates.isEmpty && f.holdEnds == 1)
        #expect(!m.shuttle.holding)
        // Times are clamped: earlier than the press, or not a number.
        m.shuttle.press(forward: true, shift: false, at: 5)
        m.shuttle.tick(at: -1)
        m.shuttle.tick(at: .nan)
        m.shuttle.tick(at: .infinity)
        #expect(f.winds == [0.4])
        m.shuttle.cancel()
        #expect(f.holdEnds == 1, "no wind, no hold to land")
    }

    @Test func aHoldWaitsForATextFieldAsAClickDoes() throws {
        let (m, f) = try demo()
        m.editor.beginTextEditing("laneName")
        m.shuttle.press(forward: true, shift: false, at: 0)
        #expect(!m.shuttle.holding)
        m.shuttle.tick(at: 0.5)
        m.shuttle.tick(at: 1.0)
        m.shuttle.release(at: 1.1)
        #expect(f.winds.isEmpty)
        #expect(f.stepBars.isEmpty)
        #expect(f.windAttempts == 0)
        #expect(m.editor.status == "A text field has the keyboard.")
    }

    /// Final wave (B2 carry): a text field that takes the keyboard in the
    /// middle of a hold stops it where it got to. After a wind it lands
    /// once (the teardown path); before one it lands nothing; the later
    /// ticks and the release do nothing more.
    @Test func aTextFieldTakingTheKeyboardMidHoldStopsTheHold() throws {
        let (m, f) = try demo()
        m.shuttle.press(forward: true, shift: false, at: 0)
        m.shuttle.tick(at: 0.5)
        #expect(f.winds == [0.4] && m.shuttle.holding)
        m.editor.beginTextEditing("laneName")
        m.shuttle.tick(at: 0.6)
        #expect(!m.shuttle.holding)
        #expect(m.editor.status == "A text field has the keyboard.")
        #expect(f.holdEnds == 1, "wound, so the pass lands where the wind got to")
        m.shuttle.tick(at: 1.5)
        m.shuttle.release(at: 1.6)
        #expect(f.winds == [0.4] && f.holdEnds == 1 && f.stepBars.isEmpty, "nothing after the stop")
        m.editor.endTextEditing("laneName")
        // Before any wind: stopped, with nothing to land.
        m.editor.status = nil
        m.shuttle.press(forward: false, shift: false, at: 2)
        m.editor.beginTextEditing("laneName")
        m.shuttle.tick(at: 2.2)
        #expect(!m.shuttle.holding)
        #expect(m.editor.status == "A text field has the keyboard.")
        #expect(f.holdEnds == 1 && f.winds == [0.4])
    }

    @Test func aRefusedWindStopsTheHoldAndSaysWhy() throws {
        let (m, f) = try demo()
        f.jumpError = .Busy(message: "Stop recording first.")
        m.shuttle.press(forward: true, shift: false, at: 0)
        m.shuttle.tick(at: 0.5)
        #expect(!m.shuttle.holding)
        m.shuttle.tick(at: 0.6)
        #expect(f.windAttempts == 1)
        #expect(m.editor.status == "Stop recording first.")
        #expect(m.alert == nil)
        #expect(f.holdEnds == 0, "nothing wound, nothing to land")
        // A wind that throws after earlier winds still ends the hold once.
        f.jumpError = nil
        m.editor.status = nil
        m.shuttle.press(forward: true, shift: false, at: 1)
        m.shuttle.tick(at: 1.5)
        #expect(f.winds == [0.4])
        f.jumpError = .Busy(message: "Stop recording first.")
        m.shuttle.tick(at: 1.6)
        #expect(!m.shuttle.holding)
        #expect(f.holdEnds == 1)
        m.shuttle.release(at: 1.7)
        #expect(f.holdEnds == 1 && f.windAttempts == 3)
        #expect(m.editor.status == "Stop recording first." && m.alert == nil)
    }

    @Test func aHeldKeyIsOneHoldLandedOnKeyUp() throws {
        let (m, f) = try demo()
        for _ in 0..<3 { m.shuttle.keyStep(forward: true) }
        #expect(f.keySteps == [true, true, true])
        #expect(f.stepBars.isEmpty)
        #expect(f.holdEnds == 0)
        #expect(m.transport.tick == 3 * 61_440)
        m.shuttle.keyUp()
        #expect(f.holdEnds == 1)
        m.shuttle.keyUp()
        #expect(f.holdEnds == 1, "one landing per hold")
        m.editor.beginTextEditing("laneName")
        #expect(m.shuttle.keyStep(forward: false) == .unavailable("A text field has the keyboard."))
        #expect(f.keySteps == [true, true, true])
        #expect(m.editor.status == "A text field has the keyboard.")
        m.shuttle.keyUp()
        #expect(f.holdEnds == 1)
        #expect(KeyDispatch.held == ["transport.rewind", "transport.fast_forward"])
    }
}
