import CoreGraphics
import Foundation
import Testing
import VizDAWCore
@testable import VizDAWAppModel

private let q: Int64 = 15_360

func ccRow(_ id: UInt64, tick: Int64, index: UInt8, value: Int64) -> EventRow {
    var r = ListInsert.blankRow(kind: .cc, tick: tick)
    r.id = id
    r.value2 = Int64(index)
    r.value = value
    return r
}

func automationRow(_ id: UInt64, tick: Int64, _ target: AutomationTargetRow, _ v: Double) -> EventRow {
    var r = ListInsert.blankRow(kind: .automation, tick: tick)
    r.id = id
    r.automation = AutomationRow(target: target, value: v)
    r.value = AutomationLaw.v32(v)
    return r
}

@Suite struct StripLaneTests {
    @Test func lawsMatchTheCore() {
        #expect(AutomationLaw.faderDb(1) == 12)
        #expect(AutomationLaw.faderDb(0) == AutomationLaw.floorDb)
        #expect(abs(AutomationLaw.faderNorm(0) - 0.630_957) < 1e-6)
        #expect(abs(AutomationLaw.faderDb(AutomationLaw.faderNorm(-6)) + 6) < 1e-9)
        #expect(AutomationLaw.pan(0.25) == -0.5)
        #expect(AutomationLaw.velocityScalePercent(0.5) == 100 && AutomationLaw.velocityScalePercent(1) == 250)
        #expect(AutomationLaw.v32(1) == Int64(UInt32.max) && AutomationLaw.normalized(0) == 0)
        #expect(AutomationLaw.text(.fader(strip: 1), 0) == "-∞ dB")
        #expect(AutomationLaw.text(.fader(strip: 1), AutomationLaw.faderNorm(-6)) == "-6.0 dB")
        #expect(AutomationLaw.text(.pan(strip: 1), 0.5) == "C" && AutomationLaw.text(.pan(strip: 1), 0.25) == "L50")
        #expect(AutomationLaw.text(.mute(strip: 1), 0.7) == "On")
        let p = ParamRow(
            id: 3, name: "Cutoff", module: "", min: 20, max: 20_020, default: 20, value: 20, stepped: false,
            automatable: true, hidden: false)
        #expect(AutomationLaw.text(.param(slot: 2, param: 3), 0.5, param: p) == "1e+04")
        #expect(AutomationLaw.text(.velocityScale(dest: 1), 0.75) == "175%")
    }

    @Test func targetsRoundTripTheirRows() {
        let all: [AutoTarget] = [
            .fader(strip: 1), .pan(strip: 2), .mute(strip: 3), .solo(strip: 4), .send(strip: 5, index: 2),
            .param(slot: 6, param: 1 << 40), .velocityScale(dest: 7),
        ]
        for t in all { #expect(AutoTarget(t.row) == t) }
        #expect(AutoTarget.mute(strip: 1).isSwitch && !AutoTarget.pan(strip: 1).isSwitch)
        let data = try! JSONEncoder().encode(StripLane.automation(.send(strip: 5, index: 2)))
        #expect(try! JSONDecoder().decode(StripLane.self, from: data) == .automation(.send(strip: 5, index: 2)))
    }

    @Test func lanesReadTheirSeries() {
        let note = noteRow(1, tick: 0, duration: q)
        let events = [
            note, ccRow(2, tick: 0, index: 7, value: 100), ccRow(3, tick: q, index: 1, value: 5),
            automationRow(4, tick: q, .stripFader(strip: 9), 0.5), automationRow(5, tick: 2 * q, .stripPan(strip: 9), 1),
        ]
        #expect(StripLane.velocity.values(events, tempo: []) == [StripValue(id: 1, tick: 0, length: q, value: 0x8000, ramp: false)])
        #expect(StripLane.duration.values(events, tempo: []).map(\.value) == [q])
        #expect(StripLane.controller(7).values(events, tempo: []).map(\.id) == [2])
        let fader = StripLane.automation(.fader(strip: 9)).values(events, tempo: [])
        #expect(fader == [StripValue(id: 4, tick: q, length: 0, value: AutomationLaw.v32(0.5), ramp: true)])
        var prog = ListInsert.blankRow(kind: .program, tick: 0)
        prog.id = 6
        prog.value = 5
        #expect(StripLane.program.values([prog], tempo: []).map(\.value) == [5 << 25])
        let tempo = [TempoPointRow(tick: 0, bpm: 120, ramp: true), TempoPointRow(tick: q, bpm: 90, ramp: false)]
        #expect(
            StripLane.tempo.values([], tempo: tempo)
                == [
                    StripValue(id: nil, tick: 0, length: 0, value: 120_000, ramp: true),
                    StripValue(id: nil, tick: q, length: 0, value: 90_000, ramp: false),
                ])
        #expect(StripLane.automation(.mute(strip: 1)).style == .steps)
        #expect(StripLane.automation(.fader(strip: 1)).style == .ramps)
        #expect(StripLane.releaseVelocity.style == .bars && StripLane.controller(1).style == .steps)
    }

