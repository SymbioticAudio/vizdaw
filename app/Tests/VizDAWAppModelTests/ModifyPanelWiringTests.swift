import Foundation
import Testing
import VizDAWCore
@testable import VizDAWAppModel

/// What the app's select-and-modify panel window and the Modify menu ask
/// of the models (Plan 4c Ruling 22): the panel's key/close hooks, its
/// text-field owners, the Apply target line and Modify Again's state.
@MainActor
@Suite struct ModifyPanelWiringTests {
    func store() -> DocumentStore {
        DocumentStore(makeEmpty: { rollFake() }, makeDemo: { rollFake() }, open: { _ in rollFake() })
    }

    private func dragRequest() -> ModifyRequest {
        ModifyRequest(task: .move, config: .preset(.move), events: [1], lanes: [LaneKey(3, 31)], range: nil)
    }

    @Test func aKeyPanelTargetsCommandsAndResigningGivesThemBack() throws {
        let s = store()
        let a = try #require(s.newEmpty())
        let b = try #require(s.newEmpty())
        let m = try #require(s.model(a))
        s.panelBecameKey(a)
        #expect(s.commandTarget(focused: b) == a && m.editor.panelKey && !m.transportKeysEnabled)
        s.panelResignedKey(a)
        #expect(s.commandTarget(focused: b) == b && !m.editor.panelKey && m.transportKeysEnabled)
    }

    @Test func closingThePanelClearsFocusKeyAndTextOwners() throws {
        let s = store()
        let a = try #require(s.newEmpty())
        let m = try #require(s.model(a))
        m.selectModify.open(.transpose)
        s.panelBecameKey(a)
        m.selectModify.setTextEditing("m0", true)
        m.selectModify.setTextEditing("m1", true)
        m.selectModify.setTextEditing("m1", false)
        #expect(m.editor.textEditors == [SelectModifyModel.textOwner("m0")])
        m.editor.beginTextEditing("list")
        s.panelClosed(a)
        #expect(s.panelFocus == nil && !m.editor.panelKey && !m.selectModify.isOpen)
        #expect(m.editor.textEditors == ["list"], "only the panel's owners go")
    }

    @Test func resigningKeyGivesTheKeyboardBackToTheTransport() throws {
        let s = store()
        let a = try #require(s.newEmpty())
        let m = try #require(s.model(a))
        s.panelBecameKey(a)
        m.editor.beginTextEditing(SelectModifyModel.textOwner("c0.1"))
        s.panelResignedKey(a)
        #expect(!m.editor.textEditing && m.transportKeysEnabled)
    }

    @Test func anotherDocumentsPanelResigningKeepsTheFocus() throws {
        let s = store()
        let a = try #require(s.newEmpty())
        let b = try #require(s.newEmpty())
        s.panelBecameKey(a)
        s.panelResignedKey(b)
        s.panelClosed(b)
        #expect(s.panelFocus == a)
    }

    @Test func closingTheDocumentLeavesNoPanelFocus() throws {
        let s = store()
        let a = try #require(s.newEmpty())
        var closing: [UUID] = []
        s.willClose = { id in
            closing.append(id)
            s.panelClosed(id)
        }
        let m = try #require(s.model(a))
        let p = m.selectModify
        p.open(.change)
        p.setTextEditing("m0", true)
        s.panelBecameKey(a)
        #expect(p.isOpen && m.editor.textEditing)
        s.close(a)
        #expect(closing == [a] && s.panelFocus == nil)
        // Only panelClosed closes the panel model and drops its text owners.
        #expect(!p.isOpen && !p.isKey)
        #expect(!m.editor.textEditing && m.editor.textEditors.isEmpty)
    }

    @Test func theApplyTargetFollowsTheDraggedRequest() throws {
        let f = rollFake()
        let m = try DocumentModel(session: f)
        let p = m.selectModify
        p.open(.change)
        #expect(!p.actsOnDraggedEvents && p.applyTarget == SelectModifyModel.selectionTarget)
        p.open(dragRequest())
        #expect(p.actsOnDraggedEvents && p.applyTarget == SelectModifyModel.draggedEventsTarget)
        #expect(p.apply())
        #expect(!p.actsOnDraggedEvents && p.applyTarget == SelectModifyModel.selectionTarget)
        p.open(dragRequest())
        m.editor.setSelection(SelectionRow(timeRange: nil, lanes: [LaneRef(seq: 3, lane: 31)], events: [2]))
        #expect(!p.actsOnDraggedEvents, "a moved-on selection drops the request")
    }

    @Test func modifyAgainNamesTheLastModifyAndWaitsForAnOpenDrag() throws {
        let f = rollFake()
        let m = try DocumentModel(session: f)
        let p = m.selectModify
        #expect(!p.canModifyAgain && p.modifyAgainTitle == "Modify Again")
        f.lastModifyRow = .move(delta: 5)
        m.editor.edit(.event(edit: .delete(events: [1])))
        #expect(p.canModifyAgain && p.modifyAgainTitle == "Modify Again: Move")
        #expect(m.editor.beginGesture("Drag", lanes: [LaneKey(3, 31)], origin: .other))
        #expect(!p.canModifyAgain)
    }

    @Test func textOwnersAreUniquePerField() {
        #expect(SelectModifyModel.textOwner("m0") != SelectModifyModel.textOwner("m1"))
        #expect(SelectModifyModel.textOwner("m0").hasPrefix(SelectModifyModel.textOwnerPrefix))
    }
}
