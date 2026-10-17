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
        if let c = open[key], !c.closed {
            c.showWindow(nil)
            return
        }
        guard let model = store.model(doc) else { return }
        weak var made: EditorWindowController?
        let c = EditorWindowController(model: model, doc: doc, window: window) { byUser in
            if byUser { model.editor.closeWindow(window.id) }
            model.dropPianoRoll(.window(window.id))
            // Every close (by the user, pruned, with the document) drops its list.
            model.dropList(.window(window.id))
            // Called from windowWillClose: dropping the last strong reference to the
            // controller inside its own delegate call frees it mid-close, so defer it.
            let closing = made
            DispatchQueue.main.async {
                if let closing, open[key] === closing { open[key] = nil }
            }
        }
        made = c
        open[key] = c
        c.showWindow(nil)
    }

    /// ⌥⌘1–⌥⌘4 and "Open in Window"; `from` a key torn window copies its
    /// target (Plan 4b Ruling 18).
    static func tearOff(_ store: DocumentStore, doc: UUID, kind: DetailEditorKind, from window: UUID? = nil) {
        guard let model = store.model(doc) else { return }
        let made = window.map { model.editor.tearOff(kind, from: $0) } ?? model.editor.tearOff(kind, shown: model.selectedSeq)
        guard let w = made else { return }
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
    private let doc: UUID
    private let id: UUID
    private let onClose: (Bool) -> Void
    private var forDocument = false
    private(set) var closed = false

    init(model: DocumentModel, doc: UUID, window w: TornWindow, onClose: @escaping (Bool) -> Void) {
        self.model = model
        self.doc = doc
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
            rootView: TornEditorView(model: model, id: w.id) { [weak self] in
                // onGone runs inside a SwiftUI update of this window's own hosting view;
                // closing (and freeing) the window there is unsafe, so close on the next hop.
                DispatchQueue.main.async { [weak self] in self?.closeForDocument() }
            })
        win.delegate = self
        if let saved {
            win.setFrame(saved, display: false)
        } else {
            win.center()
        }
    }

    required init?(coder: NSCoder) { nil }

    func closeForDocument() {
        guard !closed else { return }
        forDocument = true
        close()
    }

    func windowWillClose(_ notification: Notification) {
        guard !closed else { return }
        closed = true
        appStore.blurTorn(window: id)
        onClose(!forDocument)
    }
    func windowDidBecomeKey(_ notification: Notification) {
        guard !closed else { return }
        appStore.focusTorn(TornFocus(doc: doc, window: id))
    }
    func windowDidResignKey(_ notification: Notification) { appStore.blurTorn(window: id) }
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
            VStack(spacing: 0) {
                DetailPane(
                    model: model, state: w.detail, roll: model.pianoRoll(.window(id)), slot: .window(id),
                    onKind: { model.editor.setWindowKind(id, $0) }, onTearOff: nil, onClose: nil)
                StatusLine(editor: model.editor)
            }
        } else {
            ContentUnavailableView("Editor Closed", systemImage: "xmark.rectangle")
                .onAppear(perform: onGone)
        }
    }
}
