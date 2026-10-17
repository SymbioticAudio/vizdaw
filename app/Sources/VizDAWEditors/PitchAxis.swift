import CoreGraphics

/// Pitch ↔ y for the piano roll (spec §3.5): pitch 127 on top, one row of
/// `rowHeight` points per pitch; `offset` is the vertical scroll (points
/// from the top of pitch 127). Pure value type; nothing traps.
public struct PitchAxis: Equatable, Sendable {
    public static let minRowHeight: CGFloat = 4
    public static let maxRowHeight: CGFloat = 40
    public static let pitches = 128

    public private(set) var rowHeight: CGFloat
    public var offset: CGFloat {
        didSet { if !offset.isFinite { offset = 0 } }
    }

    public init(rowHeight: CGFloat = 10, offset: CGFloat = 0) {
        self.rowHeight = Self.clampRow(rowHeight)
        self.offset = offset.isFinite ? offset : 0
    }

    static func clampRow(_ h: CGFloat) -> CGFloat {
        guard h.isFinite else { return 10 }
        return min(max(h, minRowHeight), maxRowHeight)
    }

    public var contentHeight: CGFloat { CGFloat(Self.pitches) * rowHeight }

    public func top(_ pitch: UInt8) -> CGFloat {
        CGFloat(127 - Int(min(pitch, 127))) * rowHeight - offset
    }

    public func center(_ pitch: UInt8) -> CGFloat { top(pitch) + rowHeight / 2 }

    /// The unclamped row under `y` (0 is pitch 127's row).
    public func row(atY y: CGFloat) -> Int {
        guard y.isFinite else { return 67 }  // 67 = pitch 60's row (127 - 60)
        let r = ((y + offset) / rowHeight).rounded(.down)
        return Int(min(max(r, -1_000_000), 1_000_000))
    }

    public func pitch(atY y: CGFloat) -> UInt8 {
        UInt8(127 - min(max(row(atY: y), 0), 127))
    }

    /// Whole rows from `a` to `b`, up positive, unclamped.
    public func semitones(fromY a: CGFloat, toY b: CGFloat) -> Int {
        row(atY: a) - row(atY: b)
    }

    /// Another row height, keeping the pitch position under `y`.
    public func withRowHeight(_ h: CGFloat, keepingY y: CGFloat) -> PitchAxis {
        let new = Self.clampRow(h)
        let anchor = (y + offset) / rowHeight
        return PitchAxis(rowHeight: new, offset: anchor * new - y)
    }

    /// `pitch` centred in a view `height` tall, clamped to the content.
    public func centering(_ pitch: UInt8, height: CGFloat) -> PitchAxis {
        let h = height.isFinite ? height : 0
        var a = self
        a.offset = CGFloat(127 - Int(min(pitch, 127))) * rowHeight + rowHeight / 2 - h / 2
        return a.clamped(height: h)
    }

    public func clamped(height: CGFloat) -> PitchAxis {
        let h = height.isFinite ? height : 0
        var a = self
        a.offset = min(max(offset, 0), max(contentHeight - h, 0))
        return a
    }

    public func visible(height: CGFloat) -> ClosedRange<UInt8> {
        let h = height.isFinite ? max(height, 0) : 0
        return pitch(atY: max(h - 0.001, 0))...pitch(atY: 0)
    }
}

/// A 32-bit value ↔ y inside a pane `height` tall: the maximum at the top,
/// `inset` points kept free above and below.
public struct ValueAxis: Equatable, Sendable {
    public var height: CGFloat { didSet { if !height.isFinite { height = 0 } } }
    public var inset: CGFloat { didSet { if !inset.isFinite { inset = 0 } } }

    public init(height: CGFloat, inset: CGFloat = 3) {
        self.height = height.isFinite ? height : 0
        self.inset = inset.isFinite ? inset : 0
    }

    var span: CGFloat { max(height - 2 * inset, 1) }

    public func y(_ v: UInt32) -> CGFloat {
        inset + span * (1 - CGFloat(Double(v) / Double(UInt32.max)))
    }

    public func value(atY y: CGFloat) -> UInt32 {
        guard y.isFinite else { return 0 }
        let f = min(max(1 - Double((y - inset) / span), 0), 1)
        guard f.isFinite else { return 0 }
        return UInt32((f * Double(UInt32.max)).rounded())
    }
}
