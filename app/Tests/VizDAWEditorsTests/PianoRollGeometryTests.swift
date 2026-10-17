import CoreGraphics
import Testing
import VizDAWAppModel
import VizDAWCore
@testable import VizDAWEditors

private let q: Int64 = 15_360
private let meter = [MeterEntryRow(tick: 0, num: 4, den: 4, sharps: 0, minor: false)]
private let laneA = LaneKey(3, 31)
private let laneB = LaneKey(3, 30)
/// 96 ticks per point: a quarter is 160 pt. Rows of 10 pt, pitch 127 at y 0.
private let time = TimeAxis(ticksPerPoint: 96)
private let pitch = PitchAxis(rowHeight: 10)

private func note(_ id: UInt64, _ tick: Int64, _ p: UInt8, _ length: Int64 = q) -> EventRow {
    var r = Pencil.note(tick: tick, pitch: p, length: length)
    r.id = id
    r.velocity = 0x8000
    return r
}

private func y(_ p: UInt8) -> CGFloat { pitch.center(p) }

@Suite struct PianoRollGeometryTests {
    @Test func rectsHaveAMinimumWidth() {
        #expect(PianoRollGeometry.rect(note(1, q, 60), time: time, pitch: pitch) == CGRect(x: 160, y: 670, width: 160, height: 10))
        #expect(PianoRollGeometry.rect(note(1, 0, 60, 0), time: time, pitch: pitch).width == 3)
    }

    @Test func zonesOnWideAndNarrowNotes() {
        let wide = CGRect(x: 0, y: 0, width: 160, height: 10)
        #expect(PianoRollGeometry.zone(atX: 7, in: wide) == .start)
        #expect(PianoRollGeometry.zone(atX: 8, in: wide) == .middle)
        #expect(PianoRollGeometry.zone(atX: 151, in: wide) == .middle)
        #expect(PianoRollGeometry.zone(atX: 152, in: wide) == .end)
        let narrow = CGRect(x: 0, y: 0, width: 12, height: 10)
        #expect(PianoRollGeometry.zone(atX: 3, in: narrow) == .start)
        #expect(PianoRollGeometry.zone(atX: 6, in: narrow) == .middle)
        #expect(PianoRollGeometry.zone(atX: 9, in: narrow) == .end)
    }

