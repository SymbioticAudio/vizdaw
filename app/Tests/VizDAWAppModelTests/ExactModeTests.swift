import Testing
import VizDAWCore
@testable import VizDAWAppModel

@Suite struct ExactModeTests {
    @Test func aMoveOpensMoveOnTheDraggedEvents() throws {
        let r = try #require(
            ExactMode.request(for: .event(edit: .move(events: [1, 2], delta: 100, toLane: nil)), lanes: [chords], origin: .notes))
        #expect(r.task == .move && r.config.modify.op == .move(delta: 100) && !r.config.modify.toCopy)
        #expect(r.events == [1, 2] && r.lanes == [chords] && !r.config.select.enabled)
    }

    @Test func copiesAndTransposes() throws {
        let copy = try #require(
            ExactMode.request(for: .event(edit: .duplicate(events: [1], ticks: 50, semitones: 0)), lanes: [chords], origin: .notes))
        #expect(copy.config.modify.op == .move(delta: 50) && copy.config.modify.toCopy)
        let harmony = try #require(
            ExactMode.request(for: .event(edit: .duplicate(events: [1], ticks: 0, semitones: 3)), lanes: [chords], origin: .notes))
        #expect(harmony.task == .transpose && harmony.config.modify.op == .transposeInterval(semitones: 3))
        #expect(harmony.config.modify.toCopy)
        let up = try #require(
            ExactMode.request(for: .event(edit: .nudge(events: [1], ticks: 0, semitones: -2)), lanes: [chords], origin: .notes))
        #expect(up.task == .transpose && up.config.modify.op == .transposeInterval(semitones: -2))
        let longer = try #require(ExactMode.request(for: .event(edit: .resize(events: [1], delta: 960)), lanes: [chords], origin: .notes))
        #expect(longer.task == .change)
        #expect(longer.config.modify.op == .change(target: .duration, op: .add, amount: .constant(value: 960)))
    }

    @Test func otherDragsApplyAsUsual() {
        let to = LaneRef(seq: 3, lane: 32)
        #expect(ExactMode.request(for: .event(edit: .move(events: [1], delta: 5, toLane: to)), lanes: [chords], origin: .notes) == nil)
        #expect(ExactMode.request(for: .event(edit: .delete(events: [1])), lanes: [chords], origin: .notes) == nil)
        #expect(ExactMode.request(for: .event(edit: .insert(seq: 3, lane: 31, events: [])), lanes: [chords], origin: .notes) == nil)
        #expect(ExactMode.request(for: .lane(edit: .rename(seq: 3, lane: 31, name: "x")), lanes: [chords], origin: .notes) == nil)
    }

    @Test func regionMovesApplyAsUsual() {
        let move = EditCommand.event(edit: .move(events: [1], delta: 100, toLane: nil))
        for o in [GestureOrigin.regionStrip, .expression, .other] {
            #expect(ExactMode.request(for: move, lanes: [chords], origin: o) == nil, "\(o)")
        }
        #expect(ExactMode.request(for: move, lanes: [chords], origin: .notes) != nil)
        #expect(ExactMode.request(for: move, lanes: [chords], origin: .blocks) != nil)
    }

    @Test func diagonalDragsKeepTheirStrongerAxis() throws {
        let q16 = Int64(15_360 / 4)
        let side = try #require(
            ExactMode.request(
                for: .event(edit: .nudge(events: [1], ticks: 4 * q16, semitones: 3)), lanes: [chords], origin: .notes))
        #expect(side.task == .move && side.config.modify.op == .move(delta: 4 * q16))
        #expect(side.droppedAxis == .vertical)
        let up = try #require(
            ExactMode.request(
                for: .event(edit: .duplicate(events: [1], ticks: 5, semitones: 3)), lanes: [chords], origin: .notes))
        #expect(up.task == .transpose && up.config.modify.op == .transposeInterval(semitones: 3))
        #expect(up.config.modify.toCopy && up.droppedAxis == .horizontal)
        #expect(up.droppedAxis?.message == "The horizontal part of the drag was left out.")
        let tie = try #require(
            ExactMode.request(
                for: .event(edit: .nudge(events: [1], ticks: 3 * q16, semitones: -3)), lanes: [chords], origin: .notes))
        #expect(tie.task == .move, "time wins ties")
        let pure = try #require(
            ExactMode.request(
                for: .event(edit: .nudge(events: [1], ticks: 0, semitones: 2)), lanes: [chords], origin: .notes))
        #expect(pure.droppedAxis == nil)
    }

    @Test func thePanelCarriesTheDragsExactAmounts() throws {
        let resize = try #require(
            ExactMode.request(for: .event(edit: .resize(events: [7], delta: -1_234)), lanes: [chords], origin: .notes))
        #expect(resize.config.modify.op == .change(target: .duration, op: .add, amount: .constant(value: -1_234)))
        #expect(resize.config.modify.op.numbers == [-1_234] && resize.events == [7])
        let nudge = try #require(
            ExactMode.request(for: .event(edit: .nudge(events: [7], ticks: 777, semitones: 0)), lanes: [chords], origin: .notes))
        #expect(nudge.config.modify.op.numbers == [777] && nudge.config.modify.op == .move(delta: 777))
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
