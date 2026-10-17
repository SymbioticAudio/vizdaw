import AppKit
import Testing
import VizDAWCore
@testable import VizDAWAppModel
@testable import VizDAWEditors

/// Chords (31) of `rollFake()` in the detail pane: notes 1 (C4 at 0) and 2
/// (E4 at one quarter), 160 pt per quarter at the default zoom.
@MainActor
private func roll() throws -> (FakeSession, DocumentModel, PianoRollView) {
    let f = rollFake()
    let m = try DocumentModel(session: f)
    m.editor.openDetail(.lane(seq: 3, lane: 31), kind: .pianoRoll)
    let v = PianoRollView(model: PianoRollModel.pane(m))
    v.frame = NSRect(x: 0, y: 0, width: 800, height: 400)
    v.layout()
    return (f, m, v)
}

/// A point inside a zone of note `id`, in content coordinates.
@MainActor
private func point(_ v: PianoRollView, _ id: UInt64, _ zone: NoteZone) -> CGPoint {
    let n = v.laneNotes().flatMap(\.notes).first { $0.id == id }!
    let r = PianoRollGeometry.rect(n, time: v.axis, pitch: v.pitchAxis)
    switch zone {
    case .start: return CGPoint(x: r.minX + 2, y: r.midY)
    case .middle: return CGPoint(x: r.midX, y: r.midY)
    case .end: return CGPoint(x: r.maxX - 2, y: r.midY)
    }
}

private func log(_ f: FakeSession) -> [String] {
    f.auditions.map { "\($0.on ? "on" : "off") \($0.pitch)" }
}

@MainActor
@Suite struct PianoRollViewTests {
    @Test func pressingANoteSelectsAndSoundsItUntilRelease() throws {
        let (f, m, v) = try roll()
        let p = point(v, 1, .middle)
        v.contentMouseDown(p, clicks: 1, modifiers: [])
        #expect(log(f) == ["on 60"])
        v.contentMouseUp(p, modifiers: [])
        #expect(log(f) == ["on 60", "off 60"])
        #expect(m.editor.selection.events == [1])
    }

    @Test func aMiddleDragTransposesAndResoundsAtTheNewPitch() throws {
        let (f, m, v) = try roll()
        let p = point(v, 1, .middle)
        let up2 = CGPoint(x: p.x, y: p.y - 2 * v.pitchAxis.rowHeight)
        v.contentMouseDown(p, clicks: 1, modifiers: [])
        v.contentMouseDragged(up2, modifiers: [])
        #expect(m.editor.gesture?.label == "Transpose Notes")
        #expect(f.gestureEdits.last == .event(edit: .nudge(events: [1], ticks: 0, semitones: 2)))
        v.contentMouseUp(up2, modifiers: [])
        #expect(m.editor.gesture == nil)
        #expect(log(f) == ["on 60", "off 60", "on 62", "off 62"])
    }

    @Test func anOptionStartDragCopiesInTime() throws {
        let (f, m, v) = try roll()
        defer { withExtendedLifetime(m) {} }  // the roll holds its document weakly
        let p = point(v, 2, .start)
        let right = CGPoint(x: p.x + 160, y: p.y)
        v.contentMouseDown(p, clicks: 1, modifiers: [.option])
        v.contentMouseDragged(right, modifiers: [.option])
        v.contentMouseUp(right, modifiers: [.option])
        #expect(f.gestureEdits.last == .event(edit: .duplicate(events: [2], ticks: 15_360, semitones: 0)))
    }

    @Test func anEndDragResizes() throws {
        let (f, m, v) = try roll()
        defer { withExtendedLifetime(m) {} }  // the roll holds its document weakly
        let p = point(v, 1, .end)
        // +100 pt is 9 600 ticks: the end (15 360) goes to 24 960, nearest quarter 30 720.
        let right = CGPoint(x: p.x + 100, y: p.y)
        v.contentMouseDown(p, clicks: 1, modifiers: [])
        v.contentMouseDragged(right, modifiers: [])
        v.contentMouseUp(right, modifiers: [])
        #expect(f.gestureEdits.last == .event(edit: .resize(events: [1], delta: 15_360)))
    }

