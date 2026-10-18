import Foundation
import VizDAWCore

/// The transport bar's fields (parent spec §9, plan 4f Ruling 16): a key
/// focuses one; digits with separators enter bar·beat·unit; Enter
/// commits, Escape reverts.
public enum TransportField: String, CaseIterable, Sendable {
    case position, editIn, editOut, punchIn, punchOut

    /// Why a punch range was refused: it would pass the last position.
    public static let punchLimit = "The punch range must end by the last position."

    public var title: String {
        switch self {
        case .position: "Position"
        case .editIn: "Edit In"
        case .editOut: "Edit Out"
        case .punchIn: "Punch In"
        case .punchOut: "Punch Out"
        }
    }
}

extension DocumentModel {
    /// The shown sequence's meter (empty: 4/4 from bar 1).
    var shownMeter: [MeterEntryRow] { selectedSeq.flatMap { editor.info($0)?.meter } ?? [] }

    /// The tick a field holds; nil for an edit range that is not set.
    public func fieldTick(_ f: TransportField) -> Int64? {
        switch f {
        case .position: transport.tick
        case .editIn: editor.selection.timeRange?.start
        case .editOut: editor.selection.timeRange?.end
        case .punchIn: editor.layout.transport.punchIn
        case .punchOut: editor.layout.transport.punchOut
        }
    }

    /// A field's text: bar.beat.unit, empty when it holds nothing.
    public func fieldText(_ f: TransportField) -> String {
        fieldTick(f).map { Bbu.of($0, meter: shownMeter).text } ?? ""
    }

    /// The tick typed text names, read against the field's position (an
    /// empty part keeps that part); `:`, `,` and spaces separate like dots.
    public func fieldInput(_ f: TransportField, _ text: String) -> Int64? {
        let meter = shownMeter
        let typed = text.trimmingCharacters(in: .whitespaces)
            .map { ":, ".contains($0) ? "." : String($0) }.joined()
        let current = Bbu.of(fieldTick(f) ?? 0, meter: meter)
        return PositionInput.parse(typed, current: current)?.tick(meter: meter)
    }

    /// Commits typed text to a field (plan 4f Ruling 16). False, with the
    /// status line saying why, for unreadable text or an impossible range.
    @discardableResult
    public func commitField(_ f: TransportField, _ text: String) -> Bool {
        guard let t = fieldInput(f, text), abs(t) <= TransportMarks.tickLimit else {
            editor.status = "“\(text)” is not a position (bar.beat.unit)."
            return false
        }
        let bar = MeterMath.barTicks(MeterMath.bar(at: t, shownMeter).meter)
        let sel = editor.selection
        switch f {
        case .position:
            do {
                try moveTo(t)
            } catch {
                editor.report(error)
                return false
            }
            return true
        case .editIn:
            let r = sel.timeRange
            let length = r.map { $0.end - $0.start } ?? bar
            let end = r.map { $0.end > t ? $0.end : t + length } ?? t + bar
            editor.setSelection(
                SelectionRow(timeRange: TickRange(start: t, end: end), lanes: sel.lanes, events: sel.events))
        case .editOut:
            guard let r = sel.timeRange else {
                editor.status = "Set Edit In first."
                return false
            }
            guard t > r.start else {
                editor.status = "Edit Out must be after Edit In."
                return false
            }
            editor.setSelection(
                SelectionRow(timeRange: TickRange(start: r.start, end: t), lanes: sel.lanes, events: sel.events))
        case .punchIn, .punchOut:
            var marks = editor.layout.transport
            if f == .punchOut, t <= marks.punchIn {
                editor.status = "Punch Out must be after Punch In."
                return false
            }
            let range = f == .punchIn ? (t, marks.punchOut > t ? marks.punchOut : t + bar) : (marks.punchIn, t)
            // A range past the last position changes nothing (item 11).
            guard marks.setPunch(in: range.0, out: range.1) else {
                editor.status = TransportField.punchLimit
                return false
            }
            editor.updateLayout { $0.transport = marks }
        }
        return true
    }
}
