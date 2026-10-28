import Foundation
import Testing
import VizDAWCore
@testable import VizDAWAppModel

/// The lane input controls' model (plan 5d-2 Task 4; spec §6.9): the
/// picker's choices by the device's channel names or MIDI sources, a
/// return lane keeping its synth, the monitor cycle, the meters and their
/// two-second clip hold, and the faster poll while metering.
@MainActor
@Suite(.serialized) struct LaneInputsTests {
    /// Song (3) with `lanes`, shown.
    func doc(_ lanes: [LaneRow]) throws -> (DocumentModel, FakeSession) {
        let f = demoFake()
        f.laneTable[3] = lanes
        let s = DocumentStore(makeEmpty: { FakeSession([]) }, makeDemo: { f }, open: { _ in demoFake() })
        let m = try #require(s.newDemo().flatMap(s.model))
        m.select(3)
        return (m, f)
    }

    func audioLane(_ id: UInt64, _ input: LaneInputRow?, arm: Bool = false, monitor: MonitorModeRow = .auto) -> LaneRow {
        var l = lane(id, "Audio \(id)")
        l.audioOut = 7
        l.input = input
        l.arm = arm
        l.monitor = monitor
        return l
    }

    func titles(_ g: [InputGroup]) -> [String: [String]] {
        Dictionary(g.map { ($0.title, $0.choices.map(\.title)) }, uniquingKeysWith: { a, _ in a })
    }

    @Test func audioChoicesNameTheDevicesChannels() throws {
        let l = audioLane(30, nil)
        let (m, f) = try doc([l])
        f.inputRow = InputInfoRow(
            device: "Fake", channels: 3, names: ["Mic", "Guitar", ""], inputLatencyFrames: 0, outputLatencyFrames: 0)
        let inputs = m.inputs
        // Before the device was read: two channels.
        #expect(titles(inputs.groups(for: l))["Audio"] == ["In 1", "In 2", "In 1+2"])
        inputs.refreshInfo()
        let g = titles(inputs.groups(for: l))
        #expect(g[""] == ["No Input"])
        #expect(g["Audio"] == ["In 1 (Mic)", "In 2 (Guitar)", "In 3", "In 1+2"])
        #expect(g["MIDI Input"] == nil, "an audio lane offers no MIDI")
        let pair = try #require(inputs.groups(for: l)[1].choices.last)
        inputs.choose(pair, lane: l)
        #expect(f.edits.last == .lane(edit: .setInput(seq: 3, lane: 30, input: .audio(first: 0, stereo: true, returnOf: nil))))
        #expect(LaneInputs.title(pair.input) == "In 1+2")
        #expect(LaneInputs.title(nil) == "No Input")
    }

    @Test func aReturnLaneKeepsItsSynth() throws {
        let l = audioLane(30, .audio(first: 2, stereo: false, returnOf: 5))
        let (m, f) = try doc([l])
        f.dests = [DestRow(id: 5, name: "Moog", plugin: nil)]
        let inputs = m.inputs
        #expect(inputs.badge(l) == "⇠ Moog")
        #expect(inputs.badge(audioLane(31, .audio(first: 0, stereo: false, returnOf: nil))) == nil)
        let pair = try #require(inputs.groups(for: l)[1].choices.first { $0.title == "In 1+2" })
        inputs.choose(pair, lane: l)
        #expect(f.edits.last == .lane(edit: .setInput(seq: 3, lane: 30, input: .audio(first: 0, stereo: true, returnOf: 5))))
        // The same input again is no edit.
        let count = f.edits.count
        inputs.choose(InputChoice("In 3", l.input), lane: l)
        #expect(f.edits.count == count)
    }

    @Test func midiChoicesAreSourcesAndChannels() throws {
        LaneInputs.midiPorts = { ["Keystation", "Pads"] }
        defer { LaneInputs.midiPorts = { midiSourcePorts() } }
        var l = lane(31, "Keys")
        l.midiDest = 1
        l.input = .midi(port: "Pads", channel: nil)
        let (m, f) = try doc([l])
        let g = titles(m.inputs.groups(for: l))
        #expect(g["Audio"] == nil)
        #expect(g["MIDI Input"] == ["All MIDI Inputs", "Keystation", "Pads"])
        #expect(g["MIDI Channel"]?.first == "Any Channel" && g["MIDI Channel"]?.count == 17)
        let ch3 = try #require(m.inputs.groups(for: l)[2].choices.first { $0.title == "Channel 3" })
        m.inputs.choose(ch3, lane: l)
        #expect(f.edits.last == .lane(edit: .setInput(seq: 3, lane: 31, input: .midi(port: "Pads", channel: 2))))
        #expect(LaneInputs.title(.midi(port: "Pads", channel: 2)) == "Pads · Ch 3")
        #expect(LaneInputs.title(.midi(port: nil, channel: nil)) == "All MIDI")
        // A new lane with neither destination nor output offers both.
        let both = titles(m.inputs.groups(for: lane(32, "New")))
        #expect(both["Audio"] != nil && both["MIDI Input"] != nil)
    }

