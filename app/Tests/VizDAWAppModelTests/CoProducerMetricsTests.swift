import Foundation
import Testing
@testable import VizDAWAppModel
import VizDAWCore

/// A `MetricsRecording` that writes nothing and keeps every call, in a
/// log it may share with the model's `open` and `makePrivate` fakes.
@MainActor
final class FakeMetricsRecorder: MetricsRecording {
    var log: [String] = []
    var problemAnswer: String?
    var deleteError: VizError?

    func problem() -> String? { problemAnswer }
    func sample(clock: MetricsClockRow, category: MetricsCategoryRow?, song: MetricsSongRow?) {
        log.append("sample")
    }
    func noteUndo(clock: MetricsClockRow, song: MetricsSongRow?) { log.append("undo") }
    func noteStep(clock: MetricsClockRow, song: MetricsSongRow?, label: String) { log.append("step \(label)") }
    func noteMenu(clock: MetricsClockRow, durationMs: UInt64, chosen: String?) { log.append("menu") }
    func flush() throws { log.append("flush") }
    func close() throws { log.append("close") }
    func clear() { log.append("clear") }
    func clearAndDeleteAll() throws {
        log.append("clearAndDeleteAll")
        if let e = deleteError { throw e }
    }
}

/// A fresh temp support folder of the test's own.
func metricsSupportDir() throws -> URL {
    let dir = FileManager.default.temporaryDirectory
        .appendingPathComponent("vizdaw-metrics-\(UUID().uuidString)", isDirectory: true)
    try FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
    return dir
}

/// Settings › Co-Producer › Count where my time goes (plan 8f Ruling 1).
@MainActor
@Suite struct CoProducerMetricsTests {
    let recorder = FakeMetricsRecorder()
    /// `makePrivate` and `open` calls, in order.
    final class Calls { var log: [String] = [] }
    let calls = Calls()

    func model(
        _ defaults: UserDefaults, _ dir: URL, environment: [String: String] = [:], privateProblem: String? = nil,
        openError: VizError? = nil
    ) -> CoProducerMetrics {
        let calls = calls
        let recorder = recorder
        return CoProducerMetrics(
            defaults: defaults, supportDir: dir, environment: environment,
            makePrivate: { url in
                calls.log.append("private \(url.path)")
                return privateProblem
            },
            open: { url, block, sample in
                calls.log.append("open \(url.path) \(block) \(sample)")
                if let e = openError { throw e }
                return recorder
            })
    }

    var metricsDir: (URL) -> URL { { $0.appendingPathComponent("Metrics") } }

    @Test func nothingIsCountedWhileOff() throws {
        let dir = try metricsSupportDir()
        let defaults = MemoryDefaults()
        let m = model(defaults, dir)
        #expect(!m.on)
        #expect(m.recorder == nil)
        #expect(calls.log.isEmpty)

        defaults.set("true", forKey: CoProducerMetrics.key)
        let fromString = model(defaults, dir)
        #expect(!fromString.on)
        #expect(calls.log.isEmpty)

        // Co-Producer's own switch is a different one.
        let co = CoProducerSettings(defaults: defaults, supportDir: dir, sink: FakeCoProducerSink())
        co.setOn(true)
        #expect(co.on)
        let after = model(defaults, dir)
        #expect(!after.on)
        #expect(calls.log.isEmpty)
        #expect(!FileManager.default.fileExists(atPath: metricsDir(dir).path))
    }

    @Test func turningOnOpensAndPersists() throws {
        let dir = try metricsSupportDir()
        let defaults = MemoryDefaults()
        let m = model(defaults, dir)
        m.setOn(true)
        #expect(m.on)
        #expect(m.recorder != nil)
        #expect(calls.log == ["private \(dir.path)", "open \(dir.path) 60000 5000"])
        #expect(defaults.object(forKey: CoProducerMetrics.key) as? Bool == true)

        // Turning it on again opens nothing more.
        m.setOn(true)
        #expect(calls.log.count == 2)

        calls.log = []
        let again = model(defaults, dir)
        #expect(again.on)
        #expect(calls.log == ["private \(dir.path)", "open \(dir.path) 60000 5000"])
    }

    @Test func turningOffDropsTheOpenBlock() throws {
        let dir = try metricsSupportDir()
        let defaults = MemoryDefaults()
        let m = model(defaults, dir)
        m.setOn(true)
        m.setOn(false)
        #expect(recorder.log == ["close"])
        #expect(!m.on)
        #expect(m.recorder == nil)
        #expect(defaults.object(forKey: CoProducerMetrics.key) as? Bool == false)

        // Quitting closes, and the setting is kept.
        recorder.log = []
        m.setOn(true)
        m.quit()
        #expect(recorder.log == ["close"])
        #expect(m.recorder == nil)
        #expect(defaults.object(forKey: CoProducerMetrics.key) as? Bool == true)
    }

