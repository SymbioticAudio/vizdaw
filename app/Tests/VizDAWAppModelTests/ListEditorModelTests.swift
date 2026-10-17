import Foundation
import Testing
import VizDAWCore
@testable import VizDAWAppModel

final class FakeNoteInput: NoteInputProtocol, @unchecked Sendable {
    var queued: [NoteInRow] = []
    var starts = 0
    var stops = 0
    var failStart = false
    func start() throws -> UInt32 {
        starts += 1
        if failStart { throw VizError.Model(message: "no MIDI") }
        return 1
    }
    func stop() throws { stops += 1 }
    func take() throws -> [NoteInRow] {
        defer { queued = [] }
        return queued
    }
}

private let d4Loud = NoteInRow(pitch: 62, velocity: 0xC924, channel: 0)

@MainActor
private func listModel(_ target: EditorTarget = .lane(seq: 3, lane: 31)) throws
    -> (FakeSession, DocumentModel, ListEditorModel)
{
    let f = rollFake()
    f.dests = [DestRow(id: 5, name: "Piano", plugin: nil), DestRow(id: 6, name: "Bass Synth", plugin: nil)]
    let m = try DocumentModel(session: f)
    m.editor.openDetail(target, kind: .list)
    return (f, m, m.list(.pane))
}

@MainActor
@Suite struct ListEditorModelTests {
    @Test func rowsFollowTheTarget() throws {
        let (_, m, l) = try listModel()
        #expect(l.rows.map(\.id) == ["e1", "e2"])
        #expect(l.columns == [.position, .duration, .kind, .dest, .data1, .data2, .mute])
        m.editor.openDetail(.sequence(3), kind: .list)
        #expect(l.rows.map(\.id) == ["t0", "m0", "e1", "e3", "e2"], "map rows, then lanes in order")
        #expect(l.columns.contains(.lane))
        m.editor.openDetail(.block(seq: 3, lane: 31, start: 0, end: 15_360), kind: .list)
        #expect(l.rows.map(\.id) == ["e1"])
        #expect(l.context.destNames[5] == "Piano")
    }

    @Test func theCursorFollowsAMovedEvent() throws {
        let (f, m, l) = try listModel()
        try withExtendedLifetime(m) {
            #expect(l.commit("3", row: "e1", column: .position))
            #expect(f.edits.last == .event(edit: .move(events: [1], delta: 2 * 61_440, toLane: nil)))
            #expect(l.cursor == "e1")
            #expect(f.sel.events == [1] && f.sel.lanes == [LaneRef(seq: 3, lane: 31)], "the document selection follows")
        }
    }

    @Test func aRefusedCommitLeavesTheDocumentAndSaysWhy() throws {
        let (f, m, l) = try listModel()
        #expect(!l.commit("abc", row: "e1", column: .position))
        #expect(f.edits.isEmpty)
        #expect(m.editor.status?.contains("not a position") == true)
        #expect(l.commit("1.1.0", row: "e1", column: .position), "the same position changes nothing")
        #expect(f.edits.isEmpty)
    }

    @Test func committingARowThatVanishedDoesNothing() throws {
        let (f, m, l) = try listModel()
        #expect(!l.commit("3", row: "e99", column: .position))
        #expect(f.edits.isEmpty && m.editor.status == nil)
    }

    @Test func insertRepeatsTheLastKind() throws {
        let (f, m, l) = try listModel()
        #expect(l.lastInsertKind == .note)
        l.insert(.cc)
        guard case .event(.insert(3, 31, let rows))? = f.edits.last else {
            Issue.record("no insert")
            return
        }
        #expect(rows.first?.kind == .cc && rows.first?.tick == 0)
        #expect(l.lastInsertKind == .cc)
        l.insert()
        #expect(f.edits.count == 2)
        guard case .event(.insert(_, _, let again))? = f.edits.last else { return }
        #expect(again.first?.kind == .cc, "insert repeats the last inserted kind")
        l.insert(.reference)
        #expect(f.edits.count == 2 && m.editor.status == "References and regions are made in the arrangement.")
        l.insert(.tempo)
        #expect(f.edits.count == 2, "a lane target lists no tempo rows")
        #expect(!l.insertKinds.contains(.tempo))
        l.cursor = "e2"
        l.insert(.note)
        guard case .event(.insert(_, _, let at))? = f.edits.last else { return }
        #expect(at.first?.tick == 15_360, "at the cursor row")
    }

    @Test func filtersApplyToAllListsOrBecomeTheDefault() throws {
        let (f, m, l) = try listModel()
        withExtendedLifetime(f) {}
        l.toggle(.note)
        #expect(l.rows.isEmpty)
        let w = try #require(m.editor.tearOff(.list, shown: 3))
        let other = m.list(.window(w.id))
        #expect(other.rows.count == 2, "a new list starts from the default")
        l.applyFilterToAll()
        #expect(other.rows.isEmpty)
        l.saveFilterAsDefault()
        #expect(m.listPreferences.defaultFilter.hiddenKinds == [.note])
        l.setFilter(ListFilter())
        #expect(l.rows.count == 2)
        l.toggle(dest: 5)
        #expect(l.filter.shownDests == [6] && !l.shows(dest: 5))
        #expect(l.rows.isEmpty, "Chords plays on destination 5")
        m.editor.openDetail(.lane(seq: 3, lane: 32), kind: .list)
        #expect(l.filter.shownDests == [6], "the filter stays with the list")
        l.saveFilterAsDefault()
        #expect(m.listPreferences.defaultFilter.shownDests == nil, "destinations are not saved as a default")
    }

