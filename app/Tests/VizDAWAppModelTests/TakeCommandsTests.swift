import AppKit
import Foundation
import Testing
import VizDAWCore
@testable import VizDAWAppModel

/// Song (3) with track Groove (30) holding takes Take 1 (300) and Take 2
/// (301, playing), and Chords (31) with its one take, shown.
@MainActor
func takesDemo() throws -> (DocumentModel, FakeSession) {
    let f = editorFake()
    f.laneTable[3]?[0].takes = [
        TakeRow(id: 300, name: "Take 1", midiEventCount: 0, regionCount: 0),
        TakeRow(id: 301, name: "Take 2", midiEventCount: 0, regionCount: 0),
    ]
    f.laneTable[3]?[0].activeTakeId = 301
    f.laneTable[3]?[0].takeCount = 2
    f.laneTable[3]?[0].activeTake = "Take 2"
    f.laneTable[3]?[1].takes = [TakeRow(id: 310, name: "Take 1", midiEventCount: 0, regionCount: 0)]
    f.laneTable[3]?[1].activeTakeId = 310
    let m = try DocumentModel(session: f)
    m.select(3)
    return (m, f)
}

/// The take commands (plan 5c-2 takes Rulings 5 and 6): the Takes menu and
/// the control channel share one implementation, and refuse before any edit.
@MainActor
@Suite struct TakeCommandsTests {
    func arg(_ name: String, _ v: String) -> ControlArg { ControlArg(name: name, value: .text(value: v)) }
    func arg(_ name: String, _ v: Int64) -> ControlArg { ControlArg(name: name, value: .int(value: v)) }

    func failure(_ o: ControlOutcome) -> ControlFailure? {
        if case .failed(let f) = o { return f }
        return nil
    }

    func status(_ o: ControlOutcome) -> String? {
        if case .done(.ran(_, _, _, _, let status)) = o { return status }
        return nil
    }

    @Test func chooseRenameDeleteRunOnTheTrack() throws {
        let (m, f) = try takesDemo()
        let doc = UUID()
        let chose = TakeCommands.run(TakeCommands.choose, args: [arg("lane", 30), arg("take", 300)], doc: doc, m)
        #expect(f.edits.last == .lane(edit: .setActiveTake(seq: 3, lane: 30, take: 300)))
        #expect(status(chose) == "Take 1")
        let renamed = TakeCommands.run(
            TakeCommands.rename, args: [arg("lane", 30), arg("take", 300), arg("name", "Verse")], doc: doc, m)
        #expect(f.edits.last == .lane(edit: .renameTake(seq: 3, lane: 30, take: 300, name: "Verse")))
        #expect(status(renamed) == "Verse")
        let deleted = TakeCommands.run(TakeCommands.delete, args: [arg("lane", 30), arg("take", 301)], doc: doc, m)
        #expect(f.edits.last == .lane(edit: .removeTake(seq: 3, lane: 30, take: 301)))
        #expect(status(deleted) == "Take 2")
        #expect(f.edits.count == 3)
        #expect(TakeCommands.ids == [TakeCommands.choose, TakeCommands.rename, TakeCommands.delete])
        #expect(ControlCommands.handled.isSuperset(of: TakeCommands.ids))
    }

