import Foundation
import Testing
import VizDAWCore
@testable import VizDAWAppModel

/// demoFake with revisions for Song's lanes (30 Groove, 31 Chords).
func editorFake() -> FakeSession {
    let f = demoFake()
    f.snap.laneRevs = [
        LaneRev(seq: 3, lane: 30, rev: 1), LaneRev(seq: 3, lane: 31, rev: 2),
    ]
    f.laneEventTable["3/31"] = LaneEvents(seq: 3, lane: 31, rev: 2, events: [noteRow(1, tick: 0)])
    f.laneEventTable["3/30"] = LaneEvents(seq: 3, lane: 30, rev: 1, events: [])
    f.blocksTable["3/30"] = [blockRow(0, 4 * 61_440, events: [7], ref: 2, label: "Groove")]
    f.blocksTable["3/31"] = [blockRow(0, 61_440, events: [1])]
    return f
}

/// The next command returns `f.snap` changed by `change`, one version on.
func nextSnapshot(_ f: FakeSession, _ change: (inout DocSnapshot) -> Void) {
    var s = f.snap
    s.version += 1
    change(&s)
    f.bump(s)
}

/// Lets the main-actor publish of a draw-time failure run.
@MainActor func settle() async {
    for _ in 0..<5 { await Task.yield() }
}

let chords = LaneKey(3, 31)
let grooveLane = LaneKey(3, 30)

@MainActor
@Suite struct EditorModelTests {
    @Test func eventsAreFetchedOnceAndAgainOnlyWhenTheirRevisionMoves() throws {
        let f = editorFake()
        let m = try DocumentModel(session: f)
        let e = m.editor
        #expect(e.laneEvents(chords)?.events.count == 1)
        #expect(e.laneEvents(chords) != nil)
        #expect(e.laneEvents(grooveLane) != nil)
        #expect(e.eventFetches == 2)
        f.laneEventTable["3/31"] = LaneEvents(seq: 3, lane: 31, rev: 5, events: [])
        nextSnapshot(f) { $0.laneRevs[1].rev = 5 }
        m.rename(1, to: "Kit")
        #expect(e.laneEvents(chords)?.events.isEmpty == true)
        #expect(e.laneEvents(grooveLane) != nil)
        #expect(e.eventFetches == 3, "only Chords was refetched")
    }

    @Test func lanesThatAreGoneLeaveTheCache() throws {
        let f = editorFake()
        let m = try DocumentModel(session: f)
        _ = m.editor.laneEvents(chords)
        _ = m.editor.blocks(chords)
        nextSnapshot(f) { $0.laneRevs.removeLast() }
        m.rename(1, to: "Kit")
        #expect(!m.editor.cachedLanes.contains(chords))
    }

    @Test func blocksFollowTheRevisionAndTheRule() throws {
        let f = editorFake()
        let m = try DocumentModel(session: f)
        let e = m.editor
        #expect(e.blocks(chords).count == 1)
        _ = e.blocks(chords)
        #expect(e.blockFetches == 1)
        e.updateLayout { $0.blockRule = .fixedBars(2) }
        _ = e.blocks(chords)
        #expect(e.blockFetches == 2)
        #expect(f.lastRule == .fixedBars(bars: 2))
    }

    @Test func referenceBlocksRefetchWhenAnySequenceRowChanges() throws {
        let f = editorFake()
        let m = try DocumentModel(session: f)
        let e = m.editor
        _ = e.blocks(grooveLane)
        _ = e.blocks(chords)
        // Groove is renamed: no lane of Song changed revision.
        nextSnapshot(f) { $0.sequences[1].name = "Groove 2" }
        m.rename(2, to: "Groove 2")
        _ = e.blocks(grooveLane)
        _ = e.blocks(chords)
        #expect(f.blocksCalls == ["3/30", "3/31", "3/30"])
    }

    @Test func sequenceInfoIsCachedPerVersion() throws {
        let f = editorFake()
        let m = try DocumentModel(session: f)
        _ = m.editor.info(3)
        _ = m.editor.info(3)
        #expect(m.editor.infoFetches == 1)
        m.rename(1, to: "Kit")
        _ = m.editor.info(3)
        #expect(m.editor.infoFetches == 2)
    }

    @Test func busyReadsKeepTheStaleRowsSilently() async throws {
        let f = editorFake()
        let m = try DocumentModel(session: f)
        _ = m.editor.laneEvents(chords)
        nextSnapshot(f) { $0.laneRevs[1].rev = 9 }
        m.rename(1, to: "Kit")
        f.readError = .Busy(message: "busy")
        #expect(m.editor.laneEvents(chords)?.events.count == 1)
        #expect(m.editor.status == nil)
        f.readError = .Model(message: "gone")
        #expect(m.editor.laneEvents(chords) == nil)
        await settle()
        #expect(m.editor.status == "gone")
    }

    @Test func anEditAppliesItsSnapshotAndRefreshesTheSelection() throws {
        let f = editorFake()
        let m = try DocumentModel(session: f)
        let before = m.snapshot.version
        f.sel = SelectionRow(timeRange: nil, lanes: [], events: [1])
        let ok = m.editor.edit(.event(edit: .delete(events: [1])))
        #expect(ok)
        #expect(m.snapshot.version == before + 1)
        #expect(m.editor.selection.events == [1])
        #expect(f.edits == [.event(edit: .delete(events: [1]))])
    }

