import Foundation
import Testing
import VizDAWCore
@testable import VizDAWAppModel

@MainActor
@Suite struct DocumentModelTests {
    @Test func sidebarSplitsByTopLevel() throws {
        let m = try DocumentModel(session: demoFake())
        #expect(m.topLevel.map(\.name) == ["Song"])
        #expect(m.referenced.map(\.name) == ["Beat", "Groove"])
    }

    @Test func firstTopLevelSequenceIsSelectedAtStart() throws {
        let m = try DocumentModel(session: demoFake())
        #expect(m.selectedSeq == 3)
        #expect(m.lanes.map(\.name) == ["Groove", "Chords"])
    }

    @Test func emptyDocumentSelectsNothing() throws {
        let m = try DocumentModel(session: FakeSession([]))
        #expect(m.selectedSeq == nil)
        #expect(m.lanes.isEmpty)
        #expect(!m.canPlay)
    }

    @Test func selectionSurvivesASnapshotThatKeepsTheSequence() throws {
        let f = demoFake()
        let m = try DocumentModel(session: f)
        m.select(2)
        m.rename(1, to: "Kit")
        #expect(m.selectedSeq == 2)
    }

    @Test func selectionFallsBackWhenTheSequenceIsDeleted() throws {
        let f = demoFake()
        let m = try DocumentModel(session: f)
        m.select(1)
        var next = f.snap
        next.version += 1
        next.sequences.removeAll { $0.id == 1 }
        f.bump(next)
        m.undo()
        #expect(m.selectedSeq == 3)
        #expect(m.lanes.map(\.name) == ["Groove", "Chords"])
    }

    @Test func lanesAreRefetchedOnlyWhenVersionOrSelectionChanges() throws {
        let f = demoFake()
        let m = try DocumentModel(session: f)
        #expect(f.lanesCalls == 1)
        m.poll()
        m.select(3)  // already selected
        #expect(f.lanesCalls == 1)
        m.select(2)
        #expect(f.lanesCalls == 2)
        // Same version again: no fetch.
        f.bump(f.snap)
        m.undo()
        #expect(f.lanesCalls == 2)
        // New version: fetch.
        m.redo()
        #expect(f.lanesCalls == 3)
    }

    @Test func undoAndRedoMenuTitles() throws {
        let f = demoFake()
        let m = try DocumentModel(session: f)
        #expect(m.undoTitle == "Undo")
        #expect(m.redoTitle == "Redo")
        #expect(!m.canUndo && !m.canRedo)
        var next = f.snap
        next.version += 1
        next.undoLabel = "Rename Sequence"
        next.redoLabel = "Set Lane Mute/Solo/Arm"
        f.bump(next)
        m.rename(1, to: "Kit")
        #expect(m.undoTitle == "Undo Rename Sequence")
        #expect(m.redoTitle == "Redo Set Lane Mute/Solo/Arm")
        #expect(m.canUndo && m.canRedo)
    }

    @Test func noDeviceOnPlayBecomesAnAlertAndStaysStopped() throws {
        let f = demoFake()
        f.playError = .NoDevice(message: "no audio output device")
        let m = try DocumentModel(session: f)
        m.playStop()
        #expect(m.alert?.title == "Play Failed")
        #expect(m.alert?.message == "No audio output: no audio output device")
        #expect(!m.isPlaying)
        // The UI stays usable.
        m.rename(1, to: "Kit")
        #expect(f.renames.count == 1)
    }

    @Test func playStopTogglesAndPollIntervalFollows() throws {
        let f = demoFake()
        let m = try DocumentModel(session: f)
        #expect(m.pollInterval == .milliseconds(250))
        m.playStop()
        #expect(m.isPlaying)
        #expect(f.state.seq == 3)
        #expect(m.pollInterval == .milliseconds(33))
        m.playStop()
        #expect(!m.isPlaying)
    }

    @Test func dirtyTitle() throws {
        let f = demoFake()
        let m = try DocumentModel(session: f)
        #expect(m.windowTitle == "Untitled")
        var next = f.snap
        next.version += 1
        next.dirty = true
        f.bump(next)
        m.rename(1, to: "Kit")
        #expect(m.windowTitle == "Untitled — Edited")
    }

    @Test func saveWhenUntitledNeedsSaveAs() throws {
        let f = demoFake()
        let m = try DocumentModel(session: f)
        #expect(m.needsSaveAs)
        m.saveAs(path: "/tmp/x/demo.vizdaw")
        #expect(!m.needsSaveAs)
        #expect(m.windowTitle == "demo")
        #expect(m.alert == nil)
    }

