import Foundation
import VizDAWCore

/// Co-Producer's calls run as the menus run them (plan 8a Ruling 13).
///
/// Every command that is not a read runs between the session's
/// `coProducerEnter` and `coProducerLeave`, on the main thread, in one
/// synchronous stretch: nothing here awaits, spins the run loop or
/// dispatches, so no composer event can land while Co-Producer acts (the
/// Task 4 rule). An apply enters, takes its token, runs its plan and
/// leaves in that order, so a successful take is tied to the plan that
/// then runs.
@MainActor
public enum CoProducerCommands {
    public static let setting = "settings.co_producer"
    public static let openCopy = "file.open_copy"
    /// The line of a planned command after one that failed.
    public static let notRun = "not run (an earlier command failed)"

    /// Runs Co-Producer call `request` on `doc`. `stamp` names the saved
    /// copy the request's first entry writes ("yyyy-MM-dd HH-mm", local
    /// time, now). `ping`, `state.get`, `wait` and `ui.screenshot` are the
    /// channel's own; any other request the gate admits not is refused.
    /// `peer` is who sent it (the socket's audit token), and `approver`
    /// shows VizDAW's own approval sheet for an apply that needs a yes
    /// (plan 8b Ruling 11).
    public static func run(
        _ request: ControlRequest, doc: (id: UUID, model: DocumentModel)?, store: DocumentStore,
        host: any ControlHost, stamp: String, peer: PeerRow?, approver: any CoProducerApprover
    ) -> ControlOutcome {
        // A nested run loop inside a plugin call could deliver the
        // composer's events while Co-Producer acts.
        if controlHostBusy() {
            return .failed(.of(.busy, "a plugin call is running on the main thread; try again"))
        }
        switch request {
        case .runCommand(let id, let args, _):
            if controlCommandReads(id: id) {
                return ControlCommands.run(id, args: args, doc: doc, store: store, host: host)
            }
            // The gate refuses these first; the app refuses them again, so a
            // change to the composer's work runs only after a yes (8c Ruling 3).
            if changesWork.contains(id) {
                return .failed(.of(
                    .notAllowed,
                    "\(id) changes your work: preview it with co_producer.preview and apply it with the composer's yes."))
            }
            guard let (docID, m) = doc else { return noDocument }
            do {
                _ = try m.session.coProducerEnter(stamp: stamp)
            } catch {
                return .failed(failure(error))
            }
            defer { m.session.coProducerLeave() }
            return command(id, args, docID, m, store, host)
        case .coProducerBegin(let name, let asked, _):
            guard let (docID, m) = doc else { return noDocument }
            do {
                let row = try m.session.coProducerBegin(name: name)
                // The time a request that never enters is logged under
                // (plan 8b Task 3 ruling): its begin's.
                try m.session.coProducerSetStamp(stamp: stamp)
                if let asked { try m.session.coProducerSetAsked(asked: asked) }
                return .done(.coProducerRequest(doc: docID.uuidString, request: row, undoLabel: m.snapshot.undoLabel))
            } catch {
                return .failed(failure(error))
            }
        case .coProducerEnd:
            guard let (docID, m) = doc else { return noDocument }
            do {
                _ = try m.session.coProducerEnd()
            } catch {
                return .failed(failure(error))
            }
            refresh(m)
            return .done(.coProducerRequest(
                doc: docID.uuidString, request: m.session.coProducerRequest(), undoLabel: m.snapshot.undoLabel))
        case .coProducerStatus:
            guard let (docID, m) = doc else { return noDocument }
            return .done(.coProducerRequest(
                doc: docID.uuidString, request: m.session.coProducerRequest(), undoLabel: m.snapshot.undoLabel))
        case .coProducerPreview(let plan, _):
            guard let (docID, m) = doc else { return noDocument }
            do {
                return .done(.coProducerPreview(doc: docID.uuidString, preview: try m.session.coProducerPreview(plan: plan)))
            } catch {
                return .failed(failure(error))
            }
        case .coProducerApply(let token, let plan, _, let client):
            guard let (docID, m) = doc else { return noDocument }
            return approve(
                Apply(token: token, plan: plan, doc: docID, stamp: stamp), client: client, m, store, host,
                peer: peer, approver: approver)
        case .coProducerNotes:
            guard let (docID, m) = doc else { return noDocument }
            do {
                return .done(.coProducerNotes(doc: docID.uuidString, notes: try m.session.coProducerNotes()))
            } catch {
                return .failed(failure(error))
            }
        case .coProducerNote(let text, _):
            guard let (docID, m) = doc else { return noDocument }
            do {
                try m.session.coProducerNote(text: text)
            } catch {
                return .failed(failure(error))
            }
            return .done(.coProducerRequest(
                doc: docID.uuidString, request: m.session.coProducerRequest(), undoLabel: m.snapshot.undoLabel))
        case .coProducerExportFolder(let what, let parent, _):
            guard let (docID, m) = doc else { return noDocument }
            do {
                // The date is the app's, never Claude's (Ruling 6).
                let path = try m.session.coProducerExportFolder(what: what, date: String(stamp.prefix(10)), parent: parent)
                return .done(.coProducerFolder(doc: docID.uuidString, path: path))
            } catch {
                return .failed(failure(error))
            }
        case .coProducerExport(let what, let parent, let strips, _):
            guard let (docID, m) = doc else { return noDocument }
            return export(what, parent: parent, strips: strips, docID, m, store, host, stamp: stamp)
        default:
            return .failed(.of(.notAllowed, "Co-Producer cannot make that call."))
        }
    }

