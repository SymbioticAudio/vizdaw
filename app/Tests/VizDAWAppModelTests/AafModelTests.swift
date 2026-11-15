import Foundation
import Testing
import VizDAWCore

@testable import VizDAWAppModel

/// The AAF sheets' model (plan 6f Task 8): the export's check off the main
/// actor on every option (only the latest lands), an export that waits for
/// a running job, a failure back in the sheet; the import's info read off
/// the main actor and its request.
@MainActor
@Suite(.serialized) struct AafModelTests {
    func fake() -> FakeSession {
        let f = demoFake()
        f.aafCheckReply = { ReportRow(summary: "check \($0.depth) \($0.fps)", items: []) }
        return f
    }

    func shown(_ f: FakeSession) async throws -> (DocumentModel, AafModel) {
        let m = try DocumentModel(session: f)
        m.aaf.exportShown = true
        await m.aaf.settle()
        return (m, m.aaf)
    }

    /// A running job of `kind`, listed as `startJob` lists one.
    func running(_ f: FakeSession, _ m: DocumentModel, _ kind: JobKindRow, id: UInt64 = 99) {
        let row = JobRow(id: id, kind: kind, progress: 0.4, state: .running, message: nil, problems: [], created: [])
        f.jobRows[id] = row
        m.jobs.append(row)
    }

    func end(
        _ f: FakeSession, _ m: DocumentModel, _ id: UInt64, _ kind: JobKindRow, _ state: JobStateRow,
        message: String? = nil, report: ReportRow? = nil
    ) {
        f.jobRows[id] = JobRow(
            id: id, kind: kind, progress: 1, state: state, message: message, problems: [], created: [],
            report: report, written: report == nil ? [] : ["/tmp/out/Song AAF/Song.aaf"])
        m.pollJobs()
    }

    @Test func checkRefreshesOnEachOptionAndOnlyTheLatestLands() async throws {
        let f = fake()
        let (m, a) = try await shown(f)
        defer { withExtendedLifetime(m) {} }
        let first = try #require(f.aafChecks.first)
        #expect(f.aafChecks.count == 1 && first.path == "" && first.seq == m.selectedSeq)
        #expect(
            first.embed && first.depth == .int24 && first.stereo == .monoPairs && !first.hourStart
                && first.fps == .fps25 && first.clipGain && first.automation, "the defaults")
        #expect(a.wontTransfer?.summary == "check int24 fps25" && a.checkError == nil)
        let shownSeq = try #require(m.selectedSeq)
        #expect(a.defaultName == "Song AAF" && f.aafFolderNames == [shownSeq])
        #expect(f.checkOnMain == [false], "the check ran in a detached task")

        // Two quick changes: the first is held, the second's answer wins.
        let release = DispatchSemaphore(value: 0)
        f.aafCheckHold = { $0.depth == .int16 && $0.fps == .fps25 ? release : nil }
        f.aafCheckReply = { ReportRow(summary: "second \($0.depth) \($0.fps)", items: []) }
        a.depth = .int16
        a.fps = .fps24
        for _ in 0..<500 where a.wontTransfer?.summary != "second int16 fps24" {
            try await Task.sleep(for: .milliseconds(10))
        }
        #expect(a.wontTransfer?.summary == "second int16 fps24")
        release.signal()
        await a.settle()
        #expect(a.wontTransfer?.summary == "second int16 fps24", "a stale answer is dropped")
        #expect(f.aafChecks.count == 3)

        // Every other option asks again, with the option sent.
        f.aafCheckHold = nil
        a.embed = false
        a.stereo = .stereo
        a.hourStart = true
        a.clipGain = false
        a.automation = false
        await a.settle()
        #expect(f.aafChecks.count == 8 && !f.checkOnMain.contains(true))
        // The detached checks may log in any order: the latest is among them.
        #expect(f.aafChecks.contains { !$0.embed && $0.stereo == .stereo && $0.hourStart && !$0.clipGain && !$0.automation })
        // Setting the same value asks nothing.
        a.automation = false
        await a.settle()
        #expect(f.aafChecks.count == 8)

