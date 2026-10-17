import VizDAWCore

/// Bars and beats over a meter map (`SequenceInfo.meter`), as the core's
/// `MeterMap` counts them: the first entry is at tick 0 and also governs
/// the pickup bars before it; bar 1 starts at tick 0; entries fall on
/// barlines. Ticks stay within ±2⁵⁰ (`TimeAxis.limit`), so nothing traps.
public enum MeterMath {
    /// Ticks per quarter note.
    public static let ppq: Int64 = 15_360

    static let fourFour = MeterEntryRow(tick: 0, num: 4, den: 4, sharps: 0, minor: false)

    /// One bar.
    public struct Bar: Equatable, Sendable {
        public let number: Int64
        public let start: Int64
        public let length: Int64
        public let meter: MeterEntryRow

        public init(number: Int64, start: Int64, length: Int64, meter: MeterEntryRow) {
            self.number = number
            self.start = start
            self.length = length
            self.meter = meter
        }
    }

    public static func beatTicks(_ m: MeterEntryRow) -> Int64 { ppq * 4 / Int64(max(m.den, 1)) }

    public static func barTicks(_ m: MeterEntryRow) -> Int64 { beatTicks(m) * Int64(max(m.num, 1)) }

    /// Each entry with the number of the bar it starts.
    static func withBars(_ meter: [MeterEntryRow]) -> [(entry: MeterEntryRow, bar: Int64)] {
        let entries = meter.isEmpty ? [fourFour] : meter
        var out: [(entry: MeterEntryRow, bar: Int64)] = []
        for e in entries {
            if let prev = out.last {
                out.append((e, prev.bar + (e.tick - prev.entry.tick) / barTicks(prev.entry)))
            } else {
                out.append((e, 1))
            }
        }
        return out
    }

    public static func floorDiv(_ a: Int64, _ b: Int64) -> Int64 {
        let q = a / b
        return (a % b != 0 && (a < 0) != (b < 0)) ? q - 1 : q
    }

    /// The bar containing `tick`.
    public static func bar(at tick: Int64, _ meter: [MeterEntryRow]) -> Bar {
        let bars = withBars(meter)
        let (e, first) = bars.last(where: { $0.entry.tick <= tick }) ?? bars[0]
        let length = barTicks(e)
        let k = floorDiv(tick - e.tick, length)
        return Bar(number: first + k, start: e.tick + k * length, length: length, meter: e)
    }

    /// The first tick of bar `number`.
    public static func barStart(_ number: Int64, _ meter: [MeterEntryRow]) -> Int64 {
        let bars = withBars(meter)
        let (e, first) = bars.last(where: { $0.bar <= number }) ?? bars[0]
        return e.tick + (number - first) * barTicks(e)
    }
}
