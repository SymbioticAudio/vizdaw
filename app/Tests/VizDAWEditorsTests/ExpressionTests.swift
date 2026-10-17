import CoreGraphics
import Testing
import VizDAWAppModel
import VizDAWCore
@testable import VizDAWEditors

private let time = TimeAxis(ticksPerPoint: 96)
private let values = ValueAxis(height: 96, inset: 3)

private func note(_ id: UInt64, _ tick: Int64, _ length: Int64, _ points: [PerNotePoint] = []) -> EventRow {
    var r = Pencil.note(tick: tick, pitch: 60, length: length)
    r.id = id
    r.perNote = points
    return r
}

@Suite struct ExpressionTests {
    @Test func stepsHoldEachValueUntilTheNext() {
        let pb = PerNoteKindRow.pitchBend
        let n = note(1, 1_000, 100, [
            PerNotePoint(kind: pb, offset: 0, value: 5),
            PerNotePoint(kind: .registered(index: 1), offset: 10, value: 99),
            PerNotePoint(kind: pb, offset: 50, value: 7),
        ])
        let s = ExpressionGeometry.steps(n, kind: pb, neutral: 0x8000_0000)
        #expect(s.map { $0.from } == [1_000, 1_050])
        #expect(s.map { $0.to } == [1_050, 1_100])
        #expect(s.map { $0.value } == [5, 7])
        let empty = ExpressionGeometry.steps(note(2, 0, 100), kind: pb, neutral: 0x8000_0000)
        #expect(empty.map { $0.value } == [0x8000_0000])
        let late = ExpressionGeometry.steps(note(3, 0, 100, [PerNotePoint(kind: pb, offset: 40, value: 1)]), kind: pb, neutral: 9)
        #expect(late.map { $0.value } == [9, 1], "neutral before the first point")
    }

    @Test func targetsPreferTheSelectedNotes() {
        let notes = [note(1, 0, 1_000), note(2, 0, 1_000), note(3, 5_000, 1_000)]
        #expect(ExpressionGeometry.targets(notes, from: 100, to: 200, selected: []) == [1, 2])
        #expect(ExpressionGeometry.targets(notes, from: 100, to: 200, selected: [2, 3]) == [2])
        #expect(ExpressionGeometry.targets(notes, from: 100, to: 6_000, selected: [3]) == [3])
        #expect(ExpressionGeometry.targets(notes, from: 2_000, to: 3_000, selected: []) == [])
    }

    @Test func strokesAreThinnedToOnePointPerSpacing() {
        // One point per 4 pt = 384 ticks; later stroke points win on one tick.
        let stroke = [
            CGPoint(x: 0, y: 93), CGPoint(x: 1, y: 3), CGPoint(x: 4, y: 48), CGPoint(x: 4, y: 3), CGPoint(x: 10, y: 3),
        ]
        let pts = ExpressionGeometry.points(stroke, time: time, values: values)
        #expect(pts.map(\.tick) == [0, 384, 960])
        #expect(pts[0].value == 0)
        #expect(pts[1].value == UInt32.max)
    }

    @Test func aStrokeIsOneSetPerNoteOverItsRange() {
        let notes = [note(1, 0, 3_000)]
        let stroke = [CGPoint(x: 0, y: 3), CGPoint(x: 10, y: 93)]
        let cmd = ExpressionGeometry.command(
            stroke: stroke, erase: false, kind: .pitchBend, notes: notes, selected: [], time: time, values: values)
        #expect(cmd == .event(edit: .setPerNote(
            events: [1], kind: .pitchBend, from: 0, to: 960,
            points: [PerNoteValueRow(tick: 0, value: UInt32.max), PerNoteValueRow(tick: 960, value: 0)])))
        let erase = ExpressionGeometry.command(
            stroke: stroke, erase: true, kind: .pitchBend, notes: notes, selected: [], time: time, values: values)
        #expect(erase == .event(edit: .setPerNote(events: [1], kind: .pitchBend, from: 0, to: 960, points: [])))
        #expect(ExpressionGeometry.command(
            stroke: [CGPoint(x: 500, y: 3)], erase: false, kind: .pitchBend, notes: notes, selected: [], time: time,
            values: values) == nil, "no note under the stroke")
        #expect(ExpressionGeometry.command(
            stroke: [], erase: false, kind: .pitchBend, notes: notes, selected: [], time: time, values: values) == nil)
    }

    @Test func shortTitles() {
        #expect(ExpressionGeometry.shortTitle(.pitchBend) == "Bend")
        #expect(ExpressionGeometry.shortTitle(.registered(74)) == "Reg 74")
        #expect(ExpressionGeometry.shortTitle(.assignable(2)) == "Asg 2")
    }
}
