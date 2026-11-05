import Foundation
import VizDAWCore

/// The transport bar's ⏪ and ⏩ held to wind, and the held ⏪ ⏩ keys
/// (plan 7d Ruling 3 and Ruling 4).
///
/// - A click (a release with no wind yet) steps one bar; with ⇧ it goes
///   to the previous or next marker. A release outside the button with no
///   wind yet moves nothing.
/// - Holding winds on the surfaces' shuttle curve (`shuttleQuarters`):
///   nothing below 0.4 s, then faster the longer it is held. The button
///   calls `tick` every `tickEvery`. The hold's end lands an open
///   automation pass once (`transportHoldEnd`, controller ruling I4).
/// - A held key steps one bar per key-down and auto-repeat
///   (`transportKeyStep`, no landing); its key-up lands once.
/// - Everything waits while a text field has the keyboard (controller
///   ruling I7), and a refusal goes to the status line, never an alert.
@MainActor
public final class TransportShuttle {
    /// How often a held button winds.
    public static let tickEvery: TimeInterval = 0.1

    /// The document owns its shuttle (`DocumentModel.shuttle`).
    private weak var document: DocumentModel?

    public init(document: DocumentModel) { self.document = document }

    /// A button is held (pressed and not yet released or cancelled).
    public private(set) var holding = false

    private struct Hold {
        let forward: Bool
        /// ⇧ goes to a marker on release and never winds.
        let shift: Bool
        let start: TimeInterval
        /// Milliseconds since the press up to which it has wound.
        var lastMs: UInt64 = 0
        /// A wind went through: the release ends the hold, steps no bar.
        var wound = false
    }

    private var hold: Hold? {
        didSet { holding = hold != nil }
    }

    /// A key step went through since the last key-up.
    private var keyStepped = false

    /// ⏪ (`forward` false) or ⏩ pressed at `at` (seconds, any clock).
    public func press(forward: Bool, shift: Bool, at: TimeInterval) {
        guard let m = document else { return }
        if hold != nil { cancel() }
        guard m.transportKeysEnabled else {
            m.editor.status = TransportCommands.textFieldHasKeyboard
            return
        }
        hold = Hold(forward: forward, shift: shift, start: at)
    }

    /// The button is still held at `at`: winds by the curve since the last
    /// tick, signed, when that is above 0.
    public func tick(at: TimeInterval) {
        guard var h = hold, !h.shift, let m = document else { return }
        guard m.transportKeysEnabled else {
            // A text field took the keyboard mid-hold: the hold stops
            // where it got to.
            cancel()
            m.editor.status = TransportCommands.textFieldHasKeyboard
            return
        }
        let now = Self.ms(since: h.start, at: at)
        let q = shuttleQuarters(fromMs: h.lastMs, toMs: now)
        h.lastMs = max(h.lastMs, now)
        hold = h
        guard q > 0, q.isFinite else { return }
        do {
            _ = try m.session.transportWind(quarters: h.forward ? q : -q)
            hold?.wound = true
        } catch {
            // The refusal says why once and the hold stops; a pass the
            // earlier winds moved still lands (its own error is ignored).
            hold = nil
            say(TransportCommands.refused(error), m)
            if h.wound { land(m, report: false) }
            return
        }
        m.poll()
    }

    /// The button is released at `at`, inside it.
    public func release(at: TimeInterval) { release(at: at, inside: true) }

    /// The button is released at `at`. With no wind yet it is a click
    /// (`inside`) or nothing (outside); after winding it winds the
    /// remainder and ends the hold, wherever the release is.
    public func release(at: TimeInterval, inside: Bool) {
        guard let h = hold, let m = document else {
            hold = nil
            return
        }
        guard h.wound else {
            hold = nil
            guard inside else { return }
            let id =
                h.shift
                ? (h.forward ? "transport.next_marker" : "transport.previous_marker")
                : (h.forward ? "transport.fast_forward" : "transport.rewind")
            say(TransportCommands.run(id, m), m)
            return
        }
        tick(at: at)
        // A refused remainder already ended the hold and landed.
        guard hold != nil else { return }
        hold = nil
        land(m, report: true)
    }

    /// Ends the hold with no further wind; after winding, the open pass
    /// lands where the wind got to.
    public func cancel() {
        guard let h = hold else { return }
        hold = nil
        if h.wound, let m = document { land(m, report: true) }
    }

    /// A key-down or auto-repeat of ⏪ or ⏩: one bar, moved as a wind
    /// moves (no landing), gated as the command is; a refusal shows in
    /// the status line.
    @discardableResult
    public func keyStep(forward: Bool) -> KeyOutcome {
        guard let m = document else { return .unavailable(TransportCommands.noSequence) }
        let outcome: KeyOutcome
        if !m.transportKeysEnabled {
            outcome = .unavailable(TransportCommands.textFieldHasKeyboard)
        } else if m.selectedSeq == nil {
            outcome = .unavailable(TransportCommands.noSequence)
        } else {
            do {
                _ = try m.session.transportKeyStep(forward: forward)
                keyStepped = true
                outcome = .done
            } catch {
                outcome = TransportCommands.refused(error)
            }
            m.poll()
        }
        say(outcome, m)
        return outcome
    }

    /// The held key is up (or the window or app lost the keyboard): one
    /// landing if a key step happened since the last key-up.
    public func keyUp() {
        guard keyStepped else { return }
        keyStepped = false
        if let m = document { land(m, report: true) }
    }

    /// Milliseconds from `start` to `at`; a negative or non-finite
    /// difference is 0 (pre-flight M-10).
    private static func ms(since start: TimeInterval, at: TimeInterval) -> UInt64 {
        let d = (at - start) * 1000
        guard d.isFinite, d > 0 else { return 0 }
        // Far beyond any hold; keeps the conversion in range.
        return UInt64(min(d.rounded(), 1e15))
    }

    /// One landing (`transportHoldEnd`), then the new position.
    private func land(_ m: DocumentModel, report: Bool) {
        do {
            _ = try m.session.transportHoldEnd()
        } catch {
            if report { say(TransportCommands.refused(error), m) }
        }
        m.poll()
    }

    private func say(_ outcome: KeyOutcome, _ m: DocumentModel) {
        switch outcome {
        case .done: break
        case .unavailable(let why), .refused(let why), .busy(let why): m.editor.status = why
        }
    }
}
