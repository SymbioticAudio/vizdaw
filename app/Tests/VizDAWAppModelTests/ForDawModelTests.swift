import Foundation
import Testing
import VizDAWCore

@testable import VizDAWAppModel

/// The Export for Another DAW sheet's model (plan 6b Task 4; F10, F11).
@MainActor
@Suite(.serialized) struct ForDawModelTests {
    func fake() -> FakeSession {
        let f = mixerFake()
        f.info = ProjectInfoRow(
            format: ProjectFormatRow(sampleRate: 44_100, depth: .int16), mismatched: 0, rateMismatch: false,
            deviceRate: 0)
        f.forDawCheckReply = { req in
            ForDawCheckRow(
                report: ReportRow(summary: "Exported 2 tracks", items: []), folder: "Song for another DAW",
                files: [], strips: req.strips ?? [2, 5], start: "Every file starts at bar 1.", frames: 44_100,
                warning: nil)
        }
        return f
    }

    func shown(_ f: FakeSession) async throws -> (DocumentModel, ForDawModel) {
        let m = try DocumentModel(session: f)
        m.forDaw.shown = true
        await m.forDaw.settle()
        return (m, m.forDaw)
    }

    func ended(_ f: FakeSession, _ m: DocumentModel, _ kind: JobKindRow, _ state: JobStateRow, message: String? = nil) throws {
        let id = try #require(m.jobs.first { $0.kind == kind }?.id)
        f.jobRows[id] = JobRow(
            id: id, kind: kind, progress: 1, state: state, message: message, problems: [], created: [],
            report: state == .done ? ReportRow(summary: "Exported 2 tracks", items: []) : nil,
            written: state == .done ? ["/tmp/x/Song for another DAW"] : [])
        m.pollJobs()
    }

    @Test func showResetsToTheCoresDefaults() async throws {
        let f = fake()
        let (m, d) = try await shown(f)
        defer { withExtendedLifetime(m) {} }
        #expect(d.container == .wav && d.rate == 44_100 && !d.otherRate && d.depth == .int16)
        #expect(d.dither && !d.addToProject)
        #expect(d.stems == [2, 5] && d.check?.strips == [2, 5])
        #expect(d.strips.map(\.id) == [1, 2], "every strip but the master")
        #expect(f.forDawChecks.count == 1 && f.forDawChecks[0].strips == nil, "no second check for the stems")
        d.shown = false
        f.info.format = ProjectFormatRow(sampleRate: 50_000, depth: .float32)
        d.depth = .int24
        d.shown = true
        await d.settle()
        #expect(d.depth == .float32 && d.rate == 50_000 && d.otherRate && !d.dither)
    }

    @Test func everyChangeRefreshesTheCheckOffTheMainActor() async throws {
        let f = fake()
        let (m, d) = try await shown(f)
        defer { withExtendedLifetime(m) {} }
        var n = f.forDawChecks.count
        func next(_ what: String, _ change: () -> Void) async {
            change()
            await d.settle()
            #expect(f.forDawChecks.count == n + 1, "\(what) checks once")
            #expect(f.forDawChecks.last?.strips == d.stems.sorted(), "sends the shown stems")
            n = f.forDawChecks.count
        }
        await next("stems") { d.stems = [1, 2] }
        await next("container") { d.container = .aiff }
        await next("depth") { d.depth = .int24 }
        await next("rate") { d.rate = 96_000 }
        await next("add") { d.addToProject = true }
        #expect(f.forDawChecks.last?.addToProject == true && f.forDawChecks.last?.rate == 96_000)
        #expect(f.forDawChecks.last?.container == .aiff && f.forDawChecks.last?.depth == .int24)
        #expect(!f.checkOnMain.isEmpty && !f.checkOnMain.contains(true), "every check ran in a detached task")

        // Two changes in a row: the first is held, the second lands.
        let release = DispatchSemaphore(value: 0)
        f.forDawCheckHold = { $0.rate == 88_200 ? release : nil }
        f.forDawCheckReply = { ForDawCheckRow(
            report: ReportRow(summary: "rate \($0.rate)", items: []), folder: "F", files: [],
            strips: $0.strips ?? [], start: "", frames: 0, warning: nil) }
        d.rate = 88_200
        d.rate = 192_000
        for _ in 0..<500 where d.check?.report.summary != "rate 192000" { try await Task.sleep(for: .milliseconds(10)) }
        #expect(d.check?.report.summary == "rate 192000")
        release.signal()
        await d.settle()
        #expect(d.check?.report.summary == "rate 192000", "a stale result is dropped")
    }

