import Testing
import VizDAWCore
@testable import VizDAWAppModel

/// A reference row to sequence 2 (event 7).
func referenceRow(length: Int64? = nil, loops: UInt32 = 1, transpose: Int8 = 0) -> EventRow {
    var r = noteRow(7, tick: 0)
    r.kind = .reference
    r.refSeq = 2
    r.refLength = length
    r.refLoops = loops
    r.refTranspose = transpose
    r.duration = 4 * 61_440
    return r
}

@MainActor
@Suite struct ReferenceWorkflowTests {
    @Test func makingAReferenceNeedsARange() throws {
        let f = editorFake()
        let m = try DocumentModel(session: f)
        #expect(!m.editor.makeReferenceFromSelection(seq: 3))
        #expect(m.editor.status?.contains("time range") == true)
        #expect(f.edits.isEmpty)
    }

    @Test func aReferenceIsMadeFromTheSelection() throws {
        let f = editorFake()
        let m = try DocumentModel(session: f)
        m.editor.selectRange(TickRange(start: 0, end: 61_440), lanes: [chords])
        #expect(m.editor.makeReferenceFromSelection(seq: 3))
        #expect(
            f.edits == [
                .reference(edit: .fromSelection(seq: 3, perLane: false, roundToBars: true, loopCount: 1))
            ])
    }

    @Test func aBlockBecomesAReferenceBySelectingItFirst() throws {
        let f = editorFake()
        let m = try DocumentModel(session: f)
        #expect(m.editor.makeReference(from: blockRow(0, 61_440, events: [1]), in: chords))
        #expect(f.selectionSets.last?.timeRange == TickRange(start: 0, end: 61_440))
        #expect(f.selectionSets.last?.lanes == [LaneRef(seq: 3, lane: 31)])
        #expect(f.edits.count == 1)
    }

    @Test func copyFlattenAndRePointAreCommands() throws {
        let f = editorFake()
        let m = try DocumentModel(session: f)
        m.editor.makePrivateCopy(7)
        m.editor.flatten(7)
        m.editor.rePoint(7, to: 1)
        #expect(
            f.edits == [
                .reference(edit: .makePrivateCopy(event: 7)),
                .reference(edit: .flatten(event: 7)),
                .reference(edit: .rePoint(event: 7, target: 1)),
            ])
    }

    @Test func aRefusedRePointShowsWhy() throws {
        let f = editorFake()
        f.editError = .Model(message: "reference cycle: Groove uses Song")
        let m = try DocumentModel(session: f)
        m.editor.rePoint(7, to: 3)
        #expect(m.editor.status == "reference cycle: Groove uses Song")
    }

    @Test func theParametersDraftReadsAndWritesTheReference() throws {
        var d = ReferenceDraft(row: referenceRow(length: nil, loops: 0, transpose: -2))
        #expect(d.useSequenceLength && d.fill && d.transpose == -2)
        d.useSequenceLength = false
        d.lengthBeats = 8
        d.fill = false
        d.loops = 3
        d.transpose = 5
        #expect(
            d.command
                == .reference(edit: .setParams(event: 7, length: 8 * 15_360, loopCount: 3, transpose: 5)))
        let e = ReferenceDraft(row: referenceRow(length: 2 * 15_360, loops: 2))
        #expect(!e.useSequenceLength && e.lengthBeats == 2 && e.loops == 2 && !e.fill)
    }

    @Test func editingAReferenceOpensItsDraftAndCommitSends() throws {
        let f = editorFake()
        f.laneEventTable["3/30"] = LaneEvents(seq: 3, lane: 30, rev: 1, events: [referenceRow(loops: 2)])
        let m = try DocumentModel(session: f)
        m.editor.editReference(blockRow(0, 10, events: [7], ref: 2, label: "Groove"), in: grooveLane)
        #expect(m.editor.referenceDraft?.loops == 2)
        m.editor.referenceDraft?.transpose = 7
        m.editor.commitReference()
        #expect(m.editor.referenceDraft == nil)
        #expect(f.edits == [.reference(edit: .setParams(event: 7, length: nil, loopCount: 2, transpose: 7))])
    }

    @Test func rePointTargetsSkipTheCurrentOne() {
        let rows = [seq(1, "Beat", topLevel: false), seq(2, "Groove", topLevel: false), seq(3, "Song", topLevel: true)]
        #expect(EditorModel.rePointTargets(excluding: 2, among: rows).map(\.id) == [1, 3])
    }
}
