import AppKit
import SwiftUI
import SymbioticTokens
import VizDAWAppModel
import VizDAWCore

/// Opens a Built-in Synth destination's window from the mixer (an
/// environment action, as `openPluginWindow`).
struct OpenBuiltinSynthWindowKey: EnvironmentKey {
    static let defaultValue: @MainActor (UInt64) -> Void = { _ in }
}

extension EnvironmentValues {
    var openBuiltinSynthWindow: @MainActor (UInt64) -> Void {
        get { self[OpenBuiltinSynthWindowKey.self] }
        set { self[OpenBuiltinSynthWindowKey.self] = newValue }
    }
}

/// The Built-in Synth's window (plan release-fixes Task 8): its five
/// sounds. A sound belongs to the destination (Decision 3), so a change is
/// heard on every track that plays it, and the window says so. One window
/// per destination, as `PluginWindows`: a second double-click brings it
/// forward. It opens only from a double-click on the mixer's Built-in
/// Synth row, never from the control channel.
@MainActor
enum BuiltinSynthWindows {
    private struct Key: Hashable {
        let doc: UUID
        let dest: UInt64
    }

    private static var open: [Key: BuiltinSynthWindowController] = [:]

    /// Shows destination `dest`'s window of document `doc` (brings it to
    /// the front when open).
    static func show(_ store: DocumentStore, doc: UUID, dest: UInt64) {
        let key = Key(doc: doc, dest: dest)
        if let w = open[key] {
            w.showWindow(nil)
            return
        }
        guard let model = store.model(doc) else { return }
        let c = BuiltinSynthWindowController(document: model, docID: doc, dest: dest) {
            open[key] = nil
        }
        open[key] = c
        c.showWindow(nil)
    }

    /// A document is closing: its windows close.
    static func closeDocument(_ doc: UUID) {
        for (key, c) in Array(open) where key.doc == doc { c.close() }
    }
}

/// One Built-in Synth window.
final class BuiltinSynthWindowController: NSWindowController, NSWindowDelegate {
    private let onClose: @MainActor () -> Void

    init(document: DocumentModel, docID: UUID, dest: UInt64, onClose: @escaping @MainActor () -> Void) {
        self.onClose = onClose
        let window = NSWindow(
            contentRect: NSRect(x: 0, y: 0, width: 300, height: 200),
            styleMask: [.titled, .closable, .miniaturizable], backing: .buffered, defer: false)
        window.isReleasedWhenClosed = false
        window.title = "Built-in Synth"
        super.init(window: window)
        ControlWindows.tag(window, .other, doc: docID)
        window.delegate = self
        ThemeWindows.style(window)
        window.contentView = NSHostingView(rootView: BuiltinSynthContent(document: document, dest: dest).themedRoot())
        window.center()
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) { fatalError("not used") }

    func windowWillClose(_ notification: Notification) { onClose() }
}

/// The window's content: the sounds, then what a change reaches.
struct BuiltinSynthContent: View {
    @Environment(\.symbioticTheme) private var theme
    let document: DocumentModel
    let dest: UInt64

    var body: some View {
        let sheets = document.synth
        // Read so the window follows every step (an undo, too).
        let _ = document.snapshot.version
        VStack(alignment: .leading, spacing: theme.spacing.m) {
            Picker(
                "Sound",
                selection: Binding(
                    get: { sheets.sound(of: dest) },
                    set: { s in if let s { sheets.setSound(s, dest: dest) } })
            ) {
                ForEach(SynthSheets.sounds, id: \.self) { s in
                    Text(SynthSheets.title(s)).tag(Optional(s))
                }
            }
            .pickerStyle(.radioGroup)
            .accessibilityLabel("Sound")
            Text("Changes every track that plays this Built-in Synth.")
                .font(theme.type.caption.font)
                .foregroundStyle(theme.text.secondary.color)
                .fixedSize(horizontal: false, vertical: true)
        }
        .font(theme.type.body.font)
        .foregroundStyle(theme.text.primary.color)
        .padding(theme.spacing.xl)
        .frame(width: 300, alignment: .leading)
    }
}