    @Test func aRefusedEditShowsAStatus() throws {
        let f = editorFake()
        f.editError = .Model(message: "would make a cycle")
        let m = try DocumentModel(session: f)
        #expect(!m.editor.edit(.event(edit: .delete(events: [1]))))
        #expect(m.editor.status == "would make a cycle")
        m.editor.clearStatus()
        #expect(m.editor.status == nil)
    }

    @Test func selectingABlockSelectsItsEventsLaneAndRange() throws {
        let f = editorFake()
        let m = try DocumentModel(session: f)
        let e = m.editor
        e.selectBlock(blockRow(0, 100, events: [1, 2]), in: chords, adding: false)
        #expect(f.sel.events == [1, 2])
        #expect(f.sel.lanes == [LaneRef(seq: 3, lane: 31)])
        #expect(f.sel.timeRange == TickRange(start: 0, end: 100))
        e.selectBlock(blockRow(200, 300, events: [3]), in: grooveLane, adding: true)
        #expect(Set(f.sel.events) == [1, 2, 3])
        #expect(f.sel.timeRange == TickRange(start: 0, end: 300))
        #expect(e.isSelected(blockRow(0, 100, events: [1, 2])))
        e.clearSelection()
        #expect(f.sel.events.isEmpty && f.sel.timeRange == nil)
    }

    @Test func aRulerRangeSelectsEveryShownLane() throws {
        let f = editorFake()
        let m = try DocumentModel(session: f)
        m.editor.selectRange(TickRange(start: 0, end: 61_440), lanes: [grooveLane, chords])
        #expect(f.sel.lanes == [LaneRef(seq: 3, lane: 30), LaneRef(seq: 3, lane: 31)])
        #expect(f.sel.events.isEmpty)
    }

    @Test func theLayoutIsStoredOnSaveAndReadAtOpen() throws {
        let f = editorFake()
        let m = try DocumentModel(session: f)
        m.editor.updateLayout { $0.sequenceOrder = [2, 1] }
        m.saveAs(path: "/tmp/x/a.vizdaw")
        #expect(EditorLayout.decode(f.layout).sequenceOrder == [2, 1])
        let reopened = try DocumentModel(session: f)
        #expect(reopened.referenced.map(\.id) == [2, 1])
    }

    @Test func layoutDecodingIsTolerant() {
        #expect(EditorLayout.decode(nil) == EditorLayout())
        #expect(EditorLayout.decode("garbage") == EditorLayout())
        let partial = EditorLayout.decode(#"{"version":1,"lightMode":"on","future":42}"#)
        #expect(partial.lightMode == .on)
        #expect(partial.snap == Snap())
        var l = EditorLayout()
        l.detail = DetailState(kind: .list, target: .block(seq: 3, lane: 31, start: 0, end: 10))
        l.windows = [TornWindow(id: UUID(), detail: DetailState(kind: .pianoRoll, target: .lane(seq: 3, lane: 30)), frame: [1, 2, 3, 4])]
        #expect(EditorLayout.decode(l.encoded()) == l)
    }

    @Test func blockRuleSettingsMapToRules() {
        #expect(BlockRuleSetting.silenceGap(beats: 2).rule == .silenceGap(ticks: 30_720))
        #expect(BlockRuleSetting.fixedBars(4).rule == .fixedBars(bars: 4))
        #expect(BlockRuleSetting.silenceGap(beats: 0).rule == .silenceGap(ticks: 15_360))
        #expect(BlockRuleSetting.wholeLane.rule == .wholeLane)
    }

    @Test func aLaneMissingFromTheRevisionsIsCachedNotRefetched() throws {
        let f = editorFake()
        f.laneEventTable["3/99"] = LaneEvents(seq: 3, lane: 99, rev: 0, events: [])
        f.blocksTable["3/99"] = []
        let m = try DocumentModel(session: f)
        let lane = LaneKey(3, 99)
        _ = m.editor.laneEvents(lane)
        _ = m.editor.laneEvents(lane)
        _ = m.editor.blocks(lane)
        _ = m.editor.blocks(lane)
        #expect(m.editor.eventFetches == 1)
        #expect(m.editor.blockFetches == 1)
    }

    @Test func aReadFailureIsPublishedLaterAndOnlyOnce() async throws {
        let f = editorFake()
        let m = try DocumentModel(session: f)
        f.readError = .Model(message: "lane unreadable")
        #expect(m.editor.laneEvents(chords) == nil)
        #expect(m.editor.blocks(chords).isEmpty)
        #expect(m.editor.status == nil, "nothing observed changes while drawing")
        await settle()
        #expect(m.editor.status == "lane unreadable")
        m.editor.clearStatus()
        _ = m.editor.laneEvents(chords)
        await settle()
        #expect(m.editor.status == nil, "the same error is not shown again every redraw")
    }

    @Test func aRefusedSelectionIsRolledBack() throws {
        let f = editorFake()
        let m = try DocumentModel(session: f)
        let before = m.editor.selection
        f.selectionError = .Model(message: "no")
        m.editor.setSelection(SelectionRow(timeRange: nil, lanes: [], events: [1]))
        #expect(m.editor.selection == before)
        #expect(!m.editor.isSelected(event: 1))
        #expect(m.editor.status == "no")
    }

    @Test func anInfoFailureIsPublishedOnceAndNotRefetchedPerDraw() async throws {
        let f = editorFake()
        let m = try DocumentModel(session: f)
        f.readError = .Model(message: "no info")
        #expect(m.editor.info(3) == nil)
        #expect(m.editor.info(3) == nil)
        #expect(m.editor.infoFetches == 1)
        #expect(m.editor.status == nil)
        await settle()
        #expect(m.editor.status == "no info")
    }
}
