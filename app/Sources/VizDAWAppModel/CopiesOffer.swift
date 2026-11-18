import Foundation
import Observation
import VizDAWCore

/// Compact's offer to remove old Co-Producer copies (plan 8b Ruling 8):
/// one row per copy, oldest first, none chosen. Only the composer's click
/// on Remove Selected removes anything, and only what they chose.
@MainActor
@Observable
public final class CopiesOffer {
    public struct Row: Identifiable, Equatable {
        /// The copy's folder name, which removal takes.
        public let id: String
        /// About what removing it frees ("about 12.4 MB").
        public let size: String
        public var chosen: Bool
    }

    public private(set) var rows: [Row]

    /// Nil when there are no copies; none is chosen.
    public init?(copies: [CopyRow]) {
        guard !copies.isEmpty else { return nil }
        rows = copies.map { Row(id: $0.name, size: Self.size($0.bytes), chosen: false) }
    }

    /// Ticks or unticks row `id`; an unknown id changes nothing.
    public func toggle(_ id: String) {
        guard let i = rows.firstIndex(where: { $0.id == id }) else { return }
        rows[i].chosen.toggle()
    }

    /// Whether a row is chosen (Remove Selected is enabled).
    public var canRemove: Bool { rows.contains(where: \.chosen) }

    /// Ruling 8's text.
    public var message: String {
        let n = rows.count
        return "Compact Media is done. This song also keeps \(n) \(n == 1 ? "copy" : "copies") Co-Producer saved "
            + "before its requests. Choose any you no longer need: they go to the Trash, and the files only they "
            + "use go when you next save."
    }

    /// Removes the chosen copies; the problem lines (copies skipped or
    /// kept). Nothing is asked of the session when none is chosen.
    public func remove(_ session: any SessionProtocol) -> [String] {
        let names = rows.filter(\.chosen).map(\.id)
        guard !names.isEmpty else { return [] }
        do {
            return try session.removeCoProducerCopies(names: names).problems
        } catch {
            return ["Old copies not removed: \(SongNotes.sentence(error))."]
        }
    }

    /// "about 12.4 MB": decimal units, as Finder counts; under a megabyte
    /// in whole kilobytes, rounded up.
    public static func size(_ bytes: UInt64) -> String {
        let b = Double(bytes)
        if b >= 1_000_000_000 { return "about " + String(format: "%.1f", b / 1_000_000_000) + " GB" }
        if b >= 1_000_000 { return "about " + String(format: "%.1f", b / 1_000_000) + " MB" }
        return "about \(max(1, Int((b / 1000).rounded(.up)))) KB"
    }
}
