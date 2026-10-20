import AppKit
import Testing
import VizDAWAppModel
import VizDAWCore
@testable import VizDAWEditors

private let meter = [MeterEntryRow(tick: 0, num: 4, den: 4, sharps: 0, minor: false)]

@MainActor
@Suite struct AudioDropTests {
    @Test func aDropLandsOnTheLaneUnderItAtTheSnappedTick() {
        let lanes = [LaneKey(3, 30), LaneKey(3, 31)]
        var snap = Snap()
        #expect(
            AudioDrop.target(seq: 3, lanes: lanes, row: 1, tick: 31_104, snap: snap, meter: meter)
                == ImportTargetRow(seq: 3, lane: 31, tick: 30_720))
        // Below the lanes: new lanes.
        #expect(
            AudioDrop.target(seq: 3, lanes: lanes, row: nil, tick: 31_104, snap: snap, meter: meter)
                == ImportTargetRow(seq: 3, lane: nil, tick: 30_720))
        #expect(AudioDrop.target(seq: 3, lanes: lanes, row: 9, tick: 0, snap: snap, meter: meter)?.lane == nil)
        // Never before 0; no sequence, no target.
        #expect(AudioDrop.target(seq: 3, lanes: lanes, row: 0, tick: -900, snap: snap, meter: meter)?.tick == 0)
        #expect(AudioDrop.target(seq: nil, lanes: lanes, row: 0, tick: 0, snap: snap, meter: meter) == nil)
        snap.enabled = false
        #expect(AudioDrop.tick(31_104, snap: snap, meter: meter) == 31_104)
    }

    @Test func onlyFilesAreTaken() throws {
        let dir = FileManager.default.temporaryDirectory.appendingPathComponent("vizdaw-drop-\(UUID())")
        try FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(at: dir) }
        let file = dir.appendingPathComponent("a.wav")
        try Data("x".utf8).write(to: file)
        let pb = NSPasteboard(name: NSPasteboard.Name("vizdaw-test-\(UUID())"))
        defer { pb.releaseGlobally() }
        pb.clearContents()
        pb.writeObjects([file as NSURL, dir as NSURL, URL(string: "https://example.com/b.wav")! as NSURL])
        #expect(AudioDrop.paths(pb) == [file.path])
    }
}
