import Foundation
import Observation
import VizDAWCore

/// Where a piano roll lives (Plan 4b Ruling 22).
public enum DetailSlot: Hashable, Sendable {
    case pane
    case window(UUID)
}

/// One piano roll (spec §4b): which lanes it shows, which lane new notes go
/// to, lane colours and the note it sounds. The detail pane and every
/// torn-off window have their own; document, selection and settings are
/// shared. Owned by its `DocumentModel` (Plan 4b Ruling 22).
@Observable
@MainActor
public final class PianoRollModel {
    /// Weak: closing a document while its roll is on screen must not trap.
    /// Without it `read` yields nil and `write` does nothing.
    public weak var document: DocumentModel?
    /// Held directly (it never refers back to a roll, so no cycle).
    public let editor: EditorModel
    @ObservationIgnored private let read: @MainActor () -> DetailState?
    @ObservationIgnored let write: @MainActor (DetailState) -> Void
    /// The lane of the last pressed or selected note (Plan 4b Ruling 9).
    public var chosenLane: LaneKey?
    /// The note sounding while a note or key is pressed.
    public private(set) var sounding: AuditionNote?

    public init(
        document: DocumentModel, read: @escaping @MainActor () -> DetailState?,
        write: @escaping @MainActor (DetailState) -> Void
    ) {
        self.document = document
        self.editor = document.editor
        self.read = read
        self.write = write
    }

    /// The piano roll of the detail pane.
    public static func pane(_ document: DocumentModel) -> PianoRollModel {
        PianoRollModel(
            document: document,
            read: { [weak document] in document?.editor.layout.detail },
            write: { [weak document] d in document?.editor.updateLayout { $0.detail = d } })
    }

    /// The piano roll of torn-off window `id`.
    public static func window(_ id: UUID, of document: DocumentModel) -> PianoRollModel {
        PianoRollModel(
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

    /// Every lane of the target's sequence.
    public var sequenceLanes: [LaneRow] { seq.map { editor.laneRows($0) } ?? [] }

    /// The lanes shown, in sequence order (Plan 4b Ruling 9).
    public var shownLanes: [LaneKey] {
        guard let t = target else { return [] }
        let rows = editor.laneRows(t.seq)
        let wanted: Set<UInt64>
        if let lane = t.lane {
            wanted = Set([lane] + (state?.lanes ?? []))
        } else if let exact = state?.lanes {
            wanted = Set(exact)
        } else {
            wanted = Set(rows.filter { $0.midiDest != nil || $0.midiEventCount > 0 || $0.regionCount > 0 }.map(\.id))
        }
        return rows.filter { wanted.contains($0.id) }.map { LaneKey(t.seq, $0.id) }
    }

    public func isShown(_ lane: UInt64) -> Bool { shownLanes.contains { $0.lane == lane } }

    /// Shows or hides `lane`; the target's own lane always shows.
    public func toggleShown(_ lane: UInt64) {
        guard var s = state, s.target.lane != lane else { return }
        var set = Set(s.target.lane == nil ? shownLanes.map(\.lane) : (s.lanes ?? []))
        if set.contains(lane) { set.remove(lane) } else { set.insert(lane) }
        let ordered = sequenceLanes.map(\.id).filter(set.contains)
        s.lanes = ordered.isEmpty && s.target.lane != nil ? nil : ordered
        write(s)
    }

    /// The lane the pencil inserts into.
    public var editLane: LaneKey? {
        let shown = shownLanes
        if let c = chosenLane, shown.contains(c) { return c }
        if let t = target, let l = t.lane { return LaneKey(t.seq, l) }
        return shown.first
    }

    /// The lane's palette index: its position in its sequence.
    public func colorIndex(_ key: LaneKey) -> Int {
        editor.laneRows(key.seq).firstIndex { $0.id == key.lane } ?? 0
    }

    public func row(_ key: LaneKey) -> LaneRow? {
        editor.laneRows(key.seq).first { $0.id == key.lane }
    }

    public func notes(_ key: LaneKey) -> [EventRow] {
        editor.laneEvents(key)?.events.filter { $0.kind == .note } ?? []
    }

    public func regions(_ key: LaneKey) -> [EventRow] {
        editor.laneEvents(key)?.events.filter { $0.kind == .audioRegion } ?? []
    }

    /// A block target's range (Plan 4b Ruling 10).
    public var focusRange: TickRange? {
        guard case let .block(_, _, start, end)? = target else { return nil }
        return TickRange(start: start, end: max(end, start + 1))
    }

    /// Where a note sounds: its own destination, else its lane's.
    public func dest(of n: EventRow, in key: LaneKey) -> UInt64? {
        n.dest ?? row(key)?.midiDest
    }

    // MARK: Sound (Plan 4b Ruling 6)

    /// Sounds `pitch` (ending the previous note); nil `dest` is silence.
    public func sound(_ pitch: UInt8, velocity: UInt16, dest: UInt64?) {
        let next = dest.map { AuditionNote(dest: $0, pitch: pitch) }
        guard next != sounding else { return }
        silence()
        guard let next else { return }
        editor.auditionOn(next, velocity: velocity)
        sounding = next
    }

    public func silence() {
        guard let s = sounding else { return }
        sounding = nil
        editor.auditionOff(s)
    }

    // MARK: Selection

    /// Selects `notes` (with their lanes); `adding` keeps the selection.
    public func select(_ notes: [(lane: LaneKey, id: UInt64)], adding: Bool) {
        var s = adding ? editor.selection : SelectionRow(timeRange: nil, lanes: [], events: [])
        for n in notes {
            if !s.events.contains(n.id) { s.events.append(n.id) }
            if !s.lanes.contains(n.lane.ref) { s.lanes.append(n.lane.ref) }
        }
        editor.setSelection(s)
        if let last = notes.last { chosenLane = last.lane }
    }

    /// Shift-click: adds or removes one note.
    public func toggle(_ id: UInt64, in lane: LaneKey) {
        var s = editor.selection
        if let i = s.events.firstIndex(of: id) {
            s.events.remove(at: i)
        } else {
            s.events.append(id)
            if !s.lanes.contains(lane.ref) { s.lanes.append(lane.ref) }
            chosenLane = lane
        }
        editor.setSelection(s)
    }

    /// Delete/Backspace: the selected notes of the shown lanes.
    public func deleteSelected() {
        let shown = Set(shownLanes.flatMap { notes($0) }.map(\.id))
        let ids = editor.selection.events.filter(shown.contains)
        guard !ids.isEmpty else { return }
        editor.edit(.event(edit: .delete(events: ids)))
    }
}
