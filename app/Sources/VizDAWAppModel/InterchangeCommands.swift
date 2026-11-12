import Foundation
import VizDAWCore

/// `file.import_midi`, `file.export_midi`, `file.export_midi_clip`,
/// `file.export_for_daw`, `file.import_dawproject`, `file.export_dawproject` and `file.interchange_report` over the control
/// channel (plans 6a Task 8, 6b Task 3): the
/// Open and Save dialogs' answers as parameters, checked here before the
/// core sees them; the core checks the destinations.
///
/// Errors: a `channel_map` that is not a JSON array of
/// `{"group", "channel", "dest"}` (group and channel 0 to 15, `dest` an ID or
/// null, each pair once), `lanes` that is not track IDs separated by commas
/// (each once), `start` without `end`, or `start` not before `end` is
/// `invalid_params` and never reaches the session (so is `strips` that is not
/// strip IDs or `none`, or repeats one). A missing `path` is
/// `dialog_required` ("Open" or "Save") unless `dry_run` is set. Nothing here
/// opens a panel, a sheet or a window. Nothing here reads a file or renders
/// on the main thread either: a `new_project` import's first read and a
/// `dry_run` are `.pending` until their detached work ends (final review
/// I2, M4).
@MainActor
public enum InterchangeCommands {
    public static let importMidi = "file.import_midi", exportMidi = "file.export_midi"
    public static let exportClip = "file.export_midi_clip", report = "file.interchange_report"
    public static let exportForDaw = "file.export_for_daw"
    public static let importDawproject = "file.import_dawproject", exportDawproject = "file.export_dawproject"
    public static let ids: Set<String> = [
        importMidi, exportMidi, exportClip, exportForDaw, importDawproject, exportDawproject, report,
    ]

    /// So `parse` can answer a `Result`.
    typealias Parsed<T> = Result<T, ControlFailure>

    private static func bad<T>(_ id: String, _ why: String) -> Parsed<T> {
        .failure(.of(.invalidParams, "\(id): \(why)"))
    }

    /// `channel_map`'s JSON as the core's rows: every element checked.
    static func channelMap(_ text: String, _ id: String) -> Parsed<[ChannelMapRow]> {
        let usage = "`channel_map` is a JSON array like [{\"group\":0,\"channel\":9,\"dest\":3}]"
        guard let data = text.data(using: .utf8),
            let json = try? JSONSerialization.jsonObject(with: data),
            let list = json as? [Any]
        else { return bad(id, usage) }
        guard list.count <= 256 else { return bad(id, "`channel_map` lists at most 256 pairs") }
        func whole(_ v: Any?) -> Int? {
            guard let n = v as? NSNumber, CFGetTypeID(n) != CFBooleanGetTypeID() else { return nil }
            return n as? Int
        }
        var rows: [ChannelMapRow] = []
        var seen = Set<[Int]>()
        for element in list {
            guard let o = element as? [String: Any], Set(o.keys).isSubset(of: ["group", "channel", "dest"]),
                let g = whole(o["group"]), let c = whole(o["channel"])
            else { return bad(id, usage) }
            guard (0...15).contains(g), (0...15).contains(c) else {
                return bad(id, "`channel_map`: group and channel are 0 to 15")
            }
            guard seen.insert([g, c]).inserted else {
                return bad(id, "`channel_map` lists group \(g) channel \(c) twice")
            }
            var dest: UInt64?
            if let d = o["dest"], !(d is NSNull) {
                guard let n = whole(d), n >= 0 else { return bad(id, "`channel_map`: `dest` is a destination ID or null") }
                dest = UInt64(n)
            }
            rows.append(ChannelMapRow(group: UInt8(g), channel: UInt8(c), dest: dest))
        }
        return .success(rows)
    }

