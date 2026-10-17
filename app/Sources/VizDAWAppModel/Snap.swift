import VizDAWCore

/// The shared edit grid (spec §3.5): one setting for every editor,
/// toggled by one key (Plan Ruling 16). The grid is counted from the start
/// of the bar a tick is in, so a bar line always snaps.
public struct Snap: Codable, Equatable, Sendable {
    public enum Division: String, Codable, CaseIterable, Sendable {
        case bar, half, quarter, eighth, sixteenth, thirtySecond

        public var title: String {
            switch self {
            case .bar: "Bar"
            case .half: "1/2"
            case .quarter: "1/4"
            case .eighth: "1/8"
            case .sixteenth: "1/16"
            case .thirtySecond: "1/32"
            }
        }
    }

    public var division: Division = .quarter
    public var triplet = false
    public var dotted = false
    public var enabled = true

    public init() {}

    /// The grid step in ticks inside a bar of meter `m`.
    public func step(in m: MeterEntryRow) -> Int64 {
        let q = MeterMath.ppq
        var s: Int64
        switch division {
        case .bar: return MeterMath.barTicks(m)
        case .half: s = 2 * q
        case .quarter: s = q
        case .eighth: s = q / 2
        case .sixteenth: s = q / 4
        case .thirtySecond: s = q / 8
        }
        if triplet { s = s * 2 / 3 }
        if dotted { s = s * 3 / 2 }
        return max(s, 1)
    }

    /// `tick` on the nearest grid line of its bar (ties go later); the bar's
    /// end counts as a grid line. Unchanged when snap is off.
    public func apply(_ tick: Int64, meter: [MeterEntryRow]) -> Int64 {
        guard enabled else { return tick }
        let bar = MeterMath.bar(at: tick, meter)
        let s = step(in: bar.meter)
        let down = bar.start + ((tick - bar.start) / s) * s
        let up = min(down + s, bar.start + bar.length)
        return tick - down < up - tick ? down : up
    }
}
