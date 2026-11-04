import AppKit
import Foundation
import SymbioticTokens
import Testing
@testable import VizDAWEditors

/// Scans the app's own sources (`app/Sources/<dir>/**/*.swift`, found from
/// this file's path) for lines matching regular expressions (plan 7b Task
/// B5). This target's own copy: the two test targets share no files.
enum SourceScan {
    struct ScanError: Error, CustomStringConvertible {
        let description: String
    }

    /// `app/Sources`, from `app/Tests/VizDAWEditorsTests/ThemeScanTests.swift`.
    static var sourcesRoot: URL {
        URL(fileURLWithPath: #filePath)
            .deletingLastPathComponent()  // VizDAWEditorsTests
            .deletingLastPathComponent()  // Tests
            .deletingLastPathComponent()  // app
            .appendingPathComponent("Sources", isDirectory: true)
    }

    /// `"file:line: text"` for every line of `dirs`' Swift files that matches
    /// any of `patterns`, files in path order, skipping the files in `except`
    /// (paths below `app/Sources`).
    static func lines(in dirs: [String], matching patterns: [String], except: Set<String> = []) throws
        -> [String]
    {
        let regexes = try patterns.map { try NSRegularExpression(pattern: $0) }
        let fm = FileManager.default
        var hits: [String] = []
        for dir in dirs {
            let root = sourcesRoot.appendingPathComponent(dir, isDirectory: true)
            var isDir: ObjCBool = false
            guard fm.fileExists(atPath: root.path, isDirectory: &isDir), isDir.boolValue else {
                throw ScanError(description: "no source directory \(root.path)")
            }
            guard let walker = fm.enumerator(at: root, includingPropertiesForKeys: nil) else {
                throw ScanError(description: "cannot list \(root.path)")
            }
            var files: [URL] = []
            for case let url as URL in walker where url.pathExtension == "swift" {
                files.append(url)
            }
            for url in files.sorted(by: { $0.path < $1.path }) {
                let name = url.path.replacingOccurrences(of: sourcesRoot.path + "/", with: "")
                if except.contains(name) { continue }
                let text = try String(contentsOf: url, encoding: .utf8)
                for (i, line) in text.components(separatedBy: "\n").enumerated()
                where matches(line, regexes) > 0 {
                    hits.append("\(name):\(i + 1): \(line.trimmingCharacters(in: .whitespaces))")
                }
            }
        }
        return hits
    }

    /// How many matches `regexes` find in `line`, in all.
    static func matches(_ line: String, _ regexes: [NSRegularExpression]) -> Int {
        let range = NSRange(line.startIndex..., in: line)
        return regexes.reduce(0) { $0 + $1.numberOfMatches(in: line, range: range) }
    }

    static func matches(_ line: String, patterns: [String]) throws -> Int {
        matches(line, try patterns.map { try NSRegularExpression(pattern: $0) })
    }
}

/// Every colour, radius and font size from the theme (plan 7b Ruling 13).
@MainActor
@Suite struct ThemeScanTests {
    static let dirs = ["VizDAW", "VizDAWEditors"]
    /// The only files that may name a concrete colour.
    static let bridges: Set<String> = ["VizDAW/Theming.swift", "VizDAWEditors/EditorTheme.swift"]

    /// Ruling 13's colour forms. The bare-member list is the ruling's, plus
    /// five more system colours and SwiftUI's `.quaternary` the sweep found
    /// (`ThemedRoot` maps only `.primary`, `.secondary` and `.tertiary`).
    static let colourPatterns = [
        #"\b(NS)?Color\.(?!clear\b)[A-Za-z_]"#,
        #"\bColor\(red:|\bNSColor\(red:|\bNSColor\(calibrated|\b(NS)?Color\((white|hue|srgbRed|deviceRed|deviceWhite):"#,
        #"(?<![A-Za-z0-9_])\.(system(Red|Orange|Yellow|Green|Mint|Teal|Cyan|Blue|Indigo|Purple|Pink|Brown|Gray)|labelColor|secondaryLabelColor|tertiaryLabelColor|separatorColor|controlAccentColor|windowBackgroundColor|textColor|quaternaryLabelColor|textBackgroundColor|controlBackgroundColor|underPageBackgroundColor|selectedContentBackgroundColor|quaternary|black|white|red|orange|yellow|green|mint|teal|cyan|blue|indigo|purple|pink|brown|gray)\b"#,
    ]

