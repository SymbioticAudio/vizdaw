import CoreGraphics
import Testing
import VizDAWAppModel
import VizDAWCore
@testable import VizDAWEditors

private let q: Int64 = 15_360
private let meter = [MeterEntryRow(tick: 0, num: 4, den: 4, sharps: 0, minor: false)]
private let time = TimeAxis(ticksPerPoint: 96)

private func region(_ id: UInt64, _ tick: Int64, _ length: Int64 = 2 * q) -> EventRow {
    var r = Pencil.note(tick: tick, pitch: 0, length: length)
    r.id = id
    r.kind = .audioRegion
    return r
}

@Suite struct RegionStripTests {
    let a = LaneKey(3, 40)
    let b = LaneKey(3, 41)
    let c = LaneKey(3, 42)

    @Test func oneLinePerOutputPathInLaneOrderNoOutputLast() {
        let lines = RegionStripLayout.lines(
            [
                (lane: c, output: nil, regions: [region(9, 0)]),
                (lane: a, output: 7, regions: [region(2, q), region(1, 0)]),
                (lane: LaneKey(3, 43), output: 8, regions: []),
                (lane: b, output: 7, regions: [region(3, 4 * q)]),
            ], names: [7: "Audio 1-2"])
        #expect(lines.map(\.title) == ["Audio 1-2", "No Output"])
        #expect(lines[0].output == 7)
        #expect(lines[0].items.map(\.row.id) == [1, 2, 3], "time order across lanes")
        #expect(lines[1].items.map(\.lane) == [c])
        #expect(RegionStripLayout.lines([(lane: a, output: 5, regions: [region(1, 0)])], names: [:])[0].title == "Output 5")
        #expect(RegionStripLayout.height(lines: 0) == 0)
        #expect(RegionStripLayout.height(lines: 2) == 45)
    }

    @Test func hitsFindButtonsAndRegions() {
        let lines = RegionStripLayout.lines(
            [(lane: a, output: 7, regions: [region(1, q)]), (lane: c, output: nil, regions: [region(9, 0)])],
            names: [:])
        let side: CGFloat = 64
        #expect(RegionStripLayout.hit(CGPoint(x: 10, y: 11), lines: lines, time: time, side: side) == .mute(7))
        #expect(RegionStripLayout.hit(CGPoint(x: 30, y: 11), lines: lines, time: time, side: side) == .solo(7))
        #expect(RegionStripLayout.hit(CGPoint(x: 10, y: 33), lines: lines, time: time, side: side) == .none, "No Output has no buttons")
        #expect(
            RegionStripLayout.hit(CGPoint(x: side + 200, y: 11), lines: lines, time: time, side: side)
                == .region(RegionItem(lane: a, row: region(1, q))))
        #expect(RegionStripLayout.hit(CGPoint(x: side + 50, y: 11), lines: lines, time: time, side: side) == .none)
        #expect(RegionStripLayout.hit(CGPoint(x: 10, y: 99), lines: lines, time: time, side: side) == .none)
    }

    @Test func aRegionDragMovesItSnappingItsStart() {
        var snap = Snap()
        snap.division = .bar
        let item = RegionItem(lane: a, row: region(1, q))
        #expect(RegionStripLayout.moveCommand(item, rawTicks: 3 * q, snap: snap, meter: meter)
            == .event(edit: .move(events: [1], delta: 3 * q, toLane: nil)))
        #expect(RegionStripLayout.moveCommand(item, rawTicks: 100, snap: Snap(), meter: meter) == nil, "back on its quarter")
    }
}
