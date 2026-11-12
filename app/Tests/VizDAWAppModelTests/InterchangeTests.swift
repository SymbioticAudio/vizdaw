import Foundation
import Testing
import VizDAWCore

@testable import VizDAWAppModel

/// The MIDI import and export sheets' model and the report text (plan 6a
/// Task 9; Rulings 12, T7-a; F12, F13).
@MainActor
@Suite(.serialized) struct InterchangeTests {
    /// The demo with events in its shown sequence's tracks.
    func busy() -> FakeSession {
        let f = demoFake()
        var a = lane(30, "Groove"), b = lane(31, "Chords")
        a.midiEventCount = 12
        b.midiEventCount = 3
        f.laneTable[3] = [a, b]
        return f
    }

    func info(_ kind: MidiFileKindRow, mpe: Bool = false) -> MidiFileInfoRow {
        MidiFileInfoRow(
            kind: kind, tracks: 2,
            channels: [
                ChannelUseRow(group: 0, channel: 0, notes: 10, suggested: 5),
                ChannelUseRow(group: 0, channel: 9, notes: 4, suggested: nil),
                ChannelUseRow(group: 0, channel: 2, notes: 7, suggested: 6),
            ], hasTempo: true, mpeFound: mpe)
    }

    @Test func importSheetDefaults() async throws {
        let f = busy()
        f.midiInfoRow = info(.smf0, mpe: true)
        let m = try DocumentModel(session: f)
        m.interchange.beginImport(path: "/tmp/a.mid")
        await m.interchange.settle()
        let i = m.interchange
        #expect(i.importShown && i.importPath == "/tmp/a.mid" && i.info == f.midiInfoRow)
        #expect(i.splitChannels && i.mpe && !i.useTempo && !i.useTempoLocked)
        #expect(
            i.choices == [
                ChannelChoice(group: 0, channel: 0, dest: 5, notes: 10),
                ChannelChoice(group: 0, channel: 9, dest: nil, notes: 4),
                ChannelChoice(group: 0, channel: 2, dest: 6, notes: 7),
            ])
        f.midiInfoRow = info(.smf1)
        m.interchange.beginImport(path: "/tmp/b.mid")
        await m.interchange.settle()
        #expect(!i.splitChannels && !i.mpe)
    }

    /// Ruling T7-a: the core takes the file's tempo into a sequence without
    /// events, so the sheet never shows a choice that is ignored.
    @Test func anEmptySequenceForcesTheTempoOn() async throws {
        let empty = try DocumentModel(session: demoFake())
        empty.interchange.beginImport(path: "/tmp/a.mid")
        await empty.interchange.settle()
        #expect(empty.interchange.useTempo && empty.interchange.useTempoLocked)
        empty.interchange.useTempo = false
        #expect(empty.interchange.useTempo)
        let f = busy()
        let full = try DocumentModel(session: f)
        full.interchange.beginImport(path: "/tmp/a.mid")
        await full.interchange.settle()
        #expect(!full.interchange.useTempo && !full.interchange.useTempoLocked)
        full.interchange.useTempo = true
        #expect(full.interchange.useTempo)
        full.interchange.startImport()
        #expect(f.midiImports.last?.useTempo == true)
    }

    @Test func startImportSendsTheChoices() async throws {
        let f = busy()
        f.midiInfoRow = info(.smf1)
        let m = try DocumentModel(session: f)
        m.locate(1920)
        m.editor.setSelection(SelectionRow(timeRange: nil, lanes: [LaneRef(seq: 3, lane: 31)], events: []))
        m.interchange.beginImport(path: "/tmp/a.mid")
        await m.interchange.settle()
        m.interchange.choices[1].dest = nil
        m.interchange.choices[2].dest = 9
        m.interchange.mpe = true
        m.interchange.startImport()
        let r = try #require(f.midiImports.last)
        let seq = try #require(m.selectedSeq)
        #expect(
            r == MidiImportRequestRow(
                path: "/tmp/a.mid", newProject: false, seq: seq, lane: 31, tick: 1920,
                channelMap: [
                    ChannelMapRow(group: 0, channel: 0, dest: 5), ChannelMapRow(group: 0, channel: 9, dest: nil),
                    ChannelMapRow(group: 0, channel: 2, dest: 9),
                ], splitChannels: false, mpe: true, useTempo: false))
        #expect(!m.interchange.importShown)
    }

    @Test func aRefusedFileIsAnAlert() async throws {
        let f = busy()
        f.interchangeError = .Model(message: "not a MIDI file")
        let m = try DocumentModel(session: f)
        m.interchange.beginImport(path: "/tmp/a.mid")
        await m.interchange.settle()
        #expect(!m.interchange.importShown)
        #expect(m.alert == AlertItem(title: "Import Failed", message: "not a MIDI file"))
    }

    @Test func exportOptionsRefreshTheDryRun() async throws {
        let f = busy()
        f.checkReply = { req in ReportRow(summary: "ppq \(req.ppq) t0 \(req.type0) mpe \(req.mpe) \(req.lanes)", items: []) }
        let m = try DocumentModel(session: f)
        let i = m.interchange
        i.showExport(.smf)
        await i.settle()
        #expect(i.exportShown && i.lanes == [30, 31] && i.ppq == 960 && !i.type0)
        let base = f.smfChecks.count
        i.type0 = true
        await i.settle()
        #expect(f.smfChecks.count == base + 1)
        i.ppq = 480
        await i.settle()
        #expect(f.smfChecks.count == base + 2)
        i.lanes = [31]
        await i.settle()
        i.exportMpe = true
        await i.settle()
        #expect(f.smfChecks.count == base + 4)
        #expect(f.smfChecks.last?.lanes == [31] && f.smfChecks.last?.mpe == true && f.smfChecks.last?.ppq == 480)
        #expect(i.wontTransfer?.summary == "ppq 480 t0 true mpe true [31]")
        // The clip export has its own check.
        i.showExport(.clip)
        await i.settle()
        #expect(f.clipChecks.count == 1)
        #expect(i.wontTransfer == f.interchangeCheck)
        i.lanes = [30]
        await i.settle()
        #expect(f.clipChecks.count == 2 && f.clipChecks.last?.lanes == [30])
    }

    @Test func exportTracksStartWithTheSelectedOnes() async throws {
        let f = busy()
        let m = try DocumentModel(session: f)
        m.editor.setSelection(
            SelectionRow(timeRange: TickRange(start: 960, end: 1920), lanes: [LaneRef(seq: 3, lane: 31)], events: []))
        m.interchange.showExport(.clip)
        await m.interchange.settle()
        #expect(m.interchange.lanes == [31])
        m.interchange.useSelection = true
        await m.interchange.settle()
        #expect(f.clipChecks.last?.start == 960 && f.clipChecks.last?.end == 1920)
        m.interchange.startExport(path: "/tmp/x.midi2")
        let r = try #require(f.clipExports.last)
        #expect(r.path == "/tmp/x.midi2" && r.lanes == [31] && r.start == 960 && r.end == 1920 && r.replace)
        #expect(!m.interchange.exportShown)
    }

    @Test func reportTextGroupsAndCaps() {
        let r = ReportRow(
            summary: "Imported 2 tracks",
            items: [
                ReportItemRow(kind: .note, what: "The file's title was not kept", count: 1, details: []),
                ReportItemRow(kind: .leftOut, what: "Muted notes", count: 120, details: (1...50).map { "bar \($0)" }),
                ReportItemRow(kind: .changed, what: "Tempo", count: 1, details: ["bar 3"]),
            ])
        let g = ReportText.groups(r)
        #expect(g.map(\.title) == ["Changed", "Left out", "Notes"])
        let left = g[1].items[0]
        #expect(ReportText.line(left) == "Muted notes (120)")
        #expect(ReportText.line(g[0].items[0]) == "Tempo")
        let d = ReportText.details(left)
        #expect(d.count == 51 && d.first == "bar 1" && d.last == "and 70 more")
        #expect(ReportText.details(g[0].items[0]) == ["bar 3"])
        let only = ReportRow(
            summary: "Exported 1 track",
            items: [ReportItemRow(kind: .note, what: "A note", count: 1, details: [])])
        #expect(ReportText.groups(only).map(\.title) == ["Notes"], "empty groups are omitted")
        let plain = ReportText.plain(r)
        #expect(plain.hasPrefix("Imported 2 tracks\n"))
        #expect(plain.contains("Left out\n") && plain.contains("  Muted notes (120)\n    bar 1\n"))
        #expect(plain.contains("    and 70 more"))
        #expect(
            ReportText.plain(ReportRow(summary: "Exported 3 tracks", items: []))
                == "Exported 3 tracks\nEverything transferred.")
        #expect(!plain.lowercased().contains("lane"))
    }

    @Test func reportCommandNeverOpensTheSheet() throws {
        let f = demoFake()
        f.lastReport = ReportRow(
            summary: "Imported", items: [ReportItemRow(kind: .note, what: "x", count: 1, details: [])])
        let s = DocumentStore(makeEmpty: { FakeSession([]) }, makeDemo: { f }, open: { _ in demoFake() })
        let id = try #require(s.newDemo())
        let m = try #require(s.model(id))
        let o = InterchangeCommands.run(InterchangeCommands.report, args: [], id, m, store: s)
        guard case .done = o else { Issue.record("not done: \(o)"); return }
        #expect(!m.interchange.reportShown && m.alert == nil && !m.interchange.importShown && !m.interchange.exportShown)
    }

    @Test func checksRunOffTheMainActor() async throws {
        let f = busy()
        f.midiInfoRow = info(.smf1)
        let release = DispatchSemaphore(value: 0)
        f.checkHold = { $0.ppq == 480 ? release : nil }
        f.checkReply = { ReportRow(summary: "ppq \($0.ppq)", items: []) }
        let m = try DocumentModel(session: f)
        let i = m.interchange
        i.beginImport(path: "/tmp/a.mid")
        await i.settle()
        i.showExport(.smf)
        await i.settle()
        #expect(!f.checkOnMain.isEmpty && !f.checkOnMain.contains(true), "every check ran in a detached task")
        // Two changes in a row: the first is held, the second lands, the first is stale.
        i.ppq = 480
        i.ppq = 15_360
        // The model stays usable while the held check runs.
        i.type0 = false
        #expect(i.exportShown)
        for _ in 0..<500 where i.wontTransfer?.summary != "ppq 15360" { try await Task.sleep(for: .milliseconds(10)) }
        #expect(i.wontTransfer?.summary == "ppq 15360")
        release.signal()
        await i.settle()
        #expect(i.wontTransfer?.summary == "ppq 15360", "a stale result is dropped")
    }

    @Test func aFinishedUiJobShowsTheReport() async throws {
        let f = busy()
        f.jobStartState = .running
        f.lastReport = ReportRow(
            summary: "Imported 2 tracks", items: [ReportItemRow(kind: .note, what: "x", count: 1, details: [])])
        let m = try DocumentModel(session: f)
        m.interchange.beginImport(path: "/tmp/a.mid")
        await m.interchange.settle()
        m.interchange.startImport()
        let id = try #require(m.jobs.first { $0.kind == .importMidi }?.id)
        m.pollJobs()
        #expect(!m.interchange.reportShown, "still running")
        f.jobRows[id] = JobRow(
            id: id, kind: .importMidi, progress: 1, state: .done, message: nil, problems: [], created: [],
            report: f.lastReport, written: [])
        m.pollJobs()
        #expect(m.interchange.reportShown && m.interchange.report == f.lastReport)
        // A job the control channel started is not in `jobs`: nothing opens.
        let g = busy()
        g.jobStartState = .running
        g.lastReport = f.lastReport
        let store = DocumentStore(makeEmpty: { FakeSession([]) }, makeDemo: { g }, open: { _ in demoFake() })
        let docID = try #require(store.newDemo())
        let c = try #require(store.model(docID))
        let o = InterchangeCommands.run(
            InterchangeCommands.importMidi, args: [ControlArg(name: "path", value: .text(value: "/tmp/a.mid"))], docID, c,
            store: store)
        guard case .job(_, _, let job) = o else { Issue.record("not a job: \(o)"); return }
        g.jobRows[job] = JobRow(id: job, kind: .importMidi, progress: 1, state: .done, message: nil, problems: [], created: [])
        c.pollJobs()
        #expect(c.jobs.isEmpty && !c.interchange.reportShown && c.interchange.report == nil)
    }

    @Test func droppedMidiImportsWithDefaults() throws {
        let f = busy()
        let m = try DocumentModel(session: f)
        let target = ImportTargetRow(seq: 3, lane: nil, tick: 3840)
        m.interchange.importDropped(paths: ["/tmp/a.MID", "/tmp/b.wav", "/tmp/c.smf"], target: target)
        #expect(f.midiImports.map(\.path) == ["/tmp/a.MID", "/tmp/c.smf"])
        #expect(f.midiImports.allSatisfy { $0.seq == 3 && $0.tick == 3840 && $0.lane == nil && !$0.newProject })
        #expect(f.midiImports.allSatisfy { $0.channelMap.isEmpty && $0.splitChannels == nil && !$0.mpe && !$0.useTempo })
        #expect(f.imports.count == 1 && f.imports[0].0 == ["/tmp/b.wav"] && f.imports[0].1 == target)
        #expect(InterchangeModel.isMidi("/x/y.MIDI") && InterchangeModel.isMidi("a.mid2") == false)
        #expect(InterchangeModel.isMidi("a.midi2") && InterchangeModel.isMidi("a.Smf") && !InterchangeModel.isMidi("a.wav"))
        #expect(m.alert == nil, "the second file is not refused as busy")
    }

    /// Final review I1: the core takes one MIDI import at a time, so
    /// dropped files queue; each starts when the one before ends, as its
    /// own job (its own tracks and undo step), and a failure is reported
    /// and the queue moves on.
    @Test func severalDroppedMidiFilesImportOneAfterAnother() throws {
        let f = busy()
        f.jobStartState = .running
        let m = try DocumentModel(session: f)
        let target = ImportTargetRow(seq: 3, lane: nil, tick: 0)
        m.interchange.importDropped(paths: ["/tmp/a.mid", "/tmp/b.mid", "/tmp/c.mid"], target: target)
        #expect(f.midiImports.map(\.path) == ["/tmp/a.mid"] && m.alert == nil)
        #expect(throws: VizError.self) { try f.importMidi(req: f.midiImports[0]) }
        func end(_ k: Int, _ state: JobStateRow, _ message: String? = nil) throws {
            let id = try #require(m.jobs.first { $0.kind == .importMidi }?.id)
            f.jobRows[id] = JobRow(
                id: id, kind: .importMidi, progress: 1, state: state, message: message, problems: [], created: [],
                report: ReportRow(summary: "Imported \(k)", items: []), written: [])
            m.pollJobs()
        }
        m.pollJobs()
        #expect(f.midiImports.count == 1, "still running: the next waits")
        try end(1, .done)
        #expect(f.midiImports.map(\.path) == ["/tmp/a.mid", "/tmp/b.mid"] && m.alert == nil)
        #expect(m.interchange.report?.summary == "Imported 1")
        try end(2, .failed, "not a MIDI file")
        #expect(m.alert == AlertItem(title: ProjectBanner.failureTitle(.importMidi), message: "not a MIDI file"))
        #expect(f.midiImports.map(\.path) == ["/tmp/a.mid", "/tmp/b.mid", "/tmp/c.mid"], "moved on")
        try end(3, .done)
        #expect(m.jobs.isEmpty && f.midiImports.count == 3 && m.interchange.dropQueue.isEmpty)
        #expect(f.midiImports.allSatisfy { $0.seq == 3 && $0.lane == nil && !$0.newProject })
    }

    /// Two files whose jobs end at once (on `startJob`'s own poll), and a
    /// file the core refuses at the start: each is tried once, in order.
    @Test func twoDroppedFilesThatEndAtOnceBothImport() throws {
        let f = busy()
        let m = try DocumentModel(session: f)
        let target = ImportTargetRow(seq: 3, lane: 31, tick: 0)
        m.interchange.importDropped(paths: ["/tmp/a.mid", "/tmp/b.mid"], target: target)
        #expect(f.midiImports.map(\.path) == ["/tmp/a.mid", "/tmp/b.mid"] && m.alert == nil && m.jobs.isEmpty)
        f.interchangeError = .Model(message: "bad")
        m.interchange.importDropped(paths: ["/tmp/c.mid", "/tmp/d.mid"], target: target)
        #expect(m.alert?.title == "MIDI Import Failed" && m.interchange.dropQueue.isEmpty)
        f.interchangeError = nil
        // A cancelled drop stops the files after it.
        f.jobStartState = .running
        m.alert = nil
        m.interchange.importDropped(paths: ["/tmp/e.mid", "/tmp/f.mid"], target: target)
        let id = try #require(m.jobs.first?.id)
        f.jobRows[id] = JobRow(id: id, kind: .importMidi, progress: 1, state: .cancelled, message: nil, problems: [], created: [])
        m.pollJobs()
        #expect(f.midiImports.last?.path == "/tmp/e.mid" && m.interchange.dropQueue.isEmpty && m.alert == nil)
    }

    /// Plan 6b Task 4: the core takes one interchange job at a time, so
    /// dropped MIDI files wait for a running export.
    @Test func droppedMidiWaitsForTheExport() async throws {
        let f = busy()
        f.jobStartState = .running
        let m = try DocumentModel(session: f)
        m.forDaw.shown = true
        await m.forDaw.settle()
        m.forDaw.start(folder: "/tmp/x")
        #expect(f.forDawExports.count == 1)
        m.interchange.importDropped(
            paths: ["/tmp/a.mid", "/tmp/b.mid"], target: ImportTargetRow(seq: 3, lane: nil, tick: 0))
        #expect(f.midiImports.isEmpty && m.alert == nil && m.interchange.dropQueue.count == 2)
        let id = try #require(m.jobs.first { $0.kind == .exportProject }?.id)
        f.jobRows[id] = JobRow(id: id, kind: .exportProject, progress: 1, state: .done, message: nil, problems: [], created: [])
        m.pollJobs()
        #expect(f.midiImports.map(\.path) == ["/tmp/a.mid"] && m.alert == nil)
    }

    /// F11: only a cancelled *drop* job clears the queue; a cancelled export
    /// leaves the waiting drops, and the first then starts.
    @Test func cancelledExportKeepsTheDropQueue() async throws {
        let f = busy()
        f.jobStartState = .running
        let m = try DocumentModel(session: f)
        m.forDaw.shown = true
        await m.forDaw.settle()
        m.forDaw.start(folder: "/tmp/x")
        m.interchange.importDropped(
            paths: ["/tmp/a.mid", "/tmp/b.mid"], target: ImportTargetRow(seq: 3, lane: nil, tick: 0))
        let id = try #require(m.jobs.first { $0.kind == .exportProject }?.id)
        f.jobRows[id] = JobRow(id: id, kind: .exportProject, progress: 1, state: .cancelled, message: nil, problems: [], created: [])
        m.pollJobs()
        #expect(f.midiImports.map(\.path) == ["/tmp/a.mid"] && m.interchange.dropQueue.count == 1 && m.alert == nil)
    }

    /// The report's "and N more" follows given details only, as the core's
    /// `Report::plain` does.
    @Test func moreIsOnlyAfterGivenDetails() {
        let bare = ReportItemRow(kind: .leftOut, what: "Muted notes", count: 120, details: [])
        #expect(ReportText.details(bare).isEmpty)
        let some = ReportItemRow(kind: .leftOut, what: "Muted notes", count: 5, details: ["bar 1", "bar 2"])
        #expect(ReportText.details(some) == ["bar 1", "bar 2", "and 3 more"])
        let r = ReportRow(summary: "Exported", items: [bare])
        #expect(ReportText.plain(r) == "Exported\nLeft out\n  Muted notes (120)")
    }

    @Test func openRoutesMidiFiles() throws {
        var made: [FakeSession] = []
        let s = DocumentStore(makeEmpty: { FakeSession([]) }, makeDemo: { demoFake() }, open: { _ in demoFake() })
        s.makeProject = { _ in
            let f = demoFake()
            made.append(f)
            return f
        }
        let id = try #require(s.open(path: "/tmp/a.MID"))
        #expect(s.model(id) != nil && made.count == 1)
        let r = try #require(made[0].midiImports.last)
        #expect(r.path == "/tmp/a.MID" && r.newProject && r.channelMap.isEmpty && r.useTempo)
        #expect(s.alert == nil)
        #expect(s.open(path: "/tmp/a.txt") == nil)
        #expect(s.alert?.title == "Open Failed" && made.count == 1)
    }

    @Test func aRefusedOpenLeavesNothing() throws {
        let s = DocumentStore(makeEmpty: { FakeSession([]) }, makeDemo: { demoFake() }, open: { _ in demoFake() })
        s.makeProject = { _ in
            let f = demoFake()
            f.interchangeError = .Model(message: "not a MIDI file")
            return f
        }
        #expect(s.open(path: "/tmp/a.mid") == nil)
        #expect(s.documents.isEmpty && s.alert?.title == "Import Failed")
    }

    /// Open: a project made for a file the job then refuses goes, with the
    /// alert owned by the store.
    @Test func aFailedOpenJobClosesTheUntouchedProject() throws {
        let f = demoFake()
        f.jobStartState = .running
        let s = DocumentStore(makeEmpty: { FakeSession([]) }, makeDemo: { demoFake() }, open: { _ in demoFake() })
        s.makeProject = { _ in f }
        let id = try #require(s.open(path: "/tmp/a.mid"))
        let m = try #require(s.model(id))
        let job = try #require(m.jobs.first?.id)
        f.jobRows[job] = JobRow(
            id: job, kind: .importMidi, progress: 1, state: .failed, message: "not a MIDI file", problems: [], created: [])
        m.pollJobs()
        #expect(s.model(id) == nil && s.documents.isEmpty)
        #expect(s.alert == AlertItem(title: "Import Failed", message: "not a MIDI file"))
        // Cancelled: closes quietly.
        let g = demoFake()
        g.jobStartState = .running
        s.makeProject = { _ in g }
        let id2 = try #require(s.open(path: "/tmp/b.mid"))
        let m2 = try #require(s.model(id2))
        let j2 = try #require(m2.jobs.first?.id)
        g.jobRows[j2] = JobRow(id: j2, kind: .importMidi, progress: 1, state: .cancelled, message: nil, problems: [], created: [])
        s.alert = nil
        m2.pollJobs()
        #expect(s.documents.isEmpty && s.alert == nil)
    }

    @Test func aFailedOpenJobKeepsAProjectThatWasEdited() throws {
        let f = demoFake()
        f.jobStartState = .running
        let s = DocumentStore(makeEmpty: { FakeSession([]) }, makeDemo: { demoFake() }, open: { _ in demoFake() })
        s.makeProject = { _ in f }
        let id = try #require(s.open(path: "/tmp/a.mid"))
        let m = try #require(s.model(id))
        f.snap.undoLabel = "Rename"
        m.apply(f.snap)
        let job = try #require(m.jobs.first?.id)
        f.jobRows[job] = JobRow(
            id: job, kind: .importMidi, progress: 1, state: .failed, message: "bad", problems: [], created: [])
        m.pollJobs()
        #expect(s.model(id) != nil && m.alert == AlertItem(title: "Import Failed", message: "bad"))
    }

    @Test func aLaterExportFailureReturnsToTheSheet() async throws {
        let f = busy()
        f.jobStartState = .running
        let m = try DocumentModel(session: f)
        m.interchange.showExport(.smf)
        await m.interchange.settle()
        m.interchange.startExport(path: "/tmp/x.mid")
        #expect(!m.interchange.exportShown)
        let job = try #require(m.jobs.first { $0.kind == .exportMidi }?.id)
        f.jobRows[job] = JobRow(
            id: job, kind: .exportMidi, progress: 1, state: .failed, message: "disk full", problems: [], created: [])
        m.pollJobs()
        #expect(m.interchange.exportShown && m.interchange.exportError == "disk full" && m.alert == nil)
    }

    /// An export that fails on the poll inside `startJob` keeps its sheet
    /// open with the reason (Task 10 carry-over).
    @Test func anImmediateExportFailureKeepsTheSheet() async throws {
        let f = busy()
        f.jobStartState = .failed
        let m = try DocumentModel(session: f)
        m.interchange.showExport(.clip)
        await m.interchange.settle()
        m.interchange.startExport(path: "/tmp/x.midi2")
        #expect(m.jobs.isEmpty && f.clipExports.count == 1)
        #expect(m.interchange.exportShown && m.interchange.exportError != nil && m.alert == nil)
        // Refused up front: the sheet stays with the reason.
        f.interchangeError = .Model(message: "inside the project")
        m.interchange.startExport(path: "/tmp/p.vizdaw/x.midi2")
        #expect(m.interchange.exportShown && m.interchange.exportError == "inside the project")
    }

    /// The import sheet's "a new project": the store makes the project and
    /// imports with the sheet's choices, every channel on a new destination
    /// (the new project has none yet).
    @Test func theSheetsNewProjectImportsWithItsChoices() async throws {
        var made: [FakeSession] = []
        let s = DocumentStore(makeEmpty: { FakeSession([]) }, makeDemo: { demoFake() }, open: { _ in demoFake() })
        s.makeProject = { _ in
            let f = demoFake()
            made.append(f)
            return f
        }
        let f = busy()
        f.midiInfoRow = info(.smf1)
        let m = try DocumentModel(session: f)
        m.interchange.beginImport(path: "/tmp/a.mid")
        await m.interchange.settle()
        #expect(!m.interchange.intoNewProject)
        m.interchange.intoNewProject = true
        m.interchange.splitChannels = true
        m.interchange.mpe = true
        let id = try #require(s.importAsNewProject(m.interchange))
        #expect(s.model(id) != nil && !m.interchange.importShown && f.midiImports.isEmpty)
        let r = try #require(made.first?.midiImports.last)
        #expect(r.path == "/tmp/a.mid" && r.newProject && r.useTempo && r.splitChannels == true && r.mpe)
        #expect(r.channelMap.count == 3 && r.channelMap.allSatisfy { $0.dest == nil })
        // Read again: the choice starts as this project.
        m.interchange.beginImport(path: "/tmp/a.mid")
        await m.interchange.settle()
        #expect(!m.interchange.intoNewProject)
    }

    /// The report sheet offers Show in Finder for what an export wrote.
    @Test func anExportsReportKeepsWhatItWrote() async throws {
        let f = busy()
        f.jobStartState = .running
        let m = try DocumentModel(session: f)
        m.interchange.showExport(.smf)
        await m.interchange.settle()
        m.interchange.startExport(path: "/tmp/x.mid")
        let job = try #require(m.jobs.first { $0.kind == .exportMidi }?.id)
        let report = ReportRow(summary: "Exported 2 tracks", items: [])
        f.jobRows[job] = JobRow(
            id: job, kind: .exportMidi, progress: 1, state: .done, message: nil, problems: [], created: [],
            report: report, written: ["/tmp/x.mid"])
        m.pollJobs()
        #expect(m.interchange.reportShown && m.interchange.written == ["/tmp/x.mid"])
        // The same report again keeps them; another one has none.
        f.lastReport = report
        m.interchange.reportShown = false
        m.interchange.showLastReport()
        #expect(m.interchange.reportShown && m.interchange.written == ["/tmp/x.mid"])
        f.lastReport = ReportRow(summary: "Imported 1 track", items: [])
        m.interchange.showLastReport()
        #expect(m.interchange.report == f.lastReport && m.interchange.written.isEmpty)
    }

    /// Review round 1, final review I3: several clip tracks go into a
    /// folder, which never replaces files silently. The core names the
    /// files and says which are taken (asked off the main actor); the
    /// export replaces only when the user chose Replace.
    @Test func aClipFolderExportAsksBeforeReplacing() async throws {
        let f = busy()
        let m = try DocumentModel(session: f)
        let x = m.interchange
        x.showExport(.clip)
        await x.settle()
        f.clipNames = ClipNamesRow(names: ["01 A.midi2", "02 B.midi2"], taken: [])
        x.exportToFolder("/tmp/out")
        await x.settle()
        let asked = try #require(f.clipNameRequests.last)
        #expect(asked.path == "/tmp/out" && asked.lanes == [30, 31] && asked.replace == false)
        #expect(f.checkOnMain.allSatisfy { !$0 }, "asked off the main actor")
        #expect(f.clipExports.last?.path == "/tmp/out" && f.clipExports.last?.replace == false && x.replaceQuestion == nil)
        // One of the names is taken: asked, nothing written yet.
        x.showExport(.clip)
        await x.settle()
        f.clipNames = ClipNamesRow(names: ["01 A.midi2", "02 B.midi2"], taken: ["02 B.midi2"])
        x.exportToFolder("/tmp/out")
        await x.settle()
        #expect(f.clipExports.count == 1 && x.exportShown)
        let q = try #require(x.replaceQuestion)
        #expect(q.taken == ["02 B.midi2"] && q.message == "1 file already exists in out. Replace it?")
        x.answerReplace(false)
        #expect(f.clipExports.count == 1 && x.replaceQuestion == nil && x.exportShown)
        f.clipNames = ClipNamesRow(names: ["01 A.midi2", "02 B.midi2"], taken: ["01 A.midi2", "02 B.midi2"])
        x.exportToFolder("/tmp/out")
        await x.settle()
        #expect(x.replaceQuestion?.message == "2 files already exist in out. Replace them?")
        x.answerReplace(true)
        #expect(f.clipExports.count == 2 && f.clipExports.last?.replace == true && x.replaceQuestion == nil)
        // The core's refusal (a file that appeared meanwhile) shows in the sheet.
        x.showExport(.clip)
        await x.settle()
        f.clipNames = ClipNamesRow(names: ["01 A.midi2", "02 B.midi2"], taken: [])
        f.interchangeError = .Model(message: "01 A.midi2 exists; choose another name or replace it")
        x.exportToFolder("/tmp/out")
        await x.settle()
        #expect(x.exportShown && x.exportError == "01 A.midi2 exists; choose another name or replace it")
        #expect(m.alert == nil)
        f.interchangeError = nil
        // A folder the core refuses: the reason, nothing started.
        f.clipNamesError = .Model(message: "out is not a folder")
        x.exportToFolder("/tmp/out")
        await x.settle()
        #expect(x.exportShown && x.exportError == "out is not a folder" && f.clipExports.count == 2)
    }
}
