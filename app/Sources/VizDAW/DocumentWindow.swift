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
                    // The document goes as its window closes: SwiftUI's
                    // onDisappear does not always come for the last window.
                    .background(WindowFrameKeeper(model: model, doc: id, onClose: { store.close(id) }))
                    .focusedSceneValue(\.documentModel, model)
                    .focusedSceneValue(\.documentID, id)
                    .task(id: PollKey(key: activeState == .key, playing: model.isPlaying || model.record.isRecording)) {
                        // Background playback keeps polling so device
                        // events are handled (O27), and so does a
                        // recording until the poll sees it finished.
                        guard activeState == .key || model.isPlaying || model.record.isRecording else { return }
                        // The close button's edited dot follows the song,
                        // read at most twice a second (release gate B1).
                        var dotRead = ContinuousClock.now - .seconds(1)
                        while !Task.isCancelled {
                            model.poll()
                            if ContinuousClock.now - dotRead >= .milliseconds(500) {
                                dotRead = ContinuousClock.now
                                CloseHooks.hook(for: id)?.refresh()
                            }
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
/// It also installs the window's close hook (release gate B1).
struct WindowFrameKeeper: NSViewRepresentable {
    let model: DocumentModel
    let doc: UUID
    /// Runs when the window will close.
    var onClose: @MainActor @Sendable () -> Void = {}

    final class Keeper: NSView {
        override init(frame: NSRect) {
            super.init(frame: frame)
            clipsToBounds = true
        }

        required init?(coder: NSCoder) { nil }

        var editor: EditorModel?
        var onClose: @MainActor @Sendable () -> Void = {}
        /// The window's close hook, held here (the window holds its
        /// delegate weakly).
        var hook: CloseHook?
        private var applied = false
        private var observers: [NSObjectProtocol] = []

        // Inert for the user: clicks and VoiceOver pass through it.
        override func hitTest(_ point: NSPoint) -> NSView? { nil }
        override func isAccessibilityElement() -> Bool { false }

        override func viewDidMoveToWindow() {
            super.viewDidMoveToWindow()
            removeObservers()
            guard let w = window, let editor else { return }
            hook?.install(on: w)
            // Swift 6.1: the closures capture only strong lets made here,
            // never the window or a weak self (plan 7a Global Constraints).
            let view = self
            let close = onClose
            observers.append(
                NotificationCenter.default.addObserver(forName: NSWindow.willCloseNotification, object: w, queue: .main) { _ in
                    MainActor.assumeIsolated { close() }
                })
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
        k.onClose = onClose
        let hook = CloseHook(doc: doc, model: model)
        k.hook = hook
        CloseHooks.register(hook)
        return k
    }

    func updateNSView(_ k: Keeper, context: Context) {}

    static func dismantleNSView(_ k: Keeper, coordinator: ()) {
        k.removeObservers()
        if let hook = k.hook { CloseHooks.unregister(hook) }
    }
}

/// The document windows' close hooks by document, held weakly (each
/// window's keeper holds its own).
@MainActor
enum CloseHooks {
    private final class Weak {
        weak var hook: CloseHook?
        init(_ h: CloseHook) { hook = h }
    }

    private static var hooks: [UUID: Weak] = [:]

    static func register(_ h: CloseHook) { hooks[h.doc] = Weak(h) }

    static func unregister(_ h: CloseHook) {
        if hooks[h.doc]?.hook === h { hooks[h.doc] = nil }
    }

    static func hook(for doc: UUID) -> CloseHook? { hooks[doc]?.hook }
}

/// A document window's delegate in front of SwiftUI's (release gate B1).
/// It answers `windowShouldClose(_:)`: a song with unsaved changes asks
/// Save, Don't Save or Cancel in a sheet first; a clean one closes as
/// before. Every other delegate call goes to SwiftUI's delegate unchanged.
/// Covers ⌘W, File › Close, Window › Close and the red close button; the
/// control channel's closes call `close()`, which never asks.
@MainActor
final class CloseHook: NSObject, NSWindowDelegate {
    let doc: UUID
    let model: DocumentModel
    private(set) weak var window: NSWindow?
    /// SwiftUI's delegate, which every other call goes to. Read by the
    /// nonisolated forwarding overrides; only set on the main actor.
    nonisolated(unsafe) private weak var original: NSObjectProtocol?

    init(doc: UUID, model: DocumentModel) {
        self.doc = doc
        self.model = model
    }

    /// Puts the hook in front of `w`'s delegate (again, if SwiftUI set
    /// its own since).
    func install(on w: NSWindow) {
        window = w
        guard w.delegate !== self else { return }
        original = w.delegate
        w.delegate = self
    }

    /// The edited dot follows the song; the hook is put back if SwiftUI
    /// replaced it.
    func refresh() {
        guard let w = window else { return }
        install(on: w)
        let edited = model.hasUnsavedChanges
        if w.isDocumentEdited != edited { w.isDocumentEdited = edited }
    }

    nonisolated override func responds(to aSelector: Selector!) -> Bool {
        super.responds(to: aSelector) || original?.responds(to: aSelector) == true
    }

    nonisolated override func forwardingTarget(for aSelector: Selector!) -> Any? {
        if let o = original, o.responds(to: aSelector) { return o }
        return super.forwardingTarget(for: aSelector)
    }

    func windowShouldClose(_ sender: NSWindow) -> Bool {
        let clean = (original as? NSWindowDelegate)?.windowShouldClose?(sender) ?? true
        guard clean else { return false }
        guard model.hasUnsavedChanges else { return true }
        // One sheet at a time; a second ⌘W while it shows does nothing.
        guard sender.attachedSheet == nil else { return false }
        ask { _, _ in }
        return false
    }

    /// Asks this song's sheet; Save (when it saved) and Don't Save close
    /// the window. `done` gets the choice and whether the window closed.
    func ask(_ done: @escaping @MainActor (CloseChoice, Bool) -> Void) {
        let hook = self
        CloseSheet.ask(model: model, window: window) { choice in
            switch choice {
            case .cancel:
                done(.cancel, false)
            case .dontSave:
                hook.closeQuietly()
                done(.dontSave, true)
            case .save:
                let saved = SaveFlow.save(hook.model, forceAs: false)
                if saved { hook.closeQuietly() }
                done(.save, saved)
            }
        }
    }

    /// Closes the window without asking (its will-close closes the
    /// document), or the document when the window is gone.
    func closeQuietly() {
        if let w = window {
            w.isDocumentEdited = false
            w.close()
        }
        appStore.close(doc)
    }
}

/// The "save the changes?" alert: a sheet on the song's window, or an
/// app-modal alert when it has none.
@MainActor
enum CloseSheet {
    static func ask(model: DocumentModel, window: NSWindow?, _ answer: @escaping @MainActor (CloseChoice) -> Void) {
        let text = ClosePrompts.sheetText(name: model.snapshot.title)
        let alert = NSAlert()
        alert.alertStyle = .warning
        alert.messageText = text.message
        alert.informativeText = text.informative
        // The first button is the default (Return).
        alert.addButton(withTitle: ClosePrompts.save)
        let dont = alert.addButton(withTitle: ClosePrompts.dontSave)
        dont.keyEquivalent = "d"
        dont.keyEquivalentModifierMask = .command
        let cancel = alert.addButton(withTitle: ClosePrompts.cancel)
        cancel.keyEquivalent = "\u{1b}"
        guard let window, window.isVisible || window.isMiniaturized else {
            answer(choice(alert.runModal()))
            return
        }
        if window.isMiniaturized { window.deminiaturize(nil) }
        alert.beginSheetModal(for: window) { response in
            MainActor.assumeIsolated { answer(choice(response)) }
        }
    }

    private static func choice(_ r: NSApplication.ModalResponse) -> CloseChoice {
        switch r {
        case .alertFirstButtonReturn: return .save
        case .alertSecondButtonReturn: return .dontSave
        default: return .cancel
        }
    }
}
