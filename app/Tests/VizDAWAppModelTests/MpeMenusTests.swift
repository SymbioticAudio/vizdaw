import AppKit
import Testing
import VizDAWCore
@testable import VizDAWAppModel
@testable import VizDAWEditors

/// MPE in the track header (plan 5g-2 Task 8; Ruling 11, P11): the input
/// and destination menus' MPE sections, one undo step per choice, and the
/// input title.
@MainActor
@Suite struct MpeMenusTests {
    let full = MpeRow(zone: .lower, members: 15, bendRange: 48)

    @Test func theInputSectionsFollowTheLane() {
        var l = lane(31, "Chords")
        #expect(MpeMenus.input(l).isEmpty, "no MIDI input: no MPE")
        l.input = .midi(port: "MPE Keys", channel: nil)
        let off = MpeMenus.input(l)
        #expect(off.map(\.title) == ["MPE Input"])
        #expect(off[0].choices.map(\.title) == ["Off", "Lower Zone", "Upper Zone"])
        #expect(off[0].choices.map(\.checked) == [true, false, false])
        #expect(off[0].choices[1].mpe == full)
        l.mpe = MpeRow(zone: .upper, members: 7, bendRange: 24)
        let on = MpeMenus.input(l)
        #expect(on.map(\.title) == ["MPE Input", "Member Channels", "Pitch Bend Range"])
        #expect(on[0].choices.map(\.checked) == [false, false, true])
        #expect(on[1].choices.count == 15)
        #expect(on[1].choices[6].checked && on[1].choices[6].mpe == MpeRow(zone: .upper, members: 7, bendRange: 24))
        #expect(on[1].choices[0].mpe == MpeRow(zone: .upper, members: 1, bendRange: 24))
        #expect(on[2].choices.map(\.title) == ["2", "12", "24", "48", "96"])
        #expect(on[2].choices[2].checked)
        #expect(LaneInputs.title(lane: l) == "MPE Keys · MPE")
        l.input = .midi(port: "MPE Keys", channel: 2)
        #expect(LaneInputs.title(lane: l) == "MPE Keys · MPE", "the channel is ignored under MPE")
        l.mpe = nil
        #expect(LaneInputs.title(lane: l) == "MPE Keys · Ch 3")
        #expect(LaneInputs.title(lane: l) == LaneInputs.title(l.input))
    }

    @Test func theOutputSectionsNameTheDestination() {
        let d = DestRow(id: 4, name: "Synth", plugin: nil, external: true, returnLatencyMs: 0, mpe: nil)
        let s = MpeMenus.output(d)
        #expect(s.map(\.title) == ["MPE Output: Synth"])
        #expect(s[0].choices.map(\.title) == ["Off", "Lower Zone", "Upper Zone"])
    }

    @Test func aPluginDestinationGetsTheOutputSectionToo() {
        let d = DestRow(id: 5, name: "Pad", plugin: nil, external: false, returnLatencyMs: 0, mpe: full)
        let s = MpeMenus.output(d)
        #expect(s.map(\.title) == ["MPE Output: Pad", "Member Channels", "Pitch Bend Range"])
    }

    @Test func choicesAreOneEditEach() throws {
        let f = editorFake()
        f.laneTable[3]?[1].input = .midi(port: "MPE Keys", channel: nil)
        let m = try DocumentModel(session: f)
        m.select(3)
        let c = MpeMenus.input(m.lanes[1])[0].choices[2]
        MpeMenus.chooseInput(c, lane: m.lanes[1], m)
        #expect(f.edits.last == .lane(edit: .setMpe(seq: 3, lane: 31, mpe: MpeRow(zone: .upper, members: 15, bendRange: 48))))
        MpeMenus.chooseOutput(MpeChoice(title: "Lower Zone", mpe: full, checked: false), dest: 4, m)
        #expect(f.mpeCalls.last?.0 == 4 && f.mpeCalls.last?.1 == full)
        // The header's input menu shows the section.
        let h = LaneHeaderView(document: m)
        let titles = h.inputMenu(seq: 3, lane: m.lanes[1]).items.map(\.title)
        #expect(titles.contains("MPE Input") && titles.contains("Upper Zone"))
    }
}
