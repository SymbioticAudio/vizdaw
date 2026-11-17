import Foundation
import Testing
@testable import VizDAWAppModel
import VizDAWCore

/// Compact's offer to remove old Co-Producer copies (plan 8b Ruling 8):
/// only from the Media Pool's buttons, none ticked, and only what the
/// composer ticks goes. The fake session moves nothing, so no test reaches
/// the Trash.
@MainActor
@Suite struct CopiesOfferTests {
    let host = FakeControlHost()

    let two = [
        CopyRow(name: "2026-11-14 15-02 before Drum Bus.vizdaw", path: "/x/a", bytes: 1_200_000),
        CopyRow(name: "2026-11-15 09-30 before Strings.vizdaw", path: "/x/b", bytes: 1_249_999),
    ]

    func demo() throws -> (DocumentStore, UUID, DocumentModel, FakeSession) {
        let f = demoFake()
        let s = DocumentStore(makeEmpty: { FakeSession([]) }, makeDemo: { f }, open: { _ in demoFake() })
        let id = try #require(s.newDemo())
        let m = try #require(s.model(id))
        return (s, id, m, f)
    }

    @Test func noCopiesNoOffer() {
        #expect(CopiesOffer(copies: []) == nil)
    }

    @Test func noneChosenAtFirst() throws {
        let offer = try #require(CopiesOffer(copies: two))
        #expect(offer.rows.map(\.id) == two.map(\.name))
        #expect(offer.rows.allSatisfy { !$0.chosen })
        #expect(!offer.canRemove)
        #expect(offer.rows.map(\.size) == ["about 1.2 MB", "about 1.2 MB"])
        #expect(offer.message == "Compact Media is done. This song also keeps 2 copies Co-Producer saved before its "
            + "requests. Choose any you no longer need: they go to the Trash, and the files only they use go when "
            + "you next save.")
        offer.toggle(two[0].name)
        #expect(offer.canRemove)
        offer.toggle(two[0].name)
        #expect(!offer.canRemove)
        // An unknown id changes nothing.
        offer.toggle("../elsewhere")
        #expect(!offer.canRemove)
    }

    @Test func sizesReadAsAnEstimate() {
        #expect(CopiesOffer.size(0) == "about 1 KB")
        #expect(CopiesOffer.size(12_400) == "about 13 KB")
        #expect(CopiesOffer.size(12_400_000) == "about 12.4 MB")
        #expect(CopiesOffer.size(3_060_000_000) == "about 3.1 GB")
    }

    @Test func removeSendsOnlyTheChosen() throws {
        let (_, _, m, f) = try demo()
        f.coCopies = two
        let offer = try #require(CopiesOffer(copies: two))
        offer.toggle(two[1].name)
        let problems = offer.remove(m.session)
        #expect(f.coRemoved == [[two[1].name]])
        #expect(problems.isEmpty)
        // The session's problem lines come back to show.
        f.coRemoveProblems = ["Not a copy of this song: x."]
        let again = try #require(CopiesOffer(copies: [two[0]]))
        again.toggle(two[0].name)
        #expect(again.remove(m.session) == ["Not a copy of this song: x."])
        // None chosen: nothing is asked of the session.
        f.coRemoved = []
        let none = try #require(CopiesOffer(copies: two))
        #expect(none.remove(m.session).isEmpty)
        #expect(f.coRemoved.isEmpty)
    }

    @Test func keepAllRemovesNothing() throws {
        let (_, _, m, f) = try demo()
        f.coCopies = two
        #expect(m.media.compactFromMenu(channelOn: false))
        let offer = try #require(m.media.copiesOffer)
        offer.toggle(two[0].name)
        // Keep All: the offer goes, and nothing is removed.
        m.media.copiesOffer = nil
        #expect(f.coRemoved.isEmpty)
        #expect(!f.coLog.contains { $0.hasPrefix("remove") })
        #expect(f.coCopies == two)
    }

    @Test func removeSelectedReportsItsProblems() throws {
        let (_, _, m, f) = try demo()
        f.coCopies = two
        f.coRemoveProblems = ["Copy 2026-11-14 15-02 before Drum Bus.vizdaw stayed: the Trash refused it."]
        #expect(m.media.compactFromMenu(channelOn: false))
        m.media.copiesOffer?.toggle(two[0].name)
        m.media.removeChosenCopies()
        #expect(f.coRemoved == [[two[0].name]])
        #expect(m.media.copiesOffer == nil)
        #expect(m.media.copiesReport == f.coRemoveProblems)
    }

    @Test func copiesOfferOnlyFromTheMenu() throws {
        let (s, id, m, f) = try demo()
        f.coCopies = two
        // Over the channel: Compact runs, and nothing is offered.
        let o = ControlCommands.run("media.compact", args: [], doc: (id, m), store: s, host: host)
        guard case .done = o else {
            Issue.record("media.compact did not run: \(o)")
            return
        }
        #expect(f.compactions == 1)
        #expect(m.media.copiesOffer == nil)
        // Programmatically: the same.
        #expect(m.media.compact())
        #expect(m.media.copiesOffer == nil)
        // A click while the control channel is on opens no sheet either.
        #expect(m.media.compactFromMenu(channelOn: true))
        #expect(m.media.copiesOffer == nil)
        // From the menu: the offer, with both copies.
        #expect(m.media.compactFromMenu(channelOn: false))
        #expect(m.media.copiesOffer?.rows.map(\.id) == two.map(\.name))
        #expect(f.compactions == 4)
        // No copies: no offer.
        m.media.copiesOffer = nil
        f.coCopies = []
        #expect(m.media.compactFromMenu(channelOn: false))
        #expect(m.media.copiesOffer == nil)
        // A failed Compact offers nothing.
        f.coCopies = two
        f.poolError = .Model(message: "no")
        #expect(!m.media.compactFromMenu(channelOn: false))
        #expect(m.media.copiesOffer == nil)
        m.alert = nil
    }
}
