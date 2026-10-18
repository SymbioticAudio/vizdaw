import AppKit
import Testing
import VizDAWCore
@testable import VizDAWAppModel
@testable import VizDAWEditors

private let q: Int64 = 15_360
private let s16 = q / 4

/// `drumFake()` in a drum grid in the detail pane, 96 ticks per point (a
/// sixteenth is 40 pt). Content bands: header 0–22, Kick 22–42, Snare
/// 42–62 (90 with its velocity lane shown), Perc's header, Clap, the
/// new-lane band.
@MainActor
private func grid(snareVelocity: Bool = false) throws -> (FakeSession, DocumentModel, DrumGridContainer) {
    let f = drumFake()
    if snareVelocity { f.drumTable[3]?[0].rows[1].showVelocity = true }
    let m = try DocumentModel(session: f)
    m.editor.openDetail(.lane(seq: 3, lane: 40), kind: .drumGrid)
    let c = DrumGridContainer(model: m.drumGrid(.pane))
    c.frame = NSRect(x: 0, y: 0, width: 950, height: 300)
    c.layout()
    c.canvas.layout()
    f.calls = []
    return (f, m, c)
}

private func paint(_ ticks: [Int64], velocity: UInt16 = 100 << 9, row: UInt64 = 2) -> EditCommand {
    .drum(
        edit: .paint(
            seq: 3, lane: 40, row: row, hits: ticks.map { DrumPaintHitRow(tick: $0, velocity: velocity) },
            window: s16, duration: q / 8))
}

@MainActor
@Suite struct DrumGridViewTests {
    @Test func aClickOnAnEmptyCellPaintsIt() throws {
        let (f, m, c) = try grid()
        defer { withExtendedLifetime(m) {} }
        c.canvas.contentMouseDown(CGPoint(x: 85, y: 50), clicks: 1, modifiers: [])
        c.canvas.contentMouseUp(CGPoint(x: 85, y: 50), modifiers: [])
        #expect(f.gestureEdits.last == paint([2 * s16]))
        #expect(f.calls == ["begin Paint Drum Hits", "gesture", "end"])
        #expect(m.activeDrumGrid == .pane)
        #expect(m.drumGrid(.pane).selectedRow == DrumRowKey(lane: 40, row: 2))
    }

    @Test func aDragPaintsARunAndEscapeTakesItBack() throws {
        let (f, m, c) = try grid()
        defer { withExtendedLifetime(m) {} }
        c.canvas.contentMouseDown(CGPoint(x: 5, y: 50), clicks: 1, modifiers: [])
        c.canvas.contentMouseDragged(CGPoint(x: 125, y: 70), modifiers: [])
        #expect(f.gestureEdits.last == paint([0, s16, 2 * s16, 3 * s16]), "the stroke stays on its row")
        c.canvas.contentCancel()
        #expect(f.calls.last == "abort")
        #expect(m.editor.gesture == nil)
        c.canvas.contentMouseUp(CGPoint(x: 125, y: 70), modifiers: [])
        #expect(f.calls.last == "abort", "the release after Escape does nothing")
    }

    @Test func theEraserErasesTheHitsItCrosses() throws {
        let (f, m, c) = try grid()
        defer { withExtendedLifetime(m) {} }
        m.editor.tool = .eraser
        c.canvas.contentMouseDown(CGPoint(x: 5, y: 30), clicks: 1, modifiers: [])
        c.canvas.contentMouseDragged(CGPoint(x: 165, y: 30), modifiers: [])
        c.canvas.contentMouseUp(CGPoint(x: 165, y: 30), modifiers: [])
        #expect(f.calls.first == "begin Erase Drum Hits")
        #expect(f.gestureEdits.last == .event(edit: .delete(events: [11, 12])))
    }

    @Test func aShownVelocityLaneSetsTheVelocityFromTheHeight() throws {
        let (f, m, c) = try grid(snareVelocity: true)
        defer { withExtendedLifetime(m) {} }
        // The Snare band is 42–90: its top is 127.
        c.canvas.contentMouseDown(CGPoint(x: 85, y: 42), clicks: 1, modifiers: [])
        c.canvas.contentMouseUp(CGPoint(x: 85, y: 42), modifiers: [])
        #expect(f.gestureEdits.last == paint([2 * s16], velocity: 127 << 9))
        // Header, Kick, Snare with its lane, header, Clap, new-lane band.
        let height: CGFloat = 160
        #expect(c.canvas.contentHeight == height)
    }

