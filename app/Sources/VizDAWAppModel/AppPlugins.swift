import Foundation
import Observation
import VizDAWCore

/// The app's plugin catalog (spec §8, §9): opened once at launch, scanned
/// for new or changed plugins, and read by the plugin browser and the
/// Plugin Manager. Problems (a catalog file it cannot read or write, a
/// failed scan) become one alert each.
@Observable
@MainActor
public final class AppPlugins {
    public typealias Open = (CatalogConfig) throws -> any PluginCatalogProtocol

    /// Every known plugin, by name (refreshed after each change and scan).
    public private(set) var rows: [CatalogRowFfi] = []
    public private(set) var progress = ScanProgress(running: false, done: 0, total: 0)
    public var alert: AlertItem?
    /// Bumped whenever `rows` changes (browsers refetch).
    public private(set) var version = 0

    private let open: Open
    private var catalog: (any PluginCatalogProtocol)?

    public init(open: @escaping Open) {
        self.open = open
    }

    public var isOpen: Bool { catalog != nil }
    public var isScanning: Bool { progress.running }

    /// Opens the catalog; with `scan`, new or changed plugins are scanned
    /// in the background.
    public func start(config: CatalogConfig, scan: Bool) {
        do {
            catalog = try open(config)
        } catch {
            alert = AlertItem(error, title: "Plugins Unavailable")
            return
        }
        refresh()
        if scan { self.scan(all: false) }
    }

    /// The configuration the app uses: the catalog in `supportDir`, the
    /// scanner at `scanner`, the standard CLAP folders and Audio Units.
    public static func config(supportDir: URL, scanner: String) -> CatalogConfig {
        CatalogConfig(
            path: supportDir.appendingPathComponent("plugins.json").path,
            scanner: scanner, clapDirs: [], includeAu: true, timeoutMs: nil)
    }

    /// Rereads the rows and shows a catalog problem, if there is one.
    public func refresh() {
        guard let c = catalog else { return }
        do {
            rows = try c.rows()
            version += 1
            if let p = try c.takeProblem(), alert == nil {
                alert = AlertItem(title: "Plugin Settings", message: p)
            }
        } catch {
            alert = AlertItem(error, title: "Plugins Unavailable")
        }
    }

    /// The plugin browser's list for `kind` (usable plugins only).
    public func browser(_ kind: PluginKindTag) -> [CatalogRowFfi] {
        (try? catalog?.browser(kind: kind)) ?? []
    }

    /// Reads the scan's progress (the app calls it a few times a second);
    /// a scan that finished refreshes the rows.
    public func poll() {
        guard let c = catalog, let p = try? c.progress() else { return }
        let finished = progress.running && !p.running
        let advanced = p.done != progress.done
        progress = p
        if finished || advanced { refresh() }
    }

    public func scan(all: Bool) {
        perform("Scan Failed") { try $0.scan(all: all) }
    }

    public func rescan(_ keys: [PluginKey]) {
        perform("Rescan Failed") { try $0.rescan(plugins: keys) }
    }

    public func setEnabled(_ key: PluginKey, _ on: Bool) {
        perform("Plugin Settings") { try $0.setEnabled(plugin: key, enabled: on) }
    }

    public func clearBlocklist(_ key: PluginKey) {
        perform("Rescan Failed") { try $0.clearBlocklist(plugin: key) }
    }

    /// The bundle to show in Finder (CLAP), if any.
    public func revealPath(_ key: PluginKey) -> String? {
        (try? catalog?.revealPath(plugin: key)) ?? nil
    }

    private func perform(_ title: String, _ body: (any PluginCatalogProtocol) throws -> Void) {
        guard let c = catalog else { return }
        do {
            try body(c)
        } catch {
            alert = AlertItem(error, title: title)
        }
        refresh()
        poll()
    }
}

extension PluginKey {
    /// The key of `plugin`.
    public init(_ plugin: PluginRefRow) {
        self.init(format: plugin.format, id: plugin.id)
    }
}
