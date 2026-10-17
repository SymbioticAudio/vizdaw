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
        DocumentStore(
            makeEmpty: { try Session.newEmpty() },
            makeDemo: { try Session.newDemo() },
            open: { try Session.open(path: $0) }
        )
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

    /// The document menu commands act on: a key torn-off window's, else the
    /// focused document window's.
    public func commandTarget(focused: UUID?) -> UUID? {
        tornFocus.map(\.doc) ?? focused
    }

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

    public func newDemo() -> UUID? { add("Demo Project Failed", makeDemo) }

    /// Opens a `.vizdaw` package; other paths are refused with an alert.
    public func open(path: String) -> UUID? {
        guard (path as NSString).pathExtension == "vizdaw" else {
            alert = AlertItem(
                title: "Open Failed",
                message: "\((path as NSString).lastPathComponent) is not a .vizdaw project.")
            return nil
        }
        guard let recovery else {
            return add("Open Failed") { try self.openPath(path) }
        }
        guard let options = recovery.options(for: path) else { return nil }
        return add("Open Failed") { try recovery.openWith(path, options) }
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

    /// The store-level alert, cleared (the app shows it once).
    public func takeAlert() -> AlertItem? {
        defer { alert = nil }
        return alert
    }

    /// A window closed: its document goes (a clean close: the crash
    /// marker is removed).
    public func close(_ id: UUID) {
        if tornFocus?.doc == id { tornFocus = nil }
        mixers.removeValue(forKey: id)?.endOpenDrag()
        // Plugin windows detach their editors before the session goes.
        willClose?(id)
        if let doc = documents.removeValue(forKey: id) {
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
            let id = UUID()
            documents[id] = model
            return id
        } catch {
            alert = AlertItem(error, title: title)
            return nil
        }
    }
}
