import Foundation
import VizDAWCore

/// The Region menu (spec §10.2; plan 5b Task 8): one implementation for
/// the menu items and the control channel's `command.run`, acting on the
/// selected events (the core ignores those that are not regions).
@MainActor
public enum RegionCommands {
    public static let normalize = "region.normalize"
    public static let fadeIn = "region.fade_in_to_cursor"
    public static let fadeOut = "region.fade_out_from_cursor"
    public static let removeFades = "region.remove_fades"
    /// Open in External Editor and Reload External Edit (plan 5f-3): one
    /// region, through `runExternal` (the channel, keys, surfaces) or
    /// `runMenu` (the Region menu), never `run` (pre-flight I6).
    public static let openExternal = "region.open_external"
    public static let reloadExternal = "region.reload_external"
    public static let external: Set<String> = [openExternal, reloadExternal]
    /// Crossfade and Remove Crossfade (plan 6c Task 5): routed before the
    /// generic fade commands (pre-flight F3), never `.fadeTo`.
    public static let crossfade = "region.crossfade"
    public static let removeCrossfade = "region.remove_crossfade"
    public static let crossfadeIds: Set<String> = [crossfade, removeCrossfade]

    public static let ids: Set<String> = [
        normalize, fadeIn, fadeOut, removeFades, openExternal, reloadExternal, crossfade, removeCrossfade,
    ]

    /// The fade shapes, as menus, pickers and the inspector name them.
    nonisolated public static let fadeShapes: [(FadeShapeRow, String)] = [
        (.linear, "Linear"), (.equalPower, "Equal Power"), (.exponential, "Exponential"), (.sCurve, "S-Curve"),
    ]

    /// How long a crossfade on `r` can be (pre-flight F17): the overlap,
    /// within what the fade out leaves of the region; never below 0.
    nonisolated public static func crossfadeRoom(_ r: EventRow) -> Int64 {
        // Both sides are 0 or more, so the difference cannot overflow.
        let left = Int64(clamping: r.frames) - max(r.fadeOut.length, 0)
        return max(min(Int64(clamping: r.overlapFrames), left), 0)
    }

    /// Settings › Editing's default crossfade when no host is at hand.
    public static let defaultCrossfadeMs = EditingPreferences.defaultCrossfadeMs
    public static let noCrossfadeTarget = "Select two overlapping audio regions on one track, or the later one."
    static let noCrossfadeMessage = "The selected region has no crossfade."

    /// Without a chosen editor, keys, surface buttons and the channel say
    /// this; only the menu item opens the chooser (owner answer 2026-11-03).
    public static let noEditor = "Choose an external editor in Settings › Editing."
    static let oneRegionMessage = "Select one audio region first."

    /// The Normalize panel's starting level (Ruling 18).
    public static let defaultPeakDb = -0.3

    static let noSelection = "Select audio regions first."

    public static func enabled(_ id: String, _ m: DocumentModel) -> Bool {
        guard ids.contains(id), m.editor.gesture == nil else { return false }
        if id == crossfade { return crossfadeTarget(m) != nil }
        if id == removeCrossfade { return crossfadeTarget(m)?.row.crossfade == true }
        let n = m.editor.selection.events.count
        return external.contains(id) ? n == 1 : n > 0
    }

    public static func isOn(_ id: String, _ m: DocumentModel) -> Bool { false }

    /// Runs `id` on the selection. Normalize uses the default level; the
    /// menu item asks for one first and calls `normalize(peakDb:_:)`.
    @discardableResult
    public static func run(_ id: String, on: Bool? = nil, _ m: DocumentModel) -> KeyOutcome {
        guard ids.contains(id) else { return .unavailable("VizDAW has no handler for \(id).") }
        if id == crossfade { return Self.crossfade(m, defaultMs: defaultCrossfadeMs) }
        if id == removeCrossfade { return Self.removeCrossfade(m) }
        if id == normalize { return Self.normalize(peakDb: defaultPeakDb, m) }
        let events = m.editor.selection.events
        guard !events.isEmpty else { return .unavailable(noSelection) }
        guard m.editor.gesture == nil else { return .unavailable(EditorModel.gestureOpenMessage) }
        // Never an edit (pre-flight I6): these run through `runExternal`
        // or `runMenu`.
        if id == openExternal { return .unavailable("Use Region › Open in External Editor.") }
        if id == reloadExternal { return .unavailable("Use Region › Reload External Edit.") }
        let cmd: EditCommand =
            id == removeFades
            ? .audio(edit: .removeFades(events: events))
            : .audio(edit: .fadeTo(events: events, at: m.transport.tick, fadeIn: id == fadeIn))
        return m.editor.edit(cmd) ? .done : .refused(m.editor.status ?? "The region command was refused.")
    }

