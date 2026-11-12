import Foundation
import Observation
import VizDAWCore

/// The Export for Another DAW sheet's model (plan 6b Task 4; F10): the
/// choices, the core's check of them and the running job. The checks run
/// in detached tasks; only the latest lands. An export waits for a running
/// job instead of failing as `Busy`.
@Observable
@MainActor
public final class ForDawModel: AudioFormatChoices {
    /// Jobs the core cannot run an export beside; also what holds back
    /// dropped MIDI files while one is listed.
    public static let blocking: Set<JobKindRow> = [.bounce, .importMidi, .exportMidi, .exportProject, .exportDawproject]
    /// Jobs that change the media "Add to project" would add to.
    static let mediaJobs: Set<JobKindRow> = [.format, .convert, .relink, .externalEdit]

    /// Weak: the sheet can outlive its document.
    @ObservationIgnored private weak var document: DocumentModel?
    @ObservationIgnored private var resetting = false
    /// Showing again after a failure keeps the choices.
    @ObservationIgnored private var reopening = false
    /// The first check after showing sends no strips (the core's defaults)
    /// and sets `stems` from its answer.
    @ObservationIgnored private var awaitingDefaults = false
    @ObservationIgnored private var generation = 0
    @ObservationIgnored private var tasks: [Int: Task<Void, Never>] = [:]
    @ObservationIgnored private var taskCount = 0
    /// The request kept while a job runs.
    @ObservationIgnored private var kept: ForDawRequestRow?

    /// Showing resets the choices to the project's defaults.
    public var shown = false {
        didSet {
            // Showing again while an export waits or runs keeps it, with its
            // progress and Cancel.
            guard shown, !oldValue, !reopening, !waiting, !running else { return }
            reset()
        }
    }
    public var stems: Set<UInt64> = [] { didSet { changed() } }
    public var container: ContainerRow = .wav { didSet { changed() } }
    public var depth: BitDepthRow = .int24 { didSet { changed() } }
    public var rate: UInt32 = 48_000 { didSet { changed() } }
    /// Kept as set; on by default at 16 bits; ignored for float.
    public var dither = false { didSet { changed() } }
    public var addToProject = false { didSet { changed() } }
    /// "Other…" is the chosen rate (a field from 8,000 to 384,000).
    public var otherRate = false

    /// The core's check of the choices.
    public private(set) var check: ForDawCheckRow?
    /// The rate `check` was made at: its frames are at that rate.
    private var checkRate: UInt32 = 48_000
    /// Why the last check or start was refused, shown in the sheet.
    public private(set) var error: String?
    /// The export is kept until the running job ends.
    public private(set) var waiting = false
    public private(set) var job: UInt64?
    /// The strips the stems checklist offers (every strip but master).
    public private(set) var strips: [StripRow] = []

    public init(document: DocumentModel) { self.document = document }

    /// The job is still listed as running.
    public var running: Bool {
        guard let job, let document else { return false }
        return document.jobs.contains { $0.id == job }
    }

    public var progress: Float? {
        guard let job else { return nil }
        return document?.jobs.first { $0.id == job }?.progress
    }

    /// The stems' length before the tail, at the export rate (F7), or ""
    /// with no check.
    public var length: String {
        guard let check else { return "" }
        return MediaModel.duration(frames: check.frames, rate: checkRate) + " + tail"
    }

    /// The picker's value: a listed rate, or 0 for "Other…".
    public var rateChoice: UInt32 {
        get { otherRate ? 0 : rate }
        set {
            if newValue == 0 {
                otherRate = true
            } else {
                otherRate = false
                rate = newValue
            }
        }
    }

    /// The "Other…" field, clamped to 8,000–384,000.
    public func setOtherRate(_ hz: Int) {
        rate = UInt32(min(max(hz, Int(BounceModel.otherRates.lowerBound)), Int(BounceModel.otherRates.upperBound)))
    }

