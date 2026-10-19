import AppKit
import Observation
import SwiftUI
import VizDAWAppModel
import VizDAWCore

struct DocumentModelKey: FocusedValueKey {
    typealias Value = DocumentModel
}

struct DocumentIDKey: FocusedValueKey {
    typealias Value = UUID
}

extension FocusedValues {
    var documentModel: DocumentModel? {
        get { self[DocumentModelKey.self] }
        set { self[DocumentModelKey.self] = newValue }
    }

    /// The store's key of the focused window's document.
    var documentID: UUID? {
        get { self[DocumentIDKey.self] }
        set { self[DocumentIDKey.self] = newValue }
    }
}

/// File, Edit and Transport menus.
struct AppCommands: Commands {
    let store: DocumentStore
    @FocusedValue(\.documentModel) private var focusedDoc
    @FocusedValue(\.documentID) private var focusedID
    @Environment(\.openWindow) private var openWindow

    /// A key torn-off window's document first (Plan 4b Ruling 18), then a key
    /// select-and-modify panel's (Plan 4c Ruling 22).
    private var doc: DocumentModel? {
        store.tornFocus.flatMap { store.model($0.doc) } ?? store.panelFocus.flatMap { store.model($0) }
            ?? focusedDoc
    }
    private var docID: UUID? { store.commandTarget(focused: focusedID) }
    /// The key torn-off window, if any.
    private var tornWindow: UUID? { store.tornFocus?.window }

