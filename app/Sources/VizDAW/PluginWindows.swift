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
        open[key] = c
        c.showWindow(nil)
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
        pending.removeAll { $0.model.retryClose() }
        for (key, c) in open {
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
        window.contentView = NSHostingView(
            rootView: PluginWindowContent(model: model, editorHost: editorHost))
        window.center()
        model.opened()
        update(Seen(slots: model.document.slotsVersion, doc: model.document.snapshot.version))
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) { fatalError("not used") }

    /// Follows the slot: title, closing when gone, the editor once live.
    func update(_ now: Seen) {
        seen = now
        model.refresh()
        guard let window else { return }
        if model.isGone {
            close()
            return
        }
        window.title = model.title
        if model.editorSize == nil && model.isLive {
            let address = UInt64(UInt(bitPattern: Unmanaged.passUnretained(editorHost).toOpaque()))
            if model.openEditor(parent: address), let size = model.editorSize {
                editorHost.setFrameSize(NSSize(width: Int(size.width), height: Int(size.height)))
                if size.resizable { window.styleMask.insert(.resizable) }
            }
        }
        if let content = window.contentView {
            window.setContentSize(content.fittingSize)
        }
    }

    func windowDidResize(_ notification: Notification) {
        guard model.editorSize?.resizable == true else { return }
        let s = editorHost.frame.size
        model.resize(width: UInt32(max(1, s.width)), height: UInt32(max(1, s.height)))
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
            Divider()
            if let size = model.editorSize {
                EditorHost(view: editorHost)
                    .frame(width: CGFloat(size.width), height: CGFloat(size.height))
            } else {
                ParameterList(model: model)
            }
        }
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
