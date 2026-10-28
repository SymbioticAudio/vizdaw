import CoreGraphics
import VizDAWAppModel
import VizDAWCore

/// Rows and block rectangles of the arrangement, in `content` coordinates
/// (y = 0 is the top of the first lane before scrolling).
public struct ArrangementGeometry: Equatable, Sendable {
    public static let blockInset: CGFloat = 2
    public static let minBlockWidth: CGFloat = 4

    public var laneHeight: CGFloat = 48
    public var verticalOffset: CGFloat = 0

    public init() {}

    public func rowTop(_ i: Int) -> CGFloat { CGFloat(i) * laneHeight - verticalOffset }

    public func row(atY y: CGFloat, count: Int) -> Int? {
        guard laneHeight > 0 else { return nil }
        let i = Int(((y + verticalOffset) / laneHeight).rounded(.down))
        return (0..<count).contains(i) ? i : nil
    }

    public func blockRect(_ b: BlockRow, row: Int, axis: TimeAxis) -> CGRect {
        let x0 = axis.x(b.start)
        let x1 = axis.x(b.end)
        return CGRect(
            x: x0, y: rowTop(row) + Self.blockInset, width: max(x1 - x0, Self.minBlockWidth),
            height: laneHeight - 2 * Self.blockInset)
    }

    /// The lanes plus one spare row (the "Add Lane" row of the headers).
    public func contentHeight(rows: Int) -> CGFloat { CGFloat(rows + 1) * laneHeight }

    /// An audio region's rectangle in row `row`: its own span, a point
    /// inside the block band.
    public func regionRect(_ r: EventRow, row: Int, axis: TimeAxis) -> CGRect {
        let len = max(r.duration, 0)
        let end = r.tick > Int64.max - len ? Int64.max : r.tick + len
        let x0 = axis.x(r.tick)
        let x1 = axis.x(end)
        let inset = Self.blockInset + 1
        return CGRect(
            x: x0, y: rowTop(row) + inset, width: max(x1 - x0, Self.minBlockWidth),
            height: max(laneHeight - 2 * inset, 1))
    }
}

public struct BlockHit: Equatable, Sendable {
    public let lane: LaneKey
    public let index: Int
    public let block: BlockRow

    public init(lane: LaneKey, index: Int, block: BlockRow) {
        self.lane = lane
        self.index = index
        self.block = block
    }
}

public enum ArrangementHit: Equatable, Sendable {
    case block(BlockHit)
    case empty(lane: LaneKey?, tick: Int64)
    /// A region's fade or gain handle (plan 5b).
    case handle(RegionHandleHit)
}

public enum ArrangementHitTest {
    /// What `p` lands on; among overlapping blocks the last drawn wins.
    public static func hit(
        _ p: CGPoint, lanes: [LaneKey], blocks: [LaneKey: [BlockRow]],
        geometry: ArrangementGeometry, axis: TimeAxis
    ) -> ArrangementHit {
        let tick = axis.tick(atX: p.x)
        guard let row = geometry.row(atY: p.y, count: lanes.count) else {
            return .empty(lane: nil, tick: tick)
        }
        let key = lanes[row]
        let list = blocks[key] ?? []
        for (i, b) in list.enumerated().reversed()
        where geometry.blockRect(b, row: row, axis: axis).contains(p) {
            return .block(BlockHit(lane: key, index: i, block: b))
        }
        return .empty(lane: key, tick: tick)
    }
}

/// Light drawing mode (Plan Ruling 17).
public enum LightDrawing {
    public static let threshold = 1_500

    public static func isLight(_ mode: LightMode, visibleBlocks: Int) -> Bool {
        switch mode {
        case .on: true
        case .off: false
        case .auto: visibleBlocks > threshold
        }
    }
}

/// Moving, copying and merging blocks (parent spec §9, Plan Ruling 23).
public enum BlockDrag {
    /// The dragged distance, snapped so the pressed block's start lands
    /// on the grid.
    public static func delta(rawTicks: Int64, blockStart: Int64, snap: Snap, meter: [MeterEntryRow]) -> Int64 {
        guard snap.enabled else { return rawTicks }
        return snap.apply(blockStart + rawTicks, meter: meter) - blockStart
    }

    /// The events a drag carries: the pressed block's, plus those of every
    /// other fully selected block of the lane when the pressed one is
    /// selected.
    public static func events(pressed: BlockRow, laneBlocks: [BlockRow], selected: Set<UInt64>) -> [UInt64] {
        let isSelected = { (b: BlockRow) in !b.events.isEmpty && b.events.allSatisfy(selected.contains) }
        guard isSelected(pressed) else { return pressed.events }
        return laneBlocks.filter(isSelected).flatMap(\.events)
    }

