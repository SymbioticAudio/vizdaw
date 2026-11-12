import Foundation
import VizDAWCore

/// `file.import_midi`, `file.export_midi`, `file.export_midi_clip` and
/// `file.interchange_report` over the control channel (plan 6a Task 8): the
/// Open and Save dialogs' answers as parameters, checked here before the
/// core sees them; the core checks the destinations.
///
/// Errors: a `channel_map` that is not a JSON array of
/// `{"group", "channel", "dest"}` (group and channel 0 to 15, `dest` an ID or
/// null, each pair once), `lanes` that is not track IDs separated by commas
/// (each once), `start` without `end`, or `start` not before `end` is
/// `invalid_params` and never reaches the session. A missing `path` is
/// `dialog_required` ("Open" or "Save") unless `dry_run` is set. Nothing here
/// opens a panel, a sheet or a window.
@MainActor
public enum InterchangeCommands {
    public static let importMidi = "file.import_midi", exportMidi = "file.export_midi"
    public static let exportClip = "file.export_midi_clip", report = "file.interchange_report"
    public static let ids: Set<String> = [importMidi, exportMidi, exportClip, report]

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
            var (target, doc, req) = (m, docID, request)
            if newProject {
                // The core reads the file first: a refusal makes no project.
                do {
                    _ = try m.session.midiFileInfo(path: path)
                } catch {
                    return .failed(.thrown(error, "Import Failed"))
                }
                guard let made = store.newProject(ProjectFormatRow(sampleRate: 48_000, depth: store.defaultFormat.depth)),
                    let fresh = store.model(made)
                else {
                    if let a = store.takeAlert() { return .failed(.alert(a)) }
                    return .failed(.of(.failed, "\(id) made no document."))
                }
                (target, doc) = (fresh, made)
                madeProjects.insert(made)
                if args.int("seq") == nil, let shown = fresh.selectedSeq { req.seq = shown }
            }
            do {
                return .job(command: id, doc: doc, job: try target.session.importMidi(req: req))
            } catch {
                return .failed(.thrown(error, "Import Failed"))
            }
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
                    return dryRun
                        ? result(try m.session.exportMidiCheck(req: req))
                        : .job(command: id, doc: docID, job: try m.session.exportMidi(req: req))
                }
                let req: ClipExportRequestRow
                switch clipRequest(args, m) {
                case .failure(let f): return .failed(f)
                case .success(let r): req = r
                }
                return dryRun
                    ? result(try m.session.exportMidiClipCheck(req: req))
                    : .job(command: id, doc: docID, job: try m.session.exportMidiClip(req: req))
            } catch {
                return .failed(.thrown(error, dryRun ? "Export Check Failed" : "Export Failed"))
            }
        default:
            return .failed(.of(.unknownId, "VizDAW has no handler for \(id)."))
        }
    }
}
