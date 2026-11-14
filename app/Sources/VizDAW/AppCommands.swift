import AppKit
import Observation
import SwiftUI
import UniformTypeIdentifiers
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
        // Older SDKs (CI's macos-15 Xcode) take at most 10 items per
        // builder, so the standard groups go in one Group.
        Group {
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
                Button(keyBindings.row(InterchangeCommands.importMidi)?.title ?? "Import MIDI File…") {
                    if let doc, let path = Panels.chooseMidi() { doc.interchange.beginImport(path: path) }
                }
                .disabled(doc == nil)
                // A DAWproject file always opens as a new project (plan 6d
                // Ruling 17), so it needs no open document.
                Menu("Import Project") {
                    Button(keyBindings.row(InterchangeCommands.importDawproject)?.title ?? "DAWproject…") {
                        if let path = Panels.chooseDawprojectToOpen() { store.openDawproject(path) }
                    }
                    // With a project open the sheet asks where it goes;
                    // without one it opens as a new project (plan 6f).
                    Button(keyBindings.row(InterchangeCommands.importAaf)?.title ?? "AAF…") {
                        guard let path = Panels.chooseAafToOpen() else { return }
                        if let doc { doc.aaf.choose(path: path) } else { store.openAaf(path) }
                    }
                }
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
                Button("Project Settings…") { doc?.projectSettings.show() }
                    .disabled(doc == nil)
                Button("Bounce…") { doc?.bounce.shown = true }
                    .disabled(doc == nil)
                // Each opens its options sheet; the sheet's Export asks where
                // (plan 6a Ruling 18).
                Menu("Export") {
                    // The most complete export comes first (plan 6d).
                    Button(keyBindings.row(InterchangeCommands.exportDawproject)?.title ?? "DAWproject…") {
                        doc?.dawproject.shown = true
                    }
                    .help("Most complete: tracks, mix, plug-ins, automation")
                    Button(keyBindings.row(InterchangeCommands.exportAaf)?.title ?? "AAF…") {
                        doc?.aaf.exportShown = true
                    }
                    .help("Audio, fades and volume; MIDI as MIDI files beside it")
                    Button(keyBindings.row(InterchangeCommands.exportMidi)?.title ?? "MIDI File…") {
                        doc?.interchange.showExport(.smf)
                    }
                    Button(keyBindings.row(InterchangeCommands.exportClip)?.title ?? "MIDI Clip File…") {
                        doc?.interchange.showExport(.clip)
                    }
                    Button(keyBindings.row(InterchangeCommands.exportForDaw)?.title ?? "For Another DAW…") {
                        doc?.forDaw.shown = true
                    }
                }
                .disabled(doc == nil)
                Button(keyBindings.row(InterchangeCommands.report)?.title ?? "Last Import or Export Report…") {
                    doc?.interchange.showLastReport()
                }
                .disabled(doc?.interchange.report == nil)
                Button("Relink Media…") { doc?.media.relinkShown = true }
                    .keyboardShortcut(KeyMenu.shortcut("media.relink"))
                    // A held merge holds Relink back (plan 6e Task 11).
                    .disabled((doc?.media.relinkable.isEmpty ?? true) || (doc?.mergeHeld ?? false))
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
                Button("Media Pool") { if let docID { openWindow(id: "media-pool", value: docID) } }
                    .keyboardShortcut(KeyMenu.shortcut("window.media_pool"))
                    .disabled(docID == nil)
                Divider()
            }
            CommandGroup(before: .toolbar) {
                ForEach(["ui.view_arrange", "ui.view_mix"], id: \.self) { commandToggle($0, ViewCommands.self) }
                Divider()
                ForEach(["ui.toggle_left", "ui.toggle_bottom", "ui.toggle_right"], id: \.self) {
                    commandToggle($0, ViewCommands.self)
                }
                Divider()
            }
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
            Button("Audio Editor") { if let doc { ViewCommands.run(ViewCommands.audioEditor, on: nil, doc) } }
                .keyboardShortcut(KeyMenu.shortcut("editors.audio_editor"))
                .disabled(doc == nil)
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
        CommandMenu("Track") {
            trackItem(TrackCommands.newAudio)
            trackItem(TrackCommands.newInstrument)
            trackItem(TrackCommands.newMidi)
            Divider()
            trackItem(TrackCommands.duplicate)
            trackItem(TrackCommands.delete)
            trackItem(TrackCommands.rename)
        }
        CommandMenu("Region") {
            Button(keyBindings.row(RegionCommands.normalize)?.title ?? "Normalize…") { if let doc { normalize(doc) } }
                .keyboardShortcut(KeyMenu.shortcut(RegionCommands.normalize))
                .disabled(doc.map { !RegionCommands.enabled(RegionCommands.normalize, $0) } ?? true)
            commandItem(RegionCommands.fadeIn, RegionCommands.self)
            commandItem(RegionCommands.fadeOut, RegionCommands.self)
            commandItem(RegionCommands.removeFades, RegionCommands.self)
            Button(keyBindings.row(RegionCommands.crossfade)?.title ?? "Crossfade") {
                if let doc {
                    report(
                        RegionCommands.crossfade(doc, defaultMs: AppControlHost.shared.crossfadeMs), doc)
                }
            }
            .keyboardShortcut(KeyMenu.shortcut(RegionCommands.crossfade))
            .disabled(doc.map { !RegionCommands.enabled(RegionCommands.crossfade, $0) } ?? true)
            Button(keyBindings.row(RegionCommands.removeCrossfade)?.title ?? "Remove Crossfade") {
                if let doc { report(RegionCommands.removeCrossfade(doc), doc) }
            }
            .keyboardShortcut(KeyMenu.shortcut(RegionCommands.removeCrossfade))
            .disabled(doc.map { !RegionCommands.enabled(RegionCommands.removeCrossfade, $0) } ?? true)
            Divider()
            Button(keyBindings.row(RegionCommands.openExternal)?.title ?? "Open in External Editor") {
                if let doc { openExternal(doc) }
            }
            .keyboardShortcut(KeyMenu.shortcut(RegionCommands.openExternal))
            .disabled(doc.map { !RegionCommands.enabled(RegionCommands.openExternal, $0) } ?? true)
            Button(keyBindings.row(RegionCommands.reloadExternal)?.title ?? "Reload External Edit") {
                if let doc {
                    RegionCommands.runMenu(
                        RegionCommands.reloadExternal, doc, editor: AppControlHost.shared.externalEditor)
                }
            }
            .keyboardShortcut(KeyMenu.shortcut(RegionCommands.reloadExternal))
            .disabled(doc.map { !RegionCommands.enabled(RegionCommands.reloadExternal, $0) } ?? true)
        }
        CommandMenu("Drum") {
            drumItem("Add Drum Row", DrumCommands.addRow)
            drumToggle("Show Velocity", DrumCommands.showVelocity)
            drumItem("Split Row to Own Track", DrumCommands.splitRow)
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
            commandItem("transport.rewind", TransportCommands.self)
            commandItem("transport.fast_forward", TransportCommands.self)
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
        CommandMenu("Automation") {
            Text("Selected Strips")
            ForEach(["off", "read", "touch", "latch", "write", "trim"], id: \.self) {
                commandItem("automation.mode.\($0)", AutomationCommands.self)
            }
            Divider()
            commandToggle(AutomationCommands.suspend, AutomationCommands.self)
        }
    }

    /// A menu item for a command of `kind` (plan 4f): its title and key
    /// from the bindings; a refusal shows in the status line.
    private func commandItem(_ id: String, _ kind: any MenuCommands.Type) -> some View {
        Button(keyBindings.row(id)?.title ?? id) { if let doc { report(kind.run(id, on: nil, doc), doc) } }
            .keyboardShortcut(KeyMenu.shortcut(id))
            .disabled(doc.map { !kind.enabled(id, $0) } ?? true)
    }

    /// A Track menu item (plan tracks Ruling 5): its title and key from the
    /// bindings; New needs a sequence, the others a track too; what it
    /// cannot do shows in the status line.
    private func trackItem(_ id: String) -> some View {
        Button(keyBindings.row(id)?.title ?? id) { if let doc { TrackCommands.performFromMenu(id, doc) } }
            .keyboardShortcut(KeyMenu.shortcut(id))
            .disabled(doc.map { !TrackCommands.enabled(id, $0) } ?? true)
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

    /// Region › Normalize…: asks for the peak level, then normalizes the
    /// selected regions (Ruling 18's default level).
    /// Region › Open in External Editor (plan 5f-3 Ruling 5): with no
    /// editor chosen, the panel asks first — the menu item alone (owner
    /// answer 2026-11-03); then the copy opens.
    private func openExternal(_ doc: DocumentModel) {
        if case .none = AppControlHost.shared.externalEditor {
            guard let app = ExternalEditorPanel.choose() else { return }
            editingPreferences.externalEditor = app
        }
        RegionCommands.runMenu(RegionCommands.openExternal, doc, editor: AppControlHost.shared.externalEditor)
    }

    private func normalize(_ doc: DocumentModel) {
        guard
            let text = TextPanel.run(
                title: "Normalize",
                message: "Set the gain of each selected region so its loudest sample reaches this peak level (dB).",
                initial: String(RegionCommands.defaultPeakDb), ok: "Normalize", label: "Peak level in dB")
        else { return }
        guard let peak = RegionCommands.parsePeak(text) else {
            doc.alert = AlertItem(title: "Normalize", message: "Enter a level from −96 to 0 dB, like -0.3.")
            return
        }
        report(RegionCommands.normalize(peakDb: peak, doc), doc)
    }

    private func show(_ id: UUID?) {
        if let id { openWindow(value: id) }
    }

    private func save(as forceAs: Bool) {
        guard let doc else { return }
        switch doc.saveAction(forceAs: forceAs) {
        case .panel:
            if let path = Panels.chooseSaveLocation(title: doc.snapshot.title) {
                doc.saveAs(path: path)
            }
        case .save:
            doc.save()
        case .refused(let why):
            doc.editor.status = why
        }
    }
}

