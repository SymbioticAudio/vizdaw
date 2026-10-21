import Foundation
import Testing
import VizDAWCore
@testable import VizDAWAppModel

/// Recording in the app (plan 5c Task 13): Record asks the core with the
/// transport's replace, punch and loop; Record again stops; keep and
/// discard apply only while loop recording; step entry refuses Record;
/// and a crash's recordings are offered once, unless the control channel
/// is on.
@MainActor
@Suite struct RecordModelTests {
    let host = FakeControlHost()

    func demo(_ f: FakeSession = demoFake()) throws -> (DocumentStore, UUID, DocumentModel, FakeSession) {
        let s = DocumentStore(makeEmpty: { FakeSession([]) }, makeDemo: { f }, open: { _ in demoFake() })
        let id = try #require(s.newDemo())
        return (s, id, try #require(s.model(id)), f)
    }

    static func row(_ path: String, known: Bool) -> RecoveredFileRow {
        RecoveredFileRow(path: path, frames: 4800, sizeKnown: known, readable: true)
    }

    func run(_ id: String, _ m: DocumentModel, _ s: DocumentStore, _ doc: UUID) -> ControlFailure? {
        if case .failed(let f) = ControlCommands.run(id, args: [], doc: (doc, m), store: s, host: host) { return f }
        return nil
    }

    @Test func recordAsksWithTheTransportsModesAndStopsOnTheSecondPress() throws {
        let (s, id, m, f) = try demo()
        m.select(3)
        m.editor.updateLayout {
            $0.transport.overdub = true
            $0.transport.punch = true
            $0.transport.punchIn = 15_360
            $0.transport.punchOut = 61_440
        }
        m.toggleLoop()
        m.record.preRollBars = 2
        #expect(TransportCommands.enabled("transport.record", m))
        #expect(run("transport.record", m, s, id) == nil)
        let call = try #require(f.recordCalls.last)
        #expect(call.seq == 3)
        #expect(
            call.spec
                == RecordSpecRow(
                    replace: false, punch: TickRange(start: 15_360, end: 61_440), looped: true, countInBars: 0,
                    preRollBars: 2, passesAsTakes: true))
        #expect(m.record.isRecording && m.record.isLoopRecording)
        // Keep and discard while loop recording.
        #expect(TransportCommands.enabled("record.keep_pass", m))
        #expect(run("record.keep_pass", m, s, id) == nil)
        #expect(run("record.discard_pass", m, s, id) == nil)
        #expect(f.passPresses == [true, false])
        // Record again stops (and finishes the pass).
        #expect(run("transport.record", m, s, id) == nil)
        #expect(!m.record.isRecording && !f.state.playing)
        #expect(!TransportCommands.enabled("record.keep_pass", m))
        #expect(run("record.keep_pass", m, s, id)?.code == .unavailable)
        #expect(f.recordCalls.count == 1)
    }

    @Test func recordNeedsASequenceAndNoStepEntry() throws {
        let (s, id, m, f) = try demo()
        m.select(nil)
        #expect(!TransportCommands.enabled("transport.record", m))
        #expect(run("transport.record", m, s, id)?.code == .unavailable)
        m.select(3)
        _ = ControlCommands.run("step.entry", args: [], doc: (id, m), store: s, host: host)
        #expect(m.stepEntry.isOn)
        #expect(run("transport.record", m, s, id) == .of(.failed, RecordModel.stepEntryOn))
        m.stepEntry.stop()
        f.recordError = .Model(message: "arm a lane that has an input to record")
        #expect(run("transport.record", m, s, id) == .of(.failed, "arm a lane that has an input to record"))
        f.recordError = .Busy(message: "Stop recording first.")
        #expect(run("transport.record", m, s, id)?.code == .busy)
        #expect(f.recordCalls.isEmpty)
    }

