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
        CoProducerCommands.run(r, doc: s.model(id).map { (id, $0) }, store: s, host: host, stamp: stamp)
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
        let o = run(.coProducerApply(token: token, plan: renameThenDelete, window: nil), s, id)
        #expect(f.coLog == ["enter \(stamp)", "take", "rename", "delete", "leave"])
        guard case .coProducerApplied(let doc, let results, _) = result(o) else {
            Issue.record("not applied: \(o)")
            return
        }
        #expect(doc == id.uuidString)
        #expect(results == ["track.rename: done", "track.delete: done"])

        // A failed command stops the rest; each has its line.
        f.coLog = []
        f.editError = .Model(message: "the name is refused")
        let failed = run(.coProducerApply(token: token, plan: renameThenDelete, window: nil), s, id)
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
        let refused = run(.coProducerApply(token: token, plan: renameThenDelete, window: nil), s, id)
        #expect(failure(refused)?.message == "This preview was already used or replaced.")
        #expect(f.coLog == ["enter \(stamp)", "take", "leave"])

        // No request: nothing is taken.
        f.coLog = []
        f.coEnterError = .Model(message: "Open a request first (begin_request).")
        let none = run(.coProducerApply(token: token, plan: renameThenDelete, window: nil), s, id)
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
        #expect(f.coLog == ["begin drum bus", "preview 2", "end"])
        // The session's refusals pass on, a blank name's included.
        f.coBeginError = .Model(message: "Name the request.")
        #expect(failure(run(.coProducerBegin(name: "", asked: nil, window: nil), s, id))?.message == "Name the request.")
        f.coPreviewError = .Busy(message: "Finish the drag first.")
        #expect(failure(run(.coProducerPreview(plan: renameThenDelete, window: nil), s, id))?.code == .busy)
        #expect(failure(run(.coProducerEnd(window: nil), s, id))?.message == "No request is open.")
    }

    /// Plan 8b Task 1: the new calls and registry ids answer "Not ready
    /// yet." until Task 5 builds them.
    @Test func plan8bCallsAreNotReadyYet() throws {
        let (s, id, _, _) = try demo()
        for r: ControlRequest in [
            .coProducerNotes(window: nil),
            .coProducerNote(text: "dry verse", window: nil),
            .coProducerExportFolder(what: .stems, parent: nil, window: nil),
        ] {
            let f = failure(run(r, s, id))
            #expect(f?.code == .unavailable)
            #expect(f?.message == "Not ready yet.")
        }
        for c in ["track.make_mine", "mixer.make_mine", "mixer.set_colour", "window.song_notes"] {
            let f = failure(ControlCommands.run(c, args: [], doc: s.model(id).map { (id, $0) }, store: s, host: host))
            #expect(f?.code == .unavailable, "\(c)")
            #expect(f?.message == "Not ready yet.", "\(c)")
        }
    }

    @Test func noDocumentIsNoDocument() {
        let s = DocumentStore(makeEmpty: { FakeSession([]) }, makeDemo: { demoFake() }, open: { _ in demoFake() })
        let o = CoProducerCommands.run(.coProducerBegin(name: "x", asked: nil, window: nil), doc: nil, store: s, host: host, stamp: stamp)
        #expect(failure(o)?.code == .noDocument)
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
