import CoreGraphics
import Foundation
import VizDAWAppModel
import VizDAWCore

/// The parts of the audio editor's canvas a press can grab (plan 7c Ruling 6).
public enum AudioEditorHandle: Equatable, Sendable {
    case gain, fadeIn, fadeOut, start, end

    /// The arrangement's handle type; nil for the trim edges.
    public var regionHandle: RegionHandle? {
        switch self {
        case .gain: .gain
        case .fadeIn: .fadeIn
        case .fadeOut: .fadeOut
        case .start, .end: nil
        }
    }
}

/// A level-scale mark: `y` in the canvas, its label.
public struct LevelMark: Equatable {
    public let y: CGFloat
    public let label: String
}

/// A ruler mark: `x` in the canvas, its label.
public struct RulerMark: Equatable {
    public let x: CGFloat
    public let label: String
}

/// The audio editor's layout, hits and drag targets (plan 7c Rulings 3, 6
/// and 8). Pure. `wave` is the waveform rect; the axis's `x` is relative to
/// its left edge.
public enum AudioEditorGeometry {
    public static let scaleWidth: CGFloat = 44
    public static let rulerHeight: CGFloat = 18
    public static let gainBand: CGFloat = 6
    public static let gainSlack: CGFloat = 4
    public static let cornerSize: CGFloat = 10
    public static let edgeWidth: CGFloat = 8
    public static let slack: CGFloat = 4
    public static let minMarkGap: CGFloat = 12
    public static let amplitudes: [Int] = [1, 2, 4, 8, 16]
    /// The core's column limit for one envelope or overview request.
    public static let maxColumns: UInt32 = 8192

    private static let minus = "\u{2212}"

    /// The canvas minus the ruler at the top and the gutter at the left.
    public static func waveRect(in bounds: CGRect) -> CGRect {
        CGRect(
            x: bounds.minX + scaleWidth, y: bounds.minY + rulerHeight,
            width: max(bounds.width - scaleWidth, 0), height: max(bounds.height - rulerHeight, 0))
    }

    /// The region's rectangle: its media frames on the axis, the wave's full height.
    public static func regionRect(_ r: EventRow, axis: FrameAxis, in wave: CGRect) -> CGRect {
        let x0 = wave.minX + axis.x(Double(r.sourceOffset))
        let x1 = wave.minX + axis.x(Double(r.sourceOffset) + Double(r.frames))
        return CGRect(x: x0, y: wave.minY, width: max(x1 - x0, 0), height: wave.height)
    }

    /// The drawn handles, in hit order: the fade corners (only when the
    /// region is at least `RegionHandles.minWidth` wide), the trim edges
    /// (`min(edgeWidth, width / 2)` wide, the wave's full height) and the
    /// gain band along the top. Not clipped to the wave.
    public static func drawn(_ r: EventRow, axis: FrameAxis, in wave: CGRect) -> [(AudioEditorHandle, CGRect)] {
        let rect = regionRect(r, axis: axis, in: wave)
        guard rect.width.isFinite, rect.minX.isFinite, rect.width > 0 else { return [] }
        var out: [(AudioEditorHandle, CGRect)] = []
        if rect.width >= RegionHandles.minWidth {
            for (h, cx) in [(AudioEditorHandle.fadeIn, RegionHandles.fadeInX(r, in: rect)),
                            (.fadeOut, RegionHandles.fadeOutX(r, in: rect))] {
                out.append((h, CGRect(x: cx - cornerSize / 2, y: rect.minY, width: cornerSize, height: cornerSize)))
            }
        }
        let edge = min(edgeWidth, rect.width / 2)
        out.append((.start, CGRect(x: rect.minX, y: rect.minY, width: edge, height: rect.height)))
        out.append((.end, CGRect(x: rect.maxX - edge, y: rect.minY, width: edge, height: rect.height)))
        out.append((.gain, CGRect(x: rect.minX, y: rect.minY, width: rect.width, height: gainBand)))
        return out
    }