    @Test func blankOrUnchangedRenameSendsNoCommand() throws {
        let f = demoFake()
        let m = try DocumentModel(session: f)
        m.beginRename(1)
        #expect(m.renameDraft == "Beat")
        #expect(!m.transportKeysEnabled)
        m.commitRename("   ")
        #expect(m.transportKeysEnabled)
        m.beginRename(1)
        m.commitRename("Beat")
        #expect(f.renames.isEmpty)
        m.beginRename(1)
        m.commitRename("  Kit ")
        #expect(f.renames.map(\.1) == ["Kit"])
    }

    @Test func toggleMuteFlipsTheLaneFlag() throws {
        let f = demoFake()
        let m = try DocumentModel(session: f)
        let before = f.snap.version
        m.toggleMute(lane: 31)
        #expect(f.snap.version == before + 1)
        #expect(m.alert == nil)
        #expect(f.mutes.count == 1)
        #expect(f.mutes.first?.seq == 3)
        #expect(f.mutes.first?.lane == 31)
        #expect(f.mutes.first?.mute == true)
    }

    @Test func toggleMuteUnmutesAMutedLane() throws {
        let f = demoFake()
        f.laneTable[3] = [lane(30, "Groove"), lane(31, "Chords", mute: true)]
        let m = try DocumentModel(session: f)
        m.toggleMute(lane: 31)
        #expect(f.mutes.map(\.mute) == [false])
    }

    @Test func saveWhileUntitledSurfacesUseSaveAs() throws {
        let f = demoFake()
        let m = try DocumentModel(session: f)
        #expect(m.needsSaveAs)
        m.save()
        #expect(m.alert?.title == "Save Failed")
        #expect(m.alert?.message == "use Save As")
        #expect(m.needsSaveAs)
    }

    @Test func failingPollsRaiseOneAlert() throws {
        let f = demoFake()
        let m = try DocumentModel(session: f)
        f.transportError = .Internal(message: "boom")
        m.poll()
        let first = try #require(m.alert)
        m.poll()
        #expect(m.alert?.id == first.id)
        #expect(first.message == "Internal error: boom")
    }

    @Test func aBusyPollKeepsTheLastTransportWithoutAnAlert() throws {
        let f = demoFake()
        f.state.playing = true
        f.state.tick = 480
        let m = try DocumentModel(session: f)
        // An AU v3 editor opens: the main run loop spins inside plugin code.
        f.transportError = .Busy(message: "plugin host is busy")
        f.state.tick = 960
        m.poll()
        m.poll()
        #expect(m.alert == nil)
        #expect(m.transport.tick == 480)
        #expect(m.isPlaying)
        // The spin ends: the next poll reads the transport again.
        f.transportError = nil
        m.poll()
        #expect(m.transport.tick == 960)
        #expect(m.alert == nil)
    }

    @Test func aDocumentOpenedWhileBusyStartsStopped() throws {
        let f = demoFake()
        f.transportError = .Busy(message: "plugin host is busy")
        let m = try DocumentModel(session: f)
        #expect(m.alert == nil)
        #expect(!m.isPlaying)
        #expect(m.transport.tick == 0)
        f.transportError = nil
        f.state.tick = 240
        m.poll()
        #expect(m.transport.tick == 240)
    }

    @Test func undoAndRedoWhileBusyDoNothingSilently() throws {
        let f = demoFake()
        let m = try DocumentModel(session: f)
        let before = m.snapshot.version
        f.stepError = .Busy(message: "plugin host is busy")
        m.undo()
        m.redo()
        #expect(m.alert == nil)
        #expect(m.snapshot.version == before)
        // Other failures still alert.
        f.stepError = .Model(message: "nothing to undo")
        m.undo()
        #expect(m.alert?.title == "Undo Failed")
    }
}

@MainActor
@Suite struct DocumentStoreTests {
    func store() -> DocumentStore {
        DocumentStore(
            makeEmpty: { FakeSession([]) },
            makeDemo: { demoFake() },
            open: { path in
                if path.hasSuffix("missing.vizdaw") { throw VizError.NotFound(path: path) }
                return demoFake()
            })
    }

    @Test func createsDocumentsPerWindow() throws {
        let s = store()
        let a = try #require(s.newEmpty())
        let b = try #require(s.newDemo())
        #expect(a != b)
        #expect(s.model(b)?.snapshot.sequences.count == 3)
        s.close(a)
        #expect(s.model(a) == nil)
    }

