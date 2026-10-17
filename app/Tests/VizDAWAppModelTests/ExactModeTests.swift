import Testing
import VizDAWCore
@testable import VizDAWAppModel

@Suite struct ExactModeTests {
    @Test func aMoveOpensMoveOnTheDraggedEvents() throws {
        let r = try #require(
            ExactMode.request(for: .event(edit: .move(events: [1, 2], delta: 100, toLane: nil)), lanes: [chords]))
        #expect(r.task == .move && r.config.modify.op == .move(delta: 100) && !r.config.modify.toCopy)
        #expect(r.events == [1, 2] && r.lanes == [chords] && !r.config.select.enabled)
    }

    @Test func copiesAndTransposes() throws {
        let copy = try #require(
            ExactMode.request(for: .event(edit: .duplicate(events: [1], ticks: 50, semitones: 0)), lanes: [chords]))
        #expect(copy.config.modify.op == .move(delta: 50) && copy.config.modify.toCopy)
        let harmony = try #require(
            ExactMode.request(for: .event(edit: .duplicate(events: [1], ticks: 0, semitones: 3)), lanes: [chords]))
        #expect(harmony.task == .transpose && harmony.config.modify.op == .transposeInterval(semitones: 3))
        #expect(harmony.config.modify.toCopy)
        #expect(ExactMode.request(for: .event(edit: .duplicate(events: [1], ticks: 5, semitones: 3)), lanes: []) == nil)
        let up = try #require(
            ExactMode.request(for: .event(edit: .nudge(events: [1], ticks: 0, semitones: -2)), lanes: [chords]))
        #expect(up.task == .transpose && up.config.modify.op == .transposeInterval(semitones: -2))
        let longer = try #require(ExactMode.request(for: .event(edit: .resize(events: [1], delta: 960)), lanes: [chords]))
        #expect(longer.task == .change)
        #expect(longer.config.modify.op == .change(target: .duration, op: .add, amount: .constant(value: 960)))
    }

    @Test func otherDragsApplyAsUsual() {
        let to = LaneRef(seq: 3, lane: 32)
        #expect(ExactMode.request(for: .event(edit: .move(events: [1], delta: 5, toLane: to)), lanes: [chords]) == nil)
        #expect(ExactMode.request(for: .event(edit: .delete(events: [1])), lanes: [chords]) == nil)
        #expect(ExactMode.request(for: .event(edit: .insert(seq: 3, lane: 31, events: [])), lanes: [chords]) == nil)
        #expect(ExactMode.request(for: .lane(edit: .rename(seq: 3, lane: 31, name: "x")), lanes: [chords]) == nil)
    }

    @Test func regionMovesApplyAsUsual() {
        let move = EditCommand.event(edit: .move(events: [1], delta: 100, toLane: nil))
        #expect(ExactMode.request(for: move, lanes: [chords], label: ExactMode.regionMoveLabel) == nil)
        #expect(ExactMode.request(for: move, lanes: [chords], label: "Move Notes") != nil)
    }

    @Test func aRubberBandBecomesASelectRule() {
        let r = ExactMode.selectRequest(
            range: TickRange(start: 0, end: 15_360), pitches: 60...64, lanes: [chords], adding: true)
        #expect(r.task == .selectByRule && r.events == nil && r.range == TickRange(start: 0, end: 15_360))
        #expect(r.config.select.enabled && r.config.select.mode == .add && !r.config.modify.enabled)
        #expect(
            r.config.select.conditions == [
                ConditionRow(test: .kinds(kinds: [.note]), negate: false),
                ConditionRow(test: .timeRange(start: 0, end: 15_360), negate: false),
                ConditionRow(test: .pitch(low: 60, high: 64), negate: false),
            ])
    }
}
