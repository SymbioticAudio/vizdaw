import Foundation
import Observation
import VizDAWCore

/// A torn-off editor window that is key (Plan 4b Ruling 18).
public struct TornFocus: Equatable, Sendable {
    public let doc: UUID
    public let window: UUID

    public init(doc: UUID, window: UUID) {
        self.doc = doc
        self.window = window
    }
}

/// A DAWproject file that came from VizDAW, waiting for the return
/// prompt's answer (plan 6e Rulings 13, 14).
public struct ReturnPrompt: Equatable, Sendable {
    /// The file (absolute).
    public let path: String
    /// Its header, as the core read it.
    public let info: DawprojectInfoRow
    /// The open project it came from (its project ID, else a snapshot of
    /// its export), the most recently active when several match.
    public let match: UUID?
    /// That project's name, which the prompt shows.
    public let matchName: String?

    public init(path: String, info: DawprojectInfoRow, match: UUID?, matchName: String?) {
        self.path = path
        self.info = info
        self.match = match
        self.matchName = matchName
    }
}

/// The return prompt's buttons: "New Project", "Merge With Existing
/// Project…" and "Cancel".
public enum ReturnChoice: Sendable { case newProject, merge, cancel }

/// The open documents, one per window, keyed by the window's value.
@Observable
@MainActor
public final class DocumentStore {
    public typealias Make = () throws -> any SessionProtocol
    public typealias Open = (String) throws -> any SessionProtocol

    public private(set) var documents: [UUID: DocumentModel] = [:]
    /// Mixer windows' models, made on first use and dropped with their
    /// document.
    private var mixers: [UUID: MixerModel] = [:]
    /// Documents created without a window yet (Finder opens, launch
    /// arguments); the next window to appear adopts the first one.
    public private(set) var pending: [UUID] = []
    /// Failures that have no window to show them in.
    public var alert: AlertItem?
    /// Crash and autosave checks before a project opens (spec §10); none
    /// in tests that do not set it.
    public var recovery: Recovery?
    /// Runs before a document's session closes (the app closes that
    /// document's plugin windows and releases their editors).
    public var willClose: ((UUID) -> Void)?

    /// Makes a project in a format (set by `live()`; tests set a fake).
    public var makeProject: ((ProjectFormatRow) throws -> any SessionProtocol)?

    /// Reads a DAWproject file's header with no document open (plan 6d L8;
    /// the core's free function `dawprojectInfo(path:)`; tests set a fake).
    /// It parses the file: call it off the main thread. Both the UI's open
    /// and `file.import_dawproject` read through it.
    public var dawprojectInfo: @Sendable (String) throws -> DawprojectInfoRow = {
        try VizDAWCore.dawprojectInfo(path: $0)
    }

    /// Reads an AAF file's header with no document open (plan 6f Task 6; the
    /// core's free function `aafInfo(path:)`; tests set a fake). It parses
    /// the file: call it off the main thread. `file.import_aaf` reads
    /// through it.
    public var aafInfo: @Sendable (String) throws -> AafInfoRow = {
        try VizDAWCore.aafInfo(path: $0)
    }

    /// A file from VizDAW waiting for the return prompt's answer (plan 6e
    /// Task 10). `openDawproject` sets it for the UI's opens (File › Open,
    /// Import Project ▸ DAWproject…, Finder, the Dock); the app's
    /// `ReturnPrompts` shows it, and under the control channel shows
    /// nothing and answers Cancel. A second file read meanwhile waits its
    /// turn.
    public private(set) var returnPrompt: ReturnPrompt?
    /// Prompts waiting behind `returnPrompt`, oldest first.
    @ObservationIgnored private var queuedPrompts: [ReturnPrompt] = []

    /// Asks for the project to merge into when no open project matches: an
    /// Open panel the app sets (nil, or the panel's Cancel: nothing).
    @ObservationIgnored public var chooseMergeProject: (() -> String?)?

