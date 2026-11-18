import Foundation
import Testing
import VizDAWCore
@testable import VizDAWAppModel

/// Settings › Audio's readouts and choices (plan 5d-2 Task 7).
@MainActor
@Suite(.serialized) struct AudioReadoutsTests {
    @Test func theRoundTripIsTheDevicesOnceItOpened() {
        let open = InputInfoRow(
            device: "Studio", channels: 2, names: [], inputLatencyFrames: 38, outputLatencyFrames: 34,
            bufferFrames: 128, roundTripFrames: 328, sampleRate: 48_000)
        #expect(
            AudioPreferences.roundTripText(open, bufferFrames: 128)
                == "Round trip 6.8 ms (input 38 + output 34 + 2 × 128 frames)")
        var clamped = open
        clamped.bufferFrames = 256
        clamped.roundTripFrames = 584
        #expect(
            AudioPreferences.roundTripText(clamped, bufferFrames: 32)
                == "Round trip 12.2 ms (input 38 + output 34 + 2 × 256 frames); Studio runs at 256 frames")
        #expect(
            AudioPreferences.roundTripText(nil, bufferFrames: 128)
                == "About 5.3 ms at 48 kHz, plus the device's own latency once it opens")
        let closed = InputInfoRow(device: "", channels: 0, names: [], inputLatencyFrames: 0, outputLatencyFrames: 0)
        #expect(AudioPreferences.roundTripText(closed, bufferFrames: 1024).hasPrefix("About 42.7 ms"))
    }

    /// Task 7 review Minor 1: a stored value off the list (an old pref, or
    /// `audio.set` over the channel) is offered too, so the picker never
    /// shows an empty selection.
    @Test func aPickerKeepsAnOffListValue() {
        #expect(AudioPreferences.choices(AudioPreferences.cacheChoices, keeping: 512) == AudioPreferences.cacheChoices)
        #expect(
            AudioPreferences.choices(AudioPreferences.cacheChoices, keeping: 300)
                == [64, 128, 256, 300, 512, 1024, 2048, 4096, 8192])
        #expect(AudioPreferences.choices([44_100, 48_000], keeping: 22_050) == [22_050, 44_100, 48_000])
    }

    @Test func theDevicePickerKeepsADisconnectedChoice() {
        AudioPreferences.inputDeviceList = {
            [InputDeviceRow(name: "Studio In", uid: "s1"), InputDeviceRow(name: "Mic", uid: "m1")]
        }
        defer { AudioPreferences.inputDeviceList = { audioInputDeviceList() } }
        let p = AudioPreferences(defaults: MemoryDefaults(), sink: FakeAudioSink())
        #expect(p.inputDeviceChoices().map(\.title) == ["Same as output", "Studio In", "Mic"])
        p.setInputDevice(uid: nil, name: "Gone")
        let c = p.inputDeviceChoices()
        #expect(c.map(\.title) == ["Same as output", "Studio In", "Mic", "Gone (not connected)"])
        #expect(c.last?.name == "Gone" && c.first?.name == nil)
    }

    /// Release fixes Task 2: two devices with one name are two choices,
    /// kept apart by UID; a saved device that is gone is still offered.
    @Test func twoDevicesWithOneNameAreKeptApart() {
        AudioPreferences.inputDeviceList = {
            [InputDeviceRow(name: "A", uid: "u1"), InputDeviceRow(name: "A", uid: "u2")]
        }
        defer { AudioPreferences.inputDeviceList = { audioInputDeviceList() } }
        let p = AudioPreferences(defaults: MemoryDefaults(), sink: FakeAudioSink())
        let c = p.inputDeviceChoices().dropFirst()
        #expect(c.map(\.title) == ["A", "A (2)"])
        #expect(c.map(\.uid) == ["u1", "u2"])
        #expect(c.map(\.name) == ["A", "A"])
        p.setInputDevice(uid: "u9", name: "Old")
        let gone = p.inputDeviceChoices()
        #expect(gone.map(\.title) == ["Same as output", "A", "A (2)", "Old (not connected)"])
        #expect(gone.last?.uid == "u9" && gone.last?.name == "Old")
        // A listed UID is not "not connected", whatever name was saved.
        p.setInputDevice(uid: "u2", name: "Renamed")
        #expect(p.inputDeviceChoices().map(\.title) == ["Same as output", "A", "A (2)"])
    }

    @Test func trimAndDeviceReadouts() throws {
        #expect(AudioPreferences.trimText(12, sampleRate: 48_000) == "+12 frames (0.25 ms)")
        #expect(AudioPreferences.trimText(-96, sampleRate: 96_000) == "-96 frames (-1.00 ms)")
        #expect(AudioPreferences.trimText(0, sampleRate: 0) == "0 frames (0.00 ms)")
        let p = AudioPreferences(defaults: MemoryDefaults(), sink: FakeAudioSink())
        let f = demoFake()
        f.inputRow = InputInfoRow(
            device: "Fake", channels: 3, names: [], inputLatencyFrames: 1, outputLatencyFrames: 2, sampleRate: 48_000)
        p.readDevice(f)
        #expect(p.device?.device == "Fake")
        p.readDevice(nil)
        #expect(p.device == nil)
    }

    /// Settings › Recording's format pickers redraw when it changes.
    @Test func theDefaultProjectFormatIsObserved() {
        final class Flag: @unchecked Sendable { var on = false }
        let s = DocumentStore(makeEmpty: { FakeSession([]) }, makeDemo: { demoFake() }, open: { _ in demoFake() })
        let changed = Flag()
        withObservationTracking { _ = s.defaultFormat } onChange: { changed.on = true }
        s.defaultFormat = ProjectFormatRow(sampleRate: 96_000, depth: .float32)
        #expect(changed.on)
        #expect(s.defaultFormat == ProjectFormatRow(sampleRate: 96_000, depth: .float32))
    }
}