    /// The hit areas, in hit order: each drawn rect plus its slack (corners
    /// all round, edges outward, the band below), clipped to `wave`.
    public static func handles(_ r: EventRow, axis: FrameAxis, in wave: CGRect) -> [(AudioEditorHandle, CGRect)] {
        drawn(r, axis: axis, in: wave).compactMap { h, d in
            var area = d
            switch h {
            case .fadeIn, .fadeOut: area = d.insetBy(dx: -slack, dy: -slack)
            case .start: area = CGRect(x: d.minX - slack, y: d.minY, width: d.width + slack, height: d.height)
            case .end: area = CGRect(x: d.minX, y: d.minY, width: d.width + slack, height: d.height)
            case .gain: area = CGRect(x: d.minX, y: d.minY, width: d.width, height: d.height + gainSlack)
            }
            let clipped = area.intersection(wave)
            return clipped.isNull || clipped.isEmpty ? nil : (h, clipped)
        }
    }

    /// What a press grabs. When both fade corners reach `p` (the fades
    /// meet), the nearer corner wins, and at a tie the side of the press.
    public static func hit(_ p: CGPoint, row: EventRow, axis: FrameAxis, in wave: CGRect) -> AudioEditorHandle? {
        guard let first = handles(row, axis: axis, in: wave).first(where: { $0.1.contains(p) })?.0 else { return nil }
        guard first == .fadeIn else { return first }
        let d = drawn(row, axis: axis, in: wave)
        guard let fi = d.first(where: { $0.0 == .fadeIn })?.1, let fo = d.first(where: { $0.0 == .fadeOut })?.1,
            handles(row, axis: axis, in: wave).contains(where: { $0.0 == .fadeOut && $0.1.contains(p) })
        else { return first }
        let dIn = abs(p.x - fi.midX)
        let dOut = abs(p.x - fo.midX)
        if dIn != dOut { return dIn < dOut ? .fadeIn : .fadeOut }
        return p.x < fi.midX ? .fadeIn : .fadeOut
    }

    /// The gesture's (and undo step's) name.
    public static func label(_ h: AudioEditorHandle) -> String {
        switch h {
        case .gain: "Region Gain"
        case .fadeIn: "Fade In"
        case .fadeOut: "Fade Out"
        case .start: "Trim Start"
        case .end: "Trim End"
        }
    }

    private static let clampLimit: Int64 = 1 << 60

    private static func frames(_ v: UInt64) -> Int64 { Int64(min(v, UInt64(clampLimit))) }

    /// A trim drag's full target (Ruling 6): `front` is held within
    /// `−min(source_offset, startRoom) … frames − 1`, `back` within
    /// `min(0, −(media − offset − frames)) … frames − 1`. Nil: not a trim
    /// handle, a non-finite `dx`, or no change.
    public static func trimCommand(
        _ r: EventRow, _ h: AudioEditorHandle, dx: CGFloat, framesPerPoint: Double, mediaFrames: UInt64, startRoom: UInt64
    ) -> EditCommand? {
        guard dx.isFinite, framesPerPoint.isFinite, framesPerPoint > 0, r.frames > 0 else { return nil }
        let raw = (Double(dx) * framesPerPoint).rounded()
        let delta = Int64(min(max(raw, -1e15), 1e15))
        let len = frames(r.frames)
        switch h {
        case .start:
            let low = -min(frames(r.sourceOffset), frames(startRoom))
            let front = min(max(delta, low), len - 1)
            return front == 0 ? nil : .audio(edit: .trim(event: r.id, frontFrames: front, backFrames: 0))
        case .end:
            let tail = frames(mediaFrames) - frames(r.sourceOffset) - len
            let low = min(0, -tail)
            let back = min(max(-delta, low), len - 1)
            return back == 0 ? nil : .audio(edit: .trim(event: r.id, frontFrames: 0, backFrames: back))
        case .gain, .fadeIn, .fadeOut:
            return nil
        }
    }

