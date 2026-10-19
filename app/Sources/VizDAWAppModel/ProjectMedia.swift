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
        start(.import, "Import Failed") { try $0.importAudio(paths: paths, target: target) }
    }

    /// Project Settings: converts every file into `format` (a job).
    public func changeFormat(_ format: ProjectFormatRow) {
        start(.format, "Project Settings Failed") { try $0.setProjectFormat(format: format) }
    }

    /// The banner's Convert.
    public func convertMedia() {
        start(.convert, "Convert Failed") { try $0.convertMedia() }
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

    private func start(_ kind: JobKindRow, _ title: String, _ body: (any SessionProtocol) throws -> UInt64) {
        do {
            let id = try body(session)
            jobs.append(JobRow(id: id, kind: kind, progress: 0, state: .running, message: nil, problems: [], created: []))
            pollJobs()
        } catch {
            alert = AlertItem(error, title: title)
        }
    }

    /// Reads every running job (the plugin timer calls this through
    /// `idle`); an ended one is applied by the session and refreshes the
    /// document.
    public func pollJobs() {
        var still: [JobRow] = []
        for j in jobs {
            guard let r = try? session.job(id: j.id) else {
                still.append(j)
                continue
            }
            switch r.state {
            case .running:
                still.append(r)
            case .done:
                if r.kind != .format, r.created.isEmpty, !r.problems.isEmpty {
                    // Nothing was created: an error listing the problems.
                    alert = AlertItem(
                        title: r.kind == .import ? "Import Failed" : "Conversion Failed",
                        message: r.problems.joined(separator: "\n"))
                } else if !r.problems.isEmpty {
                    jobProblems = r.problems
                }
                refreshAfterJob()
            case .failed:
                alert = AlertItem(
                    title: r.kind == .import ? "Import Failed" : "Conversion Failed",
                    message: r.message ?? "The job failed.")
                refreshAfterJob()
            case .cancelled:
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
