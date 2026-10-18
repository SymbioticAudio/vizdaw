import Foundation
import Observation
import VizDAWCore

/// A group of the row menu's automation choices.
public struct StripMenu: Equatable, Sendable {
    public let title: String
    public let lanes: [StripLane]

    public init(title: String, lanes: [StripLane]) {
        self.title = title
        self.lanes = lanes
    }
}

/// A document's automation targets (Plan 4d Ruling 21).
public enum AutomationTargets {
    /// Names and menu groups: per strip its fader, pan (not master), mute,
    /// solo and sends; per plugin slot its automatable, visible parameters;
    /// one velocity scale per destination. Parts the session cannot read
    /// now (Busy) make the load throw, so the caller keeps what it had and
    /// retries; other errors read as empty.
    public static func load(_ session: any SessionProtocol) throws -> (names: StripNames, menus: [StripMenu]) {
        var n = StripNames()
        var menus: [StripMenu] = []
        let strips = try Self.read { try session.mixer().strips } ?? []
        for s in strips { n.strips[s.id] = s.name }
        for s in strips {
            n.sends[s.id] = s.sends.map { n.strips[$0.target] ?? "Bus \($0.target)" }
            var lanes: [StripLane] = [.automation(.fader(strip: s.id))]
            if s.kind != .master { lanes.append(.automation(.pan(strip: s.id))) }
            lanes += [.automation(.mute(strip: s.id)), .automation(.solo(strip: s.id))]
            lanes += s.sends.indices.map { .automation(.send(strip: s.id, index: UInt16(clamping: $0))) }
            menus.append(StripMenu(title: s.name, lanes: lanes))
            for slot in (s.instrument.map { [$0] } ?? []) + s.inserts {
                n.slots[slot.id] = slot.plugin.name
                let params = (try Self.read { try session.params(slot: slot.id) } ?? []).filter { $0.automatable && !$0.hidden }
                n.params[slot.id] = params
                if !params.isEmpty {
                    menus.append(
                        StripMenu(
                            title: "\(slot.plugin.name) · \(s.name)",
                            lanes: params.map { .automation(.param(slot: slot.id, param: $0.id)) }))
                }
            }
        }
        let dests = try Self.read { try session.destinations() } ?? []
        for d in dests { n.dests[d.id] = d.name }
        if !dests.isEmpty {
            menus.append(StripMenu(title: "Velocity Scale", lanes: dests.map { .automation(.velocityScale(dest: $0.id)) }))
        }
        return (n, menus)
    }

    /// A read: nil on an ordinary error, Busy rethrown.
    private static func read<T>(_ body: () throws -> T) throws -> T? {
        do { return try body() } catch VizError.Busy { throw VizError.Busy(message: "busy") } catch { return nil }
    }
}

/// A Strip Chart drag in progress (lane space).
public struct StripStroke: Equatable, Sendable {
    public let row: Int
    public let lane: StripLane
    public let start: StripPoint
    public var current: StripPoint
    /// Every pointer point so far (a free line's).
    public var points: [StripPoint]
    public let mode: StripMode
    /// A Compress rectangle (Modify with Compress).
    public let compress: Bool
    /// ⌥ at mouse-down: the selected events only.
    public let selectedOnly: Bool
    public let seed: UInt64
    /// Its gesture is open.
    public var open = false
}

/// One Strip Chart (spec §4d, Plan 4d Rulings 21–23): the lane and rows it
/// shows, the stroke in progress and the commands it sends. The detail
/// pane and every torn-off window have their own; the bar's settings are
/// shared (`EditorLayout.stripChart`).
@Observable
@MainActor
public final class StripChartModel {
    /// Weak: closing a document while its chart is on screen must not trap.
    public weak var document: DocumentModel?
    public let editor: EditorModel
    @ObservationIgnored private let read: @MainActor () -> DetailState?
    @ObservationIgnored let write: @MainActor (DetailState) -> Void
    /// For a sequence target: the lane shown.
    public var chosenLane: LaneKey?
    public private(set) var stroke: StripStroke?
    /// Seeds random lines (tests replace it).
    @ObservationIgnored public var nextSeed: () -> UInt64 = { UInt64.random(in: 0...UInt64.max) }
    @ObservationIgnored private var targetsCache: (version: UInt64, slots: Int, names: StripNames, menus: [StripMenu])?

    public static let maxRows = 4
    public static let eraseNotesMessage = "Erase works on controller, program and automation rows."

    public init(
        document: DocumentModel, read: @escaping @MainActor () -> DetailState?,
        write: @escaping @MainActor (DetailState) -> Void
    ) {
        self.document = document
        self.editor = document.editor
        self.read = read
        self.write = write
    }

    /// The Strip Chart of the detail pane.
    public static func pane(_ document: DocumentModel) -> StripChartModel {
        StripChartModel(
            document: document,
            read: { [weak document] in document?.editor.layout.detail },
            write: { [weak document] d in document?.editor.updateLayout { $0.detail = d } })
    }