        // The core's refusal is the sheet's message.
        f.aafCheckReply = nil
        f.aafCheckAnswer = .failure(VizError.Model(message: "no such sequence"))
        a.refresh()
        await a.settle()
        #expect(a.wontTransfer == nil && a.checkError == "no such sequence")
    }

    @Test func exportQueuesBehindARunningJob() async throws {
        let f = fake()
        f.jobStartState = .running
        let (m, a) = try await shown(f)
        running(f, m, .exportDawproject)
        a.fps = .fps30
        await a.settle()
        a.confirmed(folder: "/tmp/out")
        #expect(a.waiting && f.aafExports.isEmpty && m.alert == nil && a.job == nil)
        m.pollJobs()
        #expect(f.aafExports.isEmpty, "still running")
        end(f, m, 99, .exportDawproject, .done)
        #expect(f.aafExports.count == 1 && !a.waiting && a.running)
        let r = try #require(f.aafExports.last)
        #expect(r.path == "/tmp/out" && r.fps == .fps30 && r.seq == m.selectedSeq)

        // Cancel while waiting sends nothing.
        let g = fake()
        g.jobStartState = .running
        let (n, b) = try await shown(g)
        running(g, n, .bounce)
        b.confirmed(folder: "/tmp/out")
        #expect(b.waiting)
        b.cancelWaiting()
        end(g, n, 99, .bounce, .done)
        #expect(g.aafExports.isEmpty && b.job == nil && !b.waiting)
    }

    @Test func failedExportReopensWithTheMessage() async throws {
        let f = fake()
        f.jobStartState = .running
        let (m, a) = try await shown(f)
        a.confirmed(folder: "/tmp/out")
        let job = try #require(a.job)
        #expect(a.running && m.jobs.contains { $0.id == job && $0.kind == .exportAaf })
        a.exportShown = false
        end(f, m, job, .exportAaf, .failed, message: "Song AAF is already in /tmp/out.")
        #expect(a.checkError == "Song AAF is already in /tmp/out." && a.exportShown, "the sheet again")
        #expect(!m.interchange.reportShown && m.interchange.report == nil && m.alert == nil && !a.running)

        // A cancelled one is quiet; a finished one closes and shows its report.
        a.confirmed(folder: "/tmp/out")
        let second = try #require(a.job)
        end(f, m, second, .exportAaf, .cancelled)
        #expect(m.alert == nil && !m.interchange.reportShown && !a.exportShown)
        a.exportShown = true
        await a.settle()
        a.confirmed(folder: "/tmp/out")
        let third = try #require(a.job)
        end(f, m, third, .exportAaf, .done, report: ReportRow(summary: "Exported 2 tracks", items: []))
        #expect(m.interchange.reportShown && m.interchange.written == ["/tmp/out/Song AAF/Song.aaf"])
        #expect(!a.exportShown && m.alert == nil)

        // Refused as it starts: the message in the sheet, no alert.
        f.interchangeError = VizError.Model(message: "the folder is not writable")
        a.exportShown = true
        await a.settle()
        a.confirmed(folder: "/tmp/out")
        #expect(a.checkError == "the folder is not writable" && a.exportShown && m.alert == nil && !a.running)
    }

    @Test func chooseFillsTheMidiBeside() async throws {
        let f = fake()
        let m = try DocumentModel(session: f)
        let a = m.aaf
        a.readInfo = { try f.aafInfo(path: $0) }
        f.aafInfoAnswer = .success(
            AafInfoRow(
                sampleRate: 44_100, tracks: 3, audioFiles: 2, offline: 1, fromVizdaw: false,
                midiBeside: "/x/Song MIDI.mid"))
        a.choose(path: "/x/Song.aaf")
        #expect(!a.importShown, "the sheet waits for the file to be read")
        await a.settle()
        #expect(f.aafInfoPaths == ["/x/Song.aaf"] && f.checkOnMain == [false])
        #expect(a.importShown && a.importPath == "/x/Song.aaf" && a.info?.sampleRate == 44_100)
        #expect(a.midiPath == "/x/Song MIDI.mid" && a.newProject && a.joinPairs && a.mediaFolder == nil)

        // A refusal is an alert, and no sheet.
        a.importShown = false
        f.aafInfoAnswer = .failure(VizError.Model(message: "not an AAF file"))
        a.choose(path: "/x/b.aaf")
        await a.settle()
        #expect(!a.importShown && m.alert?.title == "Import Failed" && m.alert?.message.contains("not an AAF file") == true)
    }

    @Test func importIntoThisProjectSendsNewProjectFalse() async throws {
        let f = fake()
        f.jobStartState = .running
        let m = try DocumentModel(session: f)
        let a = m.aaf
        a.readInfo = { try f.aafInfo(path: $0) }
        f.aafInfoAnswer = .success(
            AafInfoRow(
                sampleRate: 44_100, tracks: 3, audioFiles: 2, offline: 0, fromVizdaw: false,
                midiBeside: "/x/Song MIDI.mid"))
        a.choose(path: "/x/Song.aaf")
        await a.settle()
        a.newProject = false
        a.joinPairs = false
        a.mediaFolder = "/media"
        let store = DocumentStore(makeEmpty: { FakeSession([]) }, makeDemo: { demoFake() }, open: { _ in demoFake() })
        a.startImport(store: store)
        #expect(
            f.aafImports
                == [
                    AafImportRequestRow(
                        path: "/x/Song.aaf", newProject: false, midiPath: "/x/Song MIDI.mid", mediaFolder: "/media",
                        joinPairs: false)
                ])
        #expect(!a.importShown && store.documents.isEmpty && m.jobs.contains { $0.kind == .importAaf })
    }
}

