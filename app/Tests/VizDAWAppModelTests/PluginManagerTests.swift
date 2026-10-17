import Foundation
import Testing
import VizDAWCore
@testable import VizDAWAppModel

@MainActor
@Suite struct PluginManagerTests {
    func manager() -> (FakeCatalog, PluginManagerModel) {
        let cat = FakeCatalog()
        cat.table = [
            catalogRow("u.diva", "Diva", vendor: "u-he", kind: .instrument),
            catalogRow("x.crash", "Crasher", vendor: "Bad", status: .blocklisted(reason: "crashed (signal 11)")),
            catalogRow("aufx/dely/appl", "AUDelay", vendor: "Apple", format: .au, status: .notValidated),
            catalogRow("", "Broken", vendor: "", status: .blocklisted(reason: "timed out")),
        ]
        let p = AppPlugins(open: { _ in cat })
        p.start(config: AppPlugins.config(supportDir: URL(fileURLWithPath: "/tmp"), scanner: "x"), scan: false)
        return (cat, PluginManagerModel(plugins: p))
    }

    @Test func rowsShowTheSpecStatusesAndFilter() {
        let (_, m) = manager()
        #expect(m.rows.map(\.statusText) == ["OK", "Blocklisted: crashed (signal 11)", "Not validated", "Blocklisted: timed out"])
        #expect(m.rows[0].kindText == "Instrument")
        m.format = .au
        #expect(m.rows.map(\.name) == ["AUDelay"])
        m.format = .all
        m.search = "U-HE"
        #expect(m.rows.map(\.name) == ["Diva"])
    }

    @Test func disablingAndEnablingReachTheCatalog() {
        let (cat, m) = manager()
        m.setEnabled(m.rows[0], false)
        #expect(cat.enabled.map(\.1) == [false])
        #expect(m.rows[0].statusText == "Disabled")
        #expect(!m.rows[0].isEnabled)
        // A bundle that failed as a whole has nothing to enable.
        m.setEnabled(m.rows[3], false)
        #expect(cat.enabled.count == 1)
    }

    @Test func selectionDrivesRescanClearAndReveal() {
        let (cat, m) = manager()
        #expect(!m.canRescan && !m.canClearBlocklist)
        m.selection = [m.rows[1].id, m.rows[3].id]
        #expect(m.canClearBlocklist)
        m.clearBlocklist()
        #expect(cat.cleared.map(\.id) == ["x.crash"])
        m.selection = [m.rows[0].id]
        m.rescanSelected()
        #expect(cat.rescans.first?.map(\.id) == ["u.diva"])
        #expect(m.revealPath == "/Library/Audio/Plug-Ins/CLAP/u.diva.clap")
        m.rescanAll()
        #expect(cat.scans == [true])
        #expect(m.progressText == "Scanning 0 of 2…")
        #expect(!m.canRescan, "one scan at a time")
    }
}
