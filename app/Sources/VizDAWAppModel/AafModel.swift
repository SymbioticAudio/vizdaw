import Foundation
import Observation
import VizDAWCore

/// The AAF sheets' model (plan 6f Task 8; spec §9), as `DawprojectModel`.
///
/// Export: the options, the core's check of what will not transfer (in
/// detached tasks; only the latest lands), the core's folder name and the
/// running job. An export waits for a running job the core cannot run it
/// beside instead of failing as `Busy`. Swift never builds the folder name:
/// `defaultName` is the core's, and the Save panel gives only the parent
/// folder. An existing "<Song> AAF" folder is never replaced; the core
/// refuses it and the sheet shows why.
///
/// Import: the file's header, read off the main actor, then the choices
/// and where it goes (a new project at the file's rate, or this one).
@Observable
@MainActor
public final class AafModel {
    /// Weak: the sheets can outlive their document.
    @ObservationIgnored private weak var document: DocumentModel?
    @ObservationIgnored private var generation = 0
    @ObservationIgnored private var tasks: [Int: Task<Void, Never>] = [:]
    @ObservationIgnored private var taskCount = 0
    /// Showing again after a failure keeps the message.
    @ObservationIgnored private var reopening = false
    /// The request kept while a job runs.
    @ObservationIgnored private var kept: AafExportRequestRow?
    /// Only the latest file chosen lands.
    @ObservationIgnored private var chooseGeneration = 0

    public init(document: DocumentModel) { self.document = document }

    // MARK: Export

    /// Opening refreshes the check and the name; showing again while an
    /// export waits or runs keeps it, with its progress and Cancel.
    public var exportShown = false {
        didSet {
            guard exportShown, !oldValue, !reopening, !waiting, !running else { return }
            reset()
        }
    }

    /// "Embed audio in the AAF file".
    public var embed = true { didSet { if embed != oldValue { changed() } } }
    /// "Bit depth".
    public var depth: AafDepthRow = .int24 { didSet { if depth != oldValue { changed() } } }
    /// "Stereo tracks": as left and right mono tracks, or as stereo tracks.
    public var stereo: AafStereoRow = .monoPairs { didSet { if stereo != oldValue { changed() } } }
    /// "Start": 01:00:00:00 instead of 00:00:00:00.
    public var hourStart = false { didSet { if hourStart != oldValue { changed() } } }
    /// "Frame rate".
    public var fps: AafFpsRow = .fps25 { didSet { if fps != oldValue { changed() } } }
    /// "Clip gain".
    public var clipGain = true { didSet { if clipGain != oldValue { changed() } } }
    /// "Track volume and pan".
    public var automation = true { didSet { if automation != oldValue { changed() } } }

    /// What the export would not carry over, from the core's check.
    public private(set) var wontTransfer: ReportRow?
    /// Why the last check, start or job was refused, shown in the sheet.
    public private(set) var checkError: String?
    /// The core's folder name for the shown sequence (`aafFolderName`).
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

    private func changed() {
        if exportShown { refresh() }
    }

    private func reset() {
        guard let document else { return }
        job = nil
        kept = nil
        waiting = false
        checkError = nil
        wontTransfer = nil
        // A name from the text of the model: no file is looked at.
        defaultName = document.selectedSeq.flatMap { try? document.session.aafFolderName(seq: $0) } ?? ""
        refresh()
    }

    private func request(path: String) -> AafExportRequestRow? {
        guard let document, let seq = document.selectedSeq else { return nil }
        return AafExportRequestRow(
            seq: seq, path: path, embed: embed, depth: depth, stereo: stereo, hourStart: hourStart, fps: fps,
            clipGain: clipGain, automation: automation)
    }

