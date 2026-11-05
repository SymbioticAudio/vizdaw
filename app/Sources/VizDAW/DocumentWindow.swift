import AppKit
import SwiftUI
import VizDAWAppModel

/// One window: shows its document (adopting a queued one or making an
/// empty one when it has none, or only a stale id), polls the transport
/// while key or playing and opens windows other code requested.
struct DocumentWindow: View {
    @Binding var id: UUID?
    let store: DocumentStore
    @Environment(\.openWindow) private var openWindow
    @Environment(\.controlActiveState) private var activeState
    /// This window's focus token (the timer's note target).
    /// (`State` spelled out: the Command Line Tools cannot expand `@State`.)
    private var focusToken = State(initialValue: UUID())

    /// Spelled out: a private stored property makes the memberwise
    /// initializer private under Xcode's compiler.
    init(id: Binding<UUID?>, store: DocumentStore) {
        _id = id
        self.store = store
    }

    var body: some View {
        Group {
            if let id, let model = store.model(id) {
                DocumentView(model: model, docID: id)
                    .background(WindowTag(kind: .document, doc: id))
                    .background(WindowFrameKeeper(model: model))
                    .focusedSceneValue(\.documentModel, model)
                    .focusedSceneValue(\.documentID, id)
                    .task(id: PollKey(key: activeState == .key, playing: model.isPlaying || model.record.isRecording)) {
                        // Background playback keeps polling so device
                        // events are handled (O27), and so does a
                        // recording until the poll sees it finished.
                        guard activeState == .key || model.isPlaying || model.record.isRecording else { return }
                        while !Task.isCancelled {
                            model.poll()
                            try? await Task.sleep(for: model.pollInterval)
                        }
                    }
                    .onAppear {
                        model.selectModify.onPresent = { ModifyPanels.show(store, doc: id) }
                        SmokeRun.start(model) { openWindow(id: "mixer", value: id) }
                        PluginWindows.reopen(store, doc: id)
                        EditorWindows.reopen(store, doc: id)
                    }
                    .onDisappear { store.close(id) }
            } else {
                ProgressView()
                    .onAppear { adopt() }
            }
        }
        .onAppear {
            // The control channel opens document windows with it.
            ControlWindows.openWindow = openWindow
            adopt()
            openQueued()
        }
        .onChange(of: store.pending) { openQueued() }
        .onChange(of: activeState) { openQueued() }
        .onChange(of: KeyDoc(id: id, key: activeState == .key), initial: true) { old, new in
            KeyDoc.changed(store, window: focusToken.wrappedValue, from: old, to: new)
        }
    }

    /// Gives the window a live document when it has none (new window,
    /// stale restored id, or its document was closed).
    private func adopt() {
        guard id.flatMap(store.model) == nil else { return }
        id = store.documentForWindow(id)
    }

    /// Opens a window for every queued document (only the key window
    /// does it, so each opens once).
    private func openQueued() {
        guard activeState == .key || store.documents.count <= 1 else { return }
        while let next = store.takePending() {
            openWindow(value: next)
        }
    }
}

/// Whether a document's own window (document or mixer) is key: the
/// timer's note target, since the timer has no SwiftUI focus.
struct KeyDoc: Equatable {
    let id: UUID?
    let key: Bool

    @MainActor static func changed(_ store: DocumentStore, window: UUID, from old: KeyDoc, to new: KeyDoc) {
        if let o = old.id, old.key, !new.key || new.id != o { store.blurDocument(o, window: window) }
        if let n = new.id, new.key { store.focusDocument(n, window: window) }
    }
}

/// Restores the document window's saved frame once and saves it after each
/// move or resize (plan 7a Ruling 11), in `ui.json` with the panes.
struct WindowFrameKeeper: NSViewRepresentable {
    let model: DocumentModel

    final class Keeper: NSView {
        override init(frame: NSRect) {
            super.init(frame: frame)
            clipsToBounds = true
        }

        required init?(coder: NSCoder) { nil }

        var editor: EditorModel?
        private var applied = false
        private var observers: [NSObjectProtocol] = []

        // Inert for the user: clicks and VoiceOver pass through it.
        override func hitTest(_ point: NSPoint) -> NSView? { nil }
        override func isAccessibilityElement() -> Bool { false }

        override func viewDidMoveToWindow() {
            super.viewDidMoveToWindow()
            removeObservers()
            guard let w = window, let editor else { return }
            // Swift 6.1: the closures capture only strong lets made here,
            // never the window or a weak self (plan 7a Global Constraints).
            let view = self
            let first = !applied
            let saved = first ? editor.layout.views.frame : nil
            applied = true
            for name in [NSWindow.didMoveNotification, NSWindow.didEndLiveResizeNotification] {
                observers.append(
                    NotificationCenter.default.addObserver(forName: name, object: w, queue: .main) { note in
                        nonisolated(unsafe) let n = note
                        MainActor.assumeIsolated {
                            if let w = n.object as? NSWindow {
                                editor.setDocumentFrame([w.frame.minX, w.frame.minY, w.frame.width, w.frame.height])
                            }
                        }
                    })
            }
            guard let saved else {
                // No saved frame: a new window opens at the default size,
                // centred, on the screen it is on (owner, 2026-11-05). Only
                // its first appearance: a later move into a window is not.
                guard first else { return }
                DispatchQueue.main.async {
                    MainActor.assumeIsolated {
                        guard let w = view.window, let screen = w.screen ?? NSScreen.main else { return }
                        w.setFrame(PaneGeometry.defaultDocumentFrame(visible: screen.visibleFrame), display: true)
                    }
                }
                return
            }
            // On the next turn, so SwiftUI's own restoration does not
            // overwrite it.
            DispatchQueue.main.async {
                MainActor.assumeIsolated {
                    guard let w = view.window else { return }
                    // A saved frame off every screen falls back to the
                    // default size, as a window with none does.
                    guard let r = PaneGeometry.sanitizedFrame(saved, screens: NSScreen.screens.map(\.frame)) else {
                        if let screen = w.screen ?? NSScreen.main {
                            w.setFrame(PaneGeometry.defaultDocumentFrame(visible: screen.visibleFrame), display: true)
                        }
                        return
                    }
                    // `setFrame` does not keep the title bar on a screen;
                    // the screen it is on does (final review n1).
                    let screen = NSScreen.screens.first { $0.frame.intersects(r) }
                    w.setFrame(w.constrainFrameRect(r, to: screen), display: true)
                }
            }
        }

        func removeObservers() {
            for o in observers { NotificationCenter.default.removeObserver(o) }
            observers = []
        }
    }

    func makeNSView(context: Context) -> Keeper {
        let k = Keeper()
        k.editor = model.editor
        return k
    }

    func updateNSView(_ k: Keeper, context: Context) {}

    static func dismantleNSView(_ k: Keeper, coordinator: ()) {
        k.removeObservers()
    }
}
