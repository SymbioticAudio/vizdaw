import Foundation
import Testing
import VizDAWCore
@testable import VizDAWAppModel

@MainActor
@Suite struct PluginBrowserTests {
    let rows = [
        catalogRow("u.diva", "Diva", vendor: "u-he", kind: .instrument),
        catalogRow("v.zeta", "zeta", vendor: "Vendor"),
        catalogRow("v.alpha", "Alpha", vendor: "Vendor"),
        catalogRow("x.loose", "Loose", vendor: ""),
        catalogRow("aufx/dely/appl", "AUDelay", vendor: "Apple", format: .au),
    ]

    @Test func groupsByVendorWithOtherLast() {
        let g = PluginBrowserModel.groups(rows, search: "")
        #expect(g.map(\.vendor) == ["Apple", "u-he", "Vendor", "Other"])
        #expect(g[2].plugins.map(\.name) == ["Alpha", "zeta"])
    }

    @Test func searchMatchesNameVendorOrIdInAnyCase() {
        #expect(PluginBrowserModel.groups(rows, search: "DIVA").map(\.vendor) == ["u-he"])
        #expect(PluginBrowserModel.groups(rows, search: " vendor ").flatMap(\.plugins).count == 2)
        #expect(PluginBrowserModel.groups(rows, search: "dely").first?.plugins.first?.name == "AUDelay")
        #expect(PluginBrowserModel.groups(rows, search: "nothing").isEmpty)
    }

    @Test func theModelListsItsKindOnly() {
        let cat = FakeCatalog()
        cat.table = rows
        let plugins = AppPlugins(open: { _ in cat })
        plugins.start(config: AppPlugins.config(supportDir: URL(fileURLWithPath: "/tmp"), scanner: "x"), scan: false)
        let m = PluginBrowserModel(kind: .instrument) { plugins.browser(.instrument) }
        #expect(m.groups.flatMap(\.plugins).map(\.name) == ["Diva"])
        m.search = "zzz"
        #expect(m.groups.isEmpty)
    }
}

@MainActor
@Suite struct AppPluginsTests {
    @Test func startOpensScansAndShowsAProblemOnce() {
        let cat = FakeCatalog()
        cat.problems = ["plugin settings not read: newer"]
        var configs: [CatalogConfig] = []
        let p = AppPlugins(open: { configs.append($0); return cat })
        let support = URL(fileURLWithPath: "/tmp/VizDAW")
        p.start(config: AppPlugins.config(supportDir: support, scanner: "/x/vizdaw-scan"), scan: true)
        #expect(configs.first?.path == "/tmp/VizDAW/plugins.json")
        #expect(configs.first?.includeAu == true)
        #expect(cat.scans == [false])
        #expect(p.isScanning)
        #expect(p.alert?.message == "plugin settings not read: newer")
        p.alert = nil
        p.refresh()
        #expect(p.alert == nil)
    }

    @Test func aFinishedScanRefreshesTheRows() {
        let cat = FakeCatalog()
        let p = AppPlugins(open: { _ in cat })
        p.start(config: AppPlugins.config(supportDir: URL(fileURLWithPath: "/tmp"), scanner: "x"), scan: true)
        #expect(p.rows.isEmpty)
        cat.table = [catalogRow("a", "A", vendor: "V")]
        cat.state = ScanProgress(running: false, done: 2, total: 2)
        p.poll()
        #expect(p.rows.count == 1)
        #expect(!p.isScanning)
    }

    @Test func actionsReachTheCatalogAndErrorsBecomeAlerts() {
        let cat = FakeCatalog()
        cat.table = [catalogRow("a", "A", vendor: "V")]
        let p = AppPlugins(open: { _ in cat })
        p.start(config: AppPlugins.config(supportDir: URL(fileURLWithPath: "/tmp"), scanner: "x"), scan: false)
        let key = PluginKey(cat.table[0].plugin)
        p.setEnabled(key, false)
        #expect(p.rows.first?.status == .disabled)
        p.rescan([key])
        p.clearBlocklist([key])
        #expect(cat.rescans == [[key]])
        #expect(cat.cleared == [[key]])
        #expect(p.revealPath(key) == "/Library/Audio/Plug-Ins/CLAP/a.clap")
        cat.scanError = .NotFound(path: "/x/vizdaw-scan")
        p.scan(all: true)
        #expect(p.alert?.title == "Scan Failed")
    }

    @Test func setEnabledReportsWhetherTheCatalogTookIt() {
        let cat = FakeCatalog()
        cat.table = [catalogRow("a", "A", vendor: "V")]
        let key = PluginKey(cat.table[0].plugin)
        let p = AppPlugins(open: { _ in cat })
        #expect(!p.setEnabled(key, false), "no catalog yet")
        p.start(config: AppPlugins.config(supportDir: URL(fileURLWithPath: "/tmp"), scanner: "x"), scan: false)
        #expect(p.setEnabled(key, false))
    }

    @Test func aCatalogThatCannotOpenIsAnAlert() {
        let p = AppPlugins(open: { _ in throw VizError.Io(message: "denied") })
        p.start(config: AppPlugins.config(supportDir: URL(fileURLWithPath: "/tmp"), scanner: "x"), scan: true)
        #expect(!p.isOpen)
        #expect(p.alert?.message == "denied")
        #expect(p.browser(.effect).isEmpty)
    }

    @Test func aProblemAndAFailedScanAreBothShownInOrder() {
        let cat = FakeCatalog()
        cat.problems = ["plugin settings not read: newer"]
        cat.scanError = .NotFound(path: "/x/vizdaw-scan")
        let p = AppPlugins(open: { _ in cat })
        p.start(config: AppPlugins.config(supportDir: URL(fileURLWithPath: "/tmp"), scanner: "x"), scan: true)
        #expect(p.alerts.map(\.title) == ["Plugin Settings", "Scan Failed"])
        p.alert = nil
        #expect(p.alert?.title == "Scan Failed")
        p.alert = nil
        #expect(p.alert == nil)
    }

    @Test func aRepeatedProblemIsQueuedOnce() {
        let cat = FakeCatalog()
        cat.stickyProblem = "not saved"
        let p = AppPlugins(open: { _ in cat })
        p.start(config: AppPlugins.config(supportDir: URL(fileURLWithPath: "/tmp"), scanner: "x"), scan: false)
        p.refresh()
        p.refresh()
        #expect(p.alerts.count == 1)
    }

    @Test func busyNeverAlertsAndAutomaticRunningScanIsSilent() {
        let cat = FakeCatalog()
        cat.scanError = .Busy(message: "busy")
        let p = AppPlugins(open: { _ in cat })
        p.start(config: AppPlugins.config(supportDir: URL(fileURLWithPath: "/tmp"), scanner: "x"), scan: true)
        p.scan(all: true)
        #expect(p.alerts.isEmpty)
        cat.scanError = .Engine(message: "a scan is already running")
        p.start(config: AppPlugins.config(supportDir: URL(fileURLWithPath: "/tmp"), scanner: "x"), scan: true)
        #expect(p.alerts.isEmpty)
        p.rescan([])
        p.scan(all: true)
        #expect(p.alerts.count == 1)
    }
}