    static let metricPatterns = [
        #"cornerRadius:\s*[0-9]"#, #"\.system\(size:\s*[0-9]"#, #"systemFont\(ofSize:"#, #"SystemFontSize"#,
    ]

    @Test func noColourLiteralsOutsideTheThemeBridges() throws {
        let hits = try SourceScan.lines(in: Self.dirs, matching: Self.colourPatterns, except: Self.bridges)
        #expect(hits.isEmpty, "\(hits.count) colour literals:\n\(hits.joined(separator: "\n"))")
    }

    @Test func noCornerRadiusOrFontSizeLiterals() throws {
        let hits = try SourceScan.lines(in: Self.dirs, matching: Self.metricPatterns, except: Self.bridges)
        #expect(hits.isEmpty, "\(hits.count) radius or font-size literals:\n\(hits.joined(separator: "\n"))")
    }

    @Test func theScanSeesEachForbiddenForm() throws {
        let bad = [
            "NSColor.red.setFill()",
            "Text(x).foregroundStyle(Color.blue)",
            "let c = Color(red: 1, green: 0, blue: 0)",
            "let c = NSColor(calibratedRed: 1, green: 0, blue: 0, alpha: 1)",
            ".background(Color.accentColor)",
            "return .red",
            #"case "green": .green"#,
            "let lanes: [NSColor] = [.systemBlue, .systemOrange]",
            "field.textColor = .labelColor",
            "RoundedRectangle(cornerRadius: 4)",
            ".font(.system(size: 11, weight: .medium))",
            "label.font = .systemFont(ofSize: NSFont.smallSystemFontSize)",
        ]
        let all = Self.colourPatterns + Self.metricPatterns
        for line in bad {
            #expect(try SourceScan.matches(line, patterns: all) >= 1, "\(line)")
        }
        #expect(try SourceScan.matches(bad[7], patterns: all) == 2)
        #expect(try SourceScan.matches(bad[11], patterns: all) == 2)
        #expect(try SourceScan.matches("theme.state.error.color", patterns: all) == 0)
        #expect(try SourceScan.matches("NSColor.clear.setFill()", patterns: all) == 0)
    }

    @Test func anEditorRedrawsWhenItsThemeChanges() throws {
        let canvas = EditorCanvas(frame: NSRect(x: 0, y: 0, width: 100, height: 100))
        canvas.wantsLayer = true
        // AppKit keeps `needsDisplay` only for a view in a window (outside
        // one it always reads false), so the canvas is in one, never shown.
        let w = NSWindow(contentRect: canvas.frame, styleMask: [.titled], backing: .buffered, defer: true)
        w.isReleasedWhenClosed = false
        w.contentView = canvas
        canvas.needsDisplay = false
        canvas.theme = .console
        #expect(canvas.needsDisplay)
        let bg = try #require(canvas.layer?.backgroundColor)
        let shown = try #require(NSColor(cgColor: bg))
        #expect(shown.usingColorSpace(.sRGB) == Theme.console.canvas.usingColorSpace(.sRGB))
    }

    /// B4 review M1: a themed view made after the last theme change starts
    /// in the current theme, not Classic Light.
    @Test func aThemedViewMadeLaterStartsInTheCurrentTheme() {
        let before = ThemedViews.current
        defer { ThemedViews.current = before }
        ThemedViews.current = .studioDark
        let canvas = EditorCanvas(frame: NSRect(x: 0, y: 0, width: 100, height: 100))
        #expect(canvas.theme == .studioDark)
        #expect(canvas.content.theme == .studioDark && canvas.ruler.theme == .studioDark)
        #expect(ListCellField().theme == .studioDark)
        #expect(KeyRecorderView().theme == .studioDark)
        let bg = canvas.layer?.backgroundColor.flatMap { NSColor(cgColor: $0) }
        #expect(bg?.usingColorSpace(.sRGB) == Theme.studioDark.canvas.usingColorSpace(.sRGB))
    }

