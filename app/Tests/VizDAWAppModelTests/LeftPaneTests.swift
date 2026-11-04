import Foundation
import Testing
import VizDAWCore
@testable import VizDAWAppModel

private func lane(_ id: UInt64, audioOut: UInt64? = nil, midiDest: UInt64? = nil, arm: Bool = false) -> LaneRow {
    LaneRow(
        id: id, name: "Lane \(id)", destination: nil, midiEventCount: 0, regionCount: 0,
        referenceCount: 0, mute: false, solo: false, looped: false, takeCount: 1,
        activeTake: "Take 1", isDrum: false, arm: arm, midiDest: midiDest, audioOut: audioOut,
        input: nil, monitor: .auto)
}

private func dest(_ id: UInt64, plugin: Bool = false, external: Bool = false, name: String = "Dest") -> DestRow {
    DestRow(
        id: id, name: name,
        plugin: plugin ? PluginRefRow(format: .clap, id: "x.\(name)", name: name, vendor: "V", version: "1") : nil,
        external: external, returnLatencyMs: 0)
}

private func strip(_ id: UInt64, _ kind: StripKindRow) -> StripRow { stripRow(id, "Strip \(id)", kind: kind) }

@MainActor private func documentWithLanes(_ lanes: [LaneRow]) -> DocumentModel {
    let f = demoFake()
    f.laneTable[3] = lanes
    return try! DocumentModel(session: f)
}

@MainActor private func fakeDocument() -> DocumentModel { try! DocumentModel(session: mixerFake()) }

/// Song (seq 3) with Groove (30) armed and followed, and `events31` on
/// Chords (31); media 7 is "loop".
@MainActor private func inspectorDocument(_ events31: [EventRow], rate: UInt32 = 48_000) -> DocumentModel {
    let f = editorFake()
    f.laneTable[3] = [lane(30, arm: true), lane(31)]
    f.laneEventTable["3/31"] = LaneEvents(seq: 3, lane: 31, rev: 2, events: events31)
    f.mediaRows = [mediaRow(7, "loop")]
    f.info.format.sampleRate = rate
    return try! DocumentModel(session: f)
}

@MainActor private func select(_ events: [UInt64], in m: DocumentModel) {
    m.editor.setSelection(SelectionRow(timeRange: nil, lanes: [], events: events))
}

private let regionLabels: Set<String> = ["Region", "Gain", "Fade In", "Fade Out"]

