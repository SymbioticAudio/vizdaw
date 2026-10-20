import AppKit
import Testing
import VizDAWCore
@testable import VizDAWAppModel
@testable import VizDAWEditors

/// Song's Chords lane (row 1, y 48–96) holds one region instead of a note:
/// 2 s (96 000 frames) over bar 1, x 0–160 at the default zoom.
@MainActor
private func regionArrangement(gain: Float = 0, offline: OfflineReasonRow? = nil) throws
    -> (DocumentModel, ArrangementView, FakeSession)
{
    let f = editorFake()
    f.laneEventTable["3/31"] = LaneEvents(
        seq: 3, lane: 31, rev: 2, events: [regionRow(1, tick: 0, duration: 61_440, frames: 96_000, gainDb: gain)])
    f.mediaRows = [mediaRow(7, "loop", offline: offline)]
    let m = try DocumentModel(session: f)
    let v = ArrangementView(document: m)
    v.frame = NSRect(x: 0, y: 0, width: 800, height: 300)
    v.layout()
    f.calls = []
    return (m, v, f)
}

/// The region's rectangle: 51–93 in y (a 3-point inset in the 48-point row).
private let top: CGFloat = 51

@MainActor
@Suite struct RegionHandleViewTests {
    @Test func draggingTheFadeInCornerIsOneFadeInStep() throws {
        let (m, v, f) = try regionArrangement()
        v.contentMouseDown(CGPoint(x: 1, y: top + 2), clicks: 1, modifiers: [])
        v.contentMouseDragged(CGPoint(x: 41, y: top + 2), modifiers: [])
        v.contentMouseUp(CGPoint(x: 41, y: top + 2), modifiers: [])
        #expect(f.calls == ["begin Fade In", "gesture", "end"])
        #expect(
            f.gestureEdits.last
                == .audio(
                    edit: .setFades(
                        event: 1, fadeIn: FadeRow(length: 24_000, shape: .linear),
                        fadeOut: FadeRow(length: 0, shape: .linear))))
        #expect(m.editor.gesture == nil)
    }

    @Test func draggingTheTopEdgeChangesTheGain() throws {
        let (_, v, f) = try regionArrangement()
        v.contentMouseDown(CGPoint(x: 80, y: top + 1), clicks: 1, modifiers: [])
        v.contentMouseDragged(CGPoint(x: 80, y: top - 19), modifiers: [])
        v.contentMouseUp(CGPoint(x: 80, y: top - 19), modifiers: [])
        #expect(f.calls == ["begin Region Gain", "gesture", "end"])
        #expect(f.gestureEdits.last == .audio(edit: .setGain(event: 1, gainDb: 5)))
    }

    @Test func optionClickOnTheTopEdgeResetsTheGain() throws {
        let (_, v, f) = try regionArrangement(gain: 3)
        v.contentMouseDown(CGPoint(x: 80, y: top + 1), clicks: 1, modifiers: [.option])
        v.contentMouseUp(CGPoint(x: 80, y: top + 1), modifiers: [.option])
        #expect(f.edits.last == .audio(edit: .setGain(event: 1, gainDb: 0)))
    }

    @Test func theBodyStillMovesTheBlock() throws {
        let (_, v, f) = try regionArrangement()
        v.contentMouseDown(CGPoint(x: 80, y: 75), clicks: 1, modifiers: [])
        v.contentMouseDragged(CGPoint(x: 300, y: 75), modifiers: [])
        v.contentMouseUp(CGPoint(x: 300, y: 75), modifiers: [])
        #expect(f.calls == ["begin Move Block", "gesture", "end"])
    }

    @Test func drawingAsksForTheVisibleWaveform() throws {
        let (_, v, f) = try regionArrangement()
        let rep = try #require(v.content.bitmapImageRepForCachingDisplay(in: v.content.bounds))
        v.content.cacheDisplay(in: v.content.bounds, to: rep)
        let call = try #require(f.waveformCalls.last)
        #expect(call.event == 1)
        #expect(call.columns >= 159 && call.columns <= 161, "one column per point")
        #expect(call.from <= 0 && call.to >= 61_000)
    }

    @Test func offlineRegionsAreNotRead() throws {
        let (_, v, f) = try regionArrangement(offline: .missing)
        let rep = try #require(v.content.bitmapImageRepForCachingDisplay(in: v.content.bounds))
        v.content.cacheDisplay(in: v.content.bounds, to: rep)
        #expect(f.waveformCalls.isEmpty)
        let labels = v.accessibilityItems().map(\.label)
        #expect(labels.contains("Region loop on Chords, bar 1, gain 0.0 dB, offline"))
    }

    @Test func regionsAreAccessibilityElementsThatSelect() throws {
        let (m, v, _) = try regionArrangement(gain: 3)
        let item = try #require(v.accessibilityItems().first { $0.label.hasPrefix("Region loop") })
        #expect(item.label == "Region loop on Chords, bar 1, gain +3.0 dB")
        item.press?()
        #expect(m.editor.selection.events == [1])
    }

    @Test func theRegionStripHasHandlesToo() throws {
        let f = audioFake()
        f.laneEventTable["3/33"] = LaneEvents(
            seq: 3, lane: 33, rev: 4, events: [regionRow(8, tick: 0, duration: 61_440, frames: 96_000)])
        let m = try DocumentModel(session: f)
        m.editor.openDetail(.sequence(3), kind: .pianoRoll)
        let v = RegionStripView(model: m.pianoRoll(.pane))
        v.frame = NSRect(x: 0, y: 0, width: 800, height: 60)
        v.timeAxis = TimeAxis(ticksPerPoint: 96)
        let side = PianoRollContainer.sideWidth
        f.calls = []
        v.pressed(at: CGPoint(x: side + 300, y: 3), clicks: 1, modifiers: [])
        v.dragged(to: CGPoint(x: side + 300, y: -17), modifiers: [])
        v.released(at: CGPoint(x: side + 300, y: -17), modifiers: [])
        #expect(f.calls == ["begin Region Gain", "gesture", "end"])
        #expect(f.gestureEdits.last == .audio(edit: .setGain(event: 8, gainDb: 5)))
        // The body still moves the region.
        f.calls = []
        v.pressed(at: CGPoint(x: side + 50, y: 11), clicks: 1, modifiers: [])
        v.dragged(to: CGPoint(x: side + 150, y: 11), modifiers: [])
        v.released(at: CGPoint(x: side + 150, y: 11), modifiers: [])
        #expect(f.calls.first == "begin Move Region")
    }

    /// Preflight M8: the strip's handles follow the tool, as the
    /// arrangement's do.
    @Test func theRegionStripsHandlesNeedTheArrow() throws {
        let f = audioFake()
        f.laneEventTable["3/33"] = LaneEvents(
            seq: 3, lane: 33, rev: 4, events: [regionRow(8, tick: 0, duration: 61_440, frames: 96_000)])
        let m = try DocumentModel(session: f)
        m.editor.openDetail(.sequence(3), kind: .pianoRoll)
        m.editor.tool = .pencil
        let v = RegionStripView(model: m.pianoRoll(.pane))
        v.frame = NSRect(x: 0, y: 0, width: 800, height: 60)
        v.timeAxis = TimeAxis(ticksPerPoint: 96)
        let side = PianoRollContainer.sideWidth
        let lines = v.lines()
        #expect(RegionStripLayout.handleHit(CGPoint(x: side + 300, y: 3), lines: lines, time: v.timeAxis, side: side) != nil)
        #expect(v.hitForPress(at: CGPoint(x: side + 300, y: 3)) == .region(lines[0].items[0]), "no handle without the arrow")
        m.editor.tool = .arrow
        if case .handle(_, .gain, _) = v.hitForPress(at: CGPoint(x: side + 300, y: 3)) {} else {
            Issue.record("the arrow grabs the gain band")
        }
    }

    /// Preflight M8: the strip's regions say their gain and whether they
    /// are offline.
    @Test func theRegionStripsRegionsSayTheirGainAndOffline() throws {
        let f = audioFake()
        f.laneEventTable["3/33"] = LaneEvents(
            seq: 3, lane: 33, rev: 4, events: [regionRow(8, tick: 0, duration: 61_440, frames: 96_000, gainDb: 3)])
        f.mediaRows = [mediaRow(7, "loop", offline: .missing)]
        let m = try DocumentModel(session: f)
        m.editor.openDetail(.sequence(3), kind: .pianoRoll)
        let v = RegionStripView(model: m.pianoRoll(.pane))
        v.frame = NSRect(x: 0, y: 0, width: 800, height: 60)
        v.timeAxis = TimeAxis(ticksPerPoint: 96)
        let labels = (v.accessibilityChildren() ?? []).compactMap { ($0 as? NSAccessibilityElement)?.accessibilityLabel() }
        #expect(labels.contains("Region loop on Loop, bar 1, gain +3.0 dB, offline"))
    }
}