    @Test func startSendsTheChoices() async throws {
        let f = fake()
        f.jobStartState = .running
        let (m, d) = try await shown(f)
        d.depth = .float32
        d.stems = [2]
        d.start(folder: "/tmp/x")
        let r = try #require(f.forDawExports.last)
        #expect(r.path == "/tmp/x" && r.strips == [2] && !r.dither && r.depth == .float32)
        #expect(r.seq == m.selectedSeq && r.rate == 44_100 && !r.addToProject)
        #expect(d.job != nil && d.running && !d.waiting)
    }

    @Test func exportWaitsForTheRunningJob() async throws {
        let f = fake()
        f.jobStartState = .running
        let (m, d) = try await shown(f)
        m.interchange.importDropped(paths: ["/tmp/a.mid"], target: ImportTargetRow(seq: 3, lane: nil, tick: 0))
        #expect(f.midiImports.count == 1)
        d.start(folder: "/tmp/x")
        #expect(f.forDawExports.isEmpty && d.waiting && m.alert == nil)
        m.pollJobs()
        #expect(f.forDawExports.isEmpty, "still running")
        try ended(f, m, .importMidi, .done)
        #expect(f.forDawExports.count == 1 && !d.waiting && d.running)

        try ended(f, m, .exportProject, .done)
        // Cancelled before the job ends: nothing starts.
        let (n, e) = try await shown(f)
        n.interchange.importDropped(paths: ["/tmp/b.mid"], target: ImportTargetRow(seq: 3, lane: nil, tick: 0))
        let before = f.forDawExports.count
        e.start(folder: "/tmp/y")
        #expect(e.waiting)
        e.cancelWaiting()
        #expect(!e.waiting)
        try ended(f, n, .importMidi, .done)
        #expect(f.forDawExports.count == before)
    }

    @Test func addWaitsForMediaJobs() async throws {
        let f = fake()
        f.jobStartState = .running
        let (m, d) = try await shown(f)
        f.jobRows[99] = JobRow(id: 99, kind: .convert, progress: 0.4, state: .running, message: nil, problems: [], created: [])
        m.jobs.append(f.jobRows[99]!)
        d.start(folder: "/tmp/x")
        #expect(f.forDawExports.count == 1, "without Add it starts at once")
        try ended(f, m, .exportProject, .done)

        d.addToProject = true
        await d.settle()
        d.start(folder: "/tmp/x")
        #expect(f.forDawExports.count == 1 && d.waiting)
        try ended(f, m, .convert, .done)
        #expect(f.forDawExports.count == 2 && !d.waiting)
        #expect(f.forDawExports.last?.addToProject == true)
    }

    @Test func aFinishedExportShowsTheReportWithTheFolder() async throws {
        let f = fake()
        f.jobStartState = .running
        let (m, d) = try await shown(f)
        d.start(folder: "/tmp/x")
        try ended(f, m, .exportProject, .done)
        #expect(m.interchange.report?.summary == "Exported 2 tracks" && m.interchange.reportShown)
        #expect(m.interchange.written == ["/tmp/x/Song for another DAW"])
        #expect(m.jobs.isEmpty && !d.running)
    }

