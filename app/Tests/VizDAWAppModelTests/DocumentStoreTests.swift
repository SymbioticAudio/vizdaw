import Foundation
import Testing
import VizDAWCore

@testable import VizDAWAppModel

/// Opening a `.dawproject` through the store (plan 6d Task 12; L8): the
/// file is read off the main actor with no document open, then a project at
/// its rate imports it. A refusal makes nothing; a failed or cancelled job
/// on a still-untouched project closes it.
extension DocumentStoreTests {
    /// The store, the formats it made projects in, the fresh projects'
    /// sessions, and the fake standing in for the free function.
    func dawStore(
        _ make: @escaping () -> FakeSession = { FakeSession([seq(9, "New", topLevel: true)]) }
    ) -> (DocumentStore, DawBox, FakeSession) {
        let info = demoFake()
        let box = DawBox()
        let s = DocumentStore(makeEmpty: { FakeSession([]) }, makeDemo: { demoFake() }, open: { _ in demoFake() })
        s.makeProject = { fmt in
            let f = make()
            box.formats.append(fmt)
            box.made.append(f)
            return f
        }
        s.dawprojectInfo = { try info.dawprojectInfo(path: $0) }
        return (s, box, info)
    }

    final class DawBox {
        var formats: [ProjectFormatRow] = []
        var made: [FakeSession] = []
    }

    @Test func openingADawprojectMakesAProjectAtItsRate() async throws {
        let (s, box, info) = dawStore()
        s.defaultFormat = ProjectFormatRow(sampleRate: 96_000, depth: .int16)
        info.dawprojectInfoAnswer = .success(
            DawprojectInfoRow(sampleRate: 44_100, tracks: 2, audioFiles: 0, fromVizdaw: false, exportId: nil))
        s.openDawproject("/tmp/a.dawproject")
        #expect(s.documents.isEmpty && box.formats.isEmpty, "nothing is made before the file is read")
        await s.settle()
        #expect(info.dawprojectInfoPaths == ["/tmp/a.dawproject"])
        #expect(info.checkOnMain == [false], "the info read ran off the main actor")
        #expect(box.formats == [ProjectFormatRow(sampleRate: 44_100, depth: .int16)])
        let fresh = try #require(box.made.first)
        #expect(fresh.dawprojectImports == [DawprojectImportRequestRow(path: "/tmp/a.dawproject")])
        let id = try #require(s.documents.keys.first)
        #expect(s.documents.count == 1 && s.pending == [id], "the new project's window opens")
        #expect(s.alert == nil)
    }

    @Test func aRefusedFileMakesNoProject() async throws {
        let (s, box, info) = dawStore()
        info.dawprojectInfoAnswer = .failure(VizError.Model(message: "not a DAWproject file"))
        s.openDawproject("/tmp/a.dawproject")
        await s.settle()
        #expect(s.documents.isEmpty && box.made.isEmpty && s.pending.isEmpty)
        let a = try #require(s.takeAlert())
        #expect(a.title == "Import Failed" && a.message.contains("not a DAWproject file"))
        // Refused by the new project's session as the job starts: it goes.
        let (t, made, _) = dawStore {
            let f = FakeSession([seq(9, "New", topLevel: true)])
            f.interchangeError = VizError.Busy(message: "Wait for the running import, export or bounce to finish.")
            return f
        }
        t.openDawproject("/tmp/b.dawproject")
        await t.settle()
        #expect(made.made.count == 1 && t.documents.isEmpty && t.pending.isEmpty)
        #expect(t.takeAlert()?.title == "Import Failed")
    }

    @Test func aCancelledImportClosesTheUntouchedProject() async throws {
        let (s, box, _) = dawStore {
            let f = FakeSession([seq(9, "New", topLevel: true)])
            f.jobStartState = .running
            return f
        }
        s.openDawproject("/tmp/a.dawproject")
        await s.settle()
        let id = try #require(s.documents.keys.first)
        let m = try #require(s.model(id))
        let f = try #require(box.made.first)
        let job = try #require(m.jobs.first?.id)
        f.jobRows[job] = JobRow(id: job, kind: .importProject, progress: 1, state: .cancelled, message: nil, problems: [], created: [])
        m.pollJobs()
        #expect(s.documents.isEmpty && s.alert == nil, "closed quietly")

        // Edited meanwhile: it stays.
        s.openDawproject("/tmp/b.dawproject")
        await s.settle()
        let id2 = try #require(s.documents.keys.first)
        let m2 = try #require(s.model(id2))
        let g = try #require(box.made.last)
        g.snap.undoLabel = "Rename"
        m2.apply(g.snap)
        let j2 = try #require(m2.jobs.first?.id)
        g.jobRows[j2] = JobRow(id: j2, kind: .importProject, progress: 1, state: .cancelled, message: nil, problems: [], created: [])
        m2.pollJobs()
        #expect(s.model(id2) != nil && s.alert == nil)
    }

