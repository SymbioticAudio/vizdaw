import Foundation
import Testing
import VizDAWCore
@testable import VizDAWAppModel

/// Song (3): Groove (30, no destination, no MIDI), Chords (31, destination
/// 5, notes 1 = C4 at 0 and 2 = E4 at one quarter) and Bass (32,
/// destination 6, note 3 = E2 at 0).
func rollFake() -> FakeSession {
    let f = editorFake()
    var chords = lane(31, "Chords")
    chords.midiDest = 5
    chords.midiEventCount = 2
    var bass = lane(32, "Bass")
    bass.midiDest = 6
    bass.midiEventCount = 1
    f.laneTable[3] = [lane(30, "Groove"), chords, bass]
    f.snap.laneRevs.append(LaneRev(seq: 3, lane: 32, rev: 3))
    f.laneEventTable["3/31"] = LaneEvents(
        seq: 3, lane: 31, rev: 2, events: [noteRow(1, tick: 0, pitch: 60), noteRow(2, tick: 15_360, pitch: 64)])
    f.laneEventTable["3/32"] = LaneEvents(seq: 3, lane: 32, rev: 3, events: [noteRow(3, tick: 0, pitch: 40)])
    return f
}

/// `PianoRollModel.document` is unowned, so a test that drops the
/// document from its tuple would leave the roll dangling.
@MainActor private var liveDocuments: [DocumentModel] = []

@MainActor
@Suite struct PianoRollModelTests {
    func make(_ target: EditorTarget, lanes: [UInt64]? = nil) throws -> (FakeSession, DocumentModel, PianoRollModel) {
        let f = rollFake()
        let m = try DocumentModel(session: f)
        liveDocuments.append(m)
        m.editor.openDetail(target, kind: .pianoRoll)
        if let lanes { m.editor.updateLayout { $0.detail?.lanes = lanes } }
        return (f, m, PianoRollModel.pane(m))
    }

    func log(_ f: FakeSession) -> [String] {
        f.auditions.map { "\($0.on ? "on" : "off") \($0.pitch)" }
    }

    @Test func aLaneTargetShowsItsLanePlusExtras() throws {
        let (_, _, r) = try make(.lane(seq: 3, lane: 31))
        #expect(r.shownLanes == [LaneKey(3, 31)])
        r.toggleShown(32)
        #expect(r.shownLanes == [LaneKey(3, 31), LaneKey(3, 32)])
        r.toggleShown(31)
        #expect(r.isShown(31), "the target lane always shows")
        r.toggleShown(32)
        #expect(r.shownLanes == [LaneKey(3, 31)])
    }

    @Test func aSequenceTargetShowsLanesWithNotesOrADestination() throws {
        let (_, m, r) = try make(.sequence(3))
        #expect(r.shownLanes == [LaneKey(3, 31), LaneKey(3, 32)])
        r.toggleShown(31)
        #expect(r.shownLanes == [LaneKey(3, 32)])
        #expect(m.editor.layout.detail?.lanes == [32])
        r.toggleShown(30)
        #expect(r.shownLanes == [LaneKey(3, 30), LaneKey(3, 32)], "sequence order")
    }

    @Test func coloursFollowTheLaneIndexInItsSequence() throws {
        let (_, _, r) = try make(.sequence(3))
        #expect(r.colorIndex(LaneKey(3, 30)) == 0)
        #expect(r.colorIndex(LaneKey(3, 32)) == 2)
    }

    @Test func theEditLaneFollowsTheLastChosenShownLane() throws {
        let (_, _, r) = try make(.lane(seq: 3, lane: 31), lanes: [32])
        #expect(r.editLane == LaneKey(3, 31))
        r.select([(LaneKey(3, 32), 3)], adding: false)
        #expect(r.editLane == LaneKey(3, 32))
    }

    @Test func theEditLaneFallsBackWhenItsLaneIsHidden() throws {
        let (_, _, r) = try make(.lane(seq: 3, lane: 31), lanes: [32])
        r.chosenLane = LaneKey(3, 32)
        r.toggleShown(32)
        #expect(r.editLane == LaneKey(3, 31))
    }

