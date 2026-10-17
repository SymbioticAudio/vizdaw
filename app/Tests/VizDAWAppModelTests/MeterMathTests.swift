import Testing
import VizDAWCore
@testable import VizDAWAppModel

let Q: Int64 = 15_360
let BAR: Int64 = 4 * Q

func meter(_ tick: Int64, _ num: UInt8, _ den: UInt8) -> MeterEntryRow {
    MeterEntryRow(tick: tick, num: num, den: den, sharps: 0, minor: false)
}

@Suite struct MeterMathTests {
    @Test func barsCountFromOneAtTickZero() {
        let m = [meter(0, 4, 4)]
        #expect(MeterMath.bar(at: 0, m) == MeterMath.Bar(number: 1, start: 0, length: BAR, meter: m[0]))
        #expect(MeterMath.bar(at: BAR - 1, m).number == 1)
        #expect(MeterMath.bar(at: BAR, m).number == 2)
        #expect(MeterMath.barStart(3, m) == 2 * BAR)
    }

    @Test func pickupBarsNumberBackwards() {
        let m = [meter(0, 4, 4)]
        #expect(MeterMath.bar(at: -1, m).number == 0)
        #expect(MeterMath.bar(at: -1, m).start == -BAR)
        #expect(MeterMath.bar(at: -BAR - 1, m).number == -1)
        #expect(MeterMath.barStart(0, m) == -BAR)
    }

    @Test func meterChangesContinueTheBarNumbers() {
        let m = [meter(0, 4, 4), meter(2 * BAR, 3, 4)]
        let third = MeterMath.bar(at: 2 * BAR, m)
        #expect(third.number == 3)
        #expect(third.length == 3 * Q)
        #expect(MeterMath.bar(at: 2 * BAR + 3 * Q, m).number == 4)
        #expect(MeterMath.barStart(4, m) == 2 * BAR + 3 * Q)
        #expect(MeterMath.beatTicks(meter(0, 6, 8)) == Q / 2)
    }

    @Test func anEmptyMeterIsFourFour() {
        #expect(MeterMath.bar(at: BAR, []).number == 2)
    }
}