    /// A drag's full target, `dx`/`dy` points from the press (nil: no change).
    public static func target(
        _ r: EventRow, _ h: AudioEditorHandle, dx: CGFloat, dy: CGFloat, fine: Bool, axis: FrameAxis, mediaFrames: UInt64,
        startRoom: UInt64, in wave: CGRect
    ) -> EditCommand? {
        switch h {
        case .gain:
            guard dy.isFinite else { return nil }
            return RegionHandles.gainCommand(r, dy: dy, fine: fine)
        case .fadeIn, .fadeOut:
            let rect = regionRect(r, axis: axis, in: wave)
            guard dx.isFinite, rect.width.isFinite, rect.width > 0, let handle = h.regionHandle else { return nil }
            // Beyond twice the region's width the fade is clamped anyway.
            let limit = rect.width * 2
            return RegionHandles.fadeCommand(r, handle, dx: min(max(dx, -limit), limit), in: rect)
        case .start, .end:
            return trimCommand(
                r, h, dx: dx, framesPerPoint: axis.framesPerPoint, mediaFrames: mediaFrames, startRoom: startRoom)
        }
    }

    // MARK: Level scale

    private static let candidates: [(Int, Double)] = [0, -3, -6, -9, -12, -18, -24, -30, -36, -48, -60].map {
        ($0, pow(10, Double($0) / 20))
    }

    /// Each channel's band, top half only (Ruling 8): the dB candidates that
    /// fit under the band's top, at least `minMarkGap` below the last kept
    /// mark and above the centre, then "−∞" at the centre.
    public static func scaleMarks(amplitude: Int, channels: Int, in wave: CGRect) -> [LevelMark] {
        guard channels >= 1, amplitude >= 1, wave.height > 0, wave.height.isFinite else { return [] }
        let band = wave.height / CGFloat(channels)
        var out: [LevelMark] = []
        for k in 0..<channels {
            let top = wave.minY + band * CGFloat(k)
            let centre = top + band / 2
            var last: CGFloat?
            for (db, linear) in candidates {
                let position = linear * Double(amplitude)
                guard position <= 1 + 1e-9 else { continue }
                let y = centre - CGFloat(min(position, 1)) * band / 2
                if let last, y < last + minMarkGap { continue }
                guard centre - y >= minMarkGap else { continue }
                last = y
                out.append(LevelMark(y: y, label: db == 0 ? "0" : "\(minus)\(-db)"))
            }
            out.append(LevelMark(y: centre, label: "\(minus)\u{221E}"))
        }
        return out
    }

    // MARK: Ruler

    private static let rulerSteps: [Double] = [
        0.001, 0.002, 0.005, 0.01, 0.02, 0.05, 0.1, 0.2, 0.5, 1, 2, 5, 10, 15, 30, 60, 120, 300, 600,
    ]

    /// Marks at the smallest step at least 80 points apart, from the file's start.
    public static func rulerMarks(axis: FrameAxis, width: CGFloat, sampleRate: UInt32) -> [RulerMark] {
        guard sampleRate > 0, axis.framesPerPoint.isFinite, axis.framesPerPoint > 0, axis.originFrame.isFinite,
            width.isFinite, width > 0
        else { return [] }
        let rate = Double(sampleRate)
        let pointsPerSecond = rate / axis.framesPerPoint
        let step = rulerSteps.first { $0 * pointsPerSecond >= 80 } ?? rulerSteps[rulerSteps.count - 1]
        let first = max(0, (axis.originFrame / rate / step).rounded(.up))
        guard first < 1e12 else { return [] }
        var out: [RulerMark] = []
        var k = first
        while out.count < 1000 {
            let seconds = k * step
            let x = axis.x(seconds * rate)
            if x > width { break }
            out.append(RulerMark(x: x, label: Formatting.time(seconds)))
            k += 1
        }
        return out
    }

    // MARK: Visible span

    /// One column per point, from 1 to the core's limit.
    public static func columns(width: CGFloat) -> UInt32 {
        guard width.isFinite, width > 0 else { return 1 }
        return UInt32(min(width.rounded(.up), Double(maxColumns)))
    }

    /// The visible in-file frames, with their x offset and width in the wave.
    public struct FrameSpan: Equatable {
        public let from: UInt64
        public let to: UInt64
        public let x: CGFloat
        public let width: CGFloat
        public let columns: UInt32
    }