/// Open and save panels.
@MainActor
enum Panels {
    /// The MIDI files File › Open and Import MIDI File… take (plan 6a).
    static var midiTypes: [UTType] {
        [UTType.midi] + ["mid", "midi", "smf", "midi2"].compactMap { UTType(filenameExtension: $0) }
    }

    /// A `.vizdaw` package, a MIDI file, a DAWproject file (plan 6d) or an
    /// AAF file (plan 6f). In
    /// the bundle the UTI (a package) makes a project a file; under `swift
    /// run` it looks like a folder. Both are selectable and the store checks
    /// the extension.
    static func chooseProject() -> String? {
        let panel = NSOpenPanel()
        panel.canChooseDirectories = true
        panel.canChooseFiles = true
        panel.treatsFilePackagesAsDirectories = false
        panel.allowsMultipleSelection = false
        panel.allowedContentTypes =
            [UTType("org.vizdaw.project"), UTType.folder].compactMap { $0 } + midiTypes + dawprojectTypes + aafTypes
        panel.prompt = "Open"
        panel.message = "Choose a VizDAW project, a MIDI file, a DAWproject file or an AAF file"
        return panel.runModal() == .OK ? panel.url?.path : nil
    }

    /// File › Import MIDI File…: a Standard MIDI File or a MIDI Clip File.
    static func chooseMidi() -> String? {
        let panel = NSOpenPanel()
        panel.canChooseDirectories = false
        panel.canChooseFiles = true
        panel.allowsMultipleSelection = false
        panel.allowedContentTypes = midiTypes
        panel.prompt = "Import"
        panel.message = "Choose a MIDI file: its tracks are added at the playhead."
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

/// The return prompt (plan 6e Ruling 14): a DAWproject file from VizDAW
/// asks whether it opens as a new project or merges into the project it
/// came from. Store-level, as `StoreAlerts`: it watches
/// `DocumentStore.returnPrompt` and answers `answerReturn`. The prompt is
/// only for files the user opened (the channel's `file.open` refuses a
/// `.dawproject`; `file.import_dawproject` answers `dialog_required`
/// "Return"); one that arrives while the channel is active shows nothing,
/// is noted and is answered Cancel.
@MainActor
enum ReturnPrompts {
    /// Shows any pending prompt, then re-arms on the next change.
    static func watch(_ store: DocumentStore) {
        withObservationTracking {
            _ = store.returnPrompt
        } onChange: {
            DispatchQueue.main.async { watch(store) }
        }
        DispatchQueue.main.async { present(store) }
    }

    /// Set while a prompt is up: a re-armed `present` that runs inside the
    /// modal loop leaves the queue to the loop already showing it.
    private static var presenting = false

    private static func present(_ store: DocumentStore) {
        guard !presenting else { return }
        presenting = true
        defer { presenting = false }
        while let prompt = store.returnPrompt {
            if ControlChannel.active {
                ControlChannel.note(
                    AlertItem(
                        title: "DAWproject Not Merged",
                        message: "A DAWproject from VizDAW was opened; it was not merged."))
                store.answerReturn(.cancel)
                continue
            }
            let alert = NSAlert()
            alert.messageText = "This DAWproject came from a VizDAW project."
            alert.informativeText =
                prompt.matchName.map { "It matches \u{201c}\($0)\u{201d}, which is open." }
                ?? "Merge it into the project it came from, or open it as a new project."
            alert.addButton(withTitle: "New Project")
            alert.addButton(withTitle: "Merge With Existing Project…")
            alert.addButton(withTitle: "Cancel")
            let choice: ReturnChoice
            switch alert.runModal() {
            case .alertFirstButtonReturn: choice = .newProject
            case .alertSecondButtonReturn: choice = .merge
            default: choice = .cancel
            }
            store.answerReturn(choice)
        }
    }
}