    @Test func aFailedStartSaysWhy() throws {
        let dir = try metricsSupportDir()
        let defaults = MemoryDefaults()
        let refused = model(defaults, dir, privateProblem: "the folder belongs to another user")
        refused.setOn(true)
        #expect(!refused.on)
        #expect(refused.recorder == nil)
        #expect(defaults.object(forKey: CoProducerMetrics.key) as? Bool == false)
        #expect(refused.problem == "Time counting could not start: the folder belongs to another user")
        // The folder refused, nothing was opened.
        #expect(!calls.log.contains { $0.hasPrefix("open") })

        let thrown = model(defaults, dir, openError: .Io(message: "The Metrics folder is not VizDAW's (a link)."))
        thrown.setOn(true)
        #expect(!thrown.on)
        #expect(defaults.object(forKey: CoProducerMetrics.key) as? Bool == false)
        #expect(thrown.problem == "Time counting could not start: The Metrics folder is not VizDAW's (a link).")

        // A stored `true` that cannot start is stored `false`.
        defaults.set(true, forKey: CoProducerMetrics.key)
        let atLaunch = model(defaults, dir, privateProblem: "no room")
        #expect(!atLaunch.on)
        #expect(defaults.object(forKey: CoProducerMetrics.key) as? Bool == false)

        // The recorder's own problem shows after a good open.
        recorder.problemAnswer = "The time records could not be read, so they were set aside as time.damaged.json and counting started afresh."
        let opened = model(defaults, dir)
        opened.setOn(true)
        #expect(opened.on)
        #expect(opened.problem == recorder.problemAnswer)
    }

    @Test func deleteAllClearsThenDeletes() throws {
        let dir = try metricsSupportDir()
        let defaults = MemoryDefaults()
        // Off: the folder goes through `metricsDeleteAll`.
        try FileManager.default.createDirectory(at: metricsDir(dir), withIntermediateDirectories: false)
        try Data("{}".utf8).write(to: metricsDir(dir).appendingPathComponent("time.json"))
        let m = model(defaults, dir)
        m.deleteAll()
        #expect(!FileManager.default.fileExists(atPath: metricsDir(dir).path))
        #expect(m.overview == nil)
        #expect(m.problem == nil)

        // On: the recorder clears and deletes under its own lock, and stays.
        m.setOn(true)
        m.deleteAll()
        #expect(recorder.log == ["clearAndDeleteAll"])
        #expect(m.recorder != nil)
        #expect(m.on)
        #expect(m.overview == nil)
        #expect(!m.confirmingDelete)

        // A failed delete says why.
        recorder.deleteError = .Io(message: "The time records could not be deleted: busy.")
        m.deleteAll()
        #expect(m.problem == "The time records could not be deleted: busy.")
    }

    @Test func fastTimingOnlyForTests() {
        #expect(CoProducerMetrics.timing(environment: [:]) == (60000, 5000))
        #expect(CoProducerMetrics.timing(environment: ["VIZDAW_METRICS_FAST": "1"]) == (60000, 5000))
        #expect(CoProducerMetrics.timing(environment: ["VIZDAW_SUPPORT_DIR": "/tmp/x"]) == (60000, 5000))
        #expect(
            CoProducerMetrics.timing(environment: ["VIZDAW_METRICS_FAST": "true", "VIZDAW_SUPPORT_DIR": "/tmp/x"])
                == (60000, 5000))
        #expect(
            CoProducerMetrics.timing(environment: ["VIZDAW_METRICS_FAST": "1", "VIZDAW_SUPPORT_DIR": "/tmp/x"])
                == (2000, 200))
    }

    @Test func fastTimingOpensFast() throws {
        let dir = try metricsSupportDir()
        let m = model(MemoryDefaults(), dir, environment: ["VIZDAW_METRICS_FAST": "1", "VIZDAW_SUPPORT_DIR": dir.path])
        m.setOn(true)
        #expect(calls.log.last == "open \(dir.path) 2000 200")
    }

    @Test func clockIsLocalDays() throws {
        var c = DateComponents()
        (c.year, c.month, c.day, c.hour, c.minute) = (2026, 11, 16, 23, 59)
        let late = try #require(Calendar.current.date(from: c))
        #expect(CoProducerMetrics.clock(late).day == "2026-11-16")
        (c.day, c.hour, c.minute) = (17, 0, 1)
        let early = try #require(Calendar.current.date(from: c))
        #expect(CoProducerMetrics.clock(early).day == "2026-11-17")
        #expect(CoProducerMetrics.clock(early).ms > CoProducerMetrics.clock(late).ms)
    }

    @Test func overviewLineReads() throws {
        let m = model(MemoryDefaults(), try metricsSupportDir())
        #expect(m.overview == nil)
        #expect(m.overviewLine == "No time records.")
        m.overview = MetricsOverviewRow(songs: 3, since: "2026-11-16", totalSeconds: 15_120)
        #expect(m.overviewLine == "Time records: 3 songs since 2026-11-16, 4 h 12 min in all.")
        m.overview = MetricsOverviewRow(songs: 1, since: "2026-11-16", totalSeconds: 600)
        #expect(m.overviewLine == "Time records: 1 song since 2026-11-16, 10 min in all.")
        m.overview = MetricsOverviewRow(songs: 1, since: "2026-11-16", totalSeconds: 30)
        #expect(m.overviewLine == "Time records: 1 song since 2026-11-16, under a minute in all.")
    }
}
