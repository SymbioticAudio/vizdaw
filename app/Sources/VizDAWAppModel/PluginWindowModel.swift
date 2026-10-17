import Foundation
import Observation
import VizDAWCore

/// One plugin window (spec §9): titled "‹strip› — ‹plugin›", the plugin's
/// own editor (or, without one, its parameters), a bypass switch in the
/// header. Closing it never removes the plugin.
@Observable
@MainActor
public final class PluginWindowModel {
    public let document: DocumentModel
    public let slot: UInt64
    public private(set) var row: SlotRow?
    public private(set) var stripName = ""
    public private(set) var params: [ParamRow] = []
    /// The open editor's size; nil while the window shows parameters.
    public private(set) var editorSize: EditorSize?
    /// Why the editor is not shown (no editor, not loaded yet).
    public private(set) var editorProblem: String?

    public init(document: DocumentModel, slot: UInt64) {
        self.document = document
        self.slot = slot
        refresh()
    }

    private var session: any SessionProtocol { document.session }

    // MARK: Derived

    public var title: String {
        let plugin = row?.plugin.name ?? "Plugin"
        return stripName.isEmpty ? plugin : "\(stripName) — \(plugin)"
    }

    /// The slot left the document (removed, or an undo of its add): the
    /// window closes.
    public var isGone: Bool { row == nil }

    public var isLive: Bool { row?.status == .live }

    /// The placeholder reason, if the plugin is not running.
    public var placeholderReason: String? {
        if case .placeholder(let r) = row?.status { return r }
        return nil
    }

    /// Parameters a user sees (not hidden).
    public var visibleParams: [ParamRow] { params.filter { !$0.hidden } }

    /// "0.50", or a whole number for stepped parameters.
    public static func valueText(_ p: ParamRow) -> String {
        p.stepped ? String(Int(p.value.rounded())) : String(format: "%.2f", p.value)
    }

    // MARK: Updates

    /// The editor open hit `Busy` (or is in progress) and is still owed;
    /// the owner retries it on the next tick.
    public private(set) var openOwed = false
    private var opening = false

    /// Rereads the slot, its strip and its parameters. A failed read (`Busy`
    /// during another editor's open) keeps what was known and returns false;
    /// only a successful read without the slot makes the window gone.
    @discardableResult
    public func refresh() -> Bool {
        var ok = true
        do {
            let mixer = try session.mixer()
            var found: SlotRow?
            var name = ""
            for s in mixer.strips {
                if let r = ([s.instrument].compactMap { $0 } + s.inserts).first(where: { $0.id == slot }) {
                    found = r
                    name = s.name
                }
            }
            row = found
            stripName = name
        } catch {
            ok = false
        }
        if let p = try? session.params(slot: slot) { params = p } else { ok = false }
        return ok
    }

    /// Opens the plugin's editor inside the view at address `parent`;
    /// false (and the reason kept) when it has none or is not running.
    /// `Busy` leaves the open owed (`openOwed`).
    @discardableResult
    public func openEditor(parent: UInt64) -> Bool {
        guard !opening else { return false }
        guard isLive else {
            editorProblem = placeholderReason ?? "Loading…"
            return false
        }
        opening = true
        defer { opening = false }
        do {
            editorSize = try session.openEditor(slot: slot, parent: parent)
            editorProblem = nil
            openOwed = false
            return true
        } catch {
            editorSize = nil
            editorProblem = AlertItem(error, title: "").message
            if case .Busy = error as? VizError { openOwed = true } else { openOwed = false }
            return false
        }
    }

    /// The window was resized by the user: the editor follows (or keeps
    /// its own size).
    public func resize(width: UInt32, height: UInt32) {
        guard editorSize?.resizable == true else { return }
        if let s = try? session.resizeEditor(slot: slot, width: width, height: height) {
            editorSize = s
        }
    }

    public func opened() {
        try? session.setWindowOpen(slot: slot, open: true)
    }

    /// The editor could not detach yet (`Busy`: another editor is opening);
    /// the owner keeps the editor's parent view alive and calls
    /// `retryClose()` each tick.
    public private(set) var closePending = false

    /// Parameters whose slider is mid-drag.
    private var dragging: [UInt64] = []

    /// The window closes: a drag still open ends (one undo step), the editor
    /// detaches (ending any gesture it left open) and the project no longer
    /// reopens the window. Returns false when the editor could not detach
    /// yet (`Busy`): the parent view must then stay alive until
    /// `retryClose()` returns true.
    @discardableResult
    public func closed() -> Bool {
        endOpenDrags()
        try? session.setWindowOpen(slot: slot, open: false)
        openOwed = false
        let done = detachEditor()
        editorSize = nil
        if done { try? session.endPluginGestures(slot: slot) }
        return done
    }

    /// Retries a close that hit `Busy`; true once the editor is detached
    /// (or the failure is not retryable).
    @discardableResult
    public func retryClose() -> Bool {
        guard closePending else { return true }
        let done = detachEditor()
        if done { try? session.endPluginGestures(slot: slot) }
        return done
    }

    private func detachEditor() -> Bool {
        do {
            try session.closeEditor(slot: slot)
        } catch {
            if case .Busy = error as? VizError {
                closePending = true
                return false
            }
        }
        closePending = false
        return true
    }

    /// Commits every slider still mid-drag (the window closing, the slot
    /// going away): the undo step must not be lost.
    public func endOpenDrags() {
        for id in dragging { commitParam(id) }
        dragging = []
    }

    // MARK: Intents

    public func toggleBypass() {
        guard let r = row else { return }
        do {
            document.apply(try session.setBypass(slot: slot, bypass: !r.bypass))
        } catch {
            document.alert = AlertItem(error, title: "Bypass Failed")
        }
        refresh()
    }

    /// A parameter control moves: heard now, no undo step yet.
    public func setParam(_ id: UInt64, _ value: Double) {
        try? session.setParam(slot: slot, param: id, value: value)
        if !dragging.contains(id) { dragging.append(id) }
        if let i = params.firstIndex(where: { $0.id == id }) { params[i].value = value }
    }

    /// The control is let go: one undo step with its final value.
    public func commitParam(_ id: UInt64) {
        dragging.removeAll { $0 == id }
        guard let p = params.first(where: { $0.id == id }) else { return }
        do {
            document.apply(try session.commitParams(slot: slot, values: [ParamValue(id: id, value: p.value)]))
        } catch {
            document.alert = AlertItem(error, title: "Parameter Failed")
        }
    }
}