    @Test func aPlayedNoteGoesIntoTheEditedPitchOrVelocityCell() throws {
        let (f, m, l) = try listModel()
        withExtendedLifetime(m) {}
        l.receive([d4Loud])
        #expect(l.fieldInput == nil, "MIDI In is off")
        l.midiIn = true
        l.editing = ListCell(row: "e1", column: .data1)
        l.receive([d4Loud])
        #expect(l.fieldInput == "D4")
        l.editing = ListCell(row: "e1", column: .data2)
        l.receive([d4Loud])
        #expect(l.fieldInput == "100")
        #expect(f.edits.isEmpty)
    }

    @Test func aPlayedNoteSetsTheCursorNote() throws {
        let (f, m, l) = try listModel()
        withExtendedLifetime(m) {}
        l.midiIn = true
        l.cursor = "e1"
        l.receive([d4Loud])
        guard case .event(.replace(1, let row))? = f.edits.last else {
            Issue.record("no replace")
            return
        }
        #expect(row.pitch == 62 && row.velocity == 0xC924)
    }

    @Test func openingAReferenceOpensItsSequence() throws {
        let (f, m, l) = try listModel(.lane(seq: 3, lane: 30))
        var r = ListInsert.blankRow(kind: .reference, tick: 0)
        r.id = 7
        r.refSeq = 2
        f.laneEventTable["3/30"] = LaneEvents(seq: 3, lane: 30, rev: 1, events: [r])
        #expect(l.openReference("e7"))
        #expect(l.target == .sequence(2))
        #expect(m.selectedSeq == 2 && m.editor.navigation == [3])
        #expect(!l.openReference("e1"), "not a reference")
    }

    @Test func noteInputGoesOnlyToListeningDocuments() throws {
        let (f, m, l) = try listModel()
        let input = FakeNoteInput()
        let hub = NoteInputHub(input: input)
        input.queued = [d4Loud]
        hub.poll([m])
        #expect(input.starts == 0 && input.queued.count == 1, "nobody listens")
        l.midiIn = true
        l.cursor = "e1"
        hub.poll([m])
        #expect(input.starts == 1 && hub.isListening)
        #expect(input.queued.isEmpty && f.edits.isEmpty, "notes played before it started are discarded")
        input.queued = [d4Loud]
        hub.poll([m])
        #expect(f.edits.count == 1)
        hub.poll([m])
        #expect(input.starts == 1, "started once")
    }

    @Test func noteInputStopsWhenTheLastListTurnsMidiInOffAndRescansOnRestart() throws {
        let (_, m, l) = try listModel()
        let input = FakeNoteInput()
        let hub = NoteInputHub(input: input)
        l.midiIn = true
        hub.poll([m])
        l.midiIn = false
        hub.poll([m])
        #expect(input.stops == 1 && !hub.isListening)
        input.queued = [d4Loud]
        hub.poll([m])
        #expect(input.stops == 1 && input.queued.count == 1, "no reads while nobody listens")
        l.midiIn = true
        hub.poll([m])
        #expect(input.starts == 2, "sources are scanned again on each start")
    }

    @Test func aFailedNoteInputStartBacksOff() throws {
        let (_, m, l) = try listModel()
        let input = FakeNoteInput()
        input.failStart = true
        var clock = Date(timeIntervalSince1970: 1000)
        let hub = NoteInputHub(input: input, retryDelay: 5, now: { clock })
        l.midiIn = true
        for _ in 0..<100 { hub.poll([m]) }
        #expect(input.starts == 1, "no retry storm")
        clock.addTimeInterval(6)
        hub.poll([m])
        #expect(input.starts == 2)
        input.failStart = false
        clock.addTimeInterval(6)
        hub.poll([m])
        #expect(input.starts == 3 && hub.isListening)
    }

    @Test func deletingTheFirstTempoSaysItStays() throws {
        let (f, m, l) = try listModel(.sequence(3))
        withExtendedLifetime(m) {}
        l.delete(["t0"])
        #expect(f.edits.isEmpty)
        #expect(m.editor.status == "The first tempo change stays.")
        l.delete(["m0"])
        #expect(m.editor.status == "The first meter change stays.")
    }

    @Test func insertUsesTheTransportOnlyOnItsOwnSequence() throws {
        let (f, m, l) = try listModel()
        f.state.seq = 4
        f.state.tick = 30_720
        m.poll()
        l.insert(.cc)
        guard case .event(.insert(_, _, let other))? = f.edits.last else {
            Issue.record("no insert")
            return
        }
        #expect(other.first?.tick == 0, "the transport is on another sequence")
        f.state.seq = 3
        m.poll()
        l.insert(.cc)
        guard case .event(.insert(_, _, let own))? = f.edits.last else {
            Issue.record("no insert")
            return
        }
        #expect(own.first?.tick == 30_720)
    }

    @Test func aCoreErrorFromAnEditReachesTheStatusLine() throws {
        let (f, m, l) = try listModel()
        f.editError = VizError.Model(message: "lane is locked")
        #expect(!l.commit("3", row: "e1", column: .position))
        #expect(m.editor.status == "lane is locked")
    }

    @Test func columnsAndSmpteComeFromThePreferences() throws {
        let (f, m, l) = try listModel()
        withExtendedLifetime(f) {}
        var c = ListColumns()
        c.smpte = true
        c.fps = 25
        l.setColumns(c)
        #expect(l.columns.contains(.smpte))
        let row = try #require(l.row("e2"))
        #expect(ListText.cell(row, .smpte, l.context) == "00:00:00:12", "a quarter at 120 BPM is half a second")
        #expect(m.listPreferences.columns.fps == 25)
    }
}
