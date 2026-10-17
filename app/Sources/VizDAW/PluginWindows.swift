import AppKit
import SwiftUI
import VizDAWAppModel
import VizDAWCore

/// Opens slot windows from the mixer (an environment action).
struct OpenPluginWindowKey: EnvironmentKey {
    static let defaultValue: @MainActor (UInt64) -> Void = { _ in }
}

extension EnvironmentValues {
    var openPluginWindow: @MainActor (UInt64) -> Void {
        get { self[OpenPluginWindowKey.self] }
        set { self[OpenPluginWindowKey.self] = newValue }
    }
}

/// Every open plugin window, by document and slot (spec §9: one window
/// per instance).
@MainActor
enum PluginWindows {
    private struct Key: Hashable {
        let doc: UUID
        let slot: UInt64
    }

    private static var open: [Key: PluginWindowController] = [:]

    /// A closed window whose editor could not detach yet (`Busy`): the
    /// parent view stays alive until `closeEditor` succeeds.
    private struct PendingClose {
        let key: Key
        let model: PluginWindowModel
        let host: NSView
    }

    private static var pending: [PendingClose] = []

    /// Keeps `host` alive and retries `model`'s close on every tick.
    static func closePending(doc: UUID, model: PluginWindowModel, host: NSView) {
        pending.append(PendingClose(key: Key(doc: doc, slot: model.slot), model: model, host: host))
    }

    /// A document is closing: its windows close and every editor detaches
    /// before the session goes. A close that stays `Busy` after a few
    /// retries is released anyway (closing the session closes the editors).
    static func closeDocument(_ doc: UUID) {
        for (key, c) in Array(open) where key.doc == doc {
            guard open[key] === c else { continue }
            c.close()
        }
        for _ in 0..<5 {
            let mine = pending.filter { $0.key.doc == doc }
            if mine.isEmpty { break }
            let done = mine.filter { $0.model.retryClose() }.map(\.model)
            pending.removeAll { p in done.contains { $0 === p.model } }
        }
        pending.removeAll { $0.key.doc == doc }
    }

    /// Shows slot `slot`'s window of document `doc` (brings it to the
    /// front when open).
    static func show(_ store: DocumentStore, doc: UUID, slot: UInt64) {
        let key = Key(doc: doc, slot: slot)
        if let w = open[key] {
            w.showWindow(nil)
            return
        }
        // The previous window's editor is still detaching: wait for it.
        guard !pending.contains(where: { $0.key == key }) else { return }
        guard let model = store.model(doc) else { return }
        let window = PluginWindowModel(document: model, slot: slot)
        // A slot that is gone (a stale saved window) gets no window.
        guard !window.isGone else { return }
        let c = PluginWindowController(model: window, docID: doc) {
            open[key] = nil
        }
        // Registered before the editor opens (which can spin the run loop):
        // a re-entrant show() finds this window.
        open[key] = c
        c.showWindow(nil)
        c.start()
    }

    /// The editor size an open window shows (the `--smoke` run).
    static func editorSize(doc: UUID, slot: UInt64) -> EditorSize? {
        open[Key(doc: doc, slot: slot)]?.model.editorSize
    }

    /// Reopens the windows that were open when the project was saved.
    static func reopen(_ store: DocumentStore, doc: UUID) {
        guard let model = store.model(doc), let slots = try? model.session.openWindows() else { return }
        for s in slots { show(store, doc: doc, slot: s) }
    }

    /// The timer's step: windows follow their slot (close when it is gone,
    /// open the editor once it runs, reread changed parameters).
    static func tick(_ store: DocumentStore) {
        // Closing and opening editors can spin the run loop and re-enter.
        guard !ticking else { return }
        ticking = true
        defer { ticking = false }
        // Never mutate `pending` or `open` while calling into the session.
        let items = pending
        var done: [PluginWindowModel] = []
        for p in items where p.model.retryClose() { done.append(p.model) }
        pending.removeAll { p in done.contains { $0 === p.model } }
        for (key, c) in Array(open) {
            guard open[key] === c else { continue }
            guard let doc = store.model(key.doc) else {
                c.close()
                continue
            }
            let now = PluginWindowController.Seen(slots: doc.slotsVersion, doc: doc.snapshot.version)
            if doc.paramsChanged.contains(key.slot) || c.seen != now {
                c.update(now)
            }
        }
        for doc in store.documents.values { doc.paramsChanged.removeAll() }
    }

    private static var ticking = false
}

/// One plugin window: header (bypass, status), then the plugin's editor in
/// a plain `NSView` handed to Rust, or its parameters.
final class PluginWindowController: NSWindowController, NSWindowDelegate {
    let model: PluginWindowModel
    /// The view the plugin's editor goes into: alive until `close_editor`
    /// ran (in `windowWillClose`), as the FFI requires.
    let editorHost = NSView(frame: NSRect(x: 0, y: 0, width: 400, height: 300))
    private let docID: UUID
    private let onClose: @MainActor () -> Void

    /// What the window last followed: an edit (an undo of the add removes
    /// the slot) or a slot change (loaded, failed) makes it look again.
    struct Seen: Equatable {
        let slots: Int
        let doc: UInt64
    }

    private(set) var seen: Seen?

    /// Header height in the content view (fixed, see `PluginWindowContent`).
    static let headerHeight: CGFloat = 37
    private var applyingSize = false

