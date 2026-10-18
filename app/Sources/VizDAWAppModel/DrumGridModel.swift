import Foundation
import Observation
import VizDAWCore

/// A drum row by lane and row ID.
public struct DrumRowKey: Hashable, Sendable {
    public let lane: UInt64
    public let row: UInt64

    public init(lane: UInt64, row: UInt64) {
        self.lane = lane
        self.row = row
    }
}

/// One drum grid (spec §4e, Plan 4e Rulings 2–8): the drum lanes of the
/// shown sequence, the selected row, the stroke in progress and the
/// commands it sends. The grid's logic (layout, spots, cells, strokes,
/// drops) is Rust's (`drum_*` functions); this model keeps the state and
/// talks to the session. The detail pane and every torn-off window have
/// their own; the bar's settings are shared (`EditorLayout.drumGrid`).
@Observable
@MainActor
public final class DrumGridModel {
    /// Weak: closing a document while its grid is on screen must not trap.
    public weak var document: DocumentModel?
    public let editor: EditorModel
    /// Where it is shown (the Drum menu acts on the last one pressed in).
    public let slot: DetailSlot
    @ObservationIgnored private let read: @MainActor () -> DetailState?
    /// The row the row commands act on.
    public var selectedRow: DrumRowKey?
    public private(set) var stroke: DrumStrokeRow?
    /// The stroke's row and the row's hits when it began (the start state
    /// its live edits are measured from).
    @ObservationIgnored private var strokeTarget: (seq: UInt64, lane: UInt64, row: UInt64, hits: [DrumHitRow])?
    @ObservationIgnored private var strokeOpen = false
    @ObservationIgnored private var lanesCache: (seq: UInt64, version: UInt64, lanes: [DrumLaneRow])?
    @ObservationIgnored private var lanesFailure: (seq: UInt64, version: UInt64)?
    @ObservationIgnored private var layoutCache: (shapes: [[Bool]], settings: DrumGridSettingsRow, layout: DrumGridLayoutRow)?

    public static let paintLabel = "Paint Drum Hits"
    public static let eraseLabel = "Erase Drum Hits"
    public static let noRowMessage = "Select a drum row first."

    public init(document: DocumentModel, slot: DetailSlot, read: @escaping @MainActor () -> DetailState?) {
        self.document = document
        self.editor = document.editor
        self.slot = slot
        self.read = read
    }

    /// The drum grid of the detail pane.
    public static func pane(_ document: DocumentModel) -> DrumGridModel {
        DrumGridModel(document: document, slot: .pane, read: { [weak document] in document?.editor.layout.detail })
    }

    /// The drum grid of torn-off window `id`.
    public static func window(_ id: UUID, of document: DocumentModel) -> DrumGridModel {
        DrumGridModel(
            document: document, slot: .window(id),
            read: { [weak document] in document?.editor.layout.windows.first { $0.id == id }?.detail })
    }

    /// A press in this grid: the Drum menu acts on it from now on.
    public func becameActive() { document?.activeDrumGrid = slot }

    // MARK: What is shown

    public var state: DetailState? { read() }
    public var target: EditorTarget? { state?.target }
    public var seq: UInt64? { target?.seq }

    /// A block target's range, scrolled to when shown.
    public var focusRange: TickRange? {
        guard case let .block(_, _, start, end)? = target else { return nil }
        return TickRange(start: start, end: max(end, start + 1))
    }

    /// The drum lanes of the shown sequence, read once per snapshot. Busy
    /// keeps what was read before; another failure shows once in the
    /// status line and reads as none.
    public var lanes: [DrumLaneRow] {
        guard let seq else { return [] }
        let v = editor.snapshotVersion
        if let c = lanesCache, c.seq == seq, c.version == v { return c.lanes }
        if let f = lanesFailure, f.seq == seq, f.version == v { return [] }
        do {
            let l = try editor.session.drumLanes(seq: seq)
            lanesCache = (seq, v, l)
            return l
        } catch VizError.Busy {
            return lanesCache?.seq == seq ? lanesCache?.lanes ?? [] : []
        } catch {
            lanesFailure = (seq, v)
            let message = (error as? VizError)?.message ?? error.localizedDescription
            // Not while a view draws: on the next main-actor turn.
            Task { @MainActor [weak editor] in editor?.status = message }
            return []
        }
    }

    public var settings: DrumGridSettings { editor.layout.drumGrid }

    public func updateSettings(_ change: (inout DrumGridSettings) -> Void) {
        editor.updateLayout { change(&$0.drumGrid) }
    }

