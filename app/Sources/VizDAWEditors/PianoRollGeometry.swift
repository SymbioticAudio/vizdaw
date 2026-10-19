import CoreGraphics
import VizDAWAppModel
import VizDAWCore

/// Where over a note the pointer is (Plan 4b Ruling 4).
public enum NoteZone: Equatable, Sendable {
    case start, middle, end
}

public struct NoteHit: Equatable, Sendable {
    public let lane: LaneKey
    public let row: EventRow
    public let zone: NoteZone

    public init(lane: LaneKey, row: EventRow, zone: NoteZone) {
        self.lane = lane
        self.row = row
        self.zone = zone
    }
}

public enum PianoRollHit: Equatable, Sendable {
    case note(NoteHit)
    case empty(tick: Int64, pitch: UInt8)
}

/// One shown lane's notes (kind `.note` only), in time order.
public struct LaneNotes: Equatable, Sendable {
    public let lane: LaneKey
    public let notes: [EventRow]

    public init(lane: LaneKey, notes: [EventRow]) {
        self.lane = lane
        self.notes = notes
    }
}

/// Note rectangles, zones and hit-testing as pure functions of rows and
/// axes (spec §3.5).
public enum PianoRollGeometry {
    public static let minNoteWidth: CGFloat = 3
    /// The widest a start or end zone gets.
    public static let maxEdge: CGFloat = 8

    public static func rect(_ n: EventRow, time: TimeAxis, pitch: PitchAxis) -> CGRect {
        CGRect(
            x: time.x(n.tick), y: pitch.top(n.pitch),
            width: max(time.width(ofTicks: n.duration), minNoteWidth), height: pitch.rowHeight)
    }

    /// Edge zones are `min(maxEdge, width / 3)` wide.
    public static func zone(atX x: CGFloat, in r: CGRect) -> NoteZone {
        let edge = min(maxEdge, r.width / 3)
        if x < r.minX + edge { return .start }
        if x >= r.maxX - edge { return .end }
        return .middle
    }

    /// The note under `p`: the active lane's first, then the others in
    /// order; within a lane the latest-starting one (Plan 4b Ruling 9).
    public static func hit(
        _ p: CGPoint, lanes: [LaneNotes], active: LaneKey?, time: TimeAxis, pitch: PitchAxis
    ) -> PianoRollHit {
        let target = pitch.pitch(atY: p.y)
        let ordered = lanes.filter { $0.lane == active } + lanes.filter { $0.lane != active }
        for l in ordered {
            let under = l.notes.last { n in
                n.pitch == target && rect(n, time: time, pitch: pitch).contains(p)
            }
            if let n = under {
                let r = rect(n, time: time, pitch: pitch)
                return .note(NoteHit(lane: l.lane, row: n, zone: zone(atX: p.x, in: r)))
            }
        }
        return .empty(tick: time.tick(atX: p.x), pitch: target)
    }

    /// The notes the pointer crossed moving from `a` to `b`, in the order
    /// it met them (Plan 4b Ruling 8: a fast eraser drag skips none).
    public static func notes(
        crossingFrom a: CGPoint, to b: CGPoint, lanes: [LaneNotes], time: TimeAxis, pitch: PitchAxis
    ) -> [UInt64] {
        var hits: [(t: CGFloat, order: Int, id: UInt64)] = []
        for l in lanes {
            for n in l.notes {
                if let t = entry(from: a, to: b, into: rect(n, time: time, pitch: pitch)) {
                    hits.append((t, hits.count, n.id))
                }
            }
        }
        return hits.sorted { ($0.t, $0.order) < ($1.t, $1.order) }.map(\.id)
    }

    /// Where along `a`–`b` (0…1) the segment enters `r`; nil when it misses
    /// (Liang–Barsky clipping).
    static func entry(from a: CGPoint, to b: CGPoint, into r: CGRect) -> CGFloat? {
        let dx = b.x - a.x
        let dy = b.y - a.y
        var t0: CGFloat = 0
        var t1: CGFloat = 1
        for (p, q) in [(-dx, a.x - r.minX), (dx, r.maxX - a.x), (-dy, a.y - r.minY), (dy, r.maxY - a.y)] {
            if p == 0 {
                if q < 0 { return nil }
                continue
            }
            let t = q / p
            if p < 0 {
                if t > t1 { return nil }
                t0 = max(t0, t)
            } else {
                if t < t0 { return nil }
                t1 = min(t1, t)
            }
        }
        return t0
    }

    /// The notes a rubber band `r` touches, lane by lane.
    public static func notes(
        in r: CGRect, lanes: [LaneNotes], time: TimeAxis, pitch: PitchAxis
    ) -> [(lane: LaneKey, id: UInt64)] {
        lanes.flatMap { l in
            l.notes.filter { rect($0, time: time, pitch: pitch).intersects(r) }.map { (l.lane, $0.id) }
        }
    }
}

/// A note drag (Plan 4b Rulings 4, 5): what it carries and its full
/// target for a pointer moved by `rawTicks` and `dy` from the press.
public struct NoteDrag: Equatable, Sendable {
    public let zone: NoteZone
    public let pressed: EventRow
    public let lane: LaneKey
    /// The dragged events.
    public let events: [UInt64]
    /// The lowest and highest pitch among them (transpose limits).
    public let lowest: UInt8
    public let highest: UInt8
    /// ⌥ when the drag began (start and middle zones only).
    public let copy: Bool

