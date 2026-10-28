import Foundation
import Testing
import VizDAWCore
@testable import VizDAWAppModel

/// Stands in for the core's audio setters.
@MainActor
final class FakeAudioSink: AudioSink {
    var calls: [String] = []
    var bufferError: VizError?
    var streamError: VizError?
    func softwareMonitoring(_ on: Bool) { calls.append("monitoring \(on)") }
    func bufferFrames(_ frames: UInt32) throws {
        if let e = bufferError { throw e }
        calls.append("buffer \(frames)")
    }
    func inputDevice(_ name: String?) { calls.append("input \(name ?? "same")") }
    func latencyTrim(_ frames: Int32) { calls.append("trim \(frames)") }
    func stream(cacheMb: UInt32, readAheadSeconds: UInt32) throws {
        if let e = streamError { throw e }
        calls.append("stream \(cacheMb) \(readAheadSeconds)")
    }
    func defaultMonitor(_ mode: MonitorModeRow) { calls.append("monitor \(mode)") }
}

/// The app's audio settings (plan 5d-2 Task 1, Ruling 2): kept in the
/// preferences, handed to the core at launch and on every change, and
/// kept only once the core took them.
@MainActor
@Suite(.serialized) struct AudioPreferencesTests {
    @Test func launchHandsEverySettingToTheCore() {
        let sink = FakeAudioSink()
        let p = AudioPreferences(defaults: MemoryDefaults(), sink: sink)
        #expect(p.settings == AudioSettings())
        p.applyAll()
        #expect(
            sink.calls == [
                "monitoring true", "input same", "trim 0", "monitor auto", "buffer 128", "stream 512 3",
            ])
        #expect(p.problem == nil)
    }

    @Test func settingsAreKeptOnlyOnceTheCoreTookThem() throws {
        let defaults = MemoryDefaults()
        let sink = FakeAudioSink()
        let p = AudioPreferences(defaults: defaults, sink: sink)
        p.setBufferFrames(256)
        p.setSoftwareMonitoring(false)
        p.setInputDevice("Studio In")
        p.setLatencyTrim(-12)
        p.setStream(cacheMb: 1024, readAheadSeconds: 6)
        p.setDefaultMonitor("off")
        p.setRecordDefaults(countIn: 2, preRoll: 1, passesAsTakes: false)
        #expect(RecordModel.defaults.countInBars == 2 && !RecordModel.defaults.passesAsTakes)
        RecordModel.defaults = RecordDefaults()
        // Read back from the preferences.
        let again = AudioPreferences(defaults: defaults, sink: FakeAudioSink())
        var want = AudioSettings()
        want.bufferFrames = 256
        want.softwareMonitoring = false
        want.inputDevice = "Studio In"
        want.latencyTrim = -12
        want.cacheMb = 1024
        want.readAheadSeconds = 6
        want.defaultMonitor = "off"
        want.countInBars = 2
        want.preRollBars = 1
        want.passesAsTakes = false
        #expect(again.settings == want)
        // Refused while recording: the setting stays and says why.
        sink.bufferError = .Busy(message: "Stop recording to change the I/O buffer size.")
        p.setBufferFrames(512)
        #expect(p.settings.bufferFrames == 256)
        #expect(p.problem == "Stop recording to change the I/O buffer size.")
        #expect(throws: VizError.self) { try p.set(AudioChange(softwareMonitoring: true, bufferFrames: 512)) }
        #expect(!p.settings.softwareMonitoring, "nothing of a refused change applies")
        sink.bufferError = nil
        p.setBufferFrames(512)
        #expect(p.settings.bufferFrames == 512 && p.problem == nil)
        // Out of range: clamped.
        p.setStream(cacheMb: 1, readAheadSeconds: 99)
        #expect(p.settings.cacheMb == 64 && p.settings.readAheadSeconds == 8)
        p.setLatencyTrim(.max)
        #expect(p.settings.latencyTrim == 192_000)
        p.setDefaultMonitor("loud")
        #expect(p.settings.defaultMonitor == "off", "an unknown mode changes nothing")
    }

    @Test func damagedSettingsKeepTheirDefaults() throws {
        let defaults = MemoryDefaults()
        let damaged = #"{"bufferFrames": 100, "cacheMb": 9, "readAheadSeconds": "x", "defaultMonitor": "loud", "latencyTrim": 999999, "countInBars": 9, "softwareMonitoring": false}"#
        defaults.set(Data(damaged.utf8), forKey: AudioPreferences.key)
        let p = AudioPreferences(defaults: defaults, sink: FakeAudioSink())
        var want = AudioSettings()
        want.softwareMonitoring = false
        want.countInBars = 4
        #expect(p.settings == want)
        defaults.set(Data("not json".utf8), forKey: AudioPreferences.key)
        #expect(AudioPreferences(defaults: defaults, sink: FakeAudioSink()).settings == AudioSettings())
    }

    @Test func newDocumentsStartFromTheRecordDefaults() throws {
        var d = RecordDefaults()
        d.countInBars = 2
        d.preRollBars = 1
        d.passesAsTakes = false
        RecordModel.defaults = d
        defer { RecordModel.defaults = RecordDefaults() }
        let s = DocumentStore(makeEmpty: { FakeSession([]) }, makeDemo: { demoFake() }, open: { _ in demoFake() })
        let m = try #require(s.newDemo().flatMap(s.model))
        #expect(m.record.countInBars == 2 && m.record.preRollBars == 1 && !m.record.passesAsTakes)
        #expect(m.record.spec().countInBars == 2)
    }

    @Test func scriptsAndTestsNeverWriteTheRealPreferences() {
        #expect(MemoryDefaults.appStore(environment: ["VIZDAW_SUPPORT_DIR": "/tmp/x"]) is MemoryDefaults)
    }
}