    @Test func aFailedExportReopensTheSheet() async throws {
        let f = fake()
        f.jobStartState = .running
        let (m, d) = try await shown(f)
        d.rate = 96_000
        d.start(folder: "/tmp/x")
        d.shown = false
        try ended(f, m, .exportProject, .failed, message: "The disk is full.")
        #expect(d.shown && d.error == "The disk is full." && m.alert == nil)
        #expect(d.rate == 96_000, "reopened with the choices kept")

        // Cancelled: quiet.
        d.shown = false
        d.start(folder: "/tmp/x")
        try ended(f, m, .exportProject, .cancelled)
        #expect(!d.shown && m.alert == nil)
    }

    @Test func noStringSaysLane() async throws {
        let f = fake()
        f.jobStartState = .running
        let (m, d) = try await shown(f)
        var strings: [String] = [ProjectBanner.jobText(.exportProject), ProjectBanner.failureTitle(.exportProject)]
        let none = try DocumentModel(session: FakeSession([]))
        none.forDaw.shown = true
        await none.forDaw.settle()
        strings.append(none.forDaw.error ?? "")
        #expect(none.forDaw.error != nil)
        d.start(folder: "/tmp/x")
        try ended(f, m, .exportProject, .failed, message: "Choose at least one strip to add its stem to the project.")
        strings.append(d.error ?? "")
        f.forDawCheckAnswer = .failure(.Model(message: "No track is shown."))
        d.stems = [1]
        await d.settle()
        strings.append(d.error ?? "")
        #expect(strings.allSatisfy { !$0.lowercased().contains("lane") })
    }

    func running(_ f: FakeSession, _ m: DocumentModel, _ id: UInt64, _ kind: JobKindRow) {
        f.jobRows[id] = JobRow(id: id, kind: kind, progress: 0.4, state: .running, message: nil, problems: [], created: [])
        m.jobs.append(f.jobRows[id]!)
    }

    func finish(_ f: FakeSession, _ id: UInt64, _ kind: JobKindRow) {
        f.jobRows[id] = JobRow(id: id, kind: kind, progress: 1, state: .done, message: nil, problems: [], created: [])
    }

    @Test func eachBlockingKindBlocksOnItsOwn() async throws {
        for kind in [JobKindRow.bounce, .exportMidi, .exportProject] {
            let f = fake()
            f.jobStartState = .running
            let (m, d) = try await shown(f)
            running(f, m, 90, kind)
            d.start(folder: "/tmp/x")
            #expect(f.forDawExports.isEmpty && d.waiting, "\(kind) blocks")
            finish(f, 90, kind)
            m.pollJobs()
            #expect(f.forDawExports.count == 1 && !d.waiting, "\(kind) ends: it starts")
        }
    }

    @Test func twoBlockersEndingInOnePollStartTheExportOnce() async throws {
        let f = fake()
        f.jobStartState = .running
        let (m, d) = try await shown(f)
        running(f, m, 90, .bounce)
        running(f, m, 91, .importMidi)
        d.start(folder: "/tmp/x")
        finish(f, 90, .bounce)
        finish(f, 91, .importMidi)
        m.pollJobs()
        #expect(f.forDawExports.count == 1 && m.alert == nil)
    }

    @Test func anUnknownJobCountsAsEnded() async throws {
        let f = fake()
        f.jobStartState = .running
        let (m, d) = try await shown(f)
        m.jobs.append(JobRow(id: 77, kind: .bounce, progress: 0, state: .running, message: nil, problems: [], created: []))
        d.start(folder: "/tmp/x")
        #expect(d.waiting)
        m.pollJobs()
        #expect(f.forDawExports.count == 1 && !d.waiting)
    }

    @Test func aDropAndAKeptExportWaitingOnTheSameJobBothProceedInOrder() async throws {
        let f = fake()
        f.jobStartState = .running
        let (m, d) = try await shown(f)
        running(f, m, 90, .bounce)
        m.interchange.importDropped(paths: ["/tmp/a.mid"], target: ImportTargetRow(seq: 3, lane: nil, tick: 0))
        d.start(folder: "/tmp/x")
        #expect(f.midiImports.isEmpty && f.forDawExports.isEmpty)
        finish(f, 90, .bounce)
        m.pollJobs()
        #expect(f.midiImports.count == 1 && f.forDawExports.isEmpty && d.waiting, "the drop first; the export waits for it")
        try ended(f, m, .importMidi, .done)
        #expect(f.forDawExports.count == 1 && !d.waiting)
    }

