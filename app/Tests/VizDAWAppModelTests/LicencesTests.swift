import Foundation
import Testing
@testable import VizDAWAppModel

/// VizDAW › Licences… (release gate Task 4): the bundle's Licences folder,
/// VizDAW's own licence first, then the third-party notices, then Multi-Q's.
@MainActor @Suite struct LicencesTests {
    func folder(_ files: [String: String]) throws -> URL {
        let dir = FileManager.default.temporaryDirectory
            .appendingPathComponent("vizdaw-licences-\(UUID().uuidString)", isDirectory: true)
        for (path, text) in files {
            let url = dir.appendingPathComponent(path)
            try FileManager.default.createDirectory(
                at: url.deletingLastPathComponent(), withIntermediateDirectories: true)
            try text.write(to: url, atomically: true, encoding: .utf8)
        }
        return dir
    }

    @Test func licences_list_names_the_three_sources() throws {
        let dir = try folder([
            "Multi-Q/x.txt": "multi-q text",
            "THIRD-PARTY.txt": "third-party text",
            "LICENSE": "GNU GENERAL PUBLIC LICENSE",
        ])
        defer { try? FileManager.default.removeItem(at: dir) }
        let model = LicencesModel(folder: dir)
        #expect(model.entries.map(\.id) == ["LICENSE", "THIRD-PARTY.txt", "Multi-Q/x.txt"])
        #expect(model.entries.map(\.title) == ["VizDAW (GPL-3.0-only)", "Third-party notices", "Multi-Q: x.txt"])
        #expect(model.note == nil)
        // The first file is selected and shown; VizDAW's own carries the
        // bundled plug-in's AGPLv3 line above the GPL text.
        #expect(model.selection == "LICENSE")
        #expect(model.text.hasPrefix(LicencesModel.productLine))
        #expect(model.text.contains(LicencesModel.agplLine))
        #expect(model.text.contains("GNU GENERAL PUBLIC LICENSE"))
        model.selection = "Multi-Q/x.txt"
        #expect(model.text == "multi-q text")
        model.selection = "THIRD-PARTY.txt"
        #expect(model.text == "third-party text")
    }

    @Test func missing_folder_shows_a_plain_note() {
        let dir = FileManager.default.temporaryDirectory
            .appendingPathComponent("vizdaw-licences-missing-\(UUID().uuidString)", isDirectory: true)
        let model = LicencesModel(folder: dir)
        #expect(model.entries.isEmpty)
        #expect(model.selection == nil)
        let note = try? #require(model.note)
        #expect(note?.isEmpty == false)
        #expect(model.text == note)
        #expect(LicencesModel(folder: nil).note == note)
    }

    @Test func the_agpl_line_is_the_owners_wording() {
        #expect(
            LicencesModel.agplLine
                == "VizDAW bundles Dusk Audio Multi-Q, which includes parts under the GNU AGPLv3 (JUCE).")
    }

    @Test func an_unreadable_file_says_so() throws {
        let dir = try folder(["THIRD-PARTY.txt": "x"])
        defer { try? FileManager.default.removeItem(at: dir) }
        let model = LicencesModel(folder: dir)
        try FileManager.default.removeItem(at: dir.appendingPathComponent("THIRD-PARTY.txt"))
        model.selection = "THIRD-PARTY.txt"
        #expect(model.text == LicencesModel.unreadable)
    }
}
