import AppKit
import Testing
import VizDAWCore
@testable import VizDAWAppModel
@testable import VizDAWEditors

private let q: Int64 = 15_360

@MainActor
@Suite struct ScaleTimeTests {
    @Test func theDragScalesAroundTheEarliestStart() throws {
        let d = try #require(ScaleTimeDrag(notes: [noteRow(1, tick: q, duration: q), noteRow(2, tick: 2 * q, duration: q)]))
        #expect(d.anchor == q && d.span == 2 * q)
        var off = Snap()
        off.enabled = false
        #expect(
            d.command(rawTicks: 2 * q, snap: off, meter: [])
                == .selectAndModify(
                    rule: ScaleTimeDrag.notesOnly, mode: .refine, modify: .scaleTime(num: 2, den: 1, anchor: q), toCopy: false))
        #expect(d.command(rawTicks: 0, snap: off, meter: []) == nil, "no change, no step")
        #expect(
            d.command(rawTicks: -100 * q, snap: off, meter: [])
                == .selectAndModify(
                    rule: ScaleTimeDrag.notesOnly, mode: .refine, modify: .scaleTime(num: 1, den: UInt32(2 * q), anchor: q), toCopy: false),
            "never shorter than a tick")
        #expect(ScaleTimeDrag(notes: []) == nil)
    }

    @Test func aCommandDragOfANoteEndScalesTheSelectedNotes() throws {
        let f = rollFake()
        let m = try DocumentModel(session: f)
        m.editor.openDetail(.lane(seq: 3, lane: 31), kind: .pianoRoll)
        let v = PianoRollView(model: PianoRollModel.pane(m))
        v.frame = NSRect(x: 0, y: 0, width: 800, height: 400)
        v.layout()
        m.editor.setSelection(SelectionRow(timeRange: nil, lanes: [LaneRef(seq: 3, lane: 31)], events: [1, 2]))
        let n = v.laneNotes().flatMap(\.notes).first { $0.id == 1 }!
        let r = PianoRollGeometry.rect(n, time: v.axis, pitch: v.pitchAxis)
        let end = CGPoint(x: r.maxX - 2, y: r.midY)
        // Notes 1 (0…q) and 2 (q…2q): +1 quarter of drag makes the span 3q.
        let right = CGPoint(x: end.x + 160, y: end.y)
        v.contentMouseDown(end, clicks: 1, modifiers: [.command])
        v.contentMouseDragged(right, modifiers: [.command])
        v.contentMouseUp(right, modifiers: [.command])
        #expect(
            f.gestureEdits.last
                == .selectAndModify(
                    rule: ScaleTimeDrag.notesOnly, mode: .refine, modify: .scaleTime(num: 3, den: 2, anchor: 0),
                    toCopy: false), "the selected notes only")
        #expect(f.calls.first == "begin Scale Time" && f.calls.last == "end", "without exact mode it applies")
        #expect(m.editor.gesture == nil)
    }

    @Test func exactModeOpensScaleTimeInThePanel() throws {
        let cmd: EditCommand = .selectAndModify(
            rule: ScaleTimeDrag.notesOnly, mode: .refine, modify: .scaleTime(num: 3, den: 2, anchor: 0), toCopy: false)
        let r = try #require(ExactMode.request(for: cmd, lanes: [LaneKey(3, 31)], origin: .notes))
        #expect(r.task == .change && r.config.modify.op == .scaleTime(num: 3, den: 2, anchor: 0))
        #expect(r.config.select.enabled && r.config.select.mode == .refine)
        #expect(r.config.select.conditions == ScaleTimeDrag.notesOnly.conditions, "the panel limits it to notes too")
        #expect(r.events == nil, "it acts on the selection (the dragged notes)")
        #expect(ExactMode.request(for: cmd, lanes: [], origin: .blocks) == nil)
    }

    private func pressedEnd() throws -> (FakeSession, DocumentModel, PianoRollView, CGPoint) {
        let f = rollFake()
        let m = try DocumentModel(session: f)
        m.editor.openDetail(.lane(seq: 3, lane: 31), kind: .pianoRoll)
        let v = PianoRollView(model: PianoRollModel.pane(m))
        v.frame = NSRect(x: 0, y: 0, width: 800, height: 400)
        v.layout()
        m.editor.setSelection(SelectionRow(timeRange: nil, lanes: [LaneRef(seq: 3, lane: 31)], events: [1, 2]))
        let n = try #require(v.laneNotes().flatMap(\.notes).first { $0.id == 1 })
        let r = PianoRollGeometry.rect(n, time: v.axis, pitch: v.pitchAxis)
        return (f, m, v, CGPoint(x: r.maxX - 2, y: r.midY))
    }

    @Test func exactModeRevertsThePreviewAndOpensThePanelWithoutApplying() throws {
        let (f, m, v, end) = try pressedEnd()
        m.editor.exactMode = true
        let right = CGPoint(x: end.x + 160, y: end.y)
        v.contentMouseDown(end, clicks: 1, modifiers: [.command])
        v.contentMouseDragged(right, modifiers: [.command])
        v.contentMouseUp(right, modifiers: [.command])
        #expect(m.editor.gesture == nil)
        #expect(f.calls.contains("begin Scale Time") && f.calls.contains("abort"), "the preview was reverted")
        #expect(!f.calls.contains("end"), "nothing committed")
        let s = m.selectModify
        #expect(s.isOpen && s.config.modify.op == .scaleTime(num: 3, den: 2, anchor: 0))
    }

    @Test func escapeAbortsTheScaleDrag() throws {
        let (f, m, v, end) = try pressedEnd()
        let right = CGPoint(x: end.x + 160, y: end.y)
        v.contentMouseDown(end, clicks: 1, modifiers: [.command])
        v.contentMouseDragged(right, modifiers: [.command])
        #expect(m.editor.gesture?.label == "Scale Time")
        #expect(m.editor.cancelGesture())
        #expect(m.editor.gesture == nil && !f.calls.contains("end"))
    }
}
