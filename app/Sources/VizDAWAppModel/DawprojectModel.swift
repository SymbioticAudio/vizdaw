import Foundation
import Observation
import VizDAWCore

/// The Export as DAWproject sheet's model (plan 6d Task 12): the region
/// gain choice, the core's check of what will not transfer, the core's file
/// name and the running job. The check runs in detached tasks; only the
/// latest lands. An export waits for a running job the core cannot run it
/// beside (the interchange jobs and the media jobs) instead of failing as
/// `Busy`. Swift never builds the file name: `defaultName` is the core's.
@Observable
@MainActor
public final class DawprojectModel {
    /// Weak: the sheet can outlive its document.
    @ObservationIgnored private weak var document: DocumentModel?
    @ObservationIgnored private var generation = 0
    @ObservationIgnored private var tasks: [Int: Task<Void, Never>] = [:]
    @ObservationIgnored private var taskCount = 0
    /// Showing again after a failure keeps the message.
    @ObservationIgnored private var reopening = false
    /// The request kept while a job runs.
    @ObservationIgnored private var kept: DawprojectExportRequestRow?

    public init(document: DocumentModel) { self.document = document }

    /// Opening refreshes the check and the name; showing again while an
    /// export waits or runs keeps it, with its progress and Cancel.
    public var shown = false {
        didSet {
            guard shown, !oldValue, !reopening, !waiting, !running else { return }
            reset()
        }
    }

    /// Whether region gain is applied to the exported audio; a change
    /// refreshes the check.
    public var regionGain: RegionGainRow = .apply {
        didSet { if shown, regionGain != oldValue { refresh() } }
    }

    /// What the export would not carry over, from the core's check.
    public private(set) var wontTransfer: ReportRow?
    /// Why the last check, start or job was refused, shown in the sheet.
    public private(set) var checkError: String?
    /// The core's file name for the shown sequence (`dawprojectName`).
    public private(set) var defaultName = ""
    /// The export is kept until the running job ends.
    public private(set) var waiting = false
    /// The export job the sheet started (it may have ended).
    public private(set) var job: UInt64?

    /// The job is still listed as running.
    public var running: Bool {
        guard let job, let document else { return false }
        return document.jobs.contains { $0.id == job }
    }

    /// The running export's progress, 0 to 1; nil with none listed.
    public var progress: Float? {
        guard let job else { return nil }
        return document?.jobs.first { $0.id == job }?.progress
    }

    private func reset() {
        guard let document else { return }
        job = nil
        kept = nil
        waiting = false
        checkError = nil
        wontTransfer = nil
        // A name from the text of the model: no file is looked at.
        defaultName = document.selectedSeq.flatMap { try? document.session.dawprojectName(seq: $0) } ?? ""
        refresh()
    }

    private func request(path: String, replace: Bool) -> DawprojectExportRequestRow? {
        guard let document, let seq = document.selectedSeq else { return nil }
        return DawprojectExportRequestRow(
            seq: seq, path: path, replace: replace, regionGain: regionGain, blocks: document.editor.layout.blockRule.rule)
    }

    /// Asks the core what the export would leave out, off the main actor;
    /// only the latest answer lands.
    public func refresh() {
        guard let document else { return }
        generation += 1
        guard let req = request(path: "", replace: false) else {
            wontTransfer = nil
            checkError = "No sequence is shown."
            return
        }
        let mine = generation
        let session = document.session
        taskCount += 1
        let id = taskCount
        tasks[id] = Task {
            let result = await Task.detached { Result { try session.exportDawprojectCheck(req: req) } }.value
            self.landed(mine, result)
            self.tasks[id] = nil
        }
    }

    private func landed(_ mine: Int, _ result: Result<ReportRow, any Error>) {
        guard mine == generation else { return }
        switch result {
        case .success(let row):
            wontTransfer = row
            checkError = nil
        case .failure(let failure):
            wontTransfer = nil
            checkError = InterchangeModel.message(failure)
        }
    }

    /// Waits until every check started so far has landed (or been dropped).
    public func settle() async {
        while let t = tasks.values.first { await t.value }
    }

    // MARK: Start

    /// Whether something is at `path` (lstat: a dangling link counts as
    /// taken). One stat, right after the Save panel stat'd the same path;
    /// tests replace it.
    @ObservationIgnored public var exists: (String) -> Bool = { path in
        (try? FileManager.default.attributesOfItem(atPath: path)) != nil
    }

    /// The Save panel's answer. The panel confirmed a replace only of what
    /// was at `path` when it returned, so `replace` is sent only then: a file
    /// that appears while the export waits is refused by the core, which
    /// names it, never replaced unconfirmed.
    public func confirmed(path: String) {
        start(path: path, replace: exists(path))
    }

    /// Whether a job the core refuses an export beside (`Busy`) is listed.
    private var blocked: Bool {
        guard let document else { return false }
        return document.mergeHeld
            || document.jobs.contains { ForDawModel.blocking.contains($0.kind) || ForDawModel.mediaJobs.contains($0.kind) }
    }

    /// Exports to `path`, or keeps the request until the running job ends.
    /// `replace` is true only when the Save panel confirmed replacing a file
    /// that was there: `confirmed(path:)` works it out and calls this.
    public func start(path: String, replace: Bool) {
        guard let req = request(path: path, replace: replace) else {
            checkError = "No sequence is shown."
            return
        }
        // One export at a time from the sheet.
        guard !running else { return }
        checkError = nil
        if blocked {
            kept = req
            waiting = true
        } else {
            begin(req)
        }
    }

    private func begin(_ req: DawprojectExportRequestRow) {
        guard let document else { return }
        kept = nil
        waiting = false
        // The sheet stays up with progress and Cancel; a refusal on
        // `startJob`'s first poll shows its message in it.
        job = document.startJob(
            .exportDawproject, "DAWproject Export Failed",
            failure: { [self] in fail(InterchangeModel.message($0)) }
        ) { try $0.exportDawproject(req: req) }
    }

    /// The sheet's Cancel while waiting: the kept export is dropped and
    /// nothing is sent.
    public func cancelWaiting() {
        kept = nil
        waiting = false
    }

    /// The sheet again, with the choices as they were and `message`. A
    /// check still in flight is dropped, so it cannot erase the message.
    private func fail(_ message: String) {
        generation += 1
        checkError = message
        reopening = true
        shown = true
        reopening = false
    }

    /// A finished or cancelled export closes the sheet (a finished one's
    /// report opens through `InterchangeModel.jobEnded`).
    func jobDone(_ job: JobRow) {
        if job.kind == .exportDawproject, job.id == self.job { shown = false }
    }

    /// A job ended (`pollJobs`, once the job list is settled): the kept
    /// request starts if nothing blocks it any more.
    func jobEnded() {
        guard let req = kept, !blocked else { return }
        begin(req)
    }

    /// An export failed: the sheet opens again with the message, and no
    /// report. Answers whether it did (a cancelled one is quiet).
    func jobFailed(_ job: JobRow, message: String) -> Bool {
        guard job.kind == .exportDawproject, job.id == self.job, job.state != .cancelled else { return false }
        fail(message)
        return true
    }
}
