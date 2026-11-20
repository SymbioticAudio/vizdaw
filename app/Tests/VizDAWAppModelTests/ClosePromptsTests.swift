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

    /// Next small fixes Task 6 (R6): Discard Changes still saves a song
    /// whose Save waits for its recording pass, and the quit alert says so.
    @Test func quit_alert_names_a_song_still_saved_after_its_pass() {
        let t = ClosePrompts.quitAlertText(discardable: ["A"], savingAfterPass: ["B"])
        #expect(t.message == ClosePrompts.quitAlertText(count: 2))
        #expect(t.info.contains("\u{201C}B\u{201D}"))
        #expect(t.info.contains("will still be saved when its recording ends"))
        #expect(!t.info.contains("\u{201C}A\u{201D}"))
        let two = ClosePrompts.quitAlertText(discardable: [], savingAfterPass: ["B", "C"])
        #expect(two.message == ClosePrompts.quitAlertText(count: 2))
        #expect(two.info.contains("\u{201C}B\u{201D}") && two.info.contains("\u{201C}C\u{201D}"))
        #expect(two.info.contains("will still be saved when their recordings end"))
    }

    @Test func quit_alert_with_no_song_waiting_is_todays() {
        let t = ClosePrompts.quitAlertText(discardable: ["A", "B", "C"], savingAfterPass: [])
        #expect(t.message == ClosePrompts.quitAlertText(count: 3))
        #expect(t.info == "")
    }

    /// R7: `app.quit`'s refusal names the unsaved songs.
    @Test func quit_refusal_names_the_songs() {
        let one = ClosePrompts.quitRefusal(titles: ["Song"])
        #expect(one.hasPrefix("1 song has unsaved changes: \u{201C}Song\u{201D}."))
        #expect(one.contains("discard: true"))
        let two = ClosePrompts.quitRefusal(titles: ["A", "B"])
        #expect(two.hasPrefix("2 songs have unsaved changes: \u{201C}A\u{201D}, \u{201C}B\u{201D}."))
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

    @Test func a_window_gone_mid_walk_ends_cancelled() {
        var walk = QuitWalk([a, b])
        walk.answer(.dontSave)
        walk.windowGone()
        #expect(walk.result == .cancel)
        #expect(walk.current == nil)
        #expect(walk.closed == [a])
        #expect(walk.open == [b])
    }

    @Test func pending_save_after_pass_offers_no_dont_save() {
        let t = ClosePrompts.sheetText(name: "Take", savePending: true)
        #expect(t.message == "Do you want to save the changes you made to \u{201C}Take\u{201D}?")
        #expect(t.informative == "Recording is still going. The take and your changes will be saved when it stops.")
        #expect(!t.offersDontSave)
        let plain = ClosePrompts.sheetText(name: "Take", savePending: false)
        #expect(plain.informative == "Your changes will be lost if you don't save them.")
        #expect(plain.offersDontSave)
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
