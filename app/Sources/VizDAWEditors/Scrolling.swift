import CoreGraphics
import VizDAWAppModel
import VizDAWCore

/// Scrolling while a drag passes an edge (Plan Ruling 18).
public enum AutoScroll {
    /// Distance from an edge where scrolling starts.
    public static let edge: CGFloat = 24
    /// Points per second for each point of overshoot.
    public static let rate: CGFloat = 20
    public static let maxSpeed: CGFloat = 1_200

    /// Horizontal scroll speed in points per second (negative: towards
    /// earlier time) for a pointer at `x` in a view `width` wide.
    public static func speed(x: CGFloat, width: CGFloat) -> CGFloat {
        if x < edge { return -min(maxSpeed, (edge - x) * rate) }
        if x > width - edge { return min(maxSpeed, (x - (width - edge)) * rate) }
        return 0
    }
}

/// Follow playback in page mode (Plan Ruling 18).
public enum PageFollow {
    public static let margin: CGFloat = 16

    /// The new origin tick when the playhead at `tick` is outside the
    /// visible width, else nil.
    public static func origin(forPlayhead tick: Double, axis: TimeAxis, width: CGFloat) -> Double? {
        let x = (tick - axis.originTick) / axis.ticksPerPoint
        guard x < 0 || x > Double(width) else { return nil }
        return tick - Double(margin) * axis.ticksPerPoint
    }
}

/// A ruler drag's time range (spec §3.5, parent §9).
public enum RulerSelection {
    /// The snapped range between the press (`anchor`, a tick, so it
    /// survives auto-scroll) and the pointer at `x`; nil when empty.
    public static func range(
        fromTick anchor: Int64, toX x: CGFloat, axis: TimeAxis, snap: Snap, meter: [MeterEntryRow]
    ) -> TickRange? {
        let other = axis.tick(atX: x)
        let a = snap.apply(min(anchor, other), meter: meter)
        let b = snap.apply(max(anchor, other), meter: meter)
        return b > a ? TickRange(start: a, end: b) : nil
    }
}
