import Foundation
import Observation
import VizDAWCore

/// One window's document: snapshot, selection, lanes and transport.
/// All state comes from the session; intents call it and refresh.
@Observable
@MainActor
public final class DocumentModel {
    public let session: any SessionProtocol
    public private(set) var snapshot: DocSnapshot
    /// The editors (plan 4a).
    public let editor: EditorModel
    /// The select-and-modify panel (plan 4c).
    public let selectModify = SelectModifyModel()
    /// Step entry (plan 4f).
    public let stepEntry = StepEntryModel()
    /// The app's key bindings (set by `DocumentStore`; keyboard slots are
    /// checked against them, plan 4f Ruling 15).
    @ObservationIgnored public var keys: KeyBindings?
    public private(set) var selectedSeq: UInt64?
    public private(set) var lanes: [LaneRow] = []
    public private(set) var transport: TransportState
    /// Playing, which sequence, and the tick while stopped; set only when
    /// they change, so views that must not redraw on every poll observe
    /// this, not `transport`.
    public private(set) var playKey = PlayKey(playing: false, seq: nil, stoppedTick: 0)
    public private(set) var looping = false
    /// Where playing last started, for Stop (plan 4f Ruling 8).
    public private(set) var playStart: Int64?
    /// The sequence whose name is being edited in the sidebar.
    public private(set) var renamingSeq: UInt64?
    /// The edited name (view state lives here: the Command Line Tools
    /// cannot expand SwiftUI's `@State` macro).
    public var renameDraft = ""
    /// The fault popover is open.
    public var showsFaults = false
    /// Sidebar sheets and the delete confirmation (plan 4a).
    public var commentDraft: CommentDraft?
    public var slotDraft: KeySlotDraft?
    public var pendingDelete: UInt64?
    public var alert: AlertItem?
    /// Version and selection the current `lanes` were fetched for.
    private var lanesKey: LanesKey?
    /// How often `lanes` were fetched (tests check refetch rules).
    public private(set) var laneFetches = 0
    /// Plugins that failed to load or refused their state (newest last).
    public private(set) var pluginProblems: [String] = []
    /// The project's format, mismatched media and device rate (5a).
    public internal(set) var projectInfo: ProjectInfoRow?
    /// Running imports and conversions, oldest first.
    public internal(set) var jobs: [JobRow] = []
    /// A finished job's per-file problems, until dismissed.
    public internal(set) var jobProblems: [String] = []
    /// The disk warning was dismissed for this playback.
    var diskWarningDismissed = false
    /// Bumped when plugin slots change (loaded, failed, restarted): views
    /// that show slots refetch.
    public private(set) var slotsVersion = 0
    /// Slots whose parameters changed outside a gesture (their windows
    /// refetch), cleared by whoever handles them.
    public var paramsChanged: Set<UInt64> = []
    /// The playhead between polls (spec §3.4); fed by `poll()`.
    @ObservationIgnored public let clock = TransportClock()
    /// One piano roll per slot (Plan 4b Ruling 22).
    @ObservationIgnored private var pianoRolls: [DetailSlot: PianoRollModel] = [:]

    /// The piano roll of the detail pane or a torn-off window, made once.
    public func pianoRoll(_ slot: DetailSlot) -> PianoRollModel {
        if let m = pianoRolls[slot] { return m }
        let m: PianoRollModel
        switch slot {
        case .pane: m = .pane(self)
        case .window(let id): m = .window(id, of: self)
        }
        pianoRolls[slot] = m
        return m
    }

    /// A torn-off window closed: its piano roll goes, silencing its note.
    public func dropPianoRoll(_ slot: DetailSlot) {
        pianoRolls.removeValue(forKey: slot)?.silence()
    }

    /// One Strip Chart per slot (Plan 4d Ruling 21).
    @ObservationIgnored private var stripCharts: [DetailSlot: StripChartModel] = [:]

    /// The Strip Chart of the detail pane or a torn-off window, made once.
    public func stripChart(_ slot: DetailSlot) -> StripChartModel {
        if let m = stripCharts[slot] { return m }
        let m: StripChartModel
        switch slot {
        case .pane: m = .pane(self)
        case .window(let id): m = .window(id, of: self)
        }
        stripCharts[slot] = m
        return m
    }

    /// A torn-off window closed: its Strip Chart goes (and its stroke).
    public func dropStripChart(_ slot: DetailSlot) {
        stripCharts.removeValue(forKey: slot)?.strokeCancelled()
    }

