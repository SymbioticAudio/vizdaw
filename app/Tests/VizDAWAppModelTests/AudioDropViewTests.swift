import AppKit
import Testing
import VizDAWCore
@testable import VizDAWAppModel
@testable import VizDAWEditors

/// Dropping files on the editors (plan 5b Task 11, Ruling 8). Song's
/// rows: Groove (30) at y 0–48, Chords (31) at 48–96; 384 ticks a point.
@MainActor
@Suite struct AudioDropViewTests {
    @Test func theArrangementImportsIntoTheLaneUnderTheDrop() throws {
        let f = editorFake()
        let m = try DocumentModel(session: f)
        let v = ArrangementView(document: m)
        v.frame = NSRect(x: 0, y: 0, width: 800, height: 300)
        v.layout()
        #expect(v.drop(["/a.wav"], at: CGPoint(x: 81, y: 70)))
        #expect(f.imports.last?.0 == ["/a.wav"])
        #expect(f.imports.last?.1 == ImportTargetRow(seq: 3, lane: 31, tick: 30_720))
        // Below the last lane: new lanes.
        #expect(v.drop(["/a.wav", "/b.wav"], at: CGPoint(x: 81, y: 200)))
        #expect(f.imports.last?.1 == ImportTargetRow(seq: 3, lane: nil, tick: 30_720))
        #expect(!v.drop([], at: CGPoint(x: 0, y: 0)))
        #expect(f.imports.count == 2)
    }

    @Test func theRegionStripImportsIntoTheRegionsLaneOrTheEditLane() throws {
        let f = audioFake()
        let m = try DocumentModel(session: f)
        m.editor.openDetail(.sequence(3), kind: .pianoRoll)
        let v = RegionStripView(model: m.pianoRoll(.pane))
        v.frame = NSRect(x: 0, y: 0, width: 800, height: 60)
        v.timeAxis = TimeAxis(ticksPerPoint: 96)
        let side = PianoRollContainer.sideWidth
        // On the Loop region (lane 33): into its lane.
        #expect(v.drop(["/a.wav"], at: CGPoint(x: side + 160, y: 11)))
        #expect(f.imports.last?.1 == ImportTargetRow(seq: 3, lane: 33, tick: 15_360))
        // Past the region: the piano roll's edit lane.
        #expect(v.drop(["/a.wav"], at: CGPoint(x: side + 700, y: 11)))
        let edit = try #require(m.pianoRoll(.pane).editLane)
        #expect(f.imports.last?.1?.lane == edit.lane)
    }
}
