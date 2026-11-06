import Foundation
import VizDAWCore

/// A request that the piano roll showing `target` scroll to `tick`; a new
/// `serial` is a new request, so asking again scrolls again.
public struct PianoRollReveal: Equatable, Sendable {
    public let target: EditorTarget
    public let tick: Int64
    public let serial: Int
}

extension EditorModel {
    /// A pencil click in Arrange on `lane` at `tick`: a MIDI or Instrument
    /// track is selected and its piano roll opens at that bar with the
    /// pencil, to draw a note from there. Audio tracks do nothing new.
    /// Returns whether it opened.
    @discardableResult
    public func pencilOpensPianoRoll(lane: LaneKey, at tick: Int64) -> Bool {
        guard let row = laneRows(lane.seq).first(where: { $0.id == lane.lane }),
            row.kind != .audio
        else { return false }
        let meter = info(lane.seq)?.meter ?? []
        let bar = MeterMath.bar(at: max(tick, 0), meter.isEmpty ? [MeterMath.fourFour] : meter)
        tool = .pencil
        setSelection(SelectionRow(timeRange: nil, lanes: [lane.ref], events: []))
        let target = EditorTarget.lane(seq: lane.seq, lane: lane.lane)
        openDetail(target, kind: .pianoRoll)
        revealSerial += 1
        pianoRollReveal = PianoRollReveal(target: target, tick: bar.start, serial: revealSerial)
        return true
    }
}
