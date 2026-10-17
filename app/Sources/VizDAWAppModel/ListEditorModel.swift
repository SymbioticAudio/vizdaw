import Foundation
import Observation
import VizDAWCore

/// A cell of a list editor.
public struct ListCell: Equatable, Sendable {
    public var row: String
    public var column: ListColumn

    public init(row: String, column: ListColumn) {
        self.row = row
        self.column = column
    }
}

/// One list editor (spec §4c): its rows, cursor, filter, inserts and edits.
/// The detail pane and every torn-off window have their own; the document,
/// selection and preferences are shared. Owned by its `DocumentModel`.
@Observable
@MainActor
public final class ListEditorModel {
    public weak var document: DocumentModel?
    public let editor: EditorModel
    public let slot: DetailSlot
    @ObservationIgnored private let read: @MainActor () -> DetailState?
    @ObservationIgnored let write: @MainActor (DetailState) -> Void
    /// The row the keyboard cursor is on.
    public var cursor: String?
    /// The cell being edited (set by the view).
    public var editing: ListCell?
    /// Text a played note puts into the edited cell (the view takes it).
    public var fieldInput: String?
    public private(set) var lastInsertKind: ListKind = .note
    /// Notes played on a MIDI source enter pitch and velocity.
    public var midiIn = false

    private struct RowsKey: Equatable {
        let target: EditorTarget
        let filter: ListFilter
        let version: UInt64
        let revs: [UInt64?]
        let smpte: Bool
    }

    @ObservationIgnored private var cache: (key: RowsKey, rows: [ListRow], seconds: [String: Double])?
    @ObservationIgnored private var destCache: (version: UInt64, rows: [DestRow])?

    public init(
        document: DocumentModel, slot: DetailSlot, read: @escaping @MainActor () -> DetailState?,
        write: @escaping @MainActor (DetailState) -> Void
    ) {
        self.document = document
        self.editor = document.editor
        self.slot = slot
        self.read = read
        self.write = write
    }

    public static func pane(_ document: DocumentModel) -> ListEditorModel {
        ListEditorModel(
            document: document, slot: .pane,
            read: { [weak document] in document?.editor.layout.detail },
            write: { [weak document] d in document?.editor.updateLayout { $0.detail = d } })
    }

    public static func window(_ id: UUID, of document: DocumentModel) -> ListEditorModel {
        ListEditorModel(
            document: document, slot: .window(id),
            read: { [weak document] in document?.editor.layout.windows.first { $0.id == id }?.detail },
            write: { [weak document] d in
                document?.editor.updateLayout { l in
                    if let i = l.windows.firstIndex(where: { $0.id == id }) { l.windows[i].detail = d }
                }
            })
    }

    // MARK: What is shown

    public var state: DetailState? { read() }
    public var target: EditorTarget? { state?.target }
    public var preferences: ListPreferences? { document?.listPreferences }
    public var filter: ListFilter { state?.listFilter ?? preferences?.defaultFilter ?? ListFilter() }
    public var columnSettings: ListColumns { preferences?.columns ?? ListColumns() }

    /// The target's lane, or every lane of a sequence target.
    public var lanes: [LaneKey] {
        guard let t = target else { return [] }
        if let l = t.lane { return [LaneKey(t.seq, l)] }
        return editor.laneRows(t.seq).map { LaneKey(t.seq, $0.id) }
    }

    public var columns: [ListColumn] { columnSettings.visible(lanes: lanes.count) }

    public var rows: [ListRow] { built().rows }

    public func row(_ id: String) -> ListRow? { rows.first { $0.id == id } }

    public func index(of id: String) -> Int? { rows.firstIndex { $0.id == id } }

    public var cursorRow: ListRow? { cursor.flatMap(row) }

    private var range: Range<Int64>? {
        if case let .block(_, _, s, e) = target { return s..<max(e, s + 1) }
        return nil
    }

