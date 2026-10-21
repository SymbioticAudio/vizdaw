import Foundation
import Observation
import VizDAWCore

/// A torn-off editor window that is key (Plan 4b Ruling 18).
public struct TornFocus: Equatable, Sendable {
    public let doc: UUID
    public let window: UUID

    public init(doc: UUID, window: UUID) {
        self.doc = doc
        self.window = window
    }
}

/// The open documents, one per window, keyed by the window's value.
@Observable
@MainActor
public final class DocumentStore {
    public typealias Make = () throws -> any SessionProtocol
    public typealias Open = (String) throws -> any SessionProtocol

    public private(set) var documents: [UUID: DocumentModel] = [:]
    /// Mixer windows' models, made on first use and dropped with their
    /// document.
    private var mixers: [UUID: MixerModel] = [:]
    /// Documents created without a window yet (Finder opens, launch
    /// arguments); the next window to appear adopts the first one.
    public private(set) var pending: [UUID] = []
    /// Failures that have no window to show them in.
    public var alert: AlertItem?
    /// Crash and autosave checks before a project opens (spec §10); none
    /// in tests that do not set it.
    public var recovery: Recovery?
    /// Runs before a document's session closes (the app closes that
    /// document's plugin windows and releases their editors).
    public var willClose: ((UUID) -> Void)?

    /// Makes a project in a format (set by `live()`; tests set a fake).
    public var makeProject: ((ProjectFormatRow) throws -> any SessionProtocol)?

    /// The app's preferences (in memory in tests; `live()` sets the real
    /// store, which is itself in memory under `VIZDAW_SUPPORT_DIR`).
    public var defaults: UserDefaults = MemoryDefaults()

    /// The "Default project format" preference (5a Ruling 18), e.g.
    /// "48000/24"; 48 kHz / 24-bit when unset or damaged.
    public var defaultFormat: ProjectFormatRow {
        get {
            let s = defaults.string(forKey: "defaultProjectFormat") ?? ""
            let p = s.split(separator: "/").map(String.init)
            return ControlFormats.parse(rate: p.first, depth: p.count > 1 ? p[1] : nil)
                ?? ProjectFormatRow(sampleRate: 48_000, depth: .int24)
        }
        set {
            let d: String =
                switch newValue.depth {
                case .int16: "16"
                case .int24: "24"
                case .float32: "32f"
                }
            defaults.set("\(newValue.sampleRate)/\(d)", forKey: "defaultProjectFormat")
        }
    }

    private let makeEmpty: Make
    private let makeDemo: Make
    private let openPath: Open

    public init(makeEmpty: @escaping Make, makeDemo: @escaping Make, open: @escaping Open) {
        self.makeEmpty = makeEmpty
        self.makeDemo = makeDemo
        self.openPath = open
    }

    /// The store backed by the Rust library.
    public static func live() -> DocumentStore {
        let s = DocumentStore(
            makeEmpty: { try Session.newEmpty() },
            makeDemo: { try Session.newDemo() },
            open: { try Session.open(path: $0) }
        )
        s.makeDemoAudio = { try Session.newDemoAudio() }
        s.makeProject = { try Session.newProject(format: $0) }
        s.recoverUntitled = { try recoverUntitledRecordings() }
        s.defaults = MemoryDefaults.appStore()
        s.listPreferences = ListPreferences(defaults: MemoryDefaults.appStore())
        return s
    }

    public func model(_ id: UUID) -> DocumentModel? { documents[id] }

    /// The torn-off editor window that is key, if any. AppKit windows
    /// publish no SwiftUI focused values, so menu commands ask this first.
    public private(set) var tornFocus: TornFocus?

    public func focusTorn(_ f: TornFocus) { tornFocus = f }

    /// Window `window` resigned key or closed.
    public func blurTorn(window: UUID) {
        if tornFocus?.window == window { tornFocus = nil }
    }

    /// The document whose select-and-modify panel is key (Plan 4c Ruling 22).
    public private(set) var panelFocus: UUID?

    public func focusPanel(_ doc: UUID) { panelFocus = doc }

