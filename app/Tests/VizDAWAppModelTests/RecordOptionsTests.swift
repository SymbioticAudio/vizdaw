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
        // Closing the popover commits a draft (final review I3); one that
        // is not a position is refused, says why, and shows the value.
        r.showsPopover = true
        r.setPunchText(.punchOut, "x")
        r.showsPopover = false
        #expect(r.punchText(.punchOut) == m.fieldText(.punchOut))
        #expect(m.editor.status?.contains("is not a position") == true)
        // Only the punch fields keep drafts (Task 2 review M5).
        r.setPunchText(.position, "9")
        #expect(r.punchText(.position) == m.fieldText(.position))
    }

    /// 5d-2 final review I3: In typed, Tab, Out typed, then the popover
    /// closes without Return: both are committed, never shown but stale.
    @Test func closingThePopoverCommitsEveryPunchDraft() throws {
        let (m, _) = try demo()
        m.select(3)
        let r = m.record
        r.setPunch(true)
        r.showsPopover = true
        r.setPunchText(.punchIn, "3")
        r.setPunchText(.punchOut, "6")
        r.showsPopover = false
        #expect(m.editor.layout.transport.punchIn == 122_880)
        #expect(m.editor.layout.transport.punchOut == 307_200)
        #expect(r.spec().punch == TickRange(start: 122_880, end: 307_200))
    }

    /// The toolbar's Record button records at the typed punch time, not
    /// the old one (final review I3).
    @Test func recordCommitsThePunchDraftsFirst() throws {
        let (m, f) = try armed()
        let r = m.record
        r.setPunch(true)
        r.showsPopover = true
        r.setPunchText(.punchIn, "2")
        r.pressRecord()
        #expect(r.isRecording)
        #expect(f.recordCalls.last?.1.punch?.start == 61_440)
        #expect(r.punchText(.punchIn) == m.fieldText(.punchIn))
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
        _ = r.toggle()
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
        _ = r.toggle()
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
        #expect(!m.wantsSavePanel(forceAs: false))
        m.save()
        #expect(m.editor.status == RecordModel.savedDuringPass)
        #expect(m.alert == nil)
        // Save As cannot move a recording project: it waits for Stop.
        #expect(!m.wantsSavePanel(forceAs: true))
        #expect(m.editor.status == RecordModel.saveAsWhileRecording)
        _ = m.record.toggle()
        f.savesAfterPassRow = false
        #expect(m.wantsSavePanel(forceAs: true))
        f.savedPath = nil
        #expect(m.wantsSavePanel(forceAs: false))
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
}
