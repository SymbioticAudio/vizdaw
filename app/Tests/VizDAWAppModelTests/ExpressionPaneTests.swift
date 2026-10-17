import AppKit
import Testing
import VizDAWCore
@testable import VizDAWAppModel
@testable import VizDAWEditors

@MainActor
@Suite struct ExpressionPaneTests {
    func pane() throws -> (FakeSession, DocumentModel, ExpressionPaneView) {
        let f = rollFake()
        let m = try DocumentModel(session: f)
        m.editor.openDetail(.lane(seq: 3, lane: 31), kind: .pianoRoll)
        let v = ExpressionPaneView(model: m.pianoRoll(.pane))
        v.frame = NSRect(x: 0, y: 0, width: 600, height: 96)
        v.timeAxis = TimeAxis(ticksPerPoint: 96)
        return (f, m, v)
    }

    @Test func aStrokeWritesPerNoteValuesAsOneStep() throws {
        let (f, m, v) = try pane()
        // Over note 1 (C4, 0 to 15 360 = x 0 to 160 in the time area).
        v.strokeBegan(at: CGPoint(x: 64 + 10, y: 3))
        v.strokeMoved(to: CGPoint(x: 64 + 20, y: 48))
        #expect(m.editor.gesture?.label == "Draw Expression")
        v.strokeEnded()
        #expect(m.editor.gesture == nil)
        guard case let .event(edit: .setPerNote(events, kind, from, to, points))? = f.gestureEdits.last else {
            Issue.record("no per-note edit: \(f.gestureEdits)")
            return
        }
        #expect(events == [1] && kind == .pitchBend && from == 960 && to == 1_920)
        #expect(points.map(\.tick) == [960, 1_920])
        #expect(points.first?.value == UInt32.max)
    }

    @Test func theEraserErasesAndAClickWritesOnePoint() throws {
        let (f, m, v) = try pane()
        m.editor.tool = .eraser
        v.strokeBegan(at: CGPoint(x: 64 + 10, y: 3))
        v.strokeMoved(to: CGPoint(x: 64 + 20, y: 3))
        v.strokeEnded()
        #expect(f.gestureEdits.last == .event(edit: .setPerNote(events: [1], kind: .pitchBend, from: 960, to: 1_920, points: [])))
        m.editor.tool = .pencil
        v.strokeBegan(at: CGPoint(x: 64 + 10, y: 93))
        v.strokeEnded()
        #expect(f.edits.last == .event(edit: .setPerNote(
            events: [1], kind: .pitchBend, from: 960, to: 960, points: [PerNoteValueRow(tick: 960, value: 0)])))
    }

    @Test func theSideColumnIsNotATimeArea() throws {
        let (f, m, v) = try pane()
        v.strokeBegan(at: CGPoint(x: 10, y: 3))
        v.strokeMoved(to: CGPoint(x: 30, y: 3))
        v.strokeEnded()
        #expect(f.gestureEdits.isEmpty && f.edits.isEmpty)
        #expect(m.editor.gesture == nil)
    }

    @Test func escapeCancelsTheStroke() throws {
        let (f, m, v) = try pane()
        v.strokeBegan(at: CGPoint(x: 64 + 10, y: 3))
        v.strokeMoved(to: CGPoint(x: 64 + 20, y: 48))
        v.strokeCancelled()
        #expect(m.editor.gesture == nil)
        #expect(f.calls.last == "abort")
    }
}
