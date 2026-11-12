import Foundation
import VizDAWCore

/// A bar above a document's editors (plan 5a Task 13).
public enum ProjectBanner: Equatable, Sendable {
    /// An import or conversion running.
    case job(id: UInt64, kind: JobKindRow, progress: Float)
    /// Media not in the project format: Convert.
    case mismatched(count: UInt32, label: String)
    /// The device would not switch to the project rate: regions are muted.
    case rateMismatch(device: Double, project: UInt32)
    /// Audio was not read from disk in time.
    case diskTooSlow
    /// Files a finished job could not handle.
    case problems([String])
}

extension ProjectBanner {
    /// A running job's bar text.
    public static func jobText(_ kind: JobKindRow) -> String {
        switch kind {
        case .import: "Importing audio…"
        case .relink: "Relinking media…"
        case .convert, .format: "Converting media…"
        case .bounce: "Bouncing…"
        case .externalEdit: "Importing the external edit…"
        case .importMidi: "Importing the MIDI file…"
        case .exportMidi: "Exporting MIDI…"
        case .exportProject: "Exporting for another DAW…"
        case .exportDawproject: "Exporting DAWproject…"
        case .importProject: "Importing DAWproject…"
        }
    }

    /// The alert title of a failed job.
    public static func failureTitle(_ kind: JobKindRow) -> String {
        switch kind {
        case .import: "Import Failed"
        case .relink: "Relink Failed"
        case .convert, .format: "Conversion Failed"
        case .bounce: "Bounce Failed"
        case .externalEdit: "External Edit Failed"
        case .importMidi: "MIDI Import Failed"
        case .exportMidi: "MIDI Export Failed"
        case .exportProject: "Export Failed"
        case .exportDawproject: "DAWproject Export Failed"
        case .importProject: "DAWproject Import Failed"
        }
    }
}

extension DocumentModel {
    /// Where Import Audio puts files (Ruling 19): the selected lane of the
    /// shown sequence for one file, else new lanes; the transport's tick
    /// (0 if before it); no target without a sequence.
    public static func importTarget(
        seq: UInt64?, selection: [LaneRef], files: Int, tick: Int64
    ) -> ImportTargetRow? {
        guard let seq else { return nil }
        let lane = files == 1 ? selection.first { $0.seq == seq }?.lane : nil
        return ImportTargetRow(seq: seq, lane: lane, tick: max(0, tick))
    }

    /// File › Import Audio…: imports `paths` as a job.
    public func importAudio(paths: [String]) {
        guard !paths.isEmpty else { return }
        let target = Self.importTarget(
            seq: selectedSeq, selection: editor.selection.lanes, files: paths.count, tick: transport.tick)
        importAudio(paths: paths, target: target)
    }

    /// Imports `paths` at `target` (a drop; nil: into the pool only).
    public func importAudio(paths: [String], target: ImportTargetRow?) {
        guard !paths.isEmpty else { return }
        startJob(.import, "Import Failed") { try $0.importAudio(paths: paths, target: target) }
    }

    /// Project Settings: converts every file into `format` (a job).
    /// Answers whether the job started (a refusal is an alert).
    @discardableResult
    public func changeFormat(_ format: ProjectFormatRow) -> Bool {
        startJob(.format, "Project Settings Failed") { try $0.setProjectFormat(format: format) } != nil
    }

    /// The banner's Convert.
    public func convertMedia() {
        startJob(.convert, "Convert Failed") { try $0.convertMedia() }
    }

    /// The device-rate banner's button: the project takes the device's rate.
    public func changeRateToDevice() {
        guard let i = projectInfo, i.rateMismatch, i.deviceRate > 0 else { return }
        changeFormat(ProjectFormatRow(sampleRate: UInt32(i.deviceRate.rounded()), depth: i.format.depth))
    }

    /// Asks job `id` to stop.
    public func cancelJob(_ id: UInt64) {
        try? session.cancelJob(id: id)
    }

    /// Starts a job and lists it; a refusal is an alert titled `title`.
    /// With `failure` the refusal goes there instead (a sheet that shows
    /// it itself). Answers the new job's ID.
    @discardableResult
    func startJob(
        _ kind: JobKindRow, _ title: String, failure: ((Error) -> Void)? = nil,
        _ body: (any SessionProtocol) throws -> UInt64
    ) -> UInt64? {
        do {
            let id = try body(session)
            jobs.append(JobRow(id: id, kind: kind, progress: 0, state: .running, message: nil, problems: [], created: []))
            pollJobs()
            return id
        } catch {
            if let failure { failure(error) } else { alert = AlertItem(error, title: title) }
            return nil
        }
    }

