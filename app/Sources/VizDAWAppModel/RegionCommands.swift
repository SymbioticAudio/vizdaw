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

    public static let ids: Set<String> = [normalize, fadeIn, fadeOut, removeFades, openExternal, reloadExternal]

    /// Without a chosen editor, keys, surface buttons and the channel say
    /// this; only the menu item opens the chooser (owner answer 2026-11-03).
    public static let noEditor = "Choose an external editor in Settings › Editing."
    static let oneRegionMessage = "Select one audio region first."

    /// The Normalize panel's starting level (Ruling 18).
    public static let defaultPeakDb = -0.3

    static let noSelection = "Select audio regions first."

    public static func enabled(_ id: String, _ m: DocumentModel) -> Bool {
        guard ids.contains(id), m.editor.gesture == nil else { return false }
        let n = m.editor.selection.events.count
        return external.contains(id) ? n == 1 : n > 0
    }

    public static func isOn(_ id: String, _ m: DocumentModel) -> Bool { false }

    /// Runs `id` on the selection. Normalize uses the default level; the
    /// menu item asks for one first and calls `normalize(peakDb:_:)`.
    @discardableResult
    public static func run(_ id: String, on: Bool? = nil, _ m: DocumentModel) -> KeyOutcome {
        guard ids.contains(id) else { return .unavailable("VizDAW has no handler for \(id).") }
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

extension RegionCommands {
    /// The application `id` opens with: `app` when given, else the
    /// preference; the stub needs none. An application must be an
    /// existing folder (a bundle), checked before anything is written
    /// (pre-flight I11).
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
        guard FileManager.default.fileExists(atPath: path, isDirectory: &folder), folder.boolValue else {
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
    /// for keys and surface buttons: never a dialog. Open answers
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
