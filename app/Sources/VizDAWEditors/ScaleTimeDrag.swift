import Foundation
import VizDAWAppModel
import VizDAWCore

/// The piano roll's scale-time drag (parent spec §9 exact mode, Plan 4d
/// Ruling 23): ⌘-dragging a selected note's end scales the selected
/// notes' times and lengths around their earliest start, by new span /
/// old span.
public struct ScaleTimeDrag: Equatable, Sendable {
    public static let label = "Scale Time"

    /// The earliest start.
    public let anchor: Int64
    /// From the earliest start to the latest end (≥ 1).
    public let span: Int64

    public init?(notes: [EventRow]) {
        guard let a = notes.map(\.tick).min(), let e = notes.map({ $0.tick + max($0.duration, 0) }).max(), e > a
        else { return nil }
        anchor = a
        span = e - a
    }

    /// The edit for a drag of `rawTicks`: the latest end moves (snapped
    /// when snap is on), never closer than a tick to the anchor; nil when
    /// nothing changes.
    public func command(rawTicks: Int64, snap: Snap, meter: [MeterEntryRow]) -> EditCommand? {
        let end = NoteDrag.clampTick(anchor + span + NoteDrag.clampTick(rawTicks))
        let moved = snap.enabled ? snap.apply(end, meter: meter) : end
        let newSpan = max(moved - anchor, 1)
        guard newSpan != span else { return nil }
        let g = Self.gcd(newSpan, span)
        return .selectAndModify(
            rule: nil, mode: .select,
            modify: .scaleTime(num: UInt32(clamping: newSpan / g), den: UInt32(clamping: span / g), anchor: anchor),
            toCopy: false)
    }

    static func gcd(_ a: Int64, _ b: Int64) -> Int64 {
        var (x, y) = (abs(a), abs(b))
        while y != 0 { (x, y) = (y, x % y) }
        return max(x, 1)
    }
}
