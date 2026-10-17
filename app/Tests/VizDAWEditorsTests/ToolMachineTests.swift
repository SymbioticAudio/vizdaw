import CoreGraphics
import Testing
@testable import VizDAWEditors

private typealias M = ToolMachine<String>

@Suite struct ToolMachineTests {
    @Test func aClickWithoutMovementIsAClick() {
        var m = M()
        #expect(m.down("a", at: CGPoint(x: 10, y: 10), clicks: 2, modifiers: [.shift]).isEmpty)
        #expect(m.drag(to: CGPoint(x: 12, y: 11), modifiers: []).isEmpty, "inside the dead zone")
        #expect(
            m.up(at: CGPoint(x: 12, y: 11), modifiers: [])
                == [.click(hit: "a", at: CGPoint(x: 10, y: 10), clicks: 2, modifiers: [.shift])])
        #expect(m.phase == .idle)
    }

    @Test func movingPastThreePointsBeginsADrag() {
        var m = M()
        _ = m.down("a", at: .zero, clicks: 1, modifiers: [])
        #expect(m.drag(to: CGPoint(x: 3, y: 0), modifiers: []).isEmpty, "exactly 3 is still inside")
        let fx = m.drag(to: CGPoint(x: 4, y: 0), modifiers: [.option])
        #expect(
            fx == [
                .begin(hit: "a", from: .zero, modifiers: [.option]),
                .move(hit: "a", from: .zero, to: CGPoint(x: 4, y: 0), modifiers: [.option]),
            ])
        #expect(
            m.drag(to: CGPoint(x: 9, y: 2), modifiers: [])
                == [.move(hit: "a", from: .zero, to: CGPoint(x: 9, y: 2), modifiers: [])])
        #expect(
            m.up(at: CGPoint(x: 9, y: 2), modifiers: [])
                == [.end(hit: "a", from: .zero, to: CGPoint(x: 9, y: 2), modifiers: [])])
    }

    @Test func escapeCancelsADragAndIgnoresTheRest() {
        var m = M()
        _ = m.down("a", at: .zero, clicks: 1, modifiers: [])
        _ = m.drag(to: CGPoint(x: 10, y: 0), modifiers: [])
        #expect(m.cancel() == [.cancel(hit: "a")])
        #expect(m.drag(to: CGPoint(x: 20, y: 0), modifiers: []).isEmpty)
        #expect(m.up(at: CGPoint(x: 20, y: 0), modifiers: []).isEmpty)
    }

    @Test func escapeBeforeTheDeadZoneDoesNothing() {
        var m = M()
        _ = m.down("a", at: .zero, clicks: 1, modifiers: [])
        #expect(m.cancel().isEmpty)
        #expect(m.up(at: .zero, modifiers: []).isEmpty)
    }

    @Test func aDownDuringADragCancelsTheLostOne() {
        var m = M()
        _ = m.down("a", at: .zero, clicks: 1, modifiers: [])
        _ = m.drag(to: CGPoint(x: 10, y: 0), modifiers: [])
        #expect(m.down("b", at: .zero, clicks: 1, modifiers: []) == [.cancel(hit: "a")])
    }
}