    @Test func aCrashsRecordingsAreOfferedOnceAndAnswered() throws {
        let f = demoFake()
        f.recovered = [Self.row("Media/Recorded/Mic 2026-10-20 120000.caf", known: false)]
        let (_, _, m, _) = try demo(f)
        #expect(m.record.showsRecoveryAlert(channelOn: false))
        #expect(!m.record.showsRecoveryAlert(channelOn: true))
        #expect(m.record.recoveryMessage.contains("Mic 2026-10-20 120000"))
        m.record.answerRecovery(import: true)
        #expect(f.recoverAnswers == [true])
        #expect(!m.record.showsRecoveryAlert(channelOn: false))
        // "Later": nothing is asked of the core.
        let g = demoFake()
        g.recovered = [Self.row("Media/Recorded/a.caf", known: false), Self.row("Media/Recorded/b.caf", known: false)]
        let (_, _, n, _) = try demo(g)
        #expect(n.record.recoveryMessage.hasPrefix("2 recordings "))
        n.record.answerRecovery(import: nil)
        #expect(g.recoverAnswers.isEmpty && !n.record.showsRecoveryAlert(channelOn: false))
        // Under the control channel nothing is offered (`record.recover`
        // answers).
        let h = demoFake()
        h.recovered = [Self.row("Media/Recorded/a.caf", known: false)]
        let (_, _, o, _) = try demo(h)
        o.record.answerRecovery(import: nil)
        o.record.offerRecoveryIfNeeded(channelOn: true)
        #expect(!o.record.showsRecoveryAlert(channelOn: false))
    }

    @Test func thePollFollowsTheRecording() throws {
        let (s, id, m, f) = try demo()
        m.select(3)
        _ = run("transport.record", m, s, id)
        #expect(m.record.isRecording)
        // The core ended it by itself (a full disk): the poll notices.
        f.recordRow.recording = false
        f.state.playing = false
        m.poll()
        #expect(!m.record.isRecording)
    }

    // Pre-flight M9: a take that was undone, then saved and closed, is not
    // "left by VizDAW quitting unexpectedly".
    @Test func theRecoveryWordingTellsCrashLeftoversFromTakesNotInTheProject() throws {
        let crash = demoFake()
        crash.recovered = [Self.row("Media/Recorded/a.caf", known: false)]
        let (_, _, c, _) = try demo(crash)
        #expect(c.record.recoveryMessage.contains("quitting unexpectedly"))

        let undone = demoFake()
        undone.recovered = [Self.row("Media/Recorded/Mic 1.caf", known: true)]
        let (_, _, u, _) = try demo(undone)
        #expect(!u.record.recoveryMessage.contains("quitting unexpectedly"))
        #expect(u.record.recoveryMessage.contains("not in the project"))
        #expect(u.record.recoveryMessage.contains("Mic 1"))

        let both = demoFake()
        both.recovered = [
            Self.row("Media/Recorded/a.caf", known: false), Self.row("Media/Recorded/b.caf", known: true),
        ]
        let (_, _, m, _) = try demo(both)
        #expect(m.record.recoveryMessage.hasPrefix("2 recordings "))
        #expect(m.record.recoveryMessage.contains("not in the project"))
        #expect(m.record.recoveryMessage.contains("quitting unexpectedly"))
    }

    @Test func launchRecoveryRunsOnceAndPostsItsLineThroughTheQueue() throws {
        let s = DocumentStore(makeEmpty: { FakeSession([]) }, makeDemo: { demoFake() }, open: { _ in demoFake() })
        var calls = 0
        s.recoverUntitled = {
            calls += 1
            return "2 recordings were moved to the Trash."
        }
        s.alert = AlertItem(title: "Busy", message: "showing")
        s.runLaunchRecovery()
        s.runLaunchRecovery()
        #expect(calls == 1)
        #expect(s.takeAlert()?.message == "showing")
        #expect(s.takeAlert()?.title == "Recordings Moved to the Trash")
        #expect(s.takeAlert() == nil)
    }

