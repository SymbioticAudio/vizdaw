import AppKit
import Observation
import SwiftUI
import VizDAWAppModel

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
            Button("New") { show(store.newEmpty()) }
                .keyboardShortcut("n")
            Button("Open…") { if let path = Panels.chooseProject() { show(store.open(path: path)) } }
                .keyboardShortcut("o")
            Button("Create Demo Project") { show(store.newDemo()) }
            Button("Create Demo Project with Audio") { show(store.newDemoAudio()) }
        }
        CommandGroup(replacing: .saveItem) {
            Button("Close") { NSApp.keyWindow?.performClose(nil) }
                .keyboardShortcut("w")
            Button("Save") { save(as: false) }
                .keyboardShortcut("s")
                .disabled(doc == nil)
            Button("Save As…") { save(as: true) }
                .keyboardShortcut("s", modifiers: [.command, .shift])
                .disabled(doc == nil)
        }
        CommandGroup(replacing: .undoRedo) {
            Button(doc?.undoTitle ?? "Undo") { doc?.undo() }
                .keyboardShortcut("z")
                .disabled(!(doc?.canUndo ?? false) || doc?.editor.gesture != nil)
            Button(doc?.redoTitle ?? "Redo") { doc?.redo() }
                .keyboardShortcut("z", modifiers: [.command, .shift])
                .disabled(!(doc?.canRedo ?? false) || doc?.editor.gesture != nil)
        }
        CommandGroup(before: .windowList) {
            Button("Mixer") { if let docID { openWindow(id: "mixer", value: docID) } }
                .keyboardShortcut("m", modifiers: [.command, .option])
                .disabled(docID == nil)
            Button("Plugin Manager") { openWindow(id: "plugins") }
                .keyboardShortcut("p", modifiers: [.command, .option])
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
                .keyboardShortcut(KeyEquivalent(Character(kind.key)), modifiers: .command)
                .disabled(doc == nil)
            }
            Divider()
            ForEach(DetailEditorKind.allCases, id: \.self) { kind in
                Button("Open \(kind.title) in Window") {
                    if let docID { EditorWindows.tearOff(store, doc: docID, kind: kind, from: tornWindow) }
                }
                .keyboardShortcut(KeyEquivalent(Character(kind.key)), modifiers: [.command, .option])
                .disabled(docID == nil)
            }
            Divider()
            Button("Close Detail Editor") { doc?.editor.closeDetail() }
                .disabled(doc?.editor.layout.detail == nil)
            Button("Insert Event") { doc?.listForCommands()?.insert() }
                .keyboardShortcut("i")
                .disabled(doc?.listForCommands() == nil || doc?.editor.textEditing == true)
            Divider()
            nudge("Nudge Earlier", .earlier, .leftArrow)
            nudge("Nudge Later", .later, .rightArrow)
            nudge("Nudge Up", .up, .upArrow)
            nudge("Nudge Down", .down, .downArrow)
        }
        CommandMenu("Modify") {
            panelItem(.selectByRule, "s")
            panelItem(.transpose, "t")
            panelItem(.quantize, "u")
            panelItem(.change, "m")
            panelItem(.move, "v")
            Divider()
            Button(doc?.selectModify.modifyAgainTitle ?? "Modify Again") {
                doc?.selectModify.modifyAgain()
            }
            .keyboardShortcut("r", modifiers: [.command, .control])
            .help(SelectModifyModel.modifyAgainHelp)
            .disabled(!(doc?.selectModify.canModifyAgain ?? false))
            Menu("Templates") {
                ForEach(doc?.editor.modifyTemplates() ?? [], id: \.name) { t in
                    Button(t.name) { doc?.selectModify.run(t) }
                }
            }
            .disabled((doc?.editor.modifyTemplates() ?? []).isEmpty)
        }
        CommandMenu("Sequence") {
            Button("Make Reference from Selection") {
                if let doc, let seq = doc.selectedSeq { doc.editor.makeReferenceFromSelection(seq: seq) }
            }
            .keyboardShortcut("r", modifiers: [.command, .option])
            .disabled(doc?.selectedSeq == nil || doc?.editor.gesture != nil)
            Button("Back to Parent Sequence") { doc?.editor.back() }
                .keyboardShortcut("[", modifiers: [.command])
                .disabled(doc?.editor.navigation.isEmpty ?? true)
        }
        CommandMenu("Transport") {
            Button(doc?.isPlaying == true ? "Stop" : "Play") { doc?.playStop() }
                .keyboardShortcut(.space, modifiers: [])
                .disabled(!(doc?.canPlay ?? false) || !(doc?.transportKeysEnabled ?? false))
            Toggle("Loop", isOn: Binding(get: { doc?.looping ?? false }, set: { _ in doc?.toggleLoop() }))
                .disabled(doc == nil)
            Button("Return to Start") { doc?.returnToStart() }
                .keyboardShortcut(.return, modifiers: [])
                .disabled(!(doc?.returnKeyEnabled ?? false))
        }
    }

    /// ⌥-arrows nudge the selection (Plan 4b Ruling 16); off while a text
    /// field has the keyboard, so ⌥-arrows there still move the cursor.
    private func nudge(_ title: String, _ dir: NudgeDirection, _ key: KeyEquivalent) -> some View {
        Button(title) { doc?.editor.nudge(dir, settings: editingPreferences.nudge) }
            .keyboardShortcut(key, modifiers: .option)
            .disabled(
                doc == nil || !(doc?.transportKeysEnabled ?? false) || (doc?.editor.selection.events.isEmpty ?? true))
    }

    /// A Modify menu command: the panel set to its task (Plan 4c Ruling 10).
    private func panelItem(_ task: PanelTask, _ key: Character) -> some View {
        Button("\(task.title)…") { doc?.selectModify.open(task) }
            .keyboardShortcut(KeyEquivalent(key), modifiers: [.command, .control])
            .disabled(doc == nil)
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
            let alert = NSAlert()
            alert.alertStyle = .warning
            alert.messageText = item.title
            alert.informativeText = item.message
            alert.addButton(withTitle: "OK")
            alert.runModal()
        }
    }
}
