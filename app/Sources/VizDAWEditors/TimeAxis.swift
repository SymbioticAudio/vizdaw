import CoreGraphics

/// Tick ↔ x for time-based editors (spec §3.5): `ticksPerPoint` is the
/// zoom, `originTick` the tick at x = 0 of the canvas. Pure value type;
/// every conversion clamps to ±`limit` instead of trapping.
public struct TimeAxis: Equatable, Sendable {
    public static let minTicksPerPoint: Double = 2
    /// One 4/4 bar per point.
    public static let maxTicksPerPoint: Double = 61_440
    /// The ticks the axis reaches (2⁵⁰, about two years at 120 BPM).
    public static let limit: Int64 = 1 << 50

    public private(set) var ticksPerPoint: Double
    public private(set) var originTick: Double

    public init(ticksPerPoint: Double = 384, originTick: Double = 0) {
        self.ticksPerPoint = Self.clampZoom(ticksPerPoint)
        self.originTick = Self.clampOrigin(originTick)
    }

    static func clampZoom(_ v: Double) -> Double {
        guard v.isFinite else { return v > 0 ? maxTicksPerPoint : minTicksPerPoint }
        return min(max(v, minTicksPerPoint), maxTicksPerPoint)
    }

    static func clampOrigin(_ v: Double) -> Double {
        if v.isNaN { return 0 }
        return min(max(v, -Double(limit)), Double(limit))
    }

    /// A tick from a double: NaN is 0; values beyond ±`limit` clamp.
    public static func clampTick(_ d: Double) -> Int64 {
        if d.isNaN { return 0 }
        if d >= Double(limit) { return limit }
        if d <= -Double(limit) { return -limit }
        return Int64(d.rounded(.down))
    }

    public func x(_ tick: Int64) -> CGFloat {
        let t = min(max(tick, -Self.limit), Self.limit)
        return CGFloat((Double(t) - originTick) / ticksPerPoint)
    }

    public func tick(atX x: CGFloat) -> Int64 {
        Self.clampTick(originTick + Double(x) * ticksPerPoint)
    }

    public func width(ofTicks t: Int64) -> CGFloat {
        CGFloat(Double(min(max(t, -Self.limit), Self.limit)) / ticksPerPoint)
    }

    public func visibleRange(width: CGFloat) -> ClosedRange<Int64> {
        tick(atX: 0)...tick(atX: max(width, 0))
    }

    /// Zoomed in by `factor` (> 1 zooms in) keeping the tick under `x`.
    public func zoomed(by factor: Double, aroundX x: CGFloat) -> TimeAxis {
        guard factor.isFinite, factor > 0 else { return self }
        let anchor = originTick + Double(x) * ticksPerPoint
        let tpp = Self.clampZoom(ticksPerPoint / factor)
        return TimeAxis(ticksPerPoint: tpp, originTick: anchor - Double(x) * tpp)
    }

    public func scrolled(byPoints dx: CGFloat) -> TimeAxis {
        TimeAxis(ticksPerPoint: ticksPerPoint, originTick: originTick + Double(dx) * ticksPerPoint)
    }

    public func scrolled(toTick t: Double) -> TimeAxis {
        TimeAxis(ticksPerPoint: ticksPerPoint, originTick: t)
    }
}
