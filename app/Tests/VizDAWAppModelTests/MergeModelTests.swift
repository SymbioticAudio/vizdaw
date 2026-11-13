import Foundation
import Testing
import VizDAWCore

@testable import VizDAWAppModel

/// The Merge DAWproject sheet's model (plan 6e Task 10): the Hold job
/// watched through the document's job polling, the snapshot list read off
/// the main actor, Merge and Cancel, and a held merge that queues what the
/// core would refuse beside it instead of alerting Busy.
@MainActor
@Suite(.serialized) struct MergeModelTests {
    static let pid = "0f8b6f8e-3c1a-4d2e-9b7a-5e6f7a8b9c0d"
    static let eid = "1a2b3c4d-5e6f-4a7b-8c9d-0e1f2a3b4c5d"
    static let other = "2b3c4d5e-6f7a-4b8c-9d0e-1f2a3b4c5d6e"
    static let path = "/tmp/back.dawproject"

    func info(export: String? = eid) -> DawprojectInfoRow {
        DawprojectInfoRow(
            sampleRate: 48_000, tracks: 2, audioFiles: 0, fromVizdaw: true, exportId: export, projectId: Self.pid)
    }

    /// A document whose jobs keep running until a test ends them.
    func document() throws -> (DocumentModel, FakeSession) {
        let f = demoFake()
        f.jobStartState = .running
        f.projectIdAnswer = Self.pid
        f.savedPath = "/tmp/Song.vizdaw"
        return (try DocumentModel(session: f), f)
    }

    static let previewRow = MergePreviewRow(
        applied: 3, kept: 1, added: 1, removed: 0, conflicts: 2,
        conflictLines: ["Drums: level changed in both", "Keys: notes changed in both"],
        summary: "Merged: 3 applied, 1 kept, 1 added, 0 removed, 2 in conflict", exportId: eid,
        exportedAt: 1_800_000_000)

    /// Ends job `id` as the core would answer it, then polls.
    func end(
        _ f: FakeSession, _ m: DocumentModel, _ id: UInt64, _ state: JobStateRow, message: String? = nil,
        merge: MergePreviewRow? = nil
    ) {
        f.jobRows[id] = JobRow(
            id: id, kind: .mergeDawproject, progress: 1, state: state, message: message, problems: [], created: [],
            report: nil, written: [], merge: merge)
        m.pollJobs()
    }

    /// Begun and held with the preview.
    func held() async throws -> (DocumentModel, FakeSession, MergeModel, UInt64) {
        let (m, f) = try document()
        let g = m.merge
        g.begin(path: Self.path, info: info())
        await g.settle()
        let job = try #require(g.job)
        end(f, m, job, .done, merge: Self.previewRow)
        return (m, f, g, job)
    }