    /// Enter, take, every planned command, leave: one stretch (Ruling 13).
    /// A command that fails stops the rest, and every command has its line.
    /// `approval` is VizDAW's own (plan 8b Ruling 11).
    static func apply(
        _ token: String, _ plan: [PlannedRow], _ docID: UUID, _ m: DocumentModel, _ store: DocumentStore,
        _ host: any ControlHost, stamp: String, approval: CoProducerApproval
    ) -> ControlOutcome {
        do {
            _ = try m.session.coProducerEnter(stamp: stamp)
        } catch {
            return .failed(failure(error))
        }
        defer { m.session.coProducerLeave() }
        do {
            try m.session.coProducerTake(token: token, plan: plan, approval: approval)
        } catch {
            return .failed(failure(error))
        }
        var results: [String] = []
        var failed = false
        for p in plan {
            if failed {
                results.append("\(p.id): \(notRun)")
                continue
            }
            if case .failed(let f) = command(p.id, p.args, docID, m, store, host) {
                failed = true
                results.append("\(p.id): \(f.message)")
            } else {
                results.append("\(p.id): done")
            }
        }
        return .done(.coProducerApplied(doc: docID.uuidString, results: results, undoLabel: m.snapshot.undoLabel))
    }

    /// One registry command as the channel runs it, an alert it raised
    /// taken as its failure (no sheet opens).
    static func command(
        _ id: String, _ args: [ControlArg], _ docID: UUID, _ m: DocumentModel, _ store: DocumentStore,
        _ host: any ControlHost
    ) -> ControlOutcome {
        let before = m.alert?.id
        let o = ControlCommands.run(id, args: args, doc: (docID, m), store: store, host: host)
        return recorded(takeAlert(o, m, before: before), id, args, docID, m)
    }

    /// The commands that write outside the song (8a Ruling 1's outward
    /// class), whose files the request's entry lists (plan 8b Ruling 5).
    public static let outward: Set<String> = [
        "file.bounce", "file.export_midi", "file.export_midi_clip", "file.export_dawproject", "file.export_aaf",
        "file.export_for_daw",
    ]

    /// The chores that change or remove the composer's work (plan 8c
    /// Rulings 2–4): only through `co_producer.apply`, after a yes.
    static let changesWork: Set<String> = ControlCommands.chores.subtracting(["marker.add"])

    /// The file outward command `id` writes: its `path`, unless it is a dry
    /// run, which writes nothing.
    static func outwardPath(_ id: String, _ args: [ControlArg]) -> String? {
        guard outward.contains(id), args.flag("dry_run") != true else { return nil }
        return args.text("path")
    }

    /// Outward jobs still running, by document: their files go in the
    /// request's entry when they end.
    static var outwardJobs: [UUID: Set<UInt64>] = [:]

    /// `outcome` of command `id`, with what it wrote recorded for the
    /// request's entry: its path now when it succeeded at once, the files
    /// its job wrote when the job ends (`jobEnded`; plan 8c Task 4), and so
    /// on for work still running.
    static func recorded(
        _ outcome: ControlOutcome, _ id: String, _ args: [ControlArg], _ docID: UUID, _ m: DocumentModel
    ) -> ControlOutcome {
        guard let path = outwardPath(id, args) else { return outcome }
        switch outcome {
        case .done, .opened:
            // The request may have ended meanwhile; the file is then in no entry.
            try? m.session.coProducerFile(path: path)
        case .job(_, let doc, let job):
            outwardJobs[doc, default: []].insert(job)
        case .pending(let p):
            return .pending(PendingOutcome(after: p) { recorded($0, id, args, docID, m) })
        case .failed, .measure, .awaitingApproval:
            break
        }
        return outcome
    }

