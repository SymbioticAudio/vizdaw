import AppKit
import SwiftUI
import VizDAWAppModel

/// Torn-off editor windows (spec §3.7), by document and window ID.
@MainActor
enum EditorWindows {
    private struct Key: Hashable {
        let doc: UUID
        let id: UUID
    }

    private static var open: [Key: EditorWindowController] = [:]

    static func show(_ store: DocumentStore, doc: UUID, window: TornWindow) {
        let key = Key(doc: doc, id: window.id)
        if let c = open[key] {
            c.showWindow(nil)
            return
        }
        guard let model = store.model(doc) else { return }
        let c = EditorWindowController(model: model, window: window) { byUser in
            open[key] = nil
            if byUser { model.editor.closeWindow(window.id) }
        }
        open[key] = c
        c.showWindow(nil)
    }

    /// ⌥⌘1–⌥⌘4 and "Open in Window".
    static func tearOff(_ store: DocumentStore, doc: UUID, kind: DetailEditorKind) {
        guard let model = store.model(doc), let w = model.editor.tearOff(kind, shown: model.selectedSeq)
        else { return }
        show(store, doc: doc, window: w)
    }

    /// The windows saved with the project.
    static func reopen(_ store: DocumentStore, doc: UUID) {
        guard let model = store.model(doc) else { return }
        for w in model.editor.layout.windows { show(store, doc: doc, window: w) }
    }

    /// The document is closing: its windows close but stay in its layout.
    static func closeDocument(_ doc: UUID) {
        for (key, c) in open where key.doc == doc { c.closeForDocument() }
    }
}

@MainActor
final class EditorWindowController: NSWindowController, NSWindowDelegate {
    private let model: DocumentModel
    private let id: UUID
    private let onClose: (Bool) -> Void
    private var forDocument = false

    init(model: DocumentModel, window w: TornWindow, onClose: @escaping (Bool) -> Void) {
        self.model = model
        id = w.id
        self.onClose = onClose
        let saved = w.frame.flatMap { f in
            f.count == 4 ? NSRect(x: f[0], y: f[1], width: f[2], height: f[3]) : nil
        }
        let win = NSWindow(
            contentRect: saved ?? NSRect(x: 0, y: 0, width: 720, height: 420),
            styleMask: [.titled, .closable, .resizable, .miniaturizable], backing: .buffered, defer: false)
        win.isReleasedWhenClosed = false
        win.title = "\(w.detail.kind.title) — \(model.snapshot.title)"
        super.init(window: win)
        win.contentView = NSHostingView(
            rootView: TornEditorView(model: model, id: w.id) { [weak self] in self?.closeForDocument() })
        win.delegate = self
        if let saved {
            win.setFrame(saved, display: false)
        } else {
            win.center()
        }
    }

    required init?(coder: NSCoder) { nil }

    func closeForDocument() {
        forDocument = true
        close()
    }

    func windowWillClose(_ notification: Notification) { onClose(!forDocument) }
    func windowDidMove(_ notification: Notification) { saveFrame() }
    func windowDidEndLiveResize(_ notification: Notification) { saveFrame() }

    private func saveFrame() {
        guard let f = window?.frame else { return }
        model.editor.setWindowFrame(id, [f.minX, f.minY, f.width, f.height])
    }
}

struct TornEditorView: View {
    let model: DocumentModel
    let id: UUID
    /// The layout dropped this window (its sequence or lane was deleted).
    let onGone: () -> Void

    var body: some View {
        if let w = model.editor.layout.windows.first(where: { $0.id == id }) {
            DetailPane(
                model: model, state: w.detail, onKind: { model.editor.setWindowKind(id, $0) },
                onTearOff: nil, onClose: nil)
        } else {
            ContentUnavailableView("Editor Closed", systemImage: "xmark.rectangle")
                .onAppear(perform: onGone)
        }
    }
}
