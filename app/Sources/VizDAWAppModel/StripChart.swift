import Foundation
import VizDAWCore

/// Line shapes (Plan 4d Ruling 17).
public enum ShapeKind: String, Codable, CaseIterable, Sendable {
    case straight, curved, flat, free, random, oscillator

    public var title: String {
        switch self {
        case .straight: "Straight"
        case .curved: "Curved"
        case .flat: "Flat"
        case .free: "Free"
        case .random: "Random"
        case .oscillator: "Oscillator"
        }
    }
}

/// Curves of a curved line (the panel's curves).
public enum CurveKind: String, Codable, CaseIterable, Sendable {
    case easeIn, easeOut, sCurve

    public var title: String {
        switch self {
        case .easeIn: "Ease In"
        case .easeOut: "Ease Out"
        case .sCurve: "S-Curve"
        }
    }

    public var row: CurveRow {
        switch self {
        case .easeIn: .easeIn
        case .easeOut: .easeOut
        case .sCurve: .sCurve
        }
    }
}

/// Oscillator waves.
public enum WaveKind: String, Codable, CaseIterable, Sendable {
    case sine, square, triangle, saw

    public var title: String {
        switch self {
        case .sine: "Sine"
        case .square: "Square"
        case .triangle: "Triangle"
        case .saw: "Saw"
        }
    }

    public var row: WaveRow {
        switch self {
        case .sine: .sine
        case .square: .square
        case .triangle: .triangle
        case .saw: .saw
        }
    }
}

/// How an oscillator's period is measured (Plan 4d Ruling 17).
public enum PeriodKind: String, Codable, CaseIterable, Sendable {
    case musical, clock, bars

    public var title: String {
        switch self {
        case .musical: "Musical"
        case .clock: "Clock"
        case .bars: "Bars (follow meter)"
        }
    }
}

/// The Strip Chart bar's settings, shared by every Strip Chart and saved
/// with the project (`EditorLayout.stripChart`). Decoding is tolerant.
public struct StripChartSettings: Codable, Equatable, Sendable {
    public var mode: StripMode = .draw
    public var op: StripOp = .set
    public var shape: ShapeKind = .straight
    public var curve: CurveKind = .easeOut
    public var wave: WaveKind = .sine
    public var period: PeriodKind = .musical
    public var periodTicks: Int64 = MeterMath.ppq
    public var periodMs: UInt32 = 500
    public var periodBars: UInt32 = 1
    /// Thousandths of a period (0–999).
    public var phase: UInt16 = 0
    /// Sample spacing of drawn lines, and the spacing of Thin and Thicken.
    public var step: Int64 = MeterMath.ppq / 8
    public var rowHeight: Double = 96
    /// The Strip Chart's own zoom (a 4/4 bar is 640 pt at 96).
    public var ticksPerPoint: Double = 96

    public static let rowHeightRange: ClosedRange<Double> = 40...400

    public init() {}

    enum CodingKeys: String, CodingKey {
        case mode, op, shape, curve, wave, period, periodTicks, periodMs, periodBars, phase, step, rowHeight
        case ticksPerPoint
    }

    public init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        let d = StripChartSettings()
        mode = c.field(.mode, d.mode)
        op = c.field(.op, d.op)
        shape = c.field(.shape, d.shape)
        curve = c.field(.curve, d.curve)
        wave = c.field(.wave, d.wave)
        period = c.field(.period, d.period)
        periodTicks = max(c.field(.periodTicks, d.periodTicks), 1)
        periodMs = max(c.field(.periodMs, d.periodMs), 1)
        periodBars = max(c.field(.periodBars, d.periodBars), 1)
        phase = min(c.field(.phase, d.phase), 999)
        step = max(c.field(.step, d.step), 1)
        rowHeight = EditorLayout.sane(c.field(.rowHeight, d.rowHeight), Self.rowHeightRange, d.rowHeight)
        ticksPerPoint = EditorLayout.sane(
            c.field(.ticksPerPoint, d.ticksPerPoint), EditorLayout.ticksPerPointRange, d.ticksPerPoint)
    }

    public var periodRow: PeriodRow {
        switch period {
        case .musical: .ticks(ticks: max(periodTicks, 1))
        case .clock: .millis(ms: max(periodMs, 1))
        case .bars: .bars(bars: max(periodBars, 1))
        }
    }

    /// The shape of a line drawn with these settings (a free line's points
    /// are added by `StripLineBuilder`).
    public func shapeRow(seed: UInt64) -> LineShapeRow {
        switch shape {
        case .straight: .straight
        case .curved: .curved(curve: curve.row)
        case .flat: .flat
        case .free: .free(points: [])
        case .random: .random(seed: seed)
        case .oscillator: .oscillator(wave: wave.row, period: periodRow, phase: min(phase, 999))
        }
    }
}

