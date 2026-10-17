import Testing
import VizDAWCore
@testable import VizDAWAppModel

@MainActor
@Suite struct ModifyReadTests {
    @Test func modifyReadsHappenOncePerSnapshotVersion() throws {
        let f = editorFake()
        f.lastModifyRow = .move(delta: 5)
        let m = try DocumentModel(session: f)
        #expect(m.editor.lastModify() == .move(delta: 5))
        #expect(m.editor.lastModify() == .move(delta: 5))
        #expect(f.modifyReads == 1)
        f.lastModifyRow = .delete
        m.editor.edit(.event(edit: .delete(events: [1])))
        #expect(m.editor.lastModify() == .delete)
        #expect(f.modifyReads == 2)
    }

    @Test func aFailedReadKeepsTheLastValues() throws {
        let f = editorFake()
        f.modifyTemplateRows = [
            ModifyTemplateRow(name: "Up", rule: nil, mode: .select, modify: .transposeInterval(semitones: 12), toCopy: false)
        ]
        let m = try DocumentModel(session: f)
        #expect(m.editor.modifyTemplates().map(\.name) == ["Up"])
        f.modifyReadError = .Busy(message: "busy")
        m.editor.edit(.event(edit: .delete(events: [1])))
        #expect(m.editor.modifyTemplates().map(\.name) == ["Up"])
        #expect(m.editor.status == nil, "reads stay silent")
    }

    @Test func laneNameAndListCellsDoNotClearEachOther() throws {
        let m = try DocumentModel(session: demoFake())
        m.editor.beginTextEditing("laneName")
        m.editor.beginTextEditing("list")
        m.editor.endTextEditing("laneName")
        #expect(m.editor.textEditing, "the list cell still has the keyboard")
        #expect(!m.transportKeysEnabled)
        m.editor.endTextEditing("list")
        #expect(!m.editor.textEditing)
        #expect(m.transportKeysEnabled)
    }

    @Test func transportKeysAreOffWhileThePanelIsKey() throws {
        let m = try DocumentModel(session: demoFake())
        m.editor.panelKey = true
        #expect(!m.transportKeysEnabled)
        #expect(!m.returnKeyEnabled)
        m.editor.panelKey = false
        #expect(m.transportKeysEnabled)
    }

    @Test func returnIsOffWhileAListHasFocusButSpaceIsNot() throws {
        let m = try DocumentModel(session: demoFake())
        #expect(m.returnKeyEnabled)
        m.editor.listFocused = true
        #expect(!m.returnKeyEnabled)
        #expect(m.transportKeysEnabled, "Space still plays")
    }
}
