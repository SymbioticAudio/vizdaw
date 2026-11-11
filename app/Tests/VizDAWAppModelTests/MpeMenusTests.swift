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

    @Test func choicesAreOneEditEach() throws {
        let f = editorFake()
        f.laneTable[3]?[1].input = .midi(port: "MPE Keys", channel: nil)
        f.dests = [DestRow(id: 4, name: "Synth", plugin: nil, external: true, returnLatencyMs: 0, mpe: nil)]
        let m = try DocumentModel(session: f)
        m.select(3)
        var edits = f.edits.count
        let c = MpeMenus.input(m.lanes[1])[0].choices[2]
        MpeMenus.chooseInput(c, seq: 3, lane: 31, m)
        #expect(f.edits.count == edits + 1)
        #expect(f.edits.last == .lane(edit: .setMpe(seq: 3, lane: 31, mpe: MpeRow(zone: .upper, members: 15, bendRange: 48))))
        edits = f.edits.count
        MpeMenus.chooseInput(c, seq: 3, lane: 31, m)
        #expect(f.edits.count == edits, "the checked item again is no step")
        MpeMenus.chooseOutput(MpeChoice(title: "Lower Zone", mpe: full, checked: false), dest: 4, m)
        #expect(f.mpeCalls.count == 1 && f.mpeCalls.last?.0 == 4 && f.mpeCalls.last?.1 == full)
        MpeMenus.chooseOutput(MpeChoice(title: "Lower Zone", mpe: full, checked: true), dest: 4, m)
        #expect(f.mpeCalls.count == 1, "re-choosing the checked output item is no step")
        // The header's input menu shows the section.
        let h = LaneHeaderView(document: m)
        let titles = h.inputMenu(seq: 3, lane: m.lanes[1]).items.map(\.title)
        #expect(titles.contains("MPE Input") && titles.contains("Upper Zone"))
    }

    @Test func aStaleMenuAppliesToTheCurrentSetting() throws {
        let f = editorFake()
        f.laneTable[3]?[1].input = .midi(port: "MPE Keys", channel: nil)
        f.laneTable[3]?[1].mpe = MpeRow(zone: .lower, members: 15, bendRange: 48)
        f.dests = [DestRow(id: 4, name: "Synth", plugin: nil, external: true, returnLatencyMs: 0, mpe: full)]
        let m = try DocumentModel(session: f)
        m.select(3)
        let inSections = MpeMenus.input(m.lanes[1])
        let outSections = MpeMenus.output(f.dests[0])
        // The settings change after the menus were built.
        let moved = MpeRow(zone: .upper, members: 4, bendRange: 12)
        f.laneTable[3]?[1].mpe = moved
        f.dests[0].mpe = moved
        f.snap.version += 1
        m.apply(f.snap)
        let members7 = inSections[1].choices[6]
        MpeMenus.chooseInput(members7, seq: 3, lane: 31, m)
        #expect(f.edits.last == .lane(edit: .setMpe(seq: 3, lane: 31, mpe: MpeRow(zone: .upper, members: 7, bendRange: 12))))
        MpeMenus.chooseOutput(outSections[2].choices[4], dest: 4, m)
        #expect(f.mpeCalls.last?.1 == MpeRow(zone: .upper, members: 4, bendRange: 96))
        // A zone choice keeps the current members and bend range.
        MpeMenus.chooseOutput(outSections[0].choices[1], dest: 4, m)
        #expect(f.mpeCalls.last?.1 == MpeRow(zone: .lower, members: 4, bendRange: 96))
    }

    @Test func theDestinationMenuOffersMpeForAnyDestinationTheTrackPlays() throws {
        let f = editorFake()
        let plugin = DestRow(id: 5, name: "Pad", plugin: nil, external: false, returnLatencyMs: 0, mpe: nil)
        f.dests = [plugin]
        f.laneTable[3]?[1].midiDest = 5
        let m = try DocumentModel(session: f)
        m.select(3)
        let h = LaneHeaderView(document: m)
        var titles = h.destinationMenu(seq: 3, lane: m.lanes[1]).items.map(\.title)
        #expect(titles.contains("MPE Output: Pad") && titles.contains("Lower Zone"))
        var none = m.lanes[1]
        none.midiDest = 99
        titles = h.destinationMenu(seq: 3, lane: none).items.map(\.title)
        #expect(!titles.contains { $0.hasPrefix("MPE Output") }, "no row for the track's destination: no section")
        none.midiDest = nil
        titles = h.destinationMenu(seq: 3, lane: none).items.map(\.title)
        #expect(!titles.contains { $0.hasPrefix("MPE Output") })
    }
}
