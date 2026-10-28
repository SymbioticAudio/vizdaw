import CoreGraphics
import Foundation
import VizDAWAppModel
import VizDAWCore

/// The parts of an audio region a press can grab (spec §10.2).
public enum RegionHandle: Equatable, Sendable {
    case fadeIn, fadeOut, gain
}

/// A handle under the pointer: its region (lane and row) and the region's
/// rectangle when pressed.
public struct RegionHandleHit: Equatable, Sendable {
    public let lane: LaneKey
    public let row: EventRow
    public let handle: RegionHandle
    public let rect: CGRect

    public init(lane: LaneKey, row: EventRow, handle: RegionHandle, rect: CGRect) {
        self.lane = lane
        self.row = row
        self.handle = handle
        self.rect = rect
    }
}

/// Fade and gain handles (plan 5b Ruling 7). Pure: where the handles are,
/// what a press hits, and the full target of a drag.
public enum RegionHandles {
    /// A corner handle's side.
    public static let size: CGFloat = 7
    /// The gain band's height along the top edge.
    public static let band: CGFloat = 4
    /// Narrower regions show no handles.
    public static let minWidth: CGFloat = 24
    public static let dbPerPoint: Float = 0.25
    /// With ⇧.
    public static let fineDbPerPoint: Float = 0.05
    /// The fade shapes, as the menu names them.
    public static let shapes: [(FadeShapeRow, String)] = [
        (.linear, "Linear"), (.equalPower, "Equal Power"), (.exponential, "Exponential"), (.sCurve, "S-Curve"),
    ]

    /// The gesture's (and undo step's) name.
    public static func label(_ h: RegionHandle) -> String {
        switch h {
        case .fadeIn: "Fade In"
        case .fadeOut: "Fade Out"
        case .gain: "Region Gain"
        }
    }

    static func framesPerPoint(_ r: EventRow, _ rect: CGRect) -> Double {
        Double(r.frames) / Double(max(rect.width, 1))
    }

    static func share(_ frames: Int64, _ r: EventRow) -> CGFloat {
        CGFloat(Double(max(frames, 0)) / Double(max(r.frames, 1)))
    }

    /// Where the fade in ends.
    public static func fadeInX(_ r: EventRow, in rect: CGRect) -> CGFloat {
        rect.minX + min(share(r.fadeIn.length, r), 1) * rect.width
    }

    /// Where the fade out starts.
    public static func fadeOutX(_ r: EventRow, in rect: CGRect) -> CGFloat {
        rect.maxX - min(share(r.fadeOut.length, r), 1) * rect.width
    }

    /// The handles of `r` in `rect` (none when it is narrower than
    /// `minWidth`).
    public static func rects(_ r: EventRow, in rect: CGRect) -> [(RegionHandle, CGRect)] {
        guard rect.width >= minWidth, rect.height >= size else { return [] }
        return [
            (.fadeIn, CGRect(x: fadeInX(r, in: rect) - size / 2, y: rect.minY, width: size, height: size)),
            (.fadeOut, CGRect(x: fadeOutX(r, in: rect) - size / 2, y: rect.minY, width: size, height: size)),
            (.gain, CGRect(x: rect.minX, y: rect.minY, width: rect.width, height: band)),
        ]
    }

    /// What a press at `p` grabs: a corner first, then the gain band (each
    /// with 2 points of slack).
    public static func hit(_ p: CGPoint, row r: EventRow, in rect: CGRect) -> RegionHandle? {
        for (h, box) in rects(r, in: rect) where box.insetBy(dx: -2, dy: -2).contains(p) { return h }
        return nil
    }

    /// The fades after dragging `handle` by `dx` points, clamped so both
    /// fit the region.
    public static func fades(_ r: EventRow, _ handle: RegionHandle, dx: CGFloat, in rect: CGRect) -> (
        fadeIn: Int64, fadeOut: Int64
    ) {
        let len = Int64(clamping: r.frames)
        let delta = Int64((Double(dx) * framesPerPoint(r, rect)).rounded())
        let fi = max(r.fadeIn.length, 0)
        let fo = max(r.fadeOut.length, 0)
        switch handle {
        case .fadeIn: return (min(max(fi + delta, 0), max(len - fo, 0)), fo)
        case .fadeOut: return (fi, min(max(fo - delta, 0), max(len - fi, 0)))
        case .gain: return (fi, fo)
        }
    }

    /// A fade drag's full target (nil: no change).
    public static func fadeCommand(_ r: EventRow, _ handle: RegionHandle, dx: CGFloat, in rect: CGRect) -> EditCommand? {
        let (fi, fo) = fades(r, handle, dx: dx, in: rect)
        guard fi != r.fadeIn.length || fo != r.fadeOut.length else { return nil }
        return .audio(
            edit: .setFades(
                event: r.id, fadeIn: FadeRow(length: fi, shape: r.fadeIn.shape),
                fadeOut: FadeRow(length: fo, shape: r.fadeOut.shape)))
    }

    /// The gain after dragging the band `dy` points (up is louder), in
    /// tenths of a dB within −144…+24.
    public static func gain(_ r: EventRow, dy: CGFloat, fine: Bool) -> Float {
        let g = r.gainDb - Float(dy) * (fine ? fineDbPerPoint : dbPerPoint)
        return min(max((g * 10).rounded() / 10, -144), 24)
    }