    /// The files job `j` wrote, each once: a bounce's files, else an
    /// export's `written`. A failed job's are those it had already written.
    static func written(_ j: JobRow, _ m: DocumentModel) -> [String] {
        let paths = j.kind == .bounce ? ((try? m.session.bounceResult(job: j.id)) ?? nil)?.files.map(\.path) ?? [] : j.written
        var seen = Set<String>()
        return paths.filter { seen.insert($0).inserted }
    }

    /// Job `job` of `doc` ended, well or not: the files an outward job
    /// Co-Producer ran really wrote go in the request's entry, once each,
    /// once per job (plan 8c Task 4, replacing 8b's `path`). `m` is nil when
    /// the document closed.
    public static func jobEnded(doc: UUID, job: UInt64, _ m: DocumentModel?) {
        guard outwardJobs[doc]?.remove(job) != nil else { return }
        if outwardJobs[doc]?.isEmpty == true { outwardJobs[doc] = nil }
        guard let m, let j = try? m.session.job(id: job) else { return }
        // The request may have ended meanwhile; the files are then in no entry.
        for path in written(j, m) { try? m.session.coProducerFile(path: path) }
    }

    // MARK: The one-call export (plan 8c Ruling 1)

    /// `co_producer.export`, in one stretch on the main thread with nothing
    /// awaited (8a Ruling 13): enter, then the core's plan (the app's date,
    /// never Claude's), which refuses as Busy before making its folder
    /// whatever the command's start would refuse, then the plan's one
    /// command as the menus run it, then leave. A command that still does
    /// not start drops the folder while it is empty, with its notes line
    /// (plan 8c final review). The answer comes when the job ends.
    static func export(
        _ what: ExportWhatRow, parent: String?, strips: [UInt64]?, _ docID: UUID, _ m: DocumentModel,
        _ store: DocumentStore, _ host: any ControlHost, stamp: String
    ) -> ControlOutcome {
        do {
            _ = try m.session.coProducerEnter(stamp: stamp)
        } catch {
            return .failed(failure(error))
        }
        defer { m.session.coProducerLeave() }
        let plan: ExportPlanRow
        do {
            plan = try m.session.coProducerExportPlan(
                what: what, date: String(stamp.prefix(10)), parent: parent, strips: strips)
        } catch {
            return .failed(failure(error))
        }
        let started = command(plan.id, plan.args, docID, m, store, host)
        if case .failed = started { _ = try? m.session.coProducerDropExport(folder: plan.folder) }
        return exported(started, folder: plan.folder, docID, store)
    }

    /// The export's answer from the outcome its command `started`: for a
    /// job, `{doc, folder, written, summary}` once it ends.
    static func exported(_ started: ControlOutcome, folder: String, _ docID: UUID, _ store: DocumentStore)
        -> ControlOutcome
    {
        switch started {
        case .job(_, let doc, let job):
            let before = store.model(doc)?.alert?.id
            return .pending(PendingOutcome(wait: .milliseconds(Int64(ControlJobs.waitMs))) { expired in
                exportEnded(doc, job, folder: folder, store, alertBefore: before, expired: expired)
            })
        case .pending(let p):
            return .pending(PendingOutcome(after: p) { exported($0, folder: folder, docID, store) })
        default:
            return started
        }
    }

    /// The jobs whose answer names their report's summary (Ruling 1).
    static let reported: Set<JobKindRow> = [.exportDawproject, .exportAaf, .exportMidi]

