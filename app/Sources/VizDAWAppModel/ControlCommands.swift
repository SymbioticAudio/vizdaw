import Foundation
import VizDAWCore

/// What a control call came to (control spec §4.3).
public enum ControlOutcome {
    /// Answered.
    case done(ControlResult)
    /// Failed.
    case failed(ControlFailure)
    /// Document `doc` was made or opened; the app answers once its window
    /// is up. `command` is the registry ID (nil for `app.*`).
    case opened(command: String?, doc: UUID)
}

extension ControlFailure {
    public static func of(_ code: ControlErrorCode, _ message: String, dialog: String? = nil) -> ControlFailure {
        ControlFailure(code: code, message: message, dialog: dialog)
    }

    /// A thrown error: Busy is `busy`, any other is `failed`.
    public static func thrown(_ error: any Error, _ title: String) -> ControlFailure {
        if case VizError.Busy(let message) = error { return .of(.busy, message) }
        return .alert(AlertItem(error, title: title))
    }

    /// An alert the call raised, taken instead of shown.
    public static func alert(_ a: AlertItem) -> ControlFailure { .of(.failed, "\(a.title): \(a.message)") }

    /// `dialog_required` for `dialog`, answered by `param`.
    public static func dialog(_ dialog: String, _ param: String) -> ControlFailure {
        .of(.dialogRequired, "this would open the \"\(dialog)\" dialog; pass `\(param)` instead", dialog: dialog)
    }
}

extension Array where Element == ControlArg {
    /// The text argument `name`.
    public func text(_ name: String) -> String? {
        for a in self where a.name == name {
            if case .text(let v) = a.value { return v }
        }
        return nil
    }

    /// The flag argument `name`.
    public func flag(_ name: String) -> Bool? {
        for a in self where a.name == name {
            if case .flag(let v) = a.value { return v }
        }
        return nil
    }
}

/// The windows and app-wide settings commands need; the app gives the
/// real one, tests a fake.
@MainActor
public protocol ControlHost: AnyObject {
    func showMixer(_ doc: UUID)
    func showPluginManager()
    func tearOff(_ doc: UUID, kind: DetailEditorKind)
    func closeDocument(_ doc: UUID)
    var nudge: NudgeSettings { get }
}

/// Runs registered menu commands (control spec §6) on a document model
/// the way the menu items do, minus their dialogs (spec §7).
@MainActor
public enum ControlCommands {
    static let detail: [String: DetailEditorKind] = [
        "editors.piano_roll": .pianoRoll, "editors.list": .list,
        "editors.strip_chart": .stripChart, "editors.drum_grid": .drumGrid,
    ]
    static let torn: [String: DetailEditorKind] = [
        "editors.piano_roll_window": .pianoRoll, "editors.list_window": .list,
        "editors.strip_chart_window": .stripChart, "editors.drum_grid_window": .drumGrid,
    ]
    static let nudges: [String: NudgeDirection] = [
        "editors.nudge_earlier": .earlier, "editors.nudge_later": .later,
        "editors.nudge_up": .up, "editors.nudge_down": .down,
    ]
    static let panels: [String: PanelTask] = [
        "modify.select_by_rule": .selectByRule, "modify.transpose": .transpose,
        "modify.quantize": .quantize, "modify.change": .change, "modify.move": .move,
    ]
    static let other: Set<String> = [
        "file.new", "file.open", "file.create_demo", "file.create_demo_audio", "file.close",
        "file.save", "file.save_as", "edit.undo", "edit.redo", "window.mixer",
        "window.plugin_manager", "editors.close_detail", "editors.insert_event", "modify.again",
        "modify.template", "modify.exact_mode", "sequence.make_reference", "sequence.back",
        "transport.play_stop", "transport.loop", "transport.return_to_start",
    ]

    /// Every registry ID `run` handles (a test checks it is all of them).
    public static var handled: Set<String> {
        other.union(detail.keys).union(torn.keys).union(nudges.keys).union(panels.keys).union(DrumCommands.ids)
    }

