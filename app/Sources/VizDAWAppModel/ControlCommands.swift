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
    /// A job started (import, conversion): the app answers when it ends.
    case job(command: String, doc: UUID, job: UInt64)
}

/// How a job that ended is answered (fix round 1, I-2).
public enum JobAnswer: Equatable, Sendable {
    /// Still running.
    case wait
    /// Succeeded; `status` carries warnings.
    case ok(status: String?)
    /// Failed, with this message.
    case failed(String)
}

public enum ControlJobs {
    /// How long a job's call waits (the server's app timeout is for
    /// calls that are not jobs).
    public static let waitMs: UInt64 = 600_000

    /// The answer for `j`: an import or conversion that created nothing and had problems
    /// failed; a partial success carries its problems as warnings.
    public static func answer(_ j: JobRow) -> JobAnswer {
        switch j.state {
        case .running: return .wait
        case .done:
            if j.kind != .format, j.created.isEmpty, !j.problems.isEmpty {
                return .failed(j.problems.joined(separator: "; "))
            }
            return .ok(status: j.problems.isEmpty ? nil : "warnings: " + j.problems.joined(separator: "; "))
        case .failed, .cancelled:
            return .failed(([j.message ?? "the job was cancelled"] + j.problems).joined(separator: "; "))
        }
    }

    /// The failure when the wait expires with the job still running.
    public static func expiredMessage(_ j: JobRow) -> String {
        "job \(j.id) is still running after \(waitMs / 60_000) minutes; it was not cancelled"
    }
}

/// The project formats as the channel spells them (5a Ruling 16).
public enum ControlFormats {
    /// `rate`/`depth` strings to a format, `base` filling what they leave
    /// out; nil when either is not a project rate or depth.
    public static func parse(
        rate: String?, depth: String?,
        base: ProjectFormatRow = ProjectFormatRow(sampleRate: 48_000, depth: .int24)
    ) -> ProjectFormatRow? {
        var f = base
        if let r = rate {
            guard let n = UInt32(r), projectRates().contains(n) else { return nil }
            f.sampleRate = n
        }
        if let d = depth {
            switch d {
            case "16": f.depth = .int16
            case "24": f.depth = .int24
            case "32f": f.depth = .float32
            default: return nil
            }
        }
        return f
    }

    /// The format a call's arguments ask for, `base` filling what they
    /// leave out.
    public static func format(args: [ControlArg], default base: ProjectFormatRow) -> ProjectFormatRow? {
        parse(rate: args.text("rate"), depth: args.text("depth"), base: base)
    }
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