    /// Export job `job`'s answer once it ended (nil while it runs, unless
    /// `expired`), as the channel answers a Co-Producer job: a failure
    /// alert it left is the call's failure and is cleared, so no sheet
    /// opens (Ruling P8), and its files are recorded either way.
    static func exportEnded(
        _ doc: UUID, _ job: UInt64, folder: String, _ store: DocumentStore, alertBefore: UUID?, expired: Bool
    ) -> ControlOutcome? {
        guard let m = store.model(doc) else {
            jobEnded(doc: doc, job: job, nil)
            return .failed(.of(.noDocument, "the document closed"))
        }
        let j: JobRow
        do {
            j = try m.session.job(id: job)
        } catch {
            if case VizError.Busy = error, !expired { return nil }
            jobEnded(doc: doc, job: job, m)
            return .failed(.of(.failed, "the job is unknown"))
        }
        switch m.jobAnswer(j) {
        case .wait:
            guard expired else { return nil }
            jobEnded(doc: doc, job: job, m)
            return .failed(.of(.timeout, ControlJobs.expiredMessage(j)))
        case .ok:
            m.refreshAfterJob()
            jobEnded(doc: doc, job: job, m)
            if let a = takeNewAlert(m, before: alertBefore) { return .failed(.alert(a)) }
            return .done(.coProducerExported(
                doc: doc.uuidString, folder: folder, written: written(j, m),
                summary: reported.contains(j.kind) ? j.report?.summary : nil))
        case .failed(let message):
            m.refreshAfterJob()
            _ = takeNewAlert(m, before: alertBefore)
            jobEnded(doc: doc, job: job, m)
            return .failed(.of(.failed, message))
        }
    }

    /// `outcome`, or the alert `m` shows that was not there before (`before`)
    /// as its failure; that alert is cleared either way (Ruling P8), so no
    /// sheet opens for a Co-Producer call or its job.
    public static func takeAlert(_ outcome: ControlOutcome, _ m: DocumentModel, before: UUID?) -> ControlOutcome {
        guard let a = takeNewAlert(m, before: before) else { return outcome }
        if case .failed = outcome { return outcome }
        return .failed(.alert(a))
    }

    /// The alert `m` shows that was not there before (`before`), cleared.
    public static func takeNewAlert(_ m: DocumentModel, before: UUID?) -> AlertItem? {
        guard let a = m.alert, a.id != before else { return nil }
        m.alert = nil
        return a
    }

    /// Every document's request: ended when idle for `idleMs`, or, when
    /// `all` (the switch went off), ended now, its approval sheet a no. A
    /// request whose document has an approval sheet open is not idle.
    public static func tidy(_ store: DocumentStore, all: Bool) {
        if all { refuseAll(store) }
        for (id, m) in store.documents {
            let ended: Bool
            if !all, waiting[id] != nil { continue }
            if all {
                ended = m.session.coProducerRequest() != nil && (try? m.session.coProducerEnd()) != nil
            } else {
                ended = m.session.coProducerIdle(idleMs: CoProducerSettings.idleMs) != nil
            }
            if ended { refresh(m) }
        }
    }

    /// "yyyy-MM-dd HH-mm" in local time: the saved copy's stamp (Ruling 9).
    public static func stamp(_ date: Date) -> String {
        let f = DateFormatter()
        f.locale = Locale(identifier: "en_US_POSIX")
        f.calendar = Calendar(identifier: .gregorian)
        f.timeZone = .current
        f.dateFormat = "yyyy-MM-dd HH-mm"
        return f.string(from: date)
    }

    /// `settings.co_producer` and `file.open_copy` on the developer
    /// channel (Ruling 10); refused on the Co-Producer socket by class.
    static func runAppWide(_ id: String, args: [ControlArg], store: DocumentStore, host: any ControlHost)
        -> ControlOutcome
    {
        switch id {
        case setting:
            guard let want = args.flag("on") else {
                return .failed(.of(.invalidParams, "\(setting): `on` is true or false"))
            }
            host.coProducer.setOn(want)
            if want, !host.coProducer.on {
                return .failed(.of(.failed, host.coProducer.problem ?? "Co-Producer could not start."))
            }
            return .done(ControlCommands.ran(id, nil))
        case openCopy:
            guard let path = args.text("path") else { return .failed(.dialog("Open", "path")) }
            let before = store.alert?.id
            if let doc = store.openCopy(path: path) { return .opened(command: id, doc: doc) }
            if let a = store.alert, a.id != before {
                _ = store.takeAlert()
                return .failed(.of(.failed, a.message))
            }
            return .failed(.of(.failed, "\(id) opened nothing."))
        default:
            return .failed(.of(.unknownId, "VizDAW has no handler for \(id)."))
        }
    }

    /// A Co-Producer failure: Busy stays `busy`; anything else is
    /// `failed` with the session's own words.
    static func failure(_ error: any Error) -> ControlFailure {
        if case VizError.Busy(let message) = error { return .of(.busy, message) }
        return .of(.failed, AlertItem(error, title: "Co-Producer").message)
    }

    static let noDocument = ControlOutcome.failed(.of(.noDocument, "No document is open."))

    /// The document again after its request ended (the Undo title).
    static func refresh(_ m: DocumentModel) {
        if let s = try? m.session.snapshot() { m.apply(s) }
    }
}

