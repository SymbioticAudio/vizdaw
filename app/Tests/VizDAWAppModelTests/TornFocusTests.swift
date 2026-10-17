import Foundation
import Testing
import VizDAWCore
@testable import VizDAWAppModel

@MainActor
@Suite struct TornFocusTests {
    func store() -> DocumentStore {
        DocumentStore(makeEmpty: { editorFake() }, makeDemo: { editorFake() }, open: { _ in editorFake() })
    }

    @Test func aKeyTornWindowIsTheCommandTarget() throws {
        let s = store()
        let a = try #require(s.newEmpty())
        let b = try #require(s.newEmpty())
        let w = UUID()
        #expect(s.commandTarget(focused: b) == b)
        s.focusTorn(TornFocus(doc: a, window: w))
        #expect(s.commandTarget(focused: b) == a, "the torn window of a is key")
        s.blurTorn(window: w)
        #expect(s.commandTarget(focused: b) == b)
    }

    @Test func blurringAnotherWindowKeepsTheFocus() throws {
        let s = store()
        let a = try #require(s.newEmpty())
        let w = UUID()
        s.focusTorn(TornFocus(doc: a, window: w))
        s.blurTorn(window: UUID())
        #expect(s.tornFocus == TornFocus(doc: a, window: w))
    }

    @Test func closingTheDocumentClearsTheFocus() throws {
        let s = store()
        let a = try #require(s.newEmpty())
        s.focusTorn(TornFocus(doc: a, window: UUID()))
        s.close(a)
        #expect(s.tornFocus == nil)
        #expect(s.commandTarget(focused: nil) == nil)
    }

    @Test func tearingOffFromAWindowCopiesItsTarget() throws {
        let f = editorFake()
        let m = try DocumentModel(session: f)
        m.editor.openDetail(.lane(seq: 3, lane: 31))
        let w = try #require(m.editor.tearOff(.pianoRoll, shown: 3))
        m.editor.closeDetail()
        let w2 = try #require(m.editor.tearOff(.list, from: w.id))
        #expect(w2.detail == DetailState(kind: .list, target: .lane(seq: 3, lane: 31)))
        #expect(m.editor.layout.windows.map(\.id) == [w.id, w2.id])
        #expect(m.editor.tearOff(.list, from: UUID()) == nil)
    }
}
