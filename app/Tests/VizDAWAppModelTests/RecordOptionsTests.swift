import AppKit
import Foundation
import Testing
import VizDAWCore
@testable import VizDAWAppModel
@testable import VizDAWEditors

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

    /// Ruling 7's bar Record rule (fix round 1 I1): the popover's close
    /// veto keeps it open only for a press on Record with a draft open.
    @Test func thePopoverClosesForEveryPressButRecordWithADraft() {
        #expect(!RecordModel.closeAllowed(pressOnRecord: true, draftOpen: true))
        #expect(RecordModel.closeAllowed(pressOnRecord: true, draftOpen: false))
        #expect(RecordModel.closeAllowed(pressOnRecord: false, draftOpen: true))
        #expect(RecordModel.closeAllowed(pressOnRecord: false, draftOpen: false))
    }

    /// Ruling 7's bar Record rule (fix round 1 I1, M2): a press on the
    /// bar's Record button with a draft open does not close the popover
    /// (its veto reads the press), so the draft stays with the keyboard,
    /// Record refuses and Return then sets the punch. A press elsewhere
    /// closes it and drops the draft.
    @Test func aBarRecordClickWithAnOpenDraftRefusesAndKeepsIt() throws {
        let (m, f) = try armed()
        let r = m.record
        r.setPunch(true)
        let old = m.editor.layout.transport.punchIn
        // Not deferred: a real window number, so the press's `window` is it.
        let window = NSWindow(contentRect: CGRect(x: 0, y: 0, width: 400, height: 60), styleMask: [.titled],
                              backing: .buffered, defer: false)
        let content = NSView(frame: CGRect(x: 0, y: 0, width: 400, height: 60))
        window.contentView = content
        let recordButton = NSView(frame: CGRect(x: 10, y: 10, width: 40, height: 30))
        let chevron = NSView(frame: CGRect(x: 60, y: 10, width: 30, height: 30))
        content.addSubview(recordButton)
        content.addSubview(chevron)
        RecordButtons.register(recordButton)
        defer { RecordButtons.unregister(recordButton) }
        let c = RecordPopoverController(record: r, host: .document, content: NSViewController())
        c.anchor = chevron
        func press(_ x: CGFloat, _ type: NSEvent.EventType = .leftMouseDown) throws -> NSEvent {
            try #require(
                NSEvent.mouseEvent(
                    with: type, location: CGPoint(x: x, y: 20), modifierFlags: [], timestamp: 0,
                    windowNumber: window.windowNumber, context: nil, eventNumber: 0, clickCount: 1, pressure: 1))
        }
        r.togglePopover(in: .document)
        r.punchFocusChanged(from: nil, to: .punchIn)
        r.setPunchText(.punchIn, "5")
        #expect(!c.shouldClose(for: try press(20)), "a press on Record with a draft: it stays open")
        #expect(!c.shouldClose(for: try press(70)), "the chevron toggles it itself")
        let rightOnRecord = try press(20, .rightMouseDown)
        let elsewhere = try press(200)
        #expect(c.shouldClose(for: rightOnRecord) && c.shouldClose(for: elsewhere))
        #expect(c.shouldClose(for: nil), "a key or the app resigning: it closes")
        // The press goes on to Record, which refuses; the draft and its
        // keyboard stay, and Return sets the punch.
        r.pressRecord()
        #expect(m.editor.status == RecordModel.openDraftMessage)
        #expect(f.recordCalls.isEmpty && !r.isRecording)
        #expect(r.showsPopover && r.punchText(.punchIn) == "5" && m.editor.textEditing)
        r.commitPunch(.punchIn)
        #expect(c.shouldClose(for: try press(20)), "no draft: Record closes it")
        m.editor.clearStatus()
        r.pressRecord()
        #expect(r.isRecording && f.recordCalls.last?.spec.punch?.start == 245_760)
        _ = r.toggle()
        // A press elsewhere closes it: the draft is dropped.
        r.setPunchText(.punchIn, "7")
        #expect(c.shouldClose(for: try press(200)))
        c.popoverWillClose(Notification(name: NSPopover.willCloseNotification))
        #expect(!r.showsPopover && !r.hasOpenPunchDraft && m.editor.layout.transport.punchIn == 245_760)
        #expect(old != 245_760)
    }

    /// Fix round 2 N2: the bar's Record button stays enabled while a text
    /// edit holds the keyboard (the popover's punch field or the bar's), so
    /// a click with a draft open reaches Record, which refuses and says why;
    /// the R key's menu item still waits.
    @Test func theRecordButtonIsEnabledWhatever() throws {
        let (m, f) = try armed()
        let r = m.record
        r.setPunch(true)
        #expect(r.buttonEnabled)
        r.showsPopover = true
        r.punchFocusChanged(from: nil, to: .punchIn)
        r.setPunchText(.punchIn, "5")
        #expect(m.editor.textEditing && r.hasOpenPunchDraft)
        #expect(r.buttonEnabled && !TransportCommands.enabled("transport.record", m))
        r.pressRecord()
        #expect(m.editor.status == RecordModel.openDraftMessage && f.recordCalls.isEmpty)
        r.showsPopover = false
        m.editor.beginTextEditing(TransportFieldsView.textOwner)
        r.barPunchTyped(.punchOut, "9")
        #expect(r.buttonEnabled && r.hasOpenPunchDraft)
        r.barPunchEnded()
        m.editor.endTextEditing(TransportFieldsView.textOwner)
        #expect(r.buttonEnabled)
    }

    /// Fix round 2 N3: `popoverShouldClose` itself decides from the event
    /// being dispatched (`NSApp.currentEvent` in the app; a synthetic
    /// mouse-down here, through the controller's `currentEvent` seam).
    @Test func thePopoversVetoReadsTheCurrentEvent() throws {
        let (m, _) = try armed()
        let r = m.record
        r.setPunch(true)
        let window = NSWindow(contentRect: CGRect(x: 0, y: 0, width: 400, height: 60), styleMask: [.titled],
                              backing: .buffered, defer: false)
        let content = NSView(frame: CGRect(x: 0, y: 0, width: 400, height: 60))
        window.contentView = content
        let recordButton = NSView(frame: CGRect(x: 10, y: 10, width: 40, height: 30))
        content.addSubview(recordButton)
        RecordButtons.register(recordButton)
        defer { RecordButtons.unregister(recordButton) }
        let c = RecordPopoverController(record: r, host: .document, content: NSViewController())
        r.togglePopover(in: .document)
        r.setPunchText(.punchIn, "5")
        var current: NSEvent?
        c.currentEvent = { current }
        func press(_ x: CGFloat) throws -> NSEvent {
            try #require(
                NSEvent.mouseEvent(
                    with: .leftMouseDown, location: CGPoint(x: x, y: 20), modifierFlags: [], timestamp: 0,
                    windowNumber: window.windowNumber, context: nil, eventNumber: 0, clickCount: 1, pressure: 1))
        }
        current = try press(20)
        #expect(!c.popoverShouldClose(c.popover), "a press on Record with a draft: vetoed")
        current = try press(300)
        #expect(c.popoverShouldClose(c.popover), "a press elsewhere: it closes")
        current = nil
        #expect(c.popoverShouldClose(c.popover), "no press (a key, the app resigning): it closes")
        current = try press(20)
        r.commitPunch(.punchIn)
        #expect(c.popoverShouldClose(c.popover), "no draft: a press on Record closes it")
    }

    /// Fix round 1 M4: the bar's own Punch In or Out, typed in, refuses
    /// Record with the same message on every path: the bar's Record (the
    /// edit stays), the R key's command, a surface and the channel.
    @Test func aTypedBarPunchTimeRefusesRecordOnEveryPath() throws {
        let f = demoFake()
        var audio = lane(30, "Vocal")
        audio.arm = true
        audio.input = .audio(first: 0, stereo: true, returnOf: nil)
        f.laneTable[3] = [audio]
        let s = DocumentStore(makeEmpty: { FakeSession([]) }, makeDemo: { f }, open: { _ in demoFake() })
        let id = try #require(s.newDemo())
        let m = try #require(s.model(id))
        m.select(3)
        m.record.setPunch(true)
        let v = TransportFieldsView(document: m)
        let window = NSWindow(contentRect: CGRect(x: 0, y: 0, width: 900, height: 80), styleMask: [.titled],
                              backing: .buffered, defer: false)
        let content = NSView(frame: CGRect(x: 0, y: 0, width: 900, height: 80))
        window.contentView = content
        v.frame = CGRect(x: 0, y: 0, width: 700, height: 30)
        content.addSubview(v)
        let recordButton = NSView(frame: CGRect(x: 10, y: 40, width: 40, height: 30))
        content.addSubview(recordButton)
        RecordButtons.register(recordButton)
        defer { RecordButtons.unregister(recordButton) }
        content.layoutSubtreeIfNeeded()
        let punchIn = try #require(v.fields[.punchIn])
        #expect(v.focus(.punchIn))
        v.controlTextDidBeginEditing(Notification(name: NSControl.textDidBeginEditingNotification, object: punchIn))
        punchIn.stringValue = "5"
        v.controlTextDidChange(Notification(name: NSControl.textDidChangeNotification, object: punchIn))
        #expect(m.record.hasOpenPunchDraft)
        // The bar's Record: the press keeps the edit; the button refuses.
        let press = try #require(
            NSEvent.mouseEvent(
                with: .leftMouseDown, location: CGPoint(x: 20, y: 50), modifierFlags: [], timestamp: 0,
                windowNumber: window.windowNumber, context: nil, eventNumber: 0, clickCount: 1, pressure: 1))
        v.handleMouseDown(press)
        #expect(punchIn.currentEditor() != nil && v.isWatching, "the edit stays")
        m.record.pressRecord()
        #expect(m.editor.status == RecordModel.openDraftMessage)
        // The R key's command.
        #expect(TransportCommands.run("transport.record", m) == .refused(RecordModel.openDraftMessage))
        // A surface's Record button (the fake surface path, through idle).
        var surface: [KeyOutcome] = []
        m.surfaceCommandSink = { surface.append(TransportCommands.run($0, m)) }
        f.surfaceEvents = [.command(id: "transport.record", programmatic: false)]
        m.idle()
        #expect(surface == [.refused(RecordModel.openDraftMessage)])
        // The channel.
        let host = FakeControlHost()
        guard case .failed(let e) = ControlCommands.run("transport.record", args: [], doc: (id, m), store: s, host: host)
        else {
            Issue.record("the channel recorded")
            return
        }
        #expect(e.message == RecordModel.openDraftMessage)
        #expect(f.recordCalls.isEmpty && !m.record.isRecording)
        // Return sets it; then Record records there.
        v.controlTextDidEndEditing(
            Notification(
                name: NSControl.textDidEndEditingNotification, object: punchIn,
                userInfo: ["NSTextMovement": NSTextMovement.return.rawValue]))
        #expect(!m.record.hasOpenPunchDraft && !v.isWatching)
        m.editor.clearStatus()
        m.record.pressRecord()
        #expect(m.record.isRecording && f.recordCalls.last?.spec.punch?.start == 245_760)
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
        f.recordError = .Model(message: "arm a track that has an input to record")
        m.record.pressRecord()
        #expect(m.editor.status == "arm a track that has an input to record")
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
