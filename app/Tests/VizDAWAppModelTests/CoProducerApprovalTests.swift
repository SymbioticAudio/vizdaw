import AppKit
import Foundation
import Testing
@testable import VizDAWAppModel
import VizDAWCore

/// A demo document with an open request, a preview of a hand track that
/// needs the composer's yes, the Co-Producer switch on, and every outside
/// thing faked: `FakeClientFacts`, `MemoryApprovalStore`, a fixed clock
/// and `FakeApprover` (plan 8b Task 6). Nothing opens and nothing touches
/// the Keychain.
@MainActor
final class ApprovalRig {
    let host = FakeControlHost()
    let store: DocumentStore
    let id: UUID
    let m: DocumentModel
    let f: FakeSession
    let approver: FakeApprover
    let approvals = MemoryApprovalStore()
    /// 2026-11-16 12:00 UTC.
    var now = Date(timeIntervalSince1970: 1_794_830_400)
    let token = String(repeating: "c", count: 32)
    let stamp = "2026-11-16 12-00"
    let plan = [
        PlannedRow(
            id: "track.rename",
            args: [
                ControlArg(name: "seq", value: .int(value: 3)), ControlArg(name: "lane", value: .int(value: 30)),
                ControlArg(name: "name", value: .text(value: "Lead")),
            ])
    ]
    let items = [ChangeItemRow(thing: "Track 1, Groove", change: "renamed to \"Lead\"", composersWork: true)]

    init(facts: any ClientFacts = FakeClientFacts.goodChain(), approver: FakeApprover = FakeApprover()) throws {
        let fake = demoFake()
        f = fake
        store = DocumentStore(makeEmpty: { FakeSession([]) }, makeDemo: { fake }, open: { _ in demoFake() })
        id = try #require(store.newDemo())
        m = try #require(store.model(id))
        self.approver = approver
        host.coProducer.facts = facts
        host.coProducer.approvals = approvals
        host.coProducer.now = { [unowned self] in self.now }
        host.coProducer.setOn(true)
        _ = try f.coProducerBegin(name: "tidy")
        f.coPreview = PreviewRow(token: token, needsYes: true, items: items)
        f.coLog = []
    }

    /// The song's name as the sheet shows it.
    var song: String { m.snapshot.title }

    func store(_ a: StandingApproval) {
        var all = approvals.load()
        all.append(a)
        try? approvals.save(all)
    }

    func run(_ r: ControlRequest, peer: PeerRow? = nil) -> ControlOutcome {
        CoProducerCommands.run(
            r, doc: (id, m), store: store, host: host, stamp: stamp, peer: peer, approver: approver)
    }

    func apply(peer: PeerRow?, client: String?) -> ControlOutcome {
        run(.coProducerApply(token: token, plan: plan, window: nil, client: client), peer: peer)
    }

    /// The wait the last apply opened.
    var wait: CoProducerApprovalWait? { CoProducerCommands.approvalWait(for: id) }

    func check(at t: Date? = nil) {
        CoProducerCommands.checkApprovals(store: store, host: host, now: t ?? now)
    }

    func isAwaiting(_ o: ControlOutcome) -> Bool {
        if case .awaitingApproval(let doc) = o { return doc == id }
        return false
    }

    func isApplied(_ o: ControlOutcome) -> Bool {
        if case .done(.coProducerApplied) = o { return true }
        return false
    }

    static func failure(_ o: ControlOutcome?) -> ControlFailure? {
        if case .failed(let f)? = o { return f }
        return nil
    }
}

/// VizDAW's own approval sheet (plan 8b Task 6, Ruling 11): only a real
/// Allow or a standing approval applies a change that needs a yes, and no
/// sheet opens for anything else.
@MainActor
@Suite struct CoProducerApprovalTests {
    typealias F = FakeClientFacts
    let no = "The composer did not allow this in VizDAW."
    let used = "This preview was already used or replaced."
    let openFirst = "Open a request first (begin_request)."

