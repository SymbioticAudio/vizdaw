import Foundation
import Testing
@testable import VizDAWAppModel
import VizDAWCore

/// Co-Producer calls as the app runs them (plan 8a Ruling 13): every one
/// that is not a read runs between enter and leave, on the main thread, in
/// one stretch.
@MainActor
@Suite struct CoProducerCommandsTests {
    let host = FakeControlHost()
    let stamp = "2026-11-14 15-02"

    /// A store with one demo document (Song selected) and its fake session.
    func demo() throws -> (DocumentStore, UUID, DocumentModel, FakeSession) {
        let f = demoFake()
        let s = DocumentStore(makeEmpty: { FakeSession([]) }, makeDemo: { f }, open: { _ in demoFake() })
        let id = try #require(s.newDemo())
        let m = try #require(s.model(id))
        return (s, id, m, f)
    }

    func run(_ r: ControlRequest, _ s: DocumentStore, _ id: UUID) -> ControlOutcome {
        // FakeApprover(.allow) grants VizDAW's own approval as the sheet's
        // Allow would (plan 8b Task 6).
        CoProducerCommands.run(
            r, doc: s.model(id).map { (id, $0) }, store: s, host: host, stamp: stamp, peer: nil,
            approver: FakeApprover(.allow))
    }

    func command(_ id: String, _ args: [ControlArg] = []) -> ControlRequest {
        .runCommand(id: id, args: args, window: nil)
    }

    func failure(_ o: ControlOutcome) -> ControlFailure? {
        if case .failed(let f) = o { return f }
        return nil
    }

    func result(_ o: ControlOutcome) -> ControlResult? {
        if case .done(let r) = o { return r }
        return nil
    }

    func int(_ name: String, _ v: Int64) -> ControlArg { ControlArg(name: name, value: .int(value: v)) }

    var renameThenDelete: [PlannedRow] {
        [
            PlannedRow(
                id: "track.rename",
                args: [int("seq", 3), int("lane", 30), ControlArg(name: "name", value: .text(value: "Lead"))]),
            PlannedRow(id: "track.delete", args: [int("seq", 3), int("lane", 31)]),
        ]
    }

    @Test func commandsRunBetweenEnterAndLeave() throws {
        let (s, id, _, f) = try demo()
        let o = run(command("track.new_audio"), s, id)
        #expect(failure(o) == nil)
        #expect(f.coLog == ["enter \(stamp)", "newTrack audio", "leave"])

        // A read enters nothing.
        f.coLog = []
        _ = run(command("transport.stop"), s, id)
        #expect(f.coLog == [])

        // A command that fails after enter still leaves.
        f.newTrackError = .Model(message: "no room")
        let failed = run(command("track.new_audio"), s, id)
        #expect(failure(failed)?.message == "no room")
        #expect(f.coLog == ["enter \(stamp)", "newTrack audio", "leave"])
    }

    @Test func noRequestNoChange() throws {
        let (s, id, _, f) = try demo()
        f.coEnterError = .Model(message: "Open a request first (begin_request).")
        let o = run(command("track.new_audio"), s, id)
        #expect(failure(o)?.code == .failed)
        #expect(failure(o)?.message == "Open a request first (begin_request).")
        #expect(f.coLog == ["enter \(stamp)"])
        #expect(f.newTracks.isEmpty)
        // Busy stays busy.
        f.coEnterError = .Busy(message: "Finish the drag first.")
        #expect(failure(run(command("track.new_audio"), s, id))?.code == .busy)
    }

    /// The Task 4 rule: enter, take, the plan, leave, in one stretch.
    @Test func applyRunsThePlanAndReportsEveryCommand() throws {
        let (s, id, _, f) = try demo()
        let token = String(repeating: "b", count: 32)
        let o = run(.coProducerApply(token: token, plan: renameThenDelete, window: nil, client: nil), s, id)
        #expect(f.coLog == ["enter \(stamp)", "take", "rename", "delete", "leave"])
        guard case .coProducerApplied(let doc, let results, _) = result(o) else {
            Issue.record("not applied: \(o)")
            return
        }
        #expect(doc == id.uuidString)
        #expect(results == ["track.rename: done", "track.delete: done"])
        // The preview needs a yes: the take had the sheet's approval.
        #expect(f.coTakeApprovals == [.sheet])

        // A failed command stops the rest; each has its line.
        f.coLog = []
        f.editError = .Model(message: "the name is refused")
        let failed = run(.coProducerApply(token: token, plan: renameThenDelete, window: nil, client: nil), s, id)
        guard case .coProducerApplied(_, let lines, _) = result(failed) else {
            Issue.record("not applied: \(failed)")
            return
        }
        #expect(lines == ["track.rename: the name is refused", "track.delete: not run (an earlier command failed)"])
        #expect(f.coLog == ["enter \(stamp)", "take", "rename", "leave"])

        // A refused take runs nothing, and still leaves.
        f.coLog = []
        f.editError = nil
        f.coTakeError = .Model(message: "This preview was already used or replaced.")
        let refused = run(.coProducerApply(token: token, plan: renameThenDelete, window: nil, client: nil), s, id)
        #expect(failure(refused)?.message == "This preview was already used or replaced.")
        #expect(f.coLog == ["enter \(stamp)", "take", "leave"])

        // No request: nothing is taken.
        f.coLog = []
        f.coEnterError = .Model(message: "Open a request first (begin_request).")
        let none = run(.coProducerApply(token: token, plan: renameThenDelete, window: nil, client: nil), s, id)
        #expect(failure(none)?.message == "Open a request first (begin_request).")
        #expect(f.coLog == ["enter \(stamp)"])
    }

