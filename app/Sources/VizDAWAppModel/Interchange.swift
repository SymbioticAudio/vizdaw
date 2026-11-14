import Foundation
import Observation
import VizDAWCore

/// One row of the import sheet: the destination a channel's notes go to
/// (`dest` nil: a new built-in synth).
public struct ChannelChoice: Equatable, Identifiable, Sendable {
    public var group: UInt8
    public var channel: UInt8
    public var dest: UInt64?
    /// Notes on it in the file.
    public var notes: UInt64

    public var id: UInt16 { UInt16(group) << 8 | UInt16(channel) }

    public init(group: UInt8, channel: UInt8, dest: UInt64?, notes: UInt64) {
        self.group = group
        self.channel = channel
        self.dest = dest
        self.notes = notes
    }
}

/// The report's wording, shared by the sheet, Copy and Save as Text (plan 6a
/// Task 9). Reports say "track", never "lane".
public enum ReportText {
    /// The groups in the order "Conflicts" (a merge's, plan 6e Ruling 15),
    /// "Changed", "Left out", "Notes"; empty ones are omitted.
    public static func groups(_ r: ReportRow) -> [(title: String, items: [ReportItemRow])] {
        let order: [(String, ReportKindRow)] = [
            ("Conflicts", .conflict), ("Changed", .changed), ("Left out", .leftOut), ("Notes", .note),
        ]
        return order.compactMap { title, kind in
            let items = r.items.filter { $0.kind == kind }
            return items.isEmpty ? nil : (title, items)
        }
    }

    /// "<what> (<count>)" when it happened more than once.
    public static func line(_ i: ReportItemRow) -> String {
        i.count > 1 ? "\(i.what) (\(i.count))" : i.what
    }

    /// The places given, then "and N more" when there were more. An item
    /// kept without places shows only its count (as `Report::plain`).
    public static func details(_ i: ReportItemRow) -> [String] {
        var out = i.details
        let given = UInt64(i.details.count)
        if given > 0, i.count > given { out.append("and \(i.count - given) more") }
        return out
    }

    /// The summary, then each group's title and lines with indented details.
    public static func plain(_ r: ReportRow) -> String {
        let groups = groups(r)
        guard !groups.isEmpty else { return "\(r.summary)\nEverything transferred." }
        var lines = [r.summary]
        for g in groups {
            lines.append(g.title)
            for item in g.items {
                lines.append("  \(line(item))")
                lines += details(item).map { "    \($0)" }
            }
        }
        return lines.joined(separator: "\n")
    }
}

/// The MIDI import and export sheets and the report sheet (plan 6a Task 9;
/// Rulings 12, T7-a; F12). Every synchronous core call a sheet makes runs
/// in a detached task: its result is applied on the main actor only if no
/// newer call was made since.
@Observable
@MainActor
public final class InterchangeModel {
    public enum ExportKind: Equatable, Sendable { case smf, clip }

    /// Weak: a sheet can outlive its document.
    @ObservationIgnored private weak var document: DocumentModel?
    @ObservationIgnored private var infoGeneration = 0
    @ObservationIgnored private var checkGeneration = 0
    @ObservationIgnored private var pending: [Int: Task<Void, Never>] = [:]
    @ObservationIgnored private var taskCount = 0
    @ObservationIgnored private var resetting = false

    public init(document: DocumentModel) { self.document = document }

    // MARK: Report

    /// The report the sheet shows.
    public var report: ReportRow?
    public var reportShown = false
    /// What the export that made `report` wrote (Show in Finder); empty
    /// for an import or a report the session kept from before.
    public private(set) var written: [String] = []

    /// File › Last Import/Export Report: shows the session's last report.
    public func showLastReport() {
        guard let document else { return }
        let last = ((try? document.session.interchangeReport()) ?? nil) ?? report
        guard let last else {
            document.editor.status = "There is no import or export report yet."
            return
        }
        if last != report { written = [] }
        report = last
        reportShown = true
    }

    /// A job this document's UI started ended well: its own report is shown.
    func jobEnded(_ job: JobRow) {
        guard let r = job.report else { return }
        report = r
        written = job.kind == .exportMidi || job.kind == .exportProject || job.kind == .exportDawproject || job.kind == .exportAaf
            ? job.written : []
        reportShown = true
    }

    /// A merge's report (Merge in the merge sheet, plan 6e Task 10): shown
    /// as an import's is, with nothing written.
    func show(_ r: ReportRow) {
        report = r
        written = []
        reportShown = true
    }

    /// A job this document's UI started failed. A MIDI export's refusal
    /// goes back to its sheet, which opens again; answers whether it did
    /// (never for another kind, such as a DAWproject export).
    func jobFailed(_ job: JobRow, message: String) -> Bool {
        guard job.kind == .exportMidi, job.state != .cancelled else { return false }
        exportError = message
        exportShown = true
        return true
    }

