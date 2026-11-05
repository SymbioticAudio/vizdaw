import Foundation
import Testing
import VizDAWCore
@testable import VizDAWAppModel

/// The record popover's model (plan 5d-2 Task 2; spec §6.9): the modes
/// it sets on the transport, the punch fields, the time left on disk, the
/// loop counters, the overdub note, Save while recording and the Record
/// button's refusals.
@MainActor
@Suite(.serialized) struct RecordOptionsTests {
    func demo(_ f: FakeSession = demoFake()) throws -> (DocumentModel, FakeSession) {
        let s = DocumentStore(makeEmpty: { FakeSession([]) }, makeDemo: { f }, open: { _ in demoFake() })
        let id = try #require(s.newDemo())
        return (try #require(s.model(id)), f)
    }

    /// Song (3) with a stereo audio lane and a MIDI lane, both armed.
    func armed() throws -> (DocumentModel, FakeSession) {
        let f = demoFake()
        var audio = lane(30, "Vocal")
        audio.arm = true
        audio.input = .audio(first: 0, stereo: true, returnOf: nil)
        var midi = lane(31, "Keys")
        midi.arm = true
        midi.input = .midi(port: nil, channel: nil)
        f.laneTable[3] = [audio, midi]
        let (m, _) = try demo(f)
        m.select(3)
        m.refreshProjectInfo()
        return (m, f)
    }

    @Test func thePopoverSetsTheTransportsModes() throws {
        let (m, _) = try demo()
        m.select(3)
        let r = m.record
        #expect(r.replaces && !r.punches)
        r.setReplace(false)
        r.setPunch(true)
        #expect(m.editor.layout.transport.overdub && m.editor.layout.transport.punch)
        #expect(!r.spec().replace && r.spec().punch != nil)
        // The punch fields: typed text is a draft until Return.
        r.setPunchText(.punchIn, "2")
        #expect(r.punchText(.punchIn) == "2")
        #expect(m.editor.layout.transport.punchIn == 0)
        r.commitPunch(.punchIn)
        #expect(m.editor.layout.transport.punchIn == 61_440)
        #expect(r.punchText(.punchIn) == m.fieldText(.punchIn))
        // Closing the popover drops a draft (plan 7d Ruling 7): the field
        // shows the value in use, and nothing is said.
        r.showsPopover = true
        r.setPunchText(.punchOut, "x")
        r.showsPopover = false
        #expect(r.punchText(.punchOut) == m.fieldText(.punchOut))
        #expect(m.editor.status == nil)
        // Only the punch fields keep drafts (Task 2 review M5).
        r.setPunchText(.position, "9")
        #expect(r.punchText(.position) == m.fieldText(.position))
    }

    /// Plan 7a final review I2: the mixer window's transport bar has its own
    /// chevron; the popover opens under the one that was clicked.
    @Test func openingInTheMixerWindowDoesNotShowInTheDocumentWindow() throws {
        let (m, _) = try demo()
        let r = m.record
        r.togglePopover(in: .mixerWindow)
        #expect(r.showsPopover(in: .mixerWindow) && !r.showsPopover(in: .document))
        // The other window's chevron moves it there; its own closes it.
        r.togglePopover(in: .document)
        #expect(r.showsPopover(in: .document) && !r.showsPopover(in: .mixerWindow))
        r.togglePopover(in: .document)
        #expect(!r.showsPopover && !r.showsPopover(in: .document))
    }
    /// Plan 7d Ruling 7: In typed, Out typed, then the popover closes
    /// without Return: both drafts are dropped, and the fields show the
    /// range a pass will use.
    @Test func closingThePopoverDropsEveryPunchDraft() throws {
        let (m, _) = try demo()
        m.select(3)
        let r = m.record
        r.setPunch(true)
        let before = m.editor.layout.transport
        r.showsPopover = true
        r.setPunchText(.punchIn, "3")
        r.setPunchText(.punchOut, "6")
        r.showsPopover = false
        #expect(m.editor.layout.transport.punchIn == before.punchIn)
        #expect(m.editor.layout.transport.punchOut == before.punchOut)
        #expect(r.punchText(.punchIn) == m.fieldText(.punchIn) && r.punchText(.punchOut) == m.fieldText(.punchOut))
        #expect(!r.hasOpenPunchDraft && m.editor.status == nil)
    }

    /// Controller ruling I5: Record waits for an open punch draft, from
    /// every path (`toggle()` is the bar's, the R key's, a surface's and
    /// `transport.record`'s), so a pass never punches at the old range
    /// with a typed time on screen. Return sets it; then Record records.
    @Test func recordRefusesWhileAPunchDraftIsOpen() throws {
        let (m, f) = try armed()
        let r = m.record
        r.setPunch(true)
        r.showsPopover = true
        r.setPunchText(.punchIn, "5")
        #expect(r.hasOpenPunchDraft)
        guard case .refused(let why) = r.toggle() else {
            Issue.record("Record with an open draft is refused")
            return
        }
        #expect(why == RecordModel.openDraftMessage)
        #expect(f.recordCalls.isEmpty && !r.isRecording)
        #expect(r.punchText(.punchIn) == "5", "the draft is still there")
        r.commitPunch(.punchIn)
        #expect(r.toggle() == .done && r.isRecording)
        #expect(f.recordCalls.last?.spec.punch?.start == 245_760)
        _ = r.toggle()
        // A draft equal to the field's text is not open.
        r.setPunchText(.punchIn, m.fieldText(.punchIn))
        #expect(!r.hasOpenPunchDraft)
        #expect(r.toggle() == .done && r.isRecording)
    }

    /// 5d-2 final re-review N1: a punch draft that is not a position
    /// refuses Record, so the pass never punches at the old range; the
    /// status line says why. Once the field is set (Return), Record
    /// records at the typed time.
    @Test func aRefusedPunchDraftRefusesRecord() throws {
        let (m, f) = try armed()
        let r = m.record
        r.setPunch(true)
        r.showsPopover = true
        let before = m.fieldText(.punchIn)
        r.setPunchText(.punchIn, "abc")
        r.pressRecord()
        #expect(!r.isRecording && f.recordCalls.isEmpty)
        #expect(m.editor.status == RecordModel.openDraftMessage)
        // 5e-1 Task 1 review m3: the old punch range is kept.
        #expect(m.fieldText(.punchIn) == before)
        r.setPunchText(.punchIn, "2")
        r.commitPunch(.punchIn)
        m.editor.clearStatus()
        r.pressRecord()
        #expect(r.isRecording)
        #expect(f.recordCalls.count == 1 && f.recordCalls.last?.spec.punch?.start == 61_440)
    }

    /// Ruling 7: focus moving to the other punch field commits the one it
    /// left; focus leaving the pair drops the draft.
    @Test func thePopoversPunchFieldsCancelWhenFocusLeavesThem() throws {
        let (m, _) = try demo()
        m.select(3)
        let r = m.record
        r.showsPopover = true
        let before = m.editor.layout.transport.punchIn
        r.punchFocusChanged(from: nil, to: .punchIn)
        r.setPunchText(.punchIn, "3")
        r.punchFocusChanged(from: .punchIn, to: nil)
        #expect(m.editor.layout.transport.punchIn == before)
        #expect(r.punchText(.punchIn) == m.fieldText(.punchIn))
        r.punchFocusChanged(from: nil, to: .punchIn)
        r.setPunchText(.punchIn, "3")
        r.punchFocusChanged(from: .punchIn, to: .punchOut)
        #expect(m.editor.layout.transport.punchIn == 122_880, "moving within the pair commits")
    }

    /// Ruling 7's bar Record rule: a press on the bar's Record button with
    /// an open draft closes the popover, but the draft is kept, Record is
    /// refused and the popover opens again showing it.
    @Test func aBarRecordClickWithAnOpenDraftRefusesAndKeepsIt() throws {
        let (m, f) = try armed()
        let r = m.record
        r.setPunch(true)
        let old = m.editor.layout.transport.punchIn
        r.showsPopover = true
        r.setPunchText(.punchIn, "5")
        r.recordPressBegins()
        r.showsPopover = false
        r.pressRecord()
        #expect(m.editor.status == RecordModel.openDraftMessage)
        #expect(f.recordCalls.isEmpty && !r.isRecording)
        #expect(r.punchText(.punchIn) == "5")
        #expect(r.showsPopover, "it opens again")
        // Without the press beginning on Record, the close drops it.
        m.editor.clearStatus()
        r.showsPopover = false
        #expect(!r.keepDraftsOnClose)
        r.showsPopover = true
        r.setPunchText(.punchIn, "5")
        r.showsPopover = false
        r.pressRecord()
        #expect(r.isRecording && m.editor.status == nil)
        #expect(f.recordCalls.last?.spec.punch?.start == old)
    }

    @Test func theTimeLeftCountsTheArmedAudioChannels() throws {
        let (m, f) = try armed()
        let r = m.record
        #expect(r.armedChannels == 2)
        // 48 kHz, 24-bit, two channels: 288 000 bytes a second.
        let tenMinutes: UInt64 = 288_000 * 600
        RecordModel.freeSpace = { _ in tenMinutes + RecordModel.stopBytes }
        defer { RecordModel.freeSpace = RecordModel.volumeFreeSpace }
        #expect(r.remainingText == "10 min left")
        RecordModel.freeSpace = { _ in 288_000 * 3_900 + RecordModel.stopBytes }
        #expect(r.remainingText == "1 h 5 min left")
        RecordModel.freeSpace = { _ in 1_000 }
        #expect(r.remainingText == "Less than a minute left")
        // While recording, the core's reading counts.
        f.recordRow.freeBytes = 288_000 * 7_200 + RecordModel.stopBytes
        r.toggle()
        #expect(r.isRecording)
        #expect(r.remainingText == "2 h left")
        // MIDI only: no readout.
        f.laneTable[3] = [lane(31, "Keys")]
        f.snap.version += 1
        m.refreshAfterJob()
        #expect(r.armedChannels == 0 && r.remainingText == nil)
    }

    @Test func loopCountersAndTheOverdubNote() throws {
        let (m, f) = try armed()
        let r = m.record
        #expect(r.overdubNote == nil)
        r.setReplace(false)
        #expect(r.overdubNote == RecordModel.overdubNote)
        #expect(r.passText == nil)
        m.toggleLoop()
        f.recordRow.pass = 3
        f.recordRow.kept = 1
        f.recordRow.discarded = 1
        r.toggle()
        #expect(r.passText == "Pass 3 · 1 kept · 1 discarded")
        f.recordRow.kept = 0
        f.recordRow.discarded = 0
        r.refresh()
        #expect(r.passText == "Pass 3")
        f.state.countingIn = true
        m.poll()
        #expect(r.countingIn)
    }

    @Test func saveWhileRecordingSaysWhenTheTakeIsSaved() throws {
        let (m, f) = try armed()
        f.savedPath = "/tmp/P.vizdaw"
        _ = m.record.toggle()
        f.savesAfterPassRow = true
        #expect(m.saveAction(forceAs: false) == .save)
        m.save()
        #expect(m.editor.status == RecordModel.savedDuringPass)
        #expect(m.alert == nil)
        // Save As cannot move a recording project: it waits for Stop.
        m.editor.clearStatus()
        #expect(m.saveAction(forceAs: true) == .refused(RecordModel.saveAsWhileRecording))
        #expect(m.editor.status == nil, "no side effect (T2 M2)")
        // Untitled: its own words (T2 M1).
        f.savedPath = nil
        #expect(m.saveAction(forceAs: false) == .refused(RecordModel.saveUntitledWhileRecording))
        _ = m.record.toggle()
        f.savesAfterPassRow = false
        #expect(m.saveAction(forceAs: false) == .panel)
        f.savedPath = "/tmp/P.vizdaw"
        #expect(m.saveAction(forceAs: true) == .panel)
        #expect(m.saveAction(forceAs: false) == .save)
    }

    /// 5d-2 T2 M2: a pass that ended by itself since the last poll does
    /// not refuse a Save As.
    @Test func saveActionReadsTheRecordingNow() throws {
        let (m, f) = try armed()
        f.savedPath = "/tmp/P.vizdaw"
        _ = m.record.toggle()
        f.recordRow.recording = false  // it ended at the song's end; no poll since
        #expect(m.saveAction(forceAs: true) == .panel)
    }

    /// 5d-2 T3 M1 (Ruling 12): text editing is held only while a punch
    /// field has focus, so R, Space and Return reach the window otherwise.
    @Test func textEditingIsHeldOnlyWhileAPunchFieldHasFocus() throws {
        let (m, _) = try demo()
        m.select(3)
        let r = m.record
        r.showsPopover = true
        #expect(!m.editor.textEditing)
        r.punchFocusChanged(from: nil, to: .punchIn)
        #expect(m.editor.textEditing)
        r.setPunchText(.punchIn, "3")
        r.punchFocusChanged(from: .punchIn, to: .punchOut)
        #expect(m.editor.textEditing && m.editor.layout.transport.punchIn == 122_880, "the old field's draft is in")
        r.punchFocusChanged(from: .punchOut, to: nil)
        #expect(!m.editor.textEditing)
        r.punchFocusChanged(from: nil, to: .punchOut)
        r.showsPopover = false
        #expect(!m.editor.textEditing, "closing ends it")
    }

    @Test func theRecordButtonShowsWhyItWasRefused() throws {
        let (m, f) = try armed()
        f.recordError = .Model(message: "arm a lane that has an input to record")
        m.record.pressRecord()
        #expect(m.editor.status == "arm a lane that has an input to record")
        f.recordError = nil
        m.editor.clearStatus()
        m.record.pressRecord()
        #expect(m.record.isRecording && m.editor.status == nil)
        m.record.pressRecord()
        #expect(!m.record.isRecording)
    }

    /// 5d-2 T2 M4: the time left counts the depth and mono lanes, and
    /// says nothing without a reading.
    @Test func theTimeLeftCountsEveryDepthAndMonoLanes() throws {
        let (m, f) = try armed()
        let r = m.record
        defer { RecordModel.freeSpace = RecordModel.volumeFreeSpace }
        f.laneTable[3]?[0].input = .audio(first: 0, stereo: false, returnOf: nil)
        f.snap.version += 1
        m.refreshAfterJob()
        f.info.format = ProjectFormatRow(sampleRate: 44_100, depth: .int16)
        m.refreshProjectInfo()
        // 44.1 kHz, 16-bit, one channel: 88 200 bytes a second.
        RecordModel.freeSpace = { _ in 88_200 * 600 + RecordModel.stopBytes }
        #expect(r.remainingText == "10 min left")
        f.info.format = ProjectFormatRow(sampleRate: 48_000, depth: .float32)
        m.refreshProjectInfo()
        RecordModel.freeSpace = { _ in 192_000 * 3_600 + RecordModel.stopBytes }
        #expect(r.remainingText == "1 h left")
        RecordModel.freeSpace = { _ in nil }
        #expect(r.remainingText == nil, "no reading, no readout")
        RecordModel.freeSpace = { _ in 0 }
        #expect(r.remainingText == "Less than a minute left")
    }

    /// 5d-2 T2 M3: a titled Save while recording reaches the session.
    @Test func saveWhileRecordingReachesTheSession() throws {
        let (m, f) = try armed()
        f.savedPath = "/tmp/P.vizdaw"
        f.snap.dirty = true
        _ = m.record.toggle()
        m.save()
        #expect(!f.snap.dirty, "the session saved")
    }
}