    private func built() -> (rows: [ListRow], seconds: [String: Double]) {
        guard let t = target else { return ([], [:]) }
        let keys = lanes
        let key = RowsKey(
            target: t, filter: filter, version: editor.snapshotVersion, revs: keys.map { editor.revs[$0] },
            smpte: columnSettings.smpte)
        if let c = cache, c.key == key { return (c.rows, c.seconds) }
        let laneRows = editor.laneRows(t.seq)
        let input = ListInput(
            lanes: keys.map { k in
                ListLane(
                    key: k, events: editor.laneEvents(k)?.events ?? [],
                    dest: laneRows.first { $0.id == k.lane }?.midiDest)
            },
            range: range, info: t.lane == nil ? editor.info(t.seq) : nil)
        let rows = ListRows.build(input, filter: filter)
        var seconds: [String: Double] = [:]
        if columnSettings.smpte, !rows.isEmpty,
            let secs = try? editor.session.secondsAt(seq: t.seq, ticks: rows.map(\.tick)), secs.count == rows.count
        {
            for (r, s) in zip(rows, secs) { seconds[r.id] = s }
        }
        cache = (key, rows, seconds)
        return (rows, seconds)
    }

    /// The instrument list (Destination column and filter), per snapshot.
    public var destinations: [DestRow] {
        if let c = destCache, c.version == editor.snapshotVersion { return c.rows }
        let rows = (try? editor.session.destinations()) ?? destCache?.rows ?? []
        destCache = (editor.snapshotVersion, rows)
        return rows
    }

    public var context: ListContext {
        guard let t = target else { return ListContext(seq: 0) }
        let info = editor.info(t.seq)
        let laneNames = Dictionary(
            editor.laneRows(t.seq).map { (LaneKey(t.seq, $0.id), $0.name) }, uniquingKeysWith: { a, _ in a })
        let dests = Dictionary(destinations.map { ($0.id, $0.name) }, uniquingKeysWith: { a, _ in a })
        let seqs = Dictionary(
            (document?.snapshot.sequences ?? []).map { ($0.id, $0.name) }, uniquingKeysWith: { a, _ in a })
        let session = editor.session
        return ListContext(
            seq: t.seq, meter: info?.meter ?? [], info: info, laneNames: laneNames, destNames: dests,
            seqNames: seqs, seconds: built().seconds, fps: columnSettings.fps,
            tickAtSeconds: { s in (try? session.ticksAt(seq: t.seq, seconds: [s]))?.first })
    }

    // MARK: Edits

    /// Commits typed text (Enter or Tab); false when refused (the status
    /// line says why) or the row is gone.
    @discardableResult
    public func commit(_ text: String, row id: String, column: ListColumn) -> Bool {
        guard let r = row(id) else { return false }
        let change: ListChange
        do {
            change = try ListEdit.commit(text, column: column, row: r, context: context)
        } catch let e as ListEditError {
            editor.status = e.message
            return false
        } catch {
            editor.report(error)
            return false
        }
        guard case let .edit(cmd, tick) = change else { return true }
        guard editor.edit(cmd) else { return false }
        follow(r, to: tick)
        return true
    }

    /// The cursor stays on the edited row: an event keeps its ID (and
    /// becomes the selection); a map row is found again by kind and tick.
    func follow(_ r: ListRow, to tick: Int64) {
        if r.event != nil {
            cursor = r.id
            select([r.id])
        } else {
            cursor = rows.first { $0.kind == r.kind && $0.tick == tick }?.id ?? cursor
        }
    }

    /// What Insert offers here: map kinds only for a whole sequence.
    public var insertKinds: [ListKind] {
        target?.lane == nil ? ListKind.insertable : ListKind.insertable.filter { $0.eventKind != nil }
    }

    /// Inserts `kind`, or the last inserted kind (Plan 4c Ruling 18).
    public func insert(_ kind: ListKind? = nil) {
        guard let t = target else { return }
        let k = kind ?? lastInsertKind
        if k == .reference || k == .audioRegion {
            editor.status = "References and regions are made in the arrangement."
            return
        }
        guard insertKinds.contains(k) else {
            editor.status = "Open the whole sequence to add tempo, meter or marker rows."
            return
        }
        let tick = insertTick()
        let lane = t.lane.map { LaneKey(t.seq, $0) } ?? cursorRow?.lane ?? lanes.first
        guard let cmd = ListInsert.command(k, tick: tick, lane: lane, seq: t.seq, info: editor.info(t.seq)) else {
            editor.status = k == .meter ? "There is already a meter change at the next barline." : "There is no lane to insert into."
            return
        }
        let before = Set(rows.compactMap(\.event).map(\.id))
        guard editor.edit(cmd) else { return }
        lastInsertKind = k
        let made =
            k.eventKind == nil
            ? rows.first { $0.kind == k && $0.tick >= tick }
            : rows.first { r in r.event.map { !before.contains($0.id) } ?? false }
        if let made {
            cursor = made.id
            if made.event != nil { select([made.id]) }
        }
    }