    @Test func lanesNameTargetsTextValuesAndBuildRules() {
        var names = StripNames()
        names.strips = [1: "Synth"]
        names.sends = [1: ["Reverb"]]
        names.slots = [6: "EQ"]
        names.params = [6: [
            ParamRow(
                id: 2, name: "Gain", module: "", min: 0, max: 2, default: 1, value: 1, stepped: false,
                automatable: true, hidden: false),
        ]]
        names.dests = [3: "Piano"]
        #expect(StripLane.automation(.fader(strip: 1)).title(names) == "Fader · Synth")
        #expect(StripLane.automation(.send(strip: 1, index: 0)).title(names) == "Send 1 to Reverb · Synth")
        #expect(StripLane.automation(.param(slot: 6, param: 2)).title(names) == "Gain · EQ")
        #expect(StripLane.automation(.velocityScale(dest: 3)).title(names) == "Velocity Scale · Piano")
        #expect(StripLane.automation(.fader(strip: 8)).title(names) == "Fader · Strip 8")
        #expect(StripLane.controller(7).title(names) == "Controller 7 (Volume)")
        #expect(StripLane.polyPressure(60).title(names) == "Poly Pressure C4")
        #expect(StripLane.velocity.text(Int64(Velocity7.up(100)), names) == "100")
        #expect(StripLane.pitchBend.text(Int64(Bend14.up(-8192)), names) == "-8192")
        #expect(StripLane.program.text(9 << 25, names) == "9")
        #expect(StripLane.tempo.text(120_500, names) == "120.5 BPM")
        #expect(StripLane.automation(.param(slot: 6, param: 2)).text(AutomationLaw.v32(0.25), names) == "0.5")
        let rule = StripLane.controller(7).eraseRule(from: 2 * q, to: q)
        #expect(
            rule?.conditions.map(\.test) == [
                .timeRange(start: q, end: 2 * q + 1), .kinds(kinds: [.cc]), .controller(low: 7, high: 7),
            ])
        #expect(StripLane.automation(.mute(strip: 1)).eraseRule(from: 0, to: 0)?.conditions.last?.test
            == .automation(target: .stripMute(strip: 1)))
        #expect(StripLane.velocity.eraseRule(from: 0, to: 1) == nil && StripLane.tempo.eraseRule(from: 0, to: 1) == nil)
        for lane in [StripLane.velocity, .controller(3), .tempo, .automation(.solo(strip: 2))] {
            #expect(StripLane(lane.lineTarget) == lane)
        }
    }

    @Test func scaleClampsOutsideTheRow() {
        let s = StripScale(range: 0...1000, top: 100, height: 106)
        #expect(s.y(1000) == 103 && s.y(0) == 203 && s.y(500) == 153)
        #expect(s.value(atY: 153) == 500)
        #expect(s.value(atY: -500) == 1000, "above the row is the top")
        #expect(s.value(atY: 900) == 0, "below the row is the bottom")
        #expect(s.value(atY: .nan) == 0)
        #expect(s.amount(atY: 103, op: .scale) == 200 && s.amount(atY: 153, op: .scale) == 100)
        #expect(s.amount(atY: 203, op: .add) == -500 && s.amount(atY: 103, op: .add) == 500)
        #expect(s.amount(atY: 153, op: .limitMax) == 500)
        #expect(StripOp.choices(for: .duration).contains(.legato))
        #expect(!StripOp.choices(for: .velocity).contains(.legato))
        #expect(StripOp.choices(for: .controller(1)).contains(.thin))
        #expect(!StripOp.choices(for: .velocity).contains(.thin))
        #expect(StripOp.compress.row == .compress && StripOp.limitMin.row == .limitMin)
    }
}