    /// Opens in flight (`openDawproject`), so tests can wait for them.
    @ObservationIgnored private var opening: [Int: Task<Void, Never>] = [:]
    @ObservationIgnored private var openCount = 0

    /// The app's preferences (in memory in tests; `live()` sets the real
    /// store, which is itself in memory under `VIZDAW_SUPPORT_DIR`).
    public var defaults: UserDefaults = MemoryDefaults()

    /// The "Default project format" preference (5a Ruling 18), e.g.
    /// "48000/24"; 48 kHz / 24-bit when unset or damaged.
    public var defaultFormat: ProjectFormatRow {
        get {
            // Observed through the counter: the preference itself is not.
            _ = formatChanges
            let s = defaults.string(forKey: "defaultProjectFormat") ?? ""
            let p = s.split(separator: "/").map(String.init)
            return ControlFormats.parse(rate: p.first, depth: p.count > 1 ? p[1] : nil)
                ?? ProjectFormatRow(sampleRate: 48_000, depth: .int24)
        }
        set {
            let d: String =
                switch newValue.depth {
                case .int16: "16"
                case .int24: "24"
                case .float32: "32f"
                }
            defaults.set("\(newValue.sampleRate)/\(d)", forKey: "defaultProjectFormat")
            formatChanges += 1
        }
    }

    /// Bumped when `defaultFormat` is set (Settings › Recording redraws).
    public private(set) var formatChanges = 0

    private let makeEmpty: Make
    private let makeDemo: Make
    private let openPath: Open

    public init(makeEmpty: @escaping Make, makeDemo: @escaping Make, open: @escaping Open) {
        self.makeEmpty = makeEmpty
        self.makeDemo = makeDemo
        self.openPath = open
    }

    /// The store backed by the Rust library.
    public static func live() -> DocumentStore {
        let s = DocumentStore(
            makeEmpty: { try Session.newEmpty() },
            makeDemo: { try Session.newDemo() },
            open: { try Session.open(path: $0) }
        )
        s.makeDemoAudio = { try Session.newDemoAudio() }
        s.makeProject = { try Session.newProject(format: $0) }
        s.recoverUntitled = { try recoverUntitledRecordings() }
        s.defaults = MemoryDefaults.appStore()
        s.listPreferences = ListPreferences(defaults: MemoryDefaults.appStore())
        return s
    }

    public func model(_ id: UUID) -> DocumentModel? { documents[id] }

    /// The torn-off editor window that is key, if any. AppKit windows
    /// publish no SwiftUI focused values, so menu commands ask this first.
    public private(set) var tornFocus: TornFocus?

    public func focusTorn(_ f: TornFocus) { tornFocus = f }

    /// Window `window` resigned key or closed.
    public func blurTorn(window: UUID) {
        if tornFocus?.window == window { tornFocus = nil }
    }

    /// The document whose select-and-modify panel is key (Plan 4c Ruling 22).
    public private(set) var panelFocus: UUID?

    public func focusPanel(_ doc: UUID) { panelFocus = doc }

    /// Document `doc`'s panel resigned key or closed.
    public func blurPanel(_ doc: UUID) {
        if panelFocus == doc { panelFocus = nil }
    }

    /// Document `doc`'s panel window became key: it takes the menu
    /// commands and the transport keys go off.
    public func panelBecameKey(_ doc: UUID) {
        focusPanel(doc)
        model(doc)?.selectModify.isKey = true
    }

    /// Document `doc`'s panel window resigned key: its fields give the
    /// keyboard back too (a field stays first responder in a non-key panel).
    public func panelResignedKey(_ doc: UUID) {
        blurPanel(doc)
        guard let p = model(doc)?.selectModify else { return }
        p.isKey = false
        p.endTextEditing()
    }

    /// Document `doc`'s panel window closed (by the user or with its document).
    public func panelClosed(_ doc: UUID) {
        blurPanel(doc)
        model(doc)?.selectModify.close()
    }