    public init(
        zone: NoteZone, pressed: EventRow, lane: LaneKey, events: [UInt64], lowest: UInt8, highest: UInt8,
        copy: Bool
    ) {
        self.zone = zone
        self.pressed = pressed
        self.lane = lane
        self.events = events
        self.lowest = lowest
        self.highest = highest
        self.copy = copy && zone != .end
    }

    public var label: String {
        switch zone {
        case .start: copy ? "Copy Notes" : "Move Notes"
        case .middle: copy ? "Copy Notes" : "Transpose Notes"
        case .end: "Resize Notes"
        }
    }

    /// Every selected note of the shown lanes when the pressed one is
    /// selected, the pressed one first; else the pressed one alone.
    public static func events(pressed: UInt64, selected: Set<UInt64>, lanes: [LaneNotes]) -> [UInt64] {
        guard selected.contains(pressed) else { return [pressed] }
        let others = lanes.flatMap(\.notes).map(\.id).filter { $0 != pressed && selected.contains($0) }
        return [pressed] + others
    }

    /// The time offset that puts the pressed note's start on the grid.
    public static func timeDelta(rawTicks: Int64, start: Int64, snap: Snap, meter: [MeterEntryRow]) -> Int64 {
        let start = clampTick(start)
        let raw = clampTick(rawTicks)
        guard snap.enabled else { return raw }
        return snap.apply(clampTick(start + raw), meter: meter) - start
    }

    /// Ticks held within the time axis's reach, so sums cannot overflow.
    static func clampTick(_ t: Int64) -> Int64 { min(max(t, -TimeAxis.limit), TimeAxis.limit) }

    /// Whole rows for a vertical move of `dy` points (up positive).
    public static func rawSemitones(dy: CGFloat, rowHeight: CGFloat) -> Int {
        guard dy.isFinite, rowHeight > 0 else { return 0 }
        return Int(min(max((-dy / rowHeight).rounded(), -1_000), 1_000))
    }

    /// `raw` clamped so every dragged note stays inside 0–127.
    public func semitones(raw: Int) -> Int {
        // Malformed bounds (lowest > highest, pitches above 127) still land in Int8's range.
        min(max(min(max(raw, -Int(lowest)), 127 - Int(highest)), -127), 127)
    }

    /// The duration change that puts the pressed note's end on the grid; the
    /// note keeps at least one tick.
    public func durationDelta(rawTicks: Int64, snap: Snap, meter: [MeterEntryRow]) -> Int64 {
        let tick = Self.clampTick(pressed.tick)
        let end = Self.clampTick(tick + Self.clampTick(pressed.duration))
        var newEnd = Self.clampTick(end + Self.clampTick(rawTicks))
        if snap.enabled { newEnd = snap.apply(newEnd, meter: meter) }
        newEnd = max(newEnd, tick + 1)
        return newEnd - end
    }

    /// The full target (spec §3.8: every move sends it whole); nil when
    /// nothing would change.
    public func command(
        rawTicks: Int64, dy: CGFloat, rowHeight: CGFloat, snap: Snap, meter: [MeterEntryRow]
    ) -> EditCommand? {
        switch zone {
        case .start:
            let d = Self.timeDelta(rawTicks: rawTicks, start: pressed.tick, snap: snap, meter: meter)
            guard d != 0 else { return nil }
            return copy
                ? .event(edit: .duplicate(events: events, ticks: d, semitones: 0))
                : .event(edit: .move(events: events, delta: d, toLane: nil))
        case .middle:
            let s = semitones(raw: Self.rawSemitones(dy: dy, rowHeight: rowHeight))
            guard s != 0 else { return nil }
            return copy
                ? .event(edit: .duplicate(events: events, ticks: 0, semitones: Int8(s)))
                : .event(edit: .nudge(events: events, ticks: 0, semitones: Int8(s)))
        case .end:
            let d = durationDelta(rawTicks: rawTicks, snap: snap, meter: meter)
            guard d != 0 else { return nil }
            return .event(edit: .resize(events: events, delta: d))
        }
    }

    /// The pitch the pressed note sounds at (Plan 4b Ruling 6).
    public func soundingPitch(dy: CGFloat, rowHeight: CGFloat) -> UInt8 {
        guard zone == .middle else { return pressed.pitch }
        let s = semitones(raw: Self.rawSemitones(dy: dy, rowHeight: rowHeight))
        return UInt8(min(max(Int(pressed.pitch) + s, 0), 127))
    }
}

/// VoiceOver labels (spec §3.5, Plan 4b Ruling 21).
public enum PianoRollA11y {
    public static func label(_ n: EventRow, laneName: String?, meter: [MeterEntryRow]) -> String {
        let bar = MeterMath.bar(at: n.tick, meter)
        let beatTicks = MeterMath.beatTicks(bar.meter)
        let into = n.tick - bar.start
        var s = "Note \(PitchName.name(n.pitch)), bar \(bar.number) beat \(into / beatTicks + 1)"
        if into % beatTicks != 0 { s += " tick \(into % beatTicks)" }
        s += ", velocity \(n.velocity >> 9)"
        if let laneName { s += ", \(laneName)" }
        if n.mute { s += ", muted" }
        return s
    }
}