    @Test func cellsAndHitsAreAccessibleAndPressToggles() throws {
        let (f, m, c) = try grid()
        defer { withExtendedLifetime(m) {} }
        let items = c.canvas.accessibilityItems()
        #expect(items.first?.label == "Kick, bar 1 beat 1, on, velocity 100")
        let off = try #require(items.first { $0.label == "Snare, bar 1 beat 1 tick 7680, off" })
        off.press?()
        #expect(f.gestureEdits.last == paint([2 * s16]))
        m.drumGrid(.pane).updateSettings { $0.view = .timeline }
        let hits = c.canvas.accessibilityItems().map(\.label)
        #expect(
            hits == [
                "Kick, bar 1 beat 1, on, velocity 100", "Kick, bar 1 beat 2, on, velocity 100",
                "Snare, bar 1 beat 2, on, velocity 100",
            ])
    }

    @Test func aRowHeaderDragMovesTheRowAndADoubleClickRenames() throws {
        let (f, m, c) = try grid()
        defer { withExtendedLifetime(m) {} }
        let h = c.headers
        let top = EditorCanvas.rulerHeight
        // Kick dragged onto Clap (content 84–104): into Perc.
        h.mouseDown(at: CGPoint(x: 20, y: top + 30), clicks: 1)
        h.mouseDragged(to: CGPoint(x: 20, y: top + 95))
        h.mouseUp(at: CGPoint(x: 20, y: top + 95))
        #expect(f.edits.last == .drum(edit: .moveRowToLane(seq: 3, lane: 40, row: 1, toLane: 41)))
        // A press without a drag only selects.
        let n = f.edits.count
        h.mouseDown(at: CGPoint(x: 20, y: top + 50), clicks: 1)
        h.mouseUp(at: CGPoint(x: 20, y: top + 50))
        #expect(f.edits.count == n)
        #expect(m.drumGrid(.pane).selectedRow == DrumRowKey(lane: 40, row: 2))
        h.mouseDown(at: CGPoint(x: 20, y: top + 50), clicks: 2)
        let field = try #require(h.renaming?.field)
        #expect(m.editor.textEditing)
        field.stringValue = "Rim"
        h.controlTextDidEndEditing(Notification(name: NSControl.textDidEndEditingNotification))
        #expect(f.edits.last == .drum(edit: .renameRow(seq: 3, lane: 40, row: 2, name: "Rim")))
        #expect(!m.editor.textEditing)
        #expect(
            h.rowMenu(DrumPress(lane: 0, row: 1)).items.map(\.title)
                == ["Rename…", "Show Velocity", "", "Split Row to Own Lane", "Remove Row"])
        let rows = (h.accessibilityChildren() as? [NSAccessibilityElement])?.compactMap { $0.accessibilityLabel() }
        #expect(rows == ["Kick, C2, Drums, Kit", "Snare, D2, Drums, Kit", "Clap, D#2, Perc, Kit"])
    }

    @Test func everyRowHeaderNamesItsDestinationAndDimsAnInheritedOne() throws {
        let (_, m, c) = try grid()
        defer { withExtendedLifetime(m) {} }
        let own = try #require(m.drumGrid(.pane).row(0, 0))
        var withOwn = own
        withOwn.dest = 7
        withOwn.destination = "Other"
        let a = DrumRowHeaderView.caption(withOwn)
        #expect(a.destination == "Other" && !a.inherited)
        let b = DrumRowHeaderView.caption(own)
        #expect(b.destination == "Kit" && b.inherited, "no destination of its own: the lane's, dimmed")
        _ = c
    }

    @Test func escapeDuringARowHeaderDragDropsNothing() throws {
        let (f, m, c) = try grid()
        defer { withExtendedLifetime(m) {} }
        let h = c.headers
        let top = EditorCanvas.rulerHeight
        let n = f.edits.count
        h.mouseDown(at: CGPoint(x: 20, y: top + 30), clicks: 1)
        h.mouseDragged(to: CGPoint(x: 20, y: top + 95))
        let esc = try #require(
            NSEvent.keyEvent(
                with: .keyDown, location: .zero, modifierFlags: [], timestamp: 0, windowNumber: 0, context: nil,
                characters: "\u{1b}", charactersIgnoringModifiers: "\u{1b}", isARepeat: false, keyCode: 53))
        h.keyDown(with: esc)
        h.mouseUp(at: CGPoint(x: 20, y: top + 95))
        #expect(f.edits.count == n, "the release after Escape drops nothing")
        #expect(h.acceptsFirstResponder)
    }
}