    /// The cursor row's tick, else the transport's when it is on this
    /// list's own sequence and inside the target's range, else the target's
    /// start (the list's anchor).
    func insertTick() -> Int64 {
        if let r = cursorRow { return r.tick }
        if let d = document, let t = target, d.transport.seq == t.seq,
            range?.contains(d.transport.tick) ?? true
        {
            return d.transport.tick
        }
        return range?.lowerBound ?? 0
    }

    public func delete(_ ids: [String]) {
        guard let t = target else { return }
        let set = Set(ids)
        let chosen = rows.filter { set.contains($0.id) }
        // The first tempo and meter entries are never deleted.
        let firsts = chosen.compactMap { r -> String? in
            switch r.source {
            case .tempo(0, _): return "tempo"
            case .meter(0, _): return "meter"
            default: return nil
            }
        }
        for cmd in ListDelete.commands(chosen, seq: t.seq, info: editor.info(t.seq)) {
            guard editor.edit(cmd) else { return }
        }
        if let first = firsts.first {
            editor.status = first == "tempo" ? "The first tempo change stays." : "The first meter change stays."
        }
        cursor = nil
    }

    /// Selected rows become the document selection (their events and lanes).
    public func select(_ ids: [String]) {
        let set = Set(ids)
        let chosen = rows.filter { set.contains($0.id) }
        var lanes: [LaneRef] = []
        for r in chosen {
            if let l = r.lane, !lanes.contains(l.ref) { lanes.append(l.ref) }
        }
        editor.setSelection(SelectionRow(timeRange: nil, lanes: lanes, events: chosen.compactMap(\.event).map(\.id)))
    }

    /// A reference row opens its sequence in this list (Plan 4c Ruling 24).
    @discardableResult
    public func openReference(_ id: String) -> Bool {
        guard let e = row(id)?.event, e.kind == .reference, let seq = e.refSeq, var s = state else { return false }
        editor.openSequence(seq)
        s.target = .sequence(seq)
        s.lanes = nil
        write(s)
        cursor = nil
        return true
    }

    /// Notes played on a MIDI source (Plan 4c Ruling 12).
    public func receive(_ notes: [NoteInRow]) {
        guard midiIn, let n = notes.last else { return }
        if let cell = editing {
            guard let r = row(cell.row) else { return }
            if cell.column == .data1, r.kind == .note || r.kind == .polyPressure {
                fieldInput = PitchName.name(n.pitch)
            } else if cell.column == .data2, r.kind == .note {
                fieldInput = "\(Velocity7.of(n.velocity))"
            }
            return
        }
        guard let r = cursorRow, var e = r.event, e.kind == .note else { return }
        e.pitch = n.pitch
        e.velocity = n.velocity
        if editor.edit(.event(edit: .replace(event: e.id, row: e))) { follow(r, to: e.tick) }
    }

    // MARK: Filters and columns (Plan 4c Ruling 19)

    public func setFilter(_ f: ListFilter) {
        guard var s = state else { return }
        s.listFilter = f
        write(s)
    }

    public func toggle(_ kind: ListKind) {
        var f = filter
        if f.hiddenKinds.contains(kind) { f.hiddenKinds.remove(kind) } else { f.hiddenKinds.insert(kind) }
        setFilter(f)
    }

    public func shows(dest: UInt64) -> Bool { filter.shownDests?.contains(dest) ?? true }

    public func toggle(dest: UInt64) {
        var f = filter
        let all = Set(destinations.map(\.id))
        var shown = f.shownDests ?? all
        if shown.contains(dest) { shown.remove(dest) } else { shown.insert(dest) }
        f.shownDests = shown == all ? nil : shown
        setFilter(f)
    }

    /// Every list of this document (the pane's and each window's) takes
    /// this list's filter.
    public func applyFilterToAll() {
        let f = filter
        editor.updateLayout { l in
            if l.detail?.kind == .list { l.detail?.listFilter = f }
            for i in l.windows.indices where l.windows[i].detail.kind == .list { l.windows[i].detail.listFilter = f }
        }
    }

    /// Only the hidden kinds are kept: destinations belong to this project.
    public func saveFilterAsDefault() { preferences?.defaultFilter = ListFilter(hiddenKinds: filter.hiddenKinds) }

    public func setColumns(_ c: ListColumns) { preferences?.columns = c }
}