    /// The integer argument `name`.
    public func int(_ name: String) -> Int64? {
        for a in self where a.name == name {
            if case .int(let v) = a.value { return v }
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
    /// Opens document `doc`'s media pool window (plan 5b).
    func showMediaPool(_ doc: UUID)
    var nudge: NudgeSettings { get }
    /// The app's key bindings (plan 4f).
    var keys: KeyBindings { get }
    /// Gives a transport field of document `doc`'s window the keyboard
    /// (plan 4f Ruling 16); false when the window shows none.
    func focus(_ field: TransportField, _ doc: UUID) -> Bool
    /// Applies `audio.set`'s change to the app's audio settings (plan
    /// 5d-1 Ruling 26); the core's refusal is thrown.
    func applyAudio(_ change: AudioChange) throws
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
        "keys.save", "keys.load", "keys.reset",
        "file.project_settings", "media.import", "media.convert",
        "media.relink", "media.compact", "media.rename", "window.media_pool",
        "record.set", "record.recover", "lane.set_input", "lane.set_monitor", "midi.inject",
    ]
    /// The keys that focus a transport field.
    public static let fieldKeys: [String: TransportField] = [
        "transport.go_position": .position, "transport.go_edit_in": .editIn, "transport.go_punch": .punchIn,
    ]

    /// Every registry ID `run` handles (a test checks it is all of them).
    public static var handled: Set<String> {
        other.union(detail.keys).union(torn.keys).union(nudges.keys).union(panels.keys).union(DrumCommands.ids)
            .union(fieldKeys.keys).union(RegionCommands.ids).union(SynthCommands.ids)
            .union(TransportCommands.ids).union(StepCommands.ids).union(AudioCommands.ids)
    }

    /// Runs command `id` with `args` on `doc` (the target window's document,
    /// else the front one's; nil when none is open).
    public static func run(
        _ id: String, args: [ControlArg], doc: (id: UUID, model: DocumentModel)?,
        store: DocumentStore, host: any ControlHost
    ) -> ControlOutcome {
        guard handled.contains(id) else { return .failed(.of(.unknownId, "VizDAW has no handler for \(id).")) }
        switch id {
        case "file.new":
            guard let f = ControlFormats.format(args: args, default: store.defaultFormat) else {
                return .failed(.of(.invalidParams, "`rate` or `depth` is not a project format"))
            }
            return made(id, store.newProject(f), store)
        case "file.create_demo": return made(id, store.newDemo(), store)
        case "file.create_demo_audio": return made(id, store.newDemoAudio(), store)
        case "file.open":
            guard let path = args.text("path") else { return .failed(.dialog("Open", "path")) }
            return ControlOpen.open(path: path, answers: ControlOpen.answers(args), store: store, command: id)
        case "window.plugin_manager":
            host.showPluginManager()
            return .done(ran(id, nil))
        case "keys.save", "keys.load":
            guard let path = args.text("path") else {
                return .failed(.dialog(id == "keys.save" ? "Save" : "Open", "path"))
            }
            do {
                if id == "keys.save" { try host.keys.save(to: path) } else { try host.keys.load(from: path) }
            } catch {
                return .failed(.thrown(error, id == "keys.save" ? "Save Key Bindings Failed" : "Load Key Bindings Failed"))
            }
            return .done(ran(id, nil))
        case "keys.reset":
            host.keys.resetAll()
            return .done(ran(id, nil))
        case AudioCommands.set:
            return AudioCommands.runSet(args, host: host)
        default: break
        }
        guard let (docID, m) = doc else { return .failed(.of(.noDocument, "No document is open.")) }
        // A result's `status` is this command's own (plan 4f Ruling 2): a
        // message an earlier action left is cleared first.
        m.editor.status = nil
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
        case "file.project_settings":
            let base = (try? m.session.projectInfo().format) ?? store.defaultFormat
            guard let f = ControlFormats.format(args: args, default: base),
                args.text("rate") != nil || args.text("depth") != nil
            else {
                return .failed(args.text("rate") == nil && args.text("depth") == nil
                    ? .dialog("Project Settings", "rate")
                    : .of(.invalidParams, "`rate` or `depth` is not a project format"))
            }
            do {
                return .job(command: id, doc: docID, job: try m.session.setProjectFormat(format: f))
            } catch {
                return .failed(.thrown(error, "Project Settings Failed"))
            }
        case "media.import":
            guard let path = args.text("path") else { return .failed(.dialog("Open", "path")) }
            let target = args.int("seq").map {
                ImportTargetRow(seq: UInt64($0), lane: args.int("lane").map { UInt64($0) }, tick: args.int("tick") ?? 0)
            }
            do {
                return .job(command: id, doc: docID, job: try m.session.importAudio(paths: [path], target: target))
            } catch {
                return .failed(.thrown(error, "Import Failed"))
            }
        case "media.convert":
            do {
                return .job(command: id, doc: docID, job: try m.session.convertMedia())
            } catch {
                return .failed(.thrown(error, "Convert Failed"))
            }
        case "window.mixer":
            host.showMixer(docID)
        case "window.media_pool":
            host.showMediaPool(docID)
        case "media.relink":
            guard let path = args.text("path") else { return .failed(.dialog("Relink", "path")) }
            guard let media = args.int("media").flatMap({ UInt64(exactly: $0) }), m.media.row(media) != nil else {
                return .failed(.of(.invalidParams, "media.relink: `media` must name a media item (see state.get media)"))
            }
            do {
                return .job(command: id, doc: docID, job: try m.session.relinkMedia(media: media, path: path))
            } catch {
                return .failed(.thrown(error, "Relink Failed"))
            }
        case "media.compact":
            guard m.editor.gesture == nil else { return unavailable(EditorModel.gestureOpenMessage) }
            m.media.compact()
        case "media.rename":
            guard let name = args.text("name") else { return .failed(.dialog("Rename Media", "name")) }
            guard let media = args.int("media").flatMap({ UInt64(exactly: $0) }), m.media.row(media) != nil else {
                return .failed(.of(.invalidParams, "media.rename: `media` must name a media item (see state.get media)"))
            }
            guard !name.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty else {
                return .failed(.of(.invalidParams, "media.rename: the name is empty"))
            }
            guard m.editor.gesture == nil else { return unavailable(EditorModel.gestureOpenMessage) }
            m.media.rename(media, to: name)
        case "record.set":
            m.record.refresh(quiet: true)  // a pass can end by itself with no poll since (Task 15)
            guard !m.record.isRecording else { return unavailable("Stop recording first.") }
            guard m.editor.gesture == nil else { return unavailable(EditorModel.gestureOpenMessage) }
            // Validate everything before applying anything (the loop, which
            // can fail, goes first).
            if let n = args.int("count_in"), !(0...4).contains(n) {
                return .failed(.of(.invalidParams, "record.set: `count_in` is 0 to 4"))
            }
            if let n = args.int("pre_roll"), !(0...8).contains(n) {
                return .failed(.of(.invalidParams, "record.set: `pre_roll` is 0 to 8"))
            }
            if let on = args.flag("loop"), on != m.looping {
                m.toggleLoop()
                if m.looping != on { break }  // the failure alert becomes the call's failure
            }
            if let replace = args.flag("replace") { m.editor.updateLayout { $0.transport.overdub = !replace } }
            if let punch = args.flag("punch") { m.editor.updateLayout { $0.transport.punch = punch } }
            if let n = args.int("count_in") { m.record.countInBars = UInt8(clamping: n) }
            if let n = args.int("pre_roll") { m.record.preRollBars = UInt8(clamping: n) }
            if let on = args.flag("passes_as_takes") { m.record.passesAsTakes = on }
        case "record.recover":
            let found = (try? m.session.recoveredRecordings()) ?? []
            guard !found.isEmpty else { return unavailable("There are no recovered recordings.") }
            guard m.editor.gesture == nil else { return unavailable(EditorModel.gestureOpenMessage) }
            do {
                m.apply(try m.session.recoverRecordings(import: args.text("action") == "import"))
            } catch {
                return .failed(.thrown(error, "Recovery Failed"))
            }
        case "lane.set_input", "lane.set_monitor":
            guard let seq = args.int("seq").flatMap({ UInt64(exactly: $0) }),
                let lane = args.int("lane").flatMap({ UInt64(exactly: $0) })
            else { return .failed(.of(.invalidParams, "\(id): `seq` and `lane` name the lane")) }
            guard m.editor.gesture == nil else { return unavailable(EditorModel.gestureOpenMessage) }
            let edit: LaneEdit
            if id == "lane.set_input" {
                guard let input = ControlLanes.input(args) else {
                    return .failed(.of(.invalidParams, "lane.set_input: `kind` is none, audio or midi"))
                }
                if case .audio(_, _, let returnOf?) = input.value, SynthCommands.isExternal(returnOf, m) == false {
                    return SynthCommands.notExternal(id, returnOf)
                }
                edit = .setInput(seq: seq, lane: lane, input: input.value)
            } else {
                guard let mode = ControlLanes.monitor(args.text("mode")) else {
                    return .failed(.of(.invalidParams, "lane.set_monitor: `mode` is auto, input or off"))
                }
                edit = .setMonitor(seq: seq, lane: lane, monitor: mode)
            }
            do {
                m.apply(try m.session.edit(cmd: .lane(edit: edit), filter: nil))
            } catch {
                return .failed(.thrown(error, id == "lane.set_input" ? "Set Input Failed" : "Set Monitor Failed"))
            }
        case "midi.inject":
            guard let port = args.text("port"), let bytes = ControlLanes.hexBytes(args.text("bytes") ?? "") else {
                return .failed(.of(.invalidParams, "midi.inject: `bytes` are hex bytes such as \"90 3c 64\""))
            }
            do {
                guard try m.session.injectMidi(port: port, bytes: bytes) else {
                    return unavailable("Play or record first: this document does not hold the audio engine.")
                }
            } catch VizError.Model(let message) {
                // The core refused the payload or the port name.
                return .failed(.of(.invalidParams, "midi.inject: \(message)"))
            } catch {
                return .failed(.thrown(error, "MIDI Injection Failed"))
            }
        case _ where SynthCommands.ids.contains(id):
            return SynthCommands.run(id, args: args, doc: docID, m)
        case AudioCommands.addMidi:
            return AudioCommands.runAddMidi(args, doc: docID, m)
        case RegionCommands.normalize:
            guard let text = args.text("peak_db") else { return .failed(.dialog("Normalize", "peak_db")) }
            guard let peak = RegionCommands.parsePeak(text) else {
                return .failed(.of(.invalidParams, "`peak_db` must be a level from -96 to 0 dB, like \"-0.3\""))
            }
            if let f = failure(RegionCommands.normalize(peakDb: peak, m)) { return f }
        case _ where RegionCommands.ids.contains(id):
            if let f = failure(RegionCommands.run(id, m)) { return f }
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
                } else {
                    try m.startPlaying()
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
            if let f = failure(DrumCommands.run(id, on: args.flag("on"), m)) { return f }
        case _ where fieldKeys[id] != nil:
            // A bare key: it waits while a text field has the keyboard
            // (final review M-5), as the other bare-key commands do.
            guard m.transportKeysEnabled else { return unavailable(TransportCommands.textFieldHasKeyboard) }
            guard host.focus(fieldKeys[id] ?? .position, docID) else {
                return unavailable("The window shows no transport fields.")
            }
        case _ where TransportCommands.ids.contains(id):
            if let f = failure(TransportCommands.run(id, on: args.flag("on"), m)) { return f }
        case _ where StepCommands.ids.contains(id):
            if let f = failure(StepCommands.run(id, on: args.flag("on"), pitches: args.text("pitches"), m)) {
                return f
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

    /// A command's outcome as the channel's failure; nil when it ran.
    static func failure(_ o: KeyOutcome) -> ControlOutcome? {
        switch o {
        case .done: nil
        case .unavailable(let why): unavailable(why)
        case .refused(let why): .failed(.of(.failed, why))
        case .busy(let why): .failed(.of(.busy, why))
        }
    }

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
        // An open project is never opened twice, and no window is fronted.
        if store.documentID(forPath: path) != nil {
            let name = (path as NSString).lastPathComponent
            return .failed(.of(.busy, "\(name) is already open in another window."))
        }
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
                    gesture: e.gesture != nil, status: e.status, step: stepRow(m),
                    marks: marksRow(e.layout.transport)))
        case .selection:
            return .selection(doc: doc, selection: m.editor.selection)
        case .media:
            return .media(doc: doc, items: (try? m.session.media()) ?? [])
        case .project:
            guard let info = try? m.session.projectInfo() else { return nil }
            return .project(doc: doc, info: info)
        case .record:
            guard let s = try? m.session.recordState() else { return nil }
            return .record(doc: doc, state: s)
        case .input:
            guard let info = try? m.session.inputInfo() else { return nil }
            return .inputState(doc: doc, info: info, levels: m.inputs.takeChannelLevels())
        case .windows:
            return nil
        }
    }

    /// Step entry for `state.get editor` (plan 4f).
    /// Nil as the window shows it: off, or ended (its sequence changed or
    /// its lane is gone); the position is the transport's.
    static func stepRow(_ m: DocumentModel) -> StepEntryStateRow? {
        let s = m.stepEntry
        guard let st = s.state, let t = s.target, s.endReason(m) == nil else { return nil }
        m.poll()
        return StepEntryStateRow(
            seq: t.seq, lane: t.lane, tick: m.transport.tick, value: StepEntryModel.wireName(st.value), dotted: st.dotted,
            triplet: st.triplet, steps: UInt32(st.history.count))
    }

    static func marksRow(_ t: TransportMarks) -> TransportMarksRow {
        TransportMarksRow(
            locators: t.locators, punchIn: t.punchIn, punchOut: t.punchOut, punch: t.punch, overdub: t.overdub)
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

/// The lane commands' parameters (plan 5c Task 14): `lane.set_input`'s
/// kind and channels (counted from 1 on the channel, from 0 in the core),
/// `lane.set_monitor`'s mode, and `midi.inject`'s hex bytes.
public enum ControlLanes {
    /// A box so "no input" (nil) differs from a refusal.
    public struct Input: Equatable {
        public let value: LaneInputRow?
    }

    /// The input `args` ask for, or nil when `kind` is missing or wrong.
    public static func input(_ args: [ControlArg]) -> Input? {
        // Params that belong to another kind are mistakes, not extras.
        let names = Set(args.map(\.name))
        let audioOnly: Set<String> = ["channel", "stereo", "return_of"]
        let midiOnly: Set<String> = ["port", "midi_channel"]
        switch args.text("kind") {
        case "none" where !names.isDisjoint(with: audioOnly.union(midiOnly)): return nil
        case "audio" where !names.isDisjoint(with: midiOnly): return nil
        case "midi" where !names.isDisjoint(with: audioOnly): return nil
        default: break
        }
        switch args.text("kind") {
        case "none":
            return Input(value: nil)
        case "audio":
            let channel = args.int("channel") ?? 1
            guard (1...256).contains(channel) else { return nil }
            // Without `return_of` the lane is plain audio: a link to a
            // synth's return is dropped (plan 5c-2 Task 8).
            let returnOf = args.int("return_of")
            if let returnOf, returnOf < 0 { return nil }
            return Input(
                value: .audio(
                    first: UInt16(channel - 1), stereo: args.flag("stereo") ?? false,
                    returnOf: returnOf.map { UInt64($0) }))
        case "midi":
            let ch = args.int("midi_channel")
            if let ch, !(1...16).contains(ch) { return nil }
            return Input(value: .midi(port: args.text("port"), channel: ch.map { UInt8($0 - 1) }))
        default:
            return nil
        }
    }

    /// The monitor mode `text` names.
    public static func monitor(_ text: String?) -> MonitorModeRow? {
        switch text {
        case "auto": .auto
        case "input": .input
        case "off": .off
        default: nil
        }
    }

    /// `text` as bytes: hex pairs separated by spaces (`"90 3c 64"`), at
    /// most 1024; nil for anything else.
    public static func hexBytes(_ text: String) -> Data? {
        let parts = text.split(whereSeparator: \.isWhitespace)
        guard !parts.isEmpty, parts.count <= 1024 else { return nil }
        var out = Data()
        for p in parts {
            guard p.count == 2, let b = UInt8(p, radix: 16) else { return nil }
            out.append(b)
        }
        return out
    }
}
