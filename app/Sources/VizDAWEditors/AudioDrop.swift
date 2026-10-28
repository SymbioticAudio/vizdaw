import AppKit
import VizDAWAppModel
import VizDAWCore

/// Dropping audio files on the editors (spec §10.2; plan 5b Ruling 8).
public enum AudioDrop {
    /// The drop tick: snapped when snap is on, never before 0.
    public static func tick(_ tick: Int64, snap: Snap, meter: [MeterEntryRow]) -> Int64 {
        max(0, snap.apply(tick, meter: meter))
    }

    /// Where files dropped on row `row` (nil: below the lanes) at `tick`
    /// go: that row's lane, else new lanes; nil without a sequence.
    public static func target(
        seq: UInt64?, lanes: [LaneKey], row: Int?, tick: Int64, snap: Snap, meter: [MeterEntryRow]
    ) -> ImportTargetRow? {
        guard let seq else { return nil }
        let lane = row.flatMap { lanes.indices.contains($0) ? lanes[$0].lane : nil }
        return ImportTargetRow(seq: seq, lane: lane, tick: Self.tick(tick, snap: snap, meter: meter))
    }

    /// The files (not folders, not web URLs) on a dragging pasteboard.
    @MainActor
    public static func paths(_ pb: NSPasteboard) -> [String] {
        let urls = pb.readObjects(forClasses: [NSURL.self], options: [.urlReadingFileURLsOnly: true]) as? [URL] ?? []
        return urls.filter { u in
            var dir: ObjCBool = false
            return FileManager.default.fileExists(atPath: u.path, isDirectory: &dir) && !dir.boolValue
        }.map(\.path)
    }
}