    /// `lanes` as track IDs (each once); nil text: every track of `seq`.
    static func tracks(_ text: String?, seq: UInt64, _ id: String, _ m: DocumentModel) -> Parsed<[UInt64]> {
        guard let text else {
            do {
                return .success(try m.session.lanes(seq: seq).map(\.id))
            } catch {
                return .failure(.thrown(error, "Tracks Unavailable"))
            }
        }
        var out: [UInt64] = []
        for part in text.split(separator: ",", omittingEmptySubsequences: false) {
            guard !part.isEmpty, part.unicodeScalars.allSatisfy({ ("0"..."9").contains($0) }), let n = UInt64(part) else {
                return bad(id, "`lanes` is track IDs separated by commas, e.g. \"1,2\"")
            }
            guard !out.contains(n) else { return bad(id, "`lanes` lists track \(n) twice") }
            out.append(n)
        }
        return .success(out)
    }

    /// `seq`, else the shown sequence; `orNone` lets a call that needs none
    /// (an import into a new project) go on with 0, replaced once it exists.
    private static func seqArg(
        _ args: [ControlArg], _ id: String, _ m: DocumentModel, orNone: Bool = false
    ) -> Parsed<UInt64> {
        if let s = args.int("seq") {
            guard let n = UInt64(exactly: s) else { return bad(id, "`seq` is a sequence ID") }
            return .success(n)
        }
        if orNone { return .success(0) }
        guard let shown = m.selectedSeq else { return .failure(.of(.failed, "\(id): no sequence is shown")) }
        return .success(shown)
    }

    /// The import `args` ask for on `m`'s shown sequence (`seq` else).
    static func importRequest(_ args: [ControlArg], _ path: String, _ m: DocumentModel) -> Parsed<MidiImportRequestRow> {
        let id = importMidi
        let seq: UInt64
        let fresh = args.flag("new_project") ?? false
        if fresh, args.int("lane") != nil {
            return bad(id, "`lane` names a track of this project; a new project has none")
        }
        switch seqArg(args, id, m, orNone: fresh) {
        case .failure(let f): return .failure(f)
        case .success(let s): seq = s
        }
        var lane: UInt64?
        if let l = args.int("lane") {
            guard let n = UInt64(exactly: l) else { return bad(id, "`lane` is a track ID") }
            lane = n
        }
        var map: [ChannelMapRow] = []
        if let text = args.text("channel_map") {
            switch channelMap(text, id) {
            case .failure(let f): return .failure(f)
            case .success(let rows): map = rows
            }
        }
        return .success(
            MidiImportRequestRow(
                path: path, newProject: args.flag("new_project") ?? false, seq: seq, lane: lane,
                tick: args.int("tick") ?? 0, channelMap: map, splitChannels: args.flag("split_channels"),
                mpe: args.flag("mpe") ?? false, useTempo: args.flag("use_tempo") ?? false))
    }

    /// The Standard MIDI File export `args` ask for; `path` is "" for a check.
    static func smfRequest(_ args: [ControlArg], _ m: DocumentModel) -> Parsed<SmfExportRequestRow> {
        let id = exportMidi
        let seq: UInt64
        switch seqArg(args, id, m) {
        case .failure(let f): return .failure(f)
        case .success(let s): seq = s
        }
        let lanes: [UInt64]
        switch tracks(args.text("lanes"), seq: seq, id, m) {
        case .failure(let f): return .failure(f)
        case .success(let l): lanes = l
        }
        guard let ppq = UInt16(args.text("ppq") ?? "960"), [480, 960, 15360].contains(ppq) else {
            return bad(id, "`ppq` is 480, 960 or 15360")
        }
        return .success(
            SmfExportRequestRow(
                seq: seq, lanes: lanes, path: args.text("path") ?? "", replace: args.flag("replace") ?? false,
                type0: args.text("type") == "0", ppq: ppq,
                mpe: args.flag("mpe") ?? false))
    }