    var body: some Commands {
        CommandGroup(replacing: .newItem) {
            Button("New") {
                if let f = FormatPanel.run(
                    title: "New Project", message: "Every audio file of the project is stored in this format.",
                    initial: store.defaultFormat, ok: "Create")
                {
                    store.defaultFormat = f
                    show(store.newProject(f))
                }
            }
            .keyboardShortcut(KeyMenu.shortcut("file.new"))
            Button("Open…") { if let path = Panels.chooseProject() { show(store.open(path: path)) } }
                .keyboardShortcut(KeyMenu.shortcut("file.open"))
            Button("Import Audio…") {
                let paths = Panels.chooseAudio()
                doc?.importAudio(paths: paths)
            }
            .keyboardShortcut(KeyMenu.shortcut("media.import"))
            .disabled(doc == nil)
            Button("Create Demo Project") { show(store.newDemo()) }
            Button("Create Demo Project with Audio") { show(store.newDemoAudio()) }
        }
        CommandGroup(replacing: .saveItem) {
            Button("Close") { NSApp.keyWindow?.performClose(nil) }
                .keyboardShortcut(KeyMenu.shortcut("file.close"))
            Button("Save") { save(as: false) }
                .keyboardShortcut(KeyMenu.shortcut("file.save"))
                .disabled(doc == nil)
            Button("Save As…") { save(as: true) }
                .keyboardShortcut(KeyMenu.shortcut("file.save_as"))
                .disabled(doc == nil)
            Divider()
            Button("Project Settings…") { projectSettings() }
                .disabled(doc == nil)
        }
        CommandGroup(replacing: .undoRedo) {
            Button(doc?.undoTitle ?? "Undo") { doc?.undo() }
                .keyboardShortcut(KeyMenu.shortcut("edit.undo"))
                .disabled(!(doc?.canUndo ?? false) || doc?.editor.gesture != nil)
            Button(doc?.redoTitle ?? "Redo") { doc?.redo() }
                .keyboardShortcut(KeyMenu.shortcut("edit.redo"))
                .disabled(!(doc?.canRedo ?? false) || doc?.editor.gesture != nil)
        }
        CommandGroup(before: .windowList) {
            Button("Mixer") { if let docID { openWindow(id: "mixer", value: docID) } }
                .keyboardShortcut(KeyMenu.shortcut("window.mixer"))
                .disabled(docID == nil)
            Button("Plugin Manager") { openWindow(id: "plugins") }
                .keyboardShortcut(KeyMenu.shortcut("window.plugin_manager"))
            Divider()
        }
        CommandMenu("Editors") {
            ForEach(DetailEditorKind.allCases, id: \.self) { kind in
                Button(kind.title) {
                    if let w = tornWindow {
                        doc?.editor.setWindowKind(w, kind)
                    } else {
                        doc?.editor.showDetail(kind, shown: doc?.selectedSeq)
                    }
                }
                .keyboardShortcut(KeyMenu.shortcut("editors.\(ControlStates.kindName(kind))"))
                .disabled(doc == nil)
            }
            Divider()
            ForEach(DetailEditorKind.allCases, id: \.self) { kind in
                Button("Open \(kind.title) in Window") {
                    if let docID { EditorWindows.tearOff(store, doc: docID, kind: kind, from: tornWindow) }
                }
                .keyboardShortcut(KeyMenu.shortcut("editors.\(ControlStates.kindName(kind))_window"))
                .disabled(docID == nil)
            }
            Divider()
            Button("Close Detail Editor") { doc?.editor.closeDetail() }
                .disabled(doc?.editor.layout.detail == nil)
            Button("Insert Event") { doc?.listForCommands()?.insert() }
                .keyboardShortcut(KeyMenu.shortcut("editors.insert_event"))
                .disabled(doc?.listForCommands() == nil || doc?.editor.textEditing == true)
            Divider()
            nudge("Nudge Earlier", .earlier, "editors.nudge_earlier")
            nudge("Nudge Later", .later, "editors.nudge_later")
            nudge("Nudge Up", .up, "editors.nudge_up")
            nudge("Nudge Down", .down, "editors.nudge_down")
            Divider()
            commandToggle("editors.snap", TransportCommands.self)
        }
        CommandMenu("Modify") {
            panelItem(.selectByRule, "modify.select_by_rule")
            panelItem(.transpose, "modify.transpose")
            panelItem(.quantize, "modify.quantize")
            panelItem(.change, "modify.change")
            panelItem(.move, "modify.move")
            Divider()
            Button(doc?.selectModify.modifyAgainTitle ?? "Modify Again") {
                doc?.selectModify.modifyAgain()
            }
            .keyboardShortcut(KeyMenu.shortcut("modify.again"))
            .help(SelectModifyModel.modifyAgainHelp)
            .disabled(!(doc?.selectModify.canModifyAgain ?? false))
            Menu("Templates") {
                ForEach(doc?.editor.modifyTemplates() ?? [], id: \.name) { t in
                    Button(t.name) { doc?.selectModify.run(t) }
                }
            }
            .disabled((doc?.editor.modifyTemplates() ?? []).isEmpty)
            Divider()
            Toggle(
                "Exact Mode",
                isOn: Binding(get: { doc?.editor.exactMode ?? false }, set: { doc?.editor.exactMode = $0 })
            )
            .keyboardShortcut(KeyMenu.shortcut("modify.exact_mode"))
            .disabled(doc == nil)
        }
        CommandMenu("Sequence") {
            Button("Make Reference from Selection") {
                if let doc, let seq = doc.selectedSeq { doc.editor.makeReferenceFromSelection(seq: seq) }
            }
            .keyboardShortcut(KeyMenu.shortcut("sequence.make_reference"))
            .disabled(doc?.selectedSeq == nil || doc?.editor.gesture != nil)
            Button("Back to Parent Sequence") { doc?.editor.back() }
                .keyboardShortcut(KeyMenu.shortcut("sequence.back"))
                .disabled(doc?.editor.navigation.isEmpty ?? true)
        }
        CommandMenu("Drum") {
            drumToggle("Drum Lane", DrumCommands.markLane)
            Divider()
            drumItem("Add Drum Row", DrumCommands.addRow)
            drumToggle("Show Velocity", DrumCommands.showVelocity)
            drumItem("Split Row to Own Lane", DrumCommands.splitRow)
            drumItem("Remove Drum Row", DrumCommands.removeRow)
            Divider()
            drumToggle("Timeline View", DrumCommands.timeline)
        }
        CommandMenu("Transport") {
            Button(doc?.isPlaying == true ? "Stop" : "Play") { doc?.playStop() }
                .keyboardShortcut(KeyMenu.shortcut("transport.play_stop"))
                .disabled(!(doc?.canPlay ?? false) || !(doc?.transportKeysEnabled ?? false))
            commandItem("transport.stop", TransportCommands.self)
            commandItem("transport.play_selection", TransportCommands.self)
            commandItem("transport.play_from_selection", TransportCommands.self)
            commandItem("transport.play_from_edit_in", TransportCommands.self)
            commandItem("transport.record", TransportCommands.self)
            commandItem("transport.scrub", TransportCommands.self)
            Button("Return to Start") { doc?.returnToStart() }
                .keyboardShortcut(KeyMenu.shortcut("transport.return_to_start"))
                .disabled(!(doc?.returnKeyEnabled ?? false))
            commandItem("transport.previous_marker", TransportCommands.self)
            commandItem("transport.next_marker", TransportCommands.self)
            Divider()
            Toggle("Loop", isOn: Binding(get: { doc?.looping ?? false }, set: { _ in doc?.toggleLoop() }))
                .keyboardShortcut(KeyMenu.shortcut("transport.loop"))
                .disabled(doc == nil)
            commandToggle("transport.overdub", TransportCommands.self)
            commandToggle("transport.punch", TransportCommands.self)
            Divider()
            fieldItem("transport.go_position")
            fieldItem("transport.go_edit_in")
            fieldItem("transport.go_punch")
            Divider()
            commandItem("transport.arm_previous", TransportCommands.self)
            commandItem("transport.arm_next", TransportCommands.self)
        }
        CommandMenu("Step") {
            commandToggle(StepCommands.entry, StepCommands.self)
            Divider()
            ForEach(["step.whole", "step.half", "step.quarter", "step.eighth", "step.sixteenth", "step.thirty_second"],
                    id: \.self) { commandToggle($0, StepCommands.self) }
            Divider()
            commandToggle("step.dot", StepCommands.self)
            commandToggle("step.triplet", StepCommands.self)
            commandItem("step.tie", StepCommands.self)
            commandItem("step.rest", StepCommands.self)
            commandItem("step.delete_last", StepCommands.self)
        }
    }

