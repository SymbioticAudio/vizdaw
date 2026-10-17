import CoreGraphics
import Foundation
import Testing
import VizDAWAppModel
import VizDAWCore
@testable import VizDAWEditors

private let Q: Int64 = 15_360
private let BAR: Int64 = 4 * Q
private let fourFour = [MeterEntryRow(tick: 0, num: 4, den: 4, sharps: 0, minor: false)]

private func block(_ s: Int64, _ e: Int64, _ ids: [UInt64], ref: UInt64? = nil, muted: Bool = false) -> BlockRow {
    BlockRow(
        start: s, end: e, kind: ref == nil ? .events : .reference, events: ids,
        label: ref == nil ? nil : "Groove", refSeq: ref, muted: muted)
}

private func row(_ id: UInt64, tick: Int64) -> EventRow {
    EventRow(
        id: id, tick: tick, kind: .note, mute: false, dest: nil, groupChannel: nil, pitch: 60,
        velocity: 0x8000, releaseVelocity: 0, duration: Q, value: 0, value2: 0, attribute: nil,
        perNote: [], refSeq: nil, refLength: nil, refLoops: 0, refTranspose: 0, media: nil,
        sourceOffset: 0, frames: 0, gainDb: 0, fadeIn: FadeRow(length: 0, shape: .linear),
        fadeOut: FadeRow(length: 0, shape: .linear), text: nil, textKind: nil, data: Data(),
        eightBit: false)
}

private let a = LaneKey(3, 30)
private let b = LaneKey(3, 31)

@Suite struct ArrangementLayoutTests {
    let axis = TimeAxis(ticksPerPoint: 384, originTick: 0)

    @Test func blocksSitInsideTheirRows() {
        let g = ArrangementGeometry()
        #expect(g.blockRect(block(0, BAR, [1]), row: 1, axis: axis) == CGRect(x: 0, y: 50, width: 160, height: 44))
        #expect(g.blockRect(block(BAR, BAR, [1]), row: 0, axis: axis).width == 4, "zero length stays clickable")
        #expect(g.row(atY: 47, count: 2) == 0)
        #expect(g.row(atY: 48, count: 2) == 1)
        #expect(g.row(atY: 96, count: 2) == nil)
        var scrolled = g
        scrolled.verticalOffset = 48
        #expect(scrolled.row(atY: 0, count: 2) == 1)
        #expect(g.contentHeight(rows: 2) == 144, "one spare row for adding lanes")
    }

    @Test func hitTestingPrefersTheTopmostBlock() {
        let blocks = [a: [block(0, 2 * BAR, [1, 2]), block(BAR, 2 * BAR, [9], ref: 2)]]
        let hit = ArrangementHitTest.hit(
            CGPoint(x: 200, y: 10), lanes: [a, b], blocks: blocks, geometry: ArrangementGeometry(), axis: axis)
        #expect(hit == .block(BlockHit(lane: a, index: 1, block: blocks[a]![1])))
        let empty = ArrangementHitTest.hit(
            CGPoint(x: 10, y: 60), lanes: [a, b], blocks: blocks, geometry: ArrangementGeometry(), axis: axis)
        #expect(empty == .empty(lane: b, tick: 3_840))
        let below = ArrangementHitTest.hit(
            CGPoint(x: 10, y: 500), lanes: [a, b], blocks: blocks, geometry: ArrangementGeometry(), axis: axis)
        #expect(below == .empty(lane: nil, tick: 3_840))
    }

    @Test func dragDeltaSnapsTheBlockStart() {
        #expect(BlockDrag.delta(rawTicks: 15_000, blockStart: 1_000, snap: Snap(), meter: fourFour) == 14_360)
        var off = Snap()
        off.enabled = false
        #expect(BlockDrag.delta(rawTicks: 15_000, blockStart: 1_000, snap: off, meter: fourFour) == 15_000)
    }

