import AppKit
import Testing
import VizDAWCore
@testable import VizDAWAppModel
@testable import VizDAWEditors

private let move200: EditCommand = .event(edit: .move(events: [1], delta: 200, toLane: nil))

@MainActor
@Suite struct ExactGestureTests {
    final class Counter { var n = 0 }

    func open(exact: Bool, fake: FakeSession = editorFake()) throws -> (FakeSession, DocumentModel, Counter) {
        let m = try DocumentModel(session: fake)
        let c = Counter()
        m.selectModify.onPresent = { c.n += 1 }
        m.editor.exactMode = exact
        fake.calls = []
        return (fake, m, c)
    }

    @Test func anExactDragLeavesNoUndoStepAndOpensThePanel() throws {
        let (f, m, c) = try open(exact: true)
        #expect(m.editor.beginGesture("Move Notes", lanes: [chords], origin: .notes))
        m.editor.updateGesture(move200)
        m.editor.endGesture(move200)
        #expect(f.calls == ["begin Move Notes", "gesture", "abort"], "previewed, then put back")
        #expect(m.editor.gesture == nil)
        #expect(c.n == 1 && m.selectModify.task == .move)
        #expect(m.selectModify.config.modify.op == .move(delta: 200))
        #expect(f.sel.events == [1] && f.sel.lanes == [LaneRef(seq: 3, lane: 31)], "it acts on the dragged notes")
    }

    @Test func theBaselineIsTakenAfterTheRevertAndTheSelection() throws {
        let (_, m, _) = try open(exact: true)
        #expect(m.editor.beginGesture("Move Block", lanes: [chords], origin: .blocks))
        m.editor.endGesture(move200)
        // The request is live: the selection is the one it set, not stale.
        #expect(m.selectModify.actsOnDraggedEvents)
    }

    @Test func aDiagonalDragShowsTheDroppedAxisNotice() throws {
        let (_, m, _) = try open(exact: true)
        let diagonal: EditCommand = .event(edit: .nudge(events: [1], ticks: 15_360, semitones: 1))
        #expect(m.editor.beginGesture("Move Notes", lanes: [chords], origin: .notes))
        m.editor.endGesture(diagonal)
        #expect(m.selectModify.notice == DroppedAxis.vertical.message)
    }

    @Test func onlyNoteAndBlockDragsGoExact() throws {
        for origin in [GestureOrigin.regionStrip, .expression, .other] {
            let (f, m, c) = try open(exact: true)
            #expect(m.editor.beginGesture("Drag", lanes: [chords], origin: origin))
            m.editor.endGesture(move200)
            #expect(f.calls.last == "end" && c.n == 0, "\(origin) applies as usual")
        }
        for origin in [GestureOrigin.notes, .blocks] {
            let (f, m, c) = try open(exact: true)
            #expect(m.editor.beginGesture("Drag", lanes: [chords], origin: origin))
            m.editor.endGesture(move200)
            #expect(f.calls.last == "abort" && c.n == 1, "\(origin) opens the panel")
        }
    }

    @Test func aCrossLaneMoveAppliesAsUsual() throws {
        let (f, m, c) = try open(exact: true)
        let cross: EditCommand = .event(edit: .move(events: [1], delta: 0, toLane: LaneRef(seq: 3, lane: 30)))
        #expect(m.editor.beginGesture("Move Block", lanes: [chords], origin: .blocks))
        m.editor.endGesture(cross)
        #expect(f.calls.last == "end" && c.n == 0)
    }

    @Test func escapeInExactModeOpensNothing() throws {
        let (f, m, c) = try open(exact: true)
        #expect(m.editor.beginGesture("Move Notes", lanes: [chords], origin: .notes))
        m.editor.updateGesture(move200)
        m.editor.cancelGesture()
        #expect(c.n == 0 && !m.selectModify.isOpen)
        #expect(f.calls.last == "abort")
    }

    @Test func aPencilClickInExactModeStillInserts() throws {
        let (f, m, c) = try open(exact: true)
        let insert: EditCommand = .event(edit: .insert(seq: 3, lane: 31, events: []))
        #expect(m.editor.beginGesture("Insert Note", lanes: [chords], origin: .other))
        m.editor.endGesture(insert)
        #expect(f.calls.last == "end" && c.n == 0)
    }

    @Test func withExactModeOffADragApplies() throws {
        let (f, m, c) = try open(exact: false)
        #expect(m.editor.beginGesture("Move Notes", lanes: [chords], origin: .notes))
        m.editor.updateGesture(move200)
        m.editor.endGesture(move200)
        #expect(f.calls.last == "end" && c.n == 0)
    }

