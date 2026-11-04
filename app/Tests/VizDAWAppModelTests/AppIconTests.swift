import AppKit
import Foundation
import SymbioticIcons
import Testing
@testable import VizDAWAppModel

/// The app's icons are chosen in one place, AppIcon (plan 7b Task B3); they
/// draw the SF Symbols the app had before 7b (owner decision, fix round 2).
@MainActor @Suite struct AppIconTests {
    @Test func everyAppIconSymbolIsASystemSymbol() {
        for i in AppIcon.allCases {
            #expect(NSImage(systemSymbolName: i.symbol, accessibilityDescription: nil) != nil, "\(i): \(i.symbol)")
            #expect(i.nsImage(accessibilityDescription: nil) != nil, "\(i)")
        }
    }
    @Test func thePaneTogglesUseThePaneRoles() {
        #expect([AppIcon.leftPane, .bottomPane, .rightPane].map(\.role) == [.viewLeftPane, .viewBottomPane, .viewRightPane])
        #expect(AppIcon.eq.role == .mixerEq)
    }
    /// No symbol name is written in the views: each comes from AppIcon.
    @Test func theAppUsesNoSystemSymbols() throws {
        // app/Sources, from this file's path.
        let dirs = ["VizDAW", "VizDAWEditors"]
        var hits = try SourceScan.lines(in: dirs, matching: #"(systemImage|systemName|systemSymbolName):\s*""#)
        // Any lowercase, dotted string literal that names an SF Symbol, except
        // words the views use for something else that SF Symbols also name.
        let notIcons: Set<String> = [
            "map"  // ModifyPanel: the Map field's key.
        ]
        let literal = try NSRegularExpression(pattern: #""([a-z0-9]+(?:\.[a-z0-9]+)*)""#)
        for line in try SourceScan.lines(in: dirs, matching: literal.pattern) {
            let range = NSRange(line.startIndex..., in: line)
            for m in literal.matches(in: line, range: range) {
                guard let r = Range(m.range(at: 1), in: line) else { continue }
                let name = String(line[r])
                if !notIcons.contains(name), NSImage(systemSymbolName: name, accessibilityDescription: nil) != nil {
                    hits.append(line)
                    break
                }
            }
        }
        #expect(hits.isEmpty, "\(hits)")
    }
    /// The Settings tab icons are templates with a transparent background and
    /// ink only where the glyph is (B3 fix round 1).
    @Test func theSettingsTabImagesAreTransparentTemplates() throws {
        let tabs: [AppIcon] = [
            .settingsEditing, .settingsKeys, .settingsAudio, .settingsRecording, .settingsAutomation,
            .settingsSurfaces, .settingsAppearance,
        ]
        for i in tabs {
            let img = try #require(i.nsImage(accessibilityDescription: nil), "\(i)")
            #expect(img.isTemplate, "\(i)")
            var rect = NSRect(x: 0, y: 0, width: 32, height: 32)
            let cg = try #require(img.cgImage(forProposedRect: &rect, context: nil, hints: nil), "\(i)")
            let rep = NSBitmapImageRep(cgImage: cg)
            let (w, h) = (rep.pixelsWide, rep.pixelsHigh)
            for (x, y) in [(0, 0), (w - 1, 0), (0, h - 1), (w - 1, h - 1)] {
                #expect((rep.colorAt(x: x, y: y)?.alphaComponent ?? 1) == 0, "\(i) corner \(x),\(y)")
            }
            var inked = 0
            for y in 0..<h {
                for x in 0..<w where (rep.colorAt(x: x, y: y)?.alphaComponent ?? 0) > 0.5 { inked += 1 }
            }
            // Some ink, and far from a filled image.
            #expect(inked > 8 && inked < w * h * 3 / 4, "\(i): \(inked) of \(w * h) pixels inked")
        }
    }
}
