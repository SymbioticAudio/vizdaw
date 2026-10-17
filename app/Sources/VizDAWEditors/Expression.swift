import CoreGraphics
import VizDAWAppModel
import VizDAWCore

/// Per-note expression curves and strokes (spec §4b, Plan 4b Ruling 14).
public enum ExpressionGeometry {
    public static let paneHeight: CGFloat = 96
    /// One stroke point is kept per this many points of x.
    public static let pointSpacing: CGFloat = 4

    /// A note's values of `kind` as steps over its span: `neutral` until the
    /// first point, then each value until the next point or the note's end.
    public static func steps(
        _ n: EventRow, kind: PerNoteKindRow, neutral: UInt32
    ) -> [(from: Int64, to: Int64, value: UInt32)] {
        let end = n.tick + max(n.duration, 0)
        var out: [(from: Int64, to: Int64, value: UInt32)] = []
        var at = n.tick
        var value = neutral
        for p in n.perNote where p.kind == kind {
            let t = min(max(n.tick + p.offset, n.tick), end)
            if t > at { out.append((at, t, value)) }
            at = max(at, t)
            value = p.value
        }
        out.append((at, end, value))
        return out
    }

    /// The notes a stroke over `from...to` edits: the selected ones among
    /// those it overlaps, else all it overlaps.
    public static func targets(_ notes: [EventRow], from: Int64, to: Int64, selected: Set<UInt64>) -> [UInt64] {
        let under = notes.filter { $0.tick <= to && $0.tick + $0.duration >= from }
        let chosen = under.filter { selected.contains($0.id) }
        return (chosen.isEmpty ? under : chosen).map(\.id)
    }

    /// Stroke points (time-area coordinates) as values, ascending, one per
    /// `pointSpacing` points; on one tick the later stroke point wins.
    public static func points(_ stroke: [CGPoint], time: TimeAxis, values: ValueAxis) -> [PerNoteValueRow] {
        let spacing = max(TimeAxis.clampTick(Double(pointSpacing) * time.ticksPerPoint), 1)
        var byTick: [Int64: UInt32] = [:]
        for p in stroke { byTick[time.tick(atX: p.x)] = values.value(atY: p.y) }
        var out: [PerNoteValueRow] = []
        for (tick, value) in byTick.sorted(by: { $0.key < $1.key }) {
            if let last = out.last, tick - last.tick < spacing { continue }
            out.append(PerNoteValueRow(tick: tick, value: value))
        }
        return out
    }

    /// A stroke's full target: one `SetPerNote` over its time range.
    public static func command(
        stroke: [CGPoint], erase: Bool, kind: PerNoteKindRow, notes: [EventRow], selected: Set<UInt64>,
        time: TimeAxis, values: ValueAxis
    ) -> EditCommand? {
        guard let lo = stroke.map(\.x).min(), let hi = stroke.map(\.x).max() else { return nil }
        let from = time.tick(atX: lo)
        let to = time.tick(atX: hi)
        let ids = targets(notes, from: from, to: to, selected: selected)
        guard !ids.isEmpty else { return nil }
        let pts = erase ? [] : points(stroke, time: time, values: values)
        return .event(edit: .setPerNote(events: ids, kind: kind, from: from, to: to, points: pts))
    }

    /// The pane header's label.
    public static func shortTitle(_ k: ExpressionKind) -> String {
        switch k {
        case .pitchBend: "Bend"
        case .registered(let i): "Reg \(i)"
        case .assignable(let i): "Asg \(i)"
        }
    }
}
