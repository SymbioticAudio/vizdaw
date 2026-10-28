import Foundation
import Testing
import VizDAWCore
@testable import VizDAWAppModel

/// Synth tracking over the control channel (plan 5c-2 Task 10): Track
/// Synth with channels counted from 1, a return latency typed as text
/// (0–500 ms), Measure answering with its milliseconds, and
/// `lane.set_input`'s `return_of`; hostile values are `invalid_params`
/// and the core's refusals the call's failure.
@MainActor
@Suite struct SynthCommandsTests {
    let host = FakeControlHost()

    /// Piano is the built-in synth; Juno (7) an external MIDI destination.
    func demo() throws -> (DocumentStore, UUID, DocumentModel, FakeSession) {
        let f = demoFake()
        f.dests = [
            DestRow(id: 1, name: "Piano", plugin: nil),
            DestRow(id: 7, name: "Juno", plugin: nil, external: true, returnLatencyMs: 12),
        ]
        let s = DocumentStore(makeEmpty: { FakeSession([]) }, makeDemo: { f }, open: { _ in demoFake() })
        let id = try #require(s.newDemo())
        return (s, id, try #require(s.model(id)), f)
    }

    func run(_ id: String, _ pairs: [(String, ControlValue)], _ m: DocumentModel, _ s: DocumentStore, _ doc: UUID)
        -> ControlOutcome
    {
        ControlCommands.run(
            id, args: pairs.map { ControlArg(name: $0.0, value: $0.1) }, doc: (doc, m), store: s, host: host)
    }

    func failure(_ o: ControlOutcome) -> ControlFailure? {
        if case .failed(let f) = o { return f }
        return nil
    }

    /// Measure answers once its job ends (plan 5d-2 Ruling 6), as the
    /// channel answers it.
    func settled(_ o: ControlOutcome, _ m: DocumentModel) async -> ControlOutcome {
        guard case .measure(let doc, let job) = o else { return o }
        let deadline = ContinuousClock.now + .seconds(10)
        while !job.isDone, ContinuousClock.now < deadline { try? await Task.sleep(for: .milliseconds(5)) }
        return SynthCommands.answer(job, doc: doc, m) ?? .failed(.of(.timeout, "Measure did not end"))
    }

    func measured(_ args: [(String, ControlValue)], _ m: DocumentModel, _ s: DocumentStore, _ doc: UUID) async
        -> ControlOutcome
    {
        await settled(run("destination.measure_return", args, m, s, doc), m)
    }

    @Test func trackSynthAsksTheCoreWithChannelsFromOne() throws {
        let (s, id, m, f) = try demo()
        let o = run(
            "lane.track_synth",
            [
                ("seq", .int(value: 3)), ("lane", .int(value: 31)), ("channel", .int(value: 3)),
                ("stereo", .flag(value: true)), ("arm_midi", .flag(value: true)),
                ("mute_after", .flag(value: true)),
            ], m, s, id)
        #expect(failure(o) == nil)
        let c = try #require(f.trackSynthCalls.last)
        #expect(c.seq == 3 && c.lane == 31 && c.first == 2 && c.stereo && c.armMidi && c.muteAfter)
        // Defaults: channel 1, mono, the MIDI lane disarmed, no mute.
        _ = run("lane.track_synth", [("seq", .int(value: 3)), ("lane", .int(value: 31))], m, s, id)
        let d = try #require(f.trackSynthCalls.last)
        #expect(d.first == 0 && !d.stereo && !d.armMidi && !d.muteAfter)
        // Hostile channels and missing ids never reach the core.
        let before = f.trackSynthCalls.count
        for extra: [(String, ControlValue)] in [
            [("channel", .int(value: 0))], [("channel", .int(value: 257))], [("channel", .int(value: -1))],
            [("channel", .int(value: .max))],
        ] {
            #expect(
                failure(run("lane.track_synth", [("seq", .int(value: 3)), ("lane", .int(value: 31))] + extra, m, s, id))?
                    .code == .invalidParams)
        }
        for ids: [(String, ControlValue)] in [
            [("lane", .int(value: 31))], [("seq", .int(value: 3))], [],
            [("seq", .int(value: -3)), ("lane", .int(value: 31))],
            // Wrong types are absent, so the required ids are missing.
            [("seq", .text(value: "3")), ("lane", .int(value: 31))],
            [("seq", .int(value: 3)), ("lane", .flag(value: true))],
        ] {
            #expect(failure(run("lane.track_synth", ids, m, s, id))?.code == .invalidParams)
        }
        #expect(f.trackSynthCalls.count == before)
        // The core's refusals are the call's failure: a lane that plays no
        // external synth (the built-in one), an unknown lane...
        f.synthError = .Model(message: "Piano is not an external MIDI destination")
        let e = failure(run("lane.track_synth", [("seq", .int(value: 3)), ("lane", .int(value: 30))], m, s, id))
        #expect(e?.code == .failed && e?.message.contains("external") == true)
        f.synthError = .Model(message: "no lane 999")
        #expect(
            failure(run("lane.track_synth", [("seq", .int(value: 3)), ("lane", .int(value: 999))], m, s, id))?.code
                == .failed)
        // ...and recording is `busy`, its message unchanged.
        f.synthError = .Busy(message: "Stop recording first.")
        let b = failure(run("lane.track_synth", [("seq", .int(value: 3)), ("lane", .int(value: 31))], m, s, id))
        #expect(b?.code == .busy && b?.message == "Stop recording first.")
        #expect(f.trackSynthCalls.count == before)
    }

    @Test func returnLatencyIsTextMillisecondsFrom0To500() throws {
        let (s, id, m, f) = try demo()
        #expect(
            failure(
                run("destination.set_return_latency", [("dest", .int(value: 7)), ("ms", .text(value: "12.5"))], m, s, id))
                == nil)
        #expect(f.returnLatencies.last?.dest == 7 && f.returnLatencies.last?.ms == 12.5)
        for bad in [
            "", " ", "abc", "-1", "-0.5", "500.5", "501", "nan", "NaN", "inf", "-inf", "1e9", "1e2", "0x10", "1.2.3", ".",
            "12 ms", "١٢", String(repeating: "1", count: 40), String(repeating: "9", count: 100_000),
        ] {
            #expect(
                failure(
                    run("destination.set_return_latency", [("dest", .int(value: 7)), ("ms", .text(value: bad))], m, s, id))?
                    .code == .invalidParams, "\(bad.prefix(10))")
        }
        for args: [(String, ControlValue)] in [
            [("ms", .text(value: "3"))], [("dest", .int(value: 7))], [("dest", .int(value: -7)), ("ms", .text(value: "3"))],
            // The milliseconds are text; a number is absent.
            [("dest", .int(value: 7)), ("ms", .int(value: 3))],
            // The built-in synth and an unknown destination are not external.
            [("dest", .int(value: 1)), ("ms", .text(value: "3"))], [("dest", .int(value: 99)), ("ms", .text(value: "3"))],
        ] {
            #expect(failure(run("destination.set_return_latency", args, m, s, id))?.code == .invalidParams)
        }
        #expect(f.returnLatencies.count == 1)
        #expect(SynthCommands.parseMs(" 0 ") == 0 && SynthCommands.parseMs("500") == 500)
        #expect(SynthCommands.parseMs("0.001") == 0.001 && SynthCommands.parseMs("500.0001") == nil)
        f.synthError = .Model(message: "return_latency_ms out of range")
        #expect(
            failure(
                run("destination.set_return_latency", [("dest", .int(value: 7)), ("ms", .text(value: "3"))], m, s, id))?
                .code == .failed)
    }

    @Test func measureAnswersWithTheMillisecondsAndAppliesTheStep() async throws {
        let (s, id, m, f) = try demo()
        f.measured = 24.9
        let version = m.snapshot.version
        let started = run("destination.measure_return", [("dest", .int(value: 7))], m, s, id)
        guard case .measure = started else {
            Issue.record("Measure answers when its job ends")
            return
        }
        guard case .done(.measured(let doc, let dest, let ms, let onsets)) = await settled(started, m) else {
            Issue.record("Measure did not answer with its milliseconds")
            return
        }
        #expect(doc == id.uuidString && dest == 7 && ms == 24.9 && onsets == 5)
        #expect(f.measures == [7])
        #expect(m.snapshot.version > version, "the step was applied")
        #expect(f.measuredOnMain == [false], "off the main thread")
        f.synthError = .Model(message: "No signal on the synth's return input")
        let e = failure(await measured([("dest", .int(value: 7))], m, s, id))
        #expect(e?.code == .failed && e?.message.contains("No signal") == true)
        f.synthError = .Busy(message: "Stop the transport first: Measure runs with it stopped.")
        #expect(failure(await measured([("dest", .int(value: 7))], m, s, id))?.code == .busy)
        #expect(failure(run("destination.measure_return", [], m, s, id))?.code == .invalidParams)
        #expect(f.measures == [7])
    }

    /// Ruling 3 of the dispatch: each of Measure's refusals has its code;
    /// the core's Busy messages (the measured value included) pass
    /// through unchanged.
    @Test func measureRefusalsHaveTheirCodes() async throws {
        let (s, id, m, f) = try demo()
        func measure() async -> ControlFailure? { failure(await measured([("dest", .int(value: 7))], m, s, id)) }
        for busy in [
            "Finish the open edit first (measured 25.0 ms).", "Finish the open edit first.",
            "Another measurement is running.", "Stop the transport first: Measure runs with it stopped.",
            "Stop recording first.",
        ] {
            f.synthError = .Busy(message: busy)
            let e = await measure()
            #expect(e?.code == .busy && e?.message == busy, "\(busy)")
        }
        for (error, text) in [
            (
                VizError.Engine(
                    message: "the measurement did not finish: it timed out, or the audio device stopped"),
                "did not finish"
            ),
            (
                VizError.Engine(
                    message: "the measurement stopped before it finished: the audio device went away"),
                "stopped before it finished"
            ),
            (
                VizError.Model(message: "no lane records the return of Juno: set one up with Track Synth first"),
                "Track Synth first"
            ),
            (VizError.Model(message: "No signal on the synth's return input"), "No signal"),
        ] {
            f.synthError = error
            let e = await measure()
            #expect(e?.code == .failed && e?.message.contains(text) == true, "\(text)")
            #expect(e?.dialog == nil)
        }
        #expect(m.alert == nil, "no alert is shown")
        // A destination that is not external, the built-in synth included,
        // or none at all, never reaches the core.
        for args: [(String, ControlValue)] in [
            [("dest", .int(value: 1))], [("dest", .int(value: 99))], [("dest", .int(value: -1))],
            [("dest", .text(value: "Juno"))], [("dest", .int(value: .max))],
        ] {
            #expect(failure(run("destination.measure_return", args, m, s, id))?.code == .invalidParams)
        }
        #expect(f.measures.isEmpty)
        // A drag in progress refuses it.
        #expect(m.editor.beginGesture("Move", lanes: [], origin: .other))
        #expect(await measure()?.code == .unavailable)
        #expect(f.measures.isEmpty)
    }

    @Test func laneSetInputCarriesReturnOf() throws {
        let (s, id, m, f) = try demo()
        let lane: [(String, ControlValue)] = [("seq", .int(value: 3)), ("lane", .int(value: 31))]
        #expect(
            failure(
                run(
                    "lane.set_input",
                    lane + [("kind", .text(value: "audio")), ("channel", .int(value: 3)), ("return_of", .int(value: 7))],
                    m, s, id)) == nil)
        #expect(f.edits.last == .lane(edit: .setInput(seq: 3, lane: 31, input: .audio(first: 2, stereo: false, returnOf: 7))))
        let before = f.edits.count
        for extra: [(String, ControlValue)] in [
            [("kind", .text(value: "midi")), ("return_of", .int(value: 7))],
            [("kind", .text(value: "none")), ("return_of", .int(value: 7))],
            [("kind", .text(value: "audio")), ("return_of", .int(value: -1))],
            [("kind", .text(value: "audio")), ("return_of", .int(value: .min))],
            // Not an external destination: the built-in synth, an unknown one.
            [("kind", .text(value: "audio")), ("return_of", .int(value: 1))],
            [("kind", .text(value: "audio")), ("return_of", .int(value: 99))],
        ] {
            #expect(failure(run("lane.set_input", lane + extra, m, s, id))?.code == .invalidParams)
        }
        #expect(f.edits.count == before)
    }

    /// Since plan 5c-2 Task 8 a `lane.set_input` without `return_of` is
    /// plain audio: the lane's link to a synth's return is dropped.
    @Test func laneSetInputWithoutReturnOfUnlinks() throws {
        let (s, id, m, f) = try demo()
        let lane: [(String, ControlValue)] = [("seq", .int(value: 3)), ("lane", .int(value: 31))]
        _ = run(
            "lane.set_input",
            lane + [("kind", .text(value: "audio")), ("channel", .int(value: 3)), ("return_of", .int(value: 7))], m, s, id)
        #expect(
            failure(run("lane.set_input", lane + [("kind", .text(value: "audio")), ("channel", .int(value: 3))], m, s, id))
                == nil)
        #expect(f.edits.last == .lane(edit: .setInput(seq: 3, lane: 31, input: .audio(first: 2, stereo: false, returnOf: nil))))
    }

    /// Task 10 review I1: an open edit the editor does not see as a
    /// gesture (a mixer fader drag) reaches the core, whose refusal is
    /// `busy`, as Measure's is.
    @Test func trackSynthWithAFaderDragOpenIsBusy() async throws {
        let (s, id, m, f) = try demo()
        let mixer = try MixerModel(document: m)
        mixer.dragStarted("Set Fader")
        #expect(m.editor.gesture == nil)
        let busy = "Finish the open edit first."
        for (command, args) in [
            ("lane.track_synth", [("seq", ControlValue.int(value: 3)), ("lane", .int(value: 31))]),
            ("destination.measure_return", [("dest", .int(value: 7))]),
        ] {
            f.synthError = .Busy(message: busy)
            let e = failure(await settled(run(command, args, m, s, id), m))
            #expect(e?.code == .busy && e?.message == busy, "\(command)")
        }
        mixer.dragEnded()
        #expect(f.trackSynthCalls.isEmpty && f.measures.isEmpty)
    }

    /// Task 10 review M2: a destination list that cannot be read is no
    /// verdict on the id; the core, which checks it again, decides.
    @Test func anUnreadableDestinationListLeavesTheCheckToTheCore() async throws {
        let (s, id, m, f) = try demo()
        f.destinationsError = .Internal(message: "no list")
        #expect(failure(await measured([("dest", .int(value: 7))], m, s, id)) == nil)
        #expect(f.measures == [7])
        #expect(
            failure(
                run("destination.set_return_latency", [("dest", .int(value: 7)), ("ms", .text(value: "3"))], m, s, id))
                == nil)
        #expect(f.returnLatencies.last?.dest == 7)
        let lane: [(String, ControlValue)] = [("seq", .int(value: 3)), ("lane", .int(value: 31))]
        #expect(
            failure(
                run("lane.set_input", lane + [("kind", .text(value: "audio")), ("return_of", .int(value: 7))], m, s, id))
                == nil)
        #expect(f.edits.last == .lane(edit: .setInput(seq: 3, lane: 31, input: .audio(first: 0, stereo: false, returnOf: 7))))
        // The core's own refusal is then the answer.
        f.synthError = .Model(message: "Piano is not an external MIDI destination")
        #expect(failure(await measured([("dest", .int(value: 1))], m, s, id))?.code == .failed)
    }

    /// Task 10 review M6: Measure's call is given the time it blocks for
    /// (10 s at most) on top of the app timeout, so one queued behind
    /// other calls is not cut off; other commands get none.
    @Test func measureAsksForTheTimeItBlocks() {
        #expect((SynthCommands.extraTimeMs("destination.measure_return") ?? 0) >= 10_000)
        #expect(SynthCommands.extraTimeMs("lane.track_synth") == nil)
        #expect(SynthCommands.extraTimeMs("destination.set_return_latency") == nil)
    }

    /// Final review finding 3: Measure runs only when its call's timeout
    /// was extended (its client still waits); a call answered `timeout`
    /// while queued is skipped. Other calls run without extending.
    @Test func measureRunsOnlyWhileItsClientWaits() {
        var asked: [UInt64] = []
        #expect(SynthCommands.shouldRun("destination.measure_return") { asked.append($0); return true })
        #expect(asked == [15_000])
        #expect(!SynthCommands.shouldRun("destination.measure_return") { _ in false })
        asked = []
        #expect(SynthCommands.shouldRun("lane.track_synth") { asked.append($0); return false })
        #expect(asked.isEmpty)
    }

    @Test func externalDestinationsAreTheOnesTrackSynthOffers() throws {
        let (_, _, m, _) = try demo()
        #expect(SynthCommands.externalDestinations(m).map(\.id) == [7])
    }

    @Test func theSynthCommandsAreHandled() {
        #expect(SynthCommands.ids.isSubset(of: ControlCommands.handled))
    }
}