    /// Runs command `id` with `args` on `doc` (the target window's document,
    /// else the front one's; nil when none is open).
    public static func run(
        _ id: String, args: [ControlArg], doc: (id: UUID, model: DocumentModel)?,
        store: DocumentStore, host: any ControlHost
    ) -> ControlOutcome {
        guard handled.contains(id) else { return .failed(.of(.unknownId, "VizDAW has no handler for \(id).")) }
        switch id {
        case "file.new": return made(id, store.newEmpty(), store)
        case "file.create_demo": return made(id, store.newDemo(), store)
        case "file.create_demo_audio": return made(id, store.newDemoAudio(), store)
        case "file.open":
            guard let path = args.text("path") else { return .failed(.dialog("Open", "path")) }
            return ControlOpen.open(path: path, answers: ControlOpen.answers(args), store: store, command: id)
        case "window.plugin_manager":
            host.showPluginManager()
            return .done(ran(id, nil))
        default: break
        }
        guard let (docID, m) = doc else { return .failed(.of(.noDocument, "No document is open.")) }
        let before = m.alert?.id
        switch id {
        case "file.close":
            host.closeDocument(docID)
            return .done(ran(id, docID))
        case "file.save":
            if m.needsSaveAs {
                guard let path = args.text("path") else { return .failed(.dialog("Save", "path")) }
                m.saveAs(path: path)
            } else {
                m.save()
            }
        case "file.save_as":
            guard let path = args.text("path") else { return .failed(.dialog("Save", "path")) }
            m.saveAs(path: path)
        case "edit.undo", "edit.redo":
            let undo = id == "edit.undo"
            guard undo ? m.canUndo : m.canRedo else {
                return unavailable(undo ? "Nothing to undo." : "Nothing to redo.")
            }
            guard m.editor.gesture == nil else { return unavailable(EditorModel.gestureOpenMessage) }
            // The session directly (not `m.undo()`, which swallows Busy): a busy
            // core is the call's `busy` error.
            do {
                m.apply(try undo ? m.session.undo() : m.session.redo())
            } catch {
                return .failed(.thrown(error, undo ? "Undo Failed" : "Redo Failed"))
            }
            m.editor.refreshSelection()
        case "window.mixer":
            host.showMixer(docID)
        case _ where detail[id] != nil:
            m.editor.showDetail(detail[id] ?? .pianoRoll, shown: m.selectedSeq)
        case _ where torn[id] != nil:
            host.tearOff(docID, kind: torn[id] ?? .pianoRoll)
        case "editors.close_detail":
            guard m.editor.layout.detail != nil else { return unavailable("No detail editor is open.") }
            m.editor.closeDetail()
        case "editors.insert_event":
            guard let list = m.listForCommands(), !m.editor.textEditing else {
                return unavailable("Insert Event needs a list editor that has the keyboard.")
            }
            list.insert()
        case _ where nudges[id] != nil:
            guard m.transportKeysEnabled, !m.editor.selection.events.isEmpty else {
                return unavailable("Nudge needs selected events and no text being edited.")
            }
            if !m.editor.nudge(nudges[id] ?? .later, settings: host.nudge) {
                return .failed(.of(.failed, m.editor.status ?? "The nudge was refused."))
            }
        case _ where panels[id] != nil:
            m.selectModify.open(panels[id] ?? .change)
        case "modify.again":
            guard m.selectModify.canModifyAgain else { return unavailable("There is no modification to repeat.") }
            if !m.selectModify.modifyAgain() {
                return .failed(.of(.failed, m.editor.status ?? "Modify Again was refused."))
            }
        case "modify.template":
            let templates = m.editor.modifyTemplates()
            // The menu item is disabled with no templates (ruling: unavailable).
            guard !templates.isEmpty else { return unavailable("There are no modify templates.") }
            let name = args.text("name") ?? ""
            guard let t = templates.first(where: { $0.name == name }) else {
                return .failed(.of(.invalidParams, "modify.template: no template named \"\(name)\""))
            }
            if !m.selectModify.run(t) {
                return .failed(.of(.failed, m.editor.status ?? "The template was refused."))
            }
        case "modify.exact_mode":
            m.editor.exactMode = args.flag("on") ?? !m.editor.exactMode
        case "sequence.make_reference":
            guard let seq = m.selectedSeq, m.editor.gesture == nil else {
                return unavailable("Select a sequence and finish the drag first.")
            }
            if !m.editor.makeReferenceFromSelection(seq: seq) {
                return .failed(.of(.failed, m.editor.status ?? "Make Reference was refused."))
            }
        case "sequence.back":
            guard !m.editor.navigation.isEmpty else { return unavailable("There is no parent sequence to go back to.") }
            m.editor.back()
        case "transport.play_stop":
            guard m.canPlay else { return unavailable("Select a sequence to play.") }
            guard m.transportKeysEnabled else { return unavailable("A text field has the keyboard.") }
            let playing = m.isPlaying
            do {
                if playing {
                    try m.session.stop()
                } else if let seq = m.selectedSeq {
                    try m.session.play(seq: seq)
                }
            } catch {
                return .failed(.thrown(error, playing ? "Stop Failed" : "Play Failed"))
            }
            m.poll()
        case "transport.loop":
            if (args.flag("on") ?? !m.looping) != m.looping { m.toggleLoop() }
        case "transport.return_to_start":
            guard m.returnKeyEnabled else { return unavailable("A text field or list has the keyboard.") }
            m.returnToStart()
        case _ where DrumCommands.ids.contains(id):
            switch DrumCommands.run(id, on: args.flag("on"), m) {
            case .done: break
            case .unavailable(let why): return unavailable(why)
            case .refused(let why): return .failed(.of(.failed, why))
            }
        default:
            return .failed(.of(.unknownId, "VizDAW has no handler for \(id)."))
        }
        // An alert the command raised is the call's failure, not a sheet.
        if let a = m.alert, a.id != before {
            m.alert = nil
            return .failed(.alert(a))
        }
        return .done(ran(id, docID, m))
    }

