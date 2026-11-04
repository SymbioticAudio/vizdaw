import Foundation
import Testing

@testable import VizDAWAppModel

/// A clock the tests set; never a sleep.
final class FakeClock {
    var t = Date(timeIntervalSince1970: 1_000)

    func advance(_ s: TimeInterval) { t = t.addingTimeInterval(s) }
}

/// The external-edit watcher's stable-file rule with temp files (spec
/// §12.3; plan 5f-3 Ruling 4, Review Focus 1).
@MainActor
@Suite struct ExternalEditWatcherTests {
    let clock = FakeClock()
    let watching = FakeWatching()
    let dir: URL
    let path: String

    init() throws {
        dir = FileManager.default.temporaryDirectory.appendingPathComponent("vizdaw-watch-\(UUID().uuidString)")
        try FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
        path = dir.appendingPathComponent("Kick edit 1.wav").path
    }

    func write(_ bytes: Int, _ fill: UInt8 = 1) throws {
        try Data(repeating: fill, count: bytes).write(to: URL(fileURLWithPath: path))
    }

    func watcher() -> ExternalEditWatcher {
        ExternalEditWatcher(path: path, probe: DiskProbe(), watching: watching, now: { [clock] in clock.t })
    }

    /// The fake clock at `s` seconds.
    func at(_ s: TimeInterval) { clock.t = Date(timeIntervalSince1970: 1_000 + s) }

    @Test func aChangeIsDueAfterOneStableSecond() throws {
        defer { try? FileManager.default.removeItem(at: dir) }
        try write(100)
        let w = watcher()
        #expect(watching.started == [path] && watching.live == 1)
        #expect(w.due() == nil, "no change yet")
        at(0)
        watching.fire(path)
        at(0.5)
        #expect(w.due() == nil)
        at(1.0)
        let s = try #require(w.due())
        #expect(s == DiskProbe().stamp(path) && s.size == 100)
        #expect(w.due() == s, "still due until it is applied")
        w.applied(s)
        at(5)
        #expect(w.due() == nil)
    }

    @Test func aSecondSaveRestartsTheSecond() throws {
        defer { try? FileManager.default.removeItem(at: dir) }
        try write(100)
        let w = watcher()
        at(0)
        watching.fire(path)
        at(0.8)
        try write(200, 2)
        watching.fire(path)
        at(1.2)
        #expect(w.due() == nil, "0.4 s after the second save")
        at(1.8)
        #expect(w.due()?.size == 200)
    }

    @Test func aFileStillGrowingIsNotDue() throws {
        defer { try? FileManager.default.removeItem(at: dir) }
        try write(100)
        let w = watcher()
        at(0)
        watching.fire(path)
        at(0.9)
        try write(150)  // no event: the editor is still writing
        at(1.0)
        #expect(w.due() == nil, "the stamp differs from the change's")
        at(1.9)
        #expect(w.due() == nil)
        at(2.0)
        #expect(w.due()?.size == 150, "a second after the tick that saw it grow")
    }

    @Test func oneChangeIsAppliedOnce() throws {
        defer { try? FileManager.default.removeItem(at: dir) }
        try write(100)
        let w = watcher()
        at(0)
        watching.fire(path)
        at(1)
        let s = try #require(w.due())
        w.applied(s)
        at(1.5)
        watching.fire(path)  // an event with nothing changed
        at(3)
        #expect(w.due() == nil)
        w.stop()
        #expect(watching.live == 0 && w.isStopped)
        watching.fire(path)
        #expect(w.due() == nil)
    }

    @Test func theProbeReadsTheFileARenamePutThere() throws {
        defer { try? FileManager.default.removeItem(at: dir) }
        try write(100)
        let w = watcher()
        let tmp = dir.appendingPathComponent(".Kick edit 1.tmp")
        try Data(repeating: 2, count: 300).write(to: tmp)
        #expect(rename(tmp.path, path) == 0)
        at(0)
        watching.fire(path)
        at(1)
        #expect(w.due()?.size == 300, "the probe reads the path, not the replaced file")
    }

    @Test func aDeletedFileWaitsForItsReturn() throws {
        defer { try? FileManager.default.removeItem(at: dir) }
        try write(100)
        let w = watcher()
        try FileManager.default.removeItem(atPath: path)
        at(0)
        watching.fire(path)
        at(5)
        #expect(w.due() == nil, "never due while it is missing")
        #expect(watching.live == 1, "still watched")
        try write(400)
        at(6)
        watching.fire(path)
        at(6.5)
        #expect(w.due() == nil)
        at(7)
        #expect(w.due()?.size == 400)
    }
}