    /// The Strip Chart of torn-off window `id`.
    public static func window(_ id: UUID, of document: DocumentModel) -> StripChartModel {
        StripChartModel(
            document: document,
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
    public var seq: UInt64? { target?.seq }

    /// The lane shown: the target's, else the chosen one, else the
    /// sequence's first.
    public var lane: LaneKey? {
        guard let t = target else { return nil }
        if let l = t.lane { return LaneKey(t.seq, l) }
        let rows = editor.laneRows(t.seq)
        if let c = chosenLane, c.seq == t.seq, rows.contains(where: { $0.id == c.lane }) { return c }
        return rows.first.map { LaneKey(t.seq, $0.id) }
    }

    public var rows: [StripLane] { state?.stripRows ?? [.velocity] }

    public func setRow(_ i: Int, to lane: StripLane) {
        updateRows { r in
            if r.indices.contains(i) { r[i] = lane }
        }
    }

    public func addRow() {
        updateRows { r in
            if r.count < Self.maxRows { r.append(.controller(7)) }
        }
    }

    public func removeRow(_ i: Int) {
        updateRows { r in
            if r.count > 1, r.indices.contains(i) { r.remove(at: i) }
        }
    }

    private func updateRows(_ change: (inout [StripLane]) -> Void) {
        guard var s = state else { return }
        var r = s.stripRows ?? [.velocity]
        change(&r)
        s.stripRows = r
        write(s)
    }

    public var settings: StripChartSettings { editor.layout.stripChart }

    public func updateSettings(_ change: (inout StripChartSettings) -> Void) {
        editor.updateLayout { change(&$0.stripChart) }
    }

    public var meter: [MeterEntryRow] { seq.flatMap { editor.info($0)?.meter } ?? [] }

    public func events() -> [EventRow] { lane.flatMap { editor.laneEvents($0)?.events } ?? [] }

    public func values(_ row: StripLane) -> [StripValue] {
        row.values(events(), tempo: seq.flatMap { editor.info($0)?.tempo } ?? [])
    }

    public var names: StripNames { targets().names }
    public var menus: [StripMenu] { targets().menus }

    private func targets() -> (names: StripNames, menus: [StripMenu]) {
        let v = editor.snapshotVersion
        let slots = document?.slotsVersion ?? 0
        if let c = targetsCache, c.version == v, c.slots == slots { return (c.names, c.menus) }
        // The mixer cannot change mid-gesture; each step bumps the version.
        if editor.gesture != nil, let c = targetsCache { return (c.names, c.menus) }
        do {
            let t = try AutomationTargets.load(editor.session)
            targetsCache = (v, slots, t.names, t.menus)
            return t
        } catch {
            // Busy: keep what we had; the next call retries.
            return targetsCache.map { ($0.names, $0.menus) } ?? (StripNames(), [])
        }
    }

    // MARK: Strokes

    /// The pointer went down on row `row` at `p` (`selectedOnly`: ⌥).
    public func strokeBegan(row: Int, at p: StripPoint, selectedOnly: Bool) {
        guard rows.indices.contains(row), lane != nil else { return }
        let s = settings
        stroke = StripStroke(
            row: row, lane: rows[row], start: p, current: p, points: [p], mode: s.mode,
            compress: s.mode == .modify && s.op == .compress, selectedOnly: selectedOnly, seed: nextSeed())
    }

    /// The pointer moved past the view's dead zone.
    public func strokeMoved(to p: StripPoint) {
        guard var s = stroke else { return }
        s.current = p
        s.points.append(p)
        if !s.open {
            guard open(&s) else {
                stroke = nil
                return
            }
        }
        stroke = s
        editor.updateGesture(command(s))
    }

    /// The pointer went up: the drag's final target; a click in Draw or
    /// Erase mode is a one-tick gesture, in Modify mode nothing.
    public func strokeEnded(at p: StripPoint) {
        guard var s = stroke else { return }
        stroke = nil
        s.current = p
        if s.points.last != p { s.points.append(p) }
        if !s.open {
            guard s.mode != .modify, open(&s) else { return }
        }
        editor.endGesture(command(s))
    }

    /// Escape, or the view went away: no undo step.
    public func strokeCancelled() {
        if stroke?.open == true { editor.cancelGesture() }
        stroke = nil
    }

    func command(_ s: StripStroke) -> EditCommand? {
        var set = settings
        set.mode = s.mode
        switch s.mode {
        case .erase:
            return StripCommands.erase(s.lane, from: s.start.tick, to: s.current.tick)
        case .modify where s.compress:
            return StripCommands.compress(
                s.lane, from: s.start, to: s.current, step: set.step, selectedOnly: s.selectedOnly)
        default:
            let line = StripLineBuilder.line(from: s.start, to: s.current, stroke: s.points, settings: set, seed: s.seed)
            return StripCommands.line(
                s.lane, op: StripCommands.op(s.lane, settings: set), line: line, selectedOnly: s.selectedOnly)
        }
    }

    /// Opens the stroke's gesture on its lanes (Plan 4d Ruling 22).
    private func open(_ s: inout StripStroke) -> Bool {
        guard let lane else { return false }
        if s.mode == .erase, s.lane.eraseRule(from: 0, to: 0) == nil {
            editor.status = Self.eraseNotesMessage
            return false
        }
        let sel = editor.selection
        let keep = s.mode == .modify && !sel.lanes.isEmpty
        let lanes = keep ? sel.lanes.map { LaneKey($0.seq, $0.lane) } : [lane]
        if !keep, sel.lanes != [lane.ref] {
            editor.setSelection(SelectionRow(timeRange: sel.timeRange, lanes: [lane.ref], events: sel.events))
        }
        var set = settings
        set.mode = s.mode
        let label =
            s.mode == .erase
            ? "Erase Events"
            : StripCommands.label(s.lane, op: s.compress ? .compress : StripCommands.op(s.lane, settings: set))
        let pending = editor.gesture != nil
        guard editor.beginGesture(label, lanes: Set(lanes), origin: .stripChart) else {
            if pending { editor.status = EditorModel.gestureOpenMessage }
            return false
        }
        s.open = true
        return true
    }
}
