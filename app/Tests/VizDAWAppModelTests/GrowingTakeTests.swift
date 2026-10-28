import AppKit
import Testing
import VizDAWCore
@testable import VizDAWAppModel
@testable import VizDAWEditors

/// The take that grows while it is written (plan 5d-2 Task 9; spec §6.2;
/// 5c Ruling 23): where it sits, which file frames fill it, and the
/// arrangement drawing it from the writer's live peaks. The fake's tempo
/// maps 30 720 ticks to a second; the project is 48 kHz.
@MainActor
@Suite struct GrowingTakeTests {
    /// Song recording: the pass started at 1 s (file frame 48 000), the
    /// record range at 2 s; 4 s of file written; Groove (30) records
    /// audio, Chords (31) MIDI.
    func recording(punchEnd: Int64? = nil) throws -> (DocumentModel, FakeSession) {
        let f = editorFake()
        let m = try DocumentModel(session: f)
        m.refreshProjectInfo()
        f.recordRow.lanes = [
            RecordLaneRow(lane: 30, audio: true, path: "Media/Recorded/G.caf", clipped: 0),
            RecordLaneRow(lane: 31, audio: false, path: nil, clipped: 0),
        ]
        var spec = m.record.spec()
        if let punchEnd { spec.punch = TickRange(start: 61_440, end: punchEnd) }
        try f.record(seq: 3, spec: spec)
        f.recordRow.passStartTick = 30_720
        f.recordRow.passStartFileFrame = 48_000
        f.recordRow.rangeStartTick = 61_440
        f.recordRow.frames = 4 * 48_000
        m.record.refresh()
        return (m, f)
    }

    @Test func theTakeRunsFromTheRangeStartToWhatIsWritten() throws {
        let (m, _) = try recording()
        #expect(
            m.record.growingTakes() == [
                GrowingTake(lane: 30, startTick: 61_440, endTick: 122_880, fromFrame: 96_000, toFrame: 192_000)
            ])
    }

    @Test func thePunchOutEndsIt() throws {
        let (m, _) = try recording(punchEnd: 92_160)
        #expect(
            m.record.growingTakes() == [
                GrowingTake(lane: 30, startTick: 61_440, endTick: 92_160, fromFrame: 96_000, toFrame: 144_000)
            ])
    }

    @Test func nothingGrowsBeforeTheRollOrInAnotherSequence() throws {
        let (m, f) = try recording()
        f.recordRow.passStartTick = nil
        m.record.refresh()
        #expect(m.record.growingTakes().isEmpty)
        f.recordRow.passStartTick = 30_720
        f.recordRow.frames = 96_000
        m.record.refresh()
        #expect(m.record.growingTakes().isEmpty, "nothing written inside the range yet")
        f.recordRow.frames = 192_000
        m.record.refresh()
        m.select(2)
        #expect(m.record.growingTakes().isEmpty)
    }

    /// A synth return lane's file runs one return latency behind (plan
    /// 5d-1 Task 10 review I1): it is drawn where Stop places it.
    @Test func aReturnLaneIsPlacedByItsReturnLatency() throws {
        let (m, f) = try recording()
        f.recordRow.lanes[0] = RecordLaneRow(
            lane: 30, audio: true, path: "Media/Recorded/G.caf", clipped: 0, returnFrames: 4_800)
        m.record.refresh()
        #expect(
            m.record.growingTakes() == [
                GrowingTake(lane: 30, startTick: 61_440, endTick: 119_808, fromFrame: 100_800, toFrame: 192_000)
            ])
    }

    /// A large positive latency trim can put the pass's anchor before the
    /// file's start: the take starts at the file's first frame, never traps.
    @Test func aNegativeAnchorStartsLater() throws {
        let (m, f) = try recording()
        f.recordRow.passStartFileFrame = -96_000
        f.recordRow.rangeStartTick = 30_720
        m.record.refresh()
        #expect(
            m.record.growingTakes() == [
                GrowingTake(lane: 30, startTick: 92_160, endTick: 215_040, fromFrame: 0, toFrame: 192_000)
            ])
    }

    @Test func theArrangementDrawsItFromTheLivePeaks() throws {
        let (m, f) = try recording()
        let v = ArrangementView(document: m)
        v.frame = NSRect(x: 0, y: 0, width: 800, height: 300)
        v.layout()
        let image = NSImage(size: NSSize(width: 800, height: 300))
        image.lockFocus()
        v.drawContent(in: CGRect(x: 0, y: 0, width: 800, height: 300))
        image.unlockFocus()
        let call = try #require(f.recordingWaveformCalls.last)
        #expect(call.lane == 30 && call.from == 96_000 && call.to == 192_000 && call.columns > 0)
    }
}
