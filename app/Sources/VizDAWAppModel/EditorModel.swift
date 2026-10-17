import Foundation
import Observation
import VizDAWCore

/// The editors of one document (spec §3.6): lane caches keyed by revision,
/// the shared selection, edit dispatch, and the Swift-side editor state
/// that is not document state (tool, layout, navigation). Owned by
/// `DocumentModel`. Caches are outside observation: a fetch made while a
/// view draws never invalidates that view; views observe `drawVersion`.
@Observable
@MainActor
public final class EditorModel {
    public let session: any SessionProtocol
    /// Hands a command's snapshot to the document (set by `DocumentModel`,
    /// which calls `sync` back).
    @ObservationIgnored var applySnapshot: (DocSnapshot) -> Void = { _ in }

    public private(set) var revs: [LaneKey: UInt64] = [:]
    public private(set) var snapshotVersion: UInt64 = 0
    /// Changes whenever something drawn may have changed.
    public private(set) var drawVersion = 0
    public private(set) var selection = SelectionRow(timeRange: nil, lanes: [], events: [])
    public private(set) var layout = EditorLayout()
    public var tool: Tool = .arrow
    /// The last failure of a user action (spec §5: non-modal).
    public var status: String?
    /// Sequences left by opening a reference; Back returns to the last.
    public private(set) var navigation: [UInt64] = []
    /// The drag in progress (spec §3.8).
    public internal(set) var gesture: Gesture?
    /// The reference whose parameters are being edited.
    public var referenceDraft: ReferenceDraft?

    private struct BlockEntry {
        let rev: UInt64
        let rule: BlockRule
        let blocks: [BlockRow]
        /// A sequence row changed since: reference labels and spans may be old.
        var stale = false
    }

    private struct EventEntry {
        let rev: UInt64
        let events: LaneEvents
    }

    /// Stands in for the revision of a lane the snapshot does not list, so
    /// its cache entry stays valid instead of refetching on every draw.
    private static let missingRev = UInt64.max

    @ObservationIgnored private var events: [LaneKey: EventEntry] = [:]
    /// The last failure of a draw-time read, so a repeating one is shown once.
    @ObservationIgnored private var lastReadFailure: String?
    @ObservationIgnored private var blockCache: [LaneKey: BlockEntry] = [:]
    @ObservationIgnored private var infoCache: [UInt64: (version: UInt64, info: SequenceInfo)] = [:]
    /// Sequences whose info read failed (not Busy) at a snapshot version.
    @ObservationIgnored private var infoFailures: [UInt64: UInt64] = [:]
    @ObservationIgnored private var sequenceRows: [SeqRow] = []
    @ObservationIgnored private var selectedEvents: Set<UInt64> = []

    @ObservationIgnored public private(set) var eventFetches = 0
    @ObservationIgnored public private(set) var blockFetches = 0
    @ObservationIgnored public private(set) var infoFetches = 0

    public init(session: any SessionProtocol, snapshot: DocSnapshot) {
        self.session = session
        layout = EditorLayout.decode((try? session.uiLayout()) ?? nil)
        sync(snapshot)
        refreshSelection()
    }

    /// Lanes with cached events or blocks (tests).
    var cachedLanes: Set<LaneKey> { Set(events.keys).union(blockCache.keys) }

    // MARK: Snapshots

    /// A new snapshot (spec §3.6): lanes that are gone leave the caches;
    /// lanes whose revision moved are refetched on their next read (their
    /// old rows stay as the fallback while the session is busy); blocks
    /// holding a reference go stale when any sequence row changed (Plan
    /// Ruling 2).
    func sync(_ s: DocSnapshot) {
        var next: [LaneKey: UInt64] = [:]
        for r in s.laneRevs { next[LaneKey(r.seq, r.lane)] = r.rev }
        for key in Array(events.keys) where next[key] == nil { events[key] = nil }
        for key in Array(blockCache.keys) where next[key] == nil { blockCache[key] = nil }
        if s.sequences != sequenceRows {
            for (key, entry) in blockCache where entry.blocks.contains(where: { $0.kind == .reference }) {
                blockCache[key]?.stale = true
            }
            sequenceRows = s.sequences
        }
        revs = next
        lastReadFailure = nil
        snapshotVersion = s.version
        drawVersion += 1
        afterSync(s)
        pruneLayout(s)
    }

    /// A gesture whose lanes vanished (undo in another window, a plugin
    /// timer step) is aborted (spec §5).
    func afterSync(_ s: DocSnapshot) {
        guard let g = gesture else { return }
        let alive = Set(s.laneRevs.map { LaneKey($0.seq, $0.lane) })
        guard !g.lanes.isSubset(of: alive) else { return }
        cancelGesture()
        status = "The lane was removed; the drag was cancelled."
    }

    // MARK: Reads

    /// A lane's events at its current revision (fetched when it moved).
    /// `Busy` keeps the stale rows silently; other failures go to `status`.
    public func laneEvents(_ key: LaneKey) -> LaneEvents? {
        let rev = revs[key] ?? Self.missingRev
        if let cached = events[key], cached.rev == rev { return cached.events }
        do {
            eventFetches += 1
            let fresh = try session.laneEvents(seq: key.seq, lane: key.lane)
            events[key] = EventEntry(rev: rev, events: fresh)
            return fresh
        } catch VizError.Busy {
            return events[key]?.events
        } catch {
            reportFromRead(error)
            return nil
        }
    }

