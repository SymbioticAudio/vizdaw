import Foundation
import Testing
@testable import VizDAWAppModel
import VizDAWCore

/// Release gate B1: closing or quitting with unsaved changes asks first;
/// programmatic closes never do.
@MainActor
struct ClosePromptsTests {
    let a = UUID(), b = UUID(), c = UUID()

    @Test func clean_documents_quit_now() {
        #expect(ClosePrompts.quitStep(dirty: [], testHooks: false) == .quitNow)
    }

    @Test func one_dirty_document_asks_one() {
        #expect(ClosePrompts.quitStep(dirty: [a], testHooks: false) == .askOne(a))
    }

    @Test func several_dirty_documents_ask_many() {
        #expect(ClosePrompts.quitStep(dirty: [a, b, c], testHooks: false) == .askMany(count: 3))
        #expect(
            ClosePrompts.quitAlertText(count: 3)
                == "You have 3 VizDAW documents with unsaved changes. Do you want to review these changes before quitting?")
    }

    @Test func test_hooks_quit_now_even_when_dirty() {
        #expect(ClosePrompts.quitStep(dirty: [a], testHooks: true) == .quitNow)
        #expect(ClosePrompts.quitStep(dirty: [a, b], testHooks: true) == .quitNow)
    }

    @Test func sheet_text_names_the_song() {
        let t = ClosePrompts.sheetText(name: "My Song")
        #expect(t.message == "Do you want to save the changes you made to \u{201C}My Song\u{201D}?")
        #expect(t.informative == "Your changes will be lost if you don't save them.")
    }

    @Test func cancel_at_the_second_song_stops_the_quit() {
        var walk = QuitWalk([a, b, c])
        #expect(walk.current == a)
        walk.answer(.dontSave)
        #expect(walk.current == b)
        walk.answer(.cancel)
        #expect(walk.result == .cancel)
        #expect(walk.current == nil)
        #expect(walk.closed == [a])
        #expect(walk.open == [b, c])
    }

    @Test func a_walk_with_every_song_answered_quits() {
        var walk = QuitWalk([a, b])
        walk.answer(.save, saved: true)
        walk.answer(.dontSave)
        #expect(walk.result == .quitNow)
        #expect(walk.closed == [a, b])
        #expect(walk.open.isEmpty)
    }

    @Test func cancelled_save_panel_keeps_the_window() {
        var walk = QuitWalk([a])
        walk.answer(.save, saved: false)
        #expect(walk.result == .cancel)
        #expect(walk.closed.isEmpty)
        #expect(walk.open == [a])
    }

    @Test func control_close_never_prompts() throws {
        let f = demoFake()
        let s = DocumentStore(makeEmpty: { FakeSession([]) }, makeDemo: { f }, open: { _ in demoFake() })
        let id = try #require(s.newDemo())
        let m = try #require(s.model(id))
        f.snap.dirty = true
        #expect(m.hasUnsavedChanges)
        let host = FakeControlHost()
        let r = ControlCommands.run("file.close", args: [], doc: (id, m), store: s, host: host)
        guard case .done = r else {
            Issue.record("file.close did not run: \(r)")
            return
        }
        // The document went at once, unsaved, and the window was closed
        // without its prompt.
        #expect(s.model(id) == nil)
        #expect(host.calls == ["close"])
    }

    @Test func has_unsaved_changes_reads_the_session() throws {
        let f = demoFake()
        let m = try DocumentModel(session: f)
        #expect(!m.hasUnsavedChanges)
        f.snap.dirty = true
        #expect(m.hasUnsavedChanges)
    }
}