    /// The full target of a drag of `events` from lane `from` by `delta`
    /// into lane `to`: a move (merging into `to`), or with `copy` shifted
    /// copies of the `rows` — a duplicate in their own lane (so exact mode
    /// can offer it as Move to a Copy), an insert into another lane. Nil
    /// when nothing would change.
    public static func command(
        events: [UInt64], rows: [EventRow], from: LaneKey, to: LaneKey, delta: Int64, copy: Bool
    ) -> EditCommand? {
        if delta == 0 && from == to { return nil }
        if copy && from == to {
            let present = Set(rows.map(\.id))
            let copied = events.filter(present.contains)
            guard !copied.isEmpty else { return nil }
            return .event(edit: .duplicate(events: copied, ticks: delta, semitones: 0))
        }
        if copy {
            let ids = Set(events)
            let copies = rows.filter { ids.contains($0.id) }.map { r -> EventRow in
                var c = r
                c.tick += delta
                return c
            }
            guard !copies.isEmpty else { return nil }
            return .event(edit: .insert(seq: to.seq, lane: to.lane, events: copies))
        }
        return .event(edit: .move(events: events, delta: delta, toLane: from == to ? nil : to.ref))
    }

    public static func label(copy: Bool) -> String { copy ? "Copy Block" : "Move Block" }
}

/// VoiceOver labels (spec §3.5).
public enum ArrangementA11y {
    public static func label(_ b: BlockRow, laneName: String, meter: [MeterEntryRow]) -> String {
        let first = MeterMath.bar(at: b.start, meter).number
        let last = MeterMath.bar(at: max(b.end - 1, b.start), meter).number
        let bars = first == last ? "bar \(first)" : "bars \(first) to \(last)"
        let muted = b.muted ? ", muted" : ""
        switch b.kind {
        case .reference:
            return "Reference to \(b.label ?? "a sequence") on \(laneName), \(bars)\(muted)"
        case .events:
            let n = b.events.count
            return "Block on \(laneName), \(bars), \(n) event\(n == 1 ? "" : "s")\(muted)"
        }
    }

    /// `returnOf`: the synth whose return the lane records (its badge).
    public static func header(_ lane: LaneRow, returnOf: String? = nil) -> String {
        var parts = [lane.name, lane.destination ?? "no destination"]
        if lane.mute { parts.append("muted") }
        if lane.solo { parts.append("soloed") }
        if lane.arm { parts.append("armed") }
        if lane.input != nil { parts.append("input " + LaneInputs.title(lane.input)) }
        if let r = returnOf { parts.append("recording the return of " + r) }
        switch lane.monitor {
        case .auto: break
        case .input: parts.append("monitoring input")
        case .off: parts.append("monitoring off")
        }
        return parts.joined(separator: ", ")
    }
}

/// The clickable parts of a lane header (Plan Ruling 24; the monitor
/// button and the input picker, plan 5d-2 Task 5).
public enum HeaderControl: Equatable, Sendable {
    case name, monitor, mute, solo, arm, destination, input
}

public enum LaneHeaderLayout {
    public static let width: CGFloat = 200

    public static func controls(rowTop y: CGFloat) -> [(HeaderControl, CGRect)] {
        [
            (.name, CGRect(x: 8, y: y + 4, width: 101, height: 18)),
            (.monitor, CGRect(x: 113, y: y + 4, width: 18, height: 18)),
            (.mute, CGRect(x: 134, y: y + 4, width: 18, height: 18)),
            (.solo, CGRect(x: 155, y: y + 4, width: 18, height: 18)),
            (.arm, CGRect(x: 176, y: y + 4, width: 18, height: 18)),
            (.destination, CGRect(x: 8, y: y + 24, width: 92, height: 17)),
            (.input, CGRect(x: 104, y: y + 24, width: 90, height: 17)),
        ]
    }

    /// The input meter, under the controls (shown while the lane is armed
    /// or monitors its input).
    public static func meterRect(rowTop y: CGFloat) -> CGRect {
        CGRect(x: 8, y: y + 43, width: 178, height: 3)
    }

    /// The clip mark at the meter's end.
    public static func clipRect(rowTop y: CGFloat) -> CGRect {
        CGRect(x: 189, y: y + 43, width: 5, height: 3)
    }

    public static func control(at p: CGPoint, rowTop: CGFloat) -> HeaderControl? {
        controls(rowTop: rowTop).first { $0.1.contains(p) }?.0
    }
}