    /// The document menu commands act on: a key torn-off window's, else a
    /// key select-and-modify panel's, else the focused document window's.
    public func commandTarget(focused: UUID?) -> UUID? {
        tornFocus.map(\.doc) ?? panelFocus ?? focused
    }

    /// The key window among documents' own windows (document or mixer),
    /// by a token each window view holds; the timer, which has no SwiftUI
    /// focus, asks this. Per window, so a document window and its own
    /// mixer handing focus over never clear each other, in either order.
    private var keyWindow: (doc: UUID, window: UUID)?

    /// The document whose own window is key.
    public var keyDocument: UUID? { keyWindow?.doc }

    public func focusDocument(_ doc: UUID, window: UUID) {
        // A closing window can take key after its document went.
        guard documents[doc] != nil else { return }
        keyWindow = (doc, window)
        recentKey.removeAll { $0 == doc }
        recentKey.insert(doc, at: 0)
    }

    /// Documents whose window was key, the most recent first (final review
    /// R2); a closed one is removed.
    private var recentKey: [UUID] = []

    /// Documents in the order they were made or opened (closed ones are
    /// removed).
    private var added: [UUID] = []

    /// Documents closed this launch; their windows adopt no other.
    private var closedIDs: Set<UUID> = []

    /// The front-most document window still open: the one key last.
    /// Settings › Keys "Learn MIDI" learns into it when no document window
    /// is key (final review R2). Observable, deterministic.
    public var frontDocument: DocumentModel? { recentKey.lazy.compactMap(model).first }

    /// Window `window` (of document `doc`) resigned key or closed; only the
    /// window that last took focus clears it.
    public func blurDocument(_ doc: UUID, window: UUID) {
        if keyWindow?.doc == doc, keyWindow?.window == window { keyWindow = nil }
    }

    /// The document played notes go to: the command target (Plan 4c
    /// Ruling 11).
    public var noteTarget: DocumentModel? { commandTarget(focused: keyDocument).flatMap(model) }

    /// Document `id`'s mixer (one per document), or nil once it is closed.
    /// A failure alerts once until the mixer is made: the on-demand ask is
    /// retried as the document moves (final review m5), quietly.
    public func mixer(_ id: UUID) -> MixerModel? {
        if let m = mixers[id] { return m }
        guard let doc = documents[id] else { return nil }
        do {
            let m = try MixerModel(document: doc)
            mixers[id] = m
            mixerAlerted.remove(id)
            doc.storeMixer = m
            doc.mixerArrived()
            return m
        } catch {
            if mixerAlerted.insert(id).inserted { doc.alert = AlertItem(error, title: "Mixer Unavailable") }
            return nil
        }
    }

    /// Documents whose mixer failed and has alerted (`mixer`).
    @ObservationIgnored private var mixerAlerted: Set<UUID> = []

    public func newEmpty() -> UUID? { add("New Document Failed", makeEmpty) }

    /// A new project in `format` (the New Project sheet, `file.new`).
    public func newProject(_ format: ProjectFormatRow) -> UUID? {
        guard let make = makeProject else { return newEmpty() }
        return add("New Document Failed") { try make(format) }
    }

    public func newDemo() -> UUID? { add("Demo Project Failed", makeDemo) }

    /// Makes the demo with an audio lane (Plan 4b Ruling 19); set by `live()`.
    public var makeDemoAudio: Make?

    /// List defaults and columns given to every document (Plan 4c Ruling 19).
    public var listPreferences = ListPreferences()
    /// The app's key bindings, handed to every document (plan 4f).
    public var keyBindings: KeyBindings?
    /// The external sample editor's opener and watching, handed to every
    /// document (plan 5f-3; the app sets the live ones, nil: fakes).
    @ObservationIgnored public var externalTools: ExternalTools?

    public func newDemoAudio() -> UUID? {
        guard let make = makeDemoAudio else { return nil }
        return add("Demo Project Failed", make)
    }

