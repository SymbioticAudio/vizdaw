import Foundation
import Testing
import VizDAWCore
@testable import VizDAWAppModel

/// The lane menu's sheets (plan 5d-2 Task 8; spec §6.7): Track Synth sets
/// up the return lane and stays open for Measure, which runs off the main
/// thread; New MIDI Destination adds a destination and points the lane at
/// it; under the control channel neither sheet opens.
@MainActor
@Suite(.serialized) struct SynthSheetsTests {
    /// Juno (7) is an external MIDI destination that Chords (31) plays.
    func demo() throws -> (DocumentModel, FakeSession) {
        let f = demoFake()
        f.dests = [
            DestRow(id: 1, name: "Piano", plugin: nil),
            DestRow(id: 7, name: "Juno", plugin: nil, external: true, returnLatencyMs: 12),
        ]
        f.laneTable[3]?[1].midiDest = 7
        let s = DocumentStore(makeEmpty: { FakeSession([]) }, makeDemo: { f }, open: { _ in demoFake() })
        let m = try #require(s.newDemo().flatMap(s.model))
        m.select(3)
        return (m, f)
    }

    func settle(_ sheets: SynthSheets) async {
        let deadline = ContinuousClock.now + .seconds(10)
        while sheets.measuring != nil, ContinuousClock.now < deadline {
            try? await Task.sleep(for: .milliseconds(5))
        }
    }

    @Test func trackSynthSetsUpTheReturnAndStaysOpenForMeasure() throws {
        let (m, f) = try demo()
        let sheets = m.synth
        let chords = m.lanes[1]
        #expect(sheets.synth(of: chords)?.name == "Juno")
        sheets.beginTrackSynth(seq: 3, lane: chords, channelOn: false)
        var d = try #require(sheets.trackSynth)
        #expect(d.synthName == "Juno" && d.dest == 7 && d.channel == 1 && !d.tracked)
        d.channel = 3
        d.stereo = true
        d.muteAfter = true
        sheets.trackSynth = d
        sheets.commitTrackSynth()
        let c = try #require(f.trackSynthCalls.last)
        #expect(c.seq == 3 && c.lane == 31 && c.first == 2 && c.stereo && !c.armMidi && c.muteAfter)
        #expect(sheets.trackSynth?.tracked == true, "the sheet stays for Measure")
        #expect(sheets.message?.hasPrefix("Juno audio records the return") == true)
        sheets.commitTrackSynth()
        #expect(f.trackSynthCalls.count == 1, "set up once")
        #expect(sheets.returnLatency(7) == 12)
        sheets.closeTrackSynth()
        #expect(sheets.trackSynth == nil && sheets.message == nil)
        // A refusal keeps the sheet and says why.
        sheets.beginTrackSynth(seq: 3, lane: chords, channelOn: false)
        f.synthError = .Busy(message: "Stop recording first.")
        sheets.commitTrackSynth()
        #expect(sheets.trackSynth?.tracked == false && sheets.message == "Stop recording first.")
    }

    @Test func trackSynthNeedsAnExternalSynthAndNoChannel() throws {
        let (m, _) = try demo()
        m.synth.beginTrackSynth(seq: 3, lane: m.lanes[0], channelOn: false)
        #expect(m.synth.trackSynth == nil)
        #expect(m.editor.status == "Track Synth needs a track that plays an external MIDI destination.")
        m.synth.beginTrackSynth(seq: 3, lane: m.lanes[1], channelOn: true)
        #expect(m.synth.trackSynth == nil && m.editor.status == SynthSheets.trackSynthOnChannel)
        m.synth.beginNewDestination(seq: 3, lane: m.lanes[1], channelOn: true)
        #expect(m.synth.newDestination == nil && m.editor.status == SynthSheets.newDestinationOnChannel)
    }

    @Test func measureRunsOffTheMainThreadAndShowsTheResult() async throws {
        let (m, f) = try demo()
        let sheets = m.synth
        f.measured = 12.5
        let version = m.snapshot.version
        sheets.measure(7)
        #expect(sheets.measuring != nil && sheets.message == "Measuring…")
        sheets.measure(7)
        await settle(sheets)
        #expect(sheets.measuring == nil)
        #expect(f.measures == [7], "one at a time")
        #expect(f.measuredOnMain == [false], "never on the main thread")
        #expect(sheets.message == "Return latency 12.5 ms (5 of 5 notes heard).")
        #expect(m.snapshot.version > version, "the step was applied")
        f.synthError = .Model(message: "No signal on the synth's return input")
        sheets.measure(7)
        await settle(sheets)
        #expect(sheets.message == "No signal on the synth's return input")
    }

