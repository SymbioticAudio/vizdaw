import Foundation
import Testing
import VizDAWCore
@testable import VizDAWAppModel

@MainActor
@Suite struct SidebarTests {
    @Test func keySlotsUseAppleModifierOrder() {
        #expect(KeySlotFormat.text(KeySlotRow(key: "a", modifiers: 15)) == "⌃⌥⇧⌘A")
        #expect(KeySlotFormat.text(KeySlotRow(key: "F5", modifiers: 0)) == "F5")
        #expect(KeySlotFormat.text(nil) == "")
    }

    @Test func reorderingASectionStoresTheOrder() throws {
        let m = try DocumentModel(session: editorFake())
        m.moveSequences(section: m.referenced, from: IndexSet(integer: 1), to: 0)
        #expect(m.referenced.map(\.id) == [2, 1])
        #expect(m.editor.layout.sequenceOrder == [2, 1, 3])
        #expect(m.topLevel.map(\.id) == [3])
    }

    @Test func duplicateIsACommand() throws {
        let f = editorFake()
        let m = try DocumentModel(session: f)
        m.duplicate(1)
        #expect(f.edits == [.sequence(edit: .duplicate(seq: 1))])
    }

    @Test func aNewSequenceFromATemplateIsSelected() throws {
        let f = editorFake()
        let m = try DocumentModel(session: f)
        nextSnapshot(f) { $0.sequences.append(seq(4, "Sequence 1", topLevel: true)) }
        m.newSequence(.drumKit)
        #expect(f.templates.map(\.name) == ["Sequence 1"])
        #expect(f.templates.map(\.template) == [.drumKit])
        #expect(m.selectedSeq == 4)
    }

    @Test func newNamesSkipTakenOnes() throws {
        let f = editorFake()
        f.snap.sequences.append(seq(4, "Sequence 1", topLevel: true))
        let m = try DocumentModel(session: f)
        #expect(m.uniqueName("Sequence") == "Sequence 2")
    }

    @Test func commentsAndSlotsAreCommands() throws {
        let f = editorFake()
        let m = try DocumentModel(session: f)
        m.beginComment(3)
        m.commentDraft?.text = "verse idea"
        m.commitComment()
        m.beginSlot(3)
        m.slotDraft?.key = "a"
        m.slotDraft?.option = true
        m.commitSlot()
        m.beginSlot(3)
        m.slotDraft?.key = "  "
        m.commitSlot()
        #expect(
            f.edits == [
                .sequence(edit: .setComment(seq: 3, comment: "verse idea")),
                .sequence(edit: .setKeyboardSlot(seq: 3, slot: KeySlotRow(key: "a", modifiers: 4))),
                .sequence(edit: .setKeyboardSlot(seq: 3, slot: nil)),
            ])
        #expect(m.commentDraft == nil && m.slotDraft == nil)
    }

    @Test func deletingAUsedSequenceAsksFirst() throws {
        let f = editorFake()
        let m = try DocumentModel(session: f)
        m.requestDelete(1)
        #expect(m.pendingDelete == 1)
        #expect(f.edits.isEmpty)
        m.confirmDelete()
        #expect(f.edits == [.sequence(edit: .delete(seq: 1, deleteReferences: true))])
        m.requestDelete(3)
        #expect(f.edits.last == .sequence(edit: .delete(seq: 3, deleteReferences: false)))
        #expect(m.pendingDelete == nil)
    }
}
