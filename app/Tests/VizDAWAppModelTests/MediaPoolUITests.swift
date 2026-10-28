import Foundation
import Testing
import VizDAWCore

@testable import VizDAWAppModel

/// What the app's media pool surface decides (plan 5b Task 12).
@MainActor
@Suite struct MediaPoolUITests {
    @Test func theRelinkSheetNeverOpensUnderTheControlChannel() throws {
        let f = demoFake()
        f.mediaRows = [mediaRow(1, "a", offline: .missing)]
        let m = try DocumentModel(session: f)
        #expect(m.media.showsRelinkSheet(channelOn: false))
        #expect(!m.media.showsRelinkSheet(channelOn: true), "programmatic sessions never see dialogs")
        m.media.leaveOffline()
        #expect(!m.media.showsRelinkSheet(channelOn: false))
    }

    @Test func theSheetClosesItselfOnceNothingIsLeftToRelink() throws {
        let f = demoFake()
        f.mediaRows = [mediaRow(1, "a", offline: .missing)]
        let m = try DocumentModel(session: f)
        f.mediaRows = [mediaRow(1, "a")]
        m.refreshAfterJob()
        #expect(!m.media.showsRelinkSheet(channelOn: false))
    }

    @Test func itemsAreMarkedOfferedOnlyWhenTheSheetCanShow() throws {
        let f = demoFake()
        f.mediaRows = [mediaRow(1, "a")]
        let m = try DocumentModel(session: f)
        #expect(!m.media.relinkShown)
        f.mediaRows = [mediaRow(1, "a", offline: .missing)]
        m.mediaVersion += 1
        m.media.offerRelinkIfNeeded(channelOn: true)
        #expect(!m.media.relinkShown, "the channel suppresses the sheet")
        m.media.offerRelinkIfNeeded(channelOn: false)
        #expect(m.media.relinkShown, "not marked offered while suppressed, so offered now")
        m.media.leaveOffline()
        m.media.offerRelinkIfNeeded(channelOn: false)
        #expect(!m.media.relinkShown, "once offered, not offered again")
    }
}
