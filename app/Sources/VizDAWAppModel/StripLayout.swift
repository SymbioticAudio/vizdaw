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

    /// The fader cap's drawn height: the fader draws `throwLength + faderCap`
    /// tall (`ControlMetrics.faderCap.height`, 42 since symbiotic-design
    /// 0.2.1; this module does not import `SymbioticControls`, so a test
    /// pins the two together). The slot reserves the drawn height; the cap's
    /// 52-point hit frame overhangs it by 5 points at the ends of travel,
    /// which the 6-point `gap` absorbs (final review m1).
    public static let faderCap: CGFloat = 42
    /// The level box's hit frame (`ControlMetrics.hitSize(.numberBox)`).
    public static let levelBoxHeight: CGFloat = 24
    /// The M S R I row: 24-point toggles (`ControlMetrics.hitSize(.toggle)`).
    public static let toggleRowHeight: CGFloat = 24
    /// The automation mode menu, at the 24-point hit minimum (review M2).
    public static let modeMenuHeight: CGFloat = 24
    /// The gap between the strip's sections.
    public static let gap: CGFloat = 6
    /// The strip's padding above and below.
    public static let padding: CGFloat = 10
    /// The strip's padding left and right.
    public static let sidePadding: CGFloat = 6

    /// Everything but the part above the fader and the throw, measured
    /// once: the fader cap (42), the level box (24), the M S R I row (24)
    /// and the automation menu (24), four gaps (24) and the padding above
    /// and below (20). B6 review: the cap and the level box's 24-point hit
    /// frame count, so a short pane scrolls rather than overflows.
    public static let lowerFixed: CGFloat = 158

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
    case header, instrument, input, eq, inserts, midiFx, sends, output, pan
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

    /// What a grid child is: a strip's panel, the divider, a row above the
    /// fader, or one of the parts below it.
    public enum Part: Equatable, Sendable {
        case panel, divider, fader, level, buttons, mode
        case row(StripRowKind)
    }

    /// One grid child: its column, its part and, for a row, its measured
    /// height.
    public struct Cell: Equatable, Sendable {
        public var column: Int
        public var part: Part
        public var measured: CGFloat

        public init(column: Int, part: Part, measured: CGFloat = 0) {
            self.column = column
            self.part = part
            self.measured = measured
        }
    }

    /// The gap between two strips.
    public static let spacing: CGFloat = 6

    /// Column `column`'s left edge: one strip and gap per column, and the
    /// divider's own slot (a point and a gap) from `dividerBefore` on.
    public static func x(_ column: Int, dividerBefore: Int?) -> CGFloat {
        let divider: CGFloat = dividerBefore.map { column >= $0 ? 1 + spacing : 0 } ?? 0
        return CGFloat(column) * (StripLayout.width + spacing) + divider
    }

    /// Every cell's frame (from the grid's top left) and the grid's size,
    /// for strips sharing their rows in a placement `height` tall: what the
    /// mixer's layout places (review M1).
    public static func place(_ cells: [Cell], height: CGFloat, dividerBefore: Int?) -> (frames: [CGRect], size: CGSize) {
        var measured: [Int: [StripRowKind: CGFloat]] = [:]
        var columns = 0
        for c in cells {
            columns = max(columns, c.column + 1)
            if case .row(let k) = c.part { measured[c.column, default: [:]][k] = c.measured }
        }
        let rows = rowHeights(Array(measured.values))
        let top = tops(rows)
        let up = upper(rows)
        let throwLength = StripLayout.split(height: height, upper: up).throwLength
        let total = StripLayout.contentHeight(upper: up, throwLength: throwLength)
        let inner = StripLayout.width - 2 * StripLayout.sidePadding
        let fader = faderTop(rows)
        let faderHeight = throwLength + StripLayout.faderCap
        let level = fader + faderHeight + StripLayout.gap
        let buttons = level + StripLayout.levelBoxHeight + StripLayout.gap
        let mode = buttons + StripLayout.toggleRowHeight + StripLayout.gap
        let frames = cells.map { c -> CGRect in
            let left = x(c.column, dividerBefore: dividerBefore)
            let cx = left + StripLayout.sidePadding
            switch c.part {
            case .panel: return CGRect(x: left, y: 0, width: StripLayout.width, height: total)
            case .divider:
                // The middle of the gap before `dividerBefore`.
                return CGRect(x: x(dividerBefore ?? 0, dividerBefore: dividerBefore) - spacing - 1, y: 0, width: 1, height: total)
            case .row(let k): return CGRect(x: cx, y: StripLayout.padding + (top[k] ?? 0), width: inner, height: rows[k] ?? 0)
            case .fader: return CGRect(x: cx, y: fader, width: inner, height: faderHeight)
            case .level: return CGRect(x: cx, y: level, width: inner, height: StripLayout.levelBoxHeight)
            case .buttons: return CGRect(x: cx, y: buttons, width: inner, height: StripLayout.toggleRowHeight)
            case .mode: return CGRect(x: cx, y: mode, width: inner, height: StripLayout.modeMenuHeight)
            }
        }
        let width = columns > 0 ? x(columns - 1, dividerBefore: dividerBefore) + StripLayout.width : 0
        return (frames, CGSize(width: width, height: total))
    }
}
