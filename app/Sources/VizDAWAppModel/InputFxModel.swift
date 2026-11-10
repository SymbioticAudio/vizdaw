import Foundation
import Observation
import VizDAWCore

/// The five input effects, in the Add menu's order.
public enum InputFxName: String, CaseIterable, Sendable {
    case arpeggiator, transpose, velocity, chord, inputQuantize
}

/// The input effects editor's model (plan 5g-1 Task 8): the chain of one
/// MIDI or instrument track, shown in the strip's MIDI FX section. Every
/// change is one undoable lane edit on the selected sequence; a slider
/// drags a draft and edits once, at its end.
@Observable @MainActor public final class InputFxModel {
    /// Its document, as `LaneInputs` holds it (no retain cycle).
    @ObservationIgnored public weak var document: DocumentModel?

    public init(document: DocumentModel) { self.document = document }

    /// The lane whose chain the input effects editor shows; nil when closed.
    public private(set) var lane: UInt64?
    /// The selected row.
    public var selected: Int? {
        // A draft belongs to the row it was grabbed on.
        didSet { if selected != oldValue { draft = nil } }
    }
    /// The selected effect while a slider drags (committed at its end).
    public private(set) var draft: InputFxKindRow?

    /// The most effects one chain holds.
    static let maxEffects = 8

    /// Shows `lane`'s chain with nothing selected.
    public func open(lane: UInt64) {
        self.lane = lane
        selected = nil
        draft = nil
    }

    /// Closes the editor, dropping any draft.
    public func close() {
        lane = nil
        selected = nil
        draft = nil
    }

    private var laneRow: LaneRow? {
        guard let document, let lane else { return nil }
        return document.lanes.first { $0.id == lane }
    }

    /// The open lane's chain, the draft shown in place of the selected row;
    /// `[]` when the lane is no longer in `document.lanes` (an Undo removed it).
    public var chain: [InputFxRow] {
        var rows = laneRow?.inputFx ?? []
        if let draft, let i = selected, rows.indices.contains(i) { rows[i].kind = draft }
        return rows
    }

    /// The strip's MIDI FX section exists on MIDI and instrument tracks, not
    /// audio ones (Ruling 16).
    nonisolated public static func showsSection(_ lane: LaneRow) -> Bool {
        switch lane.kind {
        case .midi, .instrument: true
        case .audio: false
        }
    }

    /// `showsSection`, and the chain has an effect that is not bypassed.
    nonisolated public static func isLit(_ lane: LaneRow) -> Bool {
        showsSection(lane) && lane.inputFx.contains { !$0.bypass }
    }

    /// Ruling 14's defaults.
    nonisolated public static func defaultKind(_ name: InputFxName) -> InputFxKindRow {
        switch name {
        case .arpeggiator:
            .arpeggiator(mode: .up, division: 16, feel: .straight, octaves: 1, gate: 50, velocity: nil, latch: false)
        case .transpose: .transpose(semitones: 12)
        case .velocity: .velocity(scale: 100, offset: 0, min: 1, max: 127, fixed: nil)
        case .chord: .chord(intervals: [4, 7])
        case .inputQuantize: .inputQuantize(grid: 3840, strength: 100)
        }
    }

    /// What a row is called.
    nonisolated public static func title(_ kind: InputFxKindRow) -> String {
        switch kind {
        case .arpeggiator: "Arpeggiator"
        case .transpose: "Transpose"
        case .velocity: "Velocity"
        case .chord: "Chord"
        case .inputQuantize: "Input Quantize"
        }
    }

    /// The Input Quantize grids by name ("1/16", "1/8T", "1/4.", ...), in
    /// `input_quantize_grids` order (Ruling 12): for 1/2, 1/4, 1/8, 1/16 and
    /// 1/32, the straight, dotted and triplet value. A quarter is 15360 ticks.
    nonisolated public static let quantizeGrids: [(title: String, ticks: Int64)] = {
        let quarter: Int64 = 15360
        let bases: [(Int, Int64)] = [(2, quarter * 2), (4, quarter), (8, quarter / 2), (16, quarter / 4), (32, quarter / 8)]
        return bases.flatMap { division, ticks in
            [("1/\(division)", ticks), ("1/\(division).", ticks * 3 / 2), ("1/\(division)T", ticks * 2 / 3)]
        }
    }()

    nonisolated private static func signed(_ n: Int) -> String {
        n < 0 ? "\u{2212}\(-n)" : "+\(n)"
    }

    nonisolated private static func gridTitle(_ ticks: Int64) -> String {
        quantizeGrids.first { $0.ticks == ticks }?.title ?? "\(ticks) ticks"
    }