// MARK: VizDAW's own approval (plan 8b Ruling 11; spec §6.8)

/// One apply waiting for the composer on VizDAW's sheet. The channel keeps
/// it until `outcome` is set, and answers the call with that.
@MainActor
public final class CoProducerApprovalWait {
    let apply: CoProducerCommands.Apply
    let pending: PendingApproval
    /// The document's version when the sheet opened: a change is a no.
    let version: UInt64
    let started: Date
    let wait: TimeInterval
    let approver: any CoProducerApprover
    /// The call's answer, once the composer (or the clock) decided.
    public internal(set) var outcome: ControlOutcome?
    /// Whether the caller still waits; the channel sets it.
    public var callerWaiting: (@MainActor () -> Bool)?

    init(
        apply: CoProducerCommands.Apply, pending: PendingApproval, version: UInt64, started: Date,
        wait: TimeInterval, approver: any CoProducerApprover
    ) {
        self.apply = apply
        self.pending = pending
        self.version = version
        self.started = started
        self.wait = wait
        self.approver = approver
    }
}

extension CoProducerCommands {
    /// How long the sheet waits for the composer.
    public static let approvalWait: TimeInterval = 600
    /// Ruling 11's answers.
    public static let notAllowed = "The composer did not allow this in VizDAW."
    public static let askingAlready = "VizDAW is already asking the composer about another change."
    public static let noApprovalWaiting = "No Co-Producer approval is waiting."
    public static let confirmFirst = "Confirm the warning first: VizDAW can't check which program this is."
    public static let cannotName = "Always allow needs a program VizDAW can name."
    /// The e2e's answer to the sheet (developer channel, test hooks only).
    public static let answerHook = "settings.co_producer_answer"

    /// An apply's call, kept while the sheet waits.
    struct Apply {
        let token: String
        let plan: [PlannedRow]
        let doc: UUID
        let stamp: String
    }

    /// The sheets waiting, by document: at most one each.
    static var waiting: [UUID: CoProducerApprovalWait] = [:]

    /// The wait open on `doc`, if any.
    public static func approvalWait(for doc: UUID) -> CoProducerApprovalWait? { waiting[doc] }

    /// Ruling 11's flow for an apply: a bad token or a preview that needs
    /// no yes goes straight to the stretch with no approval (the session
    /// refuses or takes it); a standing approval for the caller runs it
    /// with `.standing`; otherwise the sheet opens and the call waits.
    static func approve(
        _ a: Apply, client: String?, _ m: DocumentModel, _ store: DocumentStore, _ host: any ControlHost,
        peer: PeerRow?, approver: any CoProducerApprover
    ) -> ControlOutcome {
        if waiting[a.doc] != nil { return .failed(.of(.busy, askingAlready)) }
        guard let preview = try? m.session.coProducerPending(token: a.token, plan: a.plan), preview.needsYes else {
            return apply(a.token, a.plan, a.doc, m, store, host, stamp: a.stamp, approval: .none)
        }
        let settings = host.coProducer
        let identity = identify(peer, client, settings)
        if identity.key != nil, standingApproval(for: identity, in: settings.approvals.load()) != nil {
            return apply(a.token, a.plan, a.doc, m, store, host, stamp: a.stamp, approval: .standing)
        }
        let pending = PendingApproval(doc: a.doc, identity: identity, label: client, items: preview.items)
        let w = CoProducerApprovalWait(
            apply: a, pending: pending, version: m.snapshot.version, started: settings.now(),
            wait: settings.approvalWait, approver: approver)
        waiting[a.doc] = w
        let handler: @MainActor (ApprovalAnswer) -> String? = { answer in
            respond(w, answer, store, host)
        }
        m.coProducerApproval = CoProducerApprovalSheetModel(pending: pending, song: m.snapshot.title, respond: handler)
        approver.ask(pending) { _ = handler($0) }
        // An approver that answered at once (a test's) leaves nothing waiting.
        if let o = w.outcome { return o }
        return .awaitingApproval(doc: a.doc)
    }

    /// The caller's identity, worked out once per connection and label.
    static func identify(_ peer: PeerRow?, _ client: String?, _ settings: CoProducerSettings) -> ClientIdentity {
        guard let peer else { return .unknown }
        let cacheKey = "\(peer.conn)\n\(client ?? "")"
        if let id = settings.identities[cacheKey] { return id }
        let id = ClientIdentity.decide(peer: peer, label: client, facts: settings.facts)
        settings.identities[cacheKey] = id
        return id
    }

