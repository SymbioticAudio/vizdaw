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
        let t = try AutomationTargets.load(f)
        #expect(t.menus.map(\.title) == ["Synth", "Gain · Synth", "Bus 1", "Master", "Velocity Scale"])
        #expect(t.menus[0].lanes == [.automation(.fader(strip: 1)), .automation(.pan(strip: 1)), .automation(.mute(strip: 1)), .automation(.solo(strip: 1))])
        #expect(t.menus[1].lanes == [.automation(.param(slot: 10, param: 4))], "hidden parameters are left out")
        #expect(!t.menus[3].lanes.contains(.automation(.pan(strip: 0))), "master has no pan")
        #expect(t.names.title(.param(slot: 10, param: 4)) == "Gain · Gain")
        #expect(t.names.dests[2] == "Piano")
    }

    @Test func savedRowsOfDeletedTargetsAreDropped() throws {
        // Parked T12: a row saved for a strip or slot that is gone.
        let f = mixerFake()
        let m = try DocumentModel(session: f)
        defer { withExtendedLifetime(m) {} }
        m.editor.openDetail(.lane(seq: 3, lane: 31), kind: .stripChart)
        let s = StripChartModel.pane(m)
        m.editor.updateLayout {
            $0.detail?.stripRows = [
                .automation(.fader(strip: 1)), .automation(.fader(strip: 99)), .automation(.param(slot: 77, param: 1)),
                .controller(7),
            ]
        }
        #expect(s.rows == [.automation(.fader(strip: 1)), .controller(7)])
        s.setRow(1, to: .pitchBend)
        #expect(s.rows == [.automation(.fader(strip: 1)), .pitchBend], "edits act on the rows shown")
        m.editor.updateLayout { $0.detail?.stripRows = [.automation(.mute(strip: 99))] }
        #expect(s.rows == [.velocity])
    }

    @Test func aBusyReadIsNotCachedAndTheNextCallFillsTheMenus() throws {
        let f = mixerFake()
        let m = try DocumentModel(session: f)
        m.editor.openDetail(.lane(seq: 3, lane: 31), kind: .stripChart)
        let s = StripChartModel.pane(m)
        f.mixerFetchError = VizError.Busy(message: "busy")
        #expect(s.menus.isEmpty)
        f.mixerFetchError = nil
        #expect(s.menus.map(\.title).first == "Synth", "retried without a version bump")
        f.mixerFetchError = VizError.Busy(message: "busy")
        #expect(!s.menus.isEmpty, "a later Busy keeps the previous menus")
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

    @Test func aRefusedStrokeLeavesTheSelectionAlone() throws {
        // Final review Minor 3: another editor's gesture is open, so the
        // draw is refused and the selection keeps its lanes.
        let (f, m, s) = try make()
        defer { withExtendedLifetime(m) {} }
        let before = SelectionRow(timeRange: nil, lanes: [LaneRef(seq: 3, lane: 32)], events: [3])
        m.editor.setSelection(before)
        #expect(m.editor.beginGesture("Other", lanes: [LaneKey(3, 32)], origin: .notes))
        let sets = f.selectionSets.count
        s.strokeBegan(row: 0, at: StripPoint(tick: 0, value: 100), selectedOnly: false)
        s.strokeMoved(to: StripPoint(tick: q, value: 200))
        #expect(m.editor.status == EditorModel.gestureOpenMessage)
        #expect(f.selectionSets.count == sets, "no selection change")
        #expect(m.editor.selection == before)
        #expect(m.editor.gesture?.label == "Other")
        m.editor.cancelGesture()
    }

    @Test func aNewStrokeCancelsOneWhoseMouseUpWasLost() throws {
        // Parked T12: a lost mouse-up must not orphan the open gesture.
        let (f, m, s) = try make()
        defer { withExtendedLifetime(m) {} }
        s.strokeBegan(row: 0, at: StripPoint(tick: 0, value: 1), selectedOnly: false)
        s.strokeMoved(to: StripPoint(tick: q, value: 2))
        #expect(m.editor.gesture != nil)
        s.strokeBegan(row: 0, at: StripPoint(tick: 2 * q, value: 3), selectedOnly: false)
        #expect(f.calls.last == "abort" && m.editor.gesture == nil)
        s.strokeMoved(to: StripPoint(tick: 3 * q, value: 4))
        #expect(m.editor.gesture?.label == "Change Velocity", "the new stroke draws")
        s.strokeEnded(at: StripPoint(tick: 3 * q, value: 4))
        #expect(m.editor.gesture == nil && f.calls.last == "end")
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

    @Test func aTornWindowHasItsOwnStripChart() throws {
        let (_, m, s) = try make()
        s.setRow(0, to: .tempo)
        let w = try #require(m.editor.tearOff(.stripChart, shown: nil))
        let ws = m.stripChart(.window(w.id))
        #expect(ws.rows == [.tempo], "a window starts with the pane's rows")
        ws.setRow(0, to: .program)
        #expect(s.rows == [.tempo] && ws.rows == [.program])
        #expect(m.stripChart(.window(w.id)) === ws)
        m.dropStripChart(.window(w.id))
        #expect(m.stripChart(.window(w.id)) !== ws, "a closed window's chart goes")
    }

    @Test func aChartWhoseSlotStopsShowingOneGoes() throws {
        // Parked T14: a window switched to another editor, or closed, does
        // not keep its Strip Chart (nor an open stroke) until it closes.
        let (f, m, _) = try make()
        let pane = m.stripChart(.pane)
        let w = try #require(m.editor.tearOff(.stripChart, shown: nil))
        let ws = m.stripChart(.window(w.id))
        ws.strokeBegan(row: 0, at: StripPoint(tick: 0, value: 1), selectedOnly: false)
        ws.strokeMoved(to: StripPoint(tick: q, value: 2))
        #expect(m.editor.gesture != nil)
        m.editor.setWindowKind(w.id, .list)
        #expect(m.editor.gesture == nil && f.calls.last == "abort", "its stroke went with it")
        m.editor.setWindowKind(w.id, .stripChart)
        #expect(m.stripChart(.window(w.id)) !== ws)
        let again = m.stripChart(.window(w.id))
        m.editor.closeWindow(w.id)
        #expect(m.stripChart(.window(w.id)) !== again, "closing drops it too")
        #expect(m.stripChart(.pane) === pane, "the pane's stays while it shows the chart")
    }

    @Test func barPickersKeepAnOutOfListValue() {
        #expect(StripBarChoices.steps(MeterMath.ppq / 8).count == StripBarChoices.steps.count)
        #expect(StripBarChoices.steps(100).last! == ("100 ticks", 100))
        #expect(StripBarChoices.musicalPeriods(7).last! == ("7 ticks", 7))
        #expect(StripBarChoices.clockPeriods(333).last! == ("333 ms", 333))
        #expect(StripBarChoices.clockPeriods(500).count == StripBarChoices.clockPeriods.count)
        #expect(StripBarChoices.barPeriods(3).last! == ("3 bars", 3))
        #expect(StripBarChoices.barPeriods(1).first! == ("1 bar", 1))
        #expect(StripBarChoices.phases(5).last! == ("1.8°", 5))
        #expect(StripBarChoices.phases(125).map(\.0) == ["0°", "45°", "90°", "180°", "270°"])
    }
}