    /// Controller ruling: a failed UI import leaves no empty project open.
    @Test func aFailedImportClosesTheUntouchedProject() async throws {
        let (s, box, _) = dawStore {
            let f = FakeSession([seq(9, "New", topLevel: true)])
            f.jobStartState = .running
            return f
        }
        s.openDawproject("/tmp/a.dawproject")
        await s.settle()
        let id = try #require(s.documents.keys.first)
        let m = try #require(s.model(id))
        let f = try #require(box.made.first)
        let job = try #require(m.jobs.first?.id)
        f.jobRows[job] = JobRow(
            id: job, kind: .importProject, progress: 1, state: .failed, message: "the file is damaged", problems: [],
            created: [])
        m.pollJobs()
        #expect(s.documents.isEmpty)
        #expect(s.alert == AlertItem(title: "Import Failed", message: "the file is damaged"))

        // Edited meanwhile: it stays, with the alert in its window.
        s.alert = nil
        s.openDawproject("/tmp/b.dawproject")
        await s.settle()
        let id2 = try #require(s.documents.keys.first)
        let m2 = try #require(s.model(id2))
        let g = try #require(box.made.last)
        g.snap.undoLabel = "Rename"
        m2.apply(g.snap)
        let j2 = try #require(m2.jobs.first?.id)
        g.jobRows[j2] = JobRow(id: j2, kind: .importProject, progress: 1, state: .failed, message: "bad", problems: [], created: [])
        m2.pollJobs()
        #expect(s.model(id2) != nil && m2.alert == AlertItem(title: "Import Failed", message: "bad") && s.alert == nil)
    }

    @Test func aFinishedImportShowsItsReport() async throws {
        let (s, box, _) = dawStore {
            let f = FakeSession([seq(9, "New", topLevel: true)])
            f.jobStartState = .running
            return f
        }
        s.openDawproject("/tmp/a.dawproject")
        await s.settle()
        let id = try #require(s.documents.keys.first)
        let m = try #require(s.model(id))
        let f = try #require(box.made.first)
        let job = try #require(m.jobs.first?.id)
        f.jobRows[job] = JobRow(
            id: job, kind: .importProject, progress: 1, state: .done, message: nil, problems: [], created: [],
            report: ReportRow(summary: "Imported 2 tracks", items: []))
        m.pollJobs()
        #expect(s.model(id) != nil && m.interchange.reportShown && m.interchange.written.isEmpty)
        #expect(m.interchange.report?.summary.hasPrefix("Imported") == true)
    }

    @Test func openPathRoutesByExtension() async throws {
        let (s, box, _) = dawStore()
        #expect(s.open(path: "/tmp/a.dawproject") == nil)
        #expect(s.open(path: "/tmp/B.DAWPROJECT") == nil)
        await s.settle()
        // The two opens read their info on detached tasks, so either may finish first.
        #expect(Set(box.made.flatMap { $0.dawprojectImports.map(\.path) }) == ["/tmp/a.dawproject", "/tmp/B.DAWPROJECT"])
        #expect(box.made.allSatisfy { $0.dawprojectImports.count == 1 })
        #expect(s.documents.count == 2 && s.alert == nil)
        #expect(InterchangeModel.isDawproject("/x/y.DawProject") && !InterchangeModel.isDawproject("/x/y.zip"))
        #expect(s.open(path: "/tmp/a.zip") == nil)
        await s.settle()
        #expect(s.alert == AlertItem(title: "Open Failed", message: "a.zip is not a .vizdaw project.") && box.made.count == 2)
    }

    /// The control channel's `file.open` never starts the store's import
    /// (which opens a window): it points to `file.import_dawproject`.
    @Test func theChannelsOpenRefusesADawproject() async throws {
        let (s, box, info) = dawStore()
        let o = ControlOpen.open(
            path: "/tmp/a.dawproject", answers: RecoveryAnswersRow(plugins: nil, restoreAutosave: nil), store: s)
        guard case .failed(let f) = o else { Issue.record("not refused: \(o)"); return }
        #expect(f.code == .invalidParams && f.message.contains("file.import_dawproject"))
        await s.settle()
        #expect(s.documents.isEmpty && box.made.isEmpty && info.dawprojectInfoPaths.isEmpty && s.pending.isEmpty)
    }
}

