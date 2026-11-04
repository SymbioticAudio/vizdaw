import Foundation
import Testing
import VizDAWCore
@testable import VizDAWAppModel

/// The View menu's commands, their keys and `ui.*` on the channel, and
/// `state.get layout` (plan 7a Task 6; Rulings 3, 4 and 10).

fileprivate extension ControlArg {
    static func text(_ n: String, _ v: String) -> ControlArg { ControlArg(name: n, value: .text(value: v)) }
    static func flag(_ n: String, _ v: Bool) -> ControlArg { ControlArg(name: n, value: .flag(value: v)) }
    static func int(_ n: String, _ v: Int64) -> ControlArg { ControlArg(name: n, value: .int(value: v)) }
}

fileprivate extension ControlOutcome {
    var isDone: Bool {
        if case .done = self { return true }
        return false
    }
    var status: String? {
        if case .done(.ran(_, _, _, _, let status)) = self { return status }
        return nil
    }
    var code: ControlErrorCode? {
        if case .failed(let f) = self { return f.code }
        return nil
    }
    var message: String? {
        if case .failed(let f) = self { return f.message }
        return nil
    }
}

/// A store whose demo is `laneOnStrip10()`, and its one document.
@MainActor private func storeDocument() -> (doc: (id: UUID, model: DocumentModel), store: DocumentStore) {
    let s = DocumentStore(makeEmpty: { FakeSession([]) }, makeDemo: { laneOnStrip10() }, open: { _ in demoFake() })
    let id = s.newDemo()!
    return ((id, s.model(id)!), s)
}

@MainActor private func run(
    _ id: String, _ args: [ControlArg], _ doc: (id: UUID, model: DocumentModel), _ store: DocumentStore
) -> ControlOutcome {
    ControlCommands.run(id, args: args, doc: doc, store: store, host: FakeControlHost())
}

@MainActor private func selectFirstLane(_ m: DocumentModel) {
    m.editor.setSelection(SelectionRow(timeRange: nil, lanes: [LaneKey(m.selectedSeq!, m.lanes[0].id).ref], events: []))
}

