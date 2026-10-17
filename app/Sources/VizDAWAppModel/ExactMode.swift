import Foundation
import VizDAWCore

/// The panel opened by exact mode or a menu command, with what it acts on.
public struct ModifyRequest: Equatable, Sendable {
    public var task: PanelTask
    public var config: SelectModifyConfig
    /// The dragged events (nil: keep the selection's events).
    public var events: [UInt64]?
    public var lanes: [LaneKey]
    public var range: TickRange?

    public init(task: PanelTask, config: SelectModifyConfig, events: [UInt64]?, lanes: [LaneKey], range: TickRange?) {
        self.task = task
        self.config = config
        self.events = events
        self.lanes = lanes
        self.range = range
    }
}

/// Exact mode (parent spec §9, Plan 4c Ruling 23): a drag's final command
/// as the panel pre-filled with its amounts.
public enum ExactMode {
    /// The region strip's drag label; its moves apply as usual (Ruling 23).
    public static let regionMoveLabel = "Move Region"

    /// The panel for a drag that ended with `cmd`, begun as gesture `label`;
    /// nil when that drag applies as usual (region moves, pencil, eraser,
    /// cross-lane drags).
    public static func request(for cmd: EditCommand, lanes: Set<LaneKey>, label: String = "") -> ModifyRequest? {
        guard label != regionMoveLabel, case .event(let e) = cmd else { return nil }
        let sorted = lanes.sorted { ($0.seq, $0.lane) < ($1.seq, $1.lane) }
        func make(_ task: PanelTask, _ events: [UInt64], _ op: ModifyRow, copy: Bool) -> ModifyRequest {
            var c = SelectModifyConfig.preset(task)
            c.modify.op = op
            c.modify.toCopy = copy
            return ModifyRequest(task: task, config: c, events: events, lanes: sorted, range: nil)
        }
        switch e {
        case .move(let events, let delta, nil):
            return make(.move, events, .move(delta: delta), copy: false)
        case let .duplicate(events, ticks, semitones):
            if semitones == 0 { return make(.move, events, .move(delta: ticks), copy: true) }
            if ticks == 0 { return make(.transpose, events, .transposeInterval(semitones: semitones), copy: true) }
            return nil
        case let .nudge(events, ticks, semitones):
            if ticks == 0 { return make(.transpose, events, .transposeInterval(semitones: semitones), copy: false) }
            if semitones == 0 { return make(.move, events, .move(delta: ticks), copy: false) }
            return nil
        case let .resize(events, delta):
            return make(
                .change, events, .change(target: .duration, op: .add, amount: .constant(value: delta)), copy: false)
        default:
            return nil
        }
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
