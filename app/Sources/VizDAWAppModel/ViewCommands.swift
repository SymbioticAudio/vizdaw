import Foundation
import VizDAWCore

/// The View menu's main views and panes (plan 7a Rulings 4 and 10): one
/// implementation for the menu items, the keys and the control channel's
/// `ui.*` commands. None opens a dialog.
@MainActor
public enum ViewCommands {
    /// `ui.pane`, keys and channel only: it needs its arguments.
    public static let pane = "ui.pane"
    static let mainViews: [String: MainView] = ["ui.view_arrange": .arrange, "ui.view_mix": .mix]
    static let toggles: [String: PaneSide] = [
        "ui.toggle_left": .left, "ui.toggle_bottom": .bottom, "ui.toggle_right": .right,
    ]

    public static let ids: Set<String> = Set(mainViews.keys).union(toggles.keys).union([pane])

    /// Always: the views and panes can change at any time.
    public static func enabled(_ id: String, _ m: DocumentModel) -> Bool { true }

    /// A view item's check mark is the current main view; a toggle's is
    /// whether the current view shows that pane.
    public static func isOn(_ id: String, _ m: DocumentModel) -> Bool {
        let v = m.editor.layout.views
        if let main = mainViews[id] { return v.main == main }
        if let side = toggles[id] { return v.current[side].shown }
        return false
    }

    /// Runs `id` on `m`. A view item always shows its view, whatever `on`
    /// says (a checked item chosen again passes false, Ruling 4); a toggle
    /// shows (`on` true), hides (false) or flips (nil) its pane.
    @discardableResult
    public static func run(_ id: String, on: Bool?, _ m: DocumentModel) -> KeyOutcome {
        let v = m.editor.layout.views
        if let main = mainViews[id] {
            guard v.main != main else { return .done }
            m.endPaneEdits()
            m.editor.showMainView(main)
            return .done
        }
        if let side = toggles[id] {
            let shown = on ?? !v.current[side].shown
            guard shown != v.current[side].shown else { return .done }
            m.endPaneEdits()
            m.editor.setPaneShown(side, shown)
            return .done
        }
        if id == pane { return .unavailable("Set Pane needs `pane` and a `view`, `shown` or `size`.") }
        return .unavailable("VizDAW has no handler for \(id).")
    }

    /// The one path that changes a pane once its arguments are checked
    /// (`runPane` and the pane's picker): text edits end, the pane is set,
    /// then an editor kind opens the detail (or is remembered with no
    /// target), and a given `shown` wins over the detail's showing the
    /// bottom pane. A view off the pane's list is refused before anything
    /// changes (Task 6 review M1): the refusal is returned, else nil.
    @discardableResult
    public static func choose(
        _ side: PaneSide, _ view: PaneView, kind: DetailEditorKind?, shown: Bool?, size: Double?,
        in main: MainView?, _ m: DocumentModel
    ) -> String? {
        let e = m.editor
        let setup = main ?? e.layout.views.main
        guard ViewLayouts.views(side, in: setup).contains(view) else {
            return "\(view.rawValue) is not on the \(side.rawValue) pane's list in the \(setup.rawValue) view."
        }
        m.endPaneEdits()
        if let refused = e.setPane(side, view: view, shown: shown, size: size, in: main) { return refused }
        guard let kind else { return nil }
        if let t = e.layout.detail?.target ?? e.detailTarget(shown: m.selectedSeq) {
            e.openDetail(t, kind: kind, in: main)
        } else {
            e.updateLayout { $0.lastKind = kind }
        }
        if let shown { e.setPaneShown(.bottom, shown, in: main) }
        return nil
    }

