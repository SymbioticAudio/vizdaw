import Foundation
import Testing
@testable import VizDAWAppModel

/// What the app shows names no third-party product (plan 7d Ruling 10,
/// Task B8). The pattern is the owner's list, case-sensitive: `kontrol_mk3`
/// (an id) and `KONTROL` (the name a device reports) are not names.
@Suite struct NameScanTests {
    static let dirs = ["VizDAW", "VizDAWEditors", "VizDAWAppModel"]
    static let pattern =
        #"\b(Opcode|Studio Vision|SVP|Logic|Apple|Midas|Soundcraft|Steinberg|Cubase|NI|Native Instruments|Komplete|Kontrol|Microsoft)\b"#
    /// The one listed exception: a smoke-run plug-in reference, never shown.
    static let smokeLine = #"vendor: "Apple""#

    /// The string literals on a line that is not a comment, up to a `//`
    /// outside a string.
    static func literals(_ line: String) -> [String] {
        if line.trimmingCharacters(in: .whitespaces).hasPrefix("//") { return [] }
        var out: [String] = []
        var current: String?
        var escaped = false
        var prev: Character = " "
        for ch in line {
            if var s = current {
                if escaped {
                    escaped = false
                    s.append(ch)
                } else if ch == "\\" {
                    escaped = true
                } else if ch == "\"" {
                    out.append(s)
                    current = nil
                    prev = ch
                    continue
                } else {
                    s.append(ch)
                }
                current = s
            } else if ch == "\"" {
                current = ""
            } else if ch == "/" && prev == "/" {
                return out
            }
            prev = ch
        }
        return out
    }

    static func matches(_ text: String, _ regex: NSRegularExpression) -> Bool {
        regex.numberOfMatches(in: text, range: NSRange(text.startIndex..., in: text)) > 0
    }

    @Test func noThirdPartyNamesInWhatTheAppShows() throws {
        let regex = try NSRegularExpression(pattern: Self.pattern)
        // Self-checks: the literal reader and the pattern.
        #expect(Self.literals(#"Text("a \"b\" c") // "Kontrol""#) == [#"a "b" c"#])
        #expect(Self.literals(#"    /// the "Kontrol" keyboard"#).isEmpty)
        #expect(Self.literals(#"f("x", "NI's")"#) == ["x", "NI's"])
        #expect(Self.literals("let kontrolMk3 = 1").isEmpty)
        for bad in ["NI's keyboard", "NI x", "x NI", "a Kontrol keyboard", "Native Instruments GmbH"] {
            #expect(Self.matches(bad, regex), "\(bad)")
        }
        for clean in ["kontrol_mk3", "KONTROL S88 MK3 DAW", "NIGHT", "a host-integration keyboard"] {
            #expect(!Self.matches(clean, regex), "\(clean)")
        }
        // The kept ids still parse, through the path `surface.add` uses.
        #expect(SurfaceCommands.parseKind("kontrol_mk3") == .kontrolMk3)
        var hits: [String] = []
        for dir in Self.dirs {
            let root = SourceScan.sourcesRoot.appendingPathComponent(dir, isDirectory: true)
            let walker = try #require(FileManager.default.enumerator(at: root, includingPropertiesForKeys: nil))
            var files: [URL] = []
            for case let url as URL in walker where url.pathExtension == "swift" { files.append(url) }
            #expect(!files.isEmpty, "\(dir) has sources")
            for url in files.sorted(by: { $0.path < $1.path }) {
                let text = try String(contentsOf: url, encoding: .utf8)
                for (i, line) in text.components(separatedBy: "\n").enumerated() {
                    for lit in Self.literals(line)
                    where Self.matches(lit, regex)
                        && !(url.lastPathComponent == "VizDAWApp.swift" && line.contains(Self.smokeLine))
                    {
                        hits.append("\(dir)/\(url.lastPathComponent):\(i + 1): \(lit)")
                    }
                }
            }
        }
        #expect(hits.isEmpty, "\(hits.joined(separator: "\n"))")
    }
}