    static func unavailable(_ message: String) -> ControlOutcome { .failed(.of(.unavailable, message)) }

    static func ran(_ id: String, _ doc: UUID?, _ m: DocumentModel? = nil) -> ControlResult {
        .ran(
            command: id, doc: doc?.uuidString, window: nil, undoLabel: m?.snapshot.undoLabel,
            status: m?.editor.status)
    }

    /// A store command that makes a document: its window opens next.
    static func made(_ id: String, _ doc: UUID?, _ store: DocumentStore) -> ControlOutcome {
        if let doc { return .opened(command: id, doc: doc) }
        if let a = store.takeAlert() { return .failed(.alert(a)) }
        return .failed(.of(.failed, "\(id) made no document."))
    }
}

/// Opening a project without dialogs (control spec §7): the recovery
/// questions are answered from the call's parameters; a question without
/// its answer fails the call naming the dialog.
@MainActor
public enum ControlOpen {
    /// The answers in `command.run file.open` arguments.
    public static func answers(_ args: [ControlArg]) -> RecoveryAnswersRow {
        let plugins: CrashAnswerRow? =
            switch args.text("plugins") {
            case "normal": .normal
            case "off": .pluginsOff
            case "disable": .disable
            default: nil
            }
        return RecoveryAnswersRow(plugins: plugins, restoreAutosave: args.flag("restore_autosave"))
    }

    public static func open(
        path: String, answers: RecoveryAnswersRow, store: DocumentStore, command: String? = nil
    ) -> ControlOutcome {
        // The autosave question is settled before anything opens: opening to
        // find out would load plugins and consume the crash marker.
        if let r = store.recovery, answers.restoreAutosave == nil, let info = try? r.check(path),
            info.autosave != nil, !((info.crashed || r.shiftHeld()) && answers.plugins == nil)
        {
            return .failed(.dialog("Restore Autosave", "restore_autosave"))
        }
        let asked = ControlAnswers(answers)
        let saved = store.recovery?.questions
        store.recovery?.questions = asked
        defer { if let saved { store.recovery?.questions = saved } }
        let alertBefore = store.alert?.id
        let id = store.open(path: path)
        if let missing = asked.missing {
            if let id { store.close(id) }
            return .failed(.dialog(missing.dialog, missing.param))
        }
        guard let id else {
            if let a = store.alert, a.id != alertBefore {
                _ = store.takeAlert()
                return .failed(.alert(a))
            }
            return .failed(.of(.failed, "Opening \(path) was cancelled."))
        }
        return .opened(command: command, doc: id)
    }
}