/// The return prompt's state (plan 6e Task 10; Rulings 13, 14): a file
/// that came from VizDAW asks before anything is made; New Project is 6d's
/// path, Merge goes to the open match or a chosen project, Cancel does
/// nothing.
extension DocumentStoreTests {
    static let pid = "0f8b6f8e-3c1a-4d2e-9b7a-5e6f7a8b9c0d"
    static let eid = "1a2b3c4d-5e6f-4a7b-8c9d-0e1f2a3b4c5d"

    func returned(project: String? = pid, export: String? = eid) -> DawprojectInfoRow {
        DawprojectInfoRow(
            sampleRate: 44_100, tracks: 2, audioFiles: 0, fromVizdaw: true, exportId: export, projectId: project)
    }

    /// `dawStore` whose `open` makes running-job fakes (logged in `opened`).
    func returnStore() -> (DocumentStore, DawBox, FakeSession, OpenBox) {
        let (s, box, info) = dawStore {
            let f = FakeSession([seq(9, "New", topLevel: true)])
            f.jobStartState = .running
            return f
        }
        let opens = OpenBox()
        let t = DocumentStore(
            makeEmpty: { FakeSession([]) }, makeDemo: { demoFake() },
            open: { path in
                let f = demoFake()
                f.jobStartState = .running
                f.savedPath = path
                opens.paths.append(path)
                opens.made.append(f)
                return f
            })
        t.makeProject = s.makeProject
        t.dawprojectInfo = s.dawprojectInfo
        info.dawprojectInfoAnswer = .success(returned())
        return (t, box, info, opens)
    }

    final class OpenBox {
        var paths: [String] = []
        var made: [FakeSession] = []
    }

    /// An open project whose ID is `pid`, named "Song".
    func openMatch(_ s: DocumentStore) throws -> (UUID, FakeSession) {
        let (id, f) = try openProject(s, "/tmp/Song.vizdaw")
        f.projectIdAnswer = Self.pid
        return (id, f)
    }

    func openProject(_ s: DocumentStore, _ path: String) throws -> (UUID, FakeSession) {
        let id = try #require(s.open(path: path))
        let m = try #require(s.model(id))
        let f = try #require(m.session as? FakeSession)
        f.snap.title = ((path as NSString).lastPathComponent as NSString).deletingPathExtension
        m.apply(f.snap)
        return (id, f)
    }

    func noCalls(_ fakes: [FakeSession]) -> Bool {
        fakes.allSatisfy { $0.dawprojectImports.isEmpty && $0.dawprojectMerges.isEmpty }
    }

