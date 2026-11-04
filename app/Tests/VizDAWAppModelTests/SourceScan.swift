import Foundation

/// Scans the app's own sources (`app/Sources/<dir>/**/*.swift`, found from
/// this file's path) for lines matching a regular expression (plan 7b Task B3).
enum SourceScan {
    struct ScanError: Error, CustomStringConvertible {
        let description: String
    }

    /// `app/Sources`, from `app/Tests/VizDAWAppModelTests/SourceScan.swift`.
    static var sourcesRoot: URL {
        URL(fileURLWithPath: #filePath)
            .deletingLastPathComponent()  // VizDAWAppModelTests
            .deletingLastPathComponent()  // Tests
            .deletingLastPathComponent()  // app
            .appendingPathComponent("Sources", isDirectory: true)
    }

    /// `"file:line: text"` for every line of `dirs`' Swift files that matches
    /// `pattern`, files in path order.
    static func lines(in dirs: [String], matching pattern: String) throws -> [String] {
        let regex = try NSRegularExpression(pattern: pattern)
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
                let text = try String(contentsOf: url, encoding: .utf8)
                let name = url.path.replacingOccurrences(of: sourcesRoot.path + "/", with: "")
                for (i, line) in text.components(separatedBy: "\n").enumerated() {
                    let range = NSRange(line.startIndex..., in: line)
                    if regex.firstMatch(in: line, range: range) != nil {
                        hits.append("\(name):\(i + 1): \(line.trimmingCharacters(in: .whitespaces))")
                    }
                }
            }
        }
        return hits
    }
}