    @Test func showingAgainKeepsAWaitingExport() async throws {
        let f = fake()
        f.jobStartState = .running
        let (m, d) = try await shown(f)
        running(f, m, 90, .bounce)
        d.rate = 96_000
        d.start(folder: "/tmp/x")
        d.shown = false
        d.shown = true
        await d.settle()
        #expect(d.waiting && d.rate == 96_000)
        finish(f, 90, .bounce)
        m.pollJobs()
        #expect(f.forDawExports.last?.rate == 96_000)
    }

    @Test func theSheetStaysUpWhileTheExportRuns() async throws {
        let f = fake()
        f.jobStartState = .running
        let (m, d) = try await shown(f)
        d.start(folder: "/tmp/x")
        #expect(d.shown && d.running && d.job != nil, "progress and Cancel are visible")
        let job = d.job
        d.shown = false
        d.shown = true
        await d.settle()
        #expect(d.running && d.job == job, "showing again keeps the job")
        let exports = f.forDawExports.count
        d.start(folder: "/tmp/y")
        #expect(f.forDawExports.count == exports && d.job == job && !d.waiting, "no second export from the sheet")
        try ended(f, m, .exportProject, .done)
        #expect(!d.shown, "closes on success")
    }

    @Test func cancellingTheRunningExportClosesTheSheet() async throws {
        let f = fake()
        f.jobStartState = .running
        let (m, d) = try await shown(f)
        d.start(folder: "/tmp/x")
        try ended(f, m, .exportProject, .cancelled)
        #expect(!d.shown && m.alert == nil)
    }

    @Test func theLengthUsesTheRateTheCheckWasMadeAt() async throws {
        let f = fake()
        let (m, d) = try await shown(f)
        defer { withExtendedLifetime(m) {} }
        #expect(d.length == "0:01.0 + tail")
        let release = DispatchSemaphore(value: 0)
        f.forDawCheckHold = { $0.rate == 88_200 ? release : nil }
        d.rate = 88_200
        #expect(d.length == "0:01.0 + tail", "the old check is still at the old rate")
        release.signal()
        await d.settle()
        #expect(d.length == "0:00.5 + tail")
    }

    @Test func noSequenceDropsAnOlderCheck() async throws {
        let f = fake()
        let (m, d) = try await shown(f)
        let release = DispatchSemaphore(value: 0)
        f.forDawCheckHold = { _ in release }
        d.rate = 96_000
        m.forDaw.refresh()
        m.select(nil)
        d.refresh()
        release.signal()
        release.signal()
        await d.settle()
        #expect(d.check == nil && d.error == "No sequence is shown.")
    }
    @Test func lengthIsTheStemsTimeAndTheTail() async throws {
        let f = fake()
        let (m, d) = try await shown(f)
        defer { withExtendedLifetime(m) {} }
        #expect(d.length == "0:01.0 + tail", "the check's frames at the export rate")
        d.rate = 88_200
        await d.settle()
        #expect(d.length == "0:00.5 + tail")
        m.select(nil)
        d.refresh()
        await d.settle()
        #expect(d.length == "", "no check, no length")
    }

    /// Bounce and For Another DAW share the sheet's format rows.
    @Test func bothModelsOfferTheSameFormatChoices() async throws {
        let f = fake()
        let (m, d) = try await shown(f)
        defer { withExtendedLifetime(m) {} }
        func choose(_ c: some AudioFormatChoices) {
            c.container = .caf
            c.depth = .float32
            c.dither = true
            c.rateChoice = 0
            c.setOtherRate(1_000_000)
        }
        for c: any AudioFormatChoices in [m.bounce, d] {
            choose(c)
            #expect(c.container == .caf && c.depth == .float32 && c.dither)
            #expect(c.otherRate && c.rateChoice == 0 && c.rate == 384_000)
        }
    }
}