    @Test func applyWithoutTheClickIsRefused() throws {
        // An unknown caller: no peer token.
        let rig = try ApprovalRig()
        var o = rig.apply(peer: nil, client: nil)
        #expect(rig.isAwaiting(o))
        #expect(!rig.f.coLog.contains("take"))
        #expect(rig.f.coLog.isEmpty, "nothing entered: \(rig.f.coLog)")
        #expect(rig.approver.asked.map(\.identity) == [.unknown])
        #expect(rig.approver.asked.first?.items == rig.items)
        var w = try #require(rig.wait)
        #expect(rig.m.coProducerApproval != nil)

        // Don't Allow: VizDAW's no, the token dropped, nothing taken.
        rig.approver.answer(.deny)
        #expect(ApprovalRig.failure(w.outcome)?.code == .failed)
        #expect(ApprovalRig.failure(w.outcome)?.message == no)
        #expect(rig.f.coTakeApprovals.isEmpty)
        #expect(!rig.f.coLog.contains("take"))
        #expect(rig.f.coForgotten == [rig.token])
        #expect(rig.m.coProducerApproval == nil)
        #expect(rig.wait == nil)

        // Allow, once: enter, take with the sheet's approval, the plan, leave.
        o = rig.apply(peer: nil, client: nil)
        w = try #require(rig.wait)
        rig.f.coLog = []
        rig.approver.answer(.allow(always: false, confirmed: false))
        #expect(rig.f.coLog == ["enter \(rig.stamp)", "take", "rename", "leave"])
        #expect(rig.f.coTakeApprovals == [.sheet])
        #expect(rig.isApplied(try #require(w.outcome)))
        #expect(rig.approvals.load().isEmpty, "Allow alone stores nothing")
        #expect(rig.m.coProducerApproval == nil)
        // A second answer to a finished sheet does nothing.
        rig.approver.answer(.allow(always: false, confirmed: false))
        #expect(rig.f.coTakeApprovals == [.sheet])

        // No answer: ten minutes is a no.
        _ = rig.apply(peer: nil, client: nil)
        w = try #require(rig.wait)
        rig.check(at: rig.now.addingTimeInterval(599))
        #expect(w.outcome == nil)
        rig.check(at: rig.now.addingTimeInterval(600))
        #expect(ApprovalRig.failure(w.outcome)?.message == no)
        #expect(rig.approver.cancelled == [rig.id])
        #expect(rig.m.coProducerApproval == nil)

        // The song changed meanwhile.
        _ = rig.apply(peer: nil, client: nil)
        w = try #require(rig.wait)
        var snap = rig.m.snapshot
        snap.version += 1
        rig.m.apply(snap)
        rig.check()
        #expect(ApprovalRig.failure(w.outcome)?.message == no)
        #expect(rig.approver.cancelled == [rig.id, rig.id])

        // The caller went away.
        _ = rig.apply(peer: nil, client: nil)
        w = try #require(rig.wait)
        w.callerWaiting = { false }
        rig.check()
        #expect(ApprovalRig.failure(w.outcome)?.message == no)
        #expect(rig.approver.cancelled.count == 3)

        // The switch went off.
        _ = rig.apply(peer: nil, client: nil)
        w = try #require(rig.wait)
        rig.host.coProducer.setOn(false)
        rig.check()
        #expect(ApprovalRig.failure(w.outcome)?.message == no)
        #expect(rig.approver.cancelled.count == 4)
        #expect(rig.f.coTakeApprovals == [.sheet], "none of those applied")

        // The document closed.
        let closing = try ApprovalRig()
        _ = closing.apply(peer: nil, client: nil)
        w = try #require(closing.wait)
        closing.store.close(closing.id)
        closing.check()
        #expect(ApprovalRig.failure(w.outcome)?.message == no)
        #expect(closing.approver.cancelled == [closing.id])
        #expect(closing.f.coTakeApprovals.isEmpty)
        _ = o
    }