    @Test func previewArrivesFromTheHeldJob() async throws {
        let (m, f) = try document()
        let g = m.merge
        g.begin(path: Self.path, info: info())
        await g.settle()
        #expect(g.shown && g.path == Self.path && g.running && g.preview == nil && !g.canMerge)
        #expect(
            f.dawprojectMerges
                == [DawprojectMergeRequestRow(path: Self.path, snapshot: nil, step: .hold, date: MergeDate.today())])
        #expect(f.snapshotCalls == 0, "a file with its export ID needs no list")
        let job = try #require(g.job)
        end(f, m, job, .done, merge: Self.previewRow)
        #expect(g.preview == Self.previewRow && !g.running && g.held && g.canMerge && g.error == nil)
        #expect(g.shown && m.alert == nil)
    }

    @Test func aLostExportIDListsSnapshotsOffTheMainActor() async throws {
        let (m, f) = try document()
        let rows = [
            SnapshotRow(exportId: Self.other, exportedAt: 1_800_000_500, tracks: 3),
            SnapshotRow(exportId: Self.eid, exportedAt: 1_800_000_000, tracks: 1),
        ]
        f.snapshotsAnswer = .success(rows)
        let g = m.merge
        g.begin(path: Self.path, info: info(export: nil))
        await g.settle()
        #expect(g.snapshots == rows && g.shown)
        #expect(f.snapshotCalls == 1 && f.checkOnMain == [false], "the list was read off the main actor")
        #expect(f.dawprojectMerges.isEmpty && g.job == nil, "no job before a snapshot is chosen")
        // Another file while the choice waits: it waits too (review m6).
        g.begin(path: "/tmp/second.dawproject", info: info())
        await g.settle()
        #expect(g.path == Self.path && g.snapshots == rows && f.snapshotCalls == 1)
        g.chosenSnapshot = Self.eid
        await g.settle()
        #expect(
            f.dawprojectMerges
                == [DawprojectMergeRequestRow(path: Self.path, snapshot: Self.eid, step: .hold, date: MergeDate.today())])
        #expect(g.running)
    }

    @Test func noSnapshotsShowsTheSentence() async throws {
        let (m, f) = try document()
        f.snapshotsAnswer = .success([])
        let g = m.merge
        g.begin(path: Self.path, info: info(export: nil))
        await g.settle()
        #expect(g.snapshots.isEmpty)
        #expect(
            f.dawprojectMerges
                == [DawprojectMergeRequestRow(path: Self.path, snapshot: nil, step: .hold, date: MergeDate.today())])
        let job = try #require(g.job)
        // The core's sentence (its `NOT_FROM_HERE`): Swift never builds it.
        let sentence = "This file was not exported from that project."
        end(f, m, job, .failed, message: sentence)
        #expect(g.error == sentence && !g.canMerge && !g.held && g.shown)
        #expect(m.alert == nil, "the sheet shows it, not an alert")
    }

    @Test func confirmShowsTheReport() async throws {
        let (m, f, g, job) = try await held()
        let report = ReportRow(
            summary: "Merged: 3 applied, 1 kept, 1 added, 0 removed, 2 in conflict",
            items: [ReportItemRow(kind: .conflict, what: "Drums: level changed in both", count: 1, details: [])])
        f.confirmMergeAnswer = .success(report)
        g.confirm()
        #expect(f.confirmedMerges == [job])
        #expect(m.interchange.report == report && m.interchange.reportShown && m.interchange.written.isEmpty)
        #expect(!g.shown && !g.held && m.alert == nil)
    }

    /// Review Focus 3: the core's refusal stays in the sheet.
    @Test func aRefusedConfirmShowsTheSentence() async throws {
        let (m, f, g, job) = try await held()
        let sentence = "The project changed after the preview; preview the merge again."
        f.confirmMergeAnswer = .failure(VizError.Model(message: sentence))
        g.confirm()
        #expect(f.confirmedMerges == [job])
        #expect(g.error == sentence && g.shown && m.alert == nil && !m.interchange.reportShown)
        #expect(!g.canMerge, "the core dropped the hold: Merge is unavailable")
        // Busy keeps the hold (recording, an open edit): Merge stays.
        let (m2, f2, g2, _) = try await held()
        f2.confirmMergeAnswer = .failure(VizError.Busy(message: "Stop recording, then merge."))
        g2.confirm()
        #expect(g2.error == "Stop recording, then merge." && g2.shown && g2.canMerge && m2.alert == nil)
    }

    @Test func cancelSendsCancelMerge() async throws {
        let (m, f, g, job) = try await held()
        defer { withExtendedLifetime(m) {} }
        g.cancel()
        #expect(f.cancelledMerges == [job] && f.cancelled.isEmpty)
        #expect(!g.shown && !g.held && g.job == nil)

        // Running: the job is cancelled, and no hold is asked to go.
        let (m2, f2) = try document()
        let g2 = m2.merge
        g2.begin(path: Self.path, info: info())
        await g2.settle()
        let running = try #require(g2.job)
        g2.cancel()
        #expect(f2.cancelled == [running] && f2.cancelledMerges.isEmpty && !g2.shown)
        end(f2, m2, running, .cancelled)
        #expect(m2.alert == nil && !g2.shown, "ends quietly")

        // A job that finished as Cancel was pressed: its hold is let go.
        let (m3, f3) = try document()
        let g3 = m3.merge
        g3.begin(path: Self.path, info: info())
        await g3.settle()
        let late = try #require(g3.job)
        g3.cancel()
        end(f3, m3, late, .done, merge: Self.previewRow)
        #expect(f3.cancelledMerges == [late] && !g3.shown && g3.preview == nil)

        // While the snapshot list is read: nothing was sent, nothing lands.
        let (m4, f4) = try document()
        let release = DispatchSemaphore(value: 0)
        f4.snapshotsHold = release
        f4.snapshotsAnswer = .success([SnapshotRow(exportId: Self.eid, exportedAt: 1, tracks: 1)])
        let g4 = m4.merge
        g4.begin(path: Self.path, info: info(export: nil))
        g4.cancel()
        release.signal()
        await g4.settle()
        #expect(g4.snapshots.isEmpty && !g4.shown && f4.dawprojectMerges.isEmpty && f4.cancelled.isEmpty)
    }

    @Test func aDoneHoldDoesNotOpenTheReport() async throws {
        let (m, _, g, _) = try await held()
        #expect(!m.interchange.reportShown && m.interchange.report == nil)
        #expect(g.preview == Self.previewRow)
    }

    /// Carry-over: a held merge counts as running. What the core would
    /// refuse beside it waits (exports, dropped MIDI files) or is a status
    /// line (Save As), never a Busy alert; Merge or Cancel lets them go.
    @Test func aHeldMergeQueuesWhatItBlocks() async throws {
        let (m, f, g, _) = try await held()
        #expect(m.mergeHeld)
        // Save As is refused before the panel, as while recording, with the
        // core's sentence (fix round 1: Swift keeps no copy).
        let sentence = "Finish or cancel the merge waiting to be confirmed, then save as."
        f.saveAsRefusalAnswer = sentence
        #expect(m.saveAction(forceAs: true) == .refused(sentence))
        #expect(m.saveAction(forceAs: false) == .save, "a titled project's Save goes on")
        f.savedPath = nil
        #expect(m.saveAction(forceAs: false) == .refused(sentence), "an untitled Save needs Save As")
        f.savedPath = "/tmp/Song.vizdaw"
        // An export waits.
        m.dawproject.shown = true
        await m.dawproject.settle()
        m.dawproject.start(path: "/tmp/out.dawproject", replace: false)
        #expect(m.dawproject.waiting && f.dawprojectExports.isEmpty)
        // A dropped MIDI file waits.
        let seq = try #require(m.selectedSeq)
        m.interchange.importDropped(paths: ["/tmp/a.mid"], target: ImportTargetRow(seq: seq, lane: nil, tick: 0))
        #expect(f.midiImports.isEmpty)
        // A media job the core refuses is a status line, not an alert.
        f.mediaJobError = VizError.Busy(message: "Finish or cancel the merge waiting to be confirmed.")
        m.convertMedia()
        #expect(m.alert == nil && m.editor.status == "Finish or cancel the merge waiting to be confirmed.")
        f.mediaJobError = nil
        // Another merge waits for this one.
        g.begin(path: "/tmp/second.dawproject", info: info())
        await g.settle()
        #expect(f.dawprojectMerges.count == 1 && g.path == Self.path && g.preview == Self.previewRow)
        // Merge: the dropped file goes first; the export and the second
        // merge then wait for it, as for any running import.
        g.confirm()
        #expect(!m.mergeHeld && m.alert == nil)
        #expect(f.midiImports.count == 1 && m.dawproject.waiting && f.dawprojectExports.isEmpty)
        #expect(g.waiting && g.shown && g.path == "/tmp/second.dawproject" && g.preview == nil)
        let midi = try #require(m.jobs.first { $0.kind == .importMidi }?.id)
        f.jobRows[midi] = JobRow(id: midi, kind: .importMidi, progress: 1, state: .done, message: nil, problems: [], created: [])
        m.pollJobs()
        #expect(f.dawprojectExports.count == 1 && !m.dawproject.waiting && g.waiting)
        let export = try #require(m.jobs.first { $0.kind == .exportDawproject }?.id)
        f.jobRows[export] = JobRow(id: export, kind: .exportDawproject, progress: 1, state: .done, message: nil, problems: [], created: [])
        m.pollJobs()
        #expect(!g.waiting && f.dawprojectMerges.map(\.path) == [Self.path, "/tmp/second.dawproject"] && g.running)
        #expect(m.alert == nil)
    }

    /// Review I1: closing the document during a hold ends it quietly. The
    /// queued export and merge never start, whatever the sheet's binding
    /// does as the window goes.
    @Test func closingDuringAHoldStartsNothing() async throws {
        let f = demoFake()
        f.jobStartState = .running
        f.projectIdAnswer = Self.pid
        f.savedPath = "/tmp/Song.vizdaw"
        let s = DocumentStore(makeEmpty: { FakeSession([]) }, makeDemo: { f }, open: { _ in demoFake() })
        let id = try #require(s.newDemo())
        let m = try #require(s.model(id))
        let g = m.merge
        g.begin(path: Self.path, info: info())
        await g.settle()
        let job = try #require(g.job)
        end(f, m, job, .done, merge: Self.previewRow)
        #expect(m.mergeHeld)
        m.dawproject.shown = true
        await m.dawproject.settle()
        m.dawproject.start(path: "/tmp/out.dawproject", replace: false)
        g.begin(path: "/tmp/second.dawproject", info: info())
        #expect(m.dawproject.waiting && f.dawprojectExports.isEmpty && f.dawprojectMerges.count == 1)
        s.close(id)
        #expect(!g.shown && !g.held && g.job == nil && !g.waiting && !m.mergeHeld)
        // The sheet's binding writes back as the window tears down.
        g.shown = false
        g.cancel()
        g.confirm()
        m.pollJobs()
        #expect(f.dawprojectExports.isEmpty && f.dawprojectMerges.count == 1, "nothing started")
        #expect(f.cancelledMerges.isEmpty && f.confirmedMerges.isEmpty, "closing let the core drop the hold")
        #expect(s.alert == nil && m.alert == nil)
    }

    /// Task 11: a merge queued behind a confirmed one begins while the
    /// first one's report is open. The window shows one sheet at a time,
    /// so the merge sheet waits for the report; a dismissal written back
    /// meanwhile (a sheet SwiftUI did not present) is not Cancel.
    @Test func aQueuedMergeWaitsForTheReportSheet() async throws {
        let (m, f, g, _) = try await held()
        #expect(g.presented)
        g.begin(path: "/tmp/second.dawproject", info: info())
        await g.settle()
        f.confirmMergeAnswer = .success(ReportRow(summary: "Merged: 1 applied, 0 kept, 0 added, 0 removed, 0 in conflict", items: []))
        g.confirm()
        #expect(m.interchange.reportShown && g.shown && g.path == "/tmp/second.dawproject")
        #expect(!g.presented, "the report first")
        let second = try #require(g.job)
        g.presented = false
        #expect(g.shown && g.job == second && f.cancelled.isEmpty && f.cancelledMerges.isEmpty, "not cancelled")
        m.interchange.reportShown = false
        #expect(g.presented, "then the merge sheet")
        // Its own dismissal is Cancel.
        g.presented = false
        #expect(!g.shown && f.cancelled == [second])
    }

    /// Task 11: the sheet's list selects; Preview starts the Hold job with
    /// the selected export (nothing before).
    @Test func previewTakesTheSelectedSnapshot() async throws {
        let (m, f) = try document()
        f.snapshotsAnswer = .success([SnapshotRow(exportId: Self.other, exportedAt: 1_800_000_500, tracks: 3)])
        let g = m.merge
        g.begin(path: Self.path, info: info(export: nil))
        await g.settle()
        #expect(g.selectedSnapshot == Self.other, "the newest is selected")
        g.selectedSnapshot = nil
        g.previewSelected()
        #expect(f.dawprojectMerges.isEmpty)
        g.selectedSnapshot = Self.other
        g.previewSelected()
        #expect(f.dawprojectMerges.map(\.snapshot) == [Self.other] && g.running)
    }

    /// Task 11: after the core let the hold go (the project changed),
    /// Preview Again starts a new Hold job for the same file and snapshot.
    @Test func previewAgainAfterTheProjectChanged() async throws {
        let (m, f, g, _) = try await held()
        #expect(!g.canPreviewAgain)
        f.confirmMergeAnswer = .failure(VizError.Model(message: "The project changed after the preview; preview the merge again."))
        g.confirm()
        #expect(g.canPreviewAgain && !g.canMerge)
        g.previewAgain()
        #expect(f.dawprojectMerges.count == 2 && f.dawprojectMerges.last?.path == Self.path)
        #expect(g.error == nil && g.running && !g.canPreviewAgain && g.preview == nil)
        let job = try #require(g.job)
        end(f, m, job, .done, merge: Self.previewRow)
        #expect(g.canMerge)
    }

    /// Task 11: while a merge is held, a format change waits (Project
    /// Settings' OK is unavailable then) and so does Relink.
    @Test func aHeldMergeBlocksAFormatChangeAndRelink() async throws {
        let (m, _, _, _) = try await held()
        let settings = m.projectSettings
        settings.load()
        #expect(!settings.formatBlocked, "no format change")
        settings.rate = settings.rate == 48_000 ? 44_100 : 48_000
        #expect(settings.formatBlocked)
        #expect(m.media.relinkBlocked, "Relink waits too")
        #expect(!MergeModel.heldHelp.isEmpty)
    }

    /// A merge waits for a running job the core would refuse it beside.
    @Test func aMergeWaitsForARunningExport() async throws {
        let (m, f) = try document()
        let row = JobRow(id: 99, kind: .exportDawproject, progress: 0.4, state: .running, message: nil, problems: [], created: [])
        f.jobRows[99] = row
        m.jobs.append(row)
        let g = m.merge
        g.begin(path: Self.path, info: info())
        await g.settle()
        #expect(g.waiting && f.dawprojectMerges.isEmpty && g.shown)
        f.jobRows[99] = JobRow(id: 99, kind: .exportDawproject, progress: 1, state: .done, message: nil, problems: [], created: [])
        m.pollJobs()
        #expect(!g.waiting && f.dawprojectMerges.count == 1 && g.running)
    }
}