    /// Strip Charts whose slot no longer shows one (the pane or a window
    /// switched editor, or the window left the layout) go (parked T14).
    func pruneStripCharts() {
        for (slot, c) in stripCharts where c.state?.kind != .stripChart { dropStripChart(slot) }
    }

    /// One drum grid per slot (Plan 4e Ruling 2).
    @ObservationIgnored private var drumGrids: [DetailSlot: DrumGridModel] = [:]
    /// The drum grid last pressed in, which the Drum menu acts on.
    public var activeDrumGrid: DetailSlot?

    /// The drum grid of the detail pane or a torn-off window, made once.
    public func drumGrid(_ slot: DetailSlot) -> DrumGridModel {
        if let m = drumGrids[slot] { return m }
        let m: DrumGridModel
        switch slot {
        case .pane: m = .pane(self)
        case .window(let id): m = .window(id, of: self)
        }
        drumGrids[slot] = m
        return m
    }

    /// A torn-off window closed: its drum grid goes (and its stroke).
    public func dropDrumGrid(_ slot: DetailSlot) {
        drumGrids.removeValue(forKey: slot)?.strokeCancelled()
        if activeDrumGrid == slot { activeDrumGrid = nil }
    }

    /// Drum grids whose slot no longer shows one go.
    func pruneDrumGrids() {
        for (slot, g) in drumGrids where g.state?.kind != .drumGrid { dropDrumGrid(slot) }
    }

    /// The drum grid the Drum menu acts on: the last one pressed in while
    /// it still shows a drum grid, else the detail pane's when it shows one.
    public func drumGridForCommands() -> DrumGridModel? {
        if let slot = activeDrumGrid, let g = drumGrids[slot], g.state?.kind == .drumGrid { return g }
        guard editor.layout.detail?.kind == .drumGrid else { return nil }
        return drumGrid(.pane)
    }

    /// The lane Drum > Drum Lane acts on: the selection's first lane of the
    /// shown sequence.
    public var selectedLane: LaneRow? {
        guard let seq = selectedSeq,
            let ref = editor.selection.lanes.first(where: { $0.seq == seq })
        else { return nil }
        return lanes.first { $0.id == ref.lane }
    }

    /// The list default filter and columns (set by `DocumentStore`).
    @ObservationIgnored public var listPreferences = ListPreferences()
    /// One list editor per slot, while that slot shows a list.
    @ObservationIgnored private(set) var lists: [DetailSlot: ListEditorModel] = [:]
    /// The list the keyboard was last in (Insert Event, played notes);
    /// observed, so Editors > Insert Event enables when a list gets focus.
    public var activeList: DetailSlot?

    /// The list editor of the detail pane or a torn-off window, made once.
    public func list(_ slot: DetailSlot) -> ListEditorModel {
        if let l = lists[slot] { return l }
        let l: ListEditorModel
        switch slot {
        case .pane: l = .pane(self)
        case .window(let id): l = .window(id, of: self)
        }
        lists[slot] = l
        return l
    }

    /// A torn-off window closed (also with its document): its list goes.
    public func dropList(_ slot: DetailSlot) {
        lists.removeValue(forKey: slot)
        if activeList == slot { activeList = nil }
    }

    /// The document closed: every list goes (none keeps MIDI input running).
    public func dropLists() {
        for slot in Array(lists.keys) { dropList(slot) }
    }

    /// Lists whose slot no longer shows a list (the pane switched editor or
    /// closed, a window changed editor or left the layout) go, so none
    /// lingers with MIDI In on or as the active list.
    func pruneLists() {
        for (slot, l) in lists where l.state?.kind != .list { dropList(slot) }
    }

    /// The active list while its slot shows a list.
    public func listForCommands() -> ListEditorModel? {
        guard let slot = activeList, let l = lists[slot], l.state?.kind == .list else { return nil }
        return l
    }

    /// Step entry is on, or a list listens (MIDI In on, its slot still
    /// showing a list).
    public var listensForNotes: Bool { stepEntry.isOn || lists.values.contains { $0.listens } }

    /// Played notes go to step entry while it is on (plan 4f Ruling 13);
    /// else to the active list when it listens, else the pane's, else the
    /// first listening window's (by ID, so the choice is stable).
    public func receiveNotes(_ notes: [NoteInRow]) {
        if stepEntry.isOn {
            stepEntry.receive(notes)
            return
        }
        var order: [DetailSlot] = []
        if let a = activeList { order.append(a) }
        order.append(.pane)
        order += lists.keys.compactMap { k -> UUID? in
            if case .window(let id) = k { return id }
            return nil
        }.sorted { $0.uuidString < $1.uuidString }.map { .window($0) }
        order.compactMap { lists[$0] }.first { $0.listens }?.receive(notes)
    }