    @Test func takeCommandsRefuseHostileInput() throws {
        let (m, f) = try takesDemo()
        var asked = 0
        m.askTakeName = { _ in
            asked += 1
            return "Nope"
        }
        let doc = UUID()
        let run = { (id: String, args: [ControlArg]) in TakeCommands.run(id, args: args, doc: doc, m) }
        let on30 = [arg("lane", 30)]
        for name in ["a\u{0}b", "  ", ""] {
            let o = run(TakeCommands.rename, on30 + [arg("take", 300), arg("name", name)])
            #expect(failure(o)?.code == .invalidParams, "\(name.debugDescription)")
        }
        #expect(failure(run(TakeCommands.rename, on30 + [arg("take", 300), arg("name", 5)]))?.code == .invalidParams)
        #expect(failure(run(TakeCommands.rename, on30 + [arg("take", 300)]))?.code == .invalidParams)
        for id in TakeCommands.ids.sorted() {
            let extra = id == TakeCommands.rename ? [arg("name", "Verse")] : []
            let unknown = failure(run(id, on30 + [arg("take", 999)] + extra))
            #expect(unknown?.code == .unknownId, "\(id)")
            #expect(unknown?.message == "There is no take 999 on this track.", "\(id)")
            #expect(failure(run(id, on30 + extra))?.code == .invalidParams, "\(id): no take")
            #expect(failure(run(id, on30 + [arg("take", "300")] + extra))?.code == .invalidParams, "\(id)")
            #expect(failure(run(id, on30 + [arg("take", -1)] + extra))?.code == .invalidParams, "\(id)")
            let noTrack = failure(run(id, [arg("lane", 99), arg("take", 300)] + extra))
            #expect(noTrack?.message == "There is no track 99 in this sequence.", "\(id)")
            let noSeq = failure(run(id, [arg("seq", 99), arg("lane", 30), arg("take", 300)] + extra))
            #expect(noSeq?.message == "There is no sequence 99.", "\(id)")
        }
        let only = failure(run(TakeCommands.delete, [arg("lane", 31), arg("take", 310)]))
        #expect(only?.code == .failed)
        #expect(only?.message == TakeCommands.onlyTake)
        // The menu's entries refuse the same way, in the status line.
        #expect(TakeCommands.delete(seq: 3, lane: 31, take: 310, m) == .refused(TakeCommands.onlyTake))
        #expect(m.editor.status == TakeCommands.onlyTake)
        #expect(TakeCommands.rename(seq: 3, lane: 30, take: 300, to: " ", m) != .done)
        #expect(TakeCommands.choose(seq: 3, lane: 30, take: 999, m) != .done)
        #expect(f.edits.isEmpty)
        #expect(asked == 0)
        #expect(m.alert == nil)
    }

    @Test func takeCommandsWaitWhileRecording() throws {
        let (m, f) = try takesDemo()
        f.recordRow.recording = true
        m.record.refresh()
        #expect(m.record.isRecording)
        let doc = UUID()
        for id in TakeCommands.ids.sorted() {
            let extra = id == TakeCommands.rename ? [arg("name", "Verse")] : []
            let o = TakeCommands.run(id, args: [arg("lane", 30), arg("take", 300)] + extra, doc: doc, m)
            #expect(failure(o)?.code == .busy, "\(id)")
            #expect(failure(o)?.message == TrackCommands.recordingBusy, "\(id)")
        }
        #expect(TakeCommands.choose(seq: 3, lane: 30, take: 300, m) == .busy(TrackCommands.recordingBusy))
        #expect(TakeCommands.rename(seq: 3, lane: 30, take: 300, to: "Verse", m) == .busy(TrackCommands.recordingBusy))
        #expect(TakeCommands.delete(seq: 3, lane: 30, take: 301, m) == .busy(TrackCommands.recordingBusy))
        #expect(m.editor.status == TrackCommands.recordingBusy)
        #expect(f.edits.isEmpty)
    }

    @Test func renameAskingUsesTheHook() throws {
        let (m, f) = try takesDemo()
        var current: [String] = []
        m.askTakeName = { name in
            current.append(name)
            return "Chorus"
        }
        TakeCommands.renameAsking(seq: 3, lane: 30, take: 301, m)
        #expect(current == ["Take 2"])
        #expect(f.edits.last == .lane(edit: .renameTake(seq: 3, lane: 30, take: 301, name: "Chorus")))
        m.askTakeName = { _ in nil }
        TakeCommands.renameAsking(seq: 3, lane: 30, take: 301, m)
        #expect(f.edits.count == 1, "cancelled: nothing is sent")
        m.askTakeName = nil
        TakeCommands.renameAsking(seq: 3, lane: 30, take: 301, m)
        #expect(f.edits.count == 1)
        #expect(m.editor.status == TakeCommands.noAsk)
    }
}
