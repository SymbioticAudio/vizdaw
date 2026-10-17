import Foundation
import Testing
import VizDAWCore
@testable import VizDAWAppModel

@Suite struct SnapTests {
    let fourFour = [meter(0, 4, 4)]

    @Test func quarterSnapsToTheNearestBeat() {
        let s = Snap()
        #expect(s.apply(Q * 4 / 10, meter: fourFour) == 0)
        #expect(s.apply(Q * 6 / 10, meter: fourFour) == Q)
        #expect(s.apply(Q / 2, meter: fourFour) == Q, "ties go up")
    }

    @Test func offLeavesTicksAlone() {
        var s = Snap()
        s.enabled = false
        #expect(s.apply(123, meter: fourFour) == 123)
    }

    @Test func tripletsAndDots() {
        var s = Snap()
        s.triplet = true
        #expect(s.step(in: fourFour[0]) == Q * 2 / 3)
        s = Snap()
        s.division = .eighth
        s.dotted = true
        #expect(s.step(in: fourFour[0]) == Q * 3 / 4)
    }

    @Test func barSnapFollowsTheMeter() {
        var s = Snap()
        s.division = .bar
        let threeFour = [meter(0, 3, 4)]
        #expect(s.apply(30_000, meter: threeFour) == 3 * Q)
        #expect(s.apply(20_000, meter: threeFour) == 0)
    }

    @Test func aGridThatDoesNotDivideTheBarSnapsToTheBarLine() {
        var s = Snap()
        s.division = .half
        s.triplet = true  // 20 480 ticks: three per 4/4 bar, fits exactly
        #expect(s.apply(BAR - 100, meter: fourFour) == BAR)
        let fiveFour = [meter(0, 5, 4)]
        s = Snap()
        s.division = .half  // 30 720: 5/4 bar ends 15 360 after the second half
        #expect(s.apply(5 * Q - 1_000, meter: fiveFour) == 5 * Q)
    }

    @Test func pickupBarsSnapAndNumberBackwards() {
        let s = Snap()
        #expect(s.apply(-Q * 4 / 10, meter: fourFour) == 0)
        #expect(s.apply(-BAR + 100, meter: fourFour) == -BAR)
    }

    @Test func snapRoundTripsThroughJSON() throws {
        var s = Snap()
        s.division = .sixteenth
        s.triplet = true
        let data = try JSONEncoder().encode(s)
        #expect(try JSONDecoder().decode(Snap.self, from: data) == s)
    }

    @Test func floorTakesTheGridLineAtOrBefore() {
        let meter = [MeterEntryRow(tick: 0, num: 4, den: 4, sharps: 0, minor: false)]
        var s = Snap()
        s.division = .quarter
        #expect(s.floor(15_359, meter: meter) == 0)
        #expect(s.floor(15_360, meter: meter) == 15_360)
        #expect(s.floor(-1, meter: meter) == -15_360, "pickup bars")
        s.enabled = false
        #expect(s.floor(15_359, meter: meter) == 15_359)
    }
}