    /// Record pressed while Measure runs: no UI gate, the core refuses it
    /// (`Busy`, plan 5c-2 Ruling 9) and the status line says why.
    @Test func recordWhileMeasuringShowsTheCoresRefusal() async throws {
        let (m, f) = try demo()
        let sheets = m.synth
        sheets.measure(7)
        #expect(sheets.measuring != nil)
        let busy = "A synth's return latency is being measured; try again in a moment."
        f.recordError = .Busy(message: busy)
        m.record.pressRecord()
        #expect(m.editor.status == busy && !m.record.isRecording)
        await settle(sheets)
        #expect(sheets.measuring == nil && f.measures == [7])
    }

    /// Task 8 review M3: Measure's result never shows in the New MIDI
    /// Destination sheet.
    @Test func eachSheetKeepsItsOwnMessage() async throws {
        SynthSheets.ports = { ["Moog"] }
        defer { SynthSheets.ports = { midiDestinationPorts() } }
        let (m, f) = try demo()
        let sheets = m.synth
        sheets.measure(7)
        sheets.beginNewDestination(seq: 3, lane: m.lanes[1], channelOn: false)
        await settle(sheets)
        #expect(sheets.message?.hasPrefix("Return latency") == true)
        #expect(sheets.destinationMessage == nil)
        f.synthError = .Model(message: "the port is gone")
        sheets.commitNewDestination()
        #expect(sheets.destinationMessage == "the port is gone" && sheets.newDestination != nil)
        sheets.closeNewDestination()
        #expect(sheets.destinationMessage == nil)
    }

    /// Task 8 review M4, M5: the name counts as the core counts it; an added
    /// destination that cannot be found again is said.
    @Test func newDestinationCountsAndReportsAsTheCoreDoes() throws {
        SynthSheets.ports = { ["Moog"] }
        defer { SynthSheets.ports = { midiDestinationPorts() } }
        let (m, f) = try demo()
        let sheets = m.synth
        sheets.beginNewDestination(seq: 3, lane: m.lanes[1], channelOn: false)
        // 40 characters, 280 Unicode scalars: the core refuses more than 255.
        sheets.newDestination?.name = String(repeating: "\u{1F469}\u{200D}\u{1F469}\u{200D}\u{1F467}\u{200D}\u{1F466}", count: 40)
        #expect(!sheets.canAddDestination)
        sheets.newDestination?.name = "Sub 37"
        #expect(sheets.canAddDestination)
        f.destinationsError = .Internal(message: "no list")
        sheets.commitNewDestination()
        #expect(f.addedDestinations.count == 1 && sheets.newDestination == nil)
        #expect(m.editor.status == "Added Sub 37; choose it in the track's destination menu.")
    }

    @Test func newDestinationAddsItAndPointsTheLaneAtIt() throws {
        SynthSheets.ports = { ["Moog", "Juno"] }
        defer { SynthSheets.ports = { midiDestinationPorts() } }
        let (m, f) = try demo()
        let sheets = m.synth
        sheets.beginNewDestination(seq: 3, lane: m.lanes[1], channelOn: false)
        var d = try #require(sheets.newDestination)
        #expect(d.name == "Moog" && d.port == "Moog" && d.channel == 1 && d.ports == ["Moog", "Juno"])
        d.name = "  "
        sheets.newDestination = d
        #expect(!sheets.canAddDestination)
        d.name = "  Sub 37 "
        d.port = "Juno"
        d.channel = 2
        sheets.newDestination = d
        #expect(sheets.canAddDestination)
        sheets.commitNewDestination()
        let added = try #require(f.addedDestinations.last)
        #expect(added.name == "Sub 37" && added.port == "Juno" && added.channel == 1)
        let new = try #require(f.dests.last)
        #expect(f.edits.last == .lane(edit: .setDest(seq: 3, lane: 31, dest: new.id)))
        #expect(sheets.newDestination == nil)
        // No MIDI destination connected: nothing to add.
        SynthSheets.ports = { [] }
        sheets.beginNewDestination(seq: 3, lane: m.lanes[1], channelOn: false)
        #expect(sheets.newDestination?.port == "" && !sheets.canAddDestination)
        sheets.closeNewDestination()
        #expect(sheets.newDestination == nil)
    }

