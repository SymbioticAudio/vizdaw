import Foundation
import VizDAWCore

/// The detail pane and torn-off editor windows (spec §3.7).
extension EditorModel {
    /// What a detail editor opens on: the selection's single lane (with its
    /// range when there is one), else the shown sequence.
    public func detailTarget(shown seq: UInt64?) -> EditorTarget? {
        if selection.lanes.count == 1, let l = selection.lanes.first {
            if let r = selection.timeRange {
                return .block(seq: l.seq, lane: l.lane, start: r.start, end: r.end)
            }
            return .lane(seq: l.seq, lane: l.lane)
        }
        return seq.map { .sequence($0) }
    }

    /// ⌘1–⌘4: the detail pane shows `kind`, on its current target or on
    /// the selection.
    public func showDetail(_ kind: DetailEditorKind, shown seq: UInt64?) {
        if let d = layout.detail {
            openDetail(d.target, kind: kind)
        } else if let t = detailTarget(shown: seq) {
            openDetail(t, kind: kind)
        }
    }

    public func closeDetail() {
        updateLayout { $0.detail = nil }
    }

    /// A new torn-off window with `kind` on the detail pane's target (or
    /// the selection's); nil when there is nothing to show.
    @discardableResult
    public func tearOff(_ kind: DetailEditorKind, shown seq: UInt64?) -> TornWindow? {
        guard let target = layout.detail?.target ?? detailTarget(shown: seq) else { return nil }
        let w = TornWindow(
            id: UUID(),
            detail: DetailState(
                kind: kind, target: target, lanes: layout.detail?.lanes, stripRows: layout.detail?.stripRows),
            frame: nil)
        updateLayout {
            $0.windows.append(w)
            $0.lastKind = kind
        }
        return w
    }

    /// ⌥⌘1–⌥⌘4 while torn-off window `id` is key: a new window with `kind`
    /// on that window's target (Plan 4b Ruling 18).
    @discardableResult
    public func tearOff(_ kind: DetailEditorKind, from id: UUID) -> TornWindow? {
        guard var detail = layout.windows.first(where: { $0.id == id })?.detail else { return nil }
        detail.kind = kind
        let w = TornWindow(id: UUID(), detail: detail, frame: nil)
        updateLayout {
            $0.windows.append(w)
            $0.lastKind = kind
        }
        return w
    }

    public func closeWindow(_ id: UUID) {
        updateLayout { $0.windows.removeAll { $0.id == id } }
    }

    public func setWindowFrame(_ id: UUID, _ frame: [Double]) {
        guard let i = layout.windows.firstIndex(where: { $0.id == id }), layout.windows[i].frame != frame
        else { return }
        updateLayout { $0.windows[i].frame = frame }
    }

    public func setWindowKind(_ id: UUID, _ kind: DetailEditorKind) {
        guard let i = layout.windows.firstIndex(where: { $0.id == id }) else { return }
        updateLayout {
            $0.windows[i].detail.kind = kind
            $0.lastKind = kind
        }
    }

    /// Drops the detail target and windows whose sequence or lane is gone.
    func pruneLayout(_ s: DocSnapshot) {
        let seqs = Set(s.sequences.map(\.id))
        let lanes = Set(s.laneRevs.map { LaneKey($0.seq, $0.lane) })
        func gone(_ t: EditorTarget) -> Bool {
            if !seqs.contains(t.seq) { return true }
            guard let l = t.lane else { return false }
            return !lanes.contains(LaneKey(t.seq, l))
        }
        /// Extra lanes that are gone leave a piano roll silently (Plan 4b
        /// Review Focus 5).
        func cleaned(_ d: DetailState) -> DetailState {
            guard let extra = d.lanes else { return d }
            var c = d
            let kept = extra.filter { lanes.contains(LaneKey(d.target.seq, $0)) }
            c.lanes = kept.isEmpty ? nil : kept
            return c
        }
        let detailGone = layout.detail.map { gone($0.target) } ?? false
        let windowsGone = layout.windows.contains { gone($0.detail.target) }
        let lanesGone =
            (layout.detail.map { cleaned($0) != $0 } ?? false)
            || layout.windows.contains { cleaned($0.detail) != $0.detail }
        guard detailGone || windowsGone || lanesGone else { return }
        updateLayout { l in
            if detailGone { l.detail = nil }
            l.windows.removeAll { gone($0.detail.target) }
            l.detail = l.detail.map(cleaned)
            for i in l.windows.indices { l.windows[i].detail = cleaned(l.windows[i].detail) }
        }
    }

    /// How many events a target covers (the 4a placeholder's summary).
    public func eventCount(_ t: EditorTarget) -> Int? {
        switch t {
        case let .block(seq, lane, start, end):
            let stop = max(end, start + 1)
            return laneEvents(LaneKey(seq, lane))?.events.filter { $0.tick >= start && $0.tick < stop }.count
        case let .lane(seq, lane):
            return laneEvents(LaneKey(seq, lane))?.events.count
        case .sequence:
            return nil
        }
    }

    /// "Song › Chords, bars 1–2".
    public func title(_ t: EditorTarget, sequences: [SeqRow]) -> String {
        let seqName = sequences.first { $0.id == t.seq }?.name ?? "Sequence"
        guard let laneID = t.lane else { return seqName }
        let laneName = ((try? session.lanes(seq: t.seq)) ?? []).first { $0.id == laneID }?.name ?? "Lane"
        guard case let .block(_, _, start, end) = t else { return "\(seqName) › \(laneName)" }
        let meter = info(t.seq)?.meter ?? []
        let first = MeterMath.bar(at: start, meter).number
        let last = MeterMath.bar(at: max(end - 1, start), meter).number
        let bars = first == last ? "bar \(first)" : "bars \(first)–\(last)"
        return "\(seqName) › \(laneName), \(bars)"
    }
}