    /// Opens a `.vizdaw` package; a MIDI file (`mid`, `midi`, `smf`,
    /// `midi2`) makes a new project and imports into it (plan 6a Ruling 12);
    /// a `.dawproject` starts `openDawproject` and answers nil (its project
    /// comes once the file is read); other paths are refused with an alert.
    public func open(path: String) -> UUID? {
        if InterchangeModel.isMidi(path) { return openMidi(path) }
        if InterchangeModel.isDawproject(path) {
            openDawproject(path)
            return nil
        }
        guard (path as NSString).pathExtension == "vizdaw" else {
            postAlert(AlertItem(
                title: "Open Failed",
                message: "\((path as NSString).lastPathComponent) is not a .vizdaw project."))
            return nil
        }
        // A project that is already open brings its window to the front
        // (the core would refuse with "already open in another window").
        if let existing = documentID(forPath: path) {
            activate?(existing)
            return nil
        }
        guard let recovery else {
            return add("Open Failed") { try self.openPath(path) }
        }
        guard let options = recovery.options(for: path) else { return nil }
        return add("Open Failed") { try recovery.openWith(path, options) }
    }

    /// The import sheet's Import with "a new project" chosen: the sheet
    /// closes and a new project imports its file with its choices.
    public func importAsNewProject(_ sheet: InterchangeModel) -> UUID? {
        sheet.importShown = false
        return openMidi(sheet.importPath, like: sheet)
    }

    /// A new 48 kHz project holding the MIDI file at `path` (with `sheet`'s
    /// choices, else the defaults); a refusal leaves nothing behind and
    /// alerts "Import Failed".
    private func openMidi(_ path: String, like sheet: InterchangeModel? = nil) -> UUID? {
        guard let id = newProject(ProjectFormatRow(sampleRate: 48_000, depth: defaultFormat.depth)),
            let model = documents[id]
        else { return nil }
        closeIfUntouched(id, model)
        if let error = model.interchange.importIntoNewProject(path: path, like: sheet) {
            close(id)
            postAlert(AlertItem(error, title: "Import Failed"))
            return nil
        }
        // A job that failed at once has already taken the project.
        return documents[id] == nil ? nil : id
    }

    /// A job that fails or is cancelled on project `id`, made for a file,
    /// takes the project with it while it is still untouched; the alert is
    /// the store's (as the control channel's imports do).
    private func closeIfUntouched(_ id: UUID, _ model: DocumentModel) {
        model.openedFile = { [weak self, weak model] message in
            guard let self, let model else { return }
            guard !model.canUndo else {
                if let message { model.alert = AlertItem(title: "Import Failed", message: message) }
                return
            }
            close(id)
            if let message { postAlert(AlertItem(title: "Import Failed", message: message)) }
        }
    }

    /// Starts opening `path` and returns at once (L8): a detached task
    /// calls `dawprojectInfo` (no document or session needed), then on the
    /// main actor a file from VizDAW sets `returnPrompt` and waits for
    /// `answerReturn` (plan 6e); any other makes a project at the file's rate and starts
    /// `importDawproject`; its window opens (`enqueue`). A refusal makes no
    /// project and alerts "Import Failed"; a job that fails or is cancelled
    /// on a still-untouched project closes it (as `openMidi`).
    public func openDawproject(_ path: String) {
        let read = dawprojectInfo
        openCount += 1
        let n = openCount
        opening[n] = Task {
            let info = await Task.detached { Result { try read(path) } }.value
            self.dawprojectRead(path, info)
            self.opening[n] = nil
        }
    }

    /// The file was read: one from VizDAW asks first (`returnPrompt`); any
    /// other makes its project at once.
    private func dawprojectRead(_ path: String, _ info: Result<DawprojectInfoRow, any Error>) {
        let row: DawprojectInfoRow
        switch info {
        case .failure(let error):
            postAlert(AlertItem(error, title: "Import Failed"))
            return
        case .success(let r): row = r
        }
        guard !row.fromVizdaw else {
            // One lstat per open document at most (`documentHolding`).
            let match = documentID(forProjectID: row.projectId) ?? documentHolding(exportID: row.exportId)
            let prompt = ReturnPrompt(
                path: path, info: row, match: match, matchName: match.flatMap { documents[$0]?.snapshot.title })
            if returnPrompt == nil { returnPrompt = prompt } else { queuedPrompts.append(prompt) }
            return
        }
        importAsNewProject(path, row)
    }

