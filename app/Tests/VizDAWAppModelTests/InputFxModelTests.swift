import Foundation
import Testing
import VizDAWCore
@testable import VizDAWAppModel

/// The input effects editor's model (plan 5g-1 Task 8; spec §6.8 UI):
/// add, remove, reorder, bypass and settings, each one undoable lane edit;
/// a slider drag is one edit at its end; the strip's MIDI FX section lit state.
@MainActor
@Suite(.serialized) struct InputFxModelTests {
    /// Song's Chords lane (31) with `chain`, the editor open on it.
    func model(_ chain: [InputFxRow] = []) throws -> (DocumentModel, FakeSession) {
        let f = editorFake()
        f.laneTable[3]?[1].inputFx = chain
        let m = try DocumentModel(session: f)
        m.select(3)
        m.inputFx.open(lane: 31)
        return (m, f)
    }

    func last(_ f: FakeSession) -> [InputFxRow]? {
        if case .lane(edit: .setInputFx(seq: 3, lane: 31, let chain))? = f.edits.last { return chain }
        return nil
    }

    let arp = InputFxRow(kind: InputFxModel.defaultKind(.arpeggiator), bypass: false)
    let up12 = InputFxRow(kind: .transpose(semitones: 12), bypass: false)

    @Test func addRemoveReorderAndBypassAreOneEditEach() throws {
        var (m, f) = try model()
        m.inputFx.add(.arpeggiator)
        #expect(last(f) == [arp])
        (m, f) = try model([arp])
        m.inputFx.add(.transpose)
        #expect(last(f) == [arp, up12])
        (m, f) = try model([arp, up12])
        let before = f.edits.count
        m.inputFx.move(from: IndexSet(integer: 1), to: 0)
        #expect(f.edits.count == before + 1)
        #expect(last(f) == [up12, arp])
        (m, f) = try model([up12, arp])
        m.inputFx.setBypass(0, true)
        #expect(last(f) == [InputFxRow(kind: up12.kind, bypass: true), arp])
        (m, f) = try model([up12, arp])
        m.inputFx.remove(at: 0)
        #expect(last(f) == [arp])
        (m, f) = try model([up12])
        m.inputFx.update(0, .transpose(semitones: -7))
        #expect(last(f) == [InputFxRow(kind: .transpose(semitones: -7), bypass: false)])
    }

    @Test func aSliderDragIsOneEditAtItsEnd() throws {
        let (m, f) = try model([InputFxRow(kind: InputFxModel.defaultKind(.velocity), bypass: false)])
        let fx = m.inputFx
        fx.selected = 0
        let before = f.edits.count
        fx.beginDraft()
        for scale in [110, 120, 130, 140] as [UInt16] {
            fx.setDraft(.velocity(scale: scale, offset: 0, min: 1, max: 127, fixed: nil))
            #expect(fx.chain[0].kind == .velocity(scale: scale, offset: 0, min: 1, max: 127, fixed: nil))
        }
        #expect(f.edits.count == before, "no edit while dragging")
        fx.commitDraft()
        #expect(f.edits.count == before + 1)
        #expect(last(f)?[0].kind == .velocity(scale: 140, offset: 0, min: 1, max: 127, fixed: nil))
        fx.beginDraft()
        fx.commitDraft()
        #expect(f.edits.count == before + 1, "an unchanged drag edits nothing")
    }

    @Test func theChainIsBounded() throws {
        var (m, f) = try model(Array(repeating: up12, count: 8))
        var before = f.edits.count
        m.inputFx.add(.chord)
        #expect(f.edits.count == before)
        #expect(m.editor.status == "A chain holds at most 8 effects.")
        (m, f) = try model([InputFxRow(kind: InputFxModel.defaultKind(.inputQuantize), bypass: false)])
        before = f.edits.count
        m.inputFx.add(.inputQuantize)
        #expect(f.edits.count == before)
        #expect(m.editor.status == "A chain holds one Input Quantize.")
    }

    @Test func theSectionIsLitForAnActiveChain() {
        var l = lane(31, "Chords")
        #expect(!InputFxModel.isLit(l))
        l.inputFx = [InputFxRow(kind: .transpose(semitones: 3), bypass: true)]
        #expect(!InputFxModel.isLit(l), "all bypassed")
        l.inputFx.append(InputFxRow(kind: .chord(intervals: [4, 7]), bypass: false))
        #expect(InputFxModel.isLit(l))
        #expect(InputFxModel.showsSection(l))
        var audio = lane(40, "Vox", kind: .audio)
        audio.inputFx = l.inputFx
        #expect(!InputFxModel.showsSection(audio) && !InputFxModel.isLit(audio), "no MIDI FX section on an audio track")
    }

    /// Pre-flight I11: the editor's model follows the document. A lane an
    /// Undo removed shows no chain and takes no edit; another sequence
    /// closes it.
    @Test func theChainFollowsAnUndoneLaneAndClosesWithTheSequence() throws {
        let (m, f) = try model([up12])
        #expect(m.inputFx.chain == [up12])
        f.laneTable[3]?.removeAll { $0.id == 31 }
        var s = f.snap
        s.version += 1
        m.apply(s)
        #expect(m.inputFx.chain.isEmpty)
        let before = f.edits.count
        m.inputFx.add(.transpose)
        #expect(f.edits.count == before)
        let (m2, _) = try model([up12])
        m2.select(2)
        #expect(m2.inputFx.lane == nil)
    }

    @Test func titlesSummariesAndGrids() {
        #expect(InputFxModel.title(.inputQuantize(grid: 3840, strength: 100)) == "Input Quantize")
        #expect(InputFxModel.defaultKind(.inputQuantize) == .inputQuantize(grid: 3840, strength: 100))
        #expect(InputFxModel.summary(InputFxModel.defaultKind(.arpeggiator)) == "Up 1/16 · 1 oct · 50%")
        #expect(InputFxModel.summary(.transpose(semitones: -5)) == "\u{2212}5")
        #expect(InputFxModel.summary(.chord(intervals: [4, 7])) == "+4 +7")
        #expect(InputFxModel.quantizeGrids.count == 15)
        #expect(InputFxModel.quantizeGrids.contains { $0.title == "1/16" && $0.ticks == 3840 })
        #expect(InputFxModel.quantizeGrids.contains { $0.title == "1/8T" && $0.ticks == 5120 })
        #expect(InputFxModel.quantizeGrids.contains { $0.title == "1/4." && $0.ticks == 23_040 })
    }

    /// An Undo that removes the selected sequence moves the selection and
    /// closes the editor (review I1), as choosing another sequence does.
    @Test func anUndoneSequenceClosesTheEditor() throws {
        let (m, f) = try model([up12])
        #expect(m.inputFx.lane == 31)
        f.snap.sequences.removeAll { $0.id == 3 }
        var s = f.snap
        s.version += 1
        m.apply(s)
        #expect(m.selectedSeq != 3)
        #expect(m.inputFx.lane == nil)
    }

    @Test func aDraftBelongsToItsRow() throws {
        let (m, f) = try model([up12, arp])
        m.inputFx.selected = 0
        m.inputFx.beginDraft()
        m.inputFx.setDraft(.transpose(semitones: 5))
        let before = f.edits.count
        m.inputFx.selected = 1
        #expect(m.inputFx.draft == nil)
        #expect(m.inputFx.chain == [up12, arp])
        m.inputFx.commitDraft()
        #expect(f.edits.count == before)
    }
}
