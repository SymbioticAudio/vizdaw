import CoreGraphics
import Testing
import VizDAWAppModel
import VizDAWCore
@testable import VizDAWEditors

@Suite struct ScrollingTests {
    @Test func autoScrollStartsNearTheEdges() {
        #expect(AutoScroll.speed(x: 100, width: 400) == 0)
        #expect(AutoScroll.speed(x: 24, width: 400) == 0)
        #expect(AutoScroll.speed(x: 14, width: 400) == -200)
        #expect(AutoScroll.speed(x: 390, width: 400) == 280)
        #expect(AutoScroll.speed(x: -500, width: 400) == -AutoScroll.maxSpeed)
    }

    @Test func pageFollowJumpsWhenThePlayheadLeaves() {
        let a = TimeAxis(ticksPerPoint: 100, originTick: 0)
        #expect(PageFollow.origin(forPlayhead: 5_000, axis: a, width: 400) == nil)
        #expect(PageFollow.origin(forPlayhead: 41_000, axis: a, width: 400) == Double(41_000 - 1_600))
        #expect(PageFollow.origin(forPlayhead: -100, axis: a, width: 400) == Double(-100 - 1_600))
    }

    @Test func rulerRangesSnapAndIgnoreDirection() {
        let a = TimeAxis(ticksPerPoint: 128, originTick: 0)
        let m = [MeterEntryRow(tick: 0, num: 4, den: 4, sharps: 0, minor: false)]
        let r = RulerSelection.range(fromTick: a.tick(atX: 250), toX: 10, axis: a, snap: Snap(), meter: m)
        #expect(r == TickRange(start: 0, end: 30_720))
        #expect(RulerSelection.range(fromTick: 100, toX: 1, axis: a, snap: Snap(), meter: m) == nil)
    }

    @Test func wheelLinesScaleToPointsAndTrackpadPointsPassThrough() {
        let precise = WheelScroll.points(dx: 3, dy: -5, precise: true)
        #expect(precise.dx == 3 && precise.dy == -5)
        let wheel = WheelScroll.points(dx: 1, dy: -2, precise: false)
        #expect(wheel.dx == 12 && wheel.dy == -24)
    }

    @Test func oneWheelNotchZoomsAboutTenPercent() {
        #expect(abs(WheelScroll.zoomFactor(dy: 1, precise: false) - 1.1) < 1e-9)
        #expect(abs(WheelScroll.zoomFactor(dy: -1, precise: false) - 1 / 1.1) < 1e-9)
        #expect(abs(WheelScroll.zoomFactor(dy: 1, precise: true) - 1.01) < 1e-9)
        #expect(WheelScroll.zoomFactor(dy: 0, precise: false) == 1)
    }
}