    /// `ui.pane`: every argument is checked before anything changes
    /// (Ruling 10); the answer's status names what was set.
    public static func runPane(_ args: [ControlArg], _ m: DocumentModel, doc: UUID) -> ControlOutcome {
        func invalid(_ message: String) -> ControlOutcome { .failed(.of(.invalidParams, message)) }
        guard let side = args.text("pane").flatMap(PaneSide.init(rawValue:)) else {
            return invalid("ui.pane: `pane` is left, bottom or right")
        }
        var main = m.editor.layout.views.main
        if let name = args.text("main") {
            guard let named = MainView(rawValue: name) else { return invalid("ui.pane: `main` is arrange or mix") }
            main = named
        }
        let sent = args.text("view")
        let shown = args.flag("shown")
        let size = args.int("size").map(Double.init)
        guard sent != nil || shown != nil || size != nil else {
            return invalid("ui.pane: give `view`, `shown` or `size`")
        }
        var kind: DetailEditorKind?
        var view: PaneView?
        if let sent {
            kind = DetailEditorKind.allCases.first { ControlStates.kindName($0) == sent }
            view = kind != nil ? .editor : PaneView(rawValue: sent)
            guard let view, ViewLayouts.views(side, in: main).contains(view) else {
                return invalid("\(sent) is not on the \(side.rawValue) pane's list in the \(main.rawValue) view.")
            }
        }
        choose(side, view ?? m.editor.layout.views[main][side].view, kind: kind, shown: shown, size: size, in: main, m)
        let l = m.editor.layout
        let p = l.views[main][side]
        let text = "\(side.rawValue) \(viewName(side, p, l)) \(Int(p.size)) \(p.shown ? "shown" : "hidden")"
        return .done(
            .ran(command: pane, doc: doc.uuidString, window: nil, undoLabel: m.snapshot.undoLabel, status: text))
    }

    /// `state.get layout` (Ruling 10's shape).
    public static func layoutRow(_ m: DocumentModel, strips: [StripRow], destinations: [DestRow]) -> LayoutStateRow {
        let l = m.editor.layout
        let rows = MainView.allCases.map { main in
            ViewPanesRow(
                main: main.rawValue,
                panes: PaneSide.allCases.map { side in
                    let p = l.views[main][side]
                    return PaneRow(
                        pane: side.rawValue, shown: p.shown, view: viewName(side, p, l), size: p.size,
                        views: ViewLayouts.views(side, in: main).flatMap { v in
                            v == .editor ? DetailEditorKind.allCases.map(ControlStates.kindName) : [v.rawValue]
                        })
                })
        }
        // The rule's strip whatever the pane shows; what it shows is the current view's left pane.
        let rule = m.leftStrip(strips: strips, destinations: destinations)
        var leftStrip: UInt64?
        if case .strip(let id) = rule { leftStrip = id }
        let left = l.views.current.left
        let shows: String
        if !left.shown {
            shows = "hidden"
        } else if left.view == .strip {
            switch rule {
            case .strip: shows = "strip"
            case .inspector: shows = "inspector"
            case .nothing: shows = "nothing"
            }
        } else {
            shows = left.view.rawValue
        }
        return LayoutStateRow(main: l.views.main.rawValue, views: rows, leftStrip: leftStrip, leftShows: shows)
    }

    /// A pane's view as the channel names it: the bottom pane's editor is
    /// its kind (`piano_roll` …).
    public static func viewName(_ side: PaneSide, _ state: PaneState, _ layout: EditorLayout) -> String {
        guard state.view == .editor else { return state.view.rawValue }
        return ControlStates.kindName(layout.detail?.kind ?? layout.lastKind)
    }
}

extension DocumentModel {
    /// Ends the text edits of what the panes and the centre show, before
    /// they change (Ruling 10): a sequence rename is committed, the lane
    /// name and drum row fields end, and the store's mixer's strip rename
    /// is committed. Fields of sheets and popovers stay open.
    public func endPaneEdits() {
        if renamingSeq != nil { commitRename(renameDraft) }
        editor.endTextEditing("laneName")
        editor.endTextEditing("drumRow")
        storeMixer?.commitRename()
    }
}
