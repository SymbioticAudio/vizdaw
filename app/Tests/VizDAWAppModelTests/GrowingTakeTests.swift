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

    /// The core saturates the pass's file frame at `Int64.max` (5d-2 final
    /// review I2): an anchor past the frame range draws no take, never traps.
    @Test func aSaturatedFileFrameDrawsNothingAndNeverTraps() throws {
        for punch in [nil, Int64(92_160)] {
            let (m, f) = try recording(punchEnd: punch)
            f.recordRow.passStartFileFrame = .max - 1
            f.recordRow.lanes[0] = RecordLaneRow(
                lane: 30, audio: true, path: "Media/Recorded/G.caf", clipped: 0, returnFrames: 4_800)
            m.record.refresh()
            #expect(m.record.growingTakes().isEmpty)
            f.recordRow.passStartFileFrame = .min + 1
            f.recordRow.lanes[0] = RecordLaneRow(
                lane: 30, audio: true, path: "Media/Recorded/G.caf", clipped: 0, returnFrames: -4_800)
            m.record.refresh()
            #expect(m.record.growingTakes().isEmpty)
        }
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
        let calls = f.recordingWaveformCalls
        #expect(!calls.isEmpty && calls.allSatisfy { $0.lane == 30 && $0.columns > 0 })
        #expect(calls.map(\.from).min() == 96_000 && calls.map(\.to).max() == 192_000, "the slices cover the take")
    }

    /// A four-bar take over a tempo change: 120 BPM for two bars (4 s), 60 after (8 s).
    @Test func aGrowingTakeFollowsATempoChange() throws {
        let (m, f) = try recording()
        f.secondsAtMap = { ticks in
            ticks.map { $0 <= 122_880 ? Double($0) / 30_720 : 4 + Double($0 - 122_880) / 15_360 }
        }
        let take = GrowingTake(lane: 30, startTick: 0, endTick: 245_760, fromFrame: 0, toFrame: 576_000)
        let rect = CGRect(x: 0, y: 0, width: 512, height: 40)
        let slices = m.record.growingSlices(take, rect: rect, visible: rect) { Int64(($0 * 480).rounded()) }
        #expect(!slices.isEmpty && slices.count <= 32)
        #expect(slices.count == 8, "eight 64-point pieces")
        let mid = try #require(slices.first { $0.minX == 256 })
        #expect(mid.from == 192_000, "not 288 000, as a linear map gives")
        #expect(slices.first?.from == 0 && slices.last?.to == 576_000)
        for (a, b) in zip(slices, slices.dropFirst()) { #expect(a.to == b.from && a.maxX == b.minX) }
        #expect(slices.reduce(0) { $0 + Int($1.columns) } <= 8192)
        let far = CGRect(x: 0, y: 0, width: 100_000, height: 40)
        let many = m.record.growingSlices(take, rect: far, visible: far) { Int64($0) }
        #expect(many.count <= 32 && many.reduce(0) { $0 + Int($1.columns) } <= 8192)
    }

    /// 5d-2 T9 M3: only the visible slice's frames are asked for, at most
    /// 8192 columns.
    @Test func theVisibleSliceAsksForItsFramesAndAtMost8192Columns() {
        let rect = CGRect(x: 0, y: 0, width: 1000, height: 40)
        let all = GrowingTake.slice(rect: rect, visible: rect, from: 0, to: 48_000)
        #expect(all?.from == 0 && all?.to == 48_000 && all?.columns == 1000)
        let half = GrowingTake.slice(rect: rect, visible: CGRect(x: 500, y: 0, width: 600, height: 40), from: 0, to: 48_000)
        #expect(half?.from == 24_000 && half?.to == 48_000 && half?.columns == 500)
        let wide = CGRect(x: 0, y: 0, width: 20_000, height: 40)
        #expect(GrowingTake.slice(rect: wide, visible: wide, from: 0, to: 960_000)?.columns == 8192)
        #expect(GrowingTake.slice(rect: rect, visible: CGRect(x: 2000, y: 0, width: 10, height: 40), from: 0, to: 48_000) == nil)
        #expect(GrowingTake.slice(rect: rect, visible: rect, from: 5, to: 5) == nil)
    }
}