    /// The text of `error` without an alert's title.
    static func message(_ error: any Error) -> String { AlertItem(error, title: "").message }

    // MARK: Import

    public var importShown = false
    public private(set) var importPath = ""
    public private(set) var info: MidiFileInfoRow?
    public var choices: [ChannelChoice] = []
    public var splitChannels = false
    public var mpe = false
    /// The sheet's "where": a new project instead of this one
    /// (`DocumentStore.importAsNewProject`).
    public var intoNewProject = false
    private var wantsTempo = false

    /// The core takes the file's tempo into a sequence without events
    /// (Ruling T7-a): the box shows on, disabled.
    public var useTempoLocked: Bool { targetIsEmpty }

    public var useTempo: Bool {
        get { useTempoLocked || wantsTempo }
        set { wantsTempo = newValue }
    }

    private var targetIsEmpty: Bool {
        guard let document else { return false }
        return !document.lanes.contains { $0.midiEventCount > 0 || $0.regionCount > 0 || $0.referenceCount > 0 }
    }

    /// Extensions of the files Open and drops send here.
    public static func isMidi(_ path: String) -> Bool {
        ["mid", "midi", "smf", "midi2"].contains((path as NSString).pathExtension.lowercased())
    }

    /// Whether Open and drops send `path` to the DAWproject import
    /// (`DocumentStore.openDawproject`; any case of the extension).
    public static func isDawproject(_ path: String) -> Bool {
        (path as NSString).pathExtension.lowercased() == "dawproject"
    }

    /// Whether Open and drops send `path` to the AAF import
    /// (`DocumentStore.openAaf`; any case of the extension).
    public static func isAaf(_ path: String) -> Bool {
        (path as NSString).pathExtension.lowercased() == "aaf"
    }

    /// Reads `path`, then shows the import sheet; a refusal is the alert
    /// "Import Failed".
    public func beginImport(path: String) {
        guard let document else { return }
        infoGeneration += 1
        let generation = infoGeneration
        let session = document.session
        run {
            let result = await Task.detached { Result { try session.midiFileInfo(path: path) } }.value
            self.landed(generation, path, result)
        }
    }

    private func landed(_ generation: Int, _ path: String, _ result: Result<MidiFileInfoRow, any Error>) {
        guard generation == infoGeneration, let document else { return }
        switch result {
        case .failure(let error):
            document.alert = AlertItem(error, title: "Import Failed")
        case .success(let i):
            info = i
            importPath = path
            choices = i.channels.map { ChannelChoice(group: $0.group, channel: $0.channel, dest: $0.suggested, notes: $0.notes) }
            splitChannels = i.kind == .smf0
            mpe = i.mpeFound
            wantsTempo = false
            intoNewProject = false
            importShown = true
        }
    }

    /// The sheet's Import: the shown sequence at the playhead, the selected
    /// track (if one) as the drop track.
    public func startImport() {
        guard let document else { return }
        let target = DocumentModel.importTarget(
            seq: document.selectedSeq, selection: document.editor.selection.lanes, files: 1,
            tick: document.transport.tick)
        guard let target else {
            document.alert = AlertItem(title: "Import Failed", message: "No sequence is shown.")
            return
        }
        importShown = false
        let req = MidiImportRequestRow(
            path: importPath, newProject: false, seq: target.seq, lane: target.lane, tick: target.tick,
            channelMap: choices.map { ChannelMapRow(group: $0.group, channel: $0.channel, dest: $0.dest) },
            splitChannels: splitChannels, mpe: mpe, useTempo: useTempo)
        document.startJob(.importMidi, "MIDI Import Failed") { try $0.importMidi(req: req) }
    }

    /// Files dropped on a track list or the arrangement (Ruling 12): every
    /// MIDI file imports with the defaults at `target`, each its own
    /// track(s) and undo step; the rest are audio. The core takes one MIDI
    /// import at a time, so the files queue: the next starts when the one
    /// before ends (`importEnded`), and one that fails is reported and the
    /// queue moves on (final review I1).
    public func importDropped(paths: [String], target: ImportTargetRow) {
        guard let document else { return }
        for path in paths where Self.isMidi(path) {
            dropQueue.append(
                MidiImportRequestRow(
                    path: path, newProject: false, seq: target.seq, lane: target.lane, tick: target.tick,
                    channelMap: [], splitChannels: nil, mpe: false, useTempo: false))
        }
        if dropJob == nil { startNextDrop() }
        let audio = paths.filter { !Self.isMidi($0) }
        if !audio.isEmpty { document.importAudio(paths: audio, target: target) }
    }