    @Test func theActiveLaneWinsAndTheLatestNoteWins() {
        let lanes = [
            LaneNotes(lane: laneB, notes: [note(7, 0, 60, 4 * q)]),
            LaneNotes(lane: laneA, notes: [note(1, 0, 60, 2 * q), note(2, q, 60)]),
        ]
        let p = CGPoint(x: 200, y: y(60))
        guard case .note(let h) = PianoRollGeometry.hit(p, lanes: lanes, active: laneA, time: time, pitch: pitch)
        else { Issue.record("no hit"); return }
        #expect(h.lane == laneA && h.row.id == 2 && h.zone == .middle)
        guard case .note(let hb) = PianoRollGeometry.hit(p, lanes: lanes, active: laneB, time: time, pitch: pitch)
        else { Issue.record("no hit"); return }
        #expect(hb.row.id == 7)
        #expect(
            PianoRollGeometry.hit(CGPoint(x: 200, y: y(61)), lanes: lanes, active: laneA, time: time, pitch: pitch)
                == .empty(tick: 19_200, pitch: 61))
    }

    @Test func rubberBandFindsNotesOfEveryLane() {
        let lanes = [
            LaneNotes(lane: laneB, notes: [note(7, 0, 64)]),
            LaneNotes(lane: laneA, notes: [note(1, 0, 60), note(2, 4 * q, 60)]),
        ]
        let r = RubberBand.rect(from: CGPoint(x: 100, y: y(65)), to: CGPoint(x: 10, y: y(59)))
        let found = PianoRollGeometry.notes(in: r, lanes: lanes, time: time, pitch: pitch)
        #expect(found.map { $0.id } == [7, 1])
        #expect(found.map { $0.lane } == [laneB, laneA])
    }

    @Test func theDraggedSetIsTheSelectionWhenThePressedNoteIsSelected() {
        let lanes = [
            LaneNotes(lane: laneB, notes: [note(7, 0, 64)]),
            LaneNotes(lane: laneA, notes: [note(1, 0, 60), note(2, q, 62)]),
        ]
        #expect(NoteDrag.events(pressed: 1, selected: [1, 7], lanes: lanes) == [1, 7])
        #expect(NoteDrag.events(pressed: 2, selected: [1, 7], lanes: lanes) == [2])
        #expect(NoteDrag.events(pressed: 1, selected: [1, 99], lanes: lanes) == [1], "hidden lanes are not dragged")
    }

    func drag(_ zone: NoteZone, copy: Bool = false, lowest: UInt8 = 60, highest: UInt8 = 64) -> NoteDrag {
        NoteDrag(
            zone: zone, pressed: note(1, q, 60), lane: laneA, events: [1, 2], lowest: lowest, highest: highest,
            copy: copy)
    }

    @Test func startZoneMovesInTimeSnappingThePressedStart() {
        var snap = Snap()
        snap.division = .quarter
        let d = drag(.start)
        #expect(d.label == "Move Notes")
        #expect(d.command(rawTicks: 9_000, dy: -40, rowHeight: 10, snap: snap, meter: meter)
            == .event(edit: .move(events: [1, 2], delta: q, toLane: nil)))
        #expect(d.command(rawTicks: 100, dy: 0, rowHeight: 10, snap: snap, meter: meter) == nil)
        #expect(drag(.start, copy: true).command(rawTicks: -q, dy: 0, rowHeight: 10, snap: snap, meter: meter)
            == .event(edit: .duplicate(events: [1, 2], ticks: -q, semitones: 0)))
        #expect(drag(.start, copy: true).label == "Copy Notes")
    }

    @Test func middleZoneTransposes() {
        let snap = Snap()
        #expect(drag(.middle).label == "Transpose Notes")
        #expect(drag(.middle).command(rawTicks: 50_000, dy: -21, rowHeight: 10, snap: snap, meter: meter)
            == .event(edit: .nudge(events: [1, 2], ticks: 0, semitones: 2)))
        #expect(drag(.middle, copy: true).command(rawTicks: 0, dy: 30, rowHeight: 10, snap: snap, meter: meter)
            == .event(edit: .duplicate(events: [1, 2], ticks: 0, semitones: -3)))
        #expect(drag(.middle).soundingPitch(dy: -21, rowHeight: 10) == 62)
        #expect(drag(.start).soundingPitch(dy: -21, rowHeight: 10) == 60)
    }

    @Test func transposeClampsTheGroupAtTheEdges() {
        let d = drag(.middle, lowest: 60, highest: 125)
        #expect(d.semitones(raw: 10) == 2)
        #expect(d.semitones(raw: -100) == -60)
        #expect(d.command(rawTicks: 0, dy: -1_000, rowHeight: 10, snap: Snap(), meter: meter)
            == .event(edit: .nudge(events: [1, 2], ticks: 0, semitones: 2)))
        let top = drag(.middle, lowest: 127, highest: 127)
        #expect(top.command(rawTicks: 0, dy: -50, rowHeight: 10, snap: Snap(), meter: meter) == nil)
    }

    @Test func endZoneResizesSnappingTheEndAndKeepsOneTick() {
        var snap = Snap()
        snap.division = .sixteenth
        let d = drag(.end, copy: true)
        #expect(d.label == "Resize Notes", "⌥ is ignored at the end")
        #expect(d.command(rawTicks: 3_000, dy: 0, rowHeight: 10, snap: snap, meter: meter)
            == .event(edit: .resize(events: [1, 2], delta: 3_840)))
        #expect(d.durationDelta(rawTicks: -10 * q, snap: snap, meter: meter) == 1 - q)
        snap.enabled = false
        #expect(d.durationDelta(rawTicks: 7, snap: snap, meter: meter) == 7)
    }

    @Test func pencilStartsOnTheGridAndPaintsALength() {
        var snap = Snap()
        snap.division = .eighth
        #expect(Pencil.start(10_000, snap: snap, meter: meter) == 7_680)
        #expect(Pencil.step(at: 0, snap: snap, meter: meter) == 7_680)
        #expect(Pencil.length(start: 7_680, to: 7_000, snap: snap, meter: meter) == 7_680, "at least one step")
        #expect(Pencil.length(start: 7_680, to: 30_000, snap: snap, meter: meter) == 23_040)
        snap.enabled = false
        #expect(Pencil.step(at: 0, snap: snap, meter: meter) == q)
        #expect(Pencil.length(start: 0, to: 100, snap: snap, meter: meter) == q / 8)
        #expect(Pencil.length(start: 0, to: 5_000, snap: snap, meter: meter) == 5_000)
        let row = Pencil.note(tick: 7_680, pitch: 64, length: 7_680)
        #expect(row.kind == .note && row.velocity == 51_200 && row.duration == 7_680 && row.pitch == 64)
        #expect(Pencil.command(lane: laneA, tick: 7_680, pitch: 64, length: 7_680)
            == .event(edit: .insert(seq: 3, lane: 31, events: [row])))
    }

    @Test func aSegmentFindsEveryNoteItCrossesInOrder() {
        func n(_ id: UInt64, _ tick: Int64, _ p: UInt8) -> EventRow { note(id, tick, p, 960) }
        // Three notes on one row, 10 pt wide, 20 pt apart; one row above.
        let lanes = [
            LaneNotes(lane: LaneKey(1, 1), notes: [n(1, 0, 60), n(2, 1_920, 60), n(3, 3_840, 60), n(4, 1_920, 61)])
        ]
        let y = pitch.center(60)
        // Sampled points 0 and 45 only touch 1; the segment crosses 1, 2, 3.
        #expect(
            PianoRollGeometry.notes(
                crossingFrom: CGPoint(x: 5, y: y), to: CGPoint(x: 45, y: y), lanes: lanes, time: time, pitch: pitch)
                == [1, 2, 3])
        // Right to left: the order the pointer meets them.
        #expect(
            PianoRollGeometry.notes(
                crossingFrom: CGPoint(x: 45, y: y), to: CGPoint(x: 5, y: y), lanes: lanes, time: time, pitch: pitch)
                == [3, 2, 1])
        // A miss, and a single point.
        let above = pitch.center(70)
        #expect(
            PianoRollGeometry.notes(
                crossingFrom: CGPoint(x: 0, y: above), to: CGPoint(x: 60, y: above), lanes: lanes, time: time,
                pitch: pitch).isEmpty)
        #expect(
            PianoRollGeometry.notes(
                crossingFrom: CGPoint(x: 25, y: y), to: CGPoint(x: 25, y: y), lanes: lanes, time: time, pitch: pitch)
                == [2])
        #expect(Eraser.adding([2, 1, 3], to: [1]) == [1, 2, 3])
    }

    @Test func theEraserCollectsEachNoteOnce() {
        let hit = PianoRollHit.note(NoteHit(lane: laneA, row: note(2, 0, 60), zone: .middle))
        let once = Eraser.adding(hit, to: [1])
        #expect(once == [1, 2])
        #expect(Eraser.adding(hit, to: once) == [1, 2])
        #expect(Eraser.adding(.empty(tick: 0, pitch: 1), to: once) == [1, 2])
        #expect(Eraser.command([]) == nil)
        #expect(Eraser.command([1, 2]) == .event(edit: .delete(events: [1, 2])))
    }

    @Test func cursorsFollowZonesAndTools() {
        let n = { (z: NoteZone) in PianoRollHit.note(NoteHit(lane: laneA, row: note(1, 0, 60), zone: z)) }
        #expect(PianoRollCursor.kind(for: n(.start), tool: .arrow) == .openHand)
        #expect(PianoRollCursor.kind(for: n(.middle), tool: .pencil) == .upDown)
        #expect(PianoRollCursor.kind(for: n(.end), tool: .arrow) == .resizeRight)
        #expect(PianoRollCursor.kind(for: n(.end), tool: .eraser) == .eraser)
        #expect(PianoRollCursor.kind(for: .empty(tick: 0, pitch: 0), tool: .pencil) == .crosshair)
        #expect(PianoRollCursor.kind(for: .empty(tick: 0, pitch: 0), tool: .arrow) == .arrow)
    }

    @Test func extremeInputsClampInsteadOfTrapping() {
        let snap = Snap()
        let big = Int64.max
        let d = drag(.end)
        _ = d.durationDelta(rawTicks: big, snap: snap, meter: meter)
        _ = d.durationDelta(rawTicks: .min, snap: snap, meter: meter)
        _ = NoteDrag.timeDelta(rawTicks: big, start: big, snap: snap, meter: meter)
        _ = NoteDrag.timeDelta(rawTicks: .min, start: .min, snap: snap, meter: meter)
        _ = Pencil.length(start: .min, to: big, snap: snap, meter: meter)
        #expect(NoteDrag.rawSemitones(dy: .nan, rowHeight: 10) == 0)
        #expect(NoteDrag.rawSemitones(dy: .infinity, rowHeight: 10) == 0)
        #expect(NoteDrag.rawSemitones(dy: -1e300, rowHeight: 10) == 1_000)
        let bad = drag(.middle, lowest: 255, highest: 255)
        #expect(abs(bad.semitones(raw: -1_000)) <= 127)
        _ = bad.command(rawTicks: 0, dy: 1e300, rowHeight: 10, snap: snap, meter: meter)
        let nan = CGFloat.nan
        let lanes = [LaneNotes(lane: laneA, notes: [note(1, 0, 60)])]
        _ = PianoRollGeometry.hit(CGPoint(x: nan, y: nan), lanes: lanes, active: laneA, time: time, pitch: pitch)
        _ = PianoRollGeometry.rect(note(1, .max, 255, .max), time: time, pitch: pitch)
        _ = PianoRollGeometry.notes(in: RubberBand.rect(from: CGPoint(x: nan, y: 0), to: .zero), lanes: lanes, time: time, pitch: pitch)
    }

    @Test func accessibilityLabels() {
        var n = note(1, 2 * 4 * q, 60)
        n.velocity = 100 << 9
        #expect(PianoRollA11y.label(n, laneName: nil, meter: meter) == "Note C4, bar 3 beat 1, velocity 100")
        n.tick += q + 240
        n.mute = true
        #expect(PianoRollA11y.label(n, laneName: "Chords", meter: meter)
            == "Note C4, bar 3 beat 2 tick 240, velocity 100, Chords, muted")
    }
}
