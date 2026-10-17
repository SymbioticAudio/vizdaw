import Foundation
import VizDAWCore

/// A scripted `PluginCatalogProtocol` for the plugin models' tests.
final class FakeCatalog: PluginCatalogProtocol, @unchecked Sendable {
    var table: [CatalogRowFfi] = []
    var problems: [String] = []
    var scans: [Bool] = []
    var rescans: [[PluginKey]] = []
    var enabled: [(PluginKey, Bool)] = []
    var cleared: [[PluginKey]] = []
    /// Thrown by `setEnabled` when set (e.g. `.Busy`).
    var enableError: VizError?
    var state = ScanProgress(running: false, done: 0, total: 0)
    var scanError: VizError?
    /// A problem every `takeProblem` returns (never drained).
    var stickyProblem: String?
    var rowsError: VizError?

    func browser(kind: PluginKindTag) throws -> [CatalogRowFfi] {
        table.filter { $0.kind == kind && $0.status == .ok }
    }
    func clearBlocklist(plugins: [PluginKey]) throws {
        if state.running { throw VizError.Engine(message: "a scan is already running") }
        cleared.append(plugins)
        // Like the real catalog: entries go, one scan starts.
        for k in plugins {
            if let i = table.firstIndex(where: { $0.plugin.id == k.id }), case .blocklisted = table[i].status {
                table[i].status = table[i].enabled ? .ok : .disabled
            }
        }
        state = ScanProgress(running: true, done: 0, total: 1)
    }
    func progress() throws -> ScanProgress { state }
    func rescan(plugins: [PluginKey]) throws { rescans.append(plugins) }
    func revealPath(plugin: PluginKey) throws -> String? {
        plugin.format == .clap ? "/Library/Audio/Plug-Ins/CLAP/\(plugin.id).clap" : nil
    }
    func rows() throws -> [CatalogRowFfi] {
        if let e = rowsError { throw e }
        return table
    }
    func scan(all: Bool) throws {
        if let e = scanError { throw e }
        scans.append(all)
        state = ScanProgress(running: true, done: 0, total: 2)
    }
    func setEnabled(plugin: PluginKey, enabled on: Bool) throws {
        if let e = enableError { throw e }
        enabled.append((plugin, on))
        if let i = table.firstIndex(where: { $0.plugin.id == plugin.id }) {
            // The preference is separate; a blocklisted status wins over it.
            table[i].enabled = on
            switch table[i].status {
            case .ok, .disabled: table[i].status = on ? .ok : .disabled
            default: break
            }
        }
    }
    func takeProblem() throws -> String? {
        if let s = stickyProblem { return s }
        return problems.isEmpty ? nil : problems.removeFirst()
    }
}

func catalogRow(
    _ id: String, _ name: String, vendor: String, kind: PluginKindTag = .effect,
    format: PluginFormatTag = .clap, status: PluginStatusRow = .ok
) -> CatalogRowFfi {
    CatalogRowFfi(
        plugin: PluginRefRow(format: format, id: id, name: name, vendor: vendor, version: "1.0"),
        kind: kind, status: status, enabled: status != .disabled, path: "/p/\(id)")
}
