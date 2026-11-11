import Foundation
import Testing
import VizDAWCore
@testable import VizDAWAppModel

/// The strip's MIDI FX section (plan 5g-1 Task 9, Ruling 16; owner
/// 2026-11-09): which strips have it, which track it edits when one strip
/// serves several, and that a control-channel click opens nothing.
@MainActor
@Suite(.serialized) struct MidiFxSectionTests {
    let up12 = InputFxRow(kind: .transpose(semitones: 12), bypass: false)
    let down5 = InputFxRow(kind: .transpose(semitones: -5), bypass: false)

    /// Song (3): Keys A (30) and Keys B (31) play the built-in synth
    /// (destination 4, strip 1); Piano (32) plays a plugin instrument
    /// (destination 7, strip 5); Vocal (33) plays audio path 8 (strip 6).
    /// Bus 1 is strip 2, Master strip 0.
    func mixer(armB: Bool = false) throws -> (MixerModel, FakeSession) {
        let f = mixerFake()
        f.dests = [
            DestRow(id: 4, name: "Built-in Synth", plugin: nil),
            DestRow(
                id: 7, name: "Piano",
                plugin: PluginRefRow(format: .clap, id: "x.piano", name: "Piano", vendor: "V", version: "1")),
        ]
        f.mixerSnap.strips.insert(stripRow(5, "Piano", kind: .instrument(dest: 7)), at: 1)
        f.mixerSnap.strips.insert(stripRow(6, "Vocal", kind: .audioPath(path: 8)), at: 2)
        var a = lane(30, "Keys A")
        a.midiDest = 4
        var b = lane(31, "Keys B", kind: .instrument)
        b.midiDest = 4
        b.arm = armB
        var p = lane(32, "Piano")
        p.midiDest = 7
        var v = lane(33, "Vocal", kind: .audio)
        v.audioOut = 8
        f.laneTable[3] = [a, b, p, v]
        let m = try MixerModel(document: DocumentModel(session: f))
        m.document.select(3)
        return (m, f)
    }

    func strip(_ m: MixerModel, _ id: UInt64) throws -> StripRow {
        try #require(m.snapshot.strips.first { $0.id == id })
    }

    @Test func aStripTwoTracksShareEditsTheArmedOneInTheMixView() throws {
        let (m0, _) = try mixer()
        #expect(m0.midiFxLane(for: try strip(m0, 1), in: .view)?.id == 30, "none armed: the first")
        let (m, f) = try mixer(armB: true)
        let synth = try strip(m, 1)
        for p in [MixerPlacement.view, .pane, .window] {
            #expect(m.midiFxLane(for: synth, in: p)?.id == 31, "\(p): the armed one")
        }
        #expect(m.midiFxLane(for: try strip(m, 5), in: .view)?.id == 32, "a plugin instrument's own track")
        f.laneTable[3]?[1].inputFx = [up12]
        f.snap.version += 1
        m.document.refreshAfterJob()
        let edited = try #require(m.midiFxLane(for: synth, in: .view))
        #expect(InputFxModel.isLit(edited), "the section lights for the track it edits")
    }

    @Test func theLeftPaneEditsTheFocusedTrack() throws {
        let (m, _) = try mixer(armB: true)
        let synth = try strip(m, 1)
        #expect(m.midiFxLane(for: synth, in: .strip)?.id == 31, "nothing selected: the armed track is followed")
        m.document.editor.selectTrack(seq: 3, lane: 30, extend: false)
        #expect(m.midiFxLane(for: synth, in: .strip)?.id == 30, "the selected track, though another is armed")
        #expect(m.midiFxLane(for: synth, in: .view)?.id == 31, "the Mix view still edits the armed one")
        m.document.editor.selectTrack(seq: 3, lane: 32, extend: false)
        #expect(m.midiFxLane(for: synth, in: .strip) == nil, "the focused track plays another strip")
        #expect(m.midiFxLane(for: try strip(m, 5), in: .strip)?.id == 32)
    }

