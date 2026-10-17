import Foundation
import Testing
import VizDAWCore
@testable import VizDAWAppModel

/// `rollFake()` plus lane Loop (33) routed to output path 7, whose mixer
/// strip is 40 "Audio 1-2".
func audioFake() -> FakeSession {
    let f = rollFake()
    var loop = lane(33, "Loop")
    loop.audioOut = 7
    loop.regionCount = 1
    f.laneTable[3]?.append(loop)
    f.snap.laneRevs.append(LaneRev(seq: 3, lane: 33, rev: 4))
    var r = noteRow(8, tick: 0)
    r.kind = .audioRegion
    r.duration = 61_440
    f.laneEventTable["3/33"] = LaneEvents(seq: 3, lane: 33, rev: 4, events: [r])
    f.mixerSnap = MixerSnapshot(
        version: 0, strips: [stripRow(40, "Audio 1-2", kind: .audioPath(path: 7)), stripRow(0, "Master", kind: .master)],
        buses: [])
    return f
}

@MainActor
@Suite struct RegionOutputTests {
    @Test func aSequenceTargetShowsRegionLanesAndTheirOutput() throws {
        let f = audioFake()
        let m = try DocumentModel(session: f)
        m.editor.openDetail(.sequence(3), kind: .pianoRoll)
        let r = m.pianoRoll(.pane)
        #expect(r.shownLanes.contains(LaneKey(3, 33)))
        let loop = try #require(r.regionLanes().first { $0.lane == LaneKey(3, 33) })
        #expect(loop.output == 7)
        #expect(loop.regions.map(\.id) == [8])
        #expect(r.outputStrips()[7]?.id == 40)
    }

    @Test func muteAndSoloSetTheOutputPathsStrip() throws {
        let f = audioFake()
        let m = try DocumentModel(session: f)
        m.editor.openDetail(.sequence(3), kind: .pianoRoll)
        let r = m.pianoRoll(.pane)
        r.setOutputMute(7, true)
        r.setOutputSolo(7, true)
        r.setOutputMute(99, true)
        #expect(f.calls.filter { $0.hasPrefix("mute") || $0.hasPrefix("solo") } == ["mute 40 true", "solo 40 true"])
    }

    @Test func outputButtonsWaitForAnOpenDrag() throws {
        let f = audioFake()
        let m = try DocumentModel(session: f)
        m.editor.openDetail(.sequence(3), kind: .pianoRoll)
        #expect(m.editor.beginGesture("Move Notes", lanes: [LaneKey(3, 31)]))
        m.pianoRoll(.pane).setOutputMute(7, true)
        #expect(!f.calls.contains("mute 40 true"))
        #expect(m.editor.status == EditorModel.gestureOpenMessage)
    }

    @Test func muteOrSoloOnAnOutputWithoutAStripSaysSo() throws {
        let f = audioFake()
        let m = try DocumentModel(session: f)
        m.editor.openDetail(.sequence(3), kind: .pianoRoll)
        m.pianoRoll(.pane).setOutputSolo(99, true)
        #expect(m.editor.status == PianoRollModel.noStripMessage)
        #expect(!f.calls.contains { $0.hasPrefix("solo") })
    }

    @Test func theStoreMakesTheDemoWithAudio() throws {
        let s = DocumentStore(makeEmpty: { editorFake() }, makeDemo: { editorFake() }, open: { _ in editorFake() })
        #expect(s.newDemoAudio() == nil, "no maker")
        s.makeDemoAudio = { audioFake() }
        #expect(s.newDemoAudio() != nil)
    }
}