    @Test func pencilClickInsertsAGridNoteInTheEditLane() throws {
        let (f, m, v) = try roll()
        m.editor.tool = .pencil
        // x 400 is tick 38 400: the note starts on the quarter at 30 720.
        let p = CGPoint(x: 400, y: v.pitchAxis.center(67))
        v.contentMouseDown(p, clicks: 1, modifiers: [])
        v.contentMouseUp(p, modifiers: [])
        // One undo step named "Insert Note" (Plan Ruling 7).
        #expect(f.calls.contains("begin Insert Note"))
        #expect(f.gestureEdits.last == Pencil.command(lane: LaneKey(3, 31), tick: 30_720, pitch: 67, length: 15_360))
        #expect(f.edits.isEmpty)
        #expect(m.editor.gesture == nil)
        #expect(log(f) == ["on 67", "off 67"], "the new pitch sounds while pressed")
    }

    @Test func pencilDragPaintsALength() throws {
        let (f, m, v) = try roll()
        m.editor.tool = .pencil
        let y = v.pitchAxis.center(67)
        v.contentMouseDown(CGPoint(x: 400, y: y), clicks: 1, modifiers: [])
        v.contentMouseDragged(CGPoint(x: 700, y: y), modifiers: [])
        v.contentMouseUp(CGPoint(x: 700, y: y), modifiers: [])
        // x 700 is tick 67 200; nearest quarter 61 440: 30 720 long.
        #expect(f.gestureEdits.last == Pencil.command(lane: LaneKey(3, 31), tick: 30_720, pitch: 67, length: 30_720))
        #expect(m.editor.gesture == nil)
    }

    @Test func theEraserDeletesEveryNoteItCrosses() throws {
        let (f, m, v) = try roll()
        m.editor.tool = .eraser
        let a = point(v, 1, .middle)
        let b = point(v, 2, .middle)
        v.contentMouseDown(a, clicks: 1, modifiers: [])
        v.contentMouseDragged(CGPoint(x: a.x + 10, y: a.y), modifiers: [])
        v.contentMouseDragged(b, modifiers: [])
        v.contentMouseUp(b, modifiers: [])
        #expect(f.calls.contains("begin Erase Notes"))
        #expect(f.gestureEdits.last == .event(edit: .delete(events: [1, 2])))
        #expect(m.editor.gesture == nil)
        #expect(f.auditions.isEmpty, "the eraser is silent")
    }

    @Test func aRubberBandSelectsNotes() throws {
        let (_, m, v) = try roll()
        let a = point(v, 1, .start)
        let b = point(v, 2, .end)
        // From empty space above E4 to below C4.
        let from = CGPoint(x: a.x - 1, y: b.y - 15)
        let to = CGPoint(x: b.x, y: a.y + 15)
        v.contentMouseDown(from, clicks: 1, modifiers: [])
        v.contentMouseDragged(to, modifiers: [])
        v.contentMouseUp(to, modifiers: [])
        #expect(Set(m.editor.selection.events) == [1, 2])
        #expect(m.editor.gesture == nil, "selecting is not an edit")
    }

    @Test func aNoteDragEndsItsSoundEvenWhenTheFinalEditWaits() throws {
        let (f, m, v) = try roll()
        let p = point(v, 1, .middle)
        let row = v.pitchAxis.rowHeight
        v.contentMouseDown(p, clicks: 1, modifiers: [])
        v.contentMouseDragged(CGPoint(x: p.x, y: p.y - row), modifiers: [])
        f.gestureErrors = [.Busy(message: "busy")]
        f.auditionErrors = [.Busy(message: "busy")]
        v.contentMouseUp(CGPoint(x: p.x, y: p.y - 2 * row), modifiers: [])
        #expect(m.editor.gesture?.finishing == true)
        #expect(m.editor.hasPendingNoteOffs)
        m.idle()
        #expect(m.editor.gesture == nil)
        #expect(!m.editor.hasPendingNoteOffs)
        #expect(log(f) == ["on 60", "off 60", "on 61", "off 61"])
    }