    @Test func aFileFromVizdawPromptsInsteadOfOpening() async throws {
        let (s, box, info, opens) = returnStore()
        s.openDawproject("/tmp/back.dawproject")
        await s.settle()
        #expect(
            s.returnPrompt
                == ReturnPrompt(path: "/tmp/back.dawproject", info: returned(), match: nil, matchName: nil))
        #expect(box.made.isEmpty && s.documents.isEmpty && s.pending.isEmpty && opens.paths.isEmpty && s.alert == nil)
        #expect(info.checkOnMain == [false], "the file was read off the main actor")
        // A plain file still makes its project at once.
        info.dawprojectInfoAnswer = .success(
            DawprojectInfoRow(sampleRate: 48_000, tracks: 1, audioFiles: 0, fromVizdaw: false, exportId: nil))
        s.answerReturn(.cancel)
        s.openDawproject("/tmp/plain.dawproject")
        await s.settle()
        #expect(s.returnPrompt == nil && box.made.count == 1)
        #expect(box.made.first?.dawprojectImports == [DawprojectImportRequestRow(path: "/tmp/plain.dawproject")])
    }

    @Test func thePromptNamesTheOpenMatch() async throws {
        let (s, _, _, _) = returnStore()
        _ = try openProject(s, "/tmp/Other.vizdaw")
        let (id, _) = try openMatch(s)
        s.openDawproject("/tmp/back.dawproject")
        await s.settle()
        let p = try #require(s.returnPrompt)
        #expect(p.match == id && p.matchName == "Song")
        // Matched by snapshot when no open project has its ID.
        let (t, _, _, _) = returnStore()
        let (byExport, g) = try openProject(t, "/tmp/Kept.vizdaw")
        let eid = Self.eid
        g.hasSnapshotAnswer = { $0 == eid }
        t.openDawproject("/tmp/back.dawproject")
        await t.settle()
        #expect(t.returnPrompt?.match == byExport && t.returnPrompt?.matchName == "Kept")
    }

    @Test func newProjectAnswerOpensAsSixD() async throws {
        let (s, box, _, _) = returnStore()
        s.defaultFormat = ProjectFormatRow(sampleRate: 96_000, depth: .int16)
        let (_, match) = try openMatch(s)
        s.openDawproject("/tmp/back.dawproject")
        await s.settle()
        s.answerReturn(.newProject)
        #expect(s.returnPrompt == nil)
        #expect(box.formats == [ProjectFormatRow(sampleRate: 44_100, depth: .int16)])
        let fresh = try #require(box.made.first)
        #expect(fresh.dawprojectImports == [DawprojectImportRequestRow(path: "/tmp/back.dawproject")])
        #expect(box.made.count == 1 && match.dawprojectMerges.isEmpty && s.pending.count == 1)
    }

    @Test func mergeAnswerGoesToTheMatch() async throws {
        let (s, box, _, opens) = returnStore()
        let (id, match) = try openMatch(s)
        var activated: [UUID] = []
        s.activate = { activated.append($0) }
        s.openDawproject("/tmp/back.dawproject")
        await s.settle()
        s.answerReturn(.merge)
        let m = try #require(s.model(id))
        await m.merge.settle()
        #expect(s.returnPrompt == nil && m.merge.shown && m.merge.path == "/tmp/back.dawproject")
        #expect(
            match.dawprojectMerges
                == [DawprojectMergeRequestRow(path: "/tmp/back.dawproject", snapshot: nil, step: .hold, date: MergeDate.today())])
        #expect(activated == [id], "the project's window comes to the front")
        #expect(box.made.isEmpty && opens.paths == ["/tmp/Song.vizdaw"])
    }

    @Test func mergeWithoutAMatchAsksForAProject() async throws {
        let (s, box, _, opens) = returnStore()
        var asked = 0
        s.chooseMergeProject = {
            asked += 1
            return "/tmp/Chosen.vizdaw"
        }
        s.openDawproject("/tmp/back.dawproject")
        await s.settle()
        s.answerReturn(.merge)
        #expect(asked == 1 && opens.paths == ["/tmp/Chosen.vizdaw"])
        let id = try #require(s.documentID(forPath: "/tmp/Chosen.vizdaw"))
        let m = try #require(s.model(id))
        await m.merge.settle()
        #expect(m.merge.shown && s.pending == [id], "the opened project's window opens")
        #expect(opens.made.first?.dawprojectMerges.map(\.step) == [.hold] && box.made.isEmpty)
        // Already open: found, not opened again.
        s.openDawproject("/tmp/back2.dawproject")
        await s.settle()
        let first = try #require(m.merge.job)
        m.merge.cancel()
        let g = try #require(opens.made.first)
        g.jobRows[first] = JobRow(
            id: first, kind: .mergeDawproject, progress: 1, state: .cancelled, message: nil, problems: [], created: [])
        m.pollJobs()
        s.answerReturn(.merge)
        await m.merge.settle()
        #expect(asked == 2 && opens.paths == ["/tmp/Chosen.vizdaw"])
        #expect(opens.made.first?.dawprojectMerges.map(\.path) == ["/tmp/back.dawproject", "/tmp/back2.dawproject"])
        // The panel cancelled: nothing.
        s.chooseMergeProject = {
            asked += 1
            return nil
        }
        s.openDawproject("/tmp/back3.dawproject")
        await s.settle()
        s.answerReturn(.merge)
        #expect(asked == 3 && s.returnPrompt == nil && opens.paths.count == 1 && s.documents.count == 1)
        #expect(opens.made.first?.dawprojectMerges.count == 2 && s.alert == nil)
    }

    @Test func cancelAnswerDoesNothing() async throws {
        let (s, box, _, opens) = returnStore()
        let (_, match) = try openMatch(s)
        var asked = 0
        s.chooseMergeProject = {
            asked += 1
            return "/tmp/Chosen.vizdaw"
        }
        s.openDawproject("/tmp/back.dawproject")
        await s.settle()
        s.answerReturn(.cancel)
        #expect(s.returnPrompt == nil && box.made.isEmpty && opens.paths == ["/tmp/Song.vizdaw"] && asked == 0)
        #expect(noCalls([match]) && s.documents.count == 1 && s.alert == nil)
        // An answer with no prompt does nothing either.
        s.answerReturn(.merge)
        s.answerReturn(.newProject)
        #expect(box.made.isEmpty && noCalls([match]) && asked == 0)
    }

    /// A second file read while a prompt shows waits its turn (never dropped).
    @Test func promptsQueue() async throws {
        let (s, _, _, _) = returnStore()
        s.openDawproject("/tmp/one.dawproject")
        await s.settle()
        s.openDawproject("/tmp/two.dawproject")
        await s.settle()
        #expect(s.returnPrompt?.path == "/tmp/one.dawproject")
        s.answerReturn(.cancel)
        #expect(s.returnPrompt?.path == "/tmp/two.dawproject")
        s.answerReturn(.cancel)
        #expect(s.returnPrompt == nil)
    }
}

