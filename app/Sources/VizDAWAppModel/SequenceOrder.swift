import Foundation
import VizDAWCore

/// The sequence sidebar's user order (Plan Ruling 12).
public enum SequenceOrder {
    /// `rows` in the user's order: IDs listed in `order` first, in that
    /// order; the rest after them by ID. Stale IDs are ignored.
    public static func sorted(_ rows: [SeqRow], by order: [UInt64]) -> [SeqRow] {
        var rank: [UInt64: Int] = [:]
        for (i, id) in order.enumerated() where rank[id] == nil { rank[id] = i }
        return rows.sorted { a, b in
            switch (rank[a.id], rank[b.id]) {
            case let (x?, y?): x < y
            case (.some, nil): true
            case (nil, .some): false
            case (nil, nil): a.id < b.id
            }
        }
    }

    /// The full order after a sidebar section showing `visible` (IDs in
    /// display order) moved the rows at `source` to `destination` (the
    /// `onMove` convention); rows outside the section keep their places.
    public static func moved(
        all: [SeqRow], order: [UInt64], visible: [UInt64], from source: IndexSet, to destination: Int
    ) -> [UInt64] {
        let moving = source.sorted().filter { visible.indices.contains($0) }.map { visible[$0] }
        var rest = visible.enumerated().filter { !source.contains($0.offset) }.map(\.element)
        let at = destination - source.filter { $0 < destination }.count
        rest.insert(contentsOf: moving, at: min(max(at, 0), rest.count))
        let members = Set(visible)
        var next = rest.makeIterator()
        return sorted(all, by: order).map { row in
            members.contains(row.id) ? (next.next() ?? row.id) : row.id
        }
    }
}