    private func reset() {
        guard let document else { return }
        resetting = true
        let f = (try? document.session.projectInfo().format) ?? ProjectFormatRow(sampleRate: 48_000, depth: .int24)
        container = .wav
        depth = f.depth
        rate = f.sampleRate
        otherRate = !BounceModel.rates.contains(f.sampleRate)
        dither = f.depth == .int16
        addToProject = false
        job = nil
        kept = nil
        waiting = false
        check = nil
        strips = ((try? document.session.mixer().strips) ?? []).filter { $0.kind != .master }
        stems = []
        awaitingDefaults = true
        resetting = false
        refresh()
    }

    private func changed() {
        guard !resetting, shown else { return }
        refresh()
    }

    private func request(path: String) -> ForDawRequestRow? {
        guard let document, let seq = document.selectedSeq else { return nil }
        return ForDawRequestRow(
            seq: seq, strips: awaitingDefaults ? nil : stems.sorted(), container: container, depth: depth,
            rate: rate, dither: depth == .float32 ? false : dither, path: path, addToProject: addToProject)
    }

    /// Asks the core what the export would do, off the main actor; only
    /// the latest answer lands.
    public func refresh() {
        guard let document else { return }
        guard let req = request(path: "") else {
            generation += 1
            check = nil
            error = "No sequence is shown."
            return
        }
        generation += 1
        let mine = generation
        let session = document.session
        taskCount += 1
        let id = taskCount
        tasks[id] = Task {
            let result = await Task.detached { Result { try session.exportForDawCheck(req: req) } }.value
            self.landed(mine, req.rate, result)
            self.tasks[id] = nil
        }
    }

    private func landed(_ mine: Int, _ rate: UInt32, _ result: Result<ForDawCheckRow, any Error>) {
        guard mine == generation else { return }
        switch result {
        case .success(let row):
            check = row
            checkRate = rate
            error = nil
            if awaitingDefaults {
                awaitingDefaults = false
                resetting = true
                stems = Set(row.strips)
                resetting = false
            }
        case .failure(let failure):
            check = nil
            error = InterchangeModel.message(failure)
        }
    }

    /// Waits until every check started so far has landed (or been dropped).
    public func settle() async {
        while let t = tasks.values.first { await t.value }
    }

    // MARK: Start

    /// Whether a job the export cannot run beside is listed.
    private func blocked(by request: ForDawRequestRow) -> Bool {
        guard let document else { return false }
        return document.jobs.contains {
            Self.blocking.contains($0.kind) || (request.addToProject && Self.mediaJobs.contains($0.kind))
        }
    }

    /// Exports into `folder`, or keeps the request until the running job
    /// ends.
    public func start(folder: String) {
        guard let req = request(path: folder) else {
            error = "No sequence is shown."
            return
        }
        // One export at a time from the sheet: a running one is cancelled
        // or left to finish first.
        guard !running else { return }
        error = nil
        if blocked(by: req) {
            kept = req
            waiting = true
        } else {
            begin(req)
        }
    }

    private func begin(_ req: ForDawRequestRow) {
        guard let document else { return }
        kept = nil
        waiting = false
        // The sheet stays up with progress and Cancel, as Bounce's does; a
        // refusal on `startJob`'s first poll shows its message in it.
        job = document.startJob(
            .exportProject, "Export Failed",
            failure: { [self] in fail(InterchangeModel.message($0)) }
        ) { try $0.exportForDaw(req: req) }
    }

    public func cancelWaiting() {
        kept = nil
        waiting = false
    }

    /// The sheet again, with the choices as they were and `message`.
    private func fail(_ message: String) {
        error = message
        reopening = true
        shown = true
        reopening = false
    }

    /// A finished or cancelled export closes the sheet.
    func jobDone(_ job: JobRow) {
        if job.kind == .exportProject, job.id == self.job { shown = false }
    }

    /// A job ended (`pollJobs`, once the job list is settled): the kept
    /// request starts if nothing blocks it any more.
    func jobEnded() {
        guard let req = kept, !blocked(by: req) else { return }
        begin(req)
    }

    /// An export failed: the sheet opens again with the message. Answers
    /// whether it did (a cancelled one is quiet).
    func jobFailed(_ job: JobRow, message: String) -> Bool {
        guard job.kind == .exportProject, job.id == self.job else { return false }
        guard job.state != .cancelled else { return false }
        fail(message)
        return true
    }
}
