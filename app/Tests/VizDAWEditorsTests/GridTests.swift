import Testing
import VizDAWCore
@testable import VizDAWEditors

private let Q: Int64 = 15_360
private let BAR: Int64 = 4 * Q

private func meter(_ tick: Int64, _ num: UInt8, _ den: UInt8) -> MeterEntryRow {
    MeterEntryRow(tick: tick, num: num, den: den, sharps: 0, minor: false)
}

private func barNumbers(_ lines: [GridLine]) -> [Int64] {
    lines.compactMap { l -> Int64? in
        if case .bar(let n) = l.kind { n } else { nil }
    }
}

@Suite struct GridTests {
    @Test func barsAndBeatsAtNormalZoom() {
        let lines = Grid.lines(meter: [meter(0, 4, 4)], from: 0, to: 2 * BAR, ticksPerPoint: 128)
        #expect(lines.filter { $0.kind == .beat }.count == 6)
        #expect(barNumbers(lines) == [1, 2, 3])
        #expect(lines.first?.tick == 0)
    }

    @Test func beatsDropOutWhenTheyCrowd() {
        let lines = Grid.lines(meter: [meter(0, 4, 4)], from: 0, to: 4 * BAR, ticksPerPoint: Double(Q) / 4)
        #expect(lines.allSatisfy { $0.kind != .beat })
        #expect(lines.count == 5)
    }

    @Test func barsThinOutFarOut() {
        // One point per bar: every eighth bar keeps 6 points apart.
        let lines = Grid.lines(meter: [meter(0, 4, 4)], from: 0, to: 20 * BAR, ticksPerPoint: Double(BAR))
        #expect(barNumbers(lines) == [1, 9, 17])
    }

    @Test func pickupBarsAreNumberedBackwards() {
        let lines = Grid.lines(meter: [meter(0, 4, 4)], from: -2 * BAR, to: 0, ticksPerPoint: 512)
        let bars = lines.compactMap { l -> (Int64, Int64)? in
            if case .bar(let n) = l.kind { (n, l.tick) } else { nil }
        }
        #expect(bars.map(\.0) == [-1, 0, 1])
        #expect(bars.map(\.1) == [-2 * BAR, -BAR, 0])
    }

    @Test func meterChangesMoveTheBeats() {
        let m = [meter(0, 4, 4), meter(BAR, 3, 4)]
        let lines = Grid.lines(meter: m, from: BAR, to: BAR + 3 * Q, ticksPerPoint: 128)
        #expect(lines.map(\.tick) == [BAR, BAR + Q, BAR + 2 * Q, BAR + 3 * Q])
        #expect(lines.last?.kind == .bar(3))
    }

    @Test func labelsSpreadOut() {
        #expect(Grid.labelEvery(barTicks: BAR, ticksPerPoint: 128) == 1)
        #expect(Grid.labelEvery(barTicks: BAR, ticksPerPoint: Double(BAR) / 10) == 4)
    }
}