    /// The MIDI Clip File export `args` ask for; `path` is "" for a check.
    static func clipRequest(_ args: [ControlArg], _ m: DocumentModel) -> Parsed<ClipExportRequestRow> {
        let id = exportClip
        let seq: UInt64
        switch seqArg(args, id, m) {
        case .failure(let f): return .failure(f)
        case .success(let s): seq = s
        }
        let lanes: [UInt64]
        switch tracks(args.text("lanes"), seq: seq, id, m) {
        case .failure(let f): return .failure(f)
        case .success(let l): lanes = l
        }
        let start = args.int("start"), end = args.int("end")
        if let start {
            guard let end else { return bad(id, "`start` needs `end`") }
            guard start < end else { return bad(id, "`start` must be before `end`") }
        }
        return .success(
            ClipExportRequestRow(
                seq: seq, lanes: lanes, path: args.text("path") ?? "", replace: args.flag("replace") ?? false,
                start: start, end: end))
    }

    /// The export for another DAW `args` ask for; `path` is "" for a check.
    /// Defaults: the shown sequence, WAV at the project's rate and depth,
    /// the usual strips, dither on at 16 bits only.
    static func forDawRequest(_ args: [ControlArg], _ m: DocumentModel) -> Parsed<ForDawRequestRow> {
        let id = exportForDaw
        let seq: UInt64
        switch seqArg(args, id, m) {
        case .failure(let f): return .failure(f)
        case .success(let s): seq = s
        }
        var strips: [UInt64]?
        if let text = args.text("strips") {
            strips = []
            if text != "none" {
                for part in text.split(separator: ",", omittingEmptySubsequences: false) {
                    guard !part.isEmpty, part.unicodeScalars.allSatisfy({ ("0"..."9").contains($0) }),
                        let n = UInt64(part)
                    else { return bad(id, "`strips` is strip IDs separated by commas, e.g. \"1,2\", or none") }
                    guard strips?.contains(n) == false else { return bad(id, "`strips` lists strip \(n) twice") }
                    strips?.append(n)
                }
            }
        }
        let base = (try? m.session.projectInfo().format) ?? ProjectFormatRow(sampleRate: 48_000, depth: .int24)
        let container: ContainerRow
        switch args.text("container") {
        case "aiff": container = .aiff
        case "caf": container = .caf
        default: container = .wav
        }
        let depth: BitDepthRow
        switch args.text("depth") {
        case "16": depth = .int16
        case "24": depth = .int24
        case "32f": depth = .float32
        default: depth = base.depth
        }
        var rate = base.sampleRate
        if let r = args.int("rate") {
            guard let n = UInt32(exactly: r), (8_000...384_000).contains(n) else {
                return bad(id, "`rate` is 8000 to 384000 Hz")
            }
            rate = n
        }
        let dither = depth == .float32 ? false : (args.flag("dither") ?? (depth == .int16))
        return .success(
            ForDawRequestRow(
                seq: seq, strips: strips, container: container, depth: depth, rate: rate, dither: dither,
                path: args.text("path") ?? "", addToProject: args.flag("add_to_project") ?? false))
    }

    /// The DAWproject export `args` ask for; `path` is "" for a check.
    /// Default region gain: applied.
    static func dawprojectRequest(_ args: [ControlArg], _ m: DocumentModel) -> Parsed<DawprojectExportRequestRow> {
        let id = exportDawproject
        let seq: UInt64
        switch seqArg(args, id, m) {
        case .failure(let f): return .failure(f)
        case .success(let s): seq = s
        }
        return .success(
            DawprojectExportRequestRow(
                seq: seq, path: args.text("path") ?? "", replace: args.flag("replace") ?? false,
                regionGain: args.text("region_gain") == "leave_out" ? .leaveOut : .apply,
                blocks: m.editor.layout.blockRule.rule))
    }

    /// Projects `file.import_midi` made whose job has not ended.
    private static var madeProjects = Set<UUID>()

    /// Whether `file.import_midi` made project `doc` for a job not yet
    /// ended: the channel shows its window, as `file.new` does.
    public static func madeProject(_ doc: UUID) -> Bool { madeProjects.contains(doc) }