    /// Normalize… to `peakDb` (−96…0 dBFS).
    @discardableResult
    public static func normalize(peakDb: Double, _ m: DocumentModel) -> KeyOutcome {
        let events = m.editor.selection.events
        guard !events.isEmpty else { return .unavailable(noSelection) }
        guard m.editor.gesture == nil else { return .unavailable(EditorModel.gestureOpenMessage) }
        guard peakDb.isFinite, (-96...0).contains(peakDb) else {
            return .refused("The peak must be a level from −96 to 0 dB.")
        }
        do {
            m.apply(try m.session.normalizeRegions(events: events, peakDb: Float(peakDb)))
            m.editor.refreshSelection()
            return .done
        } catch VizError.Busy(let message) {
            m.editor.status = message
            return .busy(message)
        } catch {
            m.editor.report(error)
            return .refused(m.editor.status ?? "Normalize was refused.")
        }
    }

    /// The level a text names ("-0.3", "−1", "0"): nil unless it is a
    /// finite number from −96 to 0.
    public static func parsePeak(_ text: String) -> Double? {
        let t = text.trimmingCharacters(in: .whitespaces).replacingOccurrences(of: "−", with: "-")
        guard let v = Double(t), v.isFinite, (-96...0).contains(v) else { return nil }
        return v
    }
}

extension RegionCommands: MenuCommands {}

/// Crossfades (plan 6c Task 5): Region › Crossfade and Remove Crossfade,
/// and `region.crossfade` / `region.remove_crossfade` on the channel.
extension RegionCommands {
    /// The longest `length_ms` the channel accepts (an hour).
    static let maxCrossfadeMs = 3_600_000.0

    /// The later region the selection names (Ruling 5): one selected region
    /// that starts inside another, or two selected regions of one track of
    /// which one starts inside the other. Nil for anything else.
    public static func crossfadeTarget(_ m: DocumentModel) -> (row: EventRow, lane: LaneKey)? {
        let wanted = m.editor.selection.events
        guard wanted.count == 1 || wanted.count == 2, Set(wanted).count == wanted.count else { return nil }
        for ref in m.editor.selection.lanes {
            let key = LaneKey(ref.seq, ref.lane)
            guard let rows = m.editor.laneEvents(key)?.events else { continue }
            let picked = rows.filter { wanted.contains($0.id) }
            guard picked.count == wanted.count, picked.allSatisfy({ $0.kind == .audioRegion }) else { continue }
            if picked.count == 1 {
                return picked[0].overlapWith == nil ? nil : (picked[0], key)
            }
            if let later = picked.first(where: { r in picked.contains { $0.id == r.overlapWith } }) {
                return (later, key)
            }
            return nil
        }
        return nil
    }

    /// A length in milliseconds: finite, more than 0, at most an hour.
    public static func parseLengthMs(_ text: String) -> Double? {
        // Decimal text only: Double("0x10") is 16 and "inf" is a number.
        guard let t = StripEntry.clean(text), !t.isEmpty,
            t.unicodeScalars.allSatisfy({ "0123456789.eE+-".unicodeScalars.contains($0) }),
            let v = Double(t), v.isFinite, v > 0, v <= maxCrossfadeMs
        else { return nil }
        return v
    }

    /// The shape a `shape` argument names.
    public static func shape(_ text: String) -> FadeShapeRow? {
        switch text {
        case "linear": .linear
        case "equal_power": .equalPower
        case "exponential": .exponential
        case "s_curve": .sCurve
        default: nil
        }
    }

    /// The region's media rate, the project rate when the media is unknown
    /// (the model's cached read), else 48 kHz.
    public static func sampleRate(of row: EventRow, _ m: DocumentModel) -> UInt32 {
        if let r = m.media.row(row.media)?.sampleRate, r > 0 { return r }
        if let r = m.projectInfo?.format.sampleRate, r > 0 { return r }
        return 48_000
    }

    /// `ms` as frames at the region's media rate (the project rate when the
    /// media is unknown), at least 1.
    static func frames(ms: Double, of row: EventRow, _ m: DocumentModel) -> UInt64 {
        let rate = Double(sampleRate(of: row, m))
        let f = (ms * rate / 1_000).rounded()
        return f >= 1 ? UInt64(min(f, 1e15)) : 1
    }