    /// The editor roles (Ruling 13's table) and the lane colours.
    @Test func theEditorRolesAreTheTablesTokens() {
        let t = Theme.studioDark
        func same(_ a: NSColor, _ b: ThemeColor) -> Bool {
            a.usingColorSpace(.sRGB) == b.nsColor.usingColorSpace(.sRGB)
        }
        #expect(same(t.canvas, t.surface.inset) && same(t.rulerBackground, t.surface.panel))
        #expect(same(t.label, t.text.primary) && same(t.secondaryLabel, t.text.secondary))
        #expect(same(t.regionFill, t.graph.fill) && same(t.playhead, t.state.record))
        #expect(same(t.whiteKey, t.surface.raised) && same(t.blackKey, t.text.primary))
        #expect(same(t.laneColour(0), t.swatch.red) && same(t.laneColour(7), t.swatch.pink))
        #expect(same(t.laneColour(9), t.swatch.orange) && same(t.laneColour(-1), t.swatch.pink))
    }
    /// The B4 run's findings, in pixels and colours: the ruler band takes
    /// the theme's ruler background (it stayed black in Classic Light),
    /// and a region's name is the theme's text, not white on a pale fill.
    @Test func theRulerBandAndRegionNamesAreInTheTheme() throws {
        for t in [Theme.classicLight, .studioDark] {
            let c = EditorCanvas(frame: NSRect(x: 0, y: 0, width: 200, height: 100))
            c.theme = t
            c.layout()
            let shown = try #require(ThemePixels.at(c.ruler, x: 150, y: 1))
            #expect(ThemePixels.near(shown, t.rulerBackground), "\(t.id): \(shown)")
            let name = RegionDrawing.stripLabelAttributes(font: t.type.caption.nsFont, offline: false, theme: t)
            let colour = try #require(name[.foregroundColor] as? NSColor)
            #expect(ThemePixels.near(colour, t.label), "\(t.id)")
        }
        let light = RegionDrawing.stripLabelAttributes(
            font: Theme.classicLight.type.caption.nsFont, offline: false, theme: .classicLight)
        let ink = try #require((light[.foregroundColor] as? NSColor)?.usingColorSpace(.sRGB))
        #expect(ink.redComponent < 0.2, "dark text on Classic Light's pale region fill")
    }
}

/// Reading a view's drawn pixels, for the theme tests.
@MainActor
enum ThemePixels {
    /// The sRGB colour `v` draws at `x`, `y` (`y` from the top).
    static func at(_ v: NSView, x: Int, y: Int) -> NSColor? {
        guard let rep = v.bitmapImageRepForCachingDisplay(in: v.bounds) else { return nil }
        v.cacheDisplay(in: v.bounds, to: rep)
        let sx = Int(CGFloat(x) * CGFloat(rep.pixelsWide) / max(v.bounds.width, 1))
        let sy = Int(CGFloat(y) * CGFloat(rep.pixelsHigh) / max(v.bounds.height, 1))
        return rep.colorAt(x: sx, y: sy)?.usingColorSpace(.sRGB)
    }

    /// Within a few percent, in sRGB.
    static func near(_ a: NSColor, _ b: NSColor) -> Bool {
        guard let a = a.usingColorSpace(.sRGB), let b = b.usingColorSpace(.sRGB) else { return false }
        return abs(a.redComponent - b.redComponent) < 0.04 && abs(a.greenComponent - b.greenComponent) < 0.04
            && abs(a.blueComponent - b.blueComponent) < 0.04
    }
}
