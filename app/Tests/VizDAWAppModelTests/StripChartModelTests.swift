import Foundation
import Testing
import VizDAWCore
@testable import VizDAWAppModel

private let q: Int64 = 15_360

@MainActor
@Suite struct StripChartModelTests {
    func make(_ target: EditorTarget = .lane(seq: 3, lane: 31)) throws -> (FakeSession, DocumentModel, StripChartModel) {
        let f = rollFake()
        let m = try DocumentModel(session: f)
        m.editor.openDetail(target, kind: .stripChart)
        let s = StripChartModel.pane(m)
        s.nextSeed = { 7 }
        f.calls = []
        return (f, m, s)
    }

    @Test func showsTheTargetLaneAndItsRows() throws {
        let (_, m, s) = try make()
        defer { withExtendedLifetime(m) {} }
        #expect(s.lane == LaneKey(3, 31))
        #expect(s.rows == [.velocity])
        #expect(s.values(.velocity).map(\.id) == [1, 2])
        s.addRow()
        #expect(s.rows == [.velocity, .controller(7)])
        s.setRow(1, to: .pitchBend)
        s.removeRow(0)
        #expect(s.rows == [.pitchBend])
        s.removeRow(0)
        #expect(s.rows == [.pitchBend], "one row stays")
        for _ in 0..<6 { s.addRow() }
        #expect(s.rows.count == StripChartModel.maxRows)
        // The rows are saved with the detail state; another editor keeps them.
        m.editor.openDetail(.lane(seq: 3, lane: 31), kind: .pianoRoll)
        #expect(m.editor.layout.detail?.stripRows?.first == .pitchBend)
    }

    @Test func aSequenceTargetShowsTheChosenLaneElseTheFirst() throws {
        let (_, m, s) = try make(.sequence(3))
        defer { withExtendedLifetime(m) {} }
        #expect(s.lane == LaneKey(3, 30))
        s.chosenLane = LaneKey(3, 32)
        #expect(s.lane == LaneKey(3, 32))
        s.chosenLane = LaneKey(3, 99)
        #expect(s.lane == LaneKey(3, 30), "a lane that is gone falls back")
    }

    @Test func automationTargetsComeFromTheMixer() throws {
        let f = mixerFake()
        f.paramTable[10] = [
            ParamRow(
                id: 4, name: "Gain", module: "", min: 0, max: 2, default: 1, value: 1, stepped: false,
                automatable: true, hidden: false),
            ParamRow(
                id: 5, name: "Secret", module: "", min: 0, max: 1, default: 0, value: 0, stepped: false,
                automatable: true, hidden: true),
        ]
        f.dests = [DestRow(id: 2, name: "Piano", plugin: nil)]
        let t = AutomationTargets.load(f)
        #expect(t.menus.map(\.title) == ["Synth", "Gain · Synth", "Bus 1", "Master", "Velocity Scale"])
        #expect(t.menus[0].lanes == [.automation(.fader(strip: 1)), .automation(.pan(strip: 1)), .automation(.mute(strip: 1)), .automation(.solo(strip: 1))])
        #expect(t.menus[1].lanes == [.automation(.param(slot: 10, param: 4))], "hidden parameters are left out")
        #expect(!t.menus[3].lanes.contains(.automation(.pan(strip: 0))), "master has no pan")
        #expect(t.names.title(.param(slot: 10, param: 4)) == "Gain · Gain")
        #expect(t.names.dests[2] == "Piano")
    }

    @Test func aDrawDragIsOneGestureOnTheShownLane() throws {
        let (f, m, s) = try make()
        defer { withExtendedLifetime(m) {} }
        s.strokeBegan(row: 0, at: StripPoint(tick: 0, value: 100), selectedOnly: false)
        #expect(f.calls.isEmpty, "nothing opens before the pointer moves")
        s.strokeMoved(to: StripPoint(tick: q, value: 200))
        #expect(f.selectionSets.last?.lanes == [LaneRef(seq: 3, lane: 31)])
        #expect(m.editor.gesture?.label == "Change Velocity")
        let line = LineRow(from: 0, to: q, start: 100, end: 200, step: q / 8, shape: .straight)
        #expect(f.gestureEdits.last == StripCommands.line(.velocity, op: .set, line: line, selectedOnly: false))
        s.strokeEnded(at: StripPoint(tick: q, value: 200))
        #expect(m.editor.gesture == nil && f.calls.last == "end")
        #expect(s.stroke == nil)
    }

