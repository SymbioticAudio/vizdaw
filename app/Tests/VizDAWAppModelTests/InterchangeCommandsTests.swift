import Foundation
import Testing
import VizDAWCore

@testable import VizDAWAppModel

/// The MIDI import and export commands (plan 6a Task 8; F2, F13): the
/// requests as built, hostile arguments as `invalid_params` with no session
/// call, `dry_run` writing nothing, and no dialog ever opened.
@MainActor
@Suite struct InterchangeCommandsTests {
    let host = FakeControlHost()

    func demo() throws -> (DocumentStore, UUID, DocumentModel, FakeSession) {
        let f = demoFake()
        let s = DocumentStore(makeEmpty: { FakeSession([]) }, makeDemo: { f }, open: { _ in demoFake() })
        let id = try #require(s.newDemo())
        return (s, id, try #require(s.model(id)), f)
    }

    func run(_ cmd: String, _ pairs: [(String, ControlValue)], _ m: DocumentModel, _ s: DocumentStore, _ doc: UUID)
        -> ControlOutcome
    {
        ControlCommands.run(
            cmd, args: pairs.map { ControlArg(name: $0.0, value: $0.1) }, doc: (doc, m), store: s, host: host)
    }

    func failure(_ o: ControlOutcome) -> ControlFailure? {
        if case .failed(let f) = o { return f }
        return nil
    }

    func calls(_ f: FakeSession) -> Int {
        f.midiImports.count + f.smfExports.count + f.clipExports.count + f.smfChecks.count + f.clipChecks.count
    }

    @Test func importMidiBuildsTheRequest() throws {
        let (s, id, m, f) = try demo()
        let map = "[{\"group\":0,\"channel\":9,\"dest\":null}]"
        let o = run(
            InterchangeCommands.importMidi,
            [
                ("path", .text(value: "/tmp/a.mid")), ("tick", .int(value: 7680)), ("lane", .int(value: 2)),
                ("channel_map", .text(value: map)), ("mpe", .flag(value: true)),
            ], m, s, id)
        guard case .job(let command, let doc, _) = o else { Issue.record("not a job: \(o)"); return }
        #expect(command == "file.import_midi" && doc == id)
        let seq = try #require(m.selectedSeq)
        #expect(
            f.midiImports.last
                == MidiImportRequestRow(
                    path: "/tmp/a.mid", newProject: false, seq: seq, lane: 2, tick: 7680,
                    channelMap: [ChannelMapRow(group: 0, channel: 9, dest: nil)], splitChannels: nil, mpe: true,
                    useTempo: false))
        _ = run(
            InterchangeCommands.importMidi,
            [
                ("path", .text(value: "/tmp/b.mid")), ("seq", .int(value: 1)), ("split_channels", .flag(value: false)),
                ("use_tempo", .flag(value: true)),
                ("channel_map", .text(value: "[{\"group\":15,\"channel\":0,\"dest\":4},{\"group\":1,\"channel\":0}]")),
            ], m, s, id)
        #expect(
            f.midiImports.last
                == MidiImportRequestRow(
                    path: "/tmp/b.mid", newProject: false, seq: 1, lane: nil, tick: 0,
                    channelMap: [
                        ChannelMapRow(group: 15, channel: 0, dest: 4), ChannelMapRow(group: 1, channel: 0, dest: nil),
                    ], splitChannels: false, mpe: false, useTempo: true))
    }

    @Test func importMidiChecksItsArguments() throws {
        let (s, id, m, f) = try demo()
        let path: (String, ControlValue) = ("path", .text(value: "/tmp/a.mid"))
        func map(_ t: String) -> [(String, ControlValue)] { [path, ("channel_map", .text(value: t))] }
        let one = "{\"group\":0,\"channel\":9,\"dest\":null}"
        let imports: [[(String, ControlValue)]] = [
            map("not json"), map("{\"group\":0}"), map("7"), map("[7]"), map("[\(one),\(one)]"),
            map("[{\"group\":0,\"channel\":16,\"dest\":null}]"), map("[{\"group\":-1,\"channel\":0,\"dest\":null}]"),
            map("[{\"group\":0,\"channel\":0,\"dest\":1.5}]"), map("[{\"group\":0,\"channel\":0,\"dest\":\"3\"}]"),
            map("[{\"group\":0,\"channel\":0,\"dest\":-3}]"), map("[{\"group\":0,\"channel\":0,\"dest\":true}]"),
            map("[{\"group\":true,\"channel\":0,\"dest\":null}]"), map("[{\"group\":0.5,\"channel\":0,\"dest\":null}]"),
            map("[{\"group\":0,\"dest\":null}]"), map("[{\"group\":0,\"channel\":0,\"x\":1}]"), map(""),
            map("[{\"group\":0,\"channel\":0,\"dest\":1e400}]"),
            map("[" + (0..<300).map { "{\"group\":\($0 % 16),\"channel\":\($0 / 16 % 16),\"dest\":null}" }.joined(separator: ",") + "]"),
            map("[" + String(repeating: "[", count: 5000)),
        ]
        for args in imports {
            let f = failure(run(InterchangeCommands.importMidi, args, m, s, id))
            #expect(f?.code == .invalidParams && f?.message.contains("file.import_midi") == true, "\(args)")
            #expect(f?.message.contains("channel_map") == true, "\(args)")
        }
        let exports: [(String, [(String, ControlValue)], String)] = [
            (InterchangeCommands.exportMidi, [("lanes", .text(value: "1,,2"))], "lanes"),
            (InterchangeCommands.exportMidi, [("lanes", .text(value: "a"))], "lanes"),
            (InterchangeCommands.exportMidi, [("lanes", .text(value: "1,1"))], "lanes"),
            (InterchangeCommands.exportMidi, [("lanes", .text(value: "-1"))], "lanes"),
            (InterchangeCommands.exportMidi, [("lanes", .text(value: ""))], "lanes"),
            (InterchangeCommands.exportMidi, [("lanes", .text(value: "99999999999999999999999"))], "lanes"),
            (InterchangeCommands.exportMidi, [("lanes", .text(value: "١"))], "lanes"),
            (InterchangeCommands.exportMidi, [("seq", .int(value: -1))], "seq"),
            (InterchangeCommands.exportClip, [("lanes", .text(value: "1,,2"))], "lanes"),
            (InterchangeCommands.exportClip, [("lanes", .text(value: "3,3"))], "lanes"),
            (InterchangeCommands.exportClip, [("start", .int(value: 0))], "start"),
            (InterchangeCommands.exportClip, [("start", .int(value: 100)), ("end", .int(value: 100))], "start"),
            (InterchangeCommands.exportClip, [("start", .int(value: 200)), ("end", .int(value: 100))], "start"),
        ]
        for (cmd, args, name) in exports {
            for extra: [(String, ControlValue)] in [[("path", .text(value: "/tmp/a.mid"))], [("dry_run", .flag(value: true))]] {
                let f = failure(run(cmd, args + extra, m, s, id))
                #expect(f?.code == .invalidParams && f?.message.contains(cmd) == true && f?.message.contains(name) == true, "\(cmd) \(args)")
            }
        }
        #expect(calls(f) == 0, "nothing reached the core")
    }

    @Test func dryRunWritesNothing() throws {
        let (s, id, m, f) = try demo()
        f.interchangeCheck = ReportRow(summary: "Would export 2 tracks", items: [])
        let seq = try #require(m.selectedSeq)
        for cmd in [InterchangeCommands.exportMidi, InterchangeCommands.exportClip] {
            for withPath in [false, true] {
                let args: [(String, ControlValue)] =
                    [("dry_run", .flag(value: true))] + (withPath ? [("path", .text(value: "/tmp/a.mid"))] : [])
                guard case .done(let r) = run(cmd, args, m, s, id) else { Issue.record("not done: \(cmd) \(args)"); return }
                guard case .interchange(let command, let doc, let window, _, let report, let written) = r else {
                    Issue.record("not an interchange result: \(r)"); return
                }
                #expect(command == cmd && doc == id.uuidString && window == nil)
                #expect(report?.summary == "Would export 2 tracks" && written.isEmpty)
            }
        }
        #expect(f.smfChecks.count == 2 && f.clipChecks.count == 2)
        #expect(f.smfExports.isEmpty && f.clipExports.isEmpty && f.midiImports.isEmpty, "nothing was written")
        #expect(f.smfChecks.first?.seq == seq && f.smfChecks.first?.type0 == false && f.smfChecks.first?.ppq == 960)
        #expect(f.smfChecks.first?.lanes == [30, 31], "every track by default")
        #expect(f.clipChecks.first?.lanes == [30, 31] && f.clipChecks.first?.start == nil && f.clipChecks.first?.end == nil)
    }

    @Test func exportWithoutPathAsksForSave() throws {
        let (s, id, m, f) = try demo()
        for cmd in [InterchangeCommands.exportMidi, InterchangeCommands.exportClip] {
            let d = failure(run(cmd, [], m, s, id))
            #expect(d?.code == .dialogRequired && d?.dialog == "Save", "\(cmd)")
            #expect(d?.message.contains("`path`") == true, "\(cmd)")
            let d2 = failure(run(cmd, [("dry_run", .flag(value: false))], m, s, id))
            #expect(d2?.code == .dialogRequired && d2?.dialog == "Save", "\(cmd) dry_run false")
        }
        let open = failure(run(InterchangeCommands.importMidi, [], m, s, id))
        #expect(open?.code == .dialogRequired && open?.dialog == "Open")
        #expect(calls(f) == 0)
    }

    @Test func exportsBuildTheirRequests() throws {
        let (s, id, m, f) = try demo()
        let seq = try #require(m.selectedSeq)
        guard
            case .job(let c1, _, _) = run(
                InterchangeCommands.exportMidi,
                [
                    ("path", .text(value: "/tmp/a.mid")), ("type", .text(value: "0")), ("ppq", .text(value: "15360")),
                    ("lanes", .text(value: "31,30")), ("mpe", .flag(value: true)), ("replace", .flag(value: true)),
                ], m, s, id),
            case .job(let c2, _, _) = run(
                InterchangeCommands.exportClip,
                [("path", .text(value: "/tmp/dir")), ("start", .int(value: 0)), ("end", .int(value: 960)), ("seq", .int(value: 2))],
                m, s, id)
        else { Issue.record("not jobs"); return }
        #expect(c1 == "file.export_midi" && c2 == "file.export_midi_clip")
        #expect(
            f.smfExports.last
                == SmfExportRequestRow(
                    seq: seq, lanes: [31, 30], path: "/tmp/a.mid", replace: true, type0: true, ppq: 15360, mpe: true))
        #expect(
            f.clipExports.last
                == ClipExportRequestRow(
                    seq: 2, lanes: [20, 21], path: "/tmp/dir", replace: false, start: 0, end: 960))
    }

    @Test func newProjectMakesADocument() throws {
        let (s, id, m, _) = try demo()
        let fresh = FakeSession([seq(9, "New", topLevel: true)])
        var formats: [ProjectFormatRow] = []
        s.makeProject = { f in
            formats.append(f)
            return fresh
        }
        s.defaultFormat = ProjectFormatRow(sampleRate: 96_000, depth: .int16)
        let before = s.documents.count
        let o = run(
            InterchangeCommands.importMidi, [("path", .text(value: "/tmp/a.mid")), ("new_project", .flag(value: true))],
            m, s, id)
        guard case .job(let command, let doc, _) = o else { Issue.record("not a job: \(o)"); return }
        #expect(command == "file.import_midi" && doc != id)
        #expect(s.documents.count == before + 1 && s.model(doc) != nil)
        #expect(formats == [ProjectFormatRow(sampleRate: 48_000, depth: .int16)], "48 kHz at the default depth")
        #expect(fresh.midiImports.count == 1 && fresh.midiImports.first?.newProject == true)
        #expect(fresh.midiImports.first?.seq == s.model(doc)?.selectedSeq)
    }

    @Test func aRefusedNewProjectImportMakesNoProject() throws {
        let (s, id, m, f) = try demo()
        let fresh = FakeSession([seq(9, "New", topLevel: true)])
        var made = 0
        s.makeProject = { _ in
            made += 1
            return fresh
        }
        let before = s.documents.count
        let path: (String, ControlValue) = ("path", .text(value: "/tmp/a.mid"))
        let new: (String, ControlValue) = ("new_project", .flag(value: true))
        // The file is read first: a refusal leaves nothing behind.
        f.interchangeError = VizError.Model(message: "not a MIDI file")
        let r = failure(run(InterchangeCommands.importMidi, [path, new], m, s, id))
        #expect(r?.code == .failed && r?.message.contains("not a MIDI file") == true)
        f.interchangeError = nil
        #expect(s.documents.count == before && made == 0)
        // A drop track cannot exist in a new project.
        let l = failure(run(InterchangeCommands.importMidi, [path, new, ("lane", .int(value: 2))], m, s, id))
        #expect(l?.code == .invalidParams && l?.message.contains("lane") == true)
        #expect(s.documents.count == before && made == 0 && f.midiImports.isEmpty)
        // A new project needs no sequence shown in this one.
        let bare = DocumentStore(makeEmpty: { FakeSession([]) }, makeDemo: { FakeSession([]) }, open: { _ in demoFake() })
        bare.makeProject = { _ in fresh }
        let bid = try #require(bare.newEmpty())
        let bm = try #require(bare.model(bid))
        #expect(bm.selectedSeq == nil)
        guard case .job = run(InterchangeCommands.importMidi, [path, new], bm, bare, bid) else {
            Issue.record("not a job")
            return
        }
        #expect(fresh.midiImports.first?.seq == 9)
        let none = failure(run(InterchangeCommands.importMidi, [path], bm, bare, bid))
        #expect(none?.code == .failed)
    }

    @Test func aFailedImportClosesTheProjectItMade() throws {
        let (s, id, m, _) = try demo()
        let fresh = FakeSession([seq(9, "New", topLevel: true)])
        fresh.jobStartState = .failed
        s.makeProject = { _ in fresh }
        let before = s.documents.count
        let args: [(String, ControlValue)] = [("path", .text(value: "/tmp/a.mid")), ("new_project", .flag(value: true))]
        guard case .job(_, let doc, _) = run(InterchangeCommands.importMidi, args, m, s, id) else {
            Issue.record("not a job")
            return
        }
        #expect(s.documents.count == before + 1)
        InterchangeCommands.jobEnded(doc, failed: true, store: s) { s.close($0) }
        #expect(s.documents.count == before && s.model(doc) == nil)
        // A successful job keeps it.
        guard case .job(_, let ok, _) = run(InterchangeCommands.importMidi, args, m, s, id) else { return }
        InterchangeCommands.jobEnded(ok, failed: false, store: s) { s.close($0) }
        #expect(s.model(ok) != nil)
        // Not a project this command made: left alone.
        InterchangeCommands.jobEnded(id, failed: true, store: s) { s.close($0) }
        #expect(s.model(id) != nil)
    }

    @Test func importRefusalsNeverReachTheSession() throws {
        let (s, id, m, f) = try demo()
        #expect(m.editor.beginGesture("Move", lanes: [], origin: .other))
        let g = failure(run(InterchangeCommands.importMidi, [("path", .text(value: "/tmp/a.mid"))], m, s, id))
        #expect(g?.code == .unavailable)
        #expect(f.midiImports.isEmpty)
        f.interchangeError = VizError.Model(message: "bad seq")
        let b = failure(run(InterchangeCommands.exportClip, [("path", .text(value: "/tmp/d")), ("seq", .int(value: 99))], m, s, id))
        #expect(b?.code == .failed, "a lanes failure is not an empty list")
        #expect(f.clipExports.isEmpty)
    }

    @Test func reportCommandCallsTheSessionOnce() throws {
        let (s, id, m, f) = try demo()
        f.lastReport = ReportRow(summary: "Imported 2 tracks", items: [])
        guard case .done(let r) = run(InterchangeCommands.report, [], m, s, id),
            case .interchange(let command, let doc, let window, _, let report, let written) = r
        else { Issue.record("not an interchange result"); return }
        #expect(command == "file.interchange_report" && doc == id.uuidString && window == nil)
        #expect(report?.summary == "Imported 2 tracks" && written.isEmpty)
        #expect(f.interchangeReportCalls == 1 && calls(f) == 0)
        f.lastReport = nil
        guard case .done(.interchange(_, _, _, _, let none, _)) = run(InterchangeCommands.report, [], m, s, id) else {
            Issue.record("not done"); return
        }
        #expect(none == nil, "no report yet")
        #expect(f.interchangeReportCalls == 2)
    }

    @Test func aFinishedJobAnswersWithItsSummary() {
        func row(_ k: JobKindRow, _ state: JobStateRow, created: [UInt64] = [], problems: [String] = []) -> JobRow {
            JobRow(
                id: 1, kind: k, progress: 1, state: state, message: "boom", problems: problems, created: created,
                report: ReportRow(summary: "Imported 1 track", items: []), written: ["/tmp/a.mid"])
        }
        #expect(ControlJobs.answer(row(.importMidi, .done)) == .ok(status: "Imported 1 track"))
        #expect(ControlJobs.answer(row(.exportMidi, .done, problems: ["x"])) == .ok(status: "Imported 1 track"))
        #expect(ControlJobs.answer(row(.importMidi, .failed)) == .failed("boom"))
        #expect(ControlJobs.answer(row(.exportMidi, .cancelled)) == .failed("boom; already written: /tmp/a.mid"))
    }
}
