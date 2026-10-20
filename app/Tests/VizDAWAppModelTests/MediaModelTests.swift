import Foundation
import Testing
import VizDAWCore

@testable import VizDAWAppModel

/// The media pool's model (plan 5b Task 8; spec §12.3 "MediaModel with
/// FakeSession").
@MainActor
@Suite struct MediaModelTests {
    func model(_ f: FakeSession) throws -> DocumentModel { try DocumentModel(session: f) }

    @Test func rowsFollowTheMediaVersion() throws {
        let f = demoFake()
        f.mediaRows = [mediaRow(1, "kick")]
        let m = try model(f)
        #expect(m.media.rows.map(\.id) == [1])
        f.mediaRows = [mediaRow(1, "kick"), mediaRow(2, "snare")]
        #expect(m.media.rows.count == 1, "cached until the version moves")
        m.refreshAfterJob()
        #expect(m.media.rows.map(\.name) == ["kick", "snare"])
        #expect(m.media.row(2)?.name == "snare")
        #expect(m.media.row(nil) == nil)
    }

    @Test func poolRowsSayWhatTheWindowShows() {
        #expect(MediaModel.duration(frames: 96_000, rate: 48_000) == "0:02.0")
        #expect(MediaModel.duration(frames: 48_000 * 61 + 2_400, rate: 48_000) == "1:01.1")
        #expect(MediaModel.duration(frames: 48_000 * 3_725, rate: 48_000) == "1:02:05.0")
        #expect(MediaModel.duration(frames: 10, rate: 0) == "0:10.0")
        let r = MediaModel.poolRow(mediaRow(3, "pad", uses: 2, offline: .changed, used: 74.6))
        #expect(r.name == "pad" && r.channels == "Stereo" && r.origin == "Imported")
        #expect(r.uses == 2 && r.used == "75 %" && r.offline == "Changed")
        #expect(MediaModel.poolRow(mediaRow(4, "x")).offline == "")
        #expect(MediaModel.reasonText(.missing) == "Missing")
        #expect(MediaModel.reasonText(.length) == "Changed length")
    }

    @Test func renameTrimsAndSkipsBlankOrUnchangedNames() throws {
        let f = demoFake()
        f.mediaRows = [mediaRow(3, "kick")]
        let m = try model(f)
        #expect(m.media.rename(3, to: "  Kick 2 "))
        #expect(f.renamed.map(\.1) == ["Kick 2"])
        #expect(!m.media.rename(3, to: "   "))
        #expect(!m.media.rename(3, to: "kick"))
        #expect(f.renamed.count == 1)
        f.poolError = .Model(message: "nope")
        #expect(!m.media.rename(3, to: "Other"))
        #expect(m.alert?.title == "Rename Failed")
    }

    @Test func deleteAsksFirstWhenRegionsUseTheItem() throws {
        let f = demoFake()
        f.mediaRows = [mediaRow(3, "kick", uses: 2), mediaRow(4, "spare")]
        f.usersTable[3] = [
            MediaUseRow(seq: 3, lane: 31, event: 9, tick: 0, sequence: "Song", laneName: "Audio 1"),
            MediaUseRow(seq: 3, lane: 31, event: 10, tick: 61_440, sequence: "Song", laneName: "Audio 1"),
        ]
        let m = try model(f)
        let q = try #require(m.media.deleteQuestion(3))
        #expect(q.contains("used by 2 regions"))
        #expect(q.contains("Song › Audio 1, bar 1"))
        #expect(q.contains("Song › Audio 1, bar 2"))
        #expect(m.media.deleteQuestion(4) == nil)
        m.media.selection = [3, 4]
        #expect(m.media.remove(3))
        #expect(f.removedMedia == [3])
        #expect(m.media.selection == [4])
    }

    @Test func selectingUsersShowsTheirSequence() throws {
        let f = demoFake()
        f.mediaRows = [mediaRow(3, "kick", uses: 2)]
        f.usersTable[3] = [
            MediaUseRow(seq: 2, lane: 21, event: 9, tick: 0, sequence: "Groove", laneName: "Bass"),
            MediaUseRow(seq: 3, lane: 31, event: 10, tick: 0, sequence: "Song", laneName: "Chords"),
        ]
        let m = try model(f)
        m.media.selectUsers(3)
        #expect(m.selectedSeq == 2)
        #expect(m.editor.selection.events == [9])
        #expect(m.editor.selection.lanes == [LaneRef(seq: 2, lane: 21)])
        m.media.selectUsers(99)
        #expect(m.editor.status == "No region uses this item.")
    }

    @Test func selectUnusedPicksItemsWithoutRegions() throws {
        let f = demoFake()
        f.mediaRows = [mediaRow(1, "a", uses: 1), mediaRow(2, "b"), mediaRow(3, "c")]
        let m = try model(f)
        m.media.selectUnused()
        #expect(m.media.selection == [2, 3])
    }

    @Test func relinkAndSearchRunAsRelinkJobs() throws {
        let f = demoFake()
        f.mediaRows = [mediaRow(3, "kick", offline: .missing)]
        f.jobStartState = .running
        let m = try model(f)
        m.media.relink(3, path: "/x/kick.wav")
        #expect(f.relinks.first?.path == "/x/kick.wav")
        #expect(m.jobs.first?.kind == .relink)
        #expect(m.banners.contains { if case .job(3001, .relink, _) = $0 { true } else { false } })
        f.jobRows[3001]?.state = .done
        let before = m.mediaVersion
        m.pollJobs()
        #expect(m.mediaVersion == before + 1)
        m.media.searchFolder("/x")
        #expect(f.searches == ["/x"])
        f.jobRows[4001] = JobRow(
            id: 4001, kind: .relink, progress: 1, state: .done, message: nil,
            problems: ["kick: not found in /x"], created: [])
        m.pollJobs()
        #expect(m.alert?.title == "Relink Failed")
        #expect(m.alert?.message.contains("not found") == true)
    }