    @Test func selectingTogglingAndDeletingNotes() throws {
        let (f, m, r) = try make(.lane(seq: 3, lane: 31), lanes: [32])
        r.select([(LaneKey(3, 31), 1), (LaneKey(3, 32), 3)], adding: false)
        #expect(m.editor.selection.events == [1, 3])
        #expect(m.editor.selection.lanes == [LaneRef(seq: 3, lane: 31), LaneRef(seq: 3, lane: 32)])
        r.toggle(1, in: LaneKey(3, 31))
        #expect(m.editor.selection.events == [3])
        r.deleteSelected()
        #expect(f.edits.last == .event(edit: .delete(events: [3])))
    }

    @Test func soundingANewPitchEndsTheOldOne() throws {
        let (f, _, r) = try make(.lane(seq: 3, lane: 31))
        r.sound(60, velocity: 0x8000, dest: 5)
        r.sound(60, velocity: 0x8000, dest: 5)
        r.sound(62, velocity: 0x8000, dest: 5)
        r.silence()
        #expect(log(f) == ["on 60", "off 60", "on 62", "off 62"])
        #expect(r.sounding == nil)
    }

    @Test func aLaneWithoutDestinationIsSilent() throws {
        let (f, m, r) = try make(.lane(seq: 3, lane: 30))
        let groove = LaneKey(3, 30)
        var row = noteRow(9, tick: 0)
        #expect(r.dest(of: row, in: groove) == nil)
        r.sound(60, velocity: 0x8000, dest: r.dest(of: row, in: groove))
        r.silence()
        #expect(f.auditions.isEmpty)
        #expect(m.editor.status == nil)
        row.dest = 9
        #expect(r.dest(of: row, in: groove) == 9, "the event's own destination wins")
    }

    @Test func aShownLaneThatVanishesIsDropped() throws {
        let (f, m, r) = try make(.lane(seq: 3, lane: 31), lanes: [32])
        nextSnapshot(f) { $0.laneRevs.removeAll { $0.lane == 32 } }
        f.laneTable[3]?.removeAll { $0.id == 32 }
        m.rename(1, to: "Kit")
        #expect(m.editor.layout.detail?.lanes == nil)
        #expect(m.editor.layout.detail != nil, "the target lane is still there")
        #expect(r.shownLanes == [LaneKey(3, 31)])
    }

    @Test func aBlockTargetHasAFocusRange() throws {
        let (_, _, r) = try make(.block(seq: 3, lane: 31, start: 61_440, end: 122_880))
        #expect(r.focusRange == TickRange(start: 61_440, end: 122_880))
        #expect(r.shownLanes == [LaneKey(3, 31)])
    }

    @Test func laneRowsAreCachedPerSnapshotVersion() throws {
        let (f, m, _) = try make(.lane(seq: 3, lane: 31))
        let before = f.lanesCalls
        _ = m.editor.laneRows(2)
        _ = m.editor.laneRows(2)
        #expect(f.lanesCalls == before + 1)
        m.rename(1, to: "Kit")
        let mid = f.lanesCalls
        _ = m.editor.laneRows(2)
        #expect(f.lanesCalls == mid + 1)
    }

    @Test func settingsAndShownLanesDecodeTolerantly() throws {
        let l = EditorLayout.decode(#"{"pianoRoll":{"rowHeight":999,"showKeyboard":false}}"#)
        #expect(l.pianoRoll.rowHeight == 40)
        #expect(!l.pianoRoll.showKeyboard)
        #expect(l.pianoRoll.ticksPerPoint == 96)
        #expect(EditorLayout.decode(#"{"pianoRoll":7}"#).pianoRoll == PianoRollSettings())
        let d = DetailState(kind: .pianoRoll, target: .sequence(3), lanes: [31])
        #expect(try JSONDecoder().decode(DetailState.self, from: JSONEncoder().encode(d)) == d)
        let old = #"{"kind":"pianoRoll","target":{"sequence":{"_0":3}}}"#
        #expect(try JSONDecoder().decode(DetailState.self, from: Data(old.utf8)).lanes == nil, "4a layouts")
        for k in ExpressionKind.choices { #expect(ExpressionKind(k.row) == k) }
        #expect(ExpressionKind.pitchBend.neutral == 0x8000_0000)
        #expect(ExpressionKind.registered(74).title == "Registered 74 (Timbre)")
    }
}