    /// Dropped MIDI files waiting for the running one.
    @ObservationIgnored private(set) var dropQueue: [MidiImportRequestRow] = []
    /// The running dropped file's job.
    @ObservationIgnored private var dropJob: UInt64?

    /// Starts the queue's next file; one refused at the start, or ended on
    /// `startJob`'s own poll, lets the one after it go at once.
    private func startNextDrop() {
        guard let document, !startingDrops else { return }
        startingDrops = true
        defer { startingDrops = false }
        while dropJob == nil, !dropQueue.isEmpty, !document.mergeHeld,
            !document.jobs.contains(where: { ForDawModel.blocking.contains($0.kind) })
        {
            let req = dropQueue.removeFirst()
            guard let id = document.startJob(.importMidi, "MIDI Import Failed", { try $0.importMidi(req: req) })
            else { continue }
            if document.jobs.contains(where: { $0.id == id }) { dropJob = id }
        }
    }

    /// The document is closing: dropped files still waiting never start.
    func documentClosing() { dropQueue.removeAll() }

    /// `startJob` polls, which ends in `startNextDrop` again.
    @ObservationIgnored private var startingDrops = false

    /// A job the core cannot run a MIDI import beside ended (`pollJobs`,
    /// after the job list is settled): waiting dropped files may start.
    func blockingEnded() { startNextDrop() }

    /// An import job of this document ended (`pollJobs`, after its outcome
    /// is shown): the next dropped file starts. A cancelled one empties the
    /// queue: the user stopped the drop.
    func importEnded(_ job: JobRow) {
        guard job.id == dropJob else { return }
        dropJob = nil
        if job.state == .cancelled { dropQueue.removeAll() }
        startNextDrop()
    }

    /// Imports `path` into this new, empty project with the defaults
    /// (Ruling 12), or with `sheet`'s choices, every channel on a new
    /// destination (the project has none yet). The refusal, if any, is
    /// answered, not shown.
    func importIntoNewProject(path: String, like sheet: InterchangeModel? = nil) -> (any Error)? {
        guard let document else { return nil }
        var failure: (any Error)?
        let req = MidiImportRequestRow(
            path: path, newProject: true, seq: document.selectedSeq ?? 0, lane: nil, tick: 0,
            channelMap: sheet?.choices.map { ChannelMapRow(group: $0.group, channel: $0.channel, dest: nil) } ?? [],
            splitChannels: sheet?.splitChannels, mpe: sheet?.mpe ?? false, useTempo: true)
        document.startJob(.importMidi, "Import Failed", failure: { failure = $0 }) { try $0.importMidi(req: req) }
        return failure
    }

    // MARK: Export

    public var exportShown = false
    public private(set) var exportKind: ExportKind = .smf
    public var type0 = false { didSet { changed(type0 != oldValue) } }
    public var ppq: UInt16 = 960 { didSet { changed(ppq != oldValue) } }
    public var lanes: Set<UInt64> = [] { didSet { changed(lanes != oldValue) } }
    public var exportMpe = false { didSet { changed(exportMpe != oldValue) } }
    /// Clip export: only the time selection.
    public var useSelection = false { didSet { changed(useSelection != oldValue) } }
    /// What the export would not carry over, from the core's dry run.
    public private(set) var wontTransfer: ReportRow?
    /// Why the last dry run or start was refused.
    public private(set) var exportError: String?

    /// A time selection exists to export.
    public var hasSelection: Bool {
        guard let t = document?.editor.selection.timeRange else { return false }
        return t.start < t.end
    }

    /// Shows the export sheet with the choices reset: the selected tracks,
    /// else all.
    public func showExport(_ kind: ExportKind) {
        guard let document else { return }
        resetting = true
        exportError = nil
        replaceQuestion = nil
        namesGeneration += 1
        exportKind = kind
        type0 = false
        ppq = 960
        exportMpe = false
        useSelection = false
        let all = document.lanes.map(\.id)
        let picked = document.editor.selection.lanes.filter { $0.seq == document.selectedSeq }.map(\.lane).filter(all.contains)
        lanes = Set(picked.isEmpty ? all : picked)
        resetting = false
        exportShown = true
        refresh()
    }

    private func changed(_ really: Bool) {
        guard really, !resetting, exportShown else { return }
        refresh()
    }

    private func smfRequest(seq: UInt64, path: String, replace: Bool) -> SmfExportRequestRow {
        SmfExportRequestRow(seq: seq, lanes: lanes.sorted(), path: path, replace: replace, type0: type0, ppq: ppq, mpe: exportMpe)
    }

    private func clipRequest(seq: UInt64, path: String, replace: Bool) -> ClipExportRequestRow {
        var start: Int64?, end: Int64?
        if useSelection, let t = document?.editor.selection.timeRange, t.start < t.end {
            (start, end) = (t.start, t.end)
        }
        return ClipExportRequestRow(seq: seq, lanes: lanes.sorted(), path: path, replace: replace, start: start, end: end)
    }

