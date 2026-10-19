import VizDAWAppModel
import VizDAWCore

/// One line of the editor grid.
public struct GridLine: Equatable, Sendable {
    public enum Kind: Equatable, Sendable {
        /// A bar line with its bar number.
        case bar(Int64)
        case beat
    }

    public let tick: Int64
    public let kind: Kind
}

/// Bar and beat lines for the grid layer and the ruler (spec §3.5).
public enum Grid {
    /// Lines from the bar containing `from` up to `to`. Beat lines only
    /// when beats are at least `minSpacing` points apart; bar lines thinned
    /// to every 2ⁿ-th bar (counted from bar 1) when bars are closer than
    /// that. At most `maxLines` lines.
    public static func lines(
        meter: [MeterEntryRow], from: Int64, to: Int64, ticksPerPoint: Double,
        minSpacing: Double = 6, maxLines: Int = 4_000
    ) -> [GridLine] {
        var out: [GridLine] = []
        var bar = MeterMath.bar(at: from, meter)
        var visits = 0
        while bar.start <= to && out.count < maxLines && visits < 200_000 {
            visits += 1
            let barPoints = Double(bar.length) / ticksPerPoint
            var every: Int64 = 1
            while Double(every) * barPoints < minSpacing && every < (1 << 20) { every *= 2 }
            if MeterMath.floorDiv(bar.number - 1, every) * every == bar.number - 1 {
                out.append(GridLine(tick: bar.start, kind: .bar(bar.number)))
            }
            let beat = MeterMath.beatTicks(bar.meter)
            if Double(beat) / ticksPerPoint >= minSpacing {
                var t = bar.start + beat
                while t < bar.start + bar.length && out.count < maxLines {
                    if t >= from && t <= to { out.append(GridLine(tick: t, kind: .beat)) }
                    t += beat
                }
            }
            bar = MeterMath.bar(at: bar.start + bar.length, meter)
        }
        return out
    }

    /// Label every n-th bar so labels are at least `minSpacing` points
    /// apart (n a power of two).
    public static func labelEvery(barTicks: Int64, ticksPerPoint: Double, minSpacing: Double = 28) -> Int64 {
        var every: Int64 = 1
        while Double(every) * Double(barTicks) / ticksPerPoint < minSpacing && every < (1 << 20) {
            every *= 2
        }
        return every
    }
}
