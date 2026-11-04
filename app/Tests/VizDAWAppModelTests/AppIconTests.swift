import AppKit
import SymbioticIcons
import SymbioticTokens
import Testing
@testable import VizDAWAppModel

/// The app's icons come from SymbioticIcons, by meaning (plan 7b Task B3).
@MainActor @Suite struct AppIconTests {
    @Test func everyAppIconDrawsWithoutTheBrand() {
        for i in AppIcon.allCases {
            let img = NSImage.icon(i.role, size: 16, theme: .classicLight)
            #expect(img.isTemplate && img.size == NSSize(width: 16, height: 16), "\(i)")
        }
    }
    @Test func thePaneTogglesUseThePaneRoles() {
        #expect([AppIcon.leftPane, .bottomPane, .rightPane].map(\.role) == [.viewLeftPane, .viewBottomPane, .viewRightPane])
        #expect(AppIcon.eq.role == .mixerEq)
    }
    @Test func theAppUsesNoSystemSymbols() throws {
        // app/Sources, from this file's path.
        let hits = try SourceScan.lines(in: ["VizDAW", "VizDAWEditors"],
            matching: #"systemImage:|systemName:|systemSymbolName:"#)
        #expect(hits.isEmpty, "\(hits)")
    }
}
