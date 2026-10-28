import Foundation
import Testing
import VizDAWCore
@testable import VizDAWAppModel

/// The audio settings and New MIDI Destination over the control channel
/// (plan 5d-1 Task 12, Rulings 25–26): values of the wrong shape or range
/// are `invalid_params` and change nothing; the host (the app's
/// preferences) gets the rest; the core's refusals are the call's failure.
@MainActor
@Suite struct AudioCommandsTests {
    let host = FakeControlHost()

    func demo() throws -> (DocumentStore, UUID, DocumentModel, FakeSession) {
        let f = demoFake()
        let s = DocumentStore(makeEmpty: { FakeSession([]) }, makeDemo: { f }, open: { _ in demoFake() })
        let id = try #require(s.newDemo())
        return (s, id, try #require(s.model(id)), f)
    }

    func run(_ id: String, _ pairs: [(String, ControlValue)], _ s: DocumentStore, _ doc: UUID?) -> ControlOutcome {
        ControlCommands.run(
            id, args: pairs.map { ControlArg(name: $0.0, value: $0.1) },
            doc: doc.flatMap { d in s.model(d).map { (d, $0) } }, store: s, host: host)
    }

    func failure(_ o: ControlOutcome) -> ControlFailure? {
        if case .failed(let f) = o { return f }
        return nil
    }

    @Test func audioSetChecksItsValues() throws {
        let (s, _, _, _) = try demo()
        // No document needed: the settings are the app's.
        #expect(
            failure(
                run(
                    "audio.set",
                    [
                        ("software_monitoring", .flag(value: false)), ("buffer_frames", .int(value: 256)),
                        ("input_device", .text(value: "Studio In")), ("latency_trim", .int(value: -12)),
                    ], s, nil)) == nil)
        #expect(
            host.audio.last
                == AudioChange(softwareMonitoring: false, bufferFrames: 256, inputDevice: .some("Studio In"), latencyTrim: -12))
        #expect(failure(run("audio.set", [("input_device", .text(value: " "))], s, nil)) == nil)
        #expect(host.audio.last?.inputDevice == .some(nil), "blank is Same as output")
        let before = host.audio.count
        for bad: [(String, ControlValue)] in [
            [], [("buffer_frames", .int(value: 100))], [("buffer_frames", .int(value: 0))],
            [("buffer_frames", .int(value: 2048))], [("buffer_frames", .int(value: -128))],
            [("latency_trim", .int(value: 192_001))], [("latency_trim", .int(value: .min))],
            [("input_device", .text(value: String(repeating: "x", count: 300)))],
            [("input_device", .text(value: "a\nb"))], [("input_device", .text(value: "a\tb"))],
            // 200 graphemes, 400 scalars: over the core's 255.
            [("input_device", .text(value: String(repeating: "e\u{301}", count: 200)))],
            // Wrong types and unknown keys alone change nothing.
            [("buffer_frames", .text(value: "128"))], [("software_monitoring", .int(value: 1))],
            [("junk", .text(value: "x"))],
        ] {
            #expect(failure(run("audio.set", bad, s, nil))?.code == .invalidParams, "\(bad)")
        }
        #expect(host.audio.count == before, "nothing applied")
        // The core refuses a buffer change while recording: busy.
        host.audioError = .Busy(message: "Stop recording to change the I/O buffer size.")
        #expect(failure(run("audio.set", [("buffer_frames", .int(value: 512))], s, nil))?.code == .busy)
    }

    @Test func addMidiDestinationChecksItsValues() throws {
        let (s, id, _, f) = try demo()
        #expect(
            failure(
                run(
                    "destination.add_midi",
                    [("name", .text(value: " Juno ")), ("port", .text(value: "Juno MIDI")), ("channel", .int(value: 3))],
                    s, id)) == nil)
        #expect(f.addedDestinations.last?.name == "Juno" && f.addedDestinations.last?.channel == 2)
        // The port goes to the core as given: a CoreMIDI name may have edge
        // spaces, and the core matches it untrimmed (final wave, Task 11).
        #expect(
            failure(
                run(
                    "destination.add_midi", [("name", .text(value: "J")), ("port", .text(value: " Juno MIDI "))],
                    s, id)) == nil)
        #expect(f.addedDestinations.last?.port == " Juno MIDI ")
        #expect(
            failure(run("destination.add_midi", [("name", .text(value: "A")), ("port", .text(value: "P"))], s, id)) == nil)
        #expect(f.addedDestinations.last?.channel == 0, "channel 1 by default")
        let before = f.addedDestinations.count
        for bad: [(String, ControlValue)] in [
            [("port", .text(value: "P"))], [("name", .text(value: "A"))],
            [("name", .text(value: " ")), ("port", .text(value: "P"))],
            [("name", .text(value: "A")), ("port", .text(value: "  "))],
            [("name", .text(value: "A")), ("port", .text(value: "P")), ("channel", .int(value: 0))],
            [("name", .text(value: "A")), ("port", .text(value: "P")), ("channel", .int(value: 17))],
            [("name", .text(value: String(repeating: "x", count: 256))), ("port", .text(value: "P"))],
            [("name", .text(value: "A\tB")), ("port", .text(value: "P"))],
            [("name", .text(value: "A")), ("port", .text(value: "P\u{7}"))],
            [("name", .text(value: String(repeating: "e\u{301}", count: 200))), ("port", .text(value: "P"))],
        ] {
            #expect(failure(run("destination.add_midi", bad, s, id))?.code == .invalidParams, "\(bad)")
        }
        #expect(f.addedDestinations.count == before)
        f.synthError = .Model(message: "the destination's name is 1 to 255 characters")
        #expect(
            failure(run("destination.add_midi", [("name", .text(value: "A")), ("port", .text(value: "P"))], s, id))?
                .code == .failed)
        #expect(failure(run("destination.add_midi", [("name", .text(value: "A"))], s, nil))?.code == .noDocument)
    }
}