    @Test func theRelinkSheetIsOfferedForMissingOrChangedMedia() throws {
        let f = demoFake()
        f.mediaRows = [mediaRow(1, "a", offline: .format)]
        #expect(try !model(f).media.relinkShown, "Convert handles another format")
        f.mediaRows = [mediaRow(1, "a", offline: .changed), mediaRow(2, "b")]
        let m = try model(f)
        #expect(m.media.relinkShown)
        #expect(m.media.relinkable.map(\.id) == [1])
        m.media.leaveOffline()
        #expect(!m.media.relinkShown)
    }

    @Test func jobTextsAndTitles() {
        #expect(ProjectBanner.jobText(.import) == "Importing audio…")
        #expect(ProjectBanner.jobText(.relink) == "Relinking media…")
        #expect(ProjectBanner.jobText(.format) == "Converting media…")
        #expect(ProjectBanner.failureTitle(.relink) == "Relink Failed")
        #expect(ProjectBanner.failureTitle(.import) == "Import Failed")
        #expect(ProjectBanner.failureTitle(.convert) == "Conversion Failed")
    }

    @Test func dropsImportWithTheirOwnTarget() throws {
        let f = demoFake()
        let m = try model(f)
        m.importAudio(paths: ["/a.wav"], target: ImportTargetRow(seq: 3, lane: nil, tick: 1_920))
        #expect(f.imports.last?.1 == ImportTargetRow(seq: 3, lane: nil, tick: 1_920))
        m.media.importIntoPool(["/b.wav"])
        #expect(f.imports.last.map { $0.1 == nil } == true)
        m.importAudio(paths: [], target: nil)
        #expect(f.imports.count == 2)
    }

    @Test func waveformsClampTheirColumnsAndOverviewsBumpAVersion() throws {
        let f = demoFake()
        let m = try model(f)
        #expect(m.media.waveform(event: 1, fromTick: 0, toTick: 100, columns: 0) == nil)
        #expect(m.media.waveform(event: 1, fromTick: 100, toTick: 100, columns: 10) == nil)
        _ = m.media.waveform(event: 1, fromTick: 0, toTick: 100, columns: 100_000)
        #expect(f.waveformCalls.last?.columns == 8192)
        f.idleUpdates = [
            IdleUpdate(
                snapshot: nil, slotsChanged: false, paramsChanged: [], problems: [], mediaChanged: false,
                overviewsChanged: true)
        ]
        m.idle()
        #expect(m.overviewVersion == 1)
    }

    @Test func theRelinkSheetIsOfferedWhenIdleFindsChangedMedia() throws {
        let f = demoFake()
        f.mediaRows = [mediaRow(1, "a"), mediaRow(2, "b")]
        let m = try model(f)
        #expect(!m.media.relinkShown)
        // The worker re-hashed a file after open: it is Changed now.
        f.mediaRows = [mediaRow(1, "a", offline: .changed), mediaRow(2, "b")]
        let moved = IdleUpdate(
            snapshot: nil, slotsChanged: false, paramsChanged: [], problems: [], mediaChanged: true,
            overviewsChanged: false)
        f.idleUpdates = [moved]
        m.idle()
        #expect(m.media.relinkShown)
        m.media.leaveOffline()
        f.idleUpdates = [moved]
        m.idle()
        #expect(!m.media.relinkShown, "the same item is not offered twice")
        f.mediaRows = [mediaRow(1, "a", offline: .changed), mediaRow(2, "b", offline: .missing)]
        f.idleUpdates = [moved]
        m.idle()
        #expect(m.media.relinkShown, "a new item is offered")
    }

    @Test func aPoolOutlivingItsDocumentIsHarmless() throws {
        let f = demoFake()
        f.mediaRows = [mediaRow(1, "a", uses: 1)]
        var doc: DocumentModel? = try model(f)
        let pool = try #require(doc?.media)
        #expect(pool.rows.count == 1)
        doc = nil
        #expect(pool.document == nil)
        #expect(pool.rows.map(\.id) == [1], "the cached rows")
        #expect(!pool.rename(1, to: "x") && !pool.compact() && !pool.remove(1))
        #expect(pool.users(1).isEmpty && pool.deleteQuestion(1) == nil && pool.file(1) == nil)
        #expect(pool.waveform(event: 1, fromTick: 0, toTick: 10, columns: 4) == nil)
        pool.selectUsers(1)
        pool.relink(1, path: "/x")
        pool.searchFolder("/x")
        pool.importIntoPool(["/a.wav"])
        #expect(f.relinks.isEmpty && f.searches.isEmpty && f.imports.isEmpty && f.renamed.isEmpty)
    }

    @Test func aPartialRelinkAnswersOkWithWarnings() throws {
        let partial = JobRow(
            id: 4001, kind: .relink, progress: 1, state: .done, message: nil,
            problems: ["snare: not found in /x"], created: [3])
        #expect(ControlJobs.answer(partial) == .ok(status: "warnings: snare: not found in /x"))
        let f = demoFake()
        f.mediaRows = [mediaRow(3, "kick", offline: .missing), mediaRow(4, "snare", offline: .missing)]
        let m = try model(f)
        f.jobRows[4001] = partial
        m.jobs = [partial]
        m.pollJobs()
        #expect(m.alert == nil)
        #expect(m.jobProblems == ["snare: not found in /x"])
    }
}
