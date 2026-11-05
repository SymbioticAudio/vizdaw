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

/// One piece of a growing take's visible part: its span in points and the
/// file frames and columns that fill it.
public struct GrowingSlice: Equatable, Sendable {
    public var minX, maxX: CGFloat
    public var from, to: UInt64
    public var columns: UInt32
}

extension GrowingTake {
    /// The width of one piece of a growing take, in points.
    public static let pieceWidth: CGFloat = 64
    /// The most pieces a take is cut into.
    public static let maxPieces = 32
    /// The core's column limit for one request, shared by the pieces.
    static let maxColumns = 8192

    /// The frames to ask the writer's file for, and the columns to draw
    /// them in, for the part of a take's `rect` that is `visible` (at most
    /// 8192 columns); nil when nothing is visible or the frames are empty.
    public nonisolated static func slice(
        rect: CGRect, visible: CGRect, from: UInt64, to: UInt64
    ) -> (from: UInt64, to: UInt64, columns: UInt32)? {
        let vis = rect.intersection(visible)
        guard !vis.isNull, vis.width > 0, rect.width > 0, to > from else { return nil }
        let span = Double(to - from)
        let f0 = from + UInt64(span * Double((vis.minX - rect.minX) / rect.width))
        let f1 = from + UInt64(span * Double((vis.maxX - rect.minX) / rect.width))
        guard f1 > f0 else { return nil }
        let columns = UInt32(clamping: min(Int(vis.width.rounded(.up)), 8192))
        return columns > 0 ? (f0, f1, columns) : nil
    }
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

    /// The visible part of a growing take's `rect`, cut into pieces of
    /// `GrowingTake.pieceWidth` points (at most 32, widened when needed), each
    /// with the file frames that sound under it. Every piece edge's tick
    /// (`tickAt` gives the tick at an x) goes through the sequence's tempo
    /// map in one `secondsAt` call, so a tempo change inside the take bends
    /// the waveform to where it is heard: frame `fromFrame +
    /// round((secs(tick) − secs(startTick)) × rate)`, held within `fromFrame…toFrame`.
    /// The columns of all pieces total at most 8192.
    public func growingSlices(
        _ t: GrowingTake, rect: CGRect, visible: CGRect, tickAt: (CGFloat) -> Int64
    ) -> [GrowingSlice] {
        let vis = rect.intersection(visible)
        guard !vis.isNull, vis.width > 0, vis.width.isFinite, t.toFrame > t.fromFrame,
            let m = document, let seq = m.selectedSeq, let rate = m.projectInfo?.format.sampleRate, rate > 0
        else { return [] }
        let count = max(1, min(GrowingTake.maxPieces, Int((vis.width / GrowingTake.pieceWidth).rounded(.up))))
        let step = vis.width / CGFloat(count)
        let xs = (0...count).map { i in i == count ? vis.maxX : vis.minX + CGFloat(i) * step }
        let ticks = [t.startTick] + xs.map(tickAt)
        guard let secs = try? m.session.secondsAt(seq: seq, ticks: ticks), secs.count == ticks.count else { return [] }
        let hz = Double(rate)
        let span = Double(t.toFrame - t.fromFrame)
        var frames: [UInt64] = []
        for s in secs.dropFirst() {
            let off = ((s - secs[0]) * hz).rounded()
            guard off.isFinite else { return [] }
            let clamped = min(max(off, 0), span)
            // Never behind the previous edge, so the pieces stay contiguous.
            frames.append(max(t.fromFrame + UInt64(clamped), frames.last ?? 0))
        }
        let perPiece = UInt32(max(1, GrowingTake.maxColumns / count))
        var out: [GrowingSlice] = []
        for i in 0..<count where frames[i + 1] > frames[i] {
            let cols = UInt32(clamping: Int((xs[i + 1] - xs[i]).rounded(.up)))
            out.append(
                GrowingSlice(
                    minX: xs[i], maxX: xs[i + 1], from: frames[i], to: frames[i + 1],
                    columns: max(1, min(cols, perPiece))))
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