    @Test func openingAnOpenProjectFrontsItsWindowInsteadOfFailing() throws {
        let f = demoFake()
        f.savedPath = "/tmp/Songs/Take.vizdaw"
        var opened = 0
        let s = DocumentStore(makeEmpty: { FakeSession([]) }, makeDemo: { f }, open: { _ in opened += 1; return demoFake() })
        let id = try #require(s.newDemo())
        var fronted: [UUID] = []
        s.activate = { fronted.append($0) }
        #expect(s.open(path: "/tmp/songs/take.vizdaw") == nil)
        #expect(fronted == [id] && opened == 0 && s.alert == nil)
        #expect(s.documentID(forPath: "/tmp/Songs/Other.vizdaw") == nil)
        // The control channel gets the clear error, and no window is fronted.
        fronted = []
        let out = ControlOpen.open(path: "/tmp/Songs/Take.vizdaw", answers: RecoveryAnswersRow(plugins: nil, restoreAutosave: nil), store: s)
        guard case .failed(let fail) = out else { Issue.record("\(out)"); return }
        #expect(fail == .of(.busy, "Take.vizdaw is already open in another window."))
        #expect(fronted.isEmpty && s.alert == nil && opened == 0)
    }

    // Review M3: a failed or busy recovery keeps the offer.
    @Test func aBusyOrFailedRecoveryKeepsTheOfferAndReoffersIt() throws {
        let f = demoFake()
        f.recovered = [Self.row("Media/Recorded/a.caf", known: false)]
        let (_, _, m, _) = try demo(f)
        f.recoverError = .Busy(message: "busy")
        m.record.answerRecovery(import: true)
        #expect(!m.record.showsRecoveryAlert(channelOn: false) && !m.record.recoveryOffer.isEmpty)
        #expect(m.alert == nil)
        // The alert's own dismissal (nil) must not drop the held offer.
        m.record.answerRecovery(import: nil)
        #expect(!m.record.recoveryOffer.isEmpty)
        m.poll()
        #expect(m.record.showsRecoveryAlert(channelOn: false))
        m.record.answerRecovery(import: true)
        #expect(f.recoverAnswers == [true] && m.record.recoveryOffer.isEmpty)
        // A real failure keeps the offer and shows the error.
        f.recovered = [Self.row("Media/Recorded/b.caf", known: false)]
        m.record.offerRecoveryIfNeeded(channelOn: false)
        f.recoverError = .Io(message: "disk")
        m.record.answerRecovery(import: false)
        #expect(m.alert?.title == "Recovery Failed" && !m.record.recoveryOffer.isEmpty)
        m.poll()
        #expect(m.record.showsRecoveryAlert(channelOn: false))
    }

    // Review M2: a failing recordState is shown, not swallowed.
    @Test func aFailingRecordStateIsSurfaced() throws {
        let (_, _, m, f) = try demo()
        f.recordStateError = .Engine(message: "no state")
        m.record.refresh()
        #expect(m.editor.status == "no state")
        f.recordStateError = .Busy(message: "busy")
        m.editor.status = nil
        m.record.refresh()
        #expect(m.editor.status == nil)
    }
}

/// The record and lane commands over the control channel (plan 5c Task
/// 14): `record.set` sets the transport's modes and the record options,
/// `record.recover` answers the offer, `lane.set_input` and
/// `lane.set_monitor` edit lanes (channels from 1), `midi.inject` sends
/// hex bytes, and `state.get record` / `input` read the core.
@MainActor
@Suite struct RecordControlTests {
    let host = FakeControlHost()

