import CoreGraphics
import Foundation
import VizDAWAppModel
import VizDAWCore

/// The pencil (Plan 4b Ruling 7).
public enum Pencil {
    /// MIDI 1.0 velocity 100 as a 16-bit velocity.
    public static let velocity: UInt16 = 100 << 9

    /// One grid step at `tick`; a quarter note with snap off.
    public static func step(at tick: Int64, snap: Snap, meter: [MeterEntryRow]) -> Int64 {
        guard snap.enabled else { return MeterMath.ppq }
        return snap.step(in: MeterMath.bar(at: tick, meter).meter)
    }

    /// A new note's start: the grid line at or before `tick`.
    public static func start(_ tick: Int64, snap: Snap, meter: [MeterEntryRow]) -> Int64 {
        snap.floor(tick, meter: meter)
    }

    /// The length painted from `start` to the pointer at `tick`: snapped and
    /// at least one step; with snap off the raw length, at least a 32nd.
    public static func length(start: Int64, to tick: Int64, snap: Snap, meter: [MeterEntryRow]) -> Int64 {
        let start = NoteDrag.clampTick(start)
        let tick = NoteDrag.clampTick(tick)
        let step = step(at: start, snap: snap, meter: meter)
        guard snap.enabled else { return tick > start ? max(tick - start, MeterMath.ppq / 8) : MeterMath.ppq / 8 }
        guard tick > start else { return step }
        return max(snap.apply(tick, meter: meter) - start, step)
    }

    public static func note(tick: Int64, pitch: UInt8, length: Int64) -> EventRow {
        EventRow(
            id: 0, tick: tick, kind: .note, mute: false, dest: nil, groupChannel: nil, pitch: pitch,
            velocity: velocity, releaseVelocity: 0, duration: length, value: 0, value2: 0,
            attribute: nil, perNote: [], refSeq: nil, refLength: nil, refLoops: 0, refTranspose: 0,
            media: nil, sourceOffset: 0, frames: 0, gainDb: 0,
            fadeIn: FadeRow(length: 0, shape: .linear), fadeOut: FadeRow(length: 0, shape: .linear),
            text: nil, textKind: nil, data: Data(), eightBit: false)
    }

    public static func command(lane: LaneKey, tick: Int64, pitch: UInt8, length: Int64) -> EditCommand {
        .event(edit: .insert(seq: lane.seq, lane: lane.lane, events: [note(tick: tick, pitch: pitch, length: length)]))
    }
}

/// The eraser (Plan 4b Ruling 8).
public enum Eraser {
    /// `erased` plus the note `hit` landed on, each once, in order.
    public static func adding(_ hit: PianoRollHit, to erased: [UInt64]) -> [UInt64] {
        guard case .note(let n) = hit, !erased.contains(n.row.id) else { return erased }
        return erased + [n.row.id]
    }

    public static func command(_ erased: [UInt64]) -> EditCommand? {
        erased.isEmpty ? nil : .event(edit: .delete(events: erased))
    }
}

public enum RubberBand {
    public static func rect(from a: CGPoint, to b: CGPoint) -> CGRect {
        CGRect(x: min(a.x, b.x), y: min(a.y, b.y), width: abs(b.x - a.x), height: abs(b.y - a.y))
    }
}

/// The pointer's shape (Plan 4b Ruling 20); the view maps it to `NSCursor`.
public enum CursorKind: Equatable, Sendable {
    case arrow, openHand, upDown, resizeRight, crosshair, eraser
}

public enum PianoRollCursor {
    public static func kind(for hit: PianoRollHit, tool: Tool) -> CursorKind {
        if tool == .eraser { return .eraser }
        switch hit {
        case .note(let n):
            switch n.zone {
            case .start: return .openHand
            case .middle: return .upDown
            case .end: return .resizeRight
            }
        case .empty:
            return tool == .pencil ? .crosshair : .arrow
        }
    }
}
