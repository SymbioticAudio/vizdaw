import AppKit
import Testing
import VizDAWCore
@testable import VizDAWAppModel
@testable import VizDAWEditors

private let q: Int64 = 15_360

/// Chords (31) of `rollFake()` in a Strip Chart in the detail pane: notes 1
/// (C4 at 0) and 2 (E4 at one quarter), 160 pt per quarter at the default
/// zoom, one velocity row 96 pt tall.
@MainActor
private func chart() throws -> (FakeSession, DocumentModel, StripChartView) {
    let f = rollFake()
    let m = try DocumentModel(session: f)
    m.editor.openDetail(.lane(seq: 3, lane: 31), kind: .stripChart)
    let v = StripChartView(model: m.stripChart(.pane))
    v.frame = NSRect(x: 0, y: 0, width: 800, height: 300)
    v.layout()
    f.calls = []
    return (f, m, v)
}

@MainActor
@Suite struct StripChartViewTests {
    @Test func aDragInDrawModeSetsVelocitiesAlongTheLine() throws {
        let (f, m, v) = try chart()
        defer { withExtendedLifetime(m) {} }
        let s = StripScale(range: StripLane.velocity.range, top: 0, height: 96)
        v.contentMouseDown(CGPoint(x: 0, y: 30), clicks: 1, modifiers: [])
        v.contentMouseDragged(CGPoint(x: 160, y: 48), modifiers: [])
        v.contentMouseUp(CGPoint(x: 160, y: 48), modifiers: [])
        let line = LineRow(
            from: 0, to: q, start: s.value(atY: 30), end: s.value(atY: 48), step: q / 8, shape: .straight)
        #expect(f.gestureEdits.last == StripCommands.line(.velocity, op: .set, line: line, selectedOnly: false))
        #expect(f.calls.first == "begin Change Velocity" && f.calls.last == "end")
        #expect(v.path.isEmpty)
    }

    @Test func aClickInDrawModeDrawsOnePoint() throws {
        let (f, m, v) = try chart()
        defer { withExtendedLifetime(m) {} }
        v.contentMouseDown(CGPoint(x: 165, y: 48), clicks: 1, modifiers: [])
        v.contentMouseUp(CGPoint(x: 165, y: 48), modifiers: [])
        guard case .selectAndModify(_, _, .line(_, _, let line)?, _)? = f.gestureEdits.last else {
            Issue.record("no line")
            return
        }
        #expect(line.from == q && line.to == q, "snapped to the quarter, one tick")
    }

    @Test func escapeLeavesNoUndoStep() throws {
        let (f, m, v) = try chart()
        defer { withExtendedLifetime(m) {} }
        v.contentMouseDown(CGPoint(x: 0, y: 30), clicks: 1, modifiers: [])
        v.contentMouseDragged(CGPoint(x: 160, y: 48), modifiers: [])
        v.contentCancel()
        #expect(f.calls.last == "abort" && m.editor.gesture == nil)
        #expect(m.stripChart(.pane).stroke == nil && v.path.isEmpty)
    }

    @Test func modifyModeReadsTheOperationsAmount() throws {
        let (f, m, v) = try chart()
        defer { withExtendedLifetime(m) {} }
        m.stripChart(.pane).updateSettings {
            $0.mode = .modify
            $0.op = .scale
        }
        // Right of the row's title (a press there opens its menu).
        v.contentMouseDown(CGPoint(x: 220, y: 3), clicks: 1, modifiers: [.option])
        v.contentMouseDragged(CGPoint(x: 330, y: 93), modifiers: [.option])
        v.contentMouseUp(CGPoint(x: 330, y: 93), modifiers: [.option])
        guard case let .selectAndModify(_, mode, .line(_, op, line)?, _)? = f.gestureEdits.last else {
            Issue.record("no line")
            return
        }
        #expect(op == .scale && mode == .refine)
        #expect(line.start == 200 && line.end == 0, "top is 200 %, bottom 0 %")
        #expect(line.from == q && line.to == 2 * q)
    }

    @Test func rowsMapPointsAndMenusListEveryKind() throws {
        let (_, m, v) = try chart()
        defer { withExtendedLifetime(m) {} }
        m.stripChart(.pane).addRow()
        #expect(v.row(atY: 10) == 0 && v.row(atY: 100) == 1 && v.row(atY: 250) == nil)
        #expect(v.point(CGPoint(x: 170, y: 3), row: 0) == StripPoint(tick: q, value: 0xFFFF))
        let titles = v.rowMenu(0).items.map(\.title)
        for t in ["Velocity", "Release Velocity", "Length", "Controller", "Pitch Bend", "Channel Pressure",
                  "Poly Pressure", "Program", "Tempo", "Automation", "Add Row", "Remove Row"] {
            #expect(titles.contains(t), "\(t)")
        }
        let target = StripRowMenuTarget()
        var chosen: StripLane?
        target.lanes = [.tempo]
        target.choose = { chosen = $0 }
        let item = NSMenuItem(title: "Tempo", action: #selector(StripRowMenuTarget.pick(_:)), keyEquivalent: "")
        item.tag = 0
        target.pick(item)
        #expect(chosen == .tempo)
    }

    @Test func voiceOverReadsEachValue() throws {
        let (_, m, v) = try chart()
        defer { withExtendedLifetime(m) {} }
        let labels = v.accessibilityItems().map(\.label)
        #expect(labels == ["Velocity 64, bar 1 beat 1", "Velocity 64, bar 1 beat 2"])
        #expect(v.accessibilityName == "Strip Chart")
    }
}
