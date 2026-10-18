import Foundation
import Testing
@testable import VizDAWAppModel
import VizDAWCore

@MainActor
final class FakeControlHost: ControlHost {
    var calls: [String] = []
    var nudge = NudgeSettings()
    func showMixer(_ doc: UUID) { calls.append("mixer") }
    func showPluginManager() { calls.append("plugins") }
    func tearOff(_ doc: UUID, kind: DetailEditorKind) { calls.append("tear \(kind.rawValue)") }
    func closeDocument(_ doc: UUID) { calls.append("close") }
}

@MainActor
@Suite struct ControlCommandsTests {
    let host = FakeControlHost()

    /// A store with one demo document (Song selected) and its fake session.
    func demo() throws -> (DocumentStore, UUID, DocumentModel, FakeSession) {
        let f = demoFake()
        let s = DocumentStore(makeEmpty: { FakeSession([]) }, makeDemo: { f }, open: { _ in demoFake() })
        let id = try #require(s.newDemo())
        let m = try #require(s.model(id))
        return (s, id, m, f)
    }

    func run(_ id: String, _ args: [ControlArg], _ s: DocumentStore, _ doc: UUID?) -> ControlOutcome {
        ControlCommands.run(id, args: args, doc: doc.flatMap { d in s.model(d).map { (d, $0) } }, store: s, host: host)
    }

    func run(_ id: String, _ s: DocumentStore, _ doc: UUID?) -> ControlOutcome { run(id, [], s, doc) }

    func failure(_ o: ControlOutcome) -> ControlFailure? {
        if case .failed(let f) = o { return f }
        return nil
    }

    @Test func everyRegisteredMenuCommandHasAHandler() {
        #expect(Set(controlCommandIds()) == ControlCommands.handled)
    }

    @Test func theDrumMenuRunsOnTheSelectedLaneAndTheDrumGrid() throws {
        let f = drumFake()
        let s = DocumentStore(makeEmpty: { FakeSession([]) }, makeDemo: { f }, open: { _ in demoFake() })
        let id = try #require(s.newDemo())
        let m = try #require(s.model(id))
        // No selected lane, no drum grid: unavailable, nothing sent.
        #expect(failure(run(DrumCommands.markLane, s, id))?.code == .unavailable)
        #expect(failure(run(DrumCommands.addRow, s, id))?.message == "Open a drum grid first.")
        m.editor.setSelection(SelectionRow(timeRange: nil, lanes: [LaneRef(seq: 3, lane: 30)], events: []))
        _ = run(DrumCommands.markLane, [ControlArg(name: "on", value: .flag(value: true))], s, id)
        #expect(f.edits.last == .drum(edit: .markLane(seq: 3, lane: 30, drum: true)))
        m.editor.openDetail(.lane(seq: 3, lane: 40), kind: .drumGrid)
        _ = run(DrumCommands.addRow, s, id)
        #expect(f.edits.last == .drum(edit: .addRow(seq: 3, lane: 40, name: "Closed Hat", note: 42, dest: nil)))
        // Row commands need a selected row.
        m.drumGrid(.pane).selectedRow = nil
        #expect(failure(run(DrumCommands.splitRow, s, id))?.message == DrumGridModel.noRowMessage)
        m.drumGrid(.pane).select(lane: 0, row: 1)
        _ = run(DrumCommands.showVelocity, s, id)
        #expect(f.edits.last == .drum(edit: .showVelocity(seq: 3, lane: 40, row: 2, show: true)))
        _ = run(DrumCommands.splitRow, s, id)
        #expect(f.edits.last == .drum(edit: .splitRow(seq: 3, lane: 40, row: 2)))
        _ = run(DrumCommands.removeRow, s, id)
        #expect(f.edits.last == .drum(edit: .removeRow(seq: 3, lane: 40, row: 2)))
        // Timeline View is a setting: on, then toggled back.
        _ = run(DrumCommands.timeline, [ControlArg(name: "on", value: .flag(value: true))], s, id)
        #expect(m.editor.layout.drumGrid.view == .timeline)
        #expect(DrumCommands.isOn(DrumCommands.timeline, m))
        _ = run(DrumCommands.timeline, s, id)
        #expect(m.editor.layout.drumGrid.view == .matrix)
        // A refusal is the core's message.
        f.editError = .Model(message: "no")
        #expect(failure(run(DrumCommands.removeRow, s, id)) == .of(.failed, "no"))
    }

