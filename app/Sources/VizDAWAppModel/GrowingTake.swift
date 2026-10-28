import Foundation
import VizDAWCore

/// A take being written, where the arrangement draws it (plan 5d-2 Task
/// 9; plan 5d-1 Ruling 21): its span in the sequence and the file frames
/// that fill it.
public struct GrowingTake: Equatable, Sendable {
    public let lane: UInt64
    public let startTick: Int64
    public let endTick: Int64
    public let fromFrame: UInt64
    public let toFrame: UInt64
}

extension RecordModel {
    /// The shown sequence's armed audio lanes' takes while a pass records
    /// (spec §6.2: "the region draws as it grows"): from the later of the
    /// pass's start and the record range's start, to what the writer has
    /// (at most the punch-out). Each lane is placed where Stop will place
    /// it (plan 5d-1 Task 10 review I1): file frame `passStartFileFrame +
    /// lane.returnFrames` (signed: a large latency trim can put it before
    /// the file's start) is heard at `passStartTick`, and a take whose
    /// wanted start is before the file's first frame starts at that frame,
    /// later. Ticks and seconds map through the sequence's tempo; a Locate
    /// during the pass is not followed.
    public func growingTakes() -> [GrowingTake] {
        guard isRecording, let m = document, let s = state, let seq = s.seq, seq == m.selectedSeq,
            let passTick = s.passStartTick, let rate = m.projectInfo?.format.sampleRate, rate > 0
        else { return [] }
        let start = max(passTick, s.rangeStartTick)
        var ticks = [passTick, start]
        if let punch = s.spec?.punch { ticks.append(punch.end) }
        guard let secs = try? m.session.secondsAt(seq: seq, ticks: ticks), secs.count == ticks.count else { return [] }
        let hz = Double(rate)
        let written = Int64(clamping: s.frames)
        let startOff = Int64(((secs[1] - secs[0]) * hz).rounded())
        let punchOff = secs.count == 3 ? Int64(((secs[2] - secs[0]) * hz).rounded()) : nil
        var out: [GrowingTake] = []
        for lane in s.lanes where lane.audio {
            // File frame `anchor` is heard at `passTick` (plan 5d-1 Task 10 review I1); signed.
            let anchor = s.passStartFileFrame + lane.returnFrames
            let wanted = anchor + max(0, startOff)
            let from = max(0, wanted)
            var to = written
            if let p = punchOff { to = min(to, anchor + p) }
            guard to > from else { continue }
            guard
                let t = try? m.session.ticksAt(
                    seq: seq, seconds: [secs[0] + Double(from - anchor) / hz, secs[0] + Double(to - anchor) / hz]),
                t.count == 2
            else { continue }
            let startTick = from == wanted ? start : t[0]
            guard t[1] > startTick else { continue }
            out.append(
                GrowingTake(
                    lane: lane.lane, startTick: startTick, endTick: t[1], fromFrame: UInt64(from), toFrame: UInt64(to)))
        }
        return out
    }
}