    private struct LanesKey: Equatable {
        let version: UInt64
        let seq: UInt64?
    }

    public init(session: any SessionProtocol) throws {
        self.session = session
        let snap = try session.snapshot()
        snapshot = snap
        editor = EditorModel(session: session, snapshot: snap)
        // Busy (an AU v3 editor opening in a nested run loop): start
        // stopped at the beginning; the next poll reads the real state.
        do {
            transport = try session.transport()
        } catch VizError.Busy {
            transport = Self.stopped
        }
        clock.update(transport, pollInterval: Self.pollSeconds(playing: transport.playing))
        updatePlayKey()
        apply(snap)
        projectInfo = try? session.projectInfo()
        editor.applySnapshot = { [weak self] in self?.apply($0) }
        editor.selectSequence = { [weak self] in self?.select($0) }
        editor.currentSequence = { [weak self] in self?.selectedSeq }
        selectModify.document = self
        stepEntry.document = self
        editor.onExact = { [weak self] r in self?.selectModify.open(r) }
        editor.layoutChanged = { [weak self] in
            self?.pruneLists()
            self?.pruneStripCharts()
            self?.pruneDrumGrids()
        }
    }

    /// The transport shown until the first successful read.
    static let stopped = TransportState(
        playing: false, seq: nil, tick: 0, positionBbu: "1.1.0", seconds: 0,
        sampleRate: 0, device: "", faults: [], report: [], hostTimeNs: 0,
        ticksPerSecond: 30_720)

    // MARK: Derived state

    public var topLevel: [SeqRow] {
        SequenceOrder.sorted(snapshot.sequences.filter(\.topLevel), by: editor.layout.sequenceOrder)
    }
    public var referenced: [SeqRow] {
        SequenceOrder.sorted(snapshot.sequences.filter { !$0.topLevel }, by: editor.layout.sequenceOrder)
    }
    public var windowTitle: String { Formatting.windowTitle(snapshot) }
    public var undoTitle: String { snapshot.undoLabel.map { "Undo \($0)" } ?? "Undo" }
    public var redoTitle: String { snapshot.redoLabel.map { "Redo \($0)" } ?? "Redo" }
    public var canUndo: Bool { snapshot.undoLabel != nil }
    public var canRedo: Bool { snapshot.redoLabel != nil }
    public var isPlaying: Bool { transport.playing }
    /// Transport keys (Space, Return) are off while any name, comment, slot
    /// or reference field is edited, so typing a space types a space.
    public var transportKeysEnabled: Bool {
        renamingSeq == nil && commentDraft == nil && slotDraft == nil
            && editor.referenceDraft == nil && !editor.textEditing && !editor.panelKey
    }
    /// Return (Return to Start) is also off while a list editor has the
    /// keyboard: there it edits the cursor row (Plan 4c Ruling 20).
    public var returnKeyEnabled: Bool { transportKeysEnabled && !editor.listFocused }
    public var canPlay: Bool { selectedSeq != nil || isPlaying }
    /// 33 ms while playing, 250 ms when stopped (to catch the end).
    public var pollInterval: Duration { .seconds(pollSeconds) }
    /// `pollInterval` in seconds.
    public var pollSeconds: Double { Self.pollSeconds(playing: isPlaying) }

    /// The one place the poll period is defined.
    static func pollSeconds(playing: Bool) -> Double { playing ? 0.033 : 0.25 }
    public var hasFaults: Bool { !transport.faults.isEmpty }
    public var hasReport: Bool { !transport.report.isEmpty || !pluginProblems.isEmpty }
    /// Every line of the fault popover: faults, compile messages, plugin
    /// problems.
    public var messages: [String] { transport.faults + transport.report + pluginProblems }
    public var selectedName: String? {
        snapshot.sequences.first { $0.id == selectedSeq }?.name
    }

    // MARK: Intents

    public func select(_ seq: UInt64?) {
        guard seq != selectedSeq else { return }
        selectedSeq = seq
        refreshLanes()
        stepEntry.checkTarget()
    }

