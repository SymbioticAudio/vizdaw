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
        #expect(box.made.map { $0.dawprojectImports.map(\.path) } == [["/tmp/a.dawproject"], ["/tmp/B.DAWPROJECT"]])
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

/// A hostile DAWproject through the real library (offline audio, a
/// temporary support folder): names from the file reach the report sheet
/// cleaned by the core (control characters and bidirectional overrides
/// dropped), as 6a's and 6b's reports are.
@MainActor
@Suite(.serialized) struct DawprojectIntegrationTests {
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
