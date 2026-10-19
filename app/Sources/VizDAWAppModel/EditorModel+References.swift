import VizDAWCore

/// A reference's per-use parameters as the sheet edits them.
public struct ReferenceDraft: Identifiable, Equatable, Sendable {
    public let event: UInt64
    /// Use the referenced sequence's own length.
    public var useSequenceLength: Bool
    /// Per-use length in quarter notes (when not the sequence's length).
    public var lengthBeats: Int
    /// Repeat the sequence until the per-use length is filled.
    public var fill: Bool
    public var loops: Int
    public var transpose: Int

    public var id: UInt64 { event }

    public init(row: EventRow) {
        event = row.id
        useSequenceLength = row.refLength == nil
        lengthBeats = Int(max((row.refLength ?? row.duration) / MeterMath.ppq, 1))
        fill = row.refLoops == 0
        loops = Int(max(row.refLoops, 1))
        transpose = Int(row.refTranspose)
    }

    public var command: EditCommand {
        .reference(
            edit: .setParams(
                event: event,
                length: useSequenceLength ? nil : Int64(max(lengthBeats, 1)) * MeterMath.ppq,
                loopCount: fill ? 0 : UInt32(clamping: max(loops, 1)),
                transpose: Int8(clamping: transpose)))
    }
}

/// The subsequence workflow (spec §4a).
extension EditorModel {
    /// Moves the selected lanes' events in the selection's range into a new
    /// sequence and puts one reference in their place (rounded to bars).
    @discardableResult
    public func makeReferenceFromSelection(seq: UInt64) -> Bool {
        guard selection.timeRange != nil, selection.lanes.contains(where: { $0.seq == seq }) else {
            status = "Select a time range in the ruler (or a block) first."
            return false
        }
        return edit(.reference(edit: .fromSelection(seq: seq, perLane: false, roundToBars: true, loopCount: 1)))
    }

    /// A block's content becomes a reference.
    @discardableResult
    public func makeReference(from block: BlockRow, in lane: LaneKey) -> Bool {
        selectRange(TickRange(start: block.start, end: max(block.end, block.start + 1)), lanes: [lane])
        return makeReferenceFromSelection(seq: lane.seq)
    }

    public func makePrivateCopy(_ event: UInt64) {
        edit(.reference(edit: .makePrivateCopy(event: event)))
    }

    public func flatten(_ event: UInt64) {
        edit(.reference(edit: .flatten(event: event)))
    }

    public func rePoint(_ event: UInt64, to target: UInt64) {
        edit(.reference(edit: .rePoint(event: event, target: target)))
    }

    /// Opens the parameters sheet for a reference block.
    public func editReference(_ block: BlockRow, in lane: LaneKey) {
        guard let id = block.events.first,
            let row = laneEvents(lane)?.events.first(where: { $0.id == id && $0.kind == .reference })
        else { return }
        referenceDraft = ReferenceDraft(row: row)
    }

    public func commitReference() {
        guard let d = referenceDraft else { return }
        referenceDraft = nil
        edit(d.command)
    }

    /// Sequences a reference may point to instead of `current`.
    public static func rePointTargets(excluding current: UInt64?, among rows: [SeqRow]) -> [SeqRow] {
        rows.filter { $0.id != current }
    }
}
