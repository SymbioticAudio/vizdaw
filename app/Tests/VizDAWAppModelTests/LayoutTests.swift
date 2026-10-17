import Foundation
import Testing
import VizDAWCore
@testable import VizDAWAppModel

@MainActor
@Suite struct LayoutTests {
    @Test func detailEditorsOpenOnTheSelectionOrTheSequence() throws {
        let m = try DocumentModel(session: editorFake())
        let e = m.editor
        #expect(e.detailTarget(shown: 3) == .sequence(3))
        e.selectRange(TickRange(start: 0, end: 61_440), lanes: [chords])
        #expect(e.detailTarget(shown: 3) == .block(seq: 3, lane: 31, start: 0, end: 61_440))
        e.showDetail(.list, shown: 3)
        #expect(e.layout.detail == DetailState(kind: .list, target: .block(seq: 3, lane: 31, start: 0, end: 61_440)))
        e.clearSelection()
        e.showDetail(.drumGrid, shown: 3)
        #expect(e.layout.detail?.target == .block(seq: 3, lane: 31, start: 0, end: 61_440), "keeps its target")
        #expect(e.layout.detail?.kind == .drumGrid)
        e.closeDetail()
        #expect(e.layout.detail == nil)
    }

    @Test func tearingOffAddsAWindowAndClosingRemovesIt() throws {
        let m = try DocumentModel(session: editorFake())
        let e = m.editor
        let w = try #require(e.tearOff(.pianoRoll, shown: 3))
        let w2 = try #require(e.tearOff(.list, shown: 3))
        #expect(w.id != w2.id)
        #expect(e.layout.windows.map(\.detail.kind) == [.pianoRoll, .list])
        e.setWindowFrame(w.id, [10, 20, 700, 400])
        e.setWindowKind(w.id, .stripChart)
        #expect(e.layout.windows[0].frame == [10, 20, 700, 400])
        #expect(e.layout.windows[0].detail.kind == .stripChart)
        e.closeWindow(w.id)
        #expect(e.layout.windows.map(\.id) == [w2.id])
    }

    @Test func targetsOfDeletedSequencesAreDropped() throws {
        let f = editorFake()
        let m = try DocumentModel(session: f)
        m.editor.openDetail(.lane(seq: 2, lane: 20))
        _ = m.editor.tearOff(.list, shown: 2)
        nextSnapshot(f) { $0.sequences.removeAll { $0.id == 2 } }
        m.rename(1, to: "Kit")
        #expect(m.editor.layout.detail == nil)
        #expect(m.editor.layout.windows.isEmpty)
    }

    @Test func targetsOfDeletedLanesAreDropped() throws {
        let f = editorFake()
        let m = try DocumentModel(session: f)
        m.editor.openDetail(.block(seq: 3, lane: 31, start: 0, end: 61_440))
        let gone = try #require(m.editor.tearOff(.list, shown: 3))
        m.editor.closeDetail()
        m.editor.openDetail(.lane(seq: 3, lane: 30))
        let kept = try #require(m.editor.tearOff(.pianoRoll, shown: 3))
        #expect(kept.detail.target == .lane(seq: 3, lane: 30))
        nextSnapshot(f) { $0.laneRevs.removeAll { $0.lane == 31 } }
        m.rename(1, to: "Kit")
        #expect(m.editor.layout.windows.map(\.id) == [kept.id])
        #expect(!m.editor.layout.windows.contains { $0.id == gone.id })
        #expect(m.editor.layout.detail?.target == .lane(seq: 3, lane: 30), "other lanes stay")
        nextSnapshot(f) { $0.laneRevs.removeAll { $0.lane == 30 } }
        m.rename(1, to: "Kit2")
        #expect(m.editor.layout.detail == nil)
        #expect(m.editor.layout.windows.isEmpty)
    }

    @Test func eventCountsCoverTheTarget() throws {
        let f = editorFake()
        f.laneEventTable["3/31"] = LaneEvents(
            seq: 3, lane: 31, rev: 2,
            events: [noteRow(1, tick: 0), noteRow(2, tick: 15_360), noteRow(3, tick: 5 * 61_440)])
        let m = try DocumentModel(session: f)
        #expect(m.editor.eventCount(.block(seq: 3, lane: 31, start: 0, end: 61_440)) == 2)
        #expect(m.editor.eventCount(.lane(seq: 3, lane: 31)) == 3)
        #expect(m.editor.eventCount(.sequence(3)) == nil)
    }

    @Test func titlesNameSequenceLaneAndBars() throws {
        let m = try DocumentModel(session: editorFake())
        let rows = m.snapshot.sequences
        #expect(m.editor.title(.block(seq: 3, lane: 31, start: 0, end: 2 * 61_440), sequences: rows) == "Song › Chords, bars 1–2")
        #expect(m.editor.title(.lane(seq: 3, lane: 30), sequences: rows) == "Song › Groove")
        #expect(m.editor.title(.sequence(1), sequences: rows) == "Beat")
    }
}