    /// The in-file part of what the axis shows, clamped to `[0, mediaFrames]`
    /// as `Double`s before any conversion. Nil when empty or not finite.
    public static func span(axis: FrameAxis, width: CGFloat, mediaFrames: UInt64) -> FrameSpan? {
        let fpp = axis.framesPerPoint
        let origin = axis.originFrame
        guard fpp.isFinite, fpp > 0, origin.isFinite, width.isFinite, width > 0, mediaFrames > 0 else { return nil }
        let end = origin + Double(width) * fpp
        guard end.isFinite else { return nil }
        let media = Double(mediaFrames)
        let lo = min(max(origin, 0), media).rounded(.down)
        let hi = min(max(end, 0), media).rounded(.up)
        guard hi > lo else { return nil }
        let w = CGFloat((hi - lo) / fpp)
        return FrameSpan(
            from: UInt64(lo), to: UInt64(min(hi, media)), x: CGFloat((lo - origin) / fpp), width: w, columns: columns(width: w))
    }

    // MARK: Waveform

    /// The rows scaled by `amplitude` and, per column, by the envelope. An
    /// envelope that is not one value per column gives silent rows of the
    /// same shape; a non-finite value counts as silence.
    public static func shaped(_ rows: OverviewRows, envelope: [Float]?, amplitude: Int) -> OverviewRows {
        let n = Int(rows.columns)
        let ch = Int(rows.channels)
        var out = rows
        if let envelope {
            guard envelope.count == n, rows.min.count >= n * ch, rows.max.count >= n * ch else {
                out.min = [Float](repeating: 0, count: n * ch)
                out.max = [Float](repeating: 0, count: n * ch)
                return out
            }
        }
        let a = Float(max(amplitude, 1))
        func scale(_ values: [Float]) -> [Float] {
            values.enumerated().map { i, v in
                var e: Float = 1
                if let envelope, n > 0 { e = envelope[i % n].isFinite ? envelope[i % n] : 0 }
                return v * e * a
            }
        }
        out.min = scale(rows.min)
        out.max = scale(rows.max)
        return out
    }

    /// The columns where `|sample| × envelope` goes over 0 dBFS in any
    /// channel, or with `channel`, in that one only (none past the last).
    public static func clipColumns(_ rows: OverviewRows, envelope: [Float], channel: Int? = nil) -> [Int] {
        let n = Int(rows.columns)
        let ch = Int(rows.channels)
        guard n > 0, ch > 0, envelope.count == n, rows.min.count >= n * ch, rows.max.count >= n * ch else { return [] }
        let channels = channel.map { (0..<ch).contains($0) ? $0..<($0 + 1) : 0..<0 } ?? 0..<ch
        return (0..<n).filter { c in
            let e = envelope[c]
            guard e.isFinite else { return false }
            return channels.contains { k in
                let lo = rows.min[k * n + c]
                let hi = rows.max[k * n + c]
                return (lo.isFinite && abs(lo) * e > 1) || (hi.isFinite && abs(hi) * e > 1)
            }
        }
    }

    /// The envelope's line on `channel`'s band: one point per column, at
    /// `envelope × amplitude` up from the centre, held within the band.
    public static func envelopeLine(_ envelope: [Float], amplitude: Int, channel: Int, channels: Int, in wave: CGRect)
        -> [CGPoint]
    {
        guard !envelope.isEmpty, channels >= 1, (0..<channels).contains(channel), wave.width > 0, wave.height > 0 else {
            return []
        }
        let band = wave.height / CGFloat(channels)
        let centre = wave.minY + band * (CGFloat(channel) + 0.5)
        let w = wave.width / CGFloat(envelope.count)
        let a = Float(max(amplitude, 1))
        return envelope.enumerated().map { c, e in
            let level = e.isFinite ? CGFloat(min(max(e * a, 0), 1)) : 0
            return CGPoint(x: wave.minX + (CGFloat(c) + 0.5) * w, y: centre - level * band / 2)
        }
    }
}
