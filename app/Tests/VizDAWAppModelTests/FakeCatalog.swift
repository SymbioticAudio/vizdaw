import Foundation
import VizDAWCore

/// A scripted `PluginCatalogProtocol` for the plugin models' tests.
final class FakeCatalog: PluginCatalogProtocol, @unchecked Sendable {
    var table: [CatalogRowFfi] = []
    var problems: [String] = []
    var scans: [Bool] = []
    var rescans: [[PluginKey]] = []
    var enabled: [(PluginKey, Bool)] = []
    var cleared: [PluginKey] = []
    var state = ScanProgress(running: false, done: 0, total: 0)
    var scanError: VizError?

    func browser(kind: PluginKindTag) throws -> [CatalogRowFfi] {
        table.filter { $0.kind == kind && $0.status == .ok }
    }
    func clearBlocklist(plugin: PluginKey) throws { cleared.append(plugin) }
    func progress() throws -> ScanProgress { state }
    func rescan(plugins: [PluginKey]) throws { rescans.append(plugins) }
    func revealPath(plugin: PluginKey) throws -> String? {
        plugin.format == .clap ? "/Library/Audio/Plug-Ins/CLAP/\(plugin.id).clap" : nil
    }
    func rows() throws -> [CatalogRowFfi] { table }
    func scan(all: Bool) throws {
        if let e = scanError { throw e }
        scans.append(all)
        state = ScanProgress(running: true, done: 0, total: 2)
    }
    func setEnabled(plugin: PluginKey, enabled on: Bool) throws {
        enabled.append((plugin, on))
        if let i = table.firstIndex(where: { $0.plugin.id == plugin.id }) {
            table[i].status = on ? .ok : .disabled
        }
    }
    func takeProblem() throws -> String? { problems.isEmpty ? nil : problems.removeFirst() }
}

func catalogRow(
    _ id: String, _ name: String, vendor: String, kind: PluginKindTag = .effect,
    format: PluginFormatTag = .clap, status: PluginStatusRow = .ok
) -> CatalogRowFfi {
    CatalogRowFfi(
        plugin: PluginRefRow(format: format, id: id, name: name, vendor: vendor, version: "1.0"),
        kind: kind, status: status, path: "/p/\(id)")
}
