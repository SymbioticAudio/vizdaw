import Foundation
import Observation
import VizDAWCore

/// The plugin browser (spec §9): a popover from an insert slot ("+") or
/// from New Plugin Instrument…, listing usable plugins of one kind grouped
/// by vendor, filtered by a search.
@Observable
@MainActor
public final class PluginBrowserModel {
    public let kind: PluginKindTag
    public var search = ""
    private let rows: () -> [CatalogRowFfi]

    public init(kind: PluginKindTag, rows: @escaping () -> [CatalogRowFfi]) {
        self.kind = kind
        self.rows = rows
    }

    /// The groups the popover shows now.
    public var groups: [VendorGroup] { Self.groups(rows(), search: search) }

    /// One vendor's plugins.
    public struct VendorGroup: Identifiable, Equatable, Sendable {
        public let vendor: String
        public let plugins: [PluginRefRow]
        public var id: String { vendor }
    }

    /// `rows` grouped by vendor (vendors by name, "Other" last for none),
    /// plugins by name; a search matches name, vendor or id, any case.
    public nonisolated static func groups(_ rows: [CatalogRowFfi], search: String) -> [VendorGroup] {
        let q = search.trimmingCharacters(in: .whitespaces).lowercased()
        let matching = rows.map(\.plugin).filter { p in
            q.isEmpty
                || p.name.lowercased().contains(q)
                || p.vendor.lowercased().contains(q)
                || p.id.lowercased().contains(q)
        }
        let byVendor = Dictionary(grouping: matching) { $0.vendor.isEmpty ? "" : $0.vendor }
        return byVendor.keys
            .sorted { a, b in
                if a.isEmpty != b.isEmpty { return !a.isEmpty }
                return a.localizedCaseInsensitiveCompare(b) == .orderedAscending
            }
            .map { v in
                VendorGroup(
                    vendor: v.isEmpty ? "Other" : v,
                    plugins: (byVendor[v] ?? []).sorted {
                        $0.name.localizedCaseInsensitiveCompare($1.name) == .orderedAscending
                    })
            }
    }
}