    @Test func beginEndAndPreviewPassThrough() throws {
        let (s, id, _, f) = try demo()
        guard case .coProducerRequest(_, let req?, _) = result(run(.coProducerBegin(name: "drum bus", asked: nil, window: nil), s, id))
        else {
            Issue.record("begin did not answer its request")
            return
        }
        #expect(req.label == "Co-Producer: drum bus")
        guard case .coProducerRequest(_, let open?, _) = result(run(.coProducerStatus(window: nil), s, id)) else {
            Issue.record("status did not answer the open request")
            return
        }
        #expect(open.name == "drum bus")
        guard case .coProducerPreview(_, let p) = result(run(.coProducerPreview(plan: renameThenDelete, window: nil), s, id))
        else {
            Issue.record("preview did not answer")
            return
        }
        #expect(p == f.coPreview)
        guard case .coProducerRequest(_, nil, _) = result(run(.coProducerEnd(window: nil), s, id)) else {
            Issue.record("end did not answer")
            return
        }
        #expect(f.coLog == ["begin drum bus", "stamp \(stamp)", "preview 2", "end"])
        // The session's refusals pass on, a blank name's included.
        f.coBeginError = .Model(message: "Name the request.")
        #expect(failure(run(.coProducerBegin(name: "", asked: nil, window: nil), s, id))?.message == "Name the request.")
        f.coPreviewError = .Busy(message: "Finish the drag first.")
        #expect(failure(run(.coProducerPreview(plan: renameThenDelete, window: nil), s, id))?.code == .busy)
        #expect(failure(run(.coProducerEnd(window: nil), s, id))?.message == "No request is open.")
    }

    func text(_ name: String, _ v: String) -> ControlArg { ControlArg(name: name, value: .text(value: v)) }

    /// Plan 8b Task 5: `asked` follows the begin, after the request's stamp.
    @Test func beginPassesAsked() throws {
        let (s, id, _, f) = try demo()
        let o = run(.coProducerBegin(name: "drum bus", asked: "put my drums on a bus", window: nil), s, id)
        guard case .coProducerRequest(_, let req?, _) = result(o) else {
            Issue.record("begin did not answer its request: \(o)")
            return
        }
        #expect(req.name == "drum bus")
        #expect(f.coLog == ["begin drum bus", "stamp \(stamp)", "asked put my drums on a bus"])
        // No `asked`: none is set.
        _ = try f.coProducerEnd()
        f.coLog = []
        _ = run(.coProducerBegin(name: "strings", asked: nil, window: nil), s, id)
        #expect(f.coLog == ["begin strings", "stamp \(stamp)"])
        // A refused begin sets nothing.
        _ = try f.coProducerEnd()
        f.coLog = []
        f.coBeginError = .Model(message: "Name the request.")
        _ = run(.coProducerBegin(name: "", asked: "x", window: nil), s, id)
        #expect(f.coLog == ["begin "])
        // A refused `asked` is the call's failure, in the session's words.
        f.coBeginError = nil
        f.coLog = []
        f.coNoteError = .Model(message: "What was asked is over 500 characters.")
        let refused = run(.coProducerBegin(name: "pads", asked: "x", window: nil), s, id)
        #expect(failure(refused)?.message == "What was asked is over 500 characters.")
    }

    /// Plan 8d Task 5: `co_producer.plugins` answers the list the core
    /// gives for its kind, and `truncated`.
    @Test func pluginsAnswerTheList() throws {
        let (s, id, _, _) = try demo()
        var asked: [PluginKindTag?] = []
        let rows = [
            CoProducerPluginRow(plugin: "multi-q", name: "Multi-Q", maker: "VizDAW", format: "clap", kind: .effect, version: "1")
        ]
        host.coProducer.pluginList = { kind in
            asked.append(kind)
            return CoProducerPluginsRow(plugins: rows, truncated: true)
        }
        guard case .coProducerPlugins(let plugins, let truncated) = result(run(.coProducerPlugins(kind: .effect), s, id))
        else {
            Issue.record("no plug-ins")
            return
        }
        #expect(plugins == rows)
        #expect(truncated)
        #expect(asked == [.effect])
        _ = run(.coProducerPlugins(kind: nil), s, id)
        #expect(asked == [.effect, nil])
    }

    /// `co_producer.params` is the resolved document's
    /// `coProducerParams(slot:)`; its refusal is `failed` in its words.
    @Test func paramsAnswerForTheDocument() throws {
        let (s, id, _, f) = try demo()
        let p = ParamRow(
            id: 3, name: "Gain", module: "", min: 0, max: 1, default: 0.5, value: 0.5, stepped: false,
            automatable: true, hidden: false)
        f.paramTable[7] = [p]
        guard case .coProducerParams(let doc, let row) = result(run(.coProducerParams(slot: 7, window: nil), s, id))
        else {
            Issue.record("no parameters")
            return
        }
        #expect(doc == id.uuidString)
        #expect(row.slot == 7)
        #expect(row.params == [p])
        let refused = run(.coProducerParams(slot: 8, window: nil), s, id)
        #expect(failure(refused)?.code == .failed)
        #expect(failure(refused)?.message == "This song has no slot 8.")
        f.readBusy = true
        #expect(failure(run(.coProducerParams(slot: 7, window: nil), s, id))?.code == .busy)
        // No document: no_document.
        let none = CoProducerCommands.run(
            .coProducerParams(slot: 7, window: nil), doc: nil, store: s, host: host, stamp: stamp, peer: nil,
            approver: FakeApprover(.allow))
        #expect(failure(none)?.code == .noDocument)
    }

