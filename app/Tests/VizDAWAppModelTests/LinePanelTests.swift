import Foundation
import Testing
import VizDAWCore
@testable import VizDAWAppModel

private let q: Int64 = 15_360

@MainActor
@Suite struct LinePanelTests {
    func panel() throws -> (FakeSession, DocumentModel, SelectModifyModel) {
        let f = mixerFake()
        let m = try DocumentModel(session: f)
        let p = m.selectModify
        p.nextSeed = { 9 }
        p.open(.change)
        p.setModifyKind(.line)
        return (f, m, p)
    }

    func line(_ p: SelectModifyModel) -> (LineTargetRow, LineOpRow, LineRow)? {
        guard case let .line(t, op, l) = p.config.modify.op else { return nil }
        return (t, op, l)
    }

    @Test func linePickersEditTheLine() throws {
        let (_, m, p) = try panel()
        defer { withExtendedLifetime(m) {} }
        p.setLineTarget(.tempo)
        #expect(line(p)?.0 == .tempo && line(p)?.1 == .set)
        #expect(p.config.modify.op.fields[2].kind == .bpm)
        p.setLineOp(.draw)
        #expect(line(p)?.1 == .draw)
        p.setLineShape(.oscillator)
        #expect(line(p)?.2.shape == .oscillator(wave: .sine, period: .ticks(ticks: q), phase: 0))
        p.setLineWave(.saw)
        p.setLinePeriodKind(.bars)
        #expect(line(p)?.2.shape == .oscillator(wave: .saw, period: .bars(bars: 1), phase: 0))
        #expect(p.config.modify.op.fields.map(\.label).suffix(2) == ["period (bars)", "phase (‰)"])
        p.setLineShape(.random)
        #expect(line(p)?.2.shape == .random(seed: 9))
        p.setLineShape(.curved)
        p.setLineCurve(.easeIn)
        #expect(line(p)?.2.shape == .curved(curve: .easeIn))
        p.setLineShape(.free)
        let l = try #require(line(p)?.2)
        #expect(l.shape == .free(points: [LinePointRow(tick: l.from, value: l.start), LinePointRow(tick: l.to, value: l.end)]))
        #expect(ShapeKind(l.shape) == .free)
        p.setLineOp(.legato)
        p.setLineTarget(.velocity)
        #expect(line(p)?.1 == .set, "legato is for lengths only")
        p.setLineTarget(.duration)
        p.setLineOp(.legato)
        #expect(line(p)?.1 == .legato)
    }

    @Test func automationConditionsPickTheirTarget() throws {
        let (_, m, p) = try panel()
        defer { withExtendedLifetime(m) {} }
        p.addCondition(.automation)
        let i = p.config.select.conditions.count - 1
        p.setConditionTarget(.mute(strip: 1), at: i)
        #expect(p.config.select.conditions[i].test == .automation(target: .stripMute(strip: 1)))
        #expect(p.automationMenus.first?.title == "Synth")
        #expect(p.automationNames.title(.mute(strip: 1)) == "Mute · Synth")
    }

    @Test func automationTargetsRefreshWhenAPluginFinishesLoading() throws {
        // Final review Minor 4: the panel keys its target cache on the
        // slots version too, as the Strip Chart does.
        let (f, m, p) = try panel()
        defer { withExtendedLifetime(m) {} }
        #expect(!p.automationMenus.map(\.title).contains("Gain · Synth"), "not loaded yet")
        f.paramTable[10] = [
            ParamRow(
                id: 4, name: "Gain", module: "", min: 0, max: 2, default: 1, value: 1, stepped: false,
                automatable: true, hidden: false)
        ]
        f.idleUpdates = [IdleUpdate(snapshot: nil, slotsChanged: true, paramsChanged: [], problems: [])]
        m.idle()
        #expect(m.slotsVersion == 1)
        #expect(p.automationMenus.map(\.title).contains("Gain · Synth"))
        #expect(p.automationNames.title(.param(slot: 10, param: 4)) == "Gain · Gain")
    }

