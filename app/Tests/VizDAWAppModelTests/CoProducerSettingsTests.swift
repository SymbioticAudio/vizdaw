import Foundation
import Testing
@testable import VizDAWAppModel
import VizDAWCore

/// Records what Settings › Co-Producer asked of the app; never opens a
/// socket or writes a file.
@MainActor
final class FakeCoProducerSink: CoProducerSink {
    var calls: [String] = []
    /// What `start` answers: nil listens.
    var startProblem: String?
    func start(path: String) -> String? {
        calls.append("start \(path)")
        return startProblem
    }
    func stop() { calls.append("stop") }
    func markOff(_ off: Bool) { calls.append("markOff \(off)") }
}

@MainActor
@Suite struct CoProducerSettingsTests {
    /// A support folder that is never created: the fake sink writes nothing.
    let support = URL(fileURLWithPath: "/tmp/vizdaw-co-producer-settings-test")

    @Test func offByDefaultAndPersists() {
        let defaults = MemoryDefaults()
        let sink = FakeCoProducerSink()
        let s = CoProducerSettings(defaults: defaults, supportDir: support, sink: sink)
        #expect(!s.on)
        #expect(sink.calls == ["markOff true"])
        #expect(s.status == "Co-Producer is off: Claude cannot reach VizDAW.")

        s.setOn(true)
        #expect(s.on)
        #expect(defaults.object(forKey: CoProducerSettings.key) as? Bool == true)
        #expect(sink.calls.suffix(2) == ["markOff false", "start /tmp/vizdaw-co-producer-settings-test/co-producer.sock"])
        #expect(s.status == "Co-Producer is on: Claude can reach VizDAW through vizdaw-mcp.")
        #expect(s.problem == nil)

        let sink2 = FakeCoProducerSink()
        let again = CoProducerSettings(defaults: defaults, supportDir: support, sink: sink2)
        #expect(again.on)
        #expect(sink2.calls == ["markOff false", "start /tmp/vizdaw-co-producer-settings-test/co-producer.sock"])

        var ended = 0
        again.didTurnOff = { ended += 1 }
        again.setOn(false)
        #expect(!again.on)
        #expect(defaults.object(forKey: CoProducerSettings.key) as? Bool == false)
        #expect(sink2.calls.suffix(2) == ["stop", "markOff true"])
        #expect(ended == 1)
    }

    @Test func aFailedStartSaysWhyAndStaysOff() {
        let defaults = MemoryDefaults()
        let sink = FakeCoProducerSink()
        sink.startProblem = "the folder belongs to another user"
        let s = CoProducerSettings(defaults: defaults, supportDir: support, sink: sink)
        s.setOn(true)
        #expect(!s.on)
        #expect(s.problem == "Co-Producer could not start: the folder belongs to another user")
        #expect(s.status == "Co-Producer is off: Claude cannot reach VizDAW.")
        // The off marker is back, and the next launch does not try again.
        #expect(sink.calls.last == "markOff true")
        #expect(defaults.object(forKey: CoProducerSettings.key) as? Bool == false)
        // A later start that works clears the problem.
        sink.startProblem = nil
        s.setOn(true)
        #expect(s.on)
        #expect(s.problem == nil)
    }

    @Test func aDamagedSettingReadsAsOff() {
        let defaults = MemoryDefaults()
        defaults.set("yes", forKey: CoProducerSettings.key)
        let sink = FakeCoProducerSink()
        let s = CoProducerSettings(defaults: defaults, supportDir: support, sink: sink)
        #expect(!s.on)
        #expect(!sink.calls.contains { $0.hasPrefix("start") })
    }

    /// Task 5 carry: the socket folder is the one vizdaw-mcp derives, or
    /// screenshots are refused.
    @Test func theSupportFolderIsVizdawMcps() {
        #expect(CoProducerSettings.supportPath(environment: ["HOME": "/Users/x"])
            == "/Users/x/Library/Application Support/VizDAW")
        #expect(CoProducerSettings.supportPath(environment: ["HOME": "/Users/x", "VIZDAW_SUPPORT_DIR": "/tmp/s"])
            == "/tmp/s")
        // vizdaw-mcp refuses both of these, so the app has no folder to match.
        #expect(CoProducerSettings.supportPath(environment: ["HOME": "/Users/x", "VIZDAW_SUPPORT_DIR": "rel"]) == nil)
        #expect(CoProducerSettings.supportPath(environment: ["HOME": "rel"]) == nil)
        #expect(CoProducerSettings.supportPath(environment: [:]) == nil)
    }

    @Test func theCopiesLineCountsOpenSavedProjects() throws {
        let a = demoFake(), b = demoFake(), c = demoFake()
        a.savedPath = "/tmp/a.vizdaw"
        a.copies = CopiesSizeRow(count: 2, bytes: 1_500_000)
        b.savedPath = "/tmp/b.vizdaw"
        b.copies = CopiesSizeRow(count: 1, bytes: 250_000)
        // Untitled: not a saved project.
        c.copies = CopiesSizeRow(count: 5, bytes: 9_000_000)
        var queue = [a, b, c]
        let store = DocumentStore(makeEmpty: { FakeSession([]) }, makeDemo: { queue.removeFirst() }, open: { _ in demoFake() })
        for _ in 0..<3 { _ = try #require(store.newDemo()) }
        let s = CoProducerSettings(defaults: MemoryDefaults(), supportDir: support, sink: FakeCoProducerSink())
        s.readCopies(store)
        #expect(s.copiesLine == "Saved copies in open projects: 3 (1.8 MB)")
    }
}