    @Test func escapeCancelsTheDragAndSilences() throws {
        let (f, m, v) = try roll()
        let p = point(v, 1, .middle)
        v.contentMouseDown(p, clicks: 1, modifiers: [])
        v.contentMouseDragged(CGPoint(x: p.x, y: p.y - 30), modifiers: [])
        v.contentCancel()
        #expect(m.editor.gesture == nil)
        #expect(f.calls.last == "abort")
        #expect(log(f).last?.hasPrefix("off") == true)
    }

    @Test func accessibilityListsVisibleNotes() throws {
        let (_, m, v) = try roll()
        defer { withExtendedLifetime(m) {} }  // the roll holds its document weakly
        #expect(v.accessibilityItems().map(\.label) == [
            "Note C4, bar 1 beat 1, velocity 64", "Note E4, bar 1 beat 2, velocity 64",
        ])
    }

    // MARK: Controller rulings

    @Test func theHitNoteIsTheOneDrawnOnTop() throws {
        let f = rollFake()
        // Note 4 of Groove (30) lies exactly over note 3 of Bass (32).
        f.laneEventTable["3/30"] = LaneEvents(seq: 3, lane: 30, rev: 1, events: [noteRow(4, tick: 0, pitch: 40)])
        let m = try DocumentModel(session: f)
        m.editor.openDetail(.sequence(3), kind: .pianoRoll)
        m.editor.updateLayout { $0.detail?.lanes = [30, 31, 32] }
        let r = PianoRollModel.pane(m)
        r.chosenLane = LaneKey(3, 31)
        let v = view(r)
        // Bottom to top: the non-active lanes, then the active one.
        let order = v.paintOrder().map(\.lane)
        #expect(order == [LaneKey(3, 32), LaneKey(3, 30), LaneKey(3, 31)])
        let p = point(v, 3, .middle)
        v.contentMouseDown(p, clicks: 1, modifiers: [])
        v.contentMouseUp(p, modifiers: [])
        #expect(m.editor.selection.events == [4], "Groove is drawn over Bass, so it is hit")
    }

    @Test func aGroupTransposeClampsAcrossLanes() throws {
        let f = rollFake()
        let m = try DocumentModel(session: f)
        m.editor.openDetail(.sequence(3), kind: .pianoRoll)
        defer { withExtendedLifetime(m) {} }
        let r = PianoRollModel.pane(m)
        let v = view(r)
        r.select([(LaneKey(3, 31), 1), (LaneKey(3, 32), 3)], adding: false)
        let p = point(v, 1, .middle)
        let down50 = CGPoint(x: p.x, y: p.y + 50 * v.pitchAxis.rowHeight)
        v.contentMouseDown(p, clicks: 1, modifiers: [])
        v.contentMouseDragged(down50, modifiers: [])
        v.contentMouseUp(down50, modifiers: [])
        // E2 (40) in Bass is the lowest of the group: it stops at 0.
        #expect(f.gestureEdits.last == .event(edit: .nudge(events: [1, 3], ticks: 0, semitones: -40)))
    }

    @Test func aZeroLengthNoteIsNotResizedWithoutHorizontalMotion() throws {
        let f = rollFake()
        f.laneEventTable["3/31"] = LaneEvents(
            seq: 3, lane: 31, rev: 2, events: [noteRow(1, tick: 0, pitch: 60, duration: 0)])
        let m = try DocumentModel(session: f)
        m.editor.openDetail(.lane(seq: 3, lane: 31), kind: .pianoRoll)
        let v = view(PianoRollModel.pane(m))
        let n = v.laneNotes().flatMap(\.notes)[0]
        let r = PianoRollGeometry.rect(n, time: v.axis, pitch: v.pitchAxis)
        let p = CGPoint(x: r.maxX - 0.5, y: r.midY)
        v.contentMouseDown(p, clicks: 1, modifiers: [])
        v.contentMouseDragged(CGPoint(x: p.x, y: p.y + 2), modifiers: [])
        v.contentMouseUp(CGPoint(x: p.x, y: p.y + 2), modifiers: [])
        #expect(f.gestureEdits.isEmpty && f.edits.isEmpty, "inside the dead zone: a click")
        v.contentMouseDown(p, clicks: 1, modifiers: [])
        v.contentMouseDragged(CGPoint(x: p.x, y: p.y + 6), modifiers: [])
        v.contentMouseUp(CGPoint(x: p.x, y: p.y + 6), modifiers: [])
        #expect(f.gestureEdits.isEmpty, "no horizontal motion: no resize")
        #expect(m.editor.gesture == nil)
    }

    @Test func leavingTheWindowCancelsTheDragAndSilences() throws {
        let (f, m, v) = try roll()
        let p = point(v, 1, .middle)
        v.contentMouseDown(p, clicks: 1, modifiers: [])
        v.contentMouseDragged(CGPoint(x: p.x, y: p.y - 30), modifiers: [])
        v.viewWillMove(toWindow: nil)
        #expect(m.editor.gesture == nil)
        #expect(f.calls.last == "abort")
        #expect(log(f) == ["on 60", "off 60", "on 63", "off 63"])
    }

    @Test func closingTheDocumentSilences() throws {
        let f = rollFake()
        let store = DocumentStore(makeEmpty: { f }, makeDemo: { f }, open: { _ in f })
        let id = try #require(store.newEmpty())
        let m = try #require(store.documents[id])
        m.editor.openDetail(.lane(seq: 3, lane: 31), kind: .pianoRoll)
        let v = view(PianoRollModel.pane(m))
        v.contentMouseDown(point(v, 1, .middle), clicks: 1, modifiers: [])
        #expect(log(f) == ["on 60"])
        store.close(id)
        #expect(log(f) == ["on 60", "off 60"])
    }

    @Test func openingAnotherBlockRecentresAndScrolls() throws {
        let (_, m, v) = try roll()
        let middle = { v.pitchAxis.pitch(atY: v.content.bounds.height / 2) }
        #expect((61...63).contains(middle()), "centred on C4–E4")
        #expect(v.axis.originTick <= 0)
        // Bass (E2) from bar 2, in the same open roll (Plan 4b Ruling 10).
        m.editor.openDetail(.block(seq: 3, lane: 32, start: 61_440, end: 122_880), kind: .pianoRoll)
        v.refresh()
        v.layout()
        #expect((39...41).contains(middle()), "centred on E2, not \(middle())")
        #expect(v.axis.originTick == 61_440 - 16 * v.axis.ticksPerPoint)
        // A layout with the same target leaves the user's scroll alone.
        v.verticalOffset = 0
        v.layout()
        #expect(v.verticalOffset == 0)
    }

    @Test func aPencilClickWhileAGestureFinishesSaysSoAndIsSilent() throws {
        let (f, m, v) = try roll()
        #expect(m.editor.beginGesture("Move Notes", lanes: [LaneKey(3, 31)]))
        m.editor.tool = .pencil
        let p = CGPoint(x: 400, y: v.pitchAxis.center(67))
        v.contentMouseDown(p, clicks: 1, modifiers: [])
        v.contentMouseUp(p, modifiers: [])
        #expect(m.editor.status == EditorModel.gestureOpenMessage)
        #expect(!f.calls.contains("begin Insert Note"))
        #expect(log(f) == ["on 67", "off 67"])
    }

    @Test func aNoteDragWhileAGestureFinishesSaysSoAndSilences() throws {
        let (f, m, v) = try roll()
        #expect(m.editor.beginGesture("Move Notes", lanes: [LaneKey(3, 31)]))
        let p = point(v, 1, .middle)
        v.contentMouseDown(p, clicks: 1, modifiers: [])
        #expect(log(f) == ["on 60"])
        v.contentMouseDragged(CGPoint(x: p.x, y: p.y - 30), modifiers: [])
        #expect(m.editor.status == EditorModel.gestureOpenMessage)
        #expect(log(f) == ["on 60", "off 60"], "the pressed note stops when the drag is refused")
        v.contentMouseUp(CGPoint(x: p.x, y: p.y - 30), modifiers: [])
        #expect(m.editor.gesture?.label == "Move Notes", "the other gesture is untouched")
    }

    @Test func aFastEraserDragErasesTheNotesBetweenSamples() throws {
        let (f, m, v) = try roll()
        defer { withExtendedLifetime(m) {} }
        m.editor.tool = .eraser
        // A diagonal from below-left of C4 to above-right of E4: neither
        // sample lands on a note, the segments cross both.
        let a = point(v, 1, .middle)
        let b = point(v, 2, .middle)
        let from = CGPoint(x: a.x - (b.x - a.x) / 4, y: a.y - (b.y - a.y) / 4)
        let to = CGPoint(x: b.x + (b.x - a.x) / 4, y: b.y + (b.y - a.y) / 4)
        guard case .empty = PianoRollGeometry.hit(
            from, lanes: v.laneNotes(), active: nil, time: v.axis, pitch: v.pitchAxis),
            case .empty = PianoRollGeometry.hit(
                to, lanes: v.laneNotes(), active: nil, time: v.axis, pitch: v.pitchAxis)
        else {
            Issue.record("a sample landed on a note")
            return
        }
        v.contentMouseDown(from, clicks: 1, modifiers: [])
        v.contentMouseDragged(to, modifiers: [])
        v.contentMouseUp(to, modifiers: [])
        #expect(f.gestureEdits.last == .event(edit: .delete(events: [1, 2])))
    }

    @Test func leavingTheWindowCancelsARegionMove() throws {
        let f = audioFake()
        let m = try DocumentModel(session: f)
        m.editor.openDetail(.sequence(3), kind: .pianoRoll)
        let v = RegionStripView(model: m.pianoRoll(.pane))
        v.frame = NSRect(x: 0, y: 0, width: 800, height: 60)
        v.timeAxis = TimeAxis(ticksPerPoint: 96)
        let side = PianoRollContainer.sideWidth
        v.pressed(at: CGPoint(x: side + 50, y: 11), clicks: 1, modifiers: [])
        v.dragged(to: CGPoint(x: side + 150, y: 11), modifiers: [])
        #expect(m.editor.gesture?.label == "Move Region")
        // ⌘W mid-drag: the release never comes.
        v.viewWillMove(toWindow: nil)
        #expect(m.editor.gesture == nil)
        #expect(f.calls.last == "abort")
        v.released(at: CGPoint(x: side + 150, y: 11), modifiers: [])
        #expect(m.editor.gesture == nil)
    }

    @Test func laneColoursWrapAroundThePalette() {
        #expect(PianoRollPalette.color(8) == PianoRollPalette.color(0))
        #expect(PianoRollPalette.color(-1) == PianoRollPalette.color(7))
        #expect(PianoRollPalette.color(1) != PianoRollPalette.color(0))
    }
}

@MainActor
private func view(_ r: PianoRollModel) -> PianoRollView {
    let v = PianoRollView(model: r)
    v.frame = NSRect(x: 0, y: 0, width: 800, height: 400)
    v.layout()
    return v
}