    func demo() throws -> (DocumentStore, UUID, DocumentModel, FakeSession) {
        let f = demoFake()
        let s = DocumentStore(makeEmpty: { FakeSession([]) }, makeDemo: { f }, open: { _ in demoFake() })
        let id = try #require(s.newDemo())
        return (s, id, try #require(s.model(id)), f)
    }

    func run(_ id: String, _ pairs: [(String, ControlValue)], _ m: DocumentModel, _ s: DocumentStore, _ doc: UUID)
        -> ControlFailure?
    {
        let args = pairs.map { ControlArg(name: $0.0, value: $0.1) }
        if case .failed(let f) = ControlCommands.run(id, args: args, doc: (doc, m), store: s, host: host) { return f }
        return nil
    }

    @Test func recordSetSetsTheModesAndOptions() throws {
        let (s, id, m, _) = try demo()
        #expect(
            run(
                "record.set",
                [
                    ("replace", .flag(value: false)), ("punch", .flag(value: true)), ("loop", .flag(value: true)),
                    ("pre_roll", .int(value: 2)), ("passes_as_takes", .flag(value: false)),
                ], m, s, id) == nil)
        #expect(m.editor.layout.transport.overdub && m.editor.layout.transport.punch && m.looping)
        #expect(m.record.preRollBars == 2 && !m.record.passesAsTakes)
        let spec = m.record.spec()
        #expect(!spec.replace && spec.looped && spec.punch != nil && !spec.passesAsTakes)
        // Loop already on stays on.
        _ = run("record.set", [("loop", .flag(value: true))], m, s, id)
        #expect(m.looping)
    }