    /// The bands of the shown lanes (Rust's layout, cached).
    public var layout: DrumGridLayoutRow {
        let shapes = lanes.map { $0.rows.map(\.showVelocity) }
        let s = settings.row
        if let c = layoutCache, c.shapes == shapes, c.settings == s { return c.layout }
        let l = (try? drumGridLayout(shapes: shapes, settings: s)) ?? DrumGridLayoutRow(bands: [], height: 0)
        layoutCache = (shapes, s, l)
        return l
    }

    /// What height `y` (content coordinates) is over.
    public func spot(_ y: Double) -> DrumSpotRow {
        (try? drumGridSpot(layout: layout, y: y)) ?? .nothing
    }

    public func row(_ lane: Int, _ row: Int) -> DrumRowRow? {
        guard lanes.indices.contains(lane), lanes[lane].rows.indices.contains(row) else { return nil }
        return lanes[lane].rows[row]
    }

    /// Lane and row indexes of `key`.
    public func indexes(_ key: DrumRowKey) -> (lane: Int, row: Int)? {
        guard let l = lanes.firstIndex(where: { $0.lane == key.lane }),
            let r = lanes[l].rows.firstIndex(where: { $0.id == key.row })
        else { return nil }
        return (l, r)
    }

    /// The selected row while it still exists.
    public var selected: (lane: DrumLaneRow, row: DrumRowRow)? {
        guard let key = selectedRow, let i = indexes(key) else { return nil }
        return (lanes[i.lane], lanes[i.lane].rows[i.row])
    }

    /// The lane the lane controls act on: the selected row's, else the first.
    public var currentLane: DrumLaneRow? { selected?.lane ?? lanes.first }

    public func select(lane: Int, row: Int) {
        guard let r = self.row(lane, row) else { return }
        selectedRow = DrumRowKey(lane: lanes[lane].lane, row: r.id)
    }

    // MARK: Strokes (Plan 4e Rulings 5–7)

    /// A press on row `row` of lane `lane` at tick `at` (`snapped`: on the
    /// shared edit grid; `grab`: a hit's least width in ticks; `velocity`:
    /// the row's velocity lane under the pointer; `erase`: the eraser).
    public func strokeBegan(
        lane: Int, row: Int, at: Int64, snapped: Int64, grab: Int64, velocity: UInt8?, erase: Bool
    ) {
        // A stroke whose mouse-up was lost must not hold its gesture open.
        if stroke != nil { strokeCancelled() }
        guard let seq, let r = self.row(lane, row) else { return }
        let laneID = lanes[lane].lane
        selectedRow = DrumRowKey(lane: laneID, row: r.id)
        guard
            let s = try? drumStrokeBegin(
                lane: UInt32(lane), row: UInt32(row), hits: r.hits, at: at, snapped: snapped, grab: grab,
                velocity: velocity, eraseTool: erase, settings: settings.row)
        else { return }
        stroke = s
        strokeTarget = (seq, laneID, r.id, r.hits)
        strokeOpen = false
    }

    /// The pointer moved past the view's dead zone.
    public func strokeMoved(at: Int64, grab: Int64, velocity: UInt8?) {
        guard let next = extended(at: at, grab: grab, velocity: velocity) else { return }
        stroke = next
        if !strokeOpen {
            guard open(next) else {
                stroke = nil
                return
            }
        }
        editor.updateGesture(command(next))
    }

    /// The pointer went up: a click toggles one hit, a drag ends its run.
    public func strokeEnded(at: Int64, grab: Int64, velocity: UInt8?) {
        guard let s = extended(at: at, grab: grab, velocity: velocity) else { return }
        stroke = nil
        let final = command(s)
        if !strokeOpen {
            guard final != nil, open(s) else { return }
        }
        strokeOpen = false
        if let final {
            editor.endGesture(final)
        } else {
            editor.cancelGesture()
        }
    }

    /// Escape, or the view went away: no undo step.
    public func strokeCancelled() {
        if stroke != nil, strokeOpen { editor.cancelGesture() }
        stroke = nil
        strokeOpen = false
    }

    private func extended(at: Int64, grab: Int64, velocity: UInt8?) -> DrumStrokeRow? {
        guard let s = stroke, let t = strokeTarget else { return nil }
        return try? drumStrokeExtend(
            stroke: s, hits: t.hits, at: at, grab: grab, velocity: velocity, settings: settings.row)
    }

    func command(_ s: DrumStrokeRow) -> EditCommand? {
        guard let t = strokeTarget else { return nil }
        return (try? drumStrokeCommand(stroke: s, seq: t.seq, lane: t.lane, row: t.row, settings: settings.row))
            ?? nil
    }

    private func open(_ s: DrumStrokeRow) -> Bool {
        guard let t = strokeTarget else { return false }
        let pending = editor.gesture != nil
        guard
            editor.beginGesture(
                s.erase ? Self.eraseLabel : Self.paintLabel, lanes: [LaneKey(t.seq, t.lane)], origin: .other)
        else {
            if pending { editor.status = EditorModel.gestureOpenMessage }
            return false
        }
        strokeOpen = true
        return true
    }

