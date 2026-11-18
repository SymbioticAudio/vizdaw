import Foundation
import Testing
@testable import VizDAWAppModel
import VizDAWCore

/// Window › Song Notes (plan 8b Task 5, Ruling 4): the menu makes the notes
/// file and opens it with the composer's editor; over a channel it only
/// answers the path and opens nothing.
@MainActor
@Suite struct SongNotesTests {
    let host = FakeControlHost()

    func demo() throws -> (DocumentStore, UUID, DocumentModel, FakeSession) {
        let f = demoFake()
        f.coNotes = NotesRow(text: "# Song\n", truncated: false, path: "/tmp/Song.vizdaw/Notes.md")
        let s = DocumentStore(makeEmpty: { FakeSession([]) }, makeDemo: { f }, open: { _ in demoFake() })
        let id = try #require(s.newDemo())
        let m = try #require(s.model(id))
        return (s, id, m, f)
    }

    @Test func opensTheNotesFile() throws {
        let (_, _, m, f) = try demo()
        var opened: [URL] = []
        let alert = SongNotes.open(m) { url in
            opened.append(url)
            return true
        }
        #expect(alert == nil)
        #expect(opened == [URL(fileURLWithPath: "/tmp/Song.vizdaw/Notes.md")])
        #expect(f.coLog == ["notes path"])
    }

    @Test func noAppOpenedItSaysWhereItIs() throws {
        let (_, _, m, _) = try demo()
        let alert = SongNotes.open(m) { _ in false }
        #expect(alert == "No app opened Notes.md. It is at /tmp/Song.vizdaw/Notes.md.")
    }

    @Test func aRefusedNotesFileOpensNothing() throws {
        let (_, _, m, f) = try demo()
        f.coNotesError = .Model(message: "Notes.md is not a plain file, so Co-Producer leaves it alone.")
        var opened = 0
        let alert = SongNotes.open(m) { _ in
            opened += 1
            return true
        }
        #expect(alert == "Song notes could not be opened: Notes.md is not a plain file, so Co-Producer leaves it alone.")
        #expect(opened == 0)
    }

    @Test func overTheChannelItOpensNothing() throws {
        let (s, id, _, f) = try demo()
        // The channel's path takes no opener: it can launch nothing.
        let o = ControlCommands.run(SongNotes.command, args: [], doc: s.model(id).map { (id, $0) }, store: s, host: host)
        guard case .done(.ran(let command, let doc, _, _, let status)) = o else {
            Issue.record("not done: \(o)")
            return
        }
        #expect(command == SongNotes.command)
        #expect(doc == id.uuidString)
        #expect(status == "/tmp/Song.vizdaw/Notes.md")
        #expect(f.coLog == ["notes path"])
        #expect(s.model(id)?.alert == nil)
        // A refused file is the call's failure, with no alert.
        f.coNotesError = .Model(message: "Notes.md is not a plain file, so Co-Producer leaves it alone.")
        let refused = ControlCommands.run(
            SongNotes.command, args: [], doc: s.model(id).map { (id, $0) }, store: s, host: host)
        guard case .failed(let failure) = refused else {
            Issue.record("not refused: \(refused)")
            return
        }
        #expect(failure.code == .failed)
        #expect(failure.message == "Notes.md is not a plain file, so Co-Producer leaves it alone.")
        #expect(s.model(id)?.alert == nil)
        // No document: nothing to answer.
        let none = ControlCommands.run(SongNotes.command, args: [], doc: nil, store: s, host: host)
        guard case .failed(let f2) = none else {
            Issue.record("answered with no document")
            return
        }
        #expect(f2.code == .noDocument)
    }
}
