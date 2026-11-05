import AppKit
import Foundation
import SymbioticIcons
import SymbioticTokens
import Testing
@testable import VizDAWAppModel

/// Stands in for SymbioticBrand (no test target links it): a filled square
/// for every role, so a brand glyph is told from a standard or angular icon.
private struct SquareBrand: IconProvider {
    static let square = IconDrawing(shapes: [
        IconShape(paint: .fill, weight: 1, cap: .butt, join: .miter, ops: [
            .move(3, 3), .line(21, 3), .line(21, 21), .line(3, 21), .close,
        ])
    ])
    func drawing(for role: IconRole) -> IconDrawing? { Self.square }
}

/// The alpha of an image drawn at `pixels` square, row by row.
@MainActor private func alpha(_ image: NSImage, pixels: Int = 32) throws -> [UInt8] {
    var rect = NSRect(x: 0, y: 0, width: pixels, height: pixels)
    let cg = try #require(image.cgImage(forProposedRect: &rect, context: nil, hints: nil))
    var bytes = [UInt8](repeating: 0, count: pixels * pixels * 4)
    bytes.withUnsafeMutableBytes { buffer in
        let ctx = CGContext(
            data: buffer.baseAddress, width: pixels, height: pixels, bitsPerComponent: 8, bytesPerRow: pixels * 4,
            space: CGColorSpace(name: CGColorSpace.sRGB)!, bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue)
        ctx?.draw(cg, in: CGRect(x: 0, y: 0, width: pixels, height: pixels))
    }
    return stride(from: 3, to: bytes.count, by: 4).map { bytes[$0] }
}

/// The app's icons are chosen in one place, AppIcon (plan 7b Task B3). Every
/// case draws a SymbioticAudio icon (owner, 2026-11-04; final fix wave B):
/// the default set (the brand glyph where registered, else standard) or the
/// angular set. None draws an SF Symbol.
@MainActor @Suite struct AppIconTests {
    static let none = IconResolver(provider: nil)
    static let brand = IconResolver(provider: SquareBrand())