    /// The sidebar chose a sequence: reference navigation starts over.
    public func choose(_ seq: UInt64?) {
        editor.resetNavigation()
        select(seq)
    }

    public func beginRename(_ seq: UInt64) {
        renameDraft = snapshot.sequences.first { $0.id == seq }?.name ?? ""
        renamingSeq = seq
    }

    public func cancelRename() { renamingSeq = nil }

    /// Renames the sequence being edited; a blank or unchanged name
    /// just ends editing.
    public func commitRename(_ name: String) {
        guard let seq = renamingSeq else { return }
        renamingSeq = nil
        let trimmed = name.trimmingCharacters(in: .whitespacesAndNewlines)
        let current = snapshot.sequences.first { $0.id == seq }?.name
        guard !trimmed.isEmpty, trimmed != current else { return }
        rename(seq, to: trimmed)
    }

    public func rename(_ seq: UInt64, to name: String) {
        command("Rename Failed") { try $0.renameSequence(seq: seq, name: name) }
    }

    public func setMute(lane: UInt64, _ mute: Bool) {
        guard let seq = selectedSeq else { return }
        command("Mute Failed") { try $0.setLaneMute(seq: seq, lane: lane, mute: mute) }
    }

    public func toggleMute(lane: UInt64) {
        guard let row = lanes.first(where: { $0.id == lane }) else { return }
        setMute(lane: lane, !row.mute)
    }

    /// Undo and Redo do nothing while the plugin host is busy (an AU v3
    /// editor opening): the core refuses before touching history. While a
    /// drag is open they are refused with a status line (spec §3.8).
    public func undo() {
        guard gestureClosed() else { return }
        command("Undo Failed", quietWhenBusy: true) { try $0.undo() }
        editor.refreshSelection()
    }

    public func redo() {
        guard gestureClosed() else { return }
        command("Redo Failed", quietWhenBusy: true) { try $0.redo() }
        editor.refreshSelection()
    }

    public func playStop() {
        do {
            if isPlaying {
                try session.stop()
            } else if selectedSeq != nil {
                try startPlaying()
            }
        } catch {
            alert = AlertItem(error, title: isPlaying ? "Stop Failed" : "Play Failed")
        }
        poll()
    }

    /// Plays the shown sequence from `from` (nil: where the transport
    /// is), up to `to` where it stops (plan 4f Ruling 9), and remembers
    /// where it started.
    public func startPlaying(from: Int64? = nil, to: Int64? = nil) throws {
        guard let seq = selectedSeq else { throw VizError.Model(message: "Select a sequence to play.") }
        // The engine restarts at bar 1 from the end, or from another
        // sequence's position.
        let length = snapshot.sequences.first { $0.id == seq }?.lengthTicks ?? 0
        let there = transport.seq == nil || transport.seq == seq
        let start = from ?? (there && transport.tick < length ? transport.tick : 0)
        if let from {
            try session.playFrom(seq: seq, from: from, to: to)
        } else {
            try session.play(seq: seq)
        }
        playStart = start
        poll()
    }

    /// Stops and goes back to where playing started (the Stop key, plan
    /// 4f Ruling 8).
    public func stopAndReturn() throws {
        try session.stop()
        try session.locate(tick: playStart ?? 0)
        poll()
    }

    /// Moves the transport to `tick`, throwing (keys report a refusal in
    /// the status line instead of an alert).
    public func moveTo(_ tick: Int64) throws {
        try session.locate(tick: tick)
        poll()
    }

    public func toggleLoop() {
        do {
            try session.setLoop(on: !looping)
            looping.toggle()
        } catch {
            alert = AlertItem(error, title: "Loop Failed")
        }
    }

    private func updatePlayKey() {
        let k = PlayKey(transport)
        if k != playKey { playKey = k }
    }

    /// Moves the transport to `tick` (a click in a ruler).
    public func locate(_ tick: Int64) {
        do {
            try session.locate(tick: tick)
        } catch {
            alert = AlertItem(error, title: "Locate Failed")
        }
        poll()
    }

    public func returnToStart() { locate(0) }

    /// Reads the transport. A failure shows one alert, not one per poll.
    /// `Busy` (an AU v3 editor opening in a nested run loop) keeps the last
    /// transport, silently; the next poll tries again.
    public func poll() {
        do {
            transport = try session.transport()
            clock.update(transport, pollInterval: pollSeconds)
            updatePlayKey()
            if transport.rateMismatch != (projectInfo?.rateMismatch ?? false) { refreshProjectInfo() }
            if !transport.faults.contains("stream-underrun") { diskWarningDismissed = false }
        } catch VizError.Busy {
            return
        } catch {
            if alert == nil { alert = AlertItem(error, title: "Transport Failed") }
        }
    }