    /// Asks the core what the export would leave out, off the main actor;
    /// only the latest answer lands.
    public func refresh() {
        guard let document else { return }
        generation += 1
        guard let req = request(path: "") else {
            wontTransfer = nil
            checkError = "No sequence is shown."
            return
        }
        let mine = generation
        let session = document.session
        taskCount += 1
        let id = taskCount
        tasks[id] = Task {
            let result = await Task.detached { Result { try session.exportAafCheck(req: req) } }.value
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

    /// Waits until every check and read started so far has landed (or been
    /// dropped).
    public func settle() async {
        while let t = tasks.values.first { await t.value }
    }

    /// Whether a job the core refuses an export beside (`Busy`) is listed.
    private var blocked: Bool {
        guard let document else { return false }
        return document.mergeHeld
            || document.jobs.contains { ForDawModel.blocking.contains($0.kind) || ForDawModel.mediaJobs.contains($0.kind) }
    }

    /// The Save panel's parent folder: the core writes "<Song> AAF" in it
    /// (never over one that is there), now or once the running job ends.
    public func confirmed(folder: String) {
        guard let req = request(path: folder) else {
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

    private func begin(_ req: AafExportRequestRow) {
        guard let document else { return }
        kept = nil
        waiting = false
        // The sheet stays up with progress and Cancel; a refusal on
        // `startJob`'s first poll shows its message in it.
        job = document.startJob(
            .exportAaf, "AAF Export Failed",
            failure: { [self] in fail(InterchangeModel.message($0)) }
        ) { try $0.exportAaf(req: req) }
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
        exportShown = true
        reopening = false
    }

    /// A finished or cancelled export closes the sheet (a finished one's
    /// report opens through `InterchangeModel.jobEnded`).
    func jobDone(_ job: JobRow) {
        if job.kind == .exportAaf, job.id == self.job { exportShown = false }
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
        guard job.kind == .exportAaf, job.id == self.job, job.state != .cancelled else { return false }
        fail(message)
        return true
    }

    // MARK: Import

    /// The import sheet; it shows once the chosen file has been read.
    public var importShown = false
    /// The chosen file (absolute).
    public private(set) var importPath = ""
    /// Its header, as the core read it.
    public private(set) var info: AafInfoRow?
    /// "Open as a new project" (true) or "Add to this project".
    public var newProject = true
    /// "Join left and right tracks into stereo".
    public var joinPairs = true
    /// "MIDI file beside it": filled from the file's `midiBeside`.
    public var midiPath: String?
    /// "Look for missing audio in…": an absolute folder, or nil.
    public var mediaFolder: String?

    /// Reads an AAF file's header (the core's free function; tests set a
    /// fake). It parses the file: it is called only from a detached task.
    @ObservationIgnored public var readInfo: @Sendable (String) throws -> AafInfoRow = {
        try VizDAWCore.aafInfo(path: $0)
    }

    /// Reads `path` off the main actor, then shows the import sheet with
    /// the defaults and the MIDI file found beside it; a refusal is the
    /// alert "Import Failed". Only the latest file chosen lands.
    public func choose(path: String) {
        chooseGeneration += 1
        let mine = chooseGeneration
        let read = readInfo
        taskCount += 1
        let id = taskCount
        tasks[id] = Task {
            let result = await Task.detached { Result { try read(path) } }.value
            self.read(mine, path, result)
            self.tasks[id] = nil
        }
    }

    private func read(_ mine: Int, _ path: String, _ result: Result<AafInfoRow, any Error>) {
        guard mine == chooseGeneration, let document else { return }
        switch result {
        case .failure(let error):
            document.alert = AlertItem(error, title: "Import Failed")
        case .success(let row):
            importPath = path
            info = row
            newProject = true
            joinPairs = true
            midiPath = row.midiBeside
            mediaFolder = nil
            importShown = true
        }
    }

    /// The import sheet's Import. "Open as a new project": the store makes
    /// a project at the file's rate and imports there (its window opens);
    /// "Add to this project": this document's session imports it.
    public func startImport(store: DocumentStore) {
        guard let document, let info, !importPath.isEmpty else { return }
        let midi = midiPath.flatMap { $0.isEmpty ? nil : $0 }
        let folder = mediaFolder.flatMap { $0.isEmpty ? nil : $0 }
        let req = AafImportRequestRow(
            path: importPath, newProject: newProject, midiPath: midi, mediaFolder: folder, joinPairs: joinPairs)
        importShown = false
        if newProject {
            store.importAafAsNewProject(req, rate: info.sampleRate)
        } else {
            document.startJob(.importAaf, "AAF Import Failed") { try $0.importAaf(req: req) }
        }
    }
}