    /// The two reads enter nothing.
    @Test func readsEnterNothing() throws {
        let (s, id, _, f) = try demo()
        f.paramTable[7] = []
        host.coProducer.pluginList = { _ in CoProducerPluginsRow(plugins: [], truncated: false) }
        for r in [
            ControlRequest.coProducerPlugins(kind: nil), .coProducerPlugins(kind: .effect),
            .coProducerPlugins(kind: .instrument), .coProducerParams(slot: 7, window: nil),
            .coProducerParams(slot: 4, window: nil),
        ] {
            _ = run(r, s, id)
        }
        #expect(f.coLog.isEmpty)
        #expect(f.mixerRuns.isEmpty)
    }

    func mixerPlan(_ id: String = "mixer.set_strip") -> [PlannedRow] {
        [PlannedRow(id: id, args: [int("strip", 1), ControlArg(name: "gain_db", value: .text(value: "-3"))])]
    }

    /// Plan 8d Task 5: a failing `runMixer` reached through
    /// `co_producer.apply` is its line, and no sheet opens (8a Ruling 13).
    @Test func noSheetsForMixerEdits() throws {
        let (s, id, m, f) = try demo()
        f.mixerError = .Model(message: "Strip 9 is not in this song.")
        let token = String(repeating: "c", count: 32)
        let o = run(.coProducerApply(token: token, plan: mixerPlan() + mixerPlan("mixer.remove_bus"), window: nil, client: nil), s, id)
        guard case .coProducerApplied(_, let lines, _) = result(o) else {
            Issue.record("not applied: \(o)")
            return
        }
        #expect(lines == [
            "mixer.set_strip: Strip 9 is not in this song.", "mixer.remove_bus: not run (an earlier command failed)",
        ])
        #expect(m.alert == nil)
        #expect(m.coProducerApproval == nil)
    }

    /// Plan 8d Task 5 (Task 4's carry): a mixer edit runs only after a
    /// take. `command.run` of one is refused before enter; through
    /// `co_producer.apply` it runs between the take and leave; a refused
    /// take or the composer's no runs nothing.
    @Test func mixerEditsRunOnlyAfterATake() throws {
        let (s, id, m, f) = try demo()
        let strip = int("strip", 1)
        for edit in MixerCommands.coProducerEdits where edit != "track.new_plugin_instrument" {
            let o = run(command(edit, [strip]), s, id)
            #expect(failure(o)?.code == .notAllowed, "\(edit): \(o)")
        }
        // A new bus fed from a strip changes that strip: a change.
        #expect(failure(run(command(MixerCommands.newBus, [strip]), s, id))?.code == .notAllowed)
        #expect(f.coLog.isEmpty)
        #expect(f.mixerRuns.isEmpty)
        #expect(f.calls.isEmpty)

        // A new plug-in instrument track adds: it runs between enter and leave.
        let added = run(
            command("track.new_plugin_instrument", [ControlArg(name: "plugin", value: .text(value: "built-in-synth"))]),
            s, id)
        #expect(failure(added) == nil, "\(added)")
        #expect(f.coLog == ["enter \(stamp)", "runMixer track.new_plugin_instrument", "leave"])

        // After the take.
        f.coLog = []
        let token = String(repeating: "d", count: 32)
        let applied = run(.coProducerApply(token: token, plan: mixerPlan(), window: nil, client: nil), s, id)
        guard case .coProducerApplied(_, let lines, _) = result(applied) else {
            Issue.record("not applied: \(applied)")
            return
        }
        #expect(lines == ["mixer.set_strip: done"])
        #expect(f.coLog == ["enter \(stamp)", "take", "runMixer mixer.set_strip", "leave"])

        // A refused take runs nothing.
        f.coLog = []
        f.coTakeError = .Model(message: "This preview was already used or replaced.")
        _ = run(.coProducerApply(token: token, plan: mixerPlan(), window: nil, client: nil), s, id)
        #expect(f.coLog == ["enter \(stamp)", "take", "leave"])
        f.coTakeError = nil

        // The composer's no runs nothing, and takes nothing.
        f.coLog = []
        let runs = f.mixerRuns.count
        let denied = CoProducerCommands.run(
            .coProducerApply(token: token, plan: mixerPlan(), window: nil, client: nil), doc: (id, m), store: s,
            host: host, stamp: stamp, peer: nil, approver: FakeApprover(.deny))
        #expect(failure(denied)?.message == CoProducerCommands.notAllowed)
        #expect(f.coLog.isEmpty)
        #expect(f.mixerRuns.count == runs)
    }

    /// Plan 8b Task 5: the notes, a note and an export folder reach the
    /// session; the folder's date is the stamp's day.
    @Test func notesNoteAndFolderPassThrough() throws {
        let (s, id, _, f) = try demo()
        f.coNotes = NotesRow(text: "# Song\nkeep it dark\n", truncated: true, path: "/x/Song.vizdaw/Notes.md")
        guard case .coProducerNotes(let doc, let notes) = result(run(.coProducerNotes(window: nil), s, id)) else {
            Issue.record("notes did not answer")
            return
        }
        #expect(doc == id.uuidString)
        #expect(notes == f.coNotes)
        #expect(f.coLog == ["notes"])

        _ = try f.coProducerBegin(name: "drum bus")
        f.coLog = []
        guard case .coProducerRequest(_, let req?, _) = result(run(.coProducerNote(text: "dry verse", window: nil), s, id))
        else {
            Issue.record("note did not answer the request")
            return
        }
        #expect(req.name == "drum bus")
        #expect(f.coLog == ["note dry verse"])

        f.coLog = []
        let folder = run(.coProducerExportFolder(what: .stems, parent: "/Users/x/Exports", window: nil), s, id)
        guard case .coProducerFolder(_, let path) = result(folder) else {
            Issue.record("export_folder did not answer a folder: \(folder)")
            return
        }
        #expect(path == "/Users/x/Exports/Untitled — Stems — 2026-11-14")
        #expect(f.coLog == ["export folder stems 2026-11-14 /Users/x/Exports"])
        f.coLog = []
        _ = run(.coProducerExportFolder(what: .midi, parent: nil, window: nil), s, id)
        #expect(f.coLog == ["export folder midi 2026-11-14 -"])

        // The session's refusals pass on.
        f.coNoteError = .Model(message: "Open a request first (begin_request).")
        #expect(failure(run(.coProducerNote(text: "x", window: nil), s, id))?.message
            == "Open a request first (begin_request).")
        f.coNoteError = .Model(message: "Ask the composer where exports should go, then call export_folder with parent.")
        #expect(failure(run(.coProducerExportFolder(what: .mix, parent: nil, window: nil), s, id))?.message
            == "Ask the composer where exports should go, then call export_folder with parent.")
        f.coNotesError = .Model(message: "Notes.md is not a plain file, so Co-Producer leaves it alone.")
        #expect(failure(run(.coProducerNotes(window: nil), s, id))?.message
            == "Notes.md is not a plain file, so Co-Producer leaves it alone.")
        // No document: no call.
        for r: ControlRequest in [
            .coProducerNotes(window: nil), .coProducerNote(text: "x", window: nil),
            .coProducerExportFolder(what: .stems, parent: nil, window: nil),
        ] {
            let o = CoProducerCommands.run(
                r, doc: nil, store: s, host: host, stamp: stamp, peer: nil, approver: FakeApprover(.allow))
            #expect(failure(o)?.code == .noDocument)
        }
    }

    /// The Task 3 review's carry: turning the switch off ends the open
    /// request through the session's end, which writes its entry.
    @Test func turningTheSwitchOffEndsTheRequest() throws {
        let (s, _, _, f) = try demo()
        let settings = CoProducerSettings(
            defaults: MemoryDefaults(), supportDir: URL(fileURLWithPath: "/tmp/vizdaw-switch-off"),
            sink: FakeCoProducerSink())
        settings.didTurnOff = { CoProducerCommands.tidy(s, all: true) }
        settings.setOn(true)
        _ = try f.coProducerBegin(name: "one")
        f.coLog = []
        settings.setOn(false)
        #expect(f.coLog == ["end"])
        #expect(f.coRequest == nil)
    }

    @Test func noDocumentIsNoDocument() {
        let s = DocumentStore(makeEmpty: { FakeSession([]) }, makeDemo: { demoFake() }, open: { _ in demoFake() })
        let o = CoProducerCommands.run(
            .coProducerBegin(name: "x", asked: nil, window: nil), doc: nil, store: s, host: host, stamp: stamp,
            peer: nil, approver: FakeApprover(.allow))
        #expect(failure(o)?.code == .noDocument)
    }

    // MARK: Plan 8c Task 4: chores and the one-call export

    var choreArgs: [ControlArg] { [int("seq", 1), text("lanes", "1,2"), int("semitones", 2)] }

    /// The carry from Tasks 2 and 3: a note chore, a marker rename or a
    /// marker removal changes the composer's work, so `command.run` refuses
    /// it here as the gate does, with no enter and no session call.
    @Test func changeChoresAreRefusedOnCommandRun() throws {
        let (s, id, _, f) = try demo()
        for chore in [
            "modify.quantize_notes", "modify.transpose_notes", "modify.velocity_notes", "marker.rename",
            "marker.remove",
        ] {
            f.coLog = []
            let o = run(command(chore, choreArgs), s, id)
            #expect(failure(o)?.code == .notAllowed, "\(chore)")
            #expect(failure(o)?.message
                == "\(chore) changes your work: preview it with co_producer.preview and apply it with the composer's yes.")
            #expect(f.coLog == [], "\(chore)")
        }
        #expect(f.chores.isEmpty)
        // Adding a marker adds freely (the 8c Task 1 ruling): it runs in the stretch.
        f.coLog = []
        let added = run(command("marker.add", [int("seq", 1), int("tick", 0), text("name", "Verse")]), s, id)
        #expect(failure(added) == nil)
        #expect(f.coLog == ["enter \(stamp)", "chore marker.add", "leave"])
    }

    /// A change chore runs only after `co_producer_take` got a yes: the
    /// sheet's Allow, or a standing approval for the caller; a no runs nothing.
    @Test func changeChoresRunOnlyAfterAGrantedTake() throws {
        let (s, id, _, f) = try demo()
        let token = String(repeating: "d", count: 32)
        let plan = [
            PlannedRow(id: "modify.transpose_notes", args: choreArgs),
            PlannedRow(id: "marker.remove", args: [int("seq", 1), int("index", 2)]),
        ]
        let o = run(.coProducerApply(token: token, plan: plan, window: nil, client: nil), s, id)
        guard case .coProducerApplied(_, let lines, _) = result(o) else {
            Issue.record("not applied: \(o)")
            return
        }
        #expect(lines == ["modify.transpose_notes: done", "marker.remove: done"])
        #expect(f.coLog == ["enter \(stamp)", "take", "chore modify.transpose_notes", "chore marker.remove", "leave"])
        #expect(f.coTakeApprovals == [.sheet])
        #expect(f.chores.map(\.id) == ["modify.transpose_notes", "marker.remove"])
        #expect(f.chores.first?.args == choreArgs)

        // The composer said no on the sheet: no take, no chore.
        f.coLog = []
        f.chores = []
        let no = CoProducerCommands.run(
            .coProducerApply(token: token, plan: plan, window: nil, client: nil), doc: (id, try #require(s.model(id))),
            store: s, host: host, stamp: stamp, peer: nil, approver: FakeApprover(.deny))
        #expect(failure(no)?.message == CoProducerCommands.notAllowed)
        #expect(f.coLog == [])
        #expect(f.chores.isEmpty)

        // A refused take runs no chore, and still leaves.
        f.coTakeError = .Model(message: "This preview was already used or replaced.")
        let refused = run(.coProducerApply(token: token, plan: plan, window: nil, client: nil), s, id)
        #expect(failure(refused)?.message == "This preview was already used or replaced.")
        #expect(f.coLog == ["enter \(stamp)", "take", "leave"])
        #expect(f.chores.isEmpty)

        // A standing approval for the caller is the other yes.
        let rig = try ApprovalRig()
        rig.store(StandingApproval(
            key: "signed:ABCDE12345:com.example.chat", verified: true, name: "Helper Chat", team: FakeClientFacts.team,
            identifier: "com.example.chat", path: nil, label: nil, granted: rig.now))
        let standing = rig.run(
            .coProducerApply(token: rig.token, plan: plan, window: nil, client: "Helper Chat"),
            peer: FakeClientFacts.peerRow(conn: 1))
        #expect(rig.isApplied(standing))
        #expect(rig.approver.asked.isEmpty)
        #expect(rig.f.coTakeApprovals == [.standing])
        #expect(rig.f.coLog == [
            "enter \(rig.stamp)", "take", "chore modify.transpose_notes", "chore marker.remove", "leave",
        ])
    }

    /// The export's deferred answer, once there.
    func settled(_ o: ControlOutcome) async -> ControlOutcome? {
        guard case .pending(let p) = o else {
            Issue.record("the export is not deferred: \(o)")
            return nil
        }
        await p.settle()
        return p.outcome
    }

    /// A file a bounce wrote.
    func bounced(_ path: String) -> BouncedFileRow {
        BouncedFileRow(path: path, frames: 96_000, rate: 48_000, bits: "24-bit", channels: 2, clipped: 0)
    }

    /// A stems plan as the core makes it.
    func stemsPlan(_ folder: String) -> ExportPlanRow {
        ExportPlanRow(
            folder: folder, id: BounceCommands.id,
            args: [text("source", "stems"), text("strips", "2,3"), text("path", folder)])
    }

    /// `co_producer.export` plans with the app's date, runs the plan's
    /// command between enter and leave as the menus run it, and answers
    /// from the finished job.
    @Test func exportRunsThePlannedCommand() async throws {
        let (s, id, _, f) = try demo()
        let folder = "/Users/x/Exports/Song — Stems — 2026-11-14"
        f.coExportPlan = stemsPlan(folder)
        f.jobStartState = .running
        let o = run(.coProducerExport(what: .stems, parent: "/Users/x/Exports", strips: [2, 3], window: nil), s, id)
        // `stamp(now)`'s first 10 characters are the date.
        #expect(f.coLog == ["enter \(stamp)", "export plan stems 2026-11-14 /Users/x/Exports 2,3", "bounce", "leave"])
        let req = try #require(f.bounces.first)
        #expect(req.source == .stems(strips: [2, 3]))
        #expect(req.path == folder)
        guard case .pending(let p) = o else {
            Issue.record("the export is not deferred: \(o)")
            return
        }
        // Not yet: the job runs.
        #expect(p.outcome == nil)
        let job = try #require(f.jobRows.keys.max())
        f.resultRow = BounceResultRow(
            files: [
                bounced(folder + "/Bass.wav"),
                bounced(folder + "/Keys.wav"),
            ], media: [])
        f.jobRows[job]?.state = .done
        await p.settle()
        guard case .done(.coProducerExported(let doc, let answered, let written, let summary))? = p.outcome else {
            Issue.record("not exported: \(String(describing: p.outcome))")
            return
        }
        #expect(doc == id.uuidString)
        #expect(answered == folder)
        #expect(written == [folder + "/Bass.wav", folder + "/Keys.wav"])
        #expect(summary == nil)
    }

    /// A DAWproject export answers the job's `written` and its report's summary.
    @Test func exportOfADawprojectAnswersItsReport() async throws {
        let (s, id, _, f) = try demo()
        let folder = "/tmp/Song — DAWproject — 2026-11-14"
        f.coExportPlan = ExportPlanRow(
            folder: folder, id: InterchangeCommands.exportDawproject, args: [text("path", folder + "/Song.dawproject")])
        f.jobStartState = .running
        let o = run(.coProducerExport(what: .dawproject, parent: nil, strips: nil, window: nil), s, id)
        #expect(f.coLog == ["enter \(stamp)", "export plan dawproject 2026-11-14 - -", "leave"])
        #expect(f.dawprojectExports.map(\.path) == [folder + "/Song.dawproject"])
        let job = try #require(f.jobRows.keys.max())
        f.jobRows[job]?.state = .done
        f.jobRows[job]?.written = [folder + "/Song.dawproject"]
        f.jobRows[job]?.report = ReportRow(summary: "Exported 4 tracks", items: [])
        guard case .done(.coProducerExported(_, let answered, let written, let summary))? = await settled(o) else {
            Issue.record("not exported")
            return
        }
        #expect(answered == folder)
        #expect(written == [folder + "/Song.dawproject"])
        #expect(summary == "Exported 4 tracks")
    }

    /// The request's entry lists the files a job really wrote, once each;
    /// a failed job lists what it had already written.
    @Test func writtenFilesAreRecorded() async throws {
        let (s, id, m, f) = try demo()
        f.jobStartState = .running
        // An outward `command.run`: the job's files, not its `path`.
        let bounce = command(BounceCommands.id, [text("source", "stems"), text("strips", "1"), text("path", "/tmp/out")])
        guard case .job(_, _, let job) = run(bounce, s, id) else {
            Issue.record("the bounce is not a job")
            return
        }
        #expect(!f.coLog.contains { $0.hasPrefix("file") })
        f.jobRows[job]?.state = .done
        f.resultRow = BounceResultRow(
            files: [bounced("/tmp/out/A.wav"), bounced("/tmp/out/A.wav")],
            media: [])
        f.coLog = []
        CoProducerCommands.jobEnded(doc: id, job: job, m)
        #expect(f.coLog == ["file /tmp/out/A.wav"])
        // Once only.
        f.coLog = []
        CoProducerCommands.jobEnded(doc: id, job: job, m)
        #expect(f.coLog.isEmpty)

        // A failed export job records only what it had already written.
        f.resultRow = nil
        let midi = command(InterchangeCommands.exportMidi, [text("path", "/tmp/song.mid")])
        guard case .job(_, _, let failing) = run(midi, s, id) else {
            Issue.record("the MIDI export is not a job")
            return
        }
        f.jobRows[failing]?.state = .failed
        f.jobRows[failing]?.message = "disk full"
        f.coLog = []
        CoProducerCommands.jobEnded(doc: id, job: failing, m)
        #expect(f.coLog.isEmpty, "it wrote nothing")
        guard case .job(_, _, let partial) = run(command(InterchangeCommands.exportMidi, [text("path", "/tmp/b.mid")]), s, id)
        else {
            Issue.record("the MIDI export is not a job")
            return
        }
        f.jobRows[partial]?.state = .failed
        f.jobRows[partial]?.written = ["/tmp/b-1.mid"]
        f.coLog = []
        CoProducerCommands.jobEnded(doc: id, job: partial, m)
        #expect(f.coLog == ["file /tmp/b-1.mid"])
        // The document closed: nothing to record into.
        CoProducerCommands.jobEnded(doc: id, job: partial, nil)

        // A command that fails at once records nothing.
        f.bounceError = .Model(message: "no room")
        f.coLog = []
        #expect(failure(run(command(BounceCommands.id, [text("path", "/tmp/c.wav")]), s, id)) != nil)
        #expect(!f.coLog.contains { $0.hasPrefix("file") })
        f.bounceError = nil

        // One answered at once records its path then; a failure, a dry run
        // or a command that is not outward records nothing.
        let ran = ControlOutcome.done(ControlCommands.ran("file.export_midi", id, m))
        f.coLog = []
        _ = CoProducerCommands.recorded(ran, "file.export_midi", [text("path", "/tmp/song.mid")], id, m)
        #expect(f.coLog == ["file /tmp/song.mid"])
        f.coLog = []
        _ = CoProducerCommands.recorded(
            .failed(.of(.failed, "no")), "file.export_midi", [text("path", "/tmp/song.mid")], id, m)
        _ = CoProducerCommands.recorded(
            ran, "file.export_midi",
            [text("path", "/tmp/song.mid"), ControlArg(name: "dry_run", value: .flag(value: true))], id, m)
        _ = CoProducerCommands.recorded(ran, "media.import", [text("path", "/tmp/in.wav")], id, m)
        _ = CoProducerCommands.recorded(ran, "file.export_midi", [], id, m)
        #expect(f.coLog.isEmpty)

        // The one-call export records its bounce's files the same way, once.
        let folder = "/tmp/Song — Stems — 2026-11-14"
        f.coExportPlan = stemsPlan(folder)
        let o = run(.coProducerExport(what: .stems, parent: nil, strips: nil, window: nil), s, id)
        let exportJob = UInt64(3000 + f.bounces.count)
        f.resultRow = BounceResultRow(files: [bounced(folder + "/Bass.wav")], media: [])
        f.jobRows[exportJob]?.state = .done
        f.coLog = []
        _ = await settled(o)
        #expect(f.coLog.filter { $0.hasPrefix("file") } == ["file \(folder)/Bass.wav"])
    }

    /// A refused plan answers its sentence and runs no command.
    @Test func aRefusedPlanRunsNothing() throws {
        let (s, id, _, f) = try demo()
        f.coNoteError = .Model(message: "There is nothing to bounce: no strip plays audio.")
        let o = run(.coProducerExport(what: .stems, parent: nil, strips: nil, window: nil), s, id)
        #expect(failure(o)?.code == .failed)
        #expect(failure(o)?.message == "There is nothing to bounce: no strip plays audio.")
        #expect(f.coLog == ["enter \(stamp)", "export plan stems 2026-11-14 - -", "leave"])
        #expect(f.bounces.isEmpty)
        // No document: no call.
        f.coLog = []
        let none = CoProducerCommands.run(
            .coProducerExport(what: .mix, parent: nil, strips: nil, window: nil), doc: nil, store: s, host: host,
            stamp: stamp, peer: nil, approver: FakeApprover(.allow))
        #expect(failure(none)?.code == .noDocument)
        #expect(f.coLog == [])
    }

    /// Plan 8c final review: an export refused at its start leaves no
    /// folder and no notes line. A bounce already running: the core's plan
    /// refuses before making the folder, so nothing runs and nothing is
    /// dropped; a command that refuses after the plan drops its folder.
    @Test func aRefusedExportLeavesNoFolder() throws {
        let (s, id, _, f) = try demo()
        let folder = "/tmp/Song — Stems — 2026-11-14"
        f.coExportPlan = stemsPlan(folder)
        f.coNoteError = .Busy(message: "Wait for the running import, export or bounce to finish.")
        let busy = run(.coProducerExport(what: .stems, parent: nil, strips: nil, window: nil), s, id)
        #expect(failure(busy)?.code == .busy)
        #expect(f.coLog == ["enter \(stamp)", "export plan stems 2026-11-14 - -", "leave"])
        #expect(f.bounces.isEmpty)

        // The plan was made, but the bounce still refused: its folder goes.
        f.coNoteError = nil
        f.bounceError = .Busy(message: "A bounce is running.")
        f.coLog = []
        let refused = run(.coProducerExport(what: .stems, parent: nil, strips: nil, window: nil), s, id)
        #expect(failure(refused)?.code == .busy)
        #expect(f.coLog == ["enter \(stamp)", "export plan stems 2026-11-14 - -", "drop \(folder)", "leave"])

        // The retry plans again (the core makes the first-numbered folder).
        f.bounceError = nil
        f.coLog = []
        let retry = run(.coProducerExport(what: .stems, parent: nil, strips: nil, window: nil), s, id)
        guard case .pending = retry else {
            Issue.record("the retry did not start: \(retry)")
            return
        }
        #expect(f.coLog == ["enter \(stamp)", "export plan stems 2026-11-14 - -", "bounce", "leave"])
    }

    /// Recording (or a held drag): the enter refuses, so no plan is made.
    @Test func anExportWhileRecordingPlansNothing() throws {
        let (s, id, _, f) = try demo()
        f.coEnterError = .Busy(message: "Stop recording first.")
        let o = run(.coProducerExport(what: .mix, parent: "/tmp", strips: nil, window: nil), s, id)
        #expect(failure(o)?.code == .busy)
        #expect(failure(o)?.message == "Stop recording first.")
        #expect(f.coLog == ["enter \(stamp)"])
        #expect(f.bounces.isEmpty)
    }

    /// 8a Ruling 13: a failing export job is the call's failure, and its
    /// alert is taken, so no sheet opens; the files it wrote are recorded.
    @Test func noSheetsForChores() async throws {
        let (s, id, m, f) = try demo()
        let folder = "/tmp/Song — Stems — 2026-11-14"
        f.coExportPlan = stemsPlan(folder)
        f.jobStartState = .running
        let o = run(.coProducerExport(what: .stems, parent: nil, strips: nil, window: nil), s, id)
        let job = try #require(f.jobRows.keys.max())
        // The job fails after one file, and the app raised its alert.
        f.resultRow = BounceResultRow(files: [bounced(folder + "/Bass.wav")], media: [])
        f.jobRows[job]?.state = .failed
        f.jobRows[job]?.message = "disk full"
        m.alert = AlertItem(title: "Bounce Failed", message: "disk full")
        f.coLog = []
        let answer = await settled(o)
        #expect(failure(try #require(answer))?.code == .failed)
        #expect(failure(try #require(answer))?.message.hasPrefix("disk full") == true)
        #expect(m.alert == nil)
        #expect(f.coLog == ["file \(folder)/Bass.wav"])
        // A chore the session refuses raises no alert either.
        f.choreError = .Model(message: "Track 4 has no notes there.")
        #expect(failure(run(command("marker.add", choreArgs), s, id))?.message == "Track 4 has no notes there.")
        #expect(m.alert == nil)
    }

    @Test func requestsTheGateRefusesAreNotAllowedHereToo() throws {
        let (s, id, _, f) = try demo()
        let o = run(.quit, s, id)
        #expect(failure(o)?.code == .notAllowed)
        #expect(f.coLog == [])
    }

    @Test func turningOffStopsAndClosesRequests() throws {
        let a = demoFake(), b = demoFake()
        var queue = [a, b]
        let s = DocumentStore(makeEmpty: { FakeSession([]) }, makeDemo: { queue.removeFirst() }, open: { _ in demoFake() })
        _ = try #require(s.newDemo())
        _ = try #require(s.newDemo())
        _ = try a.coProducerBegin(name: "one")
        a.coLog = []
        CoProducerCommands.tidy(s, all: true)
        #expect(a.coLog == ["end"])
        #expect(a.coRequest == nil)
        #expect(b.coLog == [])
    }

    @Test func idleTidyEndsOnlyIdleRequests() throws {
        let (s, _, m, f) = try demo()
        _ = try f.coProducerBegin(name: "one")
        f.coLog = []
        CoProducerCommands.tidy(s, all: false)
        #expect(f.coLog == ["idle 300000"])
        #expect(f.coRequest != nil)
        // One that ended refreshes the document (its Undo title).
        f.coIdleLabel = "Co-Producer: One"
        f.snap.undoLabel = "Co-Producer: One"
        CoProducerCommands.tidy(s, all: false)
        #expect(m.snapshot.undoLabel == "Co-Producer: One")
    }

    /// Ruling P8: an alert a call (or its job) left on the document is
    /// the call's failure, and no sheet opens for it.
    @Test func alertsAreTheCallsFailure() throws {
        let (_, id, m, _) = try demo()
        let earlier = AlertItem(title: "Earlier", message: "already showing")
        m.alert = earlier
        // An alert that was there before is not this call's.
        let kept = CoProducerCommands.takeAlert(.done(ControlCommands.ran("x", id, m)), m, before: earlier.id)
        #expect(failure(kept) == nil)
        #expect(m.alert == earlier)
        m.alert = AlertItem(title: "Export Failed", message: "disk full")
        let o = CoProducerCommands.takeAlert(.done(ControlCommands.ran("x", id, m)), m, before: earlier.id)
        #expect(failure(o)?.message == "Export Failed: disk full")
        #expect(m.alert == nil)
        // A call that failed keeps its own failure; the alert still goes.
        m.alert = AlertItem(title: "Bounce Failed", message: "no room")
        let f = CoProducerCommands.takeAlert(.failed(.of(.busy, "busy")), m, before: nil)
        #expect(failure(f)?.code == .busy)
        #expect(m.alert == nil)
    }

    @Test func stampIsLocalMinutes() throws {
        var c = DateComponents()
        c.year = 2026
        c.month = 11
        c.day = 14
        c.hour = 15
        c.minute = 2
        c.second = 59
        let date = try #require(Calendar.current.date(from: c))
        #expect(CoProducerCommands.stamp(date) == "2026-11-14 15-02")
    }

    @Test func settingAndOpenCopyCommandsRefuseBadInput() throws {
        let (s, id, _, _) = try demo()
        let on = ControlCommands.run(
            CoProducerCommands.setting, args: [ControlArg(name: "on", value: .flag(value: true))],
            doc: s.model(id).map { (id, $0) }, store: s, host: host)
        #expect(failure(on) == nil)
        #expect(host.coProducer.on)
        let dir = FileManager.default.temporaryDirectory.appendingPathComponent("vizdaw-copy-\(UUID().uuidString)")
        let package = dir.appendingPathComponent("Song.vizdaw").path
        let copy = ControlCommands.run(
            CoProducerCommands.openCopy, args: [ControlArg(name: "path", value: .text(value: package))],
            doc: nil, store: s, host: host)
        #expect(failure(copy)?.message == "That is not a Co-Producer copy.")
        // Without a path it would ask: the channel opens no panel.
        let ask = ControlCommands.run(CoProducerCommands.openCopy, args: [], doc: nil, store: s, host: host)
        #expect(failure(ask)?.code == .dialogRequired)
        // Hostile input: a missing or wrongly typed `on`, a huge or NUL path.
        let noOn = ControlCommands.run(CoProducerCommands.setting, args: [], doc: nil, store: s, host: host)
        #expect(failure(noOn)?.code == .invalidParams)
        let textOn = ControlCommands.run(
            CoProducerCommands.setting, args: [ControlArg(name: "on", value: .text(value: "yes"))],
            doc: nil, store: s, host: host)
        #expect(failure(textOn)?.code == .invalidParams)
        #expect(host.coProducer.on)
        for bad in [String(repeating: "a", count: 70_000), "/x/Co-Producer/Copies/a\0.vizdaw/..", ""] {
            let o = ControlCommands.run(
                CoProducerCommands.openCopy, args: [ControlArg(name: "path", value: .text(value: bad))],
                doc: nil, store: s, host: host)
            #expect(failure(o)?.message == "That is not a Co-Producer copy.")
        }
        #expect(s.documents.count == 1)
        #expect(host.calls.isEmpty)
        #expect(s.alert == nil)
    }

    @Test func aCopyPathIsKnownByItsPlace() {
        #expect(DocumentStore.isCopy("/x/Song.vizdaw/Co-Producer/Copies/2026-11-14 15-02 before Song.vizdaw"))
        #expect(DocumentStore.isCopy("/x/Song.vizdaw/co-producer/copies/a.VIZDAW"))
        #expect(!DocumentStore.isCopy("/x/Song.vizdaw"))
        #expect(!DocumentStore.isCopy("/x/Copies/a.vizdaw"))
        #expect(!DocumentStore.isCopy("/x/Co-Producer/Copies/.vizdaw"))
    }

    @Test func openRoutesACopyToOpenCopy() throws {
        let s = DocumentStore(makeEmpty: { FakeSession([]) }, makeDemo: { demoFake() }, open: { _ in
            Issue.record("a copy never opens as a project")
            return demoFake()
        })
        var opened: [String] = []
        s.openCopySession = { path in
            opened.append(path)
            return demoFake()
        }
        let path = "/x/Song.vizdaw/Co-Producer/Copies/2026-11-14 15-02 before Song.vizdaw"
        #expect(s.open(path: path) != nil)
        #expect(opened == [path])
    }
}