    /// Region › Crossfade and its key: the default length, or the overlap
    /// if shorter, equal power, as one edit.
    @discardableResult
    public static func crossfade(_ m: DocumentModel, defaultMs: Double) -> KeyOutcome {
        guard m.editor.gesture == nil else { return .unavailable(EditorModel.gestureOpenMessage) }
        guard let t = crossfadeTarget(m) else { return .unavailable(noCrossfadeTarget) }
        let length = CrossfadeLengthRow.upTo(frames: frames(ms: defaultMs, of: t.row, m))
        guard m.editor.edit(.audio(edit: .crossfade(event: t.row.id, length: length, shape: .equalPower))) else {
            return .refused(m.editor.status ?? "The crossfade was refused.")
        }
        return .done
    }

    /// Region › Remove Crossfade: the fade in stays as a plain one.
    @discardableResult
    public static func removeCrossfade(_ m: DocumentModel) -> KeyOutcome {
        guard m.editor.gesture == nil else { return .unavailable(EditorModel.gestureOpenMessage) }
        guard let t = crossfadeTarget(m), t.row.crossfade else { return .unavailable(noCrossfadeMessage) }
        guard m.editor.edit(.audio(edit: .removeCrossfade(event: t.row.id))) else {
            return .refused(m.editor.status ?? "Remove Crossfade was refused.")
        }
        return .done
    }

    /// `region.crossfade` and `region.remove_crossfade` on the channel: the
    /// ids name the later region, or none name the selection's. Bad
    /// arguments are `invalid_params` naming the command and the parameter,
    /// before any edit; the core's refusals are `failed`; an open gesture
    /// or no target is `unavailable`. Never a dialog.
    public static func runChannel(
        _ id: String, args: [ControlArg], doc: UUID, _ m: DocumentModel, defaultMs: Double
    ) -> ControlOutcome {
        func bad(_ param: String, _ why: String) -> ControlOutcome {
            .failed(.of(.invalidParams, "\(id): `\(param)` \(why)"))
        }
        for name in ["seq", "lane", "event", "length_ms", "shape"] where args.filter({ $0.name == name }).count > 1 {
            return bad(name, "is given more than once")
        }
        let given = ["seq", "lane", "event"].filter { n in args.contains { $0.name == n } }
        guard given.isEmpty || given.count == 3 else {
            return bad(
                ["seq", "lane", "event"].first { !given.contains($0) } ?? "event",
                "is missing: give `seq`, `lane` and `event` together, or none for the selection")
        }
        var lengthMs: Double?
        var shape = FadeShapeRow.equalPower
        if id == crossfade {
            if args.contains(where: { $0.name == "length_ms" }) {
                guard let text = args.text("length_ms"), let ms = parseLengthMs(text) else {
                    return bad("length_ms", "must be text for milliseconds more than 0 and at most 3600000, like \"12.5\"")
                }
                lengthMs = ms
            }
            if args.contains(where: { $0.name == "shape" }) {
                guard let text = args.text("shape"), let s = Self.shape(text) else {
                    return bad("shape", "is linear, equal_power, exponential or s_curve")
                }
                shape = s
            }
        }
        let target: (row: EventRow, lane: LaneKey)
        if given.isEmpty {
            guard m.editor.gesture == nil else { return ControlCommands.unavailable(EditorModel.gestureOpenMessage) }
            guard let t = crossfadeTarget(m) else { return ControlCommands.unavailable(noCrossfadeTarget) }
            if id == removeCrossfade, !t.row.crossfade { return ControlCommands.unavailable(noCrossfadeMessage) }
            target = t
        } else {
            guard let seq = MpeCommands.id(args, "seq") else { return bad("seq", "must be a sequence ID") }
            guard let lane = MpeCommands.id(args, "lane") else { return bad("lane", "must be a track ID") }
            guard let event = MpeCommands.id(args, "event") else { return bad("event", "must be a region ID") }
            let key = LaneKey(seq, lane)
            guard let rows = m.editor.laneEvents(key)?.events else {
                return bad("lane", "names no track of that sequence")
            }
            guard let row = rows.first(where: { $0.id == event && $0.kind == .audioRegion }) else {
                return bad("event", "must be an audio region on that track's active take")
            }
            guard m.editor.gesture == nil else { return ControlCommands.unavailable(EditorModel.gestureOpenMessage) }
            target = (row, key)
        }
        let edit: AudioEdit
        if id == crossfade {
            let frames: CrossfadeLengthRow
            if let lengthMs {
                frames = .frames(frames: Self.frames(ms: lengthMs, of: target.row, m))
            } else {
                frames = .upTo(frames: Self.frames(ms: defaultMs, of: target.row, m))
            }
            edit = .crossfade(event: target.row.id, length: frames, shape: shape)
        } else {
            edit = .removeCrossfade(event: target.row.id)
        }
        do {
            m.apply(try m.session.edit(cmd: .audio(edit: edit), filter: nil))
            m.editor.refreshSelection()
        } catch {
            return .failed(.thrown(error, id == crossfade ? "Crossfade Failed" : "Remove Crossfade Failed"))
        }
        return .done(ControlCommands.ran(id, doc, m))
    }
}

