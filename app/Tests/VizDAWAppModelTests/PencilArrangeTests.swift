import AppKit
import Testing
import VizDAWCore
@testable import VizDAWAppModel
@testable import VizDAWEditors

/// A pencil click on a track in Arrange opens its piano roll at that bar.
@MainActor
@Suite struct PencilArrangeTests {
    private static let bar2: Int64 = 61_440

    private func make() throws -> (FakeSession, DocumentModel) {
        let f = rollFake()
        f.laneTable[3] = (f.laneTable[3] ?? []) + [lane(90, "Vocals", kind: .audio), lane(91, "Synth", kind: .instrument)]
        return (f, try DocumentModel(session: f))
    }

    @Test func aMidiTrackOpensItsPianoRollAtTheClickedBarWithThePencil() throws {
        let (_, m) = try make()
        #expect(m.editor.pencilOpensPianoRoll(lane: LaneKey(3, 31), at: Self.bar2 + 5_000))
        let d = try #require(m.editor.layout.detail)
        #expect(d.kind == .pianoRoll && d.target == .lane(seq: 3, lane: 31))
        #expect(m.editor.tool == .pencil)
        #expect(m.editor.selection.lanes == [LaneRef(seq: 3, lane: 31)])
        #expect(m.editor.pianoRollReveal?.tick == Self.bar2)
    }

    @Test func theRollShowsThatBarAtItsLeftEdgeAndAgainAfterScrollingAway() throws {
        let (_, m) = try make()
        m.editor.pencilOpensPianoRoll(lane: LaneKey(3, 91), at: Self.bar2 + 1)
        let v = PianoRollView(model: PianoRollModel.pane(m))
        v.frame = NSRect(x: 0, y: 0, width: 800, height: 400)
        v.layout()
        let shown = v.axis.visibleRange(width: 800)
        #expect(shown.contains(Self.bar2))
        #expect(abs(v.axis.originTick - Double(Self.bar2)) <= 4 * v.axis.ticksPerPoint)
        v.axis = v.axis.scrolled(toTick: 0)
        m.editor.pencilOpensPianoRoll(lane: LaneKey(3, 91), at: Self.bar2 + 1)
        v.layout()
        #expect(abs(v.axis.originTick - Double(Self.bar2)) <= 4 * v.axis.ticksPerPoint, "a new request scrolls again")
    }

    @Test func barOneStaysAtTheStartAndNegativeTicksClamp() throws {
        let (_, m) = try make()
        #expect(m.editor.pencilOpensPianoRoll(lane: LaneKey(3, 31), at: -50))
        #expect(m.editor.pianoRollReveal?.tick == 0)
    }

    @Test func anAudioTrackOrAnUnknownOneDoesNothing() throws {
        let (f, m) = try make()
        #expect(!m.editor.pencilOpensPianoRoll(lane: LaneKey(3, 90), at: Self.bar2))
        #expect(!m.editor.pencilOpensPianoRoll(lane: LaneKey(3, 12_345), at: Self.bar2))
        #expect(!m.editor.pencilOpensPianoRoll(lane: LaneKey(999, 1), at: Self.bar2))
        #expect(m.editor.layout.detail == nil && m.editor.pianoRollReveal == nil)
        #expect(m.editor.tool == .arrow && m.editor.selection.lanes.isEmpty && f.edits.isEmpty)
    }

    @Test func openingAnotherEditorDropsTheRequest() throws {
        let (_, m) = try make()
        m.editor.pencilOpensPianoRoll(lane: LaneKey(3, 31), at: Self.bar2)
        m.editor.openDetail(.lane(seq: 3, lane: 31), kind: .list)
        #expect(m.editor.pianoRollReveal == nil)
    }

    @Test func aPencilClickInTheArrangementViewOpensTheRoll() throws {
        let f = editorFake()
        let m = try DocumentModel(session: f)
        let v = ArrangementView(document: m)
        v.frame = NSRect(x: 0, y: 0, width: 800, height: 300)
        v.layout()
        // Arrow: a click selects, opens nothing.
        v.contentMouseDown(CGPoint(x: 50, y: 70), clicks: 1, modifiers: [])
        v.contentMouseUp(CGPoint(x: 50, y: 70), modifiers: [])
        #expect(m.editor.layout.detail == nil)
        m.editor.tool = .pencil
        // Past the block, in the empty part of the Chords row (x 700 is tick 268 800).
        v.contentMouseDown(CGPoint(x: 700, y: 70), clicks: 1, modifiers: [])
        v.contentMouseUp(CGPoint(x: 700, y: 70), modifiers: [])
        #expect(m.editor.layout.detail?.kind == .pianoRoll)
        #expect(m.editor.layout.detail?.target == .lane(seq: 3, lane: 31))
        #expect(m.editor.pianoRollReveal?.tick == 245_760, "bar 5 starts at 4 x 61 440")
    }
}
