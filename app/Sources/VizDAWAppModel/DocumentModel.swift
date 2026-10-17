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
    public private(set) var selectedSeq: UInt64?
    public private(set) var lanes: [LaneRow] = []
    public private(set) var transport: TransportState
    /// Playing, which sequence, and the tick while stopped; set only when
    /// they change, so views that must not redraw on every poll observe
    /// this, not `transport`.
    public private(set) var playKey = PlayKey(playing: false, seq: nil, stoppedTick: 0)
    public private(set) var looping = false
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

    /// The list default filter and columns (set by `DocumentStore`).
    @ObservationIgnored public var listPreferences = ListPreferences()
    /// One list editor per slot.
    @ObservationIgnored private var lists: [DetailSlot: ListEditorModel] = [:]
    /// The list the keyboard was last in (Insert Event, played notes).
    @ObservationIgnored public var activeList: DetailSlot?

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

    public func dropList(_ slot: DetailSlot) {
        lists.removeValue(forKey: slot)
        if activeList == slot { activeList = nil }
    }

    /// The active list while its slot shows a list.
    public func listForCommands() -> ListEditorModel? {
        guard let slot = activeList, let l = lists[slot], l.state?.kind == .list else { return nil }
        return l
    }

    public var listensForNotes: Bool { lists.values.contains { $0.midiIn } }

    /// Played notes go to the active list when it listens, else to a
    /// listening one.
    public func receiveNotes(_ notes: [NoteInRow]) {
        let active = listForCommands().flatMap { $0.midiIn ? $0 : nil }
        (active ?? lists.values.first { $0.midiIn })?.receive(notes)
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
        editor.applySnapshot = { [weak self] in self?.apply($0) }
        editor.selectSequence = { [weak self] in self?.select($0) }
        editor.currentSequence = { [weak self] in self?.selectedSeq }
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
            } else if let seq = selectedSeq {
                try session.play(seq: seq)
            }
        } catch {
            alert = AlertItem(error, title: isPlaying ? "Stop Failed" : "Play Failed")
        }
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
