import AppKit
import SwiftUI
import VizDAWAppModel

struct DocumentModelKey: FocusedValueKey {
    typealias Value = DocumentModel
}

extension FocusedValues {
    var documentModel: DocumentModel? {
        get { self[DocumentModelKey.self] }
        set { self[DocumentModelKey.self] = newValue }
    }
}

/// File, Edit and Transport menus.
struct AppCommands: Commands {
    let store: DocumentStore
    @FocusedValue(\.documentModel) private var doc
    @Environment(\.openWindow) private var openWindow

    var body: some Commands {
        CommandGroup(replacing: .newItem) {
            Button("New") { show(store.newEmpty()) }
                .keyboardShortcut("n")
            Button("Open…") { if let path = Panels.chooseProject() { show(store.open(path: path)) } }
                .keyboardShortcut("o")
            Button("Create Demo Project") { show(store.newDemo()) }
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
                .disabled(!(doc?.canUndo ?? false))
            Button(doc?.redoTitle ?? "Redo") { doc?.redo() }
                .keyboardShortcut("z", modifiers: [.command, .shift])
                .disabled(!(doc?.canRedo ?? false))
        }
        CommandMenu("Transport") {
            Button(doc?.isPlaying == true ? "Stop" : "Play") { doc?.playStop() }
                .keyboardShortcut(.space, modifiers: [])
                .disabled(!(doc?.canPlay ?? false) || !(doc?.transportKeysEnabled ?? false))
            Toggle("Loop", isOn: Binding(get: { doc?.looping ?? false }, set: { _ in doc?.toggleLoop() }))
                .disabled(doc == nil)
            Button("Return to Start") { doc?.returnToStart() }
                .keyboardShortcut(.return, modifiers: [])
                .disabled(!(doc?.transportKeysEnabled ?? false))
        }
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
    /// A `.vizdaw` package. Without a registered UTI (under `swift run`)
    /// packages look like folders, so folders are selectable and the
    /// store checks the extension.
    static func chooseProject() -> String? {
        let panel = NSOpenPanel()
        panel.canChooseDirectories = true
        panel.canChooseFiles = false
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