    /// A gain drag's full target (nil: no change).
    public static func gainCommand(_ r: EventRow, dy: CGFloat, fine: Bool) -> EditCommand? {
        let g = gain(r, dy: dy, fine: fine)
        return g == r.gainDb ? nil : .audio(edit: .setGain(event: r.id, gainDb: g))
    }

    /// A handle drag's full target, `dx`/`dy` points from the press (the
    /// gain follows `dy`, a fade `dx`; nil: no change). Preflight M9: one
    /// target for both editors.
    public static func target(
        _ r: EventRow, _ handle: RegionHandle, rect: CGRect, dx: CGFloat, dy: CGFloat, fine: Bool
    ) -> EditCommand? {
        switch handle {
        case .gain: gainCommand(r, dy: dy, fine: fine)
        case .fadeIn, .fadeOut: fadeCommand(r, handle, dx: dx, in: rect)
        }
    }

    /// ⌥-click on the band: 0 dB (nil when it is already).
    public static func resetGain(_ r: EventRow) -> EditCommand? {
        r.gainDb == 0 ? nil : .audio(edit: .setGain(event: r.id, gainDb: 0))
    }

    /// A fade's new shape (nil: the same shape, or not a fade).
    public static func shapeCommand(_ r: EventRow, _ handle: RegionHandle, _ shape: FadeShapeRow) -> EditCommand? {
        switch handle {
        case .fadeIn where r.fadeIn.shape != shape:
            .audio(edit: .setFades(event: r.id, fadeIn: FadeRow(length: r.fadeIn.length, shape: shape), fadeOut: r.fadeOut))
        case .fadeOut where r.fadeOut.shape != shape:
            .audio(edit: .setFades(event: r.id, fadeIn: r.fadeIn, fadeOut: FadeRow(length: r.fadeOut.length, shape: shape)))
        default:
            nil
        }
    }

    /// Which fade's area `p` is in (between the region's edge and the
    /// fade's handle, for a fade with a length): the shape menu's target.
    public static func fadeArea(_ p: CGPoint, row r: EventRow, in rect: CGRect) -> RegionHandle? {
        guard rect.contains(p) else { return nil }
        if r.fadeIn.length > 0, p.x <= fadeInX(r, in: rect) { return .fadeIn }
        if r.fadeOut.length > 0, p.x >= fadeOutX(r, in: rect) { return .fadeOut }
        return nil
    }

    /// "+3.0 dB", "0.0 dB", "-6.5 dB" (Ruling 18).
    public static func gainLabel(_ db: Float) -> String {
        abs(db) < 0.05 ? "0.0 dB" : String(format: "%+.1f dB", db)
    }
}

/// Waveform bars (spec §12.3 "the overview drawing geometry").
public enum WaveformGeometry {
    /// One bar per column and channel in `rect`: channels stacked top to
    /// bottom, each bar from the column's max to its min around its
    /// channel's centre line, clipped to that channel's band; silence is a
    /// hairline; a non-finite value counts as silence. Rows whose arrays do
    /// not hold `channels × columns` values draw nothing.
    public static func bars(_ rows: OverviewRows, in rect: CGRect) -> [CGRect] {
        let n = Int(rows.columns)
        let ch = Int(rows.channels)
        guard n > 0, ch > 0, rows.min.count >= n * ch, rows.max.count >= n * ch, rect.width > 0, rect.height > 0
        else { return [] }
        let w = rect.width / CGFloat(n)
        let h = rect.height / CGFloat(ch)
        let clip = { (v: Float) -> CGFloat in v.isFinite ? CGFloat(Swift.max(-1, Swift.min(1, v))) : 0 }
        var out: [CGRect] = []
        out.reserveCapacity(n * ch)
        for k in 0..<ch {
            let mid = rect.minY + h * (CGFloat(k) + 0.5)
            for c in 0..<n {
                let lo = clip(rows.min[k * n + c])
                let hi = clip(rows.max[k * n + c])
                let top = mid - Swift.max(hi, lo) * h / 2
                let bottom = mid - Swift.min(hi, lo) * h / 2
                out.append(
                    CGRect(x: rect.minX + CGFloat(c) * w, y: top, width: Swift.max(w, 0.5), height: Swift.max(bottom - top, 0.5)))
            }
        }
        return out
    }
}

/// VoiceOver labels for regions (spec §3.5).
public enum RegionA11y {
    public static func label(_ r: EventRow, name: String, laneName: String, meter: [MeterEntryRow], offline: Bool) -> String {
        let end = r.tick > Int64.max - max(r.duration, 1) ? Int64.max : r.tick + max(r.duration, 1) - 1
        let first = MeterMath.bar(at: r.tick, meter).number
        let last = MeterMath.bar(at: max(end, r.tick), meter).number
        let bars = first == last ? "bar \(first)" : "bars \(first) to \(last)"
        var s = "Region \(name) on \(laneName), \(bars), gain \(RegionHandles.gainLabel(r.gainDb))"
        if r.fadeIn.length > 0 || r.fadeOut.length > 0 { s += ", fades" }
        if offline { s += ", offline" }
        if r.mute { s += ", muted" }
        return s
    }
}