    @Test func shapeDefaultsComeFromTheStripChartBar() throws {
        // Parked T15: the bar's settings, not a fresh StripChartSettings.
        let (_, m, p) = try panel()
        defer { withExtendedLifetime(m) {} }
        m.editor.updateLayout {
            $0.stripChart.curve = .sCurve
            $0.stripChart.wave = .triangle
            $0.stripChart.period = .clock
            $0.stripChart.periodMs = 250
            $0.stripChart.periodBars = 4
        }
        p.setLineShape(.curved)
        #expect(line(p)?.2.shape == .curved(curve: .sCurve))
        p.setLineShape(.oscillator)
        #expect(line(p)?.2.shape == .oscillator(wave: .triangle, period: .millis(ms: 250), phase: 0))
        p.setLinePeriodKind(.bars)
        #expect(line(p)?.2.shape == .oscillator(wave: .triangle, period: .bars(bars: 4), phase: 0))
    }

    @Test func editingAFreeLinesRangeMovesItsPoints() {
        // Parked T15: From/To carry the drawn points along, in proportion.
        let free = LineRow(
            from: 0, to: 4 * q, start: 0, end: 10, step: q,
            shape: .free(points: [
                LinePointRow(tick: 0, value: 0), LinePointRow(tick: q, value: 5), LinePointRow(tick: 4 * q, value: 10),
            ]))
        let op = ModifyRow.line(target: .value(target: .velocity), op: .set, line: free)
        var n = op.numbers
        n[0] = 4 * q
        n[1] = 12 * q
        guard case .line(_, _, let moved) = op.with(n) else {
            Issue.record("not a line")
            return
        }
        #expect(moved.from == 4 * q && moved.to == 12 * q)
        #expect(
            moved.shape
                == .free(points: [
                    LinePointRow(tick: 4 * q, value: 0), LinePointRow(tick: 6 * q, value: 5),
                    LinePointRow(tick: 12 * q, value: 10),
                ]))
        // Unchanged range: the points stay put.
        guard case .line(_, _, let same) = op.with(op.numbers) else { return }
        #expect(same.shape == free.shape)
    }

    @Test func everyAutomationTargetCanBeAConditionAndPolyPressureALine() throws {
        let (_, m, p) = try panel()
        defer { withExtendedLifetime(m) {} }
        p.addCondition(.automation)
        let i = p.config.select.conditions.count - 1
        let all: [(AutoTarget, AutomationTargetRow)] = [
            (.fader(strip: 1), .stripFader(strip: 1)), (.pan(strip: 1), .stripPan(strip: 1)),
            (.solo(strip: 2), .stripSolo(strip: 2)), (.send(strip: 1, index: 3), .sendLevel(strip: 1, send: 3)),
            (.param(slot: 10, param: 4), .pluginParam(slot: 10, param: 4)), (.velocityScale(dest: 2), .velocityScale(dest: 2)),
        ]
        for (t, row) in all {
            p.setConditionTarget(t, at: i)
            #expect(p.config.select.conditions[i].test == .automation(target: row))
        }
        #expect(PanelKinds.all.contains(.automation), "the Kind condition offers automation")
        let octave = try #require(LineTargetChoices.polyPressure.first { $0.lanes.contains(.polyPressure(60)) })
        #expect(octave.lanes.count == 12 && LineTargetChoices.polyPressure.flatMap(\.lanes).count == 128)
        p.setLineTarget(.polyPressure(60))
        #expect(line(p)?.0 == StripLane.polyPressure(60).lineTarget)
    }

    @Test func applyingALineSendsOneSelectAndModify() throws {
        let (f, m, p) = try panel()
        defer { withExtendedLifetime(m) {} }
        m.editor.setSelection(SelectionRow(timeRange: nil, lanes: [LaneRef(seq: 3, lane: 31)], events: [1]))
        p.config.modify.toCopy = true
        #expect(p.apply())
        guard case let .selectAndModify(_, _, modify, toCopy)? = f.edits.last else {
            Issue.record("nothing sent")
            return
        }
        #expect(modify?.isLine == true && !toCopy, "a line never goes to a copy")
    }

    @Test func theKindConditionCanPickAutomation() throws {
        let (_, m, p) = try panel()
        defer { withExtendedLifetime(m) {} }
        p.addCondition(.kind)
        let i = p.config.select.conditions.count - 1
        p.setKinds([.note, .automation], at: i)
        #expect(p.config.select.conditions[i].test == .kinds(kinds: [.note, .automation]))
    }
}