    /// Asks the core what the export would leave out, off the main actor.
    private func refresh() {
        guard let document else { return }
        guard let seq = document.selectedSeq else {
            wontTransfer = nil
            exportError = "No sequence is shown."
            return
        }
        checkGeneration += 1
        let generation = checkGeneration
        let session = document.session
        let check: @Sendable () throws -> ReportRow
        switch exportKind {
        case .smf:
            let req = smfRequest(seq: seq, path: "", replace: false)
            check = { try session.exportMidiCheck(req: req) }
        case .clip:
            let req = clipRequest(seq: seq, path: "", replace: false)
            check = { try session.exportMidiClipCheck(req: req) }
        }
        run {
            let result = await Task.detached { Result { try check() } }.value
            self.checked(generation, result)
        }
    }

    private func checked(_ generation: Int, _ result: Result<ReportRow, any Error>) {
        guard generation == checkGeneration else { return }
        switch result {
        case .success(let r):
            wontTransfer = r
            exportError = nil
        case .failure(let error):
            wontTransfer = nil
            exportError = Self.message(error)
        }
    }

    /// "N files already exist in <folder>. Replace them?" ("1 file …
    /// Replace it?") before a clip export into a folder (review round 1).
    public struct ReplaceQuestion: Equatable, Sendable {
        public let folder: String
        /// The core's names of the files already there.
        public let taken: [String]

        public var count: Int { taken.count }

        public var message: String {
            let name = (folder as NSString).lastPathComponent
            return count == 1
                ? "1 file already exists in \(name). Replace it?"
                : "\(count) files already exist in \(name). Replace them?"
        }
    }

    public private(set) var replaceQuestion: ReplaceQuestion?
    @ObservationIgnored private var namesGeneration = 0

    /// The folder panel's answer for several clip tracks. The core names
    /// the files and says which are taken (final review I3), off the main
    /// actor: none taken, the export starts without `replace`, so a file
    /// that appears meanwhile is the core's refusal in the sheet; else the
    /// user is asked first.
    public func exportToFolder(_ folder: String) {
        guard let document, let seq = document.selectedSeq else {
            exportError = "No sequence is shown."
            return
        }
        namesGeneration += 1
        let generation = namesGeneration
        let session = document.session
        let req = clipRequest(seq: seq, path: folder, replace: false)
        run {
            let result = await Task.detached { Result { try session.exportMidiClipNames(req: req) } }.value
            guard generation == self.namesGeneration else { return }
            switch result {
            case .failure(let error):
                self.exportError = Self.message(error)
            case .success(let names) where names.taken.isEmpty:
                self.startExport(path: folder, replace: false)
            case .success(let names):
                self.replaceQuestion = ReplaceQuestion(folder: folder, taken: names.taken)
            }
        }
    }

    /// Replace: the export replaces them. Cancel: nothing is written and
    /// the sheet stays.
    public func answerReplace(_ replace: Bool) {
        guard let q = replaceQuestion else { return }
        replaceQuestion = nil
        if replace { startExport(path: q.folder, replace: true) }
    }

    /// The Save panel's answer (which has confirmed replacing: `replace`),
    /// or a folder (`exportToFolder`), replacing only what the user agreed
    /// to replace.
    public func startExport(path: String, replace: Bool = true) {
        guard let document else { return }
        guard let seq = document.selectedSeq else {
            exportError = "No sequence is shown."
            return
        }
        exportError = nil
        // Hidden before the job starts: a failure on `startJob`'s first poll
        // opens the sheet again (`jobFailed`), and must stay open.
        exportShown = false
        let started: UInt64?
        switch exportKind {
        case .smf:
            let req = smfRequest(seq: seq, path: path, replace: replace)
            started = document.startJob(.exportMidi, "MIDI Export Failed", failure: { [self] in
                exportError = Self.message($0)
            }) { try $0.exportMidi(req: req) }
        case .clip:
            let req = clipRequest(seq: seq, path: path, replace: replace)
            started = document.startJob(.exportMidi, "MIDI Export Failed", failure: { [self] in
                exportError = Self.message($0)
            }) { try $0.exportMidiClip(req: req) }
        }
        if started == nil { exportShown = true }
    }

    // MARK: Waiting

    /// Runs `body` as a task the model can wait for.
    private func run(_ body: @escaping @MainActor () async -> Void) {
        taskCount += 1
        let id = taskCount
        pending[id] = Task {
            await body()
            pending[id] = nil
        }
    }

    /// Waits until every check started so far has landed (or been dropped).
    public func settle() async {
        while let t = pending.values.first { await t.value }
    }
}
