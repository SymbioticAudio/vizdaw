import Foundation
import Testing
import VizDAWCore
@testable import VizDAWAppModel

private let q: Int64 = 15_360

private func pt(_ t: Int64, _ v: Int64) -> StripPoint { StripPoint(tick: t, value: v) }

@Suite struct StripChartTests {
    @Test func aBackwardDragIsTurnedAround() {
        let s = StripChartSettings()
        let l = StripLineBuilder.line(from: pt(2 * q, 10), to: pt(0, 50), stroke: [], settings: s, seed: 1)
        #expect(l == LineRow(from: 0, to: 2 * q, start: 50, end: 10, step: q / 8, shape: .straight))
    }

    @Test func aClickDrawsOnePoint() {
        let l = StripLineBuilder.line(from: pt(q, 7), to: pt(q, 7), stroke: [], settings: StripChartSettings(), seed: 1)
        #expect(l.from == q && l.to == q && l.start == 7)
        let cmd = StripCommands.line(.controller(7), op: .draw, line: l, selectedOnly: false)
        #expect(
            cmd
                == .selectAndModify(
                    rule: RuleRow(
                        conditions: [ConditionRow(test: .timeRange(start: q, end: q + 1), negate: false)],
                        matchAll: true),
                    mode: .select, modify: .line(target: .series(series: .controller(index: 7)), op: .draw, line: l),
                    toCopy: false))
    }

    @Test func shapesComeFromTheSettings() {
        var s = StripChartSettings()
        s.shape = .flat
        let flat = StripLineBuilder.line(from: pt(q, 30), to: pt(2 * q, 90), stroke: [], settings: s, seed: 1)
        #expect(flat.start == 30 && flat.end == 30 && flat.shape == .flat, "flat holds the pressed value")
        s.shape = .curved
        s.curve = .sCurve
        #expect(StripLineBuilder.line(from: pt(0, 0), to: pt(q, 1), stroke: [], settings: s, seed: 1).shape == .curved(curve: .sCurve))
        s.shape = .random
        #expect(StripLineBuilder.line(from: pt(0, 0), to: pt(q, 1), stroke: [], settings: s, seed: 42).shape == .random(seed: 42))
        s.shape = .oscillator
        s.wave = .triangle
        s.period = .clock
        s.periodMs = 250
        s.phase = 5000
        #expect(
            StripLineBuilder.line(from: pt(0, 0), to: pt(q, 1), stroke: [], settings: s, seed: 1).shape
                == .oscillator(wave: .triangle, period: .millis(ms: 250), phase: 999))
        s.period = .bars
        s.periodBars = 0
        #expect(s.periodRow == .bars(bars: 1), "a period of 0 becomes 1")
        s.shape = .free
        let free = StripLineBuilder.line(
            from: pt(0, 5), to: pt(3 * q, 9), stroke: [pt(0, 5), pt(2 * q, 7), pt(q, 1), pt(q, 2), pt(3 * q, 9)],
            settings: s, seed: 1)
        #expect(
            free
                == LineRow(
                    from: 0, to: 3 * q, start: 5, end: 9, step: q / 8,
                    shape: .free(points: [
                        LinePointRow(tick: 0, value: 5), LinePointRow(tick: q, value: 2),
                        LinePointRow(tick: 2 * q, value: 7), LinePointRow(tick: 3 * q, value: 9),
                    ])))
    }

    @Test func commandsFollowTheModeOperationAndModifier() {
        var s = StripChartSettings()
        #expect(StripCommands.op(.controller(1), settings: s) == .draw)
        #expect(StripCommands.op(.velocity, settings: s) == .set, "drawing a note value sets it")
        s.mode = .modify
        s.op = .scale
        #expect(StripCommands.op(.velocity, settings: s) == .scale)
        let l = LineRow(from: 0, to: q, start: 0, end: 1, step: 1, shape: .straight)
        if case let .selectAndModify(_, mode, _, _) = StripCommands.line(.velocity, op: .set, line: l, selectedOnly: true) {
            #expect(mode == .refine, "⌥ limits the line to the selected events")
        } else {
            Issue.record("expected selectAndModify")
        }
        #expect(StripCommands.label(.controller(1), op: .draw) == "Draw Controllers")
        #expect(StripCommands.label(.tempo, op: .draw) == "Draw Tempo")
        #expect(StripCommands.label(.automation(.fader(strip: 1)), op: .draw) == "Draw Automation")
        #expect(StripCommands.label(.velocity, op: .set) == "Change Velocity")
    }

    @Test func aZeroWidthCompressIsOneTick() {
        let c = StripCommands.compress(.velocity, from: pt(q, 900), to: pt(q, 100), step: 0, selectedOnly: false)
        let line = LineRow(from: q, to: q, start: 100, end: 900, step: 1, shape: .flat)
        #expect(
            c
                == .selectAndModify(
                    rule: RuleRow(
                        conditions: [ConditionRow(test: .timeRange(start: q, end: q + 1), negate: false)],
                        matchAll: true),
                    mode: .select, modify: .line(target: .value(target: .velocity), op: .compress, line: line),
                    toCopy: false))
    }

    @Test func eraseDeletesTheRowsSeriesOnly() {
        #expect(
            StripCommands.erase(.pitchBend, from: q, to: 0)
                == .selectAndModify(
                    rule: StripLane.pitchBend.eraseRule(from: 0, to: q), mode: .select, modify: .delete, toCopy: false))
        #expect(StripCommands.erase(.velocity, from: 0, to: q) == nil)
    }

    @Test func exactModeOpensTheLineInThePanel() {
        let l = LineRow(from: q, to: 2 * q, start: 0, end: 100, step: 1, shape: .straight)
        let cmd = StripCommands.line(.controller(7), op: .add, line: l, selectedOnly: false)
        let r = ExactMode.request(for: cmd, lanes: [LaneKey(3, 31)], origin: .stripChart)
        #expect(r?.task == .change)
        #expect(r?.config.modify.op == .line(target: .series(series: .controller(index: 7)), op: .add, line: l))
        #expect(r?.config.select.enabled == true && r?.config.select.conditions.count == 1)
        #expect(r?.events == nil && r?.lanes == [LaneKey(3, 31)])
        #expect(r?.range == TickRange(start: q, end: 2 * q + 1))
        #expect(ExactMode.request(for: .event(edit: .delete(events: [1])), lanes: [], origin: .stripChart) == nil)
    }

    @Test func settingsDecodeTolerantly() throws {
        let json = #"{"mode":"modify","rowHeight":5000,"shape":"bogus","periodMs":250}"#
        let s = try JSONDecoder().decode(StripChartSettings.self, from: Data(json.utf8))
        #expect(s.mode == .modify && s.rowHeight == 400 && s.shape == .straight && s.periodMs == 250)
        var l = EditorLayout()
        l.stripChart.op = .thin
        #expect(EditorLayout.decode(l.encoded()).stripChart.op == .thin)
        #expect(EditorLayout.decode("{}").stripChart == StripChartSettings())
    }
}