    /// The plugin timer's step (about 60 times a second, spec §5): plugin
    /// loads finish, plugin-UI gestures become undo steps. Failures go to
    /// the fault popover, not to alerts.
    public func idle() {
        // A drag's mouse-up that met Busy is retried here (spec §3.8).
        editor.tick()
        guard let u = try? session.idle() else { return }
        if let s = u.snapshot { apply(s) }
        if u.mediaChanged {
            mediaVersion += 1
            refreshProjectInfo()
        }
        if !jobs.isEmpty { pollJobs() }
        if u.slotsChanged { slotsVersion += 1 }
        if !u.paramsChanged.isEmpty { paramsChanged.formUnion(u.paramsChanged) }
        if !u.problems.isEmpty {
            pluginProblems.append(contentsOf: u.problems)
            if pluginProblems.count > 50 { pluginProblems.removeFirst(pluginProblems.count - 50) }
        }
    }

    /// True when Save needs a path first (untitled).
    public var needsSaveAs: Bool { ((try? session.path()) ?? nil) == nil }

    public func save() {
        editor.storeLayout()
        perform("Save Failed") { try $0.save() }
    }

    public func saveAs(path: String) {
        editor.storeLayout()
        perform("Save Failed") { try $0.saveAs(path: path) }
    }

    // MARK: Plumbing

    /// False (with a status line) while an editor gesture is open.
    private func gestureClosed() -> Bool {
        guard editor.gesture == nil else {
            editor.status = EditorModel.gestureOpenMessage
            return false
        }
        return true
    }

    private func command(
        _ title: String, quietWhenBusy: Bool = false,
        _ body: (any SessionProtocol) throws -> DocSnapshot
    ) {
        do {
            apply(try body(session))
        } catch VizError.Busy where quietWhenBusy {
            return
        } catch {
            alert = AlertItem(error, title: title)
        }
    }

    private func perform(_ title: String, _ body: (any SessionProtocol) throws -> Void) {
        do {
            try body(session)
            apply(try session.snapshot())
        } catch {
            alert = AlertItem(error, title: title)
        }
    }

    /// After a job applied (an import or conversion): the document, its
    /// lanes and the media rows are new.
    public func refreshAfterJob() {
        if let s = try? session.snapshot() { apply(s) }
        mediaVersion += 1
        refreshProjectInfo()
    }

    /// Bumped when the media pool or its offline state changed (views
    /// showing media refetch).
    public internal(set) var mediaVersion = 0

    /// Takes a new snapshot: keeps the selection if the sequence still
    /// exists (else the first top-level one), refetches lanes if needed.
    func apply(_ s: DocSnapshot) {
        snapshot = s
        editor.sync(s)
        if selectedSeq.map({ id in !s.sequences.contains { $0.id == id } }) ?? true {
            selectedSeq = s.sequences.first(where: \.topLevel)?.id ?? s.sequences.first?.id
        }
        if let r = renamingSeq, !s.sequences.contains(where: { $0.id == r }) {
            renamingSeq = nil
        }
        refreshLanes()
        stepEntry.checkTarget()
    }

    private func refreshLanes() {
        let key = LanesKey(version: snapshot.version, seq: selectedSeq)
        guard key != lanesKey else { return }
        lanesKey = key
        guard let seq = selectedSeq else {
            lanes = []
            return
        }
        do {
            laneFetches += 1
            lanes = try session.lanes(seq: seq)
        } catch {
            lanes = []
            alert = AlertItem(error, title: "Lanes Unavailable")
        }
    }
}

/// Whether the transport plays, which sequence, and where it sits while
/// stopped (a locate while stopped must move the playhead; ticks while
/// playing do not change it).
public struct PlayKey: Equatable, Sendable {
    public let playing: Bool
    public let seq: UInt64?
    /// The transport's tick while stopped; nil while playing.
    public let stoppedTick: Int64?

    public init(playing: Bool, seq: UInt64?, stoppedTick: Int64?) {
        self.playing = playing
        self.seq = seq
        self.stoppedTick = stoppedTick
    }

    init(_ t: TransportState) {
        self.init(playing: t.playing, seq: t.seq, stoppedTick: t.playing ? nil : t.tick)
    }
}
