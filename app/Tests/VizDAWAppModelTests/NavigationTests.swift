import Testing
import VizDAWCore
@testable import VizDAWAppModel

@MainActor
@Suite struct NavigationTests {
    @Test func aBlockOpensInTheLastUsedDetailEditor() throws {
        let m = try DocumentModel(session: editorFake())
        m.editor.openDetail(.block(seq: 3, lane: 31, start: 0, end: 10))
        #expect(m.editor.layout.detail == DetailState(kind: .pianoRoll, target: .block(seq: 3, lane: 31, start: 0, end: 10)))
        m.editor.openDetail(.lane(seq: 3, lane: 31), kind: .list)
        m.editor.openDetail(.lane(seq: 3, lane: 30))
        #expect(m.editor.layout.detail?.kind == .list, "the last used editor")
    }

    @Test func openingAReferenceShowsItsSequenceAndBackReturns() throws {
        let m = try DocumentModel(session: editorFake())
        #expect(m.selectedSeq == 3)
        m.editor.openReference(blockRow(0, 10, events: [7], ref: 2, label: "Groove"))
        #expect(m.selectedSeq == 2)
        #expect(m.editor.navigation == [3])
        m.editor.back()
        #expect(m.selectedSeq == 3)
        #expect(m.editor.navigation.isEmpty)
    }

    @Test func choosingFromTheSidebarForgetsTheTrail() throws {
        let m = try DocumentModel(session: editorFake())
        m.editor.openReference(blockRow(0, 10, events: [7], ref: 2, label: "Groove"))
        m.choose(1)
        #expect(m.editor.navigation.isEmpty)
        #expect(m.selectedSeq == 1)
    }

    @Test func playKeyChangesOnlyWithPlayStateOrSequence() throws {
        let f = editorFake()
        let m = try DocumentModel(session: f)
        #expect(m.playKey == PlayKey(playing: false, seq: nil, stoppedTick: 0))
        m.playStop()
        #expect(m.playKey == PlayKey(playing: true, seq: 3, stoppedTick: nil))
        f.state.tick = 99
        m.poll()
        #expect(m.playKey == PlayKey(playing: true, seq: 3, stoppedTick: nil), "playing ticks do not change it")
    }

    @Test func locatingWhileStoppedChangesPlayKey() throws {
        let f = editorFake()
        f.state.seq = 3
        let m = try DocumentModel(session: f)
        #expect(m.playKey == PlayKey(playing: false, seq: 3, stoppedTick: 0))
        m.locate(61_440)
        #expect(m.playKey == PlayKey(playing: false, seq: 3, stoppedTick: 61_440), "the playhead must move")
        m.poll()
        #expect(m.playKey == PlayKey(playing: false, seq: 3, stoppedTick: 61_440))
    }

    @Test func aRulerClickLocates() throws {
        let f = editorFake()
        let m = try DocumentModel(session: f)
        m.locate(61_440)
        #expect(f.state.tick == 61_440)
    }
}
