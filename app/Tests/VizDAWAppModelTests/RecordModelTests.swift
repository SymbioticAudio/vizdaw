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
}