    @Test func aClickInDrawModeDrawsOnePoint() throws {
        let (f, m, s) = try make()
        defer { withExtendedLifetime(m) {} }
        s.setRow(0, to: .controller(1))
        s.strokeBegan(row: 0, at: StripPoint(tick: q, value: 5), selectedOnly: false)
        s.strokeEnded(at: StripPoint(tick: q, value: 5))
        let line = LineRow(from: q, to: q, start: 5, end: 5, step: q / 8, shape: .straight)
        #expect(f.gestureEdits.last == StripCommands.line(.controller(1), op: .draw, line: line, selectedOnly: false))
        #expect(f.calls.first == "begin Draw Controllers" && f.calls.last == "end")
        // A click in Modify mode does nothing.
        s.updateSettings { $0.mode = .modify }
        f.calls = []
        s.strokeBegan(row: 0, at: StripPoint(tick: q, value: 5), selectedOnly: false)
        s.strokeEnded(at: StripPoint(tick: q, value: 5))
        #expect(f.calls.isEmpty)
    }

    @Test func aModifyLineKeepsTheSelectedLanesAndOptionRefines() throws {
        let (f, m, s) = try make()
        defer { withExtendedLifetime(m) {} }
        m.editor.setSelection(SelectionRow(timeRange: nil, lanes: [LaneRef(seq: 3, lane: 32)], events: [3]))
        let sets = f.selectionSets.count
        s.updateSettings {
            $0.mode = .modify
            $0.op = .add
        }
        s.strokeBegan(row: 0, at: StripPoint(tick: 0, value: -5), selectedOnly: true)
        s.strokeMoved(to: StripPoint(tick: q, value: 5))
        #expect(f.selectionSets.count == sets, "the selection's lanes stay")
        #expect(m.editor.gesture?.lanes == [LaneKey(3, 32)])
        if case let .selectAndModify(_, mode, _, _)? = f.gestureEdits.last {
            #expect(mode == .refine)
        } else {
            Issue.record("no line")
        }
        s.strokeCancelled()
    }

    @Test func escapeLeavesNoUndoStep() throws {
        let (f, m, s) = try make()
        defer { withExtendedLifetime(m) {} }
        s.strokeBegan(row: 0, at: StripPoint(tick: 0, value: 1), selectedOnly: false)
        s.strokeMoved(to: StripPoint(tick: q, value: 2))
        s.strokeCancelled()
        #expect(f.calls.last == "abort" && m.editor.gesture == nil && s.stroke == nil)
    }

    @Test func eraseOnANoteRowSaysWhy() throws {
        let (f, m, s) = try make()
        defer { withExtendedLifetime(m) {} }
        s.updateSettings { $0.mode = .erase }
        s.strokeBegan(row: 0, at: StripPoint(tick: 0, value: 1), selectedOnly: false)
        s.strokeMoved(to: StripPoint(tick: q, value: 2))
        #expect(m.editor.status == StripChartModel.eraseNotesMessage)
        #expect(f.calls.isEmpty && m.editor.gesture == nil)
        s.setRow(0, to: .controller(7))
        s.strokeBegan(row: 0, at: StripPoint(tick: 0, value: 1), selectedOnly: false)
        s.strokeMoved(to: StripPoint(tick: q, value: 2))
        #expect(m.editor.gesture?.label == "Erase Events")
        #expect(f.gestureEdits.last == StripCommands.erase(.controller(7), from: 0, to: q))
        s.strokeEnded(at: StripPoint(tick: q, value: 2))
    }

    @Test func exactModeOpensThePanelInsteadOfApplying() throws {
        let (f, m, s) = try make()
        defer { withExtendedLifetime(m) {} }
        var opened = 0
        m.selectModify.onPresent = { opened += 1 }
        m.editor.exactMode = true
        s.setRow(0, to: .controller(7))
        s.strokeBegan(row: 0, at: StripPoint(tick: 0, value: 0), selectedOnly: false)
        s.strokeMoved(to: StripPoint(tick: q, value: 99))
        s.strokeEnded(at: StripPoint(tick: q, value: 99))
        #expect(f.calls.last == "abort", "previewed, then put back")
        #expect(opened == 1 && m.selectModify.task == .change)
        if case .line(let target, let op, _)? = m.selectModify.config.modification {
            #expect(target == .series(series: .controller(index: 7)) && op == .draw)
        } else {
            Issue.record("no line in the panel")
        }
    }
}
