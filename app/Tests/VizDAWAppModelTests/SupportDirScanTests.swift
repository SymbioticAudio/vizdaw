import Foundation
import Testing

/// Swift links the core without its test hooks, so a real session or a
/// free function that uses the support folder before `setSupportDir` would
/// write into the user's real one (plan 6d Task 12 fix round 2). Every test
/// suite that reaches the real library that way names `TestSupportDir`.
@Suite struct SupportDirScanTests {
    /// The real library's calls that make a session or use the support
    /// folder (crash markers, recovery, surfaces).
    static let realCalls = try! NSRegularExpression(
        pattern: #"Session\.(new[A-Za-z]*|open)\(|recoveryCheck\(|recoverUntitledRecordings\(|surface(sLoad|sState|Add|List|Remove|Ports)\(|DocumentStore\.live\("#)

    /// The test files, from this file's path.
    static var testsRoot: URL { URL(fileURLWithPath: #filePath).deletingLastPathComponent().deletingLastPathComponent() }

    /// Every chunk of `text` from one `@Suite` to the next (the text before
    /// the first one too) that makes a real call but never names
    /// `TestSupportDir`.
    static func unguarded(_ text: String) -> [String] {
        text.components(separatedBy: "@Suite").compactMap { chunk in
            let real = chunk.split(separator: "\n").contains { line in
                let l = String(line)
                let range = NSRange(l.startIndex..., in: l)
                return realCalls.matches(in: l, range: range).contains { m in
                    guard let r = Range(m.range, in: l) else { return false }
                    let before = l[..<r.lowerBound]
                    // A declaration (`func surfaceList(`) or a fake's method
                    // (`f.surfaceList()`) is not the real library;
                    // `try Session.newDemo()` is.
                    if before.hasSuffix("func ") { return false }
                    return l[r].hasPrefix("Session.") || l[r].hasPrefix("DocumentStore.") || !before.hasSuffix(".")
                }
            }
            guard real, !chunk.contains("TestSupportDir") else { return nil }
            return String(chunk.prefix(120))
        }
    }

    @Test func everyRealLibrarySuiteSetsTheTestSupportFolder() throws {
        let fm = FileManager.default
        let walker = try #require(fm.enumerator(at: Self.testsRoot, includingPropertiesForKeys: nil))
        var scanned = 0
        var missing: [String] = []
        for case let url as URL in walker where url.pathExtension == "swift" {
            guard !["SupportDirScanTests.swift", "TestSupportDir.swift", "FakeSession.swift"].contains(url.lastPathComponent)
            else { continue }
            let text = try String(contentsOf: url, encoding: .utf8)
            scanned += 1
            missing += Self.unguarded(text).map { "\(url.lastPathComponent): \($0)" }
        }
        #expect(scanned > 50, "the scan found the test files")
        #expect(missing.isEmpty, "\(missing)")
    }

    @Test func theScanSeesAnUnguardedSuite() {
        let bad = "@Suite struct A {\n  @Test func t() throws { _ = try Session.newDemo() }\n}\n"
        #expect(Self.unguarded(bad).count == 1)
        #expect(Self.unguarded(bad.replacingOccurrences(of: "{\n", with: "{\n init() { _ = TestSupportDir.shared }\n")).isEmpty)
        #expect(Self.unguarded("@Suite struct B { func f() { _ = fake.surfaceList() } }").isEmpty)
    }
}