    /// Document `doc`'s panel resigned key or closed.
    public func blurPanel(_ doc: UUID) {
        if panelFocus == doc { panelFocus = nil }
    }

    /// Document `doc`'s panel window became key: it takes the menu
    /// commands and the transport keys go off.
    public func panelBecameKey(_ doc: UUID) {
        focusPanel(doc)
        model(doc)?.selectModify.isKey = true
    }

    /// Document `doc`'s panel window resigned key: its fields give the
    /// keyboard back too (a field stays first responder in a non-key panel).
    public func panelResignedKey(_ doc: UUID) {
        blurPanel(doc)
        guard let p = model(doc)?.selectModify else { return }
        p.isKey = false
        p.endTextEditing()
    }

    /// Document `doc`'s panel window closed (by the user or with its document).
    public func panelClosed(_ doc: UUID) {
        blurPanel(doc)
        model(doc)?.selectModify.close()
    }

    /// The document menu commands act on: a key torn-off window's, else a
    /// key select-and-modify panel's, else the focused document window's.
    public func commandTarget(focused: UUID?) -> UUID? {
        tornFocus.map(\.doc) ?? panelFocus ?? focused
    }

    /// The key window among documents' own windows (document or mixer),
    /// by a token each window view holds; the timer, which has no SwiftUI
    /// focus, asks this. Per window, so a document window and its own
    /// mixer handing focus over never clear each other, in either order.
    private var keyWindow: (doc: UUID, window: UUID)?

    /// The document whose own window is key.
    public var keyDocument: UUID? { keyWindow?.doc }

    public func focusDocument(_ doc: UUID, window: UUID) { keyWindow = (doc, window) }

    /// Window `window` (of document `doc`) resigned key or closed; only the
    /// window that last took focus clears it.
    public func blurDocument(_ doc: UUID, window: UUID) {
        if keyWindow?.doc == doc, keyWindow?.window == window { keyWindow = nil }
    }

    /// The document played notes go to: the command target (Plan 4c
    /// Ruling 11).
    public var noteTarget: DocumentModel? { commandTarget(focused: keyDocument).flatMap(model) }

    /// Document `id`'s mixer (one per document), or nil once it is closed.
    public func mixer(_ id: UUID) -> MixerModel? {
        if let m = mixers[id] { return m }
        guard let doc = documents[id] else { return nil }
        do {
            let m = try MixerModel(document: doc)
            mixers[id] = m
            return m
        } catch {
            doc.alert = AlertItem(error, title: "Mixer Unavailable")
            return nil
        }
    }

    public func newEmpty() -> UUID? { add("New Document Failed", makeEmpty) }

    /// A new project in `format` (the New Project sheet, `file.new`).
    public func newProject(_ format: ProjectFormatRow) -> UUID? {
        guard let make = makeProject else { return newEmpty() }
        return add("New Document Failed") { try make(format) }
    }

    public func newDemo() -> UUID? { add("Demo Project Failed", makeDemo) }

    /// Makes the demo with an audio lane (Plan 4b Ruling 19); set by `live()`.
    public var makeDemoAudio: Make?

    /// List defaults and columns given to every document (Plan 4c Ruling 19).
    public var listPreferences = ListPreferences()
    /// The app's key bindings, handed to every document (plan 4f).
    public var keyBindings: KeyBindings?

    public func newDemoAudio() -> UUID? {
        guard let make = makeDemoAudio else { return nil }
        return add("Demo Project Failed", make)
    }

    /// Opens a `.vizdaw` package; other paths are refused with an alert.
    public func open(path: String) -> UUID? {
        guard (path as NSString).pathExtension == "vizdaw" else {
            alert = AlertItem(
                title: "Open Failed",
                message: "\((path as NSString).lastPathComponent) is not a .vizdaw project.")
            return nil
        }
        // A project that is already open brings its window to the front
        // (the core would refuse with "already open in another window").
        if let existing = documentID(forPath: path) {
            activate?(existing)
            return nil
        }
        guard let recovery else {
            return add("Open Failed") { try self.openPath(path) }
        }
        guard let options = recovery.options(for: path) else { return nil }
        return add("Open Failed") { try recovery.openWith(path, options) }
    }