    /// The owner's two lists, case by case.
    @Test func eachCaseDrawsFromTheOwnersSet() {
        let glyph: [AppIcon] = [
            .warning, .problem, .info, .mixer, .add, .confirm, .close, .drumGrid, .list, .muted, .loop, .split,
            .empty, .pencil, .diskSlow, .expand, .collapse, .menuDown, .leftPane, .bottomPane, .rightPane,
            .settingsEditing, .settingsAutomation, .settingsSurfaces, .eq, .midiLearn,
        ]
        let angular: [AppIcon] = [
            .fault, .faultIndicator, .remove, .zoomIn, .zoomOut, .audio, .pianoRoll, .record, .armed, .recording,
            .settingsKeys, .settingsAudio, .settingsRecording, .play, .stop, .returnToStart, .rewind, .fastForward,
        ]
        for i in glyph { #expect(i.source == .glyph, "\(i)") }
        for i in angular { #expect(i.source == .angular, "\(i)") }
        // In neither list: Settings › Appearance (added in B4) draws the
        // default set, as an unlisted case does.
        #expect(AppIcon.settingsAppearance.source == .glyph)
        let listed = Set(glyph + angular + [.settingsAppearance])
        #expect(listed.count == glyph.count + angular.count + 1, "a case is in both lists")
        #expect(listed == Set(AppIcon.allCases))
        #expect(IconSource.glyph.set == .standard && IconSource.angular.set == .angular)
    }

    /// The three transport jump icons are angular, on 0.2.2's roles, and
    /// draw ink at 16 points with and without the brand.
    @Test func theTransportJumpIconsAreAngular() throws {
        let cases: [(AppIcon, IconRole)] = [
            (.returnToStart, .transportReturnToZero), (.rewind, .transportRewind), (.fastForward, .transportFastForward),
        ]
        for (icon, role) in cases {
            #expect(icon.role == role, "\(icon)")
            #expect(icon.source == .angular, "\(icon)")
            for resolver in [Self.none, Self.brand] {
                let img = icon.nsImage(accessibilityDescription: nil, size: 16, theme: .classicLight, resolver: resolver)
                #expect(img.isTemplate && img.size == NSSize(width: 16, height: 16), "\(icon)")
                #expect(try alpha(img).filter { $0 > 0 }.count > 8, "\(icon): blank")
            }
        }
    }

    /// 0.2.1's own roles for the fault dot and the Record button while
    /// recording; Settings › Recording keeps transport.record.
    @Test func theRolesFollowTheDesignRelease() {
        #expect(AppIcon.faultIndicator.role == .statusFaultIndicator)
        #expect(AppIcon.recording.role == .transportRecording)
        #expect(AppIcon.settingsRecording.role == .transportRecord)
        #expect([AppIcon.leftPane, .bottomPane, .rightPane].map(\.role) == [.viewLeftPane, .viewBottomPane, .viewRightPane])
        #expect(AppIcon.eq.role == .mixerEq)
        // Every angular case has an angular icon to draw.
        for i in AppIcon.allCases where i.source == .angular { #expect(i.role.angular != nil, "\(i)") }
    }

    /// Every case is a 16-point template image with ink, without the brand
    /// (the tests' and a fork's build) and with one registered.
    @Test func everyCaseIsASixteenPointTemplateWithAndWithoutTheBrand() throws {
        for resolver in [Self.none, Self.brand] {
            for i in AppIcon.allCases {
                let img = i.nsImage(accessibilityDescription: "Label of \(i)", theme: .classicLight, resolver: resolver)
                #expect(img.isTemplate, "\(i)")
                #expect(img.size == NSSize(width: 16, height: 16), "\(i): \(img.size)")
                #expect(img.accessibilityDescription == "Label of \(i)", "\(i)")
                let inked = try alpha(img).filter { $0 > 0 }.count
                #expect(inked > 8 && inked < 32 * 32, "\(i): \(inked) pixels inked")
            }
        }
    }

    /// Glyph cases draw the brand glyph where one is registered, else the
    /// standard icon; angular cases draw the angular icon either way.
    @Test func glyphCasesFollowTheBrandAndAngularCasesDoNot() throws {
        let square = try alpha(NSImage.icon(.statusInfo, size: 16, theme: .classicLight, resolver: Self.brand))
        for i in AppIcon.allCases {
            let without = try alpha(i.nsImage(accessibilityDescription: nil, theme: .classicLight, resolver: Self.none))
            let with = try alpha(i.nsImage(accessibilityDescription: nil, theme: .classicLight, resolver: Self.brand))
            switch i.source {
            case .glyph:
                let standard = try alpha(NSImage.icon(i.role, size: 16, theme: .classicLight, resolver: Self.none))
                #expect(without == standard, "\(i): no brand draws the standard icon")
                #expect(with == square, "\(i): the brand glyph")
            case .angular:
                let angular = try alpha(
                    NSImage.icon(i.role, size: 16, theme: .classicLight, set: .angular, resolver: Self.none))
                #expect(without == angular && with == angular, "\(i): the angular icon")
                let standard = try alpha(NSImage.icon(i.role, size: 16, theme: .classicLight, resolver: Self.none))
                #expect(angular != standard, "\(i): angular differs from standard")
            }
        }
    }

    /// The SwiftUI helpers and the default theme draw through the same image.
    @Test func theHelpersDrawTheIconsInTheCurrentTheme() {
        for i in AppIcon.allCases {
            let img = i.nsImage(accessibilityDescription: i.role.label)
            #expect(img.isTemplate && img.size == NSSize(width: 16, height: 16), "\(i)")
            #expect(img.accessibilityDescription == i.role.label, "\(i)")
        }
        _ = AppIcon.warning.image
        _ = AppIcon.confirm.label("Read")
    }

    /// `symbol` stays as data for reverting a case: each still names a
    /// system symbol, though nothing draws it.
    @Test func theRevertSymbolsStillNameSystemSymbols() {
        for i in AppIcon.allCases {
            #expect(NSImage(systemSymbolName: i.symbol, accessibilityDescription: nil) != nil, "\(i): \(i.symbol)")
        }
    }

    /// No view draws an SF Symbol: no symbol name is written in the views,
    /// no system-symbol API is called anywhere in the app, and `symbol` is
    /// read only by AppIcon itself.
    @Test func theAppUsesNoSystemSymbols() throws {
        // app/Sources, from this file's path.
        let dirs = ["VizDAW", "VizDAWEditors"]
        var hits = try SourceScan.lines(
            in: dirs + ["VizDAWAppModel"], matching: #"(systemImage|systemName|systemSymbolName)\s*:"#)
        hits += try SourceScan.lines(in: dirs, matching: #"\.symbol\b"#)
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
    /// ink only where the icon is (B3 fix round 1), with and without the brand.
    @Test func theSettingsTabImagesAreTransparentTemplates() throws {
        let tabs: [AppIcon] = [
            .settingsEditing, .settingsKeys, .settingsAudio, .settingsRecording, .settingsAutomation,
            .settingsSurfaces, .settingsAppearance,
        ]
        for resolver in [Self.none, Self.brand] {
            for i in tabs {
                let img = i.nsImage(accessibilityDescription: nil, theme: .classicLight, resolver: resolver)
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
}
