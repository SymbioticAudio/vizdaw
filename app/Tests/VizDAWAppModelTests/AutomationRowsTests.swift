import Foundation
import Testing
import VizDAWCore
@testable import VizDAWAppModel

private func automationEvent(_ id: UInt64, _ target: AutomationTargetRow, _ value: Double) -> EventRow {
    var r = ListInsert.blankRow(kind: .automation, tick: 0)
    r.id = id
    r.automation = AutomationRow(target: target, value: value)
    r.value = Int64((value * Double(UInt32.max)).rounded())
    return r
}

@Suite struct AutomationRowsTests {
    @Test func automationRowsShowTheirTargetAndPercent() {
        let e = automationEvent(4, .sendLevel(strip: 3, send: 1), 0.25)
        let row = ListRow(.event(lane: listLaneA, row: e))
        let c = listContext()
        #expect(row.kind == .automation)
        #expect(ListText.cell(row, .kind, c) == "Automation")
        #expect(ListText.cell(row, .data1, c) == "Send 2 · Strip 3")
        #expect(ListText.cell(row, .data2, c) == "25.0%")
        #expect(ListText.editable(row, .data2) && !ListText.editable(row, .data1))
        #expect(ListText.summary(row, c) == "Automation, bar 1 beat 1, Send 2 · Strip 3, 25.0%")
        #expect(!ListKind.automation.isMidi)
        #expect(ListKind(.automation) == .automation && ListKind.automation.eventKind == .automation)
        #expect(AutomationText.short(.pluginParam(slot: 2, param: 7)) == "Parameter 7 · Plugin 2")
        #expect(AutomationText.short(.velocityScale(dest: 5)) == "Velocity Scale · Destination 5")
    }

    @Test func typingAPercentReplacesTheValue() throws {
        let e = automationEvent(4, .stripFader(strip: 1), 0.25)
        let row = ListRow(.event(lane: listLaneA, row: e))
        let change = try ListEdit.commit("50", column: .data2, row: row, context: listContext())
        var expected = e
        expected.automation = AutomationRow(target: .stripFader(strip: 1), value: 0.5)
        expected.value = Int64((0.5 * Double(UInt32.max)).rounded())
        #expect(change == .edit(.event(edit: .replace(event: 4, row: expected)), tick: 0))
        #expect(throws: ListEditError.self) {
            try ListEdit.commit("150", column: .data2, row: row, context: listContext())
        }
    }

    /// Task 6 (groups-vcas-transport): a VCA's fader is named as the core
    /// names its track, its values read in dB, it has a Strip Chart menu,
    /// and the mixer polls its heard value while playing.
    @MainActor @Test func vcaFaderTargetsAreNamed() throws {
        let t = AutomationTargetRow.vcaFader(vca: 3)
        var names = StripNames()
        names.vcas[3] = "Drums"
        #expect(StripLane.automation(AutoTarget(t)).title(names) == "VCA Drums Fader")
        #expect(AutoTarget(t).row == t)
        #expect(AutomationText.short(t) == "VCA 3 Fader")
        #expect(AutomationLaw.text(.vcaFader(vca: 3), AutomationLaw.faderNorm(-6)) == "-6.0 dB")
        names.strips[1] = "Synth"
        #expect(names.knows(.vcaFader(vca: 3)) && !names.knows(.vcaFader(vca: 4)), "a deleted VCA is missing")

        let f = mixerFake()
        f.mixerSnap.vcas = [
            VcaRow(
                id: 3, name: "Drums", colour: .blue, members: [1], gainDb: 0, mute: false, solo: false,
                automation: .read)
        ]
        let loaded = try AutomationTargets.load(f)
        #expect(loaded.names.vcas[3] == "Drums")
        #expect(loaded.menus.last == StripMenu(title: "VCAs", lanes: [.automation(.vcaFader(vca: 3))]))

        let m = try MixerModel(document: DocumentModel(session: f))
        f.heard = [t: AutomationLaw.faderNorm(-6)]
        try f.play(seq: 1)
        m.document.poll()
        m.pollAutomation()
        #expect(f.heardCalls.last?.contains(t) == true, "the VCA's fader is polled while playing")
    }
}