    @Test func theMonitorButtonCyclesAutoInputOff() throws {
        let l = audioLane(30, nil)
        let (m, f) = try doc([l])
        #expect(LaneInputs.next(.auto) == .input && LaneInputs.next(.input) == .off && LaneInputs.next(.off) == .auto)
        #expect([MonitorModeRow.auto, .input, .off].map(LaneInputs.label) == ["A", "I", "–"])
        m.inputs.cycleMonitor(l)
        #expect(f.edits.last == .lane(edit: .setMonitor(seq: 3, lane: 30, monitor: .input)))
    }

    @Test func metersShowArmedOrMonitoredLanesAndHoldTheClip() throws {
        let armed = audioLane(30, .audio(first: 1, stereo: true, returnOf: nil), arm: true)
        let idle = audioLane(31, .audio(first: 0, stereo: false, returnOf: nil))
        let (m, f) = try doc([armed, idle])
        let inputs = m.inputs
        var t = ContinuousClock.now
        inputs.now = { t }
        #expect(inputs.metering)
        #expect(m.pollSeconds == DocumentModel.pollSeconds(playing: true), "meters poll fast")
        f.inputLevelsRow = [0.9, 0.5, 0.7]
        inputs.pollMeters()
        #expect(inputs.level(30) == 0.7)
        #expect(inputs.level(31) == 0, "an unarmed Auto lane shows no meter")
        f.inputLevelsRow = [0, 0, 0]
        inputs.pollMeters()
        #expect(inputs.level(30) == 0.7 * MixerMath.meterDecay)
        #expect(!inputs.isClipped(30))
        f.inputLevelsRow = [0, 1.2, 0]
        inputs.pollMeters()
        #expect(inputs.isClipped(30))
        f.inputLevelsRow = [0, 0, 0]
        t = t + .milliseconds(1_900)
        inputs.pollMeters()
        #expect(inputs.isClipped(30), "held for two seconds")
        t = t + .milliseconds(200)
        inputs.pollMeters()
        #expect(!inputs.isClipped(30))
        // A recording's clipped samples light it too.
        f.recordRow.lanes = [
            RecordLaneRow(lane: 30, audio: true, path: nil, clipped: 3)
        ]
        _ = m.record.toggle()
        inputs.pollMeters()
        #expect(inputs.isClipped(30))
    }

    @Test func noMeterNoFastPoll() throws {
        let (m, f) = try doc([audioLane(30, .audio(first: 0, stereo: false, returnOf: nil))])
        #expect(!m.inputs.metering)
        #expect(m.pollSeconds == DocumentModel.pollSeconds(playing: false))
        f.inputLevelsRow = [1]
        m.inputs.pollMeters()
        #expect(m.inputs.levels.isEmpty && m.inputs.clipped.isEmpty)
    }

    /// Pre-flight I2 (5d-1 carry #1): the header meters and `state.get
    /// input` read the same read-and-reset peaks; neither steals the other's.
    @Test func theMetersAndTheChannelEachSeeEveryPeak() throws {
        let (m, f) = try doc([audioLane(30, .audio(first: 0, stereo: false, returnOf: nil), arm: true)])
        f.inputLevelsReset = true
        f.inputLevelsRow = [1.2]
        guard case .inputState(_, _, let first)? = ControlStates.state(.input, UUID(), m) else {
            Issue.record("input")
            return
        }
        #expect(first == [1.2])
        m.inputs.pollMeters()
        #expect(m.inputs.isClipped(30), "the channel no longer steals the clip")
        f.inputLevelsRow = [0.7]
        m.inputs.pollMeters()
        guard case .inputState(_, _, let second)? = ControlStates.state(.input, UUID(), m) else {
            Issue.record("input")
            return
        }
        #expect(second == [0.7], "the meters no longer steal the peak")
    }

    /// Pre-flight I3 (plan 5d-1 M18): with Software Monitoring off, an
    /// Input-monitored lane that is not armed shows no meter.
    @Test func softwareMonitoringOffHidesAnInputMonitoredMeter() throws {
        defer { LaneInputs.softwareMonitoring = { audioSettings().softwareMonitoring } }
        LaneInputs.softwareMonitoring = { true }
        let (m, f) = try doc([audioLane(30, .audio(first: 0, stereo: false, returnOf: nil), monitor: .input)])
        #expect(m.inputs.metering)
        f.inputLevelsRow = [0.5]
        LaneInputs.softwareMonitoring = { false }
        m.inputs.pollMeters()
        #expect(!m.inputs.metering)
        #expect(!m.inputs.monitoringOn)
        #expect(m.pollSeconds == DocumentModel.pollSeconds(playing: false))
        #expect(m.inputs.level(30) == 0)
    }

    /// Pre-flight I5 (5d-1 carry #9): while the input opens (the microphone
    /// prompt may be up) the status line says so.
    @Test func anOpeningInputSaysSo() throws {
        let (m, f) = try doc([audioLane(30, .audio(first: 0, stereo: false, returnOf: nil), arm: true)])
        f.inputRow.opening = true
        m.inputs.pollMeters()
        #expect(m.inputs.opening)
        #expect(m.editor.status == LaneInputs.openingNote)
        f.inputRow.opening = false
        m.inputs.pollMeters()
        #expect(!m.inputs.opening)
        #expect(m.editor.status == nil)
    }
}
