import CoreGraphics
import Testing
@testable import VizDAWEditors

@Suite struct TimeAxisTests {
    @Test func ticksAndPointsRoundTrip() {
        let a = TimeAxis(ticksPerPoint: 100, originTick: 1_000)
        #expect(a.x(1_000) == 0)
        #expect(a.x(2_000) == 10)
        #expect(a.tick(atX: 10) == 2_000)
        #expect(a.width(ofTicks: 15_360) == 153.6)
        #expect(a.visibleRange(width: 100) == 1_000...11_000)
    }

    @Test func zoomKeepsTheTickUnderThePointer() {
        let a = TimeAxis(ticksPerPoint: 100, originTick: 1_000)
        let z = a.zoomed(by: 2, aroundX: 50)
        #expect(z.ticksPerPoint == 50)
        #expect(z.tick(atX: 50) == a.tick(atX: 50))
    }

    @Test func zoomClampsToItsLimits() {
        let a = TimeAxis()
        #expect(a.zoomed(by: 1e9, aroundX: 0).ticksPerPoint == TimeAxis.minTicksPerPoint)
        #expect(a.zoomed(by: 1e-9, aroundX: 0).ticksPerPoint == TimeAxis.maxTicksPerPoint)
    }

    @Test func extremeOffsetsClampInsteadOfTrapping() {
        let far = TimeAxis().scrolled(byPoints: 1e30)
        #expect(far.originTick == Double(TimeAxis.limit))
        #expect(far.tick(atX: .greatestFiniteMagnitude) == TimeAxis.limit)
        let back = TimeAxis().scrolled(byPoints: -1e30)
        #expect(back.tick(atX: 0) == -TimeAxis.limit)
        #expect(TimeAxis.clampTick(.nan) == 0)
        #expect(TimeAxis().x(Int64.max).isFinite)
        #expect(TimeAxis().scrolled(toTick: .infinity).originTick == Double(TimeAxis.limit))
    }
}