extension RegionCommands {
    /// The application `id` opens with: `app` when given, else the
    /// preference; the stub needs none. An application must be an
    /// absolute path to an existing folder ending in `.app` (a bundle),
    /// checked before anything is written (pre-flight I11; final review
    /// M6, M7).
    enum EditorApp {
        case app(URL?)
        case refused(ControlFailure)
    }

    static func editorApp(_ arg: String?, _ editor: ExternalEditorChoice) -> EditorApp {
        let path: String
        if let arg {
            path = arg
        } else {
            switch editor {
            case .none: return .refused(.of(.unavailable, noEditor))
            case .stub: return .app(nil)
            case .app(let p): path = p
            }
        }
        var folder: ObjCBool = false
        guard path.hasPrefix("/"), URL(fileURLWithPath: path).pathExtension == "app",
            FileManager.default.fileExists(atPath: path, isDirectory: &folder), folder.boolValue
        else {
            return .refused(.of(.failed, "\(path) is not an application."))
        }
        return .app(URL(fileURLWithPath: path))
    }

    /// The one selected event, or nil.
    static func oneRegion(_ m: DocumentModel) -> UInt64? {
        let events = m.editor.selection.events
        return events.count == 1 ? events.first : nil
    }

    /// `region.open_external` and `region.reload_external` (Ruling 7), also
    /// Open for keys and surface buttons (their Reload goes through
    /// `runMenu`: `KeyDispatch.command`, final review M9): never a dialog.
    /// Open answers
    /// "editing <copy>"; Reload answers its import job (`.job`), or
    /// "unchanged".
    public static func runExternal(
        _ id: String, args: [ControlArg], doc: UUID, _ m: DocumentModel, editor: ExternalEditorChoice
    ) -> ControlOutcome {
        guard let region = oneRegion(m) else { return ControlCommands.unavailable(oneRegionMessage) }
        guard m.editor.gesture == nil else { return ControlCommands.unavailable(EditorModel.gestureOpenMessage) }
        if id == reloadExternal {
            switch m.external.reload(region, announce: false) {
            case .importing(let job): return .job(command: id, doc: doc, job: job)
            case .unchanged:
                m.editor.status = "unchanged"
                return .done(ControlCommands.ran(id, doc, m))
            case .unavailable(let why): return ControlCommands.unavailable(why)
            case .busy(let why): return .failed(.of(.busy, why))
            case .failed(let why): return .failed(.of(.failed, why))
            }
        }
        switch editorApp(args.text("app"), editor) {
        case .refused(let f):
            return .failed(f)
        case .app(let app):
            do {
                let path = try m.external.open(region, app: app)
                m.editor.status = "editing \(path)"
                return .done(ControlCommands.ran(id, doc, m))
            } catch {
                return .failed(.thrown(error, ExternalEditModel.openFailedTitle))
            }
        }
    }

    /// The Region menu's two items (Ruling 7): the app's menu action has
    /// already asked for an editor when none was set. Reload's import shows
    /// its banner and a failure's alert, as any job; a refusal is an alert
    /// titled for the item; `unavailable` goes to the status line.
    public static func runMenu(_ id: String, _ m: DocumentModel, editor: ExternalEditorChoice) {
        guard let region = oneRegion(m) else {
            m.editor.status = oneRegionMessage
            return
        }
        if id == reloadExternal {
            switch m.external.reload(region) {
            case .unchanged: m.editor.status = "The external edit is unchanged."
            case .failed(let why): m.alert = AlertItem(title: "Reload External Edit Failed", message: why)
            case .unavailable(let why), .busy(let why): m.editor.status = why
            case .importing: break
            }
            return
        }
        switch editorApp(nil, editor) {
        case .refused(let f):
            m.alert = AlertItem(title: ExternalEditModel.openFailedTitle, message: f.message)
        case .app(let app):
            do {
                try m.external.open(region, app: app)
            } catch {
                m.alert = AlertItem(error, title: ExternalEditModel.openFailedTitle)
            }
        }
    }
}
