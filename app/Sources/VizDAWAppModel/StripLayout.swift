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

    /// The fader cap's height: the fader draws `throwLength + faderCap`
    /// tall (`ControlMetrics.faderCap`; this module does not import
    /// `SymbioticControls`).
    public static let faderCap: CGFloat = 52
    /// The level box's hit frame (`ControlMetrics.hitSize(.numberBox)`).
    public static let levelBoxHeight: CGFloat = 24
    /// The M S R I row: 24-point toggles (`ControlMetrics.hitSize(.toggle)`).
    public static let toggleRowHeight: CGFloat = 24
    /// The automation mode menu.
    public static let modeMenuHeight: CGFloat = 22
    /// The gap between the strip's sections.
    public static let gap: CGFloat = 6
    /// The strip's padding above and below.
    public static let padding: CGFloat = 10

    /// Everything but the part above the fader and the throw, measured
    /// once: the fader cap (52), the level box (24), the M S R I row (24)
    /// and the automation menu (22), four gaps (24) and the padding above
    /// and below (20). B6 review: the cap and the level box's 24-point hit
    /// frame count, so a short pane scrolls rather than overflows.
    public static let lowerFixed: CGFloat = 166

    /// The strip's whole height for an `upper` part and a throw.
    public static func contentHeight(upper: CGFloat, throwLength: CGFloat) -> CGFloat {
        upper + throwLength + lowerFixed
    }

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

/// The rows of a strip's part above the fader, top to bottom.
public enum StripRowKind: Int, CaseIterable, Sendable {
    case header, instrument, input, eq, inserts, sends, output, pan
}

/// One mixer's shared rows (fix round 1): each row is as tall as the
/// tallest strip's, and a strip without it keeps the space empty, so the
/// faders, level boxes and buttons line up across the strips.
public enum StripGrid {
    /// The gap between two rows above the fader.
    public static let rowGap: CGFloat = 5

    /// Each row's height: the most any strip measured; a row no strip has
    /// is left out (it takes no space).
    public static func rowHeights(_ strips: [[StripRowKind: CGFloat]]) -> [StripRowKind: CGFloat] {
        var rows: [StripRowKind: CGFloat] = [:]
        for s in strips {
            for (k, h) in s where h > 0 { rows[k] = max(rows[k] ?? 0, h) }
        }
        return rows
    }

    /// Where each row starts, down from the top of the part above the fader.
    public static func tops(_ rows: [StripRowKind: CGFloat]) -> [StripRowKind: CGFloat] {
        var y: CGFloat = 0
        var tops: [StripRowKind: CGFloat] = [:]
        for k in StripRowKind.allCases {
            guard let h = rows[k] else { continue }
            tops[k] = y
            y += h + rowGap
        }
        return tops
    }

    /// The part above the fader: the rows and the gaps between them.
    public static func upper(_ rows: [StripRowKind: CGFloat]) -> CGFloat {
        let present = StripRowKind.allCases.compactMap { rows[$0] }
        guard !present.isEmpty else { return 0 }
        return present.reduce(0, +) + CGFloat(present.count - 1) * rowGap
    }

    /// The fader section's top, down from the strip's top.
    public static func faderTop(_ rows: [StripRowKind: CGFloat]) -> CGFloat {
        StripLayout.padding + upper(rows) + StripLayout.gap
    }
}
