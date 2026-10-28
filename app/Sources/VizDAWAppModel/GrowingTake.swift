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
        // Checked throughout (5d-2 final review I2): the core saturates the
        // pass's file frame at `Int64.max`, and a trap here is a crash
        // mid-recording; a value out of range draws no take.
        guard let startOff = Self.frames((secs[1] - secs[0]) * hz) else { return [] }
        var punchOff: Int64?
        if secs.count == 3 {
            guard let p = Self.frames((secs[2] - secs[0]) * hz) else { return [] }
            punchOff = p
        }
        var out: [GrowingTake] = []
        for lane in s.lanes where lane.audio {
            // File frame `anchor` is heard at `passTick` (plan 5d-1 Task 10 review I1); signed.
            guard let anchor = Self.add(s.passStartFileFrame, lane.returnFrames),
                let wanted = Self.add(anchor, max(0, startOff))
            else { continue }
            let from = max(0, wanted)
            var to = written
            if let p = punchOff {
                guard let end = Self.add(anchor, p) else { continue }
                to = min(to, end)
            }
            guard to > from, let fromOff = Self.sub(from, anchor), let toOff = Self.sub(to, anchor) else {
                continue
            }
            guard
                let t = try? m.session.ticksAt(
                    seq: seq, seconds: [secs[0] + Double(fromOff) / hz, secs[0] + Double(toOff) / hz]),
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

    private static func add(_ a: Int64, _ b: Int64) -> Int64? {
        let (r, o) = a.addingReportingOverflow(b)
        return o ? nil : r
    }

    private static func sub(_ a: Int64, _ b: Int64) -> Int64? {
        let (r, o) = a.subtractingReportingOverflow(b)
        return o ? nil : r
    }

    /// `x` rounded to whole frames, or nil when it is not finite or out of range.
    private static func frames(_ x: Double) -> Int64? {
        Int64(exactly: x.rounded())
    }
}
