import Foundation
import VizDAWCore

/// Which gesture a drag came from; Ruling 23 lets only note and block
/// drags open the panel.
public enum GestureOrigin: Equatable, Sendable {
    case notes, blocks, regionStrip, expression, other
    /// A Strip Chart line or rectangle (4d).
    case stripChart
}

/// The part of a diagonal drag the panel left out.
public enum DroppedAxis: Equatable, Sendable {
    case vertical, horizontal

    public var message: String {
        switch self {
        case .vertical: "The vertical part of the drag was left out."
        case .horizontal: "The horizontal part of the drag was left out."
        }
    }
}

/// The panel opened by exact mode or a menu command, with what it acts on.
public struct ModifyRequest: Equatable, Sendable {
    public var task: PanelTask
    public var config: SelectModifyConfig
    /// The dragged events (nil: keep the selection's events).
    public var events: [UInt64]?
    public var lanes: [LaneKey]
    public var range: TickRange?
    /// Set when a diagonal drag was reduced to its stronger axis.
    public var droppedAxis: DroppedAxis?

    public init(
        task: PanelTask, config: SelectModifyConfig, events: [UInt64]?, lanes: [LaneKey], range: TickRange?,
        droppedAxis: DroppedAxis? = nil
    ) {
        self.task = task
        self.config = config
        self.events = events
        self.lanes = lanes
        self.range = range
        self.droppedAxis = droppedAxis
    }
}

/// Exact mode (parent spec §9, Plan 4c Ruling 23): a drag's final command
/// as the panel pre-filled with its amounts.
public enum ExactMode {
    /// The panel for a drag from `origin` that ended with `cmd`; nil when
    /// that drag applies as usual (region moves, pencil, eraser, expression
    /// strokes, cross-lane drags).
    public static func request(for cmd: EditCommand, lanes: Set<LaneKey>, origin: GestureOrigin) -> ModifyRequest? {
        let sorted = lanes.sorted { ($0.seq, $0.lane) < ($1.seq, $1.lane) }
        if origin == .stripChart { return lineRequest(cmd, lanes: sorted) }
        guard origin == .notes || origin == .blocks, case .event(let e) = cmd else { return nil }
        func make(
            _ task: PanelTask, _ events: [UInt64], _ op: ModifyRow, copy: Bool, dropped: DroppedAxis? = nil
        ) -> ModifyRequest {
            var c = SelectModifyConfig.preset(task)
            c.modify.op = op
            c.modify.toCopy = copy
            return ModifyRequest(task: task, config: c, events: events, lanes: sorted, range: nil, droppedAxis: dropped)
        }
        /// A drag in both axes keeps the stronger one: sixteenths moved
        /// against semitones, time winning ties.
        func shifted(_ events: [UInt64], _ ticks: Int64, _ semitones: Int8, copy: Bool) -> ModifyRequest? {
            let timeWins =
                semitones == 0
                || (ticks != 0 && Double(abs(ticks)) / Double(MeterMath.ppq / 4) >= Double(abs(Int(semitones))))
            if timeWins {
                return make(
                    .move, events, .move(delta: ticks), copy: copy, dropped: semitones == 0 ? nil : .vertical)
            }
            return make(
                .transpose, events, .transposeInterval(semitones: semitones), copy: copy,
                dropped: ticks == 0 ? nil : .horizontal)
        }
        switch e {
        case .move(let events, let delta, nil):
            return make(.move, events, .move(delta: delta), copy: false)
        case let .duplicate(events, ticks, semitones):
            return shifted(events, ticks, semitones, copy: true)
        case let .nudge(events, ticks, semitones):
            return shifted(events, ticks, semitones, copy: false)
        case let .resize(events, delta):
            return make(
                .change, events, .change(target: .duration, op: .add, amount: .constant(value: delta)), copy: false)
        default:
            return nil
        }
    }

    /// A Strip Chart line or rectangle as the panel set to Change Values
    /// with the line and its Position condition (Plan 4d Ruling 23).
    static func lineRequest(_ cmd: EditCommand, lanes: [LaneKey]) -> ModifyRequest? {
        guard case let .selectAndModify(rule, mode, .line(target, op, line)?, _) = cmd else { return nil }
        var c = SelectModifyConfig.preset(.change)
        c.select.enabled = rule != nil
        c.select.mode = mode
        c.select.conditions = rule?.conditions ?? []
        c.modify.enabled = true
        c.modify.op = .line(target: target, op: op, line: line)
        c.modify.toCopy = false
        return ModifyRequest(
            task: .change, config: c, events: nil, lanes: lanes, range: TickRange(start: line.from, end: line.to + 1))
    }

    /// A rubber band as Select by Rule: notes starting in its range and
    /// pitches, in the shown lanes.
    public static func selectRequest(
        range: TickRange, pitches: ClosedRange<UInt8>, lanes: [LaneKey], adding: Bool
    ) -> ModifyRequest {
        var c = SelectModifyConfig.preset(.selectByRule)
        c.select.mode = adding ? .add : .select
        c.select.conditions = [
            ConditionRow(test: .kinds(kinds: [.note]), negate: false),
            ConditionRow(test: .timeRange(start: range.start, end: range.end), negate: false),
            ConditionRow(test: .pitch(low: pitches.lowerBound, high: pitches.upperBound), negate: false),
        ]
        return ModifyRequest(task: .selectByRule, config: c, events: nil, lanes: lanes, range: range)
    }
}