/// A hostile DAWproject through the real library (offline audio, a
/// temporary support folder): names from the file reach the report sheet
/// cleaned by the core (control characters and bidirectional overrides
/// dropped), as 6a's and 6b's reports are.
@MainActor
@Suite(.serialized) struct DawprojectIntegrationTests {
    init() { _ = TestSupportDir.shared }

    @Test func hostileNamesReachTheReportCleaned() async throws {
        setenv("VIZDAW_AUDIO", "offline", 1)
        let dir = FileManager.default.temporaryDirectory
            .appendingPathComponent("vizdaw-swift-dawproject-\(ProcessInfo.processInfo.processIdentifier)")
        try? FileManager.default.removeItem(at: dir)
        try FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(at: dir) }
        _ = TestSupportDir.shared
        let src = dir.appendingPathComponent("src")
        try FileManager.default.createDirectory(at: src, withIntermediateDirectories: true)
        let project = """
            <?xml version="1.0" encoding="UTF-8" standalone="yes"?>
            <Project version="1.0">
              <Application name="Fixture" version="1"/>
              <Transport>
                <Tempo id="tempo" unit="bpm" value="120"/>
                <TimeSignature id="timesig" numerator="4" denominator="4"/>
              </Transport>
              <Structure>
                <Track id="t1" name="Ke\u{1}y&#x202E;s\u{202E}" comment="Hello" contentType="notes">
                  <Channel id="c1" role="regular" audioChannels="2" destination="cmaster">
                    <Volume id="c1v" unit="linear" value="0.8"/>
                  </Channel>
                </Track>
                <Track id="master" name="Master" contentType="audio notes">
                  <Channel id="cmaster" role="master" audioChannels="2">
                    <Volume id="cmasterv" unit="linear" value="1"/>
                  </Channel>
                </Track>
              </Structure>
              <Arrangement id="arr">
                <Lanes id="lanes" timeUnit="beats"/>
              </Arrangement>
            </Project>
            """
        try Data(project.utf8).write(to: src.appendingPathComponent("project.xml"))
        let file = dir.appendingPathComponent("Hostile.dawproject")
        let zip = Process()
        zip.executableURL = URL(fileURLWithPath: "/usr/bin/zip")
        zip.arguments = ["-q", "-X", "-j", file.path, src.appendingPathComponent("project.xml").path]
        try zip.run()
        zip.waitUntilExit()
        #expect(zip.terminationStatus == 0)

        let s = DocumentStore(makeEmpty: { FakeSession([]) }, makeDemo: { demoFake() }, open: { _ in demoFake() })
        s.makeProject = { try Session.newProject(format: $0) }
        defer { s.closeAll() }
        s.openDawproject(file.path)
        await s.settle()
        #expect(s.alert == nil, "\(s.alert?.message ?? "")")
        let m = try #require(s.documents.values.first)
        for _ in 0..<500 where !m.jobs.isEmpty {
            m.pollJobs()
            try await Task.sleep(for: .milliseconds(10))
        }
        #expect(m.jobs.isEmpty && m.alert == nil, "\(m.alert?.message ?? "")")
        let report = try #require(m.interchange.report)
        #expect(m.interchange.reportShown && report.summary.hasPrefix("Imported"))
        let shown = ReportText.plain(report)
        #expect(shown.contains("track 'Keys'"), "the name lands in a report line: \(shown)")
        let bad = shown.unicodeScalars.filter {
            ($0 != "\n" && CharacterSet.controlCharacters.contains($0))
                || (0x202A...0x202E).contains($0.value) || (0x2066...0x2069).contains($0.value)
        }
        #expect(bad.isEmpty, "\(bad.map { String($0.value, radix: 16) })")
        #expect(m.lanes.map(\.name).contains("Keys"))
    }
}
