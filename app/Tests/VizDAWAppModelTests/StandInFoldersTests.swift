import Foundation
import Testing
@testable import VizDAWAppModel
import VizDAWCore

/// Fakes the core's list and delete: nothing here touches a folder.
@MainActor
@Suite struct StandInFoldersTests {
    let support = URL(fileURLWithPath: "/tmp/vizdaw-stand-ins-test")

    private func model(_ rows: [StandIn], open: [String] = []) -> StandInFolders {
        let m = StandInFolders(supportDir: support, openIDs: { open })
        m.list = { rows }
        return m
    }

    @Test func line_hidden_when_none() {
        let m = model([])
        m.refresh()
        #expect(m.line == nil)
        // Songs open right now do not count either.
        let open = model([StandIn(id: "a", bytes: 10, modified: 1)], open: ["a"])
        open.refresh()
        #expect(open.line == nil)
    }

    @Test func line_counts_and_sizes() {
        let m = model(
            [StandIn(id: "a", bytes: 1_500_000, modified: 1), StandIn(id: "b", bytes: 500_000, modified: 2),
             StandIn(id: "c", bytes: 9, modified: 3)],
            open: ["c"])
        m.refresh()
        let size = ByteCountFormatter.string(fromByteCount: 2_000_000, countStyle: .file)
        #expect(m.line == "Notes and copies kept for 2 unsaved songs (\(size))")
        #expect(m.confirmMessage == "Delete the notes and copies Co-Producer kept for 2 songs that were never saved?")
    }

    @Test func deleteAsksWithTheOpenSongsAndReads() {
        let m = model([StandIn(id: "a", bytes: 1, modified: 1)], open: ["z"])
        var asked: [[String]] = []
        m.delete = { asked.append($0); return 1 }
        m.refresh()
        m.confirming = true
        m.deleteAll()
        #expect(asked == [["z"]])
        #expect(!m.confirming)
    }

    @Test func aFailedDeleteIsSaid() {
        let m = model([StandIn(id: "a", bytes: 1, modified: 1)])
        m.delete = { _ in throw VizError.Model(message: "nope") }
        m.refresh()
        m.deleteAll()
        #expect(m.problem?.hasPrefix("Could not delete") == true)
    }

    @Test func anIdReadFailureDeletesNothing() {
        let m = StandInFolders(supportDir: support, openIDs: { throw VizError.Model(message: "busy") })
        m.list = { [StandIn(id: "a", bytes: 1, modified: 1)] }
        var asked = 0
        m.delete = { _ in asked += 1; return 1 }
        m.deleteAll()
        #expect(asked == 0)
        #expect(m.problem == "Couldn't check which songs are open, so nothing was deleted.")
        #expect(m.line == nil)
    }
}