    nonisolated private static func modeTitle(_ mode: ArpModeRow) -> String {
        switch mode {
        case .up: "Up"
        case .down: "Down"
        case .upDown: "Up/Down"
        case .downUp: "Down/Up"
        case .asPlayed: "As Played"
        case .random: "Random"
        case .chord: "Chord"
        }
    }

    /// A row's settings in one line.
    nonisolated public static func summary(_ kind: InputFxKindRow) -> String {
        switch kind {
        case let .arpeggiator(mode, division, feel, octaves, gate, _, _):
            let suffix = switch feel {
            case .straight: ""
            case .dotted: "."
            case .triplet: "T"
            }
            return "\(modeTitle(mode)) 1/\(division)\(suffix) · \(octaves) oct · \(gate)%"
        case let .transpose(semitones):
            return signed(Int(semitones))
        case let .velocity(scale, offset, min, max, fixed):
            if let fixed { return "= \(fixed)" }
            let range = (min != 1 || max != 127) ? " \(min)\u{2013}\(max)" : ""
            return "\u{00D7}\(scale)% \(signed(Int(offset)))\(range)"
        case let .chord(intervals):
            return intervals.map { signed(Int($0)) }.joined(separator: " ")
        case let .inputQuantize(grid, strength):
            return "\(gridTitle(grid)) · \(strength)%"
        }
    }

    /// Adds `name`'s default at the end and selects it. Refused past 8
    /// effects, and a second Input Quantize: the status line says why.
    public func add(_ name: InputFxName) {
        guard let document, laneRow != nil else { return }
        let rows = committed
        if rows.count >= Self.maxEffects {
            document.editor.status = "A chain holds at most 8 effects."
            return
        }
        if name == .inputQuantize, rows.contains(where: { if case .inputQuantize = $0.kind { true } else { false } }) {
            document.editor.status = "A chain holds one Input Quantize."
            return
        }
        if send(rows + [InputFxRow(kind: Self.defaultKind(name), bypass: false)]) {
            selected = rows.count
        }
    }

    /// Removes row `i`; the selection follows.
    public func remove(at i: Int) {
        var rows = committed
        guard rows.indices.contains(i) else { return }
        rows.remove(at: i)
        guard send(rows) else { return }
        draft = nil
        selected = selected.flatMap { s in s == i ? nil : (s > i ? s - 1 : s) }
    }

    /// Reorders as `Array.move(fromOffsets:toOffset:)` does; the selection
    /// follows its row.
    public func move(from: IndexSet, to: Int) {
        var rows = committed
        guard !from.isEmpty, from.allSatisfy(rows.indices.contains), (0...rows.count).contains(to) else { return }
        let order = Array(rows.indices)
        var moved = order.filter { !from.contains($0) }
        let at = to - from.filter { $0 < to }.count
        moved.insert(contentsOf: order.filter { from.contains($0) }, at: at)
        rows = moved.map { rows[$0] }
        guard moved != order, send(rows) else { return }
        draft = nil
        selected = selected.flatMap { moved.firstIndex(of: $0) }
    }

    /// Bypasses or restores row `i`.
    public func setBypass(_ i: Int, _ on: Bool) {
        var rows = committed
        guard rows.indices.contains(i), rows[i].bypass != on else { return }
        rows[i].bypass = on
        send(rows)
    }

    /// Replaces row `i`'s settings (a button or stepper: one edit).
    public func update(_ i: Int, _ kind: InputFxKindRow) {
        var rows = committed
        guard rows.indices.contains(i), rows[i].kind != kind else { return }
        rows[i].kind = kind
        send(rows)
    }

    /// A slider grabbed: the selected row's settings become the draft.
    public func beginDraft() {
        guard let i = selected, let row = committed[safe: i] else { return }
        draft = row.kind
    }

    /// While the slider moves: the draft changes, no edit.
    public func setDraft(_ kind: InputFxKindRow) {
        guard draft != nil else { return }
        draft = kind
    }

    /// Released: one edit if the draft differs from the row.
    public func commitDraft() {
        guard let kind = draft else { return }
        defer { draft = nil }
        guard let i = selected else { return }
        update(i, kind)
    }

    /// The open lane's chain as the document holds it (no draft).
    private var committed: [InputFxRow] { laneRow?.inputFx ?? [] }

    /// One lane edit on the selected sequence; false when refused or the
    /// lane is gone.
    @discardableResult
    private func send(_ rows: [InputFxRow]) -> Bool {
        guard let document, let lane, laneRow != nil, let seq = document.selectedSeq else { return false }
        return document.editor.edit(.lane(edit: .setInputFx(seq: seq, lane: lane, chain: rows)))
    }
}

private extension Array {
    subscript(safe i: Int) -> Element? { indices.contains(i) ? self[i] : nil }
}