    @Test func playStopPlaysAndStopsTheSelectedSequence() throws {
        let (s, id, m, f) = try demo()
        #expect(m.selectedSeq == 3)
        guard case .done(.ran(let command, let doc, _, _, _)) = run("transport.play_stop", s, id) else {
            Issue.record("not done")
            return
        }
        #expect(command == "transport.play_stop" && doc == id.uuidString)
        #expect(f.state.playing && f.state.seq == 3 && m.isPlaying)
        _ = run("transport.play_stop", s, id)
        #expect(!f.state.playing)
    }

    @Test func busyAndFailedPlaysAreErrorsNotAlerts() throws {
        let (s, id, m, f) = try demo()
        f.playError = .Busy(message: "plugin host is busy")
        #expect(failure(run("transport.play_stop", s, id))?.code == .busy)
        f.playError = .NoDevice(message: "none")
        let e = try #require(failure(run("transport.play_stop", s, id)))
        #expect(e.code == .failed && e.message.hasPrefix("Play Failed: "))
        #expect(m.alert == nil, "no sheet appears")
    }

    @Test func saveNeedsAPathUntilTheDocumentHasOne() throws {
        let (s, id, _, f) = try demo()
        let e = try #require(failure(run("file.save", s, id)))
        #expect(e.code == .dialogRequired && e.dialog == "Save")
        let path = ControlArg(name: "path", value: .text(value: "/tmp/Song.vizdaw"))
        guard case .done = run("file.save", [path], s, id) else {
            Issue.record("save with a path failed")
            return
        }
        #expect(f.savedPath == "/tmp/Song.vizdaw")
        guard case .done = run("file.save", s, id) else {
            Issue.record("a saved document saves without a path")
            return
        }
        #expect(failure(run("file.save_as", s, id))?.dialog == "Save")
    }

    @Test func disabledCommandsAreUnavailable() throws {
        let (s, id, _, _) = try demo()
        #expect(failure(run("edit.undo", s, id))?.code == .unavailable)
        #expect(failure(run("editors.close_detail", s, id))?.code == .unavailable)
        #expect(failure(run("sequence.back", s, id))?.code == .unavailable)
        #expect(failure(run("editors.nudge_later", s, id))?.code == .unavailable)
        #expect(failure(run("modify.again", s, id))?.code == .unavailable)
        #expect(failure(run("editors.insert_event", s, id))?.code == .unavailable)
    }

    @Test func undoAndRedoOnABusyCoreAreBusyErrors() throws {
        let (s, id, m, f) = try demo()
        var snap = m.snapshot
        snap.undoLabel = "Edit"
        snap.redoLabel = "Edit"
        m.apply(snap)
        f.stepError = .Busy(message: "plugin host is busy")
        #expect(failure(run("edit.undo", s, id))?.code == .busy)
        #expect(failure(run("edit.redo", s, id))?.code == .busy)
        f.stepError = nil
        guard case .done = run("edit.undo", s, id) else {
            Issue.record("undo failed")
            return
        }
        #expect(f.undoCalls == 1)
    }

    @Test func aMissingAutosaveAnswerFailsBeforeAnythingOpens() {
        var opened = 0
        let s = DocumentStore(makeEmpty: { FakeSession([]) }, makeDemo: { demoFake() }, open: { _ in demoFake() })
        s.recovery = Recovery(
            check: { _ in RecoveryInfo(crashed: true, loading: [], autosave: "/p/auto.vizdaw") },
            openWith: { _, _ in
                opened += 1
                return demoFake()
            },
            disable: { _ in true }, shiftHeld: { false }, questions: FakeQuestions())
        let normal = RecoveryAnswersRow(plugins: .normal, restoreAutosave: nil)
        let e = failure(ControlOpen.open(path: "/p/Song.vizdaw", answers: normal, store: s))
        #expect(e?.code == .dialogRequired && e?.dialog == "Restore Autosave")
        #expect(opened == 0 && s.documents.isEmpty, "nothing was opened")
        let yes = RecoveryAnswersRow(plugins: .normal, restoreAutosave: true)
        guard case .opened = ControlOpen.open(path: "/p/Song.vizdaw", answers: yes, store: s) else {
            Issue.record("not opened")
            return
        }
        #expect(opened == 1)
    }

    @Test func aTemplateCommandWithNoTemplatesIsUnavailableNotInvalid() throws {
        let (s, id, m, _) = try demo()
        #expect(m.editor.modifyTemplates().isEmpty, "the menu item would be disabled")
        let name = ControlArg(name: "name", value: .text(value: "Nope"))
        #expect(failure(run("modify.template", [name], s, id))?.code == .unavailable)
        #expect(failure(run("modify.template", s, id))?.code == .unavailable)
    }

