import Foundation
import Testing
import VizDAWCore

@testable import VizDAWAppModel

/// `file.bounce` over the control channel (plan 5f-2 Task 5; Rulings 2, 3,
/// 10, 11; pre-flight I7, M6): the request as built, hostile arguments as
/// `invalid_params`, and the answer with the files and the warnings.
@MainActor
@Suite struct BounceCommandsTests {
    let host = FakeControlHost()

    func demo() throws -> (DocumentStore, UUID, DocumentModel, FakeSession) {
        let f = demoFake()
        let s = DocumentStore(makeEmpty: { FakeSession([]) }, makeDemo: { f }, open: { _ in demoFake() })
        let id = try #require(s.newDemo())
        return (s, id, try #require(s.model(id)), f)
    }

    func run(_ pairs: [(String, ControlValue)], _ m: DocumentModel, _ s: DocumentStore, _ doc: UUID)
        -> ControlOutcome
    {
        ControlCommands.run(
            BounceCommands.id, args: pairs.map { ControlArg(name: $0.0, value: $0.1) }, doc: (doc, m), store: s,
            host: host)
    }

    func failure(_ o: ControlOutcome) -> ControlFailure? {
        if case .failed(let f) = o { return f }
        return nil
    }

    @Test func bounceCommandsCheckTheirArguments() throws {
        let (s, id, m, f) = try demo()
        let wav: (String, ControlValue) = ("path", .text(value: "/tmp/x.wav"))
        let folder: (String, ControlValue) = ("path", .text(value: "/tmp/stems"))
        let stems: (String, ControlValue) = ("source", .text(value: "stems"))
        let selection: (String, ControlValue) = ("range", .text(value: "selection"))
        for args: [(String, ControlValue)] in [
            [("path", .text(value: "/tmp/x.aif"))],
            [wav, ("format", .text(value: "caf"))],
            [stems, folder],
            [stems, folder, ("strips", .text(value: "1,,2"))],
            [stems, folder, ("strips", .text(value: "1, 2"))],
            [stems, folder, ("strips", .text(value: "-1"))],
            [stems, folder, ("strips", .text(value: "a,b"))],
            [stems, folder, ("strips", .text(value: "1,1"))],
            // More malformed strip lists (Task 5 dispatch).
            [stems, folder, ("strips", .text(value: ""))],
            [stems, folder, ("strips", .text(value: "1,"))],
            [stems, folder, ("strips", .text(value: ",1"))],
            [stems, folder, ("strips", .text(value: "+1"))],
            [stems, folder, ("strips", .text(value: "99999999999999999999999"))],
            [stems, folder, ("strips", .text(value: "١"))],
            [wav, selection, ("start", .int(value: 0))],
            [wav, selection, ("start", .int(value: 100)), ("end", .int(value: 100))],
            [wav, ("source", .text(value: "selection")), ("range", .text(value: "song"))],
            [wav, ("start", .int(value: 0)), ("end", .int(value: 100))],
            [wav, selection],
        ] {
            #expect(failure(run(args, m, s, id))?.code == .invalidParams, "\(args)")
        }
        let d = failure(run([("add_to_project", .flag(value: false))], m, s, id))
        #expect(d?.code == .dialogRequired && d?.dialog == "Bounce", "add_to_project false alone needs the dialog")
        #expect(f.bounces.isEmpty, "nothing reached the core")
    }

    @Test func bounceStartsAJobAndAnswersWithTheFiles() throws {
        let (s, id, m, f) = try demo()
        f.info.format = ProjectFormatRow(sampleRate: 44_100, depth: .int16)
        let o = run([("path", .text(value: "/tmp/x.wav"))], m, s, id)
        guard case .job(let command, let doc, let job) = o else {
            Issue.record("not a job: \(o)")
            return
        }
        #expect(command == "file.bounce" && doc == id)
        let seq = try #require(m.selectedSeq)
        #expect(
            f.bounces.last
                == BounceRequestRow(
                    seq: seq, source: .mix, range: .song, container: .wav, depth: .int16, rate: 44_100, dither: true,
                    path: "/tmp/x.wav", addToProject: false, replace: false),
            "the project's rate and depth; dither on at 16 bits")
        _ = run(
            [
                ("source", .text(value: "stems")), ("strips", .text(value: "2,1")), ("path", .text(value: "/tmp/stems")),
                ("format", .text(value: "aiff")), ("bits", .text(value: "32f")), ("rate", .int(value: 96_000)),
                ("dither", .flag(value: true)), ("range", .text(value: "selection")), ("start", .int(value: 0)),
                ("end", .int(value: 15_360)), ("add_to_project", .flag(value: true)), ("replace", .flag(value: true)),
            ], m, s, id)
        let all = try #require(f.bounces.last)
        #expect(all.source == .stems(strips: [2, 1]) && all.range == .ticks(start: 0, end: 15_360))
        #expect(all.container == .aiff && all.depth == .float32 && all.rate == 96_000 && !all.dither, "float ignores dither")
        #expect(all.addToProject && all.replace)
        _ = run([("source", .text(value: "selection")), wavArg(), ("start", .int(value: 10)), ("end", .int(value: 20))], m, s, id)
        #expect(f.bounces.last?.range == .ticks(start: 10, end: 20) && f.bounces.last?.source == .mix)
        // Ruling 10's answer; a bounce with problems is ok, with warnings (pre-flight I7).
        let file = BouncedFileRow(path: "/tmp/x.wav", frames: 96_000, rate: 48_000, bits: "24-bit", channels: 2, clipped: 0)
        var done = JobRow(id: job, kind: .bounce, progress: 1, state: .done, message: nil, problems: [], created: [])
        #expect(
            ControlJobs.answer(done, bounce: BounceResultRow(files: [file], media: []))
                == .ok(status: "wrote /tmp/x.wav (96000 frames, 48000 Hz, 24-bit)"))
        done.problems = ["x.wav: 12 samples clipped"]
        #expect(
            ControlJobs.answer(done, bounce: BounceResultRow(files: [file], media: [7]))
                == .ok(status: "wrote /tmp/x.wav (96000 frames, 48000 Hz, 24-bit); added 1 media; warnings: x.wav: 12 samples clipped"))
        let imported = JobRow(id: 1, kind: .import, progress: 1, state: .done, message: nil, problems: ["a.wav: unreadable"], created: [])
        #expect(ControlJobs.answer(imported) == .failed("a.wav: unreadable"), "an import that made nothing still fails")
        // The core's refusals are the call's failure.
        f.bounceError = VizError.Model(message: "x.wav exists; choose another name or replace it")
        #expect(failure(run([wavArg()], m, s, id))?.code == .failed)
        f.bounceError = VizError.Busy(message: "A bounce is running.")
        #expect(failure(run([wavArg()], m, s, id))?.code == .busy)
    }

    func wavArg() -> (String, ControlValue) { ("path", .text(value: "/tmp/x.wav")) }
}