/// A pointer position in a row: a tick and a value on the row's scale.
public struct StripPoint: Equatable, Sendable {
    public var tick: Int64
    public var value: Int64

    public init(tick: Int64, value: Int64) {
        self.tick = tick
        self.value = value
    }
}

/// Drags to lines (Plan 4d Ruling 17).
public enum StripLineBuilder {
    /// A free line's points: ascending ticks, the later point winning on a
    /// tick.
    public static func freePoints(_ stroke: [StripPoint]) -> [LinePointRow] {
        var byTick: [Int64: Int64] = [:]
        for p in stroke { byTick[p.tick] = p.value }
        return byTick.keys.sorted().compactMap { t in byTick[t].map { LinePointRow(tick: t, value: $0) } }
    }

    /// The line a drag from `a` to `b` draws: its ends in time order (a
    /// right-to-left drag is turned around), Flat holding `a`'s value, a
    /// Free line through `stroke`.
    public static func line(
        from a: StripPoint, to b: StripPoint, stroke: [StripPoint], settings s: StripChartSettings, seed: UInt64
    ) -> LineRow {
        let step = max(s.step, 1)
        if s.shape == .free {
            let pts = freePoints(stroke.isEmpty ? [a, b] : stroke)
            if let f = pts.first, let l = pts.last {
                return LineRow(from: f.tick, to: l.tick, start: f.value, end: l.value, step: step, shape: .free(points: pts))
            }
        }
        let (lo, hi) = a.tick <= b.tick ? (a, b) : (b, a)
        if s.shape == .flat {
            return LineRow(from: lo.tick, to: hi.tick, start: a.value, end: a.value, step: step, shape: .flat)
        }
        return LineRow(from: lo.tick, to: hi.tick, start: lo.value, end: hi.value, step: step, shape: s.shapeRow(seed: seed))
    }
}

/// Strip Chart gestures as editor commands (Plan 4d Rulings 15, 22).
public enum StripCommands {
    /// The operation of a Draw or Modify line: Draw writes a series and
    /// sets a note value; Modify uses the bar's operation.
    public static func op(_ lane: StripLane, settings: StripChartSettings) -> LineOpRow {
        guard settings.mode == .modify else { return lane.isNoteValue ? .set : .draw }
        return settings.op.row
    }

    /// A line over `[from, to]` of the selected lanes; `selectedOnly`
    /// limits it to the selected events.
    public static func line(_ lane: StripLane, op: LineOpRow, line: LineRow, selectedOnly: Bool) -> EditCommand {
        .selectAndModify(
            rule: RuleRow(
                conditions: [ConditionRow(test: .timeRange(start: line.from, end: line.to + 1), negate: false)],
                matchAll: true),
            mode: selectedOnly ? .refine : .select,
            modify: .line(target: lane.lineTarget, op: op, line: line), toCopy: false)
    }

    /// A Compress rectangle: the values in its time range onto its bottom
    /// (`start`) and top (`end`).
    public static func compress(
        _ lane: StripLane, from a: StripPoint, to b: StripPoint, step: Int64, selectedOnly: Bool
    ) -> EditCommand {
        let rect = LineRow(
            from: min(a.tick, b.tick), to: max(a.tick, b.tick), start: min(a.value, b.value),
            end: max(a.value, b.value), step: max(step, 1), shape: .flat)
        return line(lane, op: .compress, line: rect, selectedOnly: selectedOnly)
    }

    /// The eraser over `[from, to]`: the row's series goes; nil for note
    /// values and tempo.
    public static func erase(_ lane: StripLane, from: Int64, to: Int64) -> EditCommand? {
        lane.eraseRule(from: from, to: to).map { .selectAndModify(rule: $0, mode: .select, modify: .delete, toCopy: false) }
    }

    /// The undo step's name (the core's).
    public static func label(_ lane: StripLane, op: LineOpRow) -> String {
        ModifyKind.lineTitle(lane.lineTarget, op)
    }
}
