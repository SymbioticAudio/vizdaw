import Foundation
import Observation
import VizDAWCore

/// The Plugin Manager window (spec §9): every known plugin with its
/// status; enable/disable, rescan selected or all, clear a blocklist entry,
/// reveal in Finder; a search field and a format filter.
@Observable
@MainActor
public final class PluginManagerModel {
    public let plugins: AppPlugins
    public var search = ""
    public var format: FormatFilter = .all
    /// Selected rows (`ManagerRow.id`).
    public var selection: Set<String> = []

    public enum FormatFilter: String, CaseIterable, Identifiable, Sendable {
        case all = "All"
        case au = "AU"
        case clap = "CLAP"
        public var id: String { rawValue }
    }

    public init(plugins: AppPlugins) {
        self.plugins = plugins
    }

    /// One table row.
    public struct ManagerRow: Identifiable, Equatable, Sendable {
        public let row: CatalogRowFfi
        public var id: String { "\(row.plugin.format):\(row.plugin.id)|\(row.path)" }
        public var name: String { row.plugin.name }
        public var vendor: String { row.plugin.vendor }
        public var version: String { row.plugin.version }
        public var formatText: String { row.plugin.format == .au ? "AU" : "CLAP" }
        public var kindText: String {
            switch row.kind {
            case .instrument: "Instrument"
            case .effect: "Effect"
            case nil: "—"
            }
        }
        /// `OK`, `Disabled`, `Blocklisted: reason`, `Not validated`,
        /// `Missing` (spec §9).
        public var statusText: String {
            switch row.status {
            case .ok: "OK"
            case .disabled: "Disabled"
            case .blocklisted(let reason): "Blocklisted: \(reason)"
            case .notValidated: "Not validated"
            case .missing: "Missing"
            }
        }
        public var isEnabled: Bool { row.enabled }
        public var isBlocklisted: Bool {
            if case .blocklisted = row.status { return true }
            return false
        }
        /// A whole bundle that failed has no plugin id: nothing to enable.
        public var hasPlugin: Bool { !row.plugin.id.isEmpty }
        public var key: PluginKey { PluginKey(row.plugin) }
    }

    /// The rows the table shows now.
    public var rows: [ManagerRow] {
        let q = search.trimmingCharacters(in: .whitespaces).lowercased()
        return plugins.rows.map(ManagerRow.init).filter { r in
            let f: Bool =
                switch format {
                case .all: true
                case .au: r.row.plugin.format == .au
                case .clap: r.row.plugin.format == .clap
                }
            return f && (q.isEmpty || r.name.lowercased().contains(q) || r.vendor.lowercased().contains(q))
        }
    }

    private var selected: [ManagerRow] { rows.filter { selection.contains($0.id) } }

    /// "Scanning 3 of 40…", or "" when idle.
    public var progressText: String {
        let p = plugins.progress
        return p.running ? "Scanning \(p.done) of \(p.total)…" : ""
    }

    // MARK: Intents

    public func setEnabled(_ r: ManagerRow, _ on: Bool) {
        guard r.hasPlugin else { return }
        plugins.setEnabled(r.key, on)
    }

    public func rescanSelected() {
        let keys = selected.filter(\.hasPlugin).map(\.key)
        if !keys.isEmpty { plugins.rescan(keys) }
    }

    public func rescanAll() { plugins.scan(all: true) }

    /// Clears the selected blocklist entries; each bundle is scanned again.
    public func clearBlocklist() {
        let keys = selected.filter { $0.isBlocklisted && $0.hasPlugin }.map(\.key)
        if !keys.isEmpty { plugins.clearBlocklist(keys) }
    }

    public var canClearBlocklist: Bool {
        !plugins.isScanning && selected.contains { $0.isBlocklisted && $0.hasPlugin }
    }
    public var canRescan: Bool { !selected.isEmpty && !plugins.isScanning }

    /// The first selected row's bundle, for Reveal in Finder.
    public var revealPath: String? {
        selected.lazy.compactMap { self.plugins.revealPath($0.key) }.first
    }
}