    /// `ControlJobs.answer` for `r`, with the bounce's files when `r` is a
    /// bounce (plan 5f-2 Ruling 10); the app's polling and the control
    /// channel both answer through here.
    public func jobAnswer(_ r: JobRow) -> JobAnswer {
        let bounced = r.kind == .bounce ? (try? session.bounceResult(job: r.id)) ?? nil : nil
        return ControlJobs.answer(r, bounce: bounced)
    }

    /// Reads every running job (the plugin timer calls this through
    /// `idle` while any is listed); an ended one is applied by the session
    /// and refreshes the document. The outcome is `ControlJobs.answer`'s,
    /// shared with the control channel: a failure is an alert, warnings a
    /// Problems banner; a job the user cancelled ends quietly. A job the
    /// session no longer knows is dropped (only `Busy` is retried), so the
    /// polling stops once none is left.
    public func pollJobs() {
        var still: [JobRow] = []
        var openFailure: String??
        var importsEnded: [JobRow] = []
        var blockersEnded: [JobRow] = []
        for j in jobs {
            let r: JobRow
            do {
                r = try session.job(id: j.id)
            } catch {
                if case .Busy = error as? VizError {
                    still.append(j)
                } else {
                    blockersEnded.append(j)
                }
                continue
            }
            switch jobAnswer(r) {
            case .wait:
                still.append(r)
                continue
            case .ok:
                if !r.problems.isEmpty { jobProblems = r.problems }
                refreshAfterJob()
                // A MIDI import or export the UI started shows its report.
                if [.importMidi, .exportMidi, .exportProject, .exportDawproject, .importProject].contains(r.kind) { interchange.jobEnded(r) }
                if r.kind == .importMidi { openedMidi = nil }
                forDaw.jobDone(r)
            case .failed(let message):
                if r.state == .cancelled { forDaw.jobDone(r) }
                if r.kind == .importMidi, openedMidi != nil {
                    // Open made this project for the file: the store closes it.
                    openFailure = .some(r.state == .cancelled ? nil : message)
                } else if r.state != .cancelled, !(r.kind == .exportMidi && interchange.jobFailed(r, message: message)),
                    !(r.kind == .exportProject && forDaw.jobFailed(r, message: message))
                {
                    alert = AlertItem(title: ProjectBanner.failureTitle(r.kind), message: message)
                }
                refreshAfterJob()
            }
            if r.kind == .importMidi { importsEnded.append(r) }
            blockersEnded.append(r)
        }
        jobs = still
        if let failure = openFailure, let hook = openedMidi {
            openedMidi = nil
            hook(failure)
        }
        // Dropped files waiting for this one start now (final review I1),
        // after `jobs` is settled: `startJob` adds to it.
        for r in importsEnded { interchange.importEnded(r) }
        // Anything waiting for a job to end (dropped MIDI files, an export)
        // goes now that the list is settled. A cancelled export keeps the
        // dropped files queued (F11): only `importEnded` clears them.
        for r in blockersEnded where ForDawModel.blocking.contains(r.kind) || ForDawModel.mediaJobs.contains(r.kind) {
            if ForDawModel.blocking.contains(r.kind) { interchange.blockingEnded() }
            forDaw.jobEnded()
        }
    }

    public func refreshProjectInfo() {
        projectInfo = try? session.projectInfo()
    }

    /// The app became active: files may have changed in Finder.
    public func recheckMedia() {
        if (try? session.recheckMedia()) == true {
            mediaVersion += 1
            refreshProjectInfo()
            media.offerRelinkIfNeeded()
        }
    }

    public func dismissDiskWarning() { diskWarningDismissed = true }

    public func dismissProblems() { jobProblems = [] }

    /// What the bars above the editors show, top first.
    public var banners: [ProjectBanner] {
        var out: [ProjectBanner] = jobs.map { .job(id: $0.id, kind: $0.kind, progress: $0.progress) }
        if let i = projectInfo {
            if i.rateMismatch {
                out.append(.rateMismatch(device: i.deviceRate, project: i.format.sampleRate))
            }
            if i.mismatched > 0, !jobs.contains(where: { $0.kind == .convert || $0.kind == .format }) {
                out.append(.mismatched(count: i.mismatched, label: formatLabel(format: i.format)))
            }
        }
        if transport.faults.contains("stream-underrun"), !diskWarningDismissed {
            out.append(.diskTooSlow)
        }
        if !jobProblems.isEmpty { out.append(.problems(jobProblems)) }
        return out
    }
}