    // MARK: Built-in Synth sounds and the destination editor (plan release-fixes Task 8)

    /// 7 is the Built-in Synth (Piano), 9 the external "Juno" (12 ms).
    func soundDemo() throws -> (DocumentModel, FakeSession) {
        let f = demoFake()
        f.dests = [
            DestRow(id: 7, name: "Built-in Synth", plugin: nil, channel: 0, builtinSound: .piano),
            DestRow(id: 9, name: "Juno", plugin: nil, external: true, returnLatencyMs: 12, channel: 0),
        ]
        let s = DocumentStore(makeEmpty: { FakeSession([]) }, makeDemo: { f }, open: { _ in demoFake() })
        let m = try #require(s.newDemo().flatMap(s.model))
        return (m, f)
    }

    @Test func soundsRoundTripThroughTheSession() throws {
        let (m, f) = try soundDemo()
        let sheets = m.synth
        #expect(sheets.sound(of: 7) == .piano)
        #expect(sheets.sound(of: 9) == nil && sheets.sound(of: 999) == nil)
        let version = m.snapshot.version
        #expect(sheets.setSound(.bass, dest: 7) == .done)
        #expect(f.builtinSounds.count == 1 && f.builtinSounds.last?.dest == 7 && f.builtinSounds.last?.sound == .bass)
        #expect(sheets.sound(of: 7) == .bass, "read back from the session's row")
        #expect(m.snapshot.version > version, "the step was applied")
        // Not a Built-in Synth: no call.
        #expect(sheets.setSound(.drums, dest: 9) != .done)
        #expect(sheets.setSound(.drums, dest: 999) != .done)
        #expect(f.builtinSounds.count == 1)
        // The core's refusal is said.
        f.synthError = .Busy(message: "Finish the open edit first.")
        #expect(sheets.setSound(.lead, dest: 7) == .busy("Finish the open edit first."))
        #expect(SynthSheets.sounds == [.piano, .bass, .pad, .lead, .drums])
        #expect(SynthSheets.sounds.map(SynthSheets.title) == ["Piano", "Bass", "Pad", "Lead", "Drums"])
    }

    @Test func theDestinationEditorCommitsLatencyAndMeasures() async throws {
        let (m, f) = try soundDemo()
        let sheets = m.synth
        sheets.beginEditDestination(9, channelOn: false)
        let d = try #require(sheets.editDestination)
        #expect(d.dest == 9 && d.name == "Juno" && d.latencyText == "12")
        for bad in ["abc", "-1", "501", "1\u{0}"] {
            sheets.editDestination?.latencyText = bad
            #expect(!sheets.commitLatency(), "\(bad)")
        }
        #expect(f.returnLatencies.isEmpty, "no call for a refused value")
        sheets.editDestination?.latencyText = "12.5"
        #expect(sheets.commitLatency())
        #expect(f.returnLatencies.count == 1 && f.returnLatencies.last?.dest == 9 && f.returnLatencies.last?.ms == 12.5)
        #expect(sheets.editDestination != nil, "Return keeps the sheet")
        // Measure, as the Track Synth sheet's: off the main thread.
        f.measured = 14.5
        sheets.measure(9)
        #expect(sheets.measuring != nil && sheets.message == "Measuring…")
        await settle(sheets)
        #expect(f.measures == [9] && f.measuredOnMain == [false])
        #expect(sheets.message == "Return latency 14.5 ms (5 of 5 notes heard).")
        #expect(sheets.editDestination?.latencyText == "14.5", "the field shows what was measured")
        sheets.closeEditDestination()
        #expect(sheets.editDestination == nil && sheets.message == nil)
        // Only an external destination; never under the control channel.
        sheets.beginEditDestination(7, channelOn: false)
        #expect(sheets.editDestination == nil)
        sheets.beginEditDestination(9, channelOn: true)
        #expect(sheets.editDestination == nil && m.editor.status == SynthSheets.editDestinationOnChannel)
        #expect(SynthSheets.editDestinationOnChannel == "Use destination.set_return_latency or destination.measure_return.")
    }
}