    @Test func openRefusesFoldersWithoutTheExtension() {
        let s = store()
        #expect(s.open(path: "/Users/me/Music") == nil)
        #expect(s.alert?.message == "Music is not a .vizdaw project.")
    }

    @Test func openMissingProjectShowsTheError() {
        let s = store()
        #expect(s.open(path: "/x/missing.vizdaw") == nil)
        #expect(s.alert?.message == "No VizDAW project at /x/missing.vizdaw.")
    }

    @Test func pendingDocumentsAreAdoptedInOrder() throws {
        let s = store()
        let a = s.newDemo()
        let b = s.newEmpty()
        s.enqueue(a)
        s.enqueue(b)
        #expect(s.takePending() == a)
        #expect(s.takePending() == b)
        #expect(s.takePending() == nil)
    }

    @Test func windowWithoutAnIdAdoptsTheQueuedDocument() throws {
        let s = store()
        let a = s.newDemo()
        s.enqueue(a)
        #expect(s.documentForWindow(nil) == a)
        #expect(s.pending.isEmpty)
    }

    @Test func windowWithoutAnIdAndNoQueueGetsANewDocument() throws {
        let s = store()
        let id = try #require(s.documentForWindow(nil))
        #expect(s.model(id) != nil)
        #expect(s.documents.count == 1)
    }

    @Test func windowWithALiveIdKeepsIt() throws {
        let s = store()
        let a = try #require(s.newDemo())
        s.enqueue(s.newEmpty())
        #expect(s.documentForWindow(a) == a)
        #expect(s.pending.count == 1)
    }

    @Test func windowWithAStaleIdAdoptsOrCreates() throws {
        let s = store()
        let stale = UUID()
        let queued = s.newDemo()
        s.enqueue(queued)
        #expect(s.documentForWindow(stale) == queued)
        let closed = try #require(s.newDemo())
        s.close(closed)
        let fresh = try #require(s.documentForWindow(closed))
        #expect(fresh != closed)
        #expect(s.model(fresh) != nil)
    }

    @Test func takeAlertClearsTheStoreAlert() {
        let s = store()
        _ = s.open(path: "/Users/me/Music")
        let a = s.takeAlert()
        #expect(a?.title == "Open Failed")
        #expect(s.alert == nil)
        #expect(s.takeAlert() == nil)
    }

    /// Task 10 review M1: an alert posted while another is showing (the
    /// launch's lost-recordings line) waits its turn, never dropped.
    @Test func postedAlertsQueueBehindTheOneShowing() {
        let s = store()
        _ = s.open(path: "/Users/me/Music")
        s.postAlert(AlertItem(title: "Recordings Moved to the Trash", message: "1 recording"))
        s.postAlert(AlertItem(title: "Third", message: "3"))
        #expect(s.takeAlert()?.title == "Open Failed")
        #expect(s.alert?.title == "Recordings Moved to the Trash")
        #expect(s.takeAlert()?.message == "1 recording")
        #expect(s.takeAlert()?.title == "Third")
        #expect(s.takeAlert() == nil)
        s.postAlert(AlertItem(title: "Alone", message: ""))
        #expect(s.alert?.title == "Alone")
    }

    /// 5c final review M3: the store's own open failures queue behind a
    /// showing alert too (the launch's "Recordings Moved to the Trash"
    /// is never overwritten).
    @Test func openFailuresQueueBehindTheOneShowing() {
        let s = store()
        s.postAlert(AlertItem(title: "Recordings Moved to the Trash", message: "1 recording"))
        _ = s.open(path: "/Users/me/Music")
        _ = s.open(path: "/Users/me/missing.vizdaw")
        #expect(s.takeAlert()?.title == "Recordings Moved to the Trash")
        #expect(s.takeAlert()?.message.contains("not a .vizdaw project") == true)
        #expect(s.takeAlert()?.title == "Open Failed")
        #expect(s.takeAlert() == nil)
    }

    @Test func transportKeysAreOffWhileAnyTextIsEdited() throws {
        let m = try DocumentModel(session: demoFake())
        #expect(m.transportKeysEnabled)
        m.editor.beginTextEditing("test")
        #expect(!m.transportKeysEnabled)
        m.editor.endTextEditing("test")
        m.beginComment(3)
        #expect(!m.transportKeysEnabled)
        m.commentDraft = nil
        m.beginSlot(3)
        #expect(!m.transportKeysEnabled)
        m.slotDraft = nil
        m.editor.referenceDraft = ReferenceDraft(row: referenceRow())
        #expect(!m.transportKeysEnabled)
        m.editor.referenceDraft = nil
        #expect(m.transportKeysEnabled)
    }
}
