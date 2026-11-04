import CoreGraphics
import Foundation

/// One mark of the fader's dB scale: where it sits (0 bottom … 1 top) and
/// its label.
public struct ScaleMark: Equatable, Sendable {
    public var position: Double
    public var label: String

    public init(position: Double, label: String) {
        self.position = position
        self.label = label
    }
}

/// How the strip's height divides: the fader's throw, and whether the
/// strip scrolls because even the shortest throw does not fit.
public struct StripSplit: Equatable {
    public var throwLength: CGFloat
    public var scrolls: Bool

    public init(throwLength: CGFloat, scrolls: Bool) {
        self.throwLength = throwLength
        self.scrolls = scrolls
    }
}

/// The channel strip's measurements (plan 7b Ruling 14).
public enum StripLayout {
    public static let width: CGFloat = 120
    public static let minimumThrow: CGFloat = 160
    public static let maximumThrow: CGFloat = 480

    /// Everything below the fader, measured once: the level box (22), the
    /// M S R I row (24) and the automation menu (22), four 6-point gaps (24)
    /// and 10 points of padding above and below (20).
    public static let lowerFixed: CGFloat = 112

    /// The fader's throw for a strip `height` tall whose part above the
    /// fader is `upper` tall: what is left, from 160 to 480; the strip
    /// scrolls when even 160 does not fit.
    public static func split(height: CGFloat, upper: CGFloat) -> StripSplit {
        let room = height - upper - lowerFixed
        let length = min(maximumThrow, max(minimumThrow, room))
        return StripSplit(throwLength: length, scrolls: height < upper + lowerFixed + minimumThrow)
    }

    /// The fader's dB scale. "−∞" stands for −60 and below: the fader's
    /// position at −60 is already 0, so a separate −60 mark would sit on it.
    public static let faderMarks: [ScaleMark] = {
        let marks: [(Double, String)] = [
            (12, "+12"), (6, "+6"), (0, "0"), (-6, "\u{2212}6"), (-12, "\u{2212}12"),
            (-24, "\u{2212}24"), (-36, "\u{2212}36"), (-48, "\u{2212}48"),
        ]
        return marks.map { ScaleMark(position: MixerMath.faderPosition($0.0), label: $0.1) }
            + [ScaleMark(position: 0, label: "\u{2212}\u{221E}")]
    }()
}