    @Test func movesStayInTheirLaneOrMergeIntoAnother() {
        #expect(BlockDrag.command(events: [1], rows: [], from: a, to: a, delta: 0, copy: false) == nil)
        #expect(
            BlockDrag.command(events: [1], rows: [], from: a, to: a, delta: Q, copy: false)
                == .event(edit: .move(events: [1], delta: Q, toLane: nil)))
        #expect(
            BlockDrag.command(events: [1], rows: [], from: a, to: b, delta: 0, copy: false)
                == .event(edit: .move(events: [1], delta: 0, toLane: LaneRef(seq: 3, lane: 31))))
    }

    @Test func optionCopiesShiftedRowsIntoTheTargetLane() {
        let rows = [row(1, tick: 0), row(2, tick: Q), row(3, tick: 5 * BAR)]
        let cmd = BlockDrag.command(events: [1, 2], rows: rows, from: a, to: b, delta: BAR, copy: true)
        guard case .event(.insert(let seq, let lane, let events))? = cmd else {
            Issue.record("expected an insert, got \(String(describing: cmd))")
            return
        }
        #expect((seq, lane) == (3, 31))
        #expect(events.map(\.tick) == [BAR, BAR + Q])
        #expect(BlockDrag.label(copy: true) == "Copy Block")
        #expect(BlockDrag.label(copy: false) == "Move Block")
    }

    @Test func optionCopiesInTheSameLaneDuplicateTheShiftedEvents() {
        let rows = [row(1, tick: 0), row(2, tick: Q), row(3, tick: 5 * BAR)]
        #expect(
            BlockDrag.command(events: [1, 2], rows: rows, from: a, to: a, delta: BAR, copy: true)
                == .event(edit: .duplicate(events: [1, 2], ticks: BAR, semitones: 0)))
        #expect(BlockDrag.command(events: [1, 2], rows: rows, from: a, to: a, delta: 0, copy: true) == nil)
        #expect(BlockDrag.command(events: [9], rows: rows, from: a, to: a, delta: BAR, copy: true) == nil)
    }

    @Test func draggingASelectedBlockCarriesTheOtherSelectedBlocksOfItsLane() {
        let lane = [block(0, BAR, [1, 2]), block(2 * BAR, 3 * BAR, [3]), block(4 * BAR, 5 * BAR, [4])]
        #expect(BlockDrag.events(pressed: lane[0], laneBlocks: lane, selected: [1, 2, 4]) == [1, 2, 4])
        #expect(BlockDrag.events(pressed: lane[1], laneBlocks: lane, selected: [1, 2, 4]) == [3])
    }

    @Test func lightModeFollowsTheThreshold() {
        #expect(!LightDrawing.isLight(.auto, visibleBlocks: 1_500))
        #expect(LightDrawing.isLight(.auto, visibleBlocks: 1_501))
        #expect(LightDrawing.isLight(.on, visibleBlocks: 1))
        #expect(!LightDrawing.isLight(.off, visibleBlocks: 100_000))
    }

    @Test func accessibilityLabelsNameKindLaneAndBars() {
        #expect(
            ArrangementA11y.label(block(0, 2 * BAR, [1, 2]), laneName: "Chords", meter: fourFour)
                == "Block on Chords, bars 1 to 2, 2 events")
        #expect(
            ArrangementA11y.label(block(BAR, 2 * BAR, [9], ref: 2, muted: true), laneName: "Groove", meter: fourFour)
                == "Reference to Groove on Groove, bar 2, muted")
    }

    @Test func headerControlsAreFoundByPosition() {
        let controls = LaneHeaderLayout.controls(rowTop: 48)
        let mute = controls.first { $0.0 == .mute }!.1
        #expect(LaneHeaderLayout.control(at: CGPoint(x: mute.midX, y: mute.midY), rowTop: 48) == .mute)
        #expect(LaneHeaderLayout.control(at: CGPoint(x: 100, y: 48 + 30), rowTop: 48) == .destination)
        #expect(LaneHeaderLayout.control(at: CGPoint(x: 199, y: 48 + 46), rowTop: 48) == nil)
    }
}
