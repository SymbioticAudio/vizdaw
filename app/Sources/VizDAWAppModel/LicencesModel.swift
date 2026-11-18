import Foundation
import Observation

/// VizDAW › Licences… (release gate Task 4): the files in the bundle's
/// `Contents/Resources/Licences` folder. VizDAW's own `LICENSE` first, then
/// `THIRD-PARTY.txt` (scripts/third-party-notices.sh), then every file under
/// `Multi-Q/` (scripts/build-multiq.sh), and the text of the selected one.
@Observable
@MainActor
public final class LicencesModel {
    /// One file of the folder; `id` is its path inside the folder.
    public struct Entry: Identifiable, Equatable, Sendable {
        public let id: String
        public let title: String
        public let url: URL
    }

    /// How VizDAW's own licence is described, wherever the product's
    /// licence is named.
    public static let productLine = "VizDAW is licensed under the GNU GPL, version 3 only (GPL-3.0-only)."
    /// The owner's wording for the bundled plug-in's AGPLv3 part.
    public static let agplLine =
        "VizDAW bundles Dusk Audio Multi-Q, which includes parts under the GNU AGPLv3 (JUCE)."
    public static let missing =
        "The licence files are not in this build. VizDAW.app carries them in Contents/Resources/Licences."
    public static let unreadable = "This file could not be read."

    public let entries: [Entry]
    /// Set when the folder is missing or holds none of the files.
    public let note: String?
    /// The shown file (`Entry.id`).
    public var selection: String?
    @ObservationIgnored private var cache: [String: String] = [:]

    /// `Contents/Resources/Licences` of the running app, if it has one.
    public static var bundled: URL? {
        Bundle.main.resourceURL?.appendingPathComponent("Licences", isDirectory: true)
    }

    public init(folder: URL?) {
        let found = folder.map(Self.scan) ?? []
        entries = found
        note = found.isEmpty ? Self.missing : nil
        selection = found.first?.id
    }

    /// The selected file's text; the note when there is none.
    public var text: String {
        guard let selection, let entry = entries.first(where: { $0.id == selection }) else {
            return note ?? ""
        }
        if let hit = cache[selection] { return hit }
        var body = (try? String(contentsOf: entry.url, encoding: .utf8)) ?? Self.unreadable
        if entry.id == "LICENSE", body != Self.unreadable {
            body = Self.productLine + "\n" + Self.agplLine + "\n\n" + body
        }
        if body != Self.unreadable { cache[selection] = body }
        return body
    }

    private static func scan(_ folder: URL) -> [Entry] {
        let fm = FileManager.default
        var out: [Entry] = []
        let license = folder.appendingPathComponent("LICENSE")
        if fm.fileExists(atPath: license.path) {
            out.append(Entry(id: "LICENSE", title: "VizDAW (GPL-3.0-only)", url: license))
        }
        let notices = folder.appendingPathComponent("THIRD-PARTY.txt")
        if fm.fileExists(atPath: notices.path) {
            out.append(Entry(id: "THIRD-PARTY.txt", title: "Third-party notices", url: notices))
        }
        let multiq = folder.appendingPathComponent("Multi-Q", isDirectory: true)
        if let walk = fm.enumerator(at: multiq, includingPropertiesForKeys: [.isRegularFileKey]) {
            var files: [(String, URL)] = []
            for case let url as URL in walk {
                guard (try? url.resourceValues(forKeys: [.isRegularFileKey]))?.isRegularFile == true else { continue }
                let rel = url.resolvingSymlinksInPath().path.dropFirst(multiq.resolvingSymlinksInPath().path.count + 1)
                files.append((String(rel), url))
            }
            for (rel, url) in files.sorted(by: { $0.0.localizedStandardCompare($1.0) == .orderedAscending }) {
                out.append(Entry(id: "Multi-Q/" + rel, title: "Multi-Q: " + rel, url: url))
            }
        }
        return out
    }
}