    /// The composer's `answer` to `w`'s sheet: nil when taken, else why it
    /// was refused (the sheet then stays open).
    static func respond(
        _ w: CoProducerApprovalWait, _ answer: ApprovalAnswer, _ store: DocumentStore, _ host: any ControlHost
    ) -> String? {
        guard waiting[w.apply.doc] === w, w.outcome == nil else { return noApprovalWaiting }
        guard case .allow(let always, let confirmed) = answer else {
            refuse(w, store, cancel: false)
            return nil
        }
        let settings = host.coProducer
        if always {
            switch w.pending.identity {
            case .unknown:
                return cannotName
            case .unverified(let key, let path, let label) where confirmed:
                settings.grant(StandingApproval(
                    key: "unverified:" + key, verified: false,
                    name: CoProducerApprovalSheetModel.appName(w.pending), team: nil, identifier: nil, path: path,
                    label: label, granted: settings.now()))
            case .unverified:
                store.model(w.apply.doc)?.coProducerApproval?.step = .warning
                return confirmFirst
            case .known(let key, let team, let identifier, let name):
                settings.grant(StandingApproval(
                    key: "signed:" + key, verified: true, name: name, team: team, identifier: identifier, path: nil,
                    label: nil, granted: settings.now()))
            }
        }
        waiting[w.apply.doc] = nil
        guard let m = store.model(w.apply.doc) else {
            w.outcome = .failed(.of(.failed, notAllowed))
            return nil
        }
        m.coProducerApproval = nil
        let a = w.apply
        w.outcome = apply(a.token, a.plan, a.doc, m, store, host, stamp: a.stamp, approval: .sheet)
        return nil
    }

    /// `w` is a no: answered "The composer did not allow this in VizDAW.",
    /// its token dropped, its sheet closed; `cancel`: the composer did not
    /// answer, so the approver is told.
    static func refuse(_ w: CoProducerApprovalWait, _ store: DocumentStore, cancel: Bool) {
        guard waiting[w.apply.doc] === w, w.outcome == nil else { return }
        waiting[w.apply.doc] = nil
        w.outcome = .failed(.of(.failed, notAllowed))
        if let m = store.model(w.apply.doc) {
            m.session.coProducerForget(token: w.apply.token)
            m.coProducerApproval = nil
        }
        if cancel { w.approver.cancel(doc: w.apply.doc) }
    }

    /// Every open sheet is a no (the switch went off, quit).
    static func refuseAll(_ store: DocumentStore) {
        for w in Array(waiting.values) { refuse(w, store, cancel: true) }
    }

    /// Each tick: a sheet whose 10 minutes are up, whose song changed,
    /// whose caller left, whose document closed, or with the switch off,
    /// is a no.
    public static func checkApprovals(store: DocumentStore, host: any ControlHost, now: Date) {
        for w in Array(waiting.values) {
            let m = store.model(w.apply.doc)
            let stale =
                m == nil || !host.coProducer.on || now >= w.started.addingTimeInterval(w.wait)
                || m?.snapshot.version != w.version || w.callerWaiting?() == false
            if stale { refuse(w, store, cancel: true) }
        }
    }

    /// `settings.co_producer_answer` (Ruling 11's test hook): answers the
    /// open sheet of `doc` as a real press would. Only with
    /// `VIZDAW_TEST_HOOKS=1`; its answer is what the sheet asked.
    static func runAnswerHook(
        _ args: [ControlArg], doc: (id: UUID, model: DocumentModel)?, store: DocumentStore, host: any ControlHost
    ) -> ControlOutcome {
        guard host.coProducer.testHooks else { return .failed(.of(.notAllowed, "test hook")) }
        guard let allow = args.flag("allow") else {
            return .failed(.of(.invalidParams, "\(answerHook): `allow` is true or false"))
        }
        guard let (docID, _) = doc, let w = waiting[docID], w.outcome == nil else {
            return .failed(.of(.failed, noApprovalWaiting))
        }
        let p = w.pending
        let asked = ControlResult.coProducerApprovalAnswered(
            app: CoProducerApprovalSheetModel.appName(p), kind: p.identity.kind, label: p.label,
            path: p.identity.path, items: p.items)
        let answer: ApprovalAnswer =
            allow ? .allow(always: args.flag("always") ?? false, confirmed: args.flag("confirm") ?? false) : .deny
        if let refused = respond(w, answer, store, host) { return .failed(.of(.notAllowed, refused)) }
        return .done(asked)
    }
}