/// Recovery questions answered from a control call.
@MainActor
final class ControlAnswers: RecoveryQuestions {
    let answers: RecoveryAnswersRow
    /// The first question asked without an answer.
    private(set) var missing: (dialog: String, param: String)?

    init(_ answers: RecoveryAnswersRow) { self.answers = answers }

    func afterCrash(project: String, loading: [PluginRefRow], crashed: Bool) -> CrashChoice {
        switch answers.plugins {
        case .normal: return .normally
        case .pluginsOff: return .pluginsOff
        case .disable: return .disable
        case nil:
            if missing == nil { missing = ("Open with Plugins Off?", "plugins") }
            return .cancel
        }
    }

    func restoreAutosave(project: String) -> Bool {
        guard let r = answers.restoreAutosave else {
            if missing == nil { missing = ("Restore Autosave", "restore_autosave") }
            return false
        }
        return r
    }
}

/// `state.get` for a document, and the facts a `wait` reads.
@MainActor
public enum ControlStates {
    /// The protocol's name of a detail editor kind.
    public static func kindName(_ k: DetailEditorKind) -> String {
        switch k {
        case .pianoRoll: "piano_roll"
        case .list: "list"
        case .stripChart: "strip_chart"
        case .drumGrid: "drum_grid"
        }
    }

    public static func detailRow(_ d: DetailState) -> DetailRow {
        let kind = kindName(d.kind)
        switch d.target {
        case .block(let seq, let lane, let start, let end):
            return DetailRow(kind: kind, seq: seq, lane: lane, start: start, end: end)
        case .lane(let seq, let lane):
            return DetailRow(kind: kind, seq: seq, lane: lane, start: nil, end: nil)
        case .sequence(let seq):
            return DetailRow(kind: kind, seq: seq, lane: nil, start: nil, end: nil)
        }
    }

    /// `target` of document `id`; the transport is read fresh.
    public static func state(_ target: StateTargetRow, _ id: UUID, _ m: DocumentModel) -> ControlResult? {
        let doc = id.uuidString
        switch target {
        case .transport:
            m.poll()
            return .transport(doc: doc, state: m.transport)
        case .document:
            return .document(
                doc: doc, path: (try? m.session.path()) ?? nil, snapshot: m.snapshot,
                selectedSeq: m.selectedSeq, lanes: m.lanes,
                alert: m.alert.map { AlertRow(title: $0.title, message: $0.message) })
        case .editor:
            let e = m.editor
            return .editor(
                doc: doc,
                editor: EditorStateRow(
                    detail: e.layout.detail.map(detailRow),
                    windows: e.layout.windows.map { TornRow(id: $0.id.uuidString, detail: detailRow($0.detail)) },
                    tool: e.tool.rawValue, snapEnabled: e.layout.snap.enabled,
                    snapDivision: e.layout.snap.division.rawValue, exactMode: e.exactMode,
                    gesture: e.gesture != nil, status: e.status))
        case .selection:
            return .selection(doc: doc, selection: m.editor.selection)
        case .windows:
            return nil
        }
    }

    /// What a wait reads of document `m` (its transport read fresh: a
    /// window in the background does not poll).
    public static func facts(_ m: DocumentModel) -> DocFactsRow {
        m.poll()
        return DocFactsRow(
            playing: m.transport.playing, tick: m.transport.tick,
            detailKind: m.editor.layout.detail.map { kindName($0.kind) } ?? "none")
    }
}

/// Where the control socket goes (control spec §8): `--control PATH`
/// wins over `VIZDAW_CONTROL`; neither, or an empty one, is off.
public enum ControlLaunch {
    public static func socketPath(arguments: [String], environment: [String: String]) -> String? {
        if let i = arguments.firstIndex(of: "--control") {
            let next = arguments.index(after: i)
            return next < arguments.endIndex && !arguments[next].isEmpty ? arguments[next] : nil
        }
        if let e = environment["VIZDAW_CONTROL"], !e.isEmpty { return e }
        return nil
    }

    /// `arguments` without `--control` and its path (so the path is never
    /// taken for a project).
    public static func strip(_ arguments: [String]) -> [String] {
        guard let i = arguments.firstIndex(of: "--control") else { return arguments }
        var a = arguments
        a.remove(at: i)
        if i < a.endIndex { a.remove(at: i) }
        return a
    }
}