    @Test func alwaysAllowNeedsTheWarningWhenUnverified() throws {
        // A known host: stored as verified, today, and applied.
        let rig = try ApprovalRig()
        _ = rig.apply(peer: F.peerRow(), client: "Helper Chat")
        var w = try #require(rig.wait)
        rig.approver.answer(.allow(always: true, confirmed: false))
        #expect(rig.isApplied(try #require(w.outcome)))
        #expect(rig.f.coTakeApprovals == [.sheet])
        #expect(rig.approvals.load() == [StandingApproval(
            key: "signed:ABCDE12345:com.example.chat", verified: true, name: "Helper Chat", team: F.team,
            identifier: "com.example.chat", path: nil, label: nil, granted: rig.now)])

        // An unverified host: Always allow without the warning confirmed
        // stores nothing, applies nothing, and shows the warning.
        let cli = try ApprovalRig()
        _ = cli.apply(peer: F.peerRow(F.cliPid, conn: 2), client: "Helper CLI")
        w = try #require(cli.wait)
        cli.approver.answer(.allow(always: true, confirmed: false))
        #expect(w.outcome == nil)
        #expect(cli.approvals.load().isEmpty)
        #expect(cli.f.coTakeApprovals.isEmpty)
        let sheet = try #require(cli.m.coProducerApproval)
        #expect(sheet.step == .warning)
        #expect(sheet.warning
            == "VizDAW can't check which program this is. Any program you run from Terminal could use this approval.")
        // Cancel goes back to the sheet, the box unticked, nothing saved.
        sheet.cancelWarning()
        #expect(sheet.step == .ask)
        #expect(!sheet.always)
        #expect(cli.approvals.load().isEmpty)
        #expect(cli.wait === w)
        // The sheet itself: ticking the box and Allow shows the warning
        // first; only a real press of Always Allow Anyway saves and applies.
        sheet.always = true
        sheet.allow(realPress: true)
        #expect(sheet.step == .warning)
        #expect(w.outcome == nil)
        sheet.allowAnyway(realPress: false)
        #expect(w.outcome == nil)
        #expect(cli.approvals.load().isEmpty)
        sheet.allowAnyway(realPress: true)
        #expect(cli.isApplied(try #require(w.outcome)))
        #expect(cli.f.coTakeApprovals == [.sheet])
        #expect(cli.approvals.load() == [StandingApproval(
            key: "unverified:\(F.cliPath)\nHelper CLI", verified: false, name: "Helper CLI", team: nil,
            identifier: nil, path: F.cliPath, label: "Helper CLI", granted: cli.now)])

        // Through the approver: confirmed stores and applies.
        let cli2 = try ApprovalRig()
        _ = cli2.apply(peer: F.peerRow(F.cliPid, conn: 2), client: "Helper CLI")
        w = try #require(cli2.wait)
        cli2.approver.answer(.allow(always: true, confirmed: true))
        #expect(cli2.isApplied(try #require(w.outcome)))
        #expect(cli2.approvals.load().map(\.key) == ["unverified:\(F.cliPath)\nHelper CLI"])

        // An unknown caller can never be always allowed.
        let unknown = try ApprovalRig()
        _ = unknown.apply(peer: nil, client: "Helper Chat")
        w = try #require(unknown.wait)
        let plain = try #require(unknown.m.coProducerApproval)
        #expect(!plain.offersAlways)
        #expect(plain.unknownLine == "VizDAW cannot confirm which app is asking, so it asks every time.")
        for confirmed in [false, true] {
            unknown.approver.answer(.allow(always: true, confirmed: confirmed))
            #expect(w.outcome == nil)
            #expect(unknown.approvals.load().isEmpty)
            #expect(unknown.m.coProducerApproval?.step == .ask, "the sheet stays open")
        }
        #expect(unknown.f.coTakeApprovals.isEmpty)
        // A press that is not real allows nothing.
        plain.allow(realPress: false)
        #expect(w.outcome == nil)
        plain.allow(realPress: true)
        #expect(unknown.isApplied(try #require(w.outcome)))
    }

    @Test func theSheetSaysWhoAsks() throws {
        let rig = try ApprovalRig()
        _ = rig.apply(peer: F.peerRow(), client: "Something Else")
        let known = try #require(rig.m.coProducerApproval)
        #expect(known.title == "Allow Co-Producer to Change Your Song?")
        #expect(known.message == "Helper Chat asks to make these changes to \"\(rig.song)\":")
        #expect(known.lines == ["Track 1, Groove: renamed to \"Lead\""])
        #expect(known.alwaysLabel == "Always allow Helper Chat")
        #expect(known.alwaysLine
            == "Anything Helper Chat runs can then change your songs without VizDAW asking. You can stop this in Settings › Co-Producer.")
        rig.approver.answer(.deny)

        let cli = try ApprovalRig()
        _ = cli.apply(peer: F.peerRow(F.cliPid, conn: 2), client: nil)
        let sheet = try #require(cli.m.coProducerApproval)
        #expect(sheet.message == "An app VizDAW cannot identify asks to make these changes to \"\(cli.song)\":")
        #expect(sheet.alwaysLabel == "Always allow helper-cli", "the path's last part without a label")

        // At most 40 lines, then "and N more".
        let many = try ApprovalRig()
        many.f.coPreview = PreviewRow(
            token: many.token, needsYes: true,
            items: (1...45).map { ChangeItemRow(thing: "Track \($0)", change: "deleted", composersWork: true) })
        _ = many.apply(peer: nil, client: nil)
        let long = try #require(many.m.coProducerApproval)
        #expect(long.lines.count == 41)
        #expect(long.lines.last == "and 5 more")
    }

    @Test func noSheetWithoutAWaitingYes() throws {
        let rig = try ApprovalRig()
        for r: ControlRequest in [
            .coProducerBegin(name: "x", asked: nil, window: nil), .coProducerPreview(plan: rig.plan, window: nil),
            .coProducerStatus(window: nil), .runCommand(id: "transport.stop", args: [], window: nil),
            .coProducerNotes(window: nil),
        ] {
            _ = rig.run(r, peer: F.peerRow())
        }
        #expect(rig.approver.asked.isEmpty)

        // A bad, used or stale token, or a changed plan: answered at once.
        rig.f.coPendingError = .Model(message: used)
        rig.f.coTakeError = .Model(message: used)
        rig.f.coLog = []
        var o = rig.apply(peer: F.peerRow(), client: "Helper Chat")
        #expect(ApprovalRig.failure(o)?.message == used)
        #expect(rig.f.coTakeApprovals == [CoProducerApproval.none])
        #expect(rig.f.coLog == ["enter \(rig.stamp)", "take", "leave"])
        rig.f.coTakeError = nil

        // A preview that needs no yes: at once, with no approval.
        rig.f.coPendingError = nil
        rig.f.coPreview = PreviewRow(token: rig.token, needsYes: false, items: [])
        o = rig.apply(peer: F.peerRow(), client: "Helper Chat")
        #expect(rig.isApplied(o))
        #expect(rig.f.coTakeApprovals == [.none, .none])

        // No open request.
        rig.f.coPreview = PreviewRow(token: rig.token, needsYes: true, items: rig.items)
        rig.f.coPendingError = .Model(message: openFirst)
        rig.f.coEnterError = .Model(message: openFirst)
        o = rig.apply(peer: F.peerRow(), client: "Helper Chat")
        #expect(ApprovalRig.failure(o)?.message == openFirst)
        #expect(rig.approver.asked.isEmpty)
        rig.f.coPendingError = nil
        rig.f.coEnterError = nil

        // One sheet at a time.
        #expect(rig.isAwaiting(rig.apply(peer: F.peerRow(), client: "Helper Chat")))
        let second = rig.apply(peer: F.peerRow(), client: "Helper Chat")
        #expect(ApprovalRig.failure(second)?.code == .busy)
        #expect(ApprovalRig.failure(second)?.message == "VizDAW is already asking the composer about another change.")
        #expect(rig.approver.asked.count == 1)

        // The idle close leaves a request whose sheet is open.
        rig.f.coIdleLabel = "Co-Producer: tidy"
        rig.f.coLog = []
        CoProducerCommands.tidy(rig.store, all: false)
        #expect(!rig.f.coLog.contains { $0.hasPrefix("idle") })
        rig.approver.answer(.deny)
        CoProducerCommands.tidy(rig.store, all: false)
        #expect(rig.f.coLog.contains { $0.hasPrefix("idle") })
    }

    @Test func isRealPressRefusesSynthesizedInput() {
        let own: Int64 = 777
        #expect(CoProducerApproval.isRealPress(type: .leftMouseUp, sourcePid: 0, ownPid: own))
        #expect(CoProducerApproval.isRealPress(type: .keyDown, sourcePid: 0, ownPid: own))
        #expect(CoProducerApproval.isRealPress(type: .leftMouseUp, sourcePid: own, ownPid: own))
        #expect(CoProducerApproval.isRealPress(type: .keyDown, sourcePid: own, ownPid: own))
        #expect(!CoProducerApproval.isRealPress(type: nil, sourcePid: 0, ownPid: own), "an accessibility press")
        #expect(!CoProducerApproval.isRealPress(type: .leftMouseUp, sourcePid: 4_242, ownPid: own))
        #expect(!CoProducerApproval.isRealPress(type: .keyDown, sourcePid: 4_242, ownPid: own))
        for other: NSEvent.EventType in [.leftMouseDown, .keyUp, .rightMouseUp, .otherMouseUp, .flagsChanged, .appKitDefined] {
            #expect(!CoProducerApproval.isRealPress(type: other, sourcePid: 0, ownPid: own), "\(other)")
        }
    }

    func hook(_ rig: ApprovalRig, _ args: [ControlArg]) -> ControlOutcome {
        ControlCommands.run(
            CoProducerCommands.answerHook, args: args, doc: (rig.id, rig.m), store: rig.store, host: rig.host)
    }

    func flag(_ name: String, _ v: Bool) -> ControlArg { ControlArg(name: name, value: .flag(value: v)) }

    @Test func theAnswerHookNeedsTheLaunchFlag() throws {
        let rig = try ApprovalRig()
        _ = rig.apply(peer: F.peerRow(F.cliPid, conn: 2), client: "Helper CLI")
        var w = try #require(rig.wait)
        // Without the launch flag: not allowed, the sheet untouched.
        var o = hook(rig, [flag("allow", true)])
        #expect(ApprovalRig.failure(o)?.code == .notAllowed)
        #expect(ApprovalRig.failure(o)?.message == "test hook")
        rig.host.coProducer.environment = ["VIZDAW_TEST_HOOKS": "0"]
        #expect(ApprovalRig.failure(hook(rig, [flag("allow", true)]))?.message == "test hook")
        #expect(w.outcome == nil)

        rig.host.coProducer.environment = ["VIZDAW_TEST_HOOKS": "1"]
        // Always for an unverified caller needs the warning confirmed.
        o = hook(rig, [flag("allow", true), flag("always", true)])
        #expect(ApprovalRig.failure(o)?.code == .notAllowed)
        #expect(ApprovalRig.failure(o)?.message
            == "Confirm the warning first: VizDAW can't check which program this is.")
        #expect(w.outcome == nil)
        #expect(rig.approvals.load().isEmpty)
        o = hook(rig, [flag("allow", true), flag("always", true), flag("confirm", true)])
        guard case .done(.coProducerApprovalAnswered(let app, let kind, let label, let path, let items)) = o else {
            Issue.record("not answered: \(o)")
            return
        }
        #expect(app == "Helper CLI")
        #expect(kind == "unverified")
        #expect(label == "Helper CLI")
        #expect(path == F.cliPath)
        #expect(items == rig.items)
        #expect(rig.isApplied(try #require(w.outcome)))
        #expect(rig.approvals.load().count == 1)

        // No sheet open.
        o = hook(rig, [flag("allow", true)])
        #expect(ApprovalRig.failure(o)?.message == "No Co-Producer approval is waiting.")
        #expect(ApprovalRig.failure(o)?.code == .failed)

        // Don't Allow through the hook; an unknown caller cannot be always allowed.
        _ = rig.apply(peer: nil, client: nil)
        w = try #require(rig.wait)
        o = hook(rig, [flag("allow", true), flag("always", true), flag("confirm", true)])
        #expect(ApprovalRig.failure(o)?.message == "Always allow needs a program VizDAW can name.")
        #expect(w.outcome == nil)
        o = hook(rig, [flag("allow", false)])
        guard case .done(.coProducerApprovalAnswered(_, "unknown", nil, nil, _)) = o else {
            Issue.record("not answered: \(o)")
            return
        }
        #expect(ApprovalRig.failure(w.outcome)?.message == no)
        #expect(ApprovalRig.failure(hook(rig, []))?.code == .invalidParams)

        // The shortened wait, only with the flag, at least a second, never longer.
        let s = rig.host.coProducer
        s.environment = ["VIZDAW_APPROVAL_WAIT_MS": "2000"]
        #expect(s.approvalWait == 600)
        s.environment = ["VIZDAW_TEST_HOOKS": "1", "VIZDAW_APPROVAL_WAIT_MS": "2000"]
        #expect(s.approvalWait == 2)
        s.environment = ["VIZDAW_TEST_HOOKS": "1", "VIZDAW_APPROVAL_WAIT_MS": "10"]
        #expect(s.approvalWait == 1)
        s.environment = ["VIZDAW_TEST_HOOKS": "1", "VIZDAW_APPROVAL_WAIT_MS": "700000"]
        #expect(s.approvalWait == 600)
        s.environment = ["VIZDAW_TEST_HOOKS": "1", "VIZDAW_APPROVAL_WAIT_MS": "soon"]
        #expect(s.approvalWait == 600)
        #expect(CoProducerCommands.approvalWait == 600)
        // The shortened wait is the one the sheet keeps.
        s.environment = ["VIZDAW_TEST_HOOKS": "1", "VIZDAW_APPROVAL_WAIT_MS": "2000"]
        _ = rig.apply(peer: nil, client: nil)
        w = try #require(rig.wait)
        rig.check(at: rig.now.addingTimeInterval(2))
        #expect(ApprovalRig.failure(w.outcome)?.message == no)
    }
}