    /// A menu item for a command of `kind` (plan 4f): its title and key
    /// from the bindings; a refusal shows in the status line.
    private func commandItem(_ id: String, _ kind: any MenuCommands.Type) -> some View {
        Button(keyBindings.row(id)?.title ?? id) { if let doc { report(kind.run(id, on: nil, doc), doc) } }
            .keyboardShortcut(KeyMenu.shortcut(id))
            .disabled(doc.map { !kind.enabled(id, $0) } ?? true)
    }

    /// A toggle menu item (its check mark from the command's state).
    private func commandToggle(_ id: String, _ kind: any MenuCommands.Type) -> some View {
        Toggle(
            keyBindings.row(id)?.title ?? id,
            isOn: Binding(
                get: { doc.map { kind.isOn(id, $0) } ?? false },
                set: { on in if let doc { report(kind.run(id, on: on, doc), doc) } })
        )
        .keyboardShortcut(KeyMenu.shortcut(id))
        .disabled(doc.map { !kind.enabled(id, $0) } ?? true)
    }

    /// A key that focuses a transport field (plan 4f Ruling 16).
    private func fieldItem(_ id: String) -> some View {
        Button(keyBindings.row(id)?.title ?? id) {
            if let docID, let f = ControlCommands.fieldKeys[id] { _ = AppControlHost.shared.focus(f, docID) }
        }
        .keyboardShortcut(KeyMenu.shortcut(id))
        .disabled(docID == nil || !(doc?.transportKeysEnabled ?? false))
    }

    private func report(_ o: KeyOutcome, _ doc: DocumentModel) {
        switch o {
        case .refused(let why), .busy(let why): doc.editor.status = why
        case .done, .unavailable: break
        }
    }

    /// ⌥-arrows nudge the selection (Plan 4b Ruling 16); off while a text
    /// field has the keyboard, so ⌥-arrows there still move the cursor.
    private func nudge(_ title: String, _ dir: NudgeDirection, _ id: String) -> some View {
        Button(title) { doc?.editor.nudge(dir, settings: editingPreferences.nudge) }
            .keyboardShortcut(KeyMenu.shortcut(id))
            .disabled(
                doc == nil || !(doc?.transportKeysEnabled ?? false) || (doc?.editor.selection.events.isEmpty ?? true))
    }