    // MARK: Rows (spec §4e)

    private func drum(_ e: DrumEdit) -> Bool { editor.edit(.drum(edit: e)) }

    /// A row for the next free kit note in the selected row's lane (else
    /// the first drum lane); it becomes the selected row.
    @discardableResult
    public func addRow() -> Bool {
        guard let seq, let lane = currentLane else {
            editor.status = "There is no drum lane to add a row to."
            return false
        }
        // A `Vec<u8>` crosses the FFI as `Data`.
        guard let new = (try? drumNextRow(used: Data(lane.rows.map(\.note)))) ?? nil else {
            editor.status = "Every note already has a row in \(lane.name)."
            return false
        }
        guard drum(.addRow(seq: seq, lane: lane.lane, name: new.name, note: new.note, dest: nil)) else { return false }
        let rows = lanes.first { $0.lane == lane.lane }?.rows ?? []
        if let r = rows.first(where: { r in r.note == new.note && r.dest == nil }) {
            selectedRow = DrumRowKey(lane: lane.lane, row: r.id)
        }
        return true
    }

    @discardableResult
    public func removeSelectedRow() -> Bool {
        guard let seq, let s = selected else {
            editor.status = Self.noRowMessage
            return false
        }
        return drum(.removeRow(seq: seq, lane: s.lane.lane, row: s.row.id))
    }

    @discardableResult
    public func splitSelectedRow() -> Bool {
        guard let seq, let s = selected else {
            editor.status = Self.noRowMessage
            return false
        }
        return drum(.splitRow(seq: seq, lane: s.lane.lane, row: s.row.id))
    }

    /// Shows or hides the selected row's velocity lane (`nil` toggles).
    @discardableResult
    public func showVelocity(_ on: Bool?) -> Bool {
        guard let seq, let s = selected else {
            editor.status = Self.noRowMessage
            return false
        }
        let show = on ?? !s.row.showVelocity
        guard show != s.row.showVelocity else { return true }
        return drum(.showVelocity(seq: seq, lane: s.lane.lane, row: s.row.id, show: show))
    }

    @discardableResult
    public func rename(_ key: DrumRowKey, to name: String) -> Bool {
        let name = name.trimmingCharacters(in: .whitespacesAndNewlines)
        guard let seq, let i = indexes(key), !name.isEmpty, name != lanes[i.lane].rows[i.row].name else { return false }
        return drum(.renameRow(seq: seq, lane: key.lane, row: key.row, name: name))
    }

    /// Row `row` of lane `lane` dragged by its header and released at
    /// height `y` (content coordinates): reordered, moved to another drum
    /// lane or split into a new lane (Rust decides which).
    @discardableResult
    public func drop(lane: Int, row: Int, atY y: Double) -> Bool {
        guard let seq, let r = self.row(lane, row),
            let cmd = (try? drumRowDrop(
                layout: layout, lanes: lanes.map(\.lane), seq: seq, laneIndex: UInt32(lane), row: r.id,
                rowIndex: UInt32(row), y: y)) ?? nil
        else { return false }
        return editor.edit(cmd)
    }

    // MARK: Lanes (play quantize and nudge)

    public static let quantizeChoices: [QuantizeChoiceRow] = (try? drumQuantizeChoices()) ?? []

    public func quantizeTitle(_ lane: DrumLaneRow) -> String {
        (try? drumQuantizeTitle(quantize: lane.playQuantize)) ?? "Custom"
    }

    @discardableResult
    public func setQuantize(lane: UInt64, _ q: QuantizeRow?) -> Bool {
        guard let seq else { return false }
        return editor.edit(.lane(edit: .setPlayQuantize(seq: seq, lane: lane, quantize: q)))
    }

    /// Moves a lane's nudge (play shift) by `steps` steps of 120 ticks (a 128th of a quarter note).
    @discardableResult
    public func nudge(lane: UInt64, steps: Int64) -> Bool {
        guard let seq, let l = lanes.first(where: { $0.lane == lane }) else { return false }
        let shift = (try? drumNudged(shift: l.playShift, steps: steps)) ?? l.playShift
        guard shift != l.playShift else { return true }
        return editor.edit(.lane(edit: .setPlayShift(seq: seq, lane: lane, shift: shift)))
    }
}

extension EditorModel {
    /// Makes lane `lane` of `seq` a drum lane (rows for its notes) or a
    /// normal lane again (Drum > Drum Lane, the lane header menu).
    @discardableResult
    public func markDrumLane(seq: UInt64, lane: UInt64, on: Bool) -> Bool {
        edit(.drum(edit: .markLane(seq: seq, lane: lane, drum: on)))
    }
}
