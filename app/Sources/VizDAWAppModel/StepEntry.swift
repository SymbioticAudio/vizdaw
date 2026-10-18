import Foundation
import Observation
import VizDAWCore

/// Step entry for one document (editors spec §4f, plan 4f Rulings 11–15):
/// on or off, the lane it enters into and the state Rust's state machine
/// hands back. The step position is the transport's: steps locate it as
/// they go, and moving the transport moves where the next step goes.
@Observable
@MainActor
public final class StepEntryModel {
    /// The velocity of typed notes (a played note keeps its own): 100 as
    /// 16 bits.
    public static let typedVelocity: UInt16 = 100 << 9

    @ObservationIgnored weak var document: DocumentModel?
    /// The state; nil while step entry is off.
    public private(set) var state: StepStateRow?
    /// The sequence and lane steps go into.
    public private(set) var target: LaneRef?

    public var isOn: Bool { state != nil }

    /// The lane step entry starts on: the selection's first lane of the
    /// shown sequence, else its first lane.
    public static func lane(_ m: DocumentModel) -> LaneRef? {
        guard let seq = m.selectedSeq else { return nil }
        if let l = m.selectedLane { return LaneRef(seq: seq, lane: l.id) }
        return m.lanes.first.map { LaneRef(seq: seq, lane: $0.id) }
    }

    /// Turns step entry on at the transport's position.
    public func start() -> KeyOutcome {
        guard let m = document else { return .unavailable("No document.") }
        guard !m.isPlaying else { return .unavailable(Self.playingMessage) }
        guard let t = Self.lane(m) else { return .unavailable("Select a sequence with a lane first.") }
        m.poll()
        target = t
        state = stepStateNew(tick: m.transport.tick)
        return .done
    }

    public func stop() {
        state = nil
        target = nil
    }

    static let playingMessage = "Stop playing to enter steps."
    static let offMessage = "Step entry is off (Step > Step Entry)."

    /// One input; a step moves the transport to the next position.
    @discardableResult
    public func input(_ i: StepInputRow) -> KeyOutcome {
        guard let m = document, var s = state, let t = target else { return .unavailable(Self.offMessage) }
        guard !m.isPlaying else { return .unavailable(Self.playingMessage) }
        m.poll()
        s.tick = m.transport.tick
        do {
            let r = try m.session.stepEntry(seq: t.seq, lane: t.lane, state: s, input: i)
            state = r.state
            if let snap = r.snapshot {
                m.apply(snap)
                m.editor.refreshSelection()
            }
            if r.moved { try m.moveTo(r.state.tick) }
            return .done
        } catch {
            let message = AlertItem(error, title: "").message
            m.editor.status = message
            return .refused(message)
        }
    }

    /// Notes played on a MIDI source: each key down and up in order.
    public func receive(_ notes: [NoteInRow]) {
        for n in notes {
            input(n.on ? .noteOn(note: StepNoteRow(pitch: n.pitch, velocity: n.velocity)) : .noteOff(pitch: n.pitch))
        }
    }

    /// Typed pitches as one chord at the typed velocity.
    @discardableResult
    public func enter(_ pitches: [UInt8]) -> KeyOutcome {
        input(.notes(notes: pitches.map { StepNoteRow(pitch: $0, velocity: Self.typedVelocity) }))
    }

    /// The step bar's line: "Step Entry · Chords · 1/8 dotted".
    public var summary: String? {
        guard let s = state, let t = target, let m = document else { return nil }
        let lane = m.lanes.first { $0.id == t.lane }?.name ?? "Lane \(t.lane)"
        var v = Self.title(s.value)
        if s.dotted { v += " dotted" }
        if s.triplet { v += " triplet" }
        return "Step Entry · \(lane) · \(v)"
    }

    public static func title(_ v: NoteValueRow) -> String {
        switch v {
        case .whole: "1/1"
        case .half: "1/2"
        case .quarter: "1/4"
        case .eighth: "1/8"
        case .sixteenth: "1/16"
        case .thirtySecond: "1/32"
        }
    }
}

/// The Step menu and step entry's keys (plan 4f Ruling 12): one
/// implementation for the menu items, the keys and `command.run`.
@MainActor
public enum StepCommands {
    public static let entry = "step.entry"
    public static let notes = "step.notes"
    static let values: [String: NoteValueRow] = [
        "step.whole": .whole, "step.half": .half, "step.quarter": .quarter, "step.eighth": .eighth,
        "step.sixteenth": .sixteenth, "step.thirty_second": .thirtySecond,
    ]
    static let inputs: [String: StepInputRow] = [
        "step.dot": .dot, "step.triplet": .triplet, "step.tie": .tie, "step.rest": .rest,
        "step.delete_last": .deleteLast,
    ]

    public static var ids: Set<String> { Set(values.keys).union(inputs.keys).union([entry, notes]) }

    public static func enabled(_ id: String, _ m: DocumentModel) -> Bool {
        id == entry ? m.stepEntry.isOn || (m.selectedSeq != nil && !m.isPlaying) : m.stepEntry.isOn
    }

    /// A menu item's check mark: step entry, the chosen value, dot, triplet.
    public static func isOn(_ id: String, _ m: DocumentModel) -> Bool {
        guard let s = m.stepEntry.state else { return false }
        switch id {
        case entry: return true
        case "step.dot": return s.dotted
        case "step.triplet": return s.triplet
        default: return values[id] == s.value
        }
    }

    /// Runs `id` on `m`; `on` sets step entry (nil flips it); `pitches`
    /// is step.notes' text.
    @discardableResult
    public static func run(_ id: String, on: Bool? = nil, pitches: String? = nil, _ m: DocumentModel) -> KeyOutcome {
        let step = m.stepEntry
        if id == entry {
            let want = on ?? !step.isOn
            guard want != step.isOn else { return .done }
            if want { return step.start() }
            step.stop()
            return .done
        }
        guard step.isOn else { return .unavailable(StepEntryModel.offMessage) }
        if id == notes {
            do {
                return step.enter(Array(try stepParsePitches(text: pitches ?? "")))
            } catch {
                return .refused(AlertItem(error, title: "").message)
            }
        }
        if let v = values[id] { return step.input(.value(value: v)) }
        if let i = inputs[id] { return step.input(i) }
        return .unavailable("VizDAW has no handler for \(id).")
    }
}