@MainActor
@Suite struct ViewCommandsTests {
    @Test func viewKeysSwitchTheMainView() {
        let (doc, store) = storeDocument()
        #expect(run("ui.view_mix", [], doc, store).isDone)
        #expect(doc.model.editor.layout.views.main == .mix)
        #expect(ViewCommands.isOn("ui.view_mix", doc.model))
        _ = run("ui.view_arrange", [], doc, store)
        #expect(doc.model.editor.layout.views.main == .arrange)
        _ = ViewCommands.run("ui.view_mix", on: true, doc.model)
        _ = ViewCommands.run("ui.view_mix", on: false, doc.model)  // the checked menu item chosen again
        #expect(doc.model.editor.layout.views.main == .mix)
    }
    @Test func paneTogglesShowAndHide() {
        let (doc, store) = storeDocument()
        _ = run("ui.toggle_right", [], doc, store)
        #expect(doc.model.editor.layout.views.arrange.right.shown)
        _ = run("ui.toggle_right", [.flag("on", true)], doc, store)
        #expect(doc.model.editor.layout.views.arrange.right.shown)
        _ = run("ui.toggle_left", [.flag("on", false)], doc, store)
        #expect(!doc.model.editor.layout.views.arrange.left.shown)
    }
    @Test func uiPaneSetsViewShownAndSize() {
        let (doc, store) = storeDocument()
        let r = run("ui.pane", [.text("pane", "left"), .text("view", "sequences"), .int("size", 9_000)], doc, store)
        #expect(r.status == "left sequences 480 shown")
        #expect(doc.model.editor.status == nil)  // the answer carries it, not the status line
        _ = run("ui.pane", [.text("pane", "bottom"), .text("view", "list")], doc, store)
        #expect(doc.model.editor.layout.views.arrange.bottom.view == .editor)
        #expect(doc.model.editor.layout.lastKind == .list)
        _ = run(
            "ui.pane", [.text("pane", "right"), .text("view", "markers"), .flag("shown", true), .text("main", "mix")],
            doc, store)
        #expect(doc.model.editor.layout.views.main == .arrange)
        #expect(doc.model.editor.layout.views.mix.right == PaneState(shown: true, view: .markers, size: 260))
        _ = run("ui.pane", [.text("pane", "bottom"), .text("view", "drum_grid"), .flag("shown", false)], doc, store)
        #expect(!doc.model.editor.layout.views.arrange.bottom.shown && doc.model.editor.layout.lastKind == .drumGrid)
        _ = run("ui.pane", [.text("pane", "bottom"), .text("view", "list"), .text("main", "mix")], doc, store)
        #expect(doc.model.editor.layout.views.mix.bottom.shown && !doc.model.editor.layout.views.arrange.bottom.shown)
    }
    @Test func uiCommandsCheckTheirArguments() {
        let (doc, store) = storeDocument()
        let before = doc.model.editor.layout
        for args: [ControlArg] in [
            [.text("pane", "left")],
            [.text("pane", "left"), .text("view", "mixer")],
            [.text("pane", "right"), .text("view", "piano_roll")],
            [.text("pane", "bottom"), .text("view", "mixer"), .text("main", "mix")],
            [.text("pane", "middle"), .flag("shown", true)],
            [.text("pane", "left"), .text("main", "edit"), .flag("shown", true)],
        ] {
            #expect(run("ui.pane", args, doc, store).code == .invalidParams, "\(args)")
        }
        #expect(
            run("ui.pane", [.text("pane", "right"), .text("view", "piano_roll")], doc, store).message
                == "piano_roll is not on the right pane's list in the arrange view.")
        #expect(doc.model.editor.layout == before)  // no detail opened, lastKind and bottom.shown unchanged
        #expect(
            ControlCommands.run("ui.view_mix", args: [], doc: nil, store: store, host: FakeControlHost()).code
                == .noDocument)
    }
    @Test func stateGetLayoutReportsBothViewsAndTheLeftStrip() {
        let (doc, store) = storeDocument()  // laneOnStrip10(): lane 30 plays through strip 10
        selectFirstLane(doc.model)
        let strips = [stripRow(10, "Out 1", kind: .audioPath(path: 7))]
        let row = ViewCommands.layoutRow(doc.model, strips: strips, destinations: [])
        #expect(row.main == "arrange")
        #expect(row.views.map(\.main) == ["arrange", "mix"])
        #expect(row.views[0].panes.map(\.pane) == ["left", "bottom", "right"])
        #expect(row.views[0].panes[1].view == "piano_roll")
        #expect(row.views[1].panes[1].views == ["piano_roll", "list", "strip_chart", "drum_grid"])
        #expect(row.leftStrip == 10 && row.leftShows == "strip")
        _ = run("ui.toggle_left", [.flag("on", false)], doc, store)
        let hidden = ViewCommands.layoutRow(doc.model, strips: strips, destinations: [])
        #expect(hidden.leftShows == "hidden" && hidden.leftStrip == 10)
    }
    @Test func switchingAViewEndsTextEdits() throws {
        let (doc, store) = storeDocument()
        let m = doc.model
        let mixer = try #require(store.mixer(doc.id))
        m.beginRename(try #require(m.selectedSeq))
        m.editor.beginTextEditing("laneName")
        mixer.beginRename(try #require(mixer.strips.first), in: .strip)
        #expect(m.textFieldHasKeyboard)
        _ = run("ui.toggle_left", [], doc, store)
        #expect(m.renamingSeq == nil && !m.editor.textEditing && mixer.renamingStrip == nil)
        #expect(!m.textFieldHasKeyboard)
    }
    /// Task 6 review M1: the pane picker's `choose` refused by `setPane`
    /// (an editor kind in the right pane) opens no detail, shows no bottom
    /// pane, keeps `lastKind` and leaves an open rename alone.
    @Test func aRefusedChooseLeavesTheLayoutUntouched() throws {
        let (doc, _) = storeDocument()
        let m = doc.model
        _ = m.editor.setPane(.bottom, view: nil, shown: false, size: nil)
        m.beginRename(try #require(m.selectedSeq))
        let kept = m.editor.layout
        let refusal = ViewCommands.choose(.right, .editor, kind: .pianoRoll, shown: nil, size: nil, in: nil, m)
        #expect(refusal == "editor is not on the right pane's list in the arrange view.")
        #expect(m.editor.layout == kept)
        #expect(m.editor.layout.detail == nil && !m.editor.layout.views.arrange.bottom.shown)
        #expect(m.renamingSeq != nil, "a refused choice ends no edit")
        #expect(ViewCommands.choose(.bottom, .editor, kind: .list, shown: nil, size: nil, in: nil, m) == nil)
        #expect(m.editor.layout.views.arrange.bottom.shown && m.editor.layout.lastKind == .list)
    }
    @Test func mainViewKeysDoNotCollideWithTheEditorKeys() {
        let keys = KeyBindings(defaults: MemoryDefaults())
        #expect(
            keys.row("ui.view_arrange")?.menuKey == "1" && keys.row("ui.view_arrange")?.menuModifiers.control == true)
        #expect(keys.row("editors.piano_roll")?.menuModifiers.command == true)
        #expect(keys.row("editors.piano_roll")?.menuModifiers.control == false)
    }
    /// Task 4 review: stale controllers keys alone are reason enough to prune.
    @Test func pruningDropsControllersKeysOfGoneLanes() throws {
        let m = try DocumentModel(session: editorFake())  // sequence 3 holds lanes 30 and 31
        let live = EditorLayout.controllersKey(3, 30)
        m.editor.updateLayout {
            $0.detail = nil
            $0.windows = []
            $0.controllers = [
                live: [.velocity], EditorLayout.controllersKey(3, 99): [.velocity],
                EditorLayout.controllersKey(77, 30): [.velocity], "not a key": [.velocity],
            ]
        }
        m.editor.pruneLayout(m.snapshot)
        #expect(Array(m.editor.layout.controllers.keys) == [live])
        // Nothing stale: no layout change at all.
        let drawn = m.editor.drawVersion
        let kept = m.editor.layout
        m.editor.pruneLayout(m.snapshot)
        #expect(m.editor.layout == kept && m.editor.drawVersion == drawn)
    }
}
