import Foundation
import Observation
import VizDAWCore

/// The Merge DAWproject sheet's model (plan 6e Task 10; Rulings 12–14): the
/// Hold job that previews a merge into this project and keeps it until
/// Merge or Cancel, the project's snapshots when the file lost its export
/// ID, and the core's sentence when anything is refused.
///
/// It follows `DawprojectModel`: the snapshot list is read in a detached
/// task under a generation counter (only the latest lands), and the job is
/// watched through the document's job polling (`jobDone`, `jobFailed`).
/// A merge waits for a running job the core cannot run it beside instead of
/// failing as `Busy`, and a second merge asked for while one is running or
/// held waits for it. A held merge counts as running (Ruling 12):
/// `DocumentModel.mergeHeld` holds back what the core would refuse beside
/// it, which goes once the hold ends. Swift builds no sentence the core
/// owns: refusals, the summary and the conflict lines are the core's.
@Observable
@MainActor
public final class MergeModel {
    /// Weak: the sheet can outlive its document.
    @ObservationIgnored private weak var document: DocumentModel?
    @ObservationIgnored private var generation = 0
    @ObservationIgnored private var tasks: [Int: Task<Void, Never>] = [:]
    @ObservationIgnored private var taskCount = 0
    /// The file's header, as the store read it.
    @ObservationIgnored private var info: DawprojectInfoRow?
    /// The snapshot kept while the merge waits for a running job.
    @ObservationIgnored private var kept: String??
    /// A merge asked for while this one runs or is held.
    @ObservationIgnored private var next: (path: String, info: DawprojectInfoRow)?
    /// Jobs Cancel let go of: one that finishes anyway has its hold dropped.
    @ObservationIgnored private var abandoned: Set<UInt64> = []
    /// Set while the model itself changes `shown` or `chosenSnapshot`.
    @ObservationIgnored private var quiet = false

    public init(document: DocumentModel) { self.document = document }

    /// The sheet. Closing it (Escape, the window's close) is Cancel.
    public var shown = false {
        didSet {
            guard !quiet, oldValue, !shown else { return }
            cancel()
        }
    }
    /// The DAWproject file being merged.
    public private(set) var path = ""
    /// The project's snapshots, newest first, when the file lost its export
    /// ID (empty otherwise).
    public private(set) var snapshots: [SnapshotRow] = []
    /// The export the user picked from `snapshots`; setting it starts the
    /// preview.
    public var chosenSnapshot: String? {
        didSet {
            guard !quiet, let id = chosenSnapshot, id != oldValue, shown, job == nil, !waiting else { return }
            start(snapshot: id)
        }
    }
    /// The held merge's preview: the counts line and the conflict lines.
    public private(set) var preview: MergePreviewRow?
    /// The core's sentence for the last refusal (job, confirm or listing).
    public private(set) var error: String?
    /// The Hold job the sheet started (running, then held).
    public private(set) var job: UInt64?
    /// The Hold job finished and the core keeps its merge until Merge or
    /// Cancel: it counts as running.
    public private(set) var held = false
    /// The merge waits for a running job the core cannot run it beside.
    public private(set) var waiting = false

    /// The Hold job is still listed as running.
    public var running: Bool {
        guard let job, let document else { return false }
        return document.jobs.contains { $0.id == job }
    }

    /// The running job's progress, 0 to 1; nil with none listed.
    public var progress: Float? {
        guard let job else { return nil }
        return document?.jobs.first { $0.id == job }?.progress
    }

    /// Merge is available: a preview is held.
    public var canMerge: Bool { held && preview != nil }

    // MARK: Begin

    /// Shows the sheet for `path`. Without an export ID, the project's
    /// snapshots are listed off the main actor and the preview starts once
    /// one is chosen (at once, with none to choose: the core's refusal says
    /// why); otherwise the Hold job starts. While another merge runs or is
    /// held, or while the sheet shows another file (its snapshot list, its
    /// refusal), this one waits for it.
    public func begin(path: String, info: DawprojectInfoRow) {
        guard document != nil else { return }
        if job != nil || waiting || shown {
            next = (path, info)
            return
        }
        generation += 1
        self.path = path
        self.info = info
        quiet = true
        chosenSnapshot = nil
        shown = true
        quiet = false
        snapshots = []
        preview = nil
        error = nil
        held = false
        kept = nil
        if info.exportId == nil {
            listSnapshots()
        } else {
            start(snapshot: nil)
        }
    }

    /// `dawprojectSnapshots` reads the project's files: off the main actor.
    private func listSnapshots() {
        guard let document else { return }
        let mine = generation
        let session = document.session
        taskCount += 1
        let id = taskCount
        tasks[id] = Task {
            let result = await Task.detached { Result { try session.dawprojectSnapshots() } }.value
            self.listed(mine, result)
            self.tasks[id] = nil
        }
    }