    /// The return prompt's answer. New Project: 6d's path (a project at
    /// the file's rate imports it). Merge: into the open match, else the
    /// project `chooseMergeProject` picks (nil: nothing), found open or
    /// opened, whose window comes forward with the merge sheet. Cancel:
    /// nothing. The next waiting prompt, if any, follows.
    public func answerReturn(_ choice: ReturnChoice) {
        guard let p = returnPrompt else { return }
        returnPrompt = queuedPrompts.isEmpty ? nil : queuedPrompts.removeFirst()
        switch choice {
        case .cancel:
            return
        case .newProject:
            importAsNewProject(p.path, p.info)
        case .merge:
            let target: UUID
            if let match = p.match, documents[match] != nil {
                target = match
                activate?(match)
            } else {
                guard let project = chooseMergeProject?() else { return }
                if let already = documentID(forPath: project) {
                    target = already
                    activate?(already)
                } else {
                    guard let opened = open(path: project) else { return }
                    target = opened
                    enqueue(opened)
                }
            }
            documents[target]?.merge.begin(path: p.path, info: p.info)
        }
    }

    /// 6d's open: a project at the file's rate imports `path`.
    private func importAsNewProject(_ path: String, _ row: DawprojectInfoRow) {
        guard let id = newProject(ProjectFormatRow(sampleRate: row.sampleRate, depth: defaultFormat.depth)),
            let model = documents[id]
        else { return }
        closeIfUntouched(id, model)
        var failure: (any Error)?
        let req = DawprojectImportRequestRow(path: path)
        model.startJob(.importProject, "Import Failed", failure: { failure = $0 }) { try $0.importDawproject(req: req) }
        if let failure {
            close(id)
            postAlert(AlertItem(failure, title: "Import Failed"))
            return
        }
        // A job that failed at once has already taken the project.
        if documents[id] != nil { enqueue(id) }
    }

    /// Waits for every pending open (tests).
    public func settle() async {
        while let t = opening.values.first { await t.value }
    }

    /// Brings a document's window to the front (set by the app).
    public var activate: ((UUID) -> Void)?
    /// Every new model, before any window shows it (set by the app: the
    /// surfaces' core and command sinks, plan 5f-1 Task 10).
    public var didMake: ((UUID, DocumentModel) -> Void)?

    /// The open document saved at `path` (same file, ignoring case and
    /// symbolic links), if any.
    public func documentID(forPath path: String) -> UUID? {
        let want = Self.canonical(path)
        return documents.first { (try? $0.value.session.path()).flatMap { $0 }.map(Self.canonical) == want }?.key
    }

    /// Open documents, the most recently active first: those whose window
    /// was key, most recent first, then the rest, newest first.
    private var byActivity: [UUID] {
        let keyed = recentKey.filter { documents[$0] != nil }
        return keyed + added.reversed().filter { documents[$0] != nil && !keyed.contains($0) }
    }

    /// The most recently active open document whose project ID is `id` (nil for nil).
    public func documentID(forProjectID id: String?) -> UUID? {
        guard let id else { return nil }
        return byActivity.first { (try? documents[$0]?.session.projectId()) == id }
    }

    /// The most recently active open document holding snapshot `exportID` (nil for nil).
    /// Each ask is one lstat in the core (plan 6e Task 8).
    public func documentHolding(exportID: String?) -> UUID? {
        guard let exportID else { return nil }
        return byActivity.first { (try? documents[$0]?.session.hasSnapshot(exportId: exportID)) == true }
    }

    private static func canonical(_ path: String) -> String {
        URL(fileURLWithPath: path).resolvingSymlinksInPath().standardizedFileURL.path.lowercased()
    }