    @Test func documentCommandsNeedADocumentButAppCommandsDoNot() {
        let s = DocumentStore(makeEmpty: { FakeSession([]) }, makeDemo: { demoFake() }, open: { _ in demoFake() })
        #expect(failure(run("edit.undo", s, nil))?.code == .noDocument)
        guard case .done = run("window.plugin_manager", s, nil) else {
            Issue.record("plugin manager needs no document")
            return
        }
        #expect(host.calls == ["plugins"])
        #expect(failure(run("file.print", s, nil))?.code == .unknownId)
    }

    @Test func editorsAndWindowsGoWhereTheMenuSendsThem() throws {
        let (s, id, m, _) = try demo()
        _ = run("editors.list", s, id)
        #expect(m.editor.layout.detail?.kind == .list)
        _ = run("editors.strip_chart_window", s, id)
        _ = run("window.mixer", s, id)
        _ = run("file.close", s, id)
        #expect(host.calls == ["tear stripChart", "mixer", "close"])
        _ = run("modify.exact_mode", [ControlArg(name: "on", value: .flag(value: true))], s, id)
        #expect(m.editor.exactMode)
        _ = run("modify.exact_mode", s, id)
        #expect(!m.editor.exactMode, "no `on` toggles")
        let r = ControlStates.state(.editor, id, m)
        guard case .editor(_, let e)? = r else {
            Issue.record("no editor state")
            return
        }
        #expect(e.detail?.kind == "list" && e.detail?.seq == 3 && e.tool == "arrow")
        #expect(ControlStates.facts(m).detailKind == "list")
        _ = run("editors.close_detail", s, id)
        #expect(ControlStates.facts(m).detailKind == "none")
    }

    @Test func storeCommandsAnswerWithTheNewDocument() {
        let s = DocumentStore(
            makeEmpty: { throw VizError.Io(message: "disk full") }, makeDemo: { demoFake() },
            open: { _ in demoFake() })
        guard case .opened(let command, let doc) = run("file.create_demo", s, nil) else {
            Issue.record("no document")
            return
        }
        #expect(command == "file.create_demo" && s.model(doc) != nil)
        let e = failure(run("file.new", s, nil))
        #expect(e?.code == .failed && e?.message == "New Document Failed: disk full")
        #expect(s.alert == nil, "the store's alert was taken, not shown")
    }

    @Test func openAnswersRecoveryQuestionsFromParameters() {
        let q = FakeQuestions()
        var opened: [OpenOptions] = []
        let s = DocumentStore(makeEmpty: { FakeSession([]) }, makeDemo: { demoFake() }, open: { _ in demoFake() })
        s.recovery = Recovery(
            check: { _ in RecoveryInfo(crashed: true, loading: [], autosave: nil) },
            openWith: { _, o in
                opened.append(o)
                return demoFake()
            },
            disable: { _ in true }, shiftHeld: { false }, questions: q)
        let none = RecoveryAnswersRow(plugins: nil, restoreAutosave: nil)
        guard case .failed(let e) = ControlOpen.open(path: "/p/Song.vizdaw", answers: none, store: s) else {
            Issue.record("opened without an answer")
            return
        }
        #expect(e.code == .dialogRequired && e.dialog == "Open with Plugins Off?")
        #expect(s.documents.isEmpty && q.asked.isEmpty, "no dialog, no document")
        #expect(s.recovery?.questions === q, "the app's questions are back")
        let off = RecoveryAnswersRow(plugins: .pluginsOff, restoreAutosave: nil)
        guard case .opened = ControlOpen.open(path: "/p/Song.vizdaw", answers: off, store: s) else {
            Issue.record("not opened")
            return
        }
        #expect(opened == [OpenOptions(pluginsOff: true, autosave: nil)])
    }

    @Test func theSocketComesFromTheFlagThenTheEnvironment() {
        #expect(ControlLaunch.socketPath(arguments: ["VizDAW", "--control", "/a/s"], environment: ["VIZDAW_CONTROL": "/b/s"]) == "/a/s")
        #expect(ControlLaunch.socketPath(arguments: ["VizDAW"], environment: ["VIZDAW_CONTROL": "/b/s"]) == "/b/s")
        #expect(ControlLaunch.socketPath(arguments: ["VizDAW"], environment: ["VIZDAW_CONTROL": ""]) == nil)
        #expect(ControlLaunch.socketPath(arguments: ["VizDAW", "--control"], environment: [:]) == nil)
        #expect(ControlLaunch.strip(["VizDAW", "--control", "/a/x.vizdaw", "--demo"]) == ["VizDAW", "--demo"])
    }
}