    @Test func laneInputsAndMonitorsAreLaneEdits() throws {
        let (s, id, m, f) = try demo()
        #expect(
            run(
                "lane.set_input",
                [
                    ("seq", .int(value: 3)), ("lane", .int(value: 31)), ("kind", .text(value: "audio")),
                    ("channel", .int(value: 3)), ("stereo", .flag(value: true)),
                ], m, s, id) == nil)
        #expect(f.edits.last == .lane(edit: .setInput(seq: 3, lane: 31, input: .audio(first: 2, stereo: true))))
        _ = run(
            "lane.set_input",
            [
                ("seq", .int(value: 3)), ("lane", .int(value: 31)), ("kind", .text(value: "midi")),
                ("port", .text(value: "Keys")), ("midi_channel", .int(value: 16)),
            ], m, s, id)
        #expect(f.edits.last == .lane(edit: .setInput(seq: 3, lane: 31, input: .midi(port: "Keys", channel: 15))))
        _ = run(
            "lane.set_input", [("seq", .int(value: 3)), ("lane", .int(value: 31)), ("kind", .text(value: "none"))],
            m, s, id)
        #expect(f.edits.last == .lane(edit: .setInput(seq: 3, lane: 31, input: nil)))
        _ = run(
            "lane.set_monitor", [("seq", .int(value: 3)), ("lane", .int(value: 31)), ("mode", .text(value: "off"))],
            m, s, id)
        #expect(f.edits.last == .lane(edit: .setMonitor(seq: 3, lane: 31, monitor: .off)))
        // The core's refusal is the call's failure.
        f.editError = .Model(message: "input.channels out of range")
        #expect(
            run(
                "lane.set_input",
                [("seq", .int(value: 3)), ("lane", .int(value: 31)), ("kind", .text(value: "audio"))], m, s, id)?
                .message.contains("input.channels") == true)
    }

    @Test func midiInjectSendsHexBytes() throws {
        let (s, id, m, f) = try demo()
        #expect(run("midi.inject", [("port", .text(value: "Keys")), ("bytes", .text(value: "90 3c 64"))], m, s, id) == nil)
        #expect(f.injected.last?.port == "Keys" && f.injected.last?.bytes == Data([0x90, 0x3C, 0x64]))
        for bad in ["", "9g", "900", "90,3c", String(repeating: "90 ", count: 1025)] {
            #expect(
                run("midi.inject", [("port", .text(value: "Keys")), ("bytes", .text(value: bad))], m, s, id)?.code
                    == .invalidParams, "\(bad.prefix(10))")
        }
        #expect(ControlLanes.hexBytes("f0 7E f7") == Data([0xF0, 0x7E, 0xF7]))
    }

    @Test func hostileLaneParamsAreInvalid() throws {
        let (s, id, m, f) = try demo()
        let before = f.edits.count
        let lane: [(String, ControlValue)] = [("seq", .int(value: 3)), ("lane", .int(value: 31))]
        for extra: [(String, ControlValue)] in [
            [("kind", .text(value: "video"))],
            [("kind", .text(value: "audio")), ("channel", .int(value: 0))],
            [("kind", .text(value: "audio")), ("channel", .int(value: 257))],
            [("kind", .text(value: "midi")), ("midi_channel", .int(value: 17))],
            [("kind", .text(value: "midi")), ("midi_channel", .int(value: 0))], [],
        ] {
            #expect(run("lane.set_input", lane + extra, m, s, id)?.code == .invalidParams)
        }
        #expect(
            run("lane.set_monitor", lane + [("mode", .text(value: "loud"))], m, s, id)?.code == .invalidParams)
        #expect(
            run(
                "lane.set_monitor",
                [("seq", .int(value: -1)), ("lane", .int(value: 31)), ("mode", .text(value: "off"))], m, s, id)?
                .code == .invalidParams)
        #expect(f.edits.count == before)
        // Wrong types are ignored as absent, so the required ids are missing.
        #expect(
            run(
                "lane.set_monitor",
                [("seq", .text(value: "3")), ("lane", .int(value: 31)), ("mode", .text(value: "off"))], m, s, id)?
                .code == .invalidParams)
        #expect(ControlLanes.monitor("OFF") == nil && ControlLanes.monitor(nil) == nil)
        #expect(run("midi.inject", [("port", .text(value: "Keys"))], m, s, id)?.code == .invalidParams)
        #expect(f.injected.isEmpty)
    }

    @Test func recordWhileRecordingStopsAndRecordSetIsRefusedThen() throws {
        let (s, id, m, f) = try demo()
        #expect(run("transport.record", [], m, s, id) == nil)
        #expect(f.recordCalls.count == 1 && m.record.isRecording)
        #expect(run("record.set", [("loop", .flag(value: true))], m, s, id)?.code == .unavailable)
        #expect(run("transport.record", [], m, s, id) == nil)
        #expect(!m.record.isRecording)
    }

    // Task 15: a pass that ends by itself (the sequence's end) while no
    // poll runs (the window not key and the transport stopped) leaves the
    // last report saying "recording"; the commands read the core first.
    @Test func aPassThatEndedByItselfIsNotRecordingForTheCommands() throws {
        let (s, id, m, f) = try demo()
        #expect(run("transport.record", [], m, s, id) == nil)
        #expect(m.record.isRecording)
        f.recordRow.recording = false
        f.state.playing = false
        #expect(run("record.set", [("loop", .flag(value: true))], m, s, id) == nil)
        #expect(m.looping)
        #expect(run("transport.record", [], m, s, id) == nil)
        #expect(f.recordCalls.count == 2 && m.record.isRecording)
    }

    // Review M3: the core's refusal of the payload is invalid_params.
    @Test func aPayloadTheCoreRefusesIsInvalidParams() throws {
        let (s, id, m, f) = try demo()
        f.injectError = .Model(message: "byte 0xf0 does not start a channel message")
        for bytes in ["f0 7e f7", "3c 64", "90 3c"] {
            let failure = run("midi.inject", [("port", .text(value: "Keys")), ("bytes", .text(value: bytes))], m, s, id)
            #expect(failure?.code == .invalidParams, "\(bytes)")
        }
        // An engine fault stays failed.
        f.injectError = .Engine(message: "no engine")
        #expect(
            run("midi.inject", [("port", .text(value: "Keys")), ("bytes", .text(value: "90 3c 64"))], m, s, id)?.code
                == .failed)
    }

    // Review M4: hex pairs only.
    @Test func hexBytesWantPairs() {
        #expect(ControlLanes.hexBytes("9 3c 64") == nil)
        #expect(ControlLanes.hexBytes("90 3 64") == nil)
        #expect(ControlLanes.hexBytes("90 3c 64") == Data([0x90, 0x3C, 0x64]))
    }

    // Review M5: record.set validates first, applies all or nothing, and waits for a gesture.
    @Test func recordSetAppliesAllOrNothing() throws {
        let (s, id, m, f) = try demo()
        let overdub = m.editor.layout.transport.overdub
        // Out of range: refused before any option is applied.
        #expect(
            run("record.set", [("replace", .flag(value: !overdub)), ("count_in", .int(value: 9))], m, s, id)?.code
                == .invalidParams)
        #expect(
            run("record.set", [("replace", .flag(value: !overdub)), ("pre_roll", .int(value: -1))], m, s, id)?.code
                == .invalidParams)
        #expect(m.editor.layout.transport.overdub == overdub && m.record.countInBars == 0)
        // The loop fails: nothing else changed.
        f.loopError = .Engine(message: "no loop")
        #expect(
            run(
                "record.set",
                [("loop", .flag(value: true)), ("replace", .flag(value: !overdub)), ("pre_roll", .int(value: 3))],
                m, s, id) != nil)
        #expect(m.editor.layout.transport.overdub == overdub && m.record.preRollBars == 0 && !m.looping)
        f.loopError = nil
        // An open gesture refuses it.
        #expect(m.editor.beginGesture("Move", lanes: [], origin: .other))
        #expect(
            run("record.set", [("replace", .flag(value: !overdub))], m, s, id)?.code == .unavailable)
        #expect(m.editor.layout.transport.overdub == overdub)
    }

    // Review M6: params that do not belong to the kind are refused.
    @Test func laneInputParamsMustFitTheKind() throws {
        let (s, id, m, f) = try demo()
        let lane: [(String, ControlValue)] = [("seq", .int(value: 3)), ("lane", .int(value: 31))]
        for extra: [(String, ControlValue)] in [
            [("kind", .text(value: "audio")), ("port", .text(value: "Keys"))],
            [("kind", .text(value: "audio")), ("midi_channel", .int(value: 2))],
            [("kind", .text(value: "midi")), ("channel", .int(value: 2))],
            [("kind", .text(value: "midi")), ("stereo", .flag(value: true))],
            [("kind", .text(value: "none")), ("channel", .int(value: 2))],
            [("kind", .text(value: "none")), ("port", .text(value: "Keys"))],
        ] {
            #expect(run("lane.set_input", lane + extra, m, s, id)?.code == .invalidParams)
        }
        #expect(f.edits.isEmpty)
        #expect(
            run("lane.set_input", lane + [("kind", .text(value: "audio")), ("channel", .int(value: 2))], m, s, id)
                == nil)
    }

    // Review M7: state.get record reads once and leaves the status line alone.
    @Test func stateGetRecordIsSideEffectFree() throws {
        let (_, id, m, f) = try demo()
        let before = f.recordStateCalls
        m.editor.status = "kept"
        _ = ControlStates.state(.record, id, m)
        #expect(f.recordStateCalls == before + 1)
        f.recordStateError = .Engine(message: "no state")
        #expect(ControlStates.state(.record, id, m) == nil)
        #expect(m.editor.status == "kept")
    }

    @Test func recoverAnswersTheOfferAndStateGetReadsTheCore() throws {
        let (s, id, m, f) = try demo()
        #expect(run("record.recover", [("action", .text(value: "import"))], m, s, id)?.code == .unavailable)
        f.recovered = [RecoveredFileRow(path: "Media/Recorded/a.caf", frames: 10, sizeKnown: false, readable: true)]
        #expect(run("record.recover", [("action", .text(value: "trash"))], m, s, id) == nil)
        #expect(f.recoverAnswers == [false])
        f.recordRow.frames = 4800
        guard case .record(_, let st)? = ControlStates.state(.record, id, m) else {
            Issue.record("record")
            return
        }
        #expect(st.frames == 4800)
        f.inputRow.channels = 3
        guard case .inputState(_, let info)? = ControlStates.state(.input, id, m) else {
            Issue.record("input")
            return
        }
        #expect(info.channels == 3)
    }
}