    /// A Drum menu command (Plan 4e Ruling 13); a refusal shows in the
    /// status line like any editor command.
    private func drumItem(_ title: String, _ id: String) -> some View {
        Button(title) { if let doc { DrumCommands.run(id, doc) } }
            .keyboardShortcut(KeyMenu.shortcut(id))
            .disabled(doc.map { !DrumCommands.enabled(id, $0) } ?? true)
    }

    private func drumToggle(_ title: String, _ id: String) -> some View {
        Toggle(
            title,
            isOn: Binding(
                get: { doc.map { DrumCommands.isOn(id, $0) } ?? false },
                set: { on in if let doc { DrumCommands.run(id, on: on, doc) } })
        )
        .keyboardShortcut(KeyMenu.shortcut(id))
        .disabled(doc.map { !DrumCommands.enabled(id, $0) } ?? true)
    }

    /// A Modify menu command: the panel set to its task (Plan 4c Ruling 10).
    private func panelItem(_ task: PanelTask, _ id: String) -> some View {
        Button("\(task.title)…") { doc?.selectModify.open(task) }
            .keyboardShortcut(KeyMenu.shortcut(id))
            .disabled(doc == nil)
    }

    private func projectSettings() {
        guard let m = doc, let info = m.projectInfo ?? (try? m.session.projectInfo()) else { return }
        let media = (try? m.session.media()) ?? []
        let files = media.count
        let seconds = media.reduce(0.0) { $0 + Double($1.frames) / Double(max($1.sampleRate, 1)) }
        let minutes = seconds >= 60 ? String(format: "%.0f min", seconds / 60) : String(format: "%.0f s", seconds)
        let note =
            files == 0
            ? "The project has no audio files yet."
            : "Changing the format converts \(files) audio file\(files == 1 ? "" : "s") (\(minutes) of audio) into new files (one undo step)."
        guard
            let f = FormatPanel.run(
                title: "Project Settings", message: "Format: \(formatLabel(format: info.format)). \(note)",
                initial: info.format, ok: "Change")
        else { return }
        if f != info.format { m.changeFormat(f) }
    }

    private func show(_ id: UUID?) {
        if let id { openWindow(value: id) }
    }

    private func save(as forceAs: Bool) {
        guard let doc else { return }
        if forceAs || doc.needsSaveAs {
            if let path = Panels.chooseSaveLocation(title: doc.snapshot.title) {
                doc.saveAs(path: path)
            }
        } else {
            doc.save()
        }
    }
}

/// Open and save panels.
@MainActor
enum Panels {
    /// A `.vizdaw` package. In the bundle the UTI (a package) makes it a
    /// file; under `swift run` it looks like a folder. Both are
    /// selectable and the store checks the extension.
    static func chooseProject() -> String? {
        let panel = NSOpenPanel()
        panel.canChooseDirectories = true
        panel.canChooseFiles = true
        panel.treatsFilePackagesAsDirectories = false
        panel.allowsMultipleSelection = false
        panel.prompt = "Open"
        panel.message = "Choose a .vizdaw project"
        return panel.runModal() == .OK ? panel.url?.path : nil
    }

    /// Where to save; Rust appends `.vizdaw` when missing.
    static func chooseSaveLocation(title: String) -> String? {
        let panel = NSSavePanel()
        panel.nameFieldStringValue = "\(title).vizdaw"
        panel.canCreateDirectories = true
        return panel.runModal() == .OK ? panel.url?.path : nil
    }
}

/// Store-level failures (New, Open, Finder opens, launch arguments) have
/// no window of their own, so the app shows them as modal `NSAlert`s.
/// Per-document failures stay on their window.
@MainActor
enum StoreAlerts {
    /// Shows any pending store alert, then re-arms on the next change.
    static func watch(_ store: DocumentStore) {
        withObservationTracking {
            _ = store.alert
        } onChange: {
            DispatchQueue.main.async { watch(store) }
        }
        DispatchQueue.main.async { present(store) }
    }

    private static func present(_ store: DocumentStore) {
        while let item = store.takeAlert() {
            // Under the control channel no dialog opens (control spec §7).
            if ControlChannel.active {
                ControlChannel.note(item)
                continue
            }
            let alert = NSAlert()
            alert.alertStyle = .warning
            alert.messageText = item.title
            alert.informativeText = item.message
            alert.addButton(withTitle: "OK")
            alert.runModal()
        }
    }
}