    private func listed(_ mine: Int, _ result: Result<[SnapshotRow], any Error>) {
        guard mine == generation, shown else { return }
        switch result {
        case .success(let rows) where rows.isEmpty:
            // Nothing to choose from: the core's refusal names why.
            start(snapshot: nil)
        case .success(let rows):
            snapshots = rows
        case .failure(let failure):
            error = InterchangeModel.message(failure)
        }
    }

    /// Waits until every listing started so far has landed (or been dropped).
    public func settle() async {
        while let t = tasks.values.first { await t.value }
    }

    /// Whether a job the core refuses a merge beside (`Busy`) is listed.
    private var blocked: Bool {
        guard let document else { return false }
        return document.jobs.contains { ForDawModel.blocking.contains($0.kind) || ForDawModel.mediaJobs.contains($0.kind) }
    }

    private func start(snapshot: String?) {
        if blocked {
            kept = .some(snapshot)
            waiting = true
        } else {
            send(snapshot: snapshot)
        }
    }

    private func send(snapshot: String?) {
        guard let document else { return }
        kept = nil
        waiting = false
        error = nil
        let req = DawprojectMergeRequestRow(path: path, snapshot: snapshot, step: .hold, date: MergeDate.today())
        // A refusal (recording, an open edit) shows in the sheet.
        job = document.startJob(
            .mergeDawproject, ProjectBanner.failureTitle(.mergeDawproject),
            failure: { [self] in error = InterchangeModel.message($0) }
        ) { try $0.mergeDawproject(req: req) }
    }

    // MARK: Merge and Cancel

    /// Merge: the held merge is swapped in, its report opens and the sheet
    /// closes. A refusal stays in the sheet with the core's sentence; one
    /// other than `Busy` (the project changed after the preview) means the
    /// core let the hold go, so Merge is no longer offered.
    public func confirm() {
        guard let document, let job, held else { return }
        do {
            let report = try document.session.confirmMerge(job: job)
            self.job = nil
            held = false
            document.refreshAfterJob()
            document.interchange.show(report)
            close()
        } catch {
            self.error = InterchangeModel.message(error)
            if case .Busy = error as? VizError { return }
            self.job = nil
            held = false
            document.holdEnded()
        }
    }

    /// Cancel: a running job is cancelled, a held merge let go (with its
    /// media); the sheet closes. Nothing in the project changes.
    public func cancel() {
        guard let document else { return }
        if let job {
            if held {
                // Its only refusal is "no merge is waiting": no hold is left.
                try? document.session.cancelMerge(job: job)
            } else if running {
                abandoned.insert(job)
                document.cancelJob(job)
            }
        }
        job = nil
        held = false
        close()
    }

    /// The sheet closes; a listing in flight is dropped, what waited for
    /// the hold goes, and a merge asked for meanwhile begins.
    private func close() {
        generation += 1
        kept = nil
        waiting = false
        quiet = true
        shown = false
        quiet = false
        document?.holdEnded()
        if let n = next {
            next = nil
            begin(path: n.path, info: n.info)
        }
    }

    /// The document is closing (`DocumentStore.close`, before its session
    /// closes, which drops the hold): everything ends quietly. Nothing
    /// queued starts, nothing is sent, and the model lets the document go,
    /// so a sheet binding written back as the window goes does nothing.
    func documentClosed() {
        generation += 1
        next = nil
        kept = nil
        waiting = false
        job = nil
        held = false
        abandoned = []
        quiet = true
        shown = false
        quiet = false
        document = nil
    }

    // MARK: The job

    /// A merge job ended Done or Cancelled (`pollJobs`): the Hold job's
    /// preview is shown and the merge is held. A Done Hold job is a
    /// preview, not a report: nothing else opens.
    func jobDone(_ r: JobRow) {
        guard r.kind == .mergeDawproject else { return }
        if abandoned.remove(r.id) != nil {
            // Cancelled too late: the core holds it; let it go ("no merge
            // is waiting" means it is already gone).
            if r.state == .done, let document { try? document.session.cancelMerge(job: r.id) }
            return
        }
        guard r.id == job else { return }
        if r.state == .cancelled {
            job = nil
            return
        }
        preview = r.merge
        held = true
    }

    /// The Hold job failed: the core's sentence shows in the sheet, not
    /// an alert. Answers whether it did (a cancelled one is quiet).
    func jobFailed(_ r: JobRow, message: String) -> Bool {
        guard r.kind == .mergeDawproject else { return false }
        if abandoned.remove(r.id) != nil { return true }
        guard r.id == job, r.state != .cancelled else { return false }
        job = nil
        held = false
        error = message
        return true
    }

    /// A job ended (`pollJobs`, once the job list is settled): a waiting
    /// merge starts if nothing blocks it any more.
    func jobEnded() {
        guard waiting, let snapshot = kept, !blocked else { return }
        send(snapshot: snapshot)
    }
}