    /// Tells the command its job on `doc` ended. A project it made and the
    /// job failed or was cancelled on goes, if nothing was done in it since;
    /// `close` closes it (window and all).
    public static func jobEnded(_ doc: UUID, failed: Bool, store: DocumentStore, close: (UUID) -> Void) {
        guard madeProjects.remove(doc) != nil, failed, let m = store.model(doc), !m.canUndo else { return }
        close(doc)
    }

    /// Makes the project `file.import_midi` with `new_project` imports
    /// into and starts the import there (`pickSeq`: into its shown
    /// sequence).
    private static func importIntoNewProject(
        _ request: MidiImportRequestRow, pickSeq: Bool, store: DocumentStore
    ) -> ControlOutcome {
        guard let made = store.newProject(ProjectFormatRow(sampleRate: 48_000, depth: store.defaultFormat.depth)),
            let fresh = store.model(made)
        else {
            if let a = store.takeAlert() { return .failed(.alert(a)) }
            return .failed(.of(.failed, "\(importMidi) made no document."))
        }
        madeProjects.insert(made)
        var req = request
        if pickSeq, let shown = fresh.selectedSeq { req.seq = shown }
        do {
            return .job(command: importMidi, doc: made, job: try fresh.session.importMidi(req: req))
        } catch {
            return .failed(.thrown(error, "Import Failed"))
        }
    }

    /// Makes the project `file.import_dawproject` imports into (at the file's
    /// rate) and starts the import there. A refusal leaves no project.
    private static func importDawprojectIntoNewProject(
        _ path: String, rate: UInt32, store: DocumentStore
    ) -> ControlOutcome {
        guard let made = store.newProject(ProjectFormatRow(sampleRate: rate, depth: store.defaultFormat.depth)),
            let fresh = store.model(made)
        else {
            if let a = store.takeAlert() { return .failed(.alert(a)) }
            return .failed(.of(.failed, "\(importDawproject) made no document."))
        }
        madeProjects.insert(made)
        do {
            return .job(
                command: importDawproject, doc: made,
                job: try fresh.session.importDawproject(req: DawprojectImportRequestRow(path: path)))
        } catch {
            // Refused before a job started (e.g. Busy): nothing was done in it.
            madeProjects.remove(made)
            store.close(made)
            return .failed(.thrown(error, "Import Failed"))
        }
    }

    /// A dry run's answer, computed off the main thread (final review M4).
    private static func dryRun(
        _ id: String, _ docID: UUID, _ m: DocumentModel, _ check: @escaping @Sendable () throws -> ReportRow
    ) -> ControlOutcome {
        .pending(
            PendingOutcome({ Result { try check() } }) { r in
                switch r {
                case .success(let report):
                    return .done(
                        .interchange(
                            command: id, doc: docID.uuidString, window: nil, undoLabel: m.snapshot.undoLabel,
                            report: report, written: []))
                case .failure(let error):
                    return .failed(.thrown(error, "Export Check Failed"))
                }
            })
    }

