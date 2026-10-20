import CoreGraphics
import VizDAWAppModel
import VizDAWCore

/// One audio region and its lane.
public struct RegionItem: Equatable, Sendable {
    public let lane: LaneKey
    public let row: EventRow

    public init(lane: LaneKey, row: EventRow) {
        self.lane = lane
        self.row = row
    }
}

/// One row of the strip: an audio output path (nil: lanes without one).
public struct RegionLine: Equatable, Sendable {
    public let output: UInt64?
    public let title: String
    public let items: [RegionItem]
}

public enum RegionHit: Equatable, Sendable {
    case mute(UInt64)
    case solo(UInt64)
    case region(RegionItem)
    /// A region's fade or gain handle, with the region's rectangle (plan 5b).
    case handle(RegionItem, RegionHandle, CGRect)
    case none
}

/// The region strip between the notes and the expression pane (spec §4b,
/// Plan 4b Ruling 13). Pure: lines, rectangles, hits, the move target.
public enum RegionStripLayout {
    public static let rowHeight: CGFloat = 22

    /// One line per output path of the lanes that hold regions, in lane
    /// order, then "No Output"; each line's regions in time order.
    public static func lines(
        _ lanes: [(lane: LaneKey, output: UInt64?, regions: [EventRow])], names: [UInt64: String]
    ) -> [RegionLine] {
        var order: [UInt64] = []
        var byOutput: [UInt64: [RegionItem]] = [:]
        var loose: [RegionItem] = []
        for l in lanes where !l.regions.isEmpty {
            let items = l.regions.map { RegionItem(lane: l.lane, row: $0) }
            guard let o = l.output else {
                loose += items
                continue
            }
            if byOutput[o] == nil { order.append(o) }
            byOutput[o, default: []] += items
        }
        let sorted = { (items: [RegionItem]) in items.sorted { $0.row.tick < $1.row.tick } }
        var out = order.map { o in
            RegionLine(output: o, title: names[o] ?? "Output \(o)", items: sorted(byOutput[o] ?? []))
        }
        if !loose.isEmpty { out.append(RegionLine(output: nil, title: "No Output", items: sorted(loose))) }
        return out
    }

    public static func height(lines: Int) -> CGFloat { lines == 0 ? 0 : CGFloat(lines) * rowHeight + 1 }

    static func top(_ line: Int) -> CGFloat { CGFloat(line) * rowHeight }

    public static func rect(_ r: EventRow, line: Int, time: TimeAxis, side: CGFloat) -> CGRect {
        CGRect(
            x: side + time.x(r.tick), y: top(line) + 2, width: max(time.width(ofTicks: r.duration), 3),
            height: rowHeight - 4)
    }

    /// True when a region of `line` (at `index`) covers `title`, where the
    /// line's title would be drawn: the title is then left out, never
    /// drawn over the region (final review V1).
    public static func titleCovered(_ title: CGRect, line: RegionLine, index: Int, time: TimeAxis, side: CGFloat)
        -> Bool
    {
        line.items.contains { rect($0.row, line: index, time: time, side: side).intersects(title) }
    }

    public static func muteRect(line: Int) -> CGRect { CGRect(x: 4, y: top(line) + 3, width: 18, height: 16) }
    public static func soloRect(line: Int) -> CGRect { CGRect(x: 24, y: top(line) + 3, width: 18, height: 16) }

    public static func hit(_ p: CGPoint, lines: [RegionLine], time: TimeAxis, side: CGFloat) -> RegionHit {
        guard p.y >= 0, p.y.isFinite else { return .none }
        let i = Int((p.y / rowHeight).rounded(.down))
        guard i < lines.count else { return .none }
        let line = lines[i]
        if p.x < side {
            guard let o = line.output else { return .none }
            if muteRect(line: i).contains(p) { return .mute(o) }
            if soloRect(line: i).contains(p) { return .solo(o) }
            return .none
        }
        if let item = line.items.last(where: { rect($0.row, line: i, time: time, side: side).contains(p) }) {
            return .region(item)
        }
        return .none
    }

    /// The lane of the region under `p`: where a drop there goes.
    public static func dropLane(_ p: CGPoint, lines: [RegionLine], time: TimeAxis, side: CGFloat) -> LaneKey? {
        if case .region(let item) = hit(p, lines: lines, time: time, side: side) { return item.lane }
        return nil
    }

    /// A region handle under `p`, if any (checked before `hit`).
    public static func handleHit(_ p: CGPoint, lines: [RegionLine], time: TimeAxis, side: CGFloat) -> RegionHit? {
        guard p.x >= side, p.y.isFinite, p.y >= -2 else { return nil }
        let i = max(Int((p.y / rowHeight).rounded(.down)), 0)
        guard i < lines.count else { return nil }
        for item in lines[i].items.reversed() {
            let r = rect(item.row, line: i, time: time, side: side)
            if let h = RegionHandles.hit(p, row: item.row, in: r) { return .handle(item, h, r) }
        }
        return nil
    }

    /// A region drag's full target: its start snapped (nil: no change).
    public static func moveCommand(
        _ item: RegionItem, rawTicks: Int64, snap: Snap, meter: [MeterEntryRow]
    ) -> EditCommand? {
        let d = NoteDrag.timeDelta(rawTicks: rawTicks, start: item.row.tick, snap: snap, meter: meter)
        return d == 0 ? nil : .event(edit: .move(events: [item.row.id], delta: d, toLane: nil))
    }
}