    // MARK: Each view passes its own origin

    @Test func anArrangementBlockDragGoesExact() throws {
        let (f, m, c) = try open(exact: true)
        let v = ArrangementView(document: m)
        v.frame = NSRect(x: 0, y: 0, width: 800, height: 300)
        v.layout()
        f.calls = []
        v.contentMouseDown(CGPoint(x: 50, y: 70), clicks: 1, modifiers: [])
        v.contentMouseDragged(CGPoint(x: 300, y: 70), modifiers: [])
        v.contentMouseUp(CGPoint(x: 300, y: 70), modifiers: [])
        #expect(f.calls == ["begin Move Block", "gesture", "abort"])
        #expect(c.n == 1 && m.selectModify.task == .move)
    }

    @Test func escapeDuringAnArrangementExactDragOpensNothing() throws {
        let (f, m, c) = try open(exact: true)
        let v = ArrangementView(document: m)
        v.frame = NSRect(x: 0, y: 0, width: 800, height: 300)
        v.layout()
        f.calls = []
        v.contentMouseDown(CGPoint(x: 50, y: 70), clicks: 1, modifiers: [])
        v.contentMouseDragged(CGPoint(x: 300, y: 70), modifiers: [])
        v.contentCancel()
        v.contentMouseUp(CGPoint(x: 300, y: 70), modifiers: [])
        #expect(c.n == 0 && f.calls.last == "abort")
    }

    @Test func aPianoRollNoteDragGoesExactButAPencilClickDoesNot() throws {
        let (f, m, c) = try open(exact: true, fake: rollFake())
        m.editor.openDetail(.lane(seq: 3, lane: 31), kind: .pianoRoll)
        let v = PianoRollView(model: PianoRollModel.pane(m))
        v.frame = NSRect(x: 0, y: 0, width: 800, height: 400)
        v.layout()
        let n = v.laneNotes().flatMap(\.notes).first { $0.id == 1 }!
        let r = PianoRollGeometry.rect(n, time: v.axis, pitch: v.pitchAxis)
        let p = CGPoint(x: r.midX, y: r.midY)
        let up2 = CGPoint(x: p.x, y: p.y - 2 * v.pitchAxis.rowHeight)
        v.contentMouseDown(p, clicks: 1, modifiers: [])
        v.contentMouseDragged(up2, modifiers: [])
        v.contentMouseUp(up2, modifiers: [])
        #expect(c.n == 1 && m.selectModify.task == .transpose)
        #expect(m.editor.gesture == nil)
        m.selectModify.close()
        m.editor.tool = .pencil
        f.calls = []
        let q = CGPoint(x: 400, y: v.pitchAxis.center(67))
        v.contentMouseDown(q, clicks: 1, modifiers: [])
        v.contentMouseUp(q, modifiers: [])
        #expect(f.calls.last == "end" && c.n == 1, "pencil applies (Ruling 23)")
    }

    @Test func aRegionMoveAppliesInExactMode() throws {
        let (f, m, c) = try open(exact: true, fake: audioFake())
        m.editor.openDetail(.sequence(3), kind: .pianoRoll)
        let v = RegionStripView(model: m.pianoRoll(.pane))
        v.frame = NSRect(x: 0, y: 0, width: 800, height: 60)
        v.timeAxis = TimeAxis(ticksPerPoint: 96)
        let side = PianoRollContainer.sideWidth
        v.pressed(at: CGPoint(x: side + 50, y: 11), clicks: 1, modifiers: [])
        v.dragged(to: CGPoint(x: side + 150, y: 11), modifiers: [])
        v.released(at: CGPoint(x: side + 150, y: 11), modifiers: [])
        #expect(f.calls.last == "end" && c.n == 0)
    }

    @Test func anExpressionStrokeAppliesInExactMode() throws {
        let (f, m, c) = try open(exact: true, fake: rollFake())
        m.editor.openDetail(.lane(seq: 3, lane: 31), kind: .pianoRoll)
        let v = ExpressionPaneView(model: m.pianoRoll(.pane))
        v.frame = NSRect(x: 0, y: 0, width: 600, height: 96)
        v.timeAxis = TimeAxis(ticksPerPoint: 96)
        v.strokeBegan(at: CGPoint(x: 74, y: 3))
        v.strokeMoved(to: CGPoint(x: 84, y: 48))
        v.strokeEnded()
        #expect(f.calls.last == "end" && c.n == 0)
    }
}