    /// Runs `id` (one of `ids`) on document `docID`.
    public static func run(
        _ id: String, args: [ControlArg], _ docID: UUID, _ m: DocumentModel, store: DocumentStore
    ) -> ControlOutcome {
        func result(_ report: ReportRow?, written: [String] = []) -> ControlOutcome {
            .done(
                .interchange(
                    command: id, doc: docID.uuidString, window: nil, undoLabel: m.snapshot.undoLabel, report: report,
                    written: written))
        }
        let path = args.text("path")
        switch id {
        case report:
            do {
                return result(try m.session.interchangeReport())
            } catch {
                return .failed(.thrown(error, "Report Unavailable"))
            }
        case importMidi:
            guard let path else { return .failed(.dialog("Open", "path")) }
            let newProject = args.flag("new_project") ?? false
            guard newProject || m.editor.gesture == nil else {
                return ControlCommands.unavailable(EditorModel.gestureOpenMessage)
            }
            // Checked on this document's shown sequence before a project is made.
            let request: MidiImportRequestRow
            switch importRequest(args, path, m) {
            case .failure(let f): return .failed(f)
            case .success(let r): request = r
            }
            guard newProject else {
                do {
                    return .job(command: id, doc: docID, job: try m.session.importMidi(req: request))
                } catch {
                    return .failed(.thrown(error, "Import Failed"))
                }
            }
            // The core reads the file first, off the main thread (final
            // review I2): a refusal makes no project.
            let session = m.session
            let pickSeq = args.int("seq") == nil
            return .pending(
                PendingOutcome({ Result { try session.midiFileInfo(path: path) } }) { read in
                    if case .failure(let error) = read { return .failed(.thrown(error, "Import Failed")) }
                    return importIntoNewProject(request, pickSeq: pickSeq, store: store)
                })
        case exportMidi, exportClip:
            let dryRun = args.flag("dry_run") ?? false
            guard dryRun || path != nil else { return .failed(.dialog("Save", "path")) }
            do {
                if id == exportMidi {
                    let req: SmfExportRequestRow
                    switch smfRequest(args, m) {
                    case .failure(let f): return .failed(f)
                    case .success(let r): req = r
                    }
                    let session = m.session
                    return dryRun
                        ? Self.dryRun(id, docID, m) { try session.exportMidiCheck(req: req) }
                        : .job(command: id, doc: docID, job: try m.session.exportMidi(req: req))
                }
                let req: ClipExportRequestRow
                switch clipRequest(args, m) {
                case .failure(let f): return .failed(f)
                case .success(let r): req = r
                }
                let session = m.session
                return dryRun
                    ? Self.dryRun(id, docID, m) { try session.exportMidiClipCheck(req: req) }
                    : .job(command: id, doc: docID, job: try m.session.exportMidiClip(req: req))
            } catch {
                return .failed(.thrown(error, dryRun ? "Export Check Failed" : "Export Failed"))
            }
        case exportForDaw:
            let dryRun = args.flag("dry_run") ?? false
            let req: ForDawRequestRow
            switch forDawRequest(args, m) {
            case .failure(let f): return .failed(f)
            case .success(let r): req = r
            }
            let session = m.session
            if dryRun { return Self.dryRun(id, docID, m) { try session.exportForDawCheck(req: req).report } }
            guard let path, !path.isEmpty else { return .failed(.dialog("Save", "path")) }
            do {
                return .job(command: id, doc: docID, job: try session.exportForDaw(req: req))
            } catch {
                return .failed(.thrown(error, "Export Failed"))
            }
        case importDawproject:
            guard let path, !path.isEmpty else { return .failed(.dialog("Open", "path")) }
            // The core reads the file first, off the main thread (L8): a
            // refusal makes no project.
            let read = store.dawprojectInfo
            return .pending(
                PendingOutcome({ Result { try read(path) } }) { info in
                    switch info {
                    case .failure(let error): return .failed(.thrown(error, "Import Failed"))
                    case .success(let row):
                        return importDawprojectIntoNewProject(path, rate: row.sampleRate, store: store)
                    }
                })
        case exportDawproject:
            let dryRun = args.flag("dry_run") ?? false
            let req: DawprojectExportRequestRow
            switch dawprojectRequest(args, m) {
            case .failure(let f): return .failed(f)
            case .success(let r): req = r
            }
            let session = m.session
            if dryRun { return Self.dryRun(id, docID, m) { try session.exportDawprojectCheck(req: req) } }
            guard let path, !path.isEmpty else { return .failed(.dialog("Save", "path")) }
            do {
                return .job(command: id, doc: docID, job: try session.exportDawproject(req: req))
            } catch {
                return .failed(.thrown(error, "Export Failed"))
            }
        default:
            return .failed(.of(.unknownId, "VizDAW has no handler for \(id)."))
        }
    }
}
