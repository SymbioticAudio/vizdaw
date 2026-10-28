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
        }
    }

    /// The alert title of a failed job.
    public static func failureTitle(_ kind: JobKindRow) -> String {
        switch kind {
        case .import: "Import Failed"
        case .relink: "Relink Failed"
        case .convert, .format: "Conversion Failed"
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
    public func changeFormat(_ format: ProjectFormatRow) {
        startJob(.format, "Project Settings Failed") { try $0.setProjectFormat(format: format) }
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
    func startJob(_ kind: JobKindRow, _ title: String, _ body: (any SessionProtocol) throws -> UInt64) {
        do {
            let id = try body(session)
            jobs.append(JobRow(id: id, kind: kind, progress: 0, state: .running, message: nil, problems: [], created: []))
            pollJobs()
        } catch {
            alert = AlertItem(error, title: title)
        }
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
        for j in jobs {
            let r: JobRow
            do {
                r = try session.job(id: j.id)
            } catch {
                if case .Busy = error as? VizError { still.append(j) }
                continue
            }
            switch ControlJobs.answer(r) {
            case .wait:
                still.append(r)
            case .ok:
                if !r.problems.isEmpty { jobProblems = r.problems }
                refreshAfterJob()
            case .failed(let message):
                if r.state != .cancelled {
                    alert = AlertItem(title: ProjectBanner.failureTitle(r.kind), message: message)
                }
                refreshAfterJob()
            }
        }
        jobs = still
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
