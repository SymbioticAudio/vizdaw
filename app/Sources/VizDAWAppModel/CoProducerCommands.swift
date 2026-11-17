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
    public static func run(
        _ request: ControlRequest, doc: (id: UUID, model: DocumentModel)?, store: DocumentStore,
        host: any ControlHost, stamp: String
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
            guard let (docID, m) = doc else { return noDocument }
            do {
                _ = try m.session.coProducerEnter(stamp: stamp)
            } catch {
                return .failed(failure(error))
            }
            defer { m.session.coProducerLeave() }
            return command(id, args, docID, m, store, host)
        case .coProducerBegin(let name, _, _):
            // `asked` waits for plan 8b Task 5.
            guard let (docID, m) = doc else { return noDocument }
            do {
                let row = try m.session.coProducerBegin(name: name)
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
        case .coProducerApply(let token, let plan, _):
            guard let (docID, m) = doc else { return noDocument }
            return apply(token, plan, docID, m, store, host, stamp: stamp)
        case .coProducerNotes, .coProducerNote, .coProducerExportFolder:
            // Plan 8b Task 5 builds these.
            return .failed(.of(.unavailable, "Not ready yet."))
        default:
            return .failed(.of(.notAllowed, "Co-Producer cannot make that call."))
        }
    }

    /// Enter, take, every planned command, leave: one stretch (Ruling 13).
    /// A command that fails stops the rest, and every command has its line.
    static func apply(
        _ token: String, _ plan: [PlannedRow], _ docID: UUID, _ m: DocumentModel, _ store: DocumentStore,
        _ host: any ControlHost, stamp: String
    ) -> ControlOutcome {
        do {
            _ = try m.session.coProducerEnter(stamp: stamp)
        } catch {
            return .failed(failure(error))
        }
        defer { m.session.coProducerLeave() }
        do {
            try m.session.coProducerTake(token: token, plan: plan)
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
        return takeAlert(o, m, before: before)
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
    /// `all` (the switch went off), ended now.
    public static func tidy(_ store: DocumentStore, all: Bool) {
        for m in store.documents.values {
            let ended: Bool
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