    /// The launch-time check for recordings lost with untitled projects
    /// (set by `live()`); returns the line to show.
    public var recoverUntitled: () throws -> String? = { nil }
    private var launchRecoveryDone = false

    /// Runs `recoverUntitled` once per launch, before any document opens;
    /// its line is logged and queued as an alert (plan 5c Task 10 review
    /// M1: never dropped).
    public func runLaunchRecovery() {
        guard !launchRecoveryDone else { return }
        launchRecoveryDone = true
        guard let line = (try? recoverUntitled()) ?? nil else { return }
        FileHandle.standardError.write(Data("VizDAW: \(line)\n".utf8))
        postAlert(AlertItem(title: "Recordings Moved to the Trash", message: line))
    }

    /// Queues a document for the next window to appear.
    public func enqueue(_ id: UUID?) {
        if let id { pending.append(id) }
    }

    /// The oldest queued document, removed from the queue.
    public func takePending() -> UUID? {
        pending.isEmpty ? nil : pending.removeFirst()
    }

    /// The document a window should show: its own when it still exists,
    /// else the oldest queued one, else a new empty one (nil only if that
    /// fails). Covers fresh windows and stale restored or closed ids.
    public func documentForWindow(_ id: UUID?) -> UUID? {
        if let id, documents[id] != nil { return id }
        // A window whose document was closed is closing: it adopts nothing.
        if let id, closedIDs.contains(id) { return nil }
        return takePending() ?? newEmpty()
    }

    /// Alerts posted while another was showing, oldest first.
    private var queuedAlerts: [AlertItem] = []

    /// Shows `item` now, or after the alerts already showing or waiting
    /// (Task 10 review M1: never dropped).
    public func postAlert(_ item: AlertItem) {
        if alert == nil && queuedAlerts.isEmpty {
            alert = item
        } else {
            queuedAlerts.append(item)
        }
    }

    /// The store-level alert, cleared (the app shows it once); the next
    /// posted one takes its place.
    public func takeAlert() -> AlertItem? {
        let current = alert ?? (queuedAlerts.isEmpty ? nil : queuedAlerts.removeFirst())
        alert = queuedAlerts.isEmpty ? nil : queuedAlerts.removeFirst()
        return current
    }

    /// A window closed: its document goes (a clean close: the crash
    /// marker is removed).
    public func close(_ id: UUID) {
        closedIDs.insert(id)
        if tornFocus?.doc == id { tornFocus = nil }
        if panelFocus == id { panelFocus = nil }
        if keyWindow?.doc == id { keyWindow = nil }
        recentKey.removeAll { $0 == id }
        added.removeAll { $0 == id }
        mixers.removeValue(forKey: id)?.endOpenDrag()
        mixerAlerted.remove(id)
        // Plugin windows detach their editors before the session goes.
        willClose?(id)
        if let doc = documents.removeValue(forKey: id) {
            // A held merge ends quietly; nothing queued starts (plan 6e
            // Task 10 review I1). The session's close drops the hold.
            doc.documentClosing()
            // A note an editor sounds (a press on a closing piano roll) ends.
            doc.editor.auditionOffAll()
            // No list outlives its document (none keeps MIDI input running).
            doc.dropLists()
            if doc.isPlaying { try? doc.session.stop() }
            // Watching lasts until the project closes (spec §11).
            doc.stopExternalEdits()
            try? doc.session.close()
        }
    }

    /// The app quits: every document closes cleanly.
    public func closeAll() {
        for id in Array(documents.keys) { close(id) }
    }

    private func add(_ title: String, _ make: Make) -> UUID? {
        do {
            let model = try DocumentModel(session: make())
            model.listPreferences = listPreferences
            model.keys = keyBindings
            model.externalTools = externalTools
            let id = UUID()
            didMake?(id, model)
            documents[id] = model
            added.append(id)
            model.mixerProvider = { [weak self] in self?.mixer(id) }
            return id
        } catch {
            postAlert(AlertItem(error, title: title))
            return nil
        }
    }
}
