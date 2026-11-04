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
    /// The Settings tab icons (`VizDAWApp.tab`: `image(size: 16, theme:
    /// .classicLight)`) are templates with a transparent background and ink
    /// only where the glyph is: AppKit tints them, so an opaque or filled
    /// image would draw as a solid box (B3 fix round 1).
    @Test func theSettingsTabImagesAreTransparentTemplates() throws {
        let tabs: [AppIcon] = [
            .settingsEditing, .settingsKeys, .settingsAudio, .settingsRecording, .settingsAutomation,
            .settingsSurfaces, .settingsAppearance,
        ]
        for i in tabs {
            let img = i.image(size: 16, theme: .classicLight)
            #expect(img.isTemplate, "\(i)")
            var rect = NSRect(x: 0, y: 0, width: 32, height: 32)
            let cg = try #require(img.cgImage(forProposedRect: &rect, context: nil, hints: nil), "\(i)")
            let rep = NSBitmapImageRep(cgImage: cg)
            #expect(rep.pixelsWide == 32 && rep.pixelsHigh == 32, "\(i)")
            for (x, y) in [(0, 0), (31, 0), (0, 31), (31, 31)] {
                #expect((rep.colorAt(x: x, y: y)?.alphaComponent ?? 1) == 0, "\(i) corner \(x),\(y)")
            }
            var inked = 0
            for y in 0..<32 {
                for x in 0..<32 where (rep.colorAt(x: x, y: y)?.alphaComponent ?? 0) > 0.5 { inked += 1 }
            }
            // Some ink, and far from a filled square.
            #expect(inked > 8 && inked < 32 * 32 / 2, "\(i): \(inked) inked pixels")
        }
    }
}