@MainActor
@Suite struct LeftPaneTests {
    @Test func anAudioLaneShowsItsPathsStrip() {
        let r = StripFocus.strip(selected: [lane(1, audioOut: 7)], armed: [], destinations: [],
                                 strips: [strip(10, .audioPath(path: 7)), strip(11, .master)])
        #expect(r == .strip(10))
    }
    @Test func aPluginLaneShowsItsInstrumentStrip() {
        #expect(StripFocus.strip(selected: [lane(1, midiDest: 4)], armed: [], destinations: [dest(4, plugin: true)],
                                 strips: [strip(12, .instrument(dest: 4))]) == .strip(12))
    }
    @Test func aBuiltinSynthLaneShowsTheSynthStrip() {
        #expect(StripFocus.strip(selected: [lane(1, midiDest: 2)], armed: [], destinations: [dest(2)],
                                 strips: [strip(13, .builtinSynth)]) == .strip(13))
    }
    @Test func anExternalMidiLaneShowsTheInspector() {
        #expect(StripFocus.strip(selected: [lane(1, midiDest: 3)], armed: [], destinations: [dest(3, external: true)],
                                 strips: [strip(13, .builtinSynth)]) == .inspector)
        #expect(StripFocus.strip(selected: [lane(1)], armed: [], destinations: [], strips: []) == .inspector)
    }
    @Test func twoLanesOnOnePathShowOneStrip() {
        #expect(StripFocus.strip(selected: [lane(1, audioOut: 7), lane(2, audioOut: 7)], armed: [], destinations: [],
                                 strips: [strip(10, .audioPath(path: 7))]) == .strip(10))
    }
    @Test func differentPathsShowTheFirstSelectedLanesStrip() {
        #expect(StripFocus.strip(selected: [lane(2, audioOut: 8), lane(1, audioOut: 7)], armed: [], destinations: [],
                                 strips: [strip(10, .audioPath(path: 7)), strip(11, .audioPath(path: 8))]) == .strip(11))
    }
    @Test func noSelectionShowsTheArmedLaneElseNothing() {
        let strips = [strip(10, .audioPath(path: 7))]
        #expect(StripFocus.strip(selected: [], armed: [lane(1, audioOut: 7, arm: true)], destinations: [],
                                 strips: strips) == .strip(10))
        #expect(StripFocus.strip(selected: [], armed: [], destinations: [], strips: strips) == .nothing)
        // A selected lane the document no longer has is skipped (leftStrip, over a FakeSession):
        let m = documentWithLanes([lane(1, audioOut: 7)])
        m.editor.setSelection(SelectionRow(timeRange: nil, lanes: [LaneKey(m.selectedSeq!, 99).ref], events: []))
        #expect(m.leftStrip(strips: strips, destinations: []) == .nothing)
        // A lane in another sequence is ignored, even with the same ID:
        let otherSeq = m.selectedSeq! + 1
        m.editor.setSelection(SelectionRow(timeRange: nil, lanes: [LaneKey(otherSeq, 1).ref], events: []))
        #expect(m.leftStrip(strips: strips, destinations: []) == .nothing)
        // A present lane in the shown sequence resolves:
        m.editor.setSelection(SelectionRow(timeRange: nil, lanes: [LaneKey(m.selectedSeq!, 1).ref], events: []))
        #expect(m.leftStrip(strips: strips, destinations: []) == .strip(10))
    }
    @Test func theInspectorDescribesTheLaneAndRegion() {
        let rows = Inspector.rows(lane: lane(1, midiDest: 3), region: nil, regionName: nil,
                                  destinations: [dest(3, external: true, name: "Ext Synth")], strips: [])
        #expect(rows.first == InspectorRow(label: "Name", value: "Lane 1"))
        #expect(rows.contains(InspectorRow(label: "Plays", value: "Ext Synth")))
        #expect(Inspector.rows(lane: nil, region: nil, regionName: nil, destinations: [], strips: []).isEmpty)
    }
    @Test func theInspectorFindsARegionOnALaneOtherThanTheFollowedOne() {
        let m = inspectorDocument([regionRow(5, tick: 0, duration: 61_440, frames: 96_000)])
        select([5], in: m)
        let rows = m.inspectorRows(destinations: [], strips: [])
        #expect(rows.first == InspectorRow(label: "Name", value: "Lane 30"))   // the followed (armed) lane
        #expect(rows.contains(InspectorRow(label: "Region", value: "loop")))
    }
    @Test func aSelectedNoteOrReferenceGivesNoRegionRows() {
        var reference = noteRow(6, tick: 0)
        reference.kind = .reference
        let m = inspectorDocument([noteRow(5, tick: 0), reference])
        for id: UInt64 in [5, 6] {
            select([id], in: m)
            let rows = m.inspectorRows(destinations: [], strips: [])
            #expect(!rows.isEmpty)
            #expect(rows.allSatisfy { !regionLabels.contains($0.label) })
        }
    }
    @Test func twoSelectedEventsGiveNoRegionRows() {
        let m = inspectorDocument([
            regionRow(5, tick: 0, duration: 61_440, frames: 96_000),
            regionRow(6, tick: 61_440, duration: 61_440, frames: 96_000),
        ])
        select([5, 6], in: m)
        let rows = m.inspectorRows(destinations: [], strips: [])
        #expect(!rows.isEmpty)   // the followed lane's rows still show
        #expect(rows.allSatisfy { !regionLabels.contains($0.label) })
    }
    @Test func theInspectorsFadesUseTheProjectRate() {
        let m = inspectorDocument(
            [regionRow(5, tick: 0, duration: 61_440, frames: 96_000, fadeIn: 4410)], rate: 44_100)
        select([5], in: m)
        #expect(m.inspectorRows(destinations: [], strips: []).contains(InspectorRow(label: "Fade In", value: "100 ms")))
        m.projectInfo = nil
        #expect(
            m.inspectorRows(destinations: [], strips: []).contains(InspectorRow(label: "Fade In", value: "4410 frames")))
    }
    @Test func theInspectorNamesAnAudioPathAndARegion() {
        let ev = EventRow(
            id: 5, tick: 0, kind: .audioRegion, mute: false, dest: nil, groupChannel: nil, pitch: 0,
            velocity: 0, releaseVelocity: 0, duration: 0, value: 0, value2: 0, attribute: nil, perNote: [],
            refSeq: nil, refLength: nil, refLoops: 0, refTranspose: 0, media: 1, sourceOffset: 0,
            frames: 48000, gainDb: -3.5, fadeIn: FadeRow(length: 480, shape: .linear),
            fadeOut: FadeRow(length: 0, shape: .linear), text: nil, textKind: nil, data: Data(),
            eightBit: false, automation: nil)
        let rows = Inspector.rows(lane: lane(1, audioOut: 7), region: ev, regionName: "Take.wav", destinations: [],
                                  strips: [strip(10, .audioPath(path: 7))], sampleRate: 48000)
        #expect(rows.contains(InspectorRow(label: "Plays", value: "Audio: Strip 10")))
        #expect(rows.contains(InspectorRow(label: "Region", value: "Take.wav")))
        #expect(rows.contains(InspectorRow(label: "Gain", value: "-3.5 dB")))
        #expect(rows.contains(InspectorRow(label: "Fade In", value: "10 ms")))
        #expect(rows.contains(InspectorRow(label: "Fade Out", value: "0 ms")))
    }
    @Test func twoMixerViewsPollOncePerFrame() throws {
        let f = demoFake()
        let mixer = try MixerModel(document: DocumentModel(session: f))
        #expect(mixer.tick(now: 10.000))
        #expect(f.levelsCalls == 1)
        #expect(!mixer.tick(now: 10.010))   // the second placement, same frame
        #expect(f.levelsCalls == 1)
        #expect(mixer.tick(now: 10.031))
        #expect(f.levelsCalls == 2)
    }
    @Test func learnMapsShowInOnePlacement() {
        let learn = fakeDocument().learn
        learn.showMaps(in: .pane)
        #expect(learn.showsMaps(in: .pane) && !learn.showsMaps(in: .window))
        learn.closeMaps(from: .window)
        #expect(learn.showsMaps)
        learn.closeMaps(from: .pane)
        #expect(!learn.showsMaps)
    }
    @Test func popoversAndRenameShowInOnePlacement() throws {
        let mixer = try MixerModel(document: DocumentModel(session: mixerFake()))   // strip 1 "Synth"
        let synth = try #require(mixer.strips.first)
        mixer.openBrowser(.insert(strip: 1), in: .strip)
        #expect(mixer.showsBrowser(in: .strip) && !mixer.showsBrowser(in: .window))
        mixer.openBrowser(.instrument, in: .window)
        #expect(mixer.showsBrowser(in: .window) && !mixer.showsBrowser(in: .strip))
        mixer.browser = nil
        mixer.beginRename(synth, in: .pane)
        #expect(mixer.isRenaming(1, in: .pane) && !mixer.isRenaming(1, in: .window))
        let fader = AutomationTargetRow.stripFader(strip: 1)
        mixer.appeared(.window)
        mixer.appeared(.pane)
        mixer.touch(fader)
        mixer.disappeared(.pane)
        #expect(mixer.isTouched(fader))    // the ride in the mixer window goes on
        mixer.disappeared(.window)
        #expect(!mixer.isTouched(fader))   // the last placement gone ends it
    }
}