    init(model: PluginWindowModel, docID: UUID, onClose: @escaping @MainActor () -> Void) {
        self.model = model
        self.docID = docID
        self.onClose = onClose
        let window = NSWindow(
            contentRect: NSRect(x: 0, y: 0, width: 420, height: 360),
            styleMask: [.titled, .closable, .miniaturizable], backing: .buffered, defer: false)
        window.isReleasedWhenClosed = false
        super.init(window: window)
        window.delegate = self
        let hosting = NSHostingView(rootView: PluginWindowContent(model: model, editorHost: editorHost))
        // The window, not the SwiftUI content, decides the size.
        hosting.sizingOptions = []
        window.contentView = hosting
        window.center()
        model.opened()
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) { fatalError("not used") }

    /// The first look at the slot (after the controller is registered).
    func start() {
        update(Seen(slots: model.document.slotsVersion, doc: model.document.snapshot.version))
    }

    /// Follows the slot: title, closing when gone, the editor once live.
    /// `seen` is only set when everything succeeded; a `Busy` read or editor
    /// open is retried on the next tick.
    func update(_ now: Seen) {
        let ok = model.refresh()
        guard let window else { return }
        if model.isGone {
            seen = now
            close()
            return
        }
        window.title = model.title
        var sizeWindow = false
        if model.editorSize == nil && model.isLive {
            let address = UInt64(UInt(bitPattern: Unmanaged.passUnretained(editorHost).toOpaque()))
            if model.openEditor(parent: address), let size = model.editorSize {
                editorHost.setFrameSize(NSSize(width: Int(size.width), height: Int(size.height)))
                if size.resizable {
                    window.styleMask.insert(.resizable)
                    window.contentMinSize = NSSize(width: 120, height: Self.headerHeight + 60)
                } else {
                    window.styleMask.remove(.resizable)
                }
                sizeWindow = true
            }
        }
        seen = (ok && !model.openOwed) ? now : nil
        if model.editorSize == nil {
            // Parameters (or a message): a fixed window.
            window.styleMask.remove(.resizable)
            sizeWindow = true
        }
        if sizeWindow { applyContentSize(contentSize()) }
    }

    private func contentSize() -> NSSize {
        if let s = model.editorSize {
            return NSSize(width: CGFloat(s.width), height: CGFloat(s.height) + Self.headerHeight)
        }
        let n = model.visibleParams.count
        let h = n == 0 ? 120 : min(420, CGFloat(n) * 30 + 30)
        return NSSize(width: n == 0 ? 360 : 400, height: h + Self.headerHeight)
    }

    private func applyContentSize(_ size: NSSize) {
        guard let window else { return }
        applyingSize = true
        window.setContentSize(size)
        applyingSize = false
    }

    /// The user resized the window: the editor gets the new content size and
    /// the window takes the size the plugin answers with.
    func windowDidResize(_ notification: Notification) {
        guard !applyingSize, model.editorSize?.resizable == true, let window else { return }
        let content = window.contentLayoutRect.size
        let w = UInt32(max(1, content.width.rounded()))
        let h = UInt32(max(1, (content.height - Self.headerHeight).rounded()))
        model.resize(width: w, height: h)
        if let s = model.editorSize, s.width != w || s.height != h {
            applyContentSize(NSSize(width: CGFloat(s.width), height: CGFloat(s.height) + Self.headerHeight))
        }
    }

    func windowWillClose(_ notification: Notification) {
        if !model.closed() {
            PluginWindows.closePending(doc: docID, model: model, host: editorHost)
        }
        onClose()
    }
}

/// The window's SwiftUI content.
struct PluginWindowContent: View {
    let model: PluginWindowModel
    let editorHost: NSView

    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            HStack {
                Toggle(
                    "Bypass",
                    isOn: Binding(get: { model.row?.bypass ?? false }, set: { _ in model.toggleBypass() })
                )
                .toggleStyle(.button)
                if let r = model.placeholderReason {
                    Text(r).foregroundStyle(.secondary)
                }
                Spacer()
            }
            .padding(6)
            .frame(height: PluginWindowController.headerHeight - 1)
            Divider()
            if let size = model.editorSize {
                if size.resizable {
                    // Follows the window; the controller reports the size.
                    EditorHost(view: editorHost)
                        .frame(maxWidth: .infinity, maxHeight: .infinity)
                } else {
                    EditorHost(view: editorHost)
                        .frame(width: CGFloat(size.width), height: CGFloat(size.height))
                }
            } else {
                ParameterList(model: model)
            }
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
    }
}

/// The plain `NSView` the plugin's editor lives in.
struct EditorHost: NSViewRepresentable {
    let view: NSView
    func makeNSView(context: Context) -> NSView { view }
    func updateNSView(_ nsView: NSView, context: Context) {}
}

/// A plugin without an editor (or not running): its parameters as
/// sliders; a drag is one undo step.
struct ParameterList: View {
    let model: PluginWindowModel

    var body: some View {
        if model.visibleParams.isEmpty {
            Text(model.editorProblem ?? "No parameters.")
                .foregroundStyle(.secondary)
                .frame(width: 360, height: 120)
        } else {
            ScrollView {
                VStack(alignment: .leading, spacing: 6) {
                    ForEach(model.visibleParams, id: \.id) { p in
                        HStack {
                            Text(p.name).frame(width: 110, alignment: .leading).lineLimit(1)
                            Slider(
                                value: Binding(get: { p.value }, set: { model.setParam(p.id, $0) }),
                                in: p.min...max(p.max, p.min + 1e-9)
                            ) { editing in
                                if !editing { model.commitParam(p.id) }
                            }
                            Text(PluginWindowModel.valueText(p))
                                .font(.caption.monospacedDigit())
                                .frame(width: 48, alignment: .trailing)
                        }
                    }
                }
                .padding(10)
            }
            .frame(width: 400, height: min(420, CGFloat(model.visibleParams.count) * 30 + 30))
        }
    }
}