/// Opening an `.aaf` through the store (plan 6f Task 8), as a
/// `.dawproject`: the file is read off the main actor with no document
/// open, then a project at its rate imports it with the defaults.
extension DocumentStoreTests {
    func aafStore(
        _ make: @escaping () -> FakeSession = { FakeSession([seq(9, "New", topLevel: true)]) }
    ) -> (DocumentStore, DawBox, FakeSession) {
        let (s, box, info) = dawStore(make)
        s.aafInfo = { try info.aafInfo(path: $0) }
        return (s, box, info)
    }

    @Test func openingAnAafMakesAProjectAtItsRate() async throws {
        let (s, box, info) = aafStore()
        s.defaultFormat = ProjectFormatRow(sampleRate: 96_000, depth: .int16)
        info.aafInfoAnswer = .success(
            AafInfoRow(sampleRate: 44_100, tracks: 2, audioFiles: 2, offline: 0, fromVizdaw: false, midiBeside: nil))
        #expect(s.open(path: "/x/a.aaf") == nil)
        #expect(s.documents.isEmpty && box.formats.isEmpty, "nothing is made before the file is read")
        await s.settle()
        #expect(info.aafInfoPaths == ["/x/a.aaf"])
        #expect(info.checkOnMain == [false], "the info read ran off the main actor")
        #expect(box.formats == [ProjectFormatRow(sampleRate: 44_100, depth: .int16)])
        let fresh = try #require(box.made.first)
        #expect(
            fresh.aafImports
                == [AafImportRequestRow(path: "/x/a.aaf", newProject: true, midiPath: nil, mediaFolder: nil, joinPairs: true)])
        let id = try #require(s.documents.keys.first)
        #expect(s.documents.count == 1 && s.pending == [id], "the new project's window opens")
        #expect(s.alert == nil)
        // Any case of the extension.
        #expect(InterchangeModel.isAaf("/x/B.AAF") && !InterchangeModel.isAaf("/x/b.aafx"))
    }

    @Test func aRefusedOpenLeavesNoProject() async throws {
        let (s, box, info) = aafStore()
        info.aafInfoAnswer = .failure(VizError.Model(message: "not an AAF file"))
        #expect(s.open(path: "/x/a.aaf") == nil)
        await s.settle()
        #expect(s.documents.isEmpty && box.made.isEmpty && s.pending.isEmpty)
        let a = try #require(s.takeAlert())
        #expect(a.title == "Import Failed" && a.message.contains("not an AAF file"))

        // A failed job on the still-untouched project takes it with it.
        let (t, made, _) = aafStore {
            let f = FakeSession([seq(9, "New", topLevel: true)])
            f.jobStartState = .running
            return f
        }
        t.openAaf("/x/b.aaf")
        await t.settle()
        let id = try #require(t.documents.keys.first)
        let m = try #require(t.model(id))
        let f = try #require(made.made.first)
        let job = try #require(m.jobs.first?.id)
        f.jobRows[job] = JobRow(
            id: job, kind: .importAaf, progress: 1, state: .failed, message: "the file is damaged", problems: [],
            created: [])
        m.pollJobs()
        #expect(t.documents.isEmpty && t.alert == AlertItem(title: "Import Failed", message: "the file is damaged"))
    }

    /// The sheet's "Open as a new project": a project at the file's rate
    /// imports it with the sheet's choices.
    @Test func theSheetsNewProjectCarriesItsChoices() async throws {
        let (s, box, info) = aafStore()
        let doc = try #require(s.newProject(ProjectFormatRow(sampleRate: 48_000, depth: .int24)))
        let a = try #require(s.model(doc)).aaf
        a.readInfo = { try info.aafInfo(path: $0) }
        info.aafInfoAnswer = .success(
            AafInfoRow(sampleRate: 88_200, tracks: 1, audioFiles: 1, offline: 0, fromVizdaw: false, midiBeside: nil))
        a.choose(path: "/x/c.aaf")
        await a.settle()
        a.joinPairs = false
        a.midiPath = "/x/other.mid"
        a.startImport(store: s)
        #expect(!a.importShown && box.formats.last?.sampleRate == 88_200 && box.made.count == 2)
        let fresh = try #require(box.made.last)
        #expect(
            fresh.aafImports
                == [
                    AafImportRequestRow(
                        path: "/x/c.aaf", newProject: true, midiPath: "/x/other.mid", mediaFolder: nil, joinPairs: false)
                ])
        #expect(s.documents.count == 2 && s.pending.count == 1)
    }

    /// The control channel's `file.open` never starts the store's import
    /// (which opens a window): it points to `file.import_aaf`.
    @Test func theChannelsOpenRefusesAnAaf() async throws {
        let (s, box, info) = aafStore()
        let o = ControlOpen.open(
            path: "/x/a.aaf", answers: RecoveryAnswersRow(plugins: nil, restoreAutosave: nil), store: s)
        guard case .failed(let f) = o else { Issue.record("not refused: \(o)"); return }
        #expect(f.code == .invalidParams && f.message.contains("file.import_aaf"))
        await s.settle()
        #expect(s.documents.isEmpty && box.made.isEmpty && info.aafInfoPaths.isEmpty)
    }
}