    /// A lane's blocks under the layout's rule.
    public func blocks(_ key: LaneKey) -> [BlockRow] {
        let rule = layout.blockRule.rule
        let rev = revs[key] ?? Self.missingRev
        if let c = blockCache[key], c.rev == rev, c.rule == rule, !c.stale { return c.blocks }
        do {
            blockFetches += 1
            let b = try session.blocks(seq: key.seq, lane: key.lane, rule: rule)
            blockCache[key] = BlockEntry(rev: rev, rule: rule, blocks: b)
            return b
        } catch VizError.Busy {
            return blockCache[key]?.blocks ?? []
        } catch {
            reportFromRead(error)
            return []
        }
    }

    /// Tempo, meter and markers of `seq`, cached per snapshot version.
    public func info(_ seq: UInt64) -> SequenceInfo? {
        if let c = infoCache[seq], c.version == snapshotVersion { return c.info }
        if infoFailures[seq] == snapshotVersion { return nil }
        do {
            infoFetches += 1
            let i = try session.sequenceInfo(seq: seq)
            infoCache[seq] = (snapshotVersion, i)
            return i
        } catch VizError.Busy {
            return infoCache[seq]?.info
        } catch {
            infoFailures[seq] = snapshotVersion
            reportFromRead(error)
            return nil
        }
    }

    // MARK: Edits

    /// Sends one editor command (one undo step). Failures show in
    /// `status`; returns whether it applied.
    @discardableResult
    public func edit(_ cmd: EditCommand, filter: ViewFilterRow? = nil) -> Bool {
        do {
            applySnapshot(try session.edit(cmd: cmd, filter: filter))
            refreshSelection()
            return true
        } catch {
            report(error)
            return false
        }
    }

    public func report(_ error: any Error) {
        if let e = error as? VizError {
            status = e.message
        } else {
            status = error.localizedDescription
        }
    }

    /// A failure inside a read that runs while a view draws: observed state
    /// must not change mid-draw, so `status` is set on the next main-actor
    /// turn, once per distinct error until the next snapshot.
    private func reportFromRead(_ error: any Error) {
        let message = (error as? VizError)?.message ?? error.localizedDescription
        guard message != lastReadFailure else { return }
        lastReadFailure = message
        Task { @MainActor [weak self] in self?.status = message }
    }

    public func clearStatus() { status = nil }

    // MARK: Selection (Plan Ruling 19)

    public func setSelection(_ s: SelectionRow) {
        let before = selection
        selection = s
        selectedEvents = Set(s.events)
        drawVersion += 1
        do {
            try session.setSelection(selection: s)
        } catch {
            report(error)
            // Swift and Rust must not diverge: show what the session holds.
            let held = (try? session.selection()) ?? before
            selection = held
            selectedEvents = Set(held.events)
            drawVersion += 1
        }
    }

    /// Reads the selection back (after edits, undo and redo).
    public func refreshSelection() {
        guard let s = try? session.selection(), s != selection else { return }
        selection = s
        selectedEvents = Set(s.events)
        drawVersion += 1
    }

    public func clearSelection() {
        setSelection(SelectionRow(timeRange: nil, lanes: [], events: []))
    }

    public func isSelected(event id: UInt64) -> Bool { selectedEvents.contains(id) }

    /// Every event of `b` is selected.
    public func isSelected(_ b: BlockRow) -> Bool {
        !b.events.isEmpty && b.events.allSatisfy(selectedEvents.contains)
    }

    /// Selects a block's events, its lane and its time range; `adding`
    /// unites them with the current selection.
    public func selectBlock(_ b: BlockRow, in key: LaneKey, adding: Bool) {
        var s = adding ? selection : SelectionRow(timeRange: nil, lanes: [], events: [])
        for id in b.events where !s.events.contains(id) { s.events.append(id) }
        if !s.lanes.contains(key.ref) { s.lanes.append(key.ref) }
        let range = TickRange(start: b.start, end: max(b.end, b.start + 1))
        if let r = s.timeRange, adding {
            s.timeRange = TickRange(start: min(r.start, range.start), end: max(r.end, range.end))
        } else {
            s.timeRange = range
        }
        setSelection(s)
    }

    /// A time range over `lanes` (a ruler drag), no events.
    public func selectRange(_ range: TickRange, lanes: [LaneKey]) {
        setSelection(SelectionRow(timeRange: range, lanes: lanes.map(\.ref), events: []))
    }

    // MARK: Layout and navigation

    public func updateLayout(_ change: (inout EditorLayout) -> Void) {
        change(&layout)
        drawVersion += 1
    }

    /// Hands the layout to the session; the next save writes it.
    public func storeLayout() {
        try? session.setUiLayout(json: layout.encoded())
    }

    /// Selects a sequence in the document / reads the shown one (set by
    /// `DocumentModel`).
    @ObservationIgnored var selectSequence: (UInt64) -> Void = { _ in }
    @ObservationIgnored var currentSequence: () -> UInt64? = { nil }

    /// Shows `target` in the detail pane, in `kind` or the last-used
    /// editor (spec §3.7).
    public func openDetail(_ target: EditorTarget, kind: DetailEditorKind? = nil) {
        updateLayout { l in
            let k = kind ?? l.lastKind
            l.lastKind = k
            l.detail = DetailState(kind: k, target: target)
        }
    }

    /// Opens the sequence a reference block plays (Plan Ruling 14).
    public func openReference(_ b: BlockRow) {
        guard let target = b.refSeq, let current = currentSequence() else { return }
        push(current)
        selectSequence(target)
    }

    /// Back to the sequence the last opened reference sat in.
    public func back() {
        guard let parent = pop() else { return }
        selectSequence(parent)
    }

    public func push(_ parent: UInt64) { navigation.append(parent) }

    public func pop() -> UInt64? { navigation.popLast() }

    public func resetNavigation() { navigation = [] }
}