    /// Brings a document's window to the front (set by the app).
    public var activate: ((UUID) -> Void)?

    /// The open document saved at `path` (same file, ignoring case and
    /// symbolic links), if any.
    public func documentID(forPath path: String) -> UUID? {
        let want = Self.canonical(path)
        return documents.first { (try? $0.value.session.path()).flatMap { $0 }.map(Self.canonical) == want }?.key
    }

    private static func canonical(_ path: String) -> String {
        URL(fileURLWithPath: path).resolvingSymlinksInPath().standardizedFileURL.path.lowercased()
    }

    /// The launch-time check for recordings lost with untitled projects
    /// (set by `live()`); returns the line to show.
    public var recoverUntitled: () throws -> String? = { nil }
    private var launchRecoveryDone = false

    /// Runs `recoverUntitled` once per launch, before any document opens;
    /// its line is logged and queued as an alert (plan 5c Task 10 review
    /// M1: never dropped).
    public func runLaunchRecovery() {
        guard !launchRecoveryDone else { return }
        launchRecoveryDone = true
        guard let line = (try? recoverUntitled()) ?? nil else { return }
        FileHandle.standardError.write(Data("VizDAW: \(line)\n".utf8))
        postAlert(AlertItem(title: "Recordings Moved to the Trash", message: line))
    }

    /// Queues a document for the next window to appear.
    public func enqueue(_ id: UUID?) {
        if let id { pending.append(id) }
    }

    /// The oldest queued document, removed from the queue.
    public func takePending() -> UUID? {
        pending.isEmpty ? nil : pending.removeFirst()
    }

    /// The document a window should show: its own when it still exists,
    /// else the oldest queued one, else a new empty one (nil only if that
    /// fails). Covers fresh windows and stale restored or closed ids.
    public func documentForWindow(_ id: UUID?) -> UUID? {
        if let id, documents[id] != nil { return id }
        return takePending() ?? newEmpty()
    }

    /// Alerts posted while another was showing, oldest first.
    private var queuedAlerts: [AlertItem] = []

    /// Shows `item` now, or after the alerts already showing or waiting
    /// (Task 10 review M1: never dropped).
    public func postAlert(_ item: AlertItem) {
        if alert == nil && queuedAlerts.isEmpty {
            alert = item
        } else {
            queuedAlerts.append(item)
        }
    }

    /// The store-level alert, cleared (the app shows it once); the next
    /// posted one takes its place.
    public func takeAlert() -> AlertItem? {
        let current = alert ?? (queuedAlerts.isEmpty ? nil : queuedAlerts.removeFirst())
        alert = queuedAlerts.isEmpty ? nil : queuedAlerts.removeFirst()
        return current
    }

    /// A window closed: its document goes (a clean close: the crash
    /// marker is removed).
    public func close(_ id: UUID) {
        if tornFocus?.doc == id { tornFocus = nil }
        if panelFocus == id { panelFocus = nil }
        if keyWindow?.doc == id { keyWindow = nil }
        mixers.removeValue(forKey: id)?.endOpenDrag()
        // Plugin windows detach their editors before the session goes.
        willClose?(id)
        if let doc = documents.removeValue(forKey: id) {
            // A note an editor sounds (a press on a closing piano roll) ends.
            doc.editor.auditionOffAll()
            // No list outlives its document (none keeps MIDI input running).
            doc.dropLists()
            if doc.isPlaying { try? doc.session.stop() }
            try? doc.session.close()
        }
    }

    /// The app quits: every document closes cleanly.
    public func closeAll() {
        for id in Array(documents.keys) { close(id) }
    }

    private func add(_ title: String, _ make: Make) -> UUID? {
        do {
            let model = try DocumentModel(session: make())
            model.listPreferences = listPreferences
            model.keys = keyBindings
            let id = UUID()
            documents[id] = model
            return id
        } catch {
            alert = AlertItem(error, title: title)
            return nil
        }
    }
}
