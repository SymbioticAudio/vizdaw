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
    /// The parameter a gesture in this window last touched (its sliders,
    /// or the plugin's own editor as the core reports it): the header's
    /// Learn button learns it (plan 5f-1 Task 11).
    public private(set) var lastTouched: (slot: UInt64, param: UInt64)?

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
                if let r = s.slots.first(where: { $0.id == slot }) {
                    found = r
                    name = s.name
                }
            }
            row = found
            stripName = name
            // The instance went (its plugin was turned off): the host
            // closed its editor. The window shows the placeholder and
            // opens the editor of the next instance once it runs.
            if found != nil && !isLive && editorSize != nil {
                editorSize = nil
            }
        } catch {
            ok = false
        }
        let slotGone = ok && row == nil
        var present: Set<UInt64>?
        if let p = try? session.params(slot: slot) {
            params = p
            // A slider mid-drag keeps showing where it is.
            for (id, v) in live {
                if let i = params.firstIndex(where: { $0.id == id }) { params[i].value = v }
            }
            present = Set(p.map(\.id))
        } else {
            ok = false
        }
        forgetGone(slotGone: slotGone, present: present)
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

    /// Parameters whose slider is mid-drag and touched in the core, in the
    /// order they were touched.
    private var dragging: [UInt64] = []
    /// The plain value shown for each dragged parameter.
    private var live: [UInt64: Double] = [:]
    /// Parameters whose drag the core refused at the touch: nothing more is
    /// sent for them until the drag ends.
    private var refused: Set<UInt64> = []
    /// Parameters whose slider's editing span is open (mouse down to
    /// mouse up), touched or not: a value outside one is a click.
    private var editing: Set<UInt64> = []

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

    /// Releases every slider still mid-drag (the window closing), each
    /// once: the core lands the step and no gesture is left open.
    public func endOpenDrags() {
        refused = []
        editing = []
        guard !dragging.isEmpty else { return }
        let open = dragging
        dragging = []
        live = [:]
        for id in open { send(id, .release) }
        gestureLanded()
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

    /// The control's editing span begins (the slider's mouse-down).
    public func beginParam(_ id: UInt64) {
        editing.insert(id)
        touched(id)
    }

    private func touched(_ id: UInt64) {
        if lastTouched?.param != id { lastTouched = (slot, id) }
    }

    /// The plugin's own editor touched a parameter (the core lists it as
    /// touched): it becomes the one Learn learns.
    public func followTouched(_ targets: [AutomationTargetRow]) {
        for t in targets {
            if case .pluginParam(let s, let p) = t, s == slot {
                touched(p)
                return
            }
        }
    }

    /// Learns a MIDI control for the last touched parameter.
    public func learnLastTouched() {
        guard let t = lastTouched else { return }
        document.learn.learn(.pluginParam(slot: t.slot, param: t.param))
    }

    /// A parameter control moves (`value` plain, in the parameter's
    /// range): the first move of a drag touches the parameter at its
    /// normalised value, later ones move it (plan 5e-2 Task 5). The core
    /// sends each value to the plugin and keeps the step at the release
    /// (plan 5e-1 Ruling 15). A refused touch alerts and is not kept.
    ///
    /// A value with no editing span open (a keyboard or accessibility
    /// increment sends no editing-began) is a click: touch, value and
    /// release at once, so no touch is left open (final review m6).
    public func setParam(_ id: UInt64, _ value: Double) {
        guard !refused.contains(id), let i = params.firstIndex(where: { $0.id == id }) else { return }
        touched(id)
        let p = params[i]
        let n = Self.normalised(value, p)
        if !editing.contains(id) && !dragging.contains(id) {
            click(id, n)
            params[i].value = Self.clamped(value, p)
            return
        }
        if dragging.contains(id) {
            // A refused move keeps the touch: its release still follows.
            send(id, .value(value: n))
        } else {
            guard send(id, .touch(value: n)) else {
                refused.insert(id)
                return
            }
            dragging.append(id)
        }
        let shown = Self.clamped(value, p)
        live[id] = shown
        params[i].value = shown
    }

    /// The control is let go: the core closes the gesture (stopped, one
    /// undo step "Change Plugin Parameter") and the window shows it.
    public func commitParam(_ id: UInt64) {
        editing.remove(id)
        if refused.remove(id) != nil { return }
        guard let i = dragging.firstIndex(of: id) else { return }
        dragging.remove(at: i)
        live[id] = nil
        send(id, .release)
        gestureLanded()
    }

    /// Ruling 8: `(v − min) / (max − min)` clamped to 0–1; 0 for a range
    /// with `max <= min` (or not a number), and never NaN.
    static func normalised(_ v: Double, _ p: ParamRow) -> Double {
        guard p.max > p.min, !v.isNaN else { return 0 }
        let n = (v - p.min) / (p.max - p.min)
        return n.isNaN ? 0 : Swift.min(Swift.max(n, 0), 1)
    }

    /// `v` as the slider shows it: inside the range, never NaN.
    static func clamped(_ v: Double, _ p: ParamRow) -> Double {
        guard p.max > p.min, !v.isNaN else { return p.min }
        return Swift.min(Swift.max(v, p.min), p.max)
    }

    // MARK: Plumbing

    /// Sends one gesture phase for parameter `id`; false (with an alert)
    /// when the core refuses.
    @discardableResult
    private func send(_ id: UInt64, _ phase: GesturePhaseRow) -> Bool {
        do {
            try session.gesture(target: .pluginParam(slot: slot, param: id), phase: phase)
            return true
        } catch {
            let item = AlertItem(error, title: "Parameter Failed")
            if let a = document.alert, a.title == item.title, a.message == item.message { return false }
            document.alert = item
            return false
        }
    }

    /// Touch, value and release of parameter `id` at normalised `n`; a
    /// refused touch alerts and leaves nothing open.
    private func click(_ id: UInt64, _ n: Double) {
        do {
            try MixerModel.clickGesture(session, .pluginParam(slot: slot, param: id), n)
        } catch {
            let item = AlertItem(error, title: "Parameter Failed")
            if let a = document.alert, a.title == item.title, a.message == item.message { return }
            document.alert = item
            return
        }
        gestureLanded()
    }

    /// A gesture closed: the document shows its step now (idle would
    /// report it a tick later). The slider keeps its last value until the
    /// window's next refresh reads the kept one.
    private func gestureLanded() {
        if let s = try? session.snapshot() { document.apply(s) }
    }

    /// Releases (once, quietly: the control went) the drags whose slot or
    /// parameter a successful read no longer has, and drops their refused
    /// marks: a mark lives only for its drag.
    private func forgetGone(slotGone: Bool, present: Set<UInt64>?) {
        func gone(_ id: UInt64) -> Bool { slotGone || present.map { !$0.contains(id) } == true }
        refused = refused.filter { !gone($0) }
        editing = editing.filter { !gone($0) }
        for id in dragging where gone(id) {
            dragging.removeAll { $0 == id }
            live[id] = nil
            try? session.gesture(target: .pluginParam(slot: slot, param: id), phase: .release)
        }
    }
}