    @Test func audioStripsAndBusesHaveNoSection() throws {
        let (m, _) = try mixer()
        for id: UInt64 in [6, 2, 0] {  // Vocal (an audio path), Bus 1, Master
            let s = try strip(m, id)
            #expect(!m.showsMidiFx(s), "\(s.name)")
            #expect(m.midiFxLane(for: s, in: .view) == nil, "\(s.name)")
            #expect(m.midiFxLane(for: s, in: .strip) == nil, "\(s.name)")
        }
        #expect(!m.showsMidiFx(stripRow(9, "Piano 2", kind: .instrumentOutput(dest: 7, index: 1))), "an extra output")
        // An instrument strip that no track of the shown sequence plays:
        // the section is there, with nothing to edit.
        m.document.select(2)
        let synth = try strip(m, 1)
        #expect(m.showsMidiFx(synth))
        #expect(m.midiFxLane(for: synth, in: .view) == nil)
    }

    /// Programmatic calls open nothing (Global Constraints): with the
    /// channel on, a click on the empty slot or a filled one opens no menu
    /// and no popover, and edits nothing.
    @Test func aClickFromTheChannelOpensNothing() throws {
        let f = editorFake()
        f.laneTable[3]?[1].inputFx = [up12]
        let m = try DocumentModel(session: f)
        m.select(3)
        let fx = m.inputFx
        let before = f.edits.count
        #expect(!fx.clickEmpty(lane: 31, channelOn: true), "no effect menu")
        #expect(m.editor.status == InputFxModel.sectionOnChannel)
        #expect(fx.lane == 31)
        fx.clickSlot(0, lane: 31, strip: 1, in: .view, channelOn: true)
        #expect(fx.settings == nil, "no settings popover")
        #expect(fx.selected == 0)
        #expect(f.edits.count == before, "a click edits nothing")
        // The bypass dot is a plain toggle: the channel may press it.
        fx.toggleBypass(0, lane: 31)
        #expect(f.edits.count == before + 1)
        guard case .lane(edit: .setInputFx(seq: 3, lane: 31, let chain))? = f.edits.last else {
            Issue.record("no chain edit: \(f.edits)")
            return
        }
        #expect(chain == [InputFxRow(kind: up12.kind, bypass: true)])
    }

    @Test func aUsersClickShowsTheSlotsSettingsWhereItWasClicked() throws {
        let f = editorFake()
        f.laneTable[3]?[1].inputFx = [up12, down5]
        let m = try DocumentModel(session: f)
        m.select(3)
        let fx = m.inputFx
        #expect(fx.clickEmpty(lane: 31, channelOn: false), "the effect menu may open")
        // Dragging a slot moves it within its own track's chain only.
        #expect(InputFxModel.dragText(lane: 31, index: 1) == "midifx:31:1")
        var before = f.edits.count
        #expect(!fx.drop("midifx:30:1", onto: 0, lane: 31), "another track's slot")
        #expect(!fx.drop("slot:5", onto: 0, lane: 31), "a plugin slot")
        #expect(f.edits.count == before)
        #expect(fx.drop("midifx:31:1", onto: 0, lane: 31))
        guard case .lane(edit: .setInputFx(seq: 3, lane: 31, let moved))? = f.edits.last else {
            Issue.record("no chain edit: \(f.edits)")
            return
        }
        #expect(moved == [down5, up12])
        // A click on a filled slot shows its settings in that placement only.
        fx.clickSlot(0, lane: 31, strip: 1, in: .view, channelOn: false)
        #expect(fx.settings == InputFxModel.SettingsSlot(placement: .view, strip: 1, index: 0))
        #expect(fx.showsSettings(strip: 1, index: 0, in: .view))
        #expect(!fx.showsSettings(strip: 1, index: 0, in: .strip), "one popover, where it was clicked")
        // A slider's draft shows in the slot and is committed once when the popover closes.
        before = f.edits.count
        fx.beginDraft()
        fx.setDraft(.transpose(semitones: -3))
        #expect(fx.chain(of: m.lanes[1]).first?.kind == .transpose(semitones: -3), "the slot shows the draft")
        #expect(fx.chain(of: m.lanes[0]).isEmpty, "another track shows its own chain")
        fx.closeSettings()
        #expect(fx.settings == nil)
        #expect(f.edits.count == before + 1)
        // Another sequence closes the popover with the editor.
        fx.clickSlot(0, lane: 31, strip: 1, in: .view, channelOn: false)
        m.select(2)
        #expect(fx.settings == nil)
    }
}
