import Foundation
import VizDAWCore

/// The editor `region.open_external` and keys use without `app` (plan
/// 5f-3 Ruling 5): none chosen, the chosen application's path, or the
/// stub (`VIZDAW_EXTERNAL_EDITOR=stub`), which needs none.
public enum ExternalEditorChoice: Equatable, Sendable {
    case none
    case app(String)
    case stub
}

/// What Reload External Edit (or an import the watcher starts) came to.
public enum ExternalOutcome: Equatable, Sendable {
    /// An import started (its job).
    case importing(UInt64)
    /// Nothing changed since the last import.
    case unchanged
    /// Not for this region; the message says why.
    case unavailable(String)
    /// The core is busy (an import runs); try again.
    case busy(String)
    /// Refused or gone; the message says why.
    case failed(String)
}

/// Opens an edit copy in the chosen application (Ruling 5).
@MainActor
public protocol ExternalOpener {
    /// Opens `file` with `app` (nil: the stub, which needs none). A
    /// refusal found at once throws; one the system reports later comes to
    /// `failed` (pre-flight M9).
    func open(_ file: URL, with app: URL?, failed: @escaping @MainActor @Sendable (String) -> Void) throws
}

/// Records what it would open and launches nothing (tests).
@MainActor
public final class FakeOpener: ExternalOpener {
    public struct Opened: Equatable, Sendable {
        public let file: URL
        public let app: URL?

        public init(file: URL, app: URL?) {
            self.file = file
            self.app = app
        }
    }

    public private(set) var opened: [Opened] = []
    /// Thrown by each open (an application refused at once).
    public var error: VizError?
    /// Reported after each open (the system's open failing later).
    public var lateFailure: String?

    public init() {}

    public func open(_ file: URL, with app: URL?, failed: @escaping @MainActor @Sendable (String) -> Void) throws {
        if let error { throw error }
        opened.append(Opened(file: file, app: app))
        if let lateFailure { failed(lateFailure) }
    }
}

/// `VIZDAW_EXTERNAL_EDITOR=stub` (spec §12.1; Ruling 5): records the copy
/// and launches nothing; the e2e edits the file itself.
@MainActor
public final class StubOpener: ExternalOpener {
    public private(set) var last: URL?

    public init() {}

    public func open(_ file: URL, with app: URL?, failed: @escaping @MainActor @Sendable (String) -> Void) throws {
        last = file
    }
}

/// How documents open and watch edit copies (pre-flight I7(a)): the app
/// sets the live ones on the store (`VizDAWApp`); fakes otherwise, so no
/// test launches an application or watches the disk.
@MainActor
public struct ExternalTools {
    public var opener: any ExternalOpener
    public var watching: any FileWatching
    public var probe: any FileProbe
    public var now: () -> Date

    public init(
        opener: any ExternalOpener = FakeOpener(), watching: any FileWatching = FakeWatching(),
        probe: any FileProbe = DiskProbe(), now: @escaping () -> Date = { Date() }
    ) {
        self.opener = opener
        self.watching = watching
        self.probe = probe
        self.now = now
    }
}

/// One document's external edits (spec §11; plan 5f-3 Rulings 4, 5, 7):
/// Open writes and opens a copy and watches it; each tick imports the
/// copies that are due; Reload imports by hand; closing stops every
/// watch. Imports the tick or the menu start go through
/// `DocumentModel.startJob`, so their banner and a failure's single alert
/// come from `pollJobs` (pre-flight I7(d)); this model only reads its
/// jobs to know when a stamp was applied.
@MainActor
public final class ExternalEditModel {
    public static let openFailedTitle = "Open in External Editor Failed"
    public static let failedTitle = "External Edit Failed"
    public static let goneMessage = "The edit copy is gone; Open in External Editor again."
    public static let noEditMessage = "This region has no external edit; use Open in External Editor."
    public static let importingMessage = "The external edit is being imported."
    public static let waitingMessage = "The external edit waits for the media job to finish."

    private unowned let document: DocumentModel
    private let opener: any ExternalOpener
    private let watching: any FileWatching
    private let probe: any FileProbe
    private let now: () -> Date
    private var watchers: [UInt64: ExternalEditWatcher] = [:]
    private struct Running {
        let job: UInt64
        let stamp: FileStamp
    }
    private var running: [UInt64: Running] = [:]
    /// The stamp each region's copy had when Open wrote it.
    private var written: [UInt64: FileStamp] = [:]
    /// Regions whose import waits on another job; told once (fold check M1).
    private var waiting: Set<UInt64> = []
    private var closed = false

    public init(
        document: DocumentModel, opener: any ExternalOpener, watching: any FileWatching,
        probe: any FileProbe = DiskProbe(), now: @escaping () -> Date = { Date() }
    ) {
        self.document = document
        self.opener = opener
        self.watching = watching
        self.probe = probe
        self.now = now
    }

    /// Regions whose edit is being imported.
    public var importing: Set<UInt64> { Set(running.keys) }
    /// Regions whose copy is watched.
    public var watched: Set<UInt64> { Set(watchers.keys) }

    /// Open in External Editor (Ruling 5): the core writes the copy, the
    /// opener opens it with `app`, and the copy is watched from the stamp
    /// it was written with (Reload right after it is unchanged; pre-flight
    /// I10). A refusal throws, and nothing is watched. A failure the
    /// system reports later is the alert "Open in External Editor Failed";
    /// the watch stays (the user can open the copy by hand).
    @discardableResult
    public func open(_ region: UInt64, app: URL?) throws -> String {
        let row = try document.session.openExternal(event: region)
        // The stamp the core wrote, read before an editor can rewrite the
        // file (fold check M5); Reload counts it as applied even when the
        // launch below fails (M6).
        let wrote = probe.stamp(row.path)
        written[region] = wrote
        try opener.open(URL(fileURLWithPath: row.path), with: app) { [weak d = document] message in
            d?.alert = AlertItem(title: ExternalEditModel.openFailedTitle, message: message)
        }
        watch(region, row.path, applied: wrote)
        return row.path
    }

    /// Reload External Edit (Ruling 7): imports the copy now when it
    /// differs from what was imported. `announce`: the import shows its
    /// banner and a failure's alert, as any job (the menu); the channel
    /// answers for itself.
    public func reload(_ region: UInt64, announce: Bool = true) -> ExternalOutcome {
        guard !closed else { return .unavailable(Self.noEditMessage) }
        poll()
        if running[region] != nil { return .busy(Self.importingMessage) }
        let state: ExternalEditStateRow
        do {
            state = try document.session.externalEdit(event: region)
        } catch {
            return refusal(error)
        }
        switch state {
        case .noEdit:
            return .unavailable(Self.noEditMessage)
        case .gone:
            return .failed(Self.goneMessage)
        case .copy(let row):
            // A watch the region lost or never had (a failed launch, a copy
            // forgotten then undone) starts from the stamp Open wrote, if
            // any (fold check M6).
            if watchers[region]?.path != row.path { watch(region, row.path, applied: written[region]) }
            guard let w = watchers[region] else { return .failed(Self.goneMessage) }
            guard let s = w.changedSinceApplied() else {
                return probe.stamp(row.path) == nil ? .failed(Self.goneMessage) : .unchanged
            }
            return start(region, w, s, announce: announce)
        }
    }

    /// The document's tick (`DocumentModel.idle`): ended imports are noted,
    /// then each copy that is due and has no import running starts one.
    public func tick() {
        guard !closed else { return }
        poll()
        for (region, w) in watchers where running[region] == nil {
            if let s = w.due() { _ = start(region, w, s, announce: true) }
        }
    }

    /// After an untitled project's Save As its copies are in the Trash
    /// (Ruling 3): their watches stop, so none polls a path that never
    /// returns (fold check M7).
    public func forgetGone() {
        for region in Array(watchers.keys) {
            if case .copy? = try? document.session.externalEdit(event: region) { continue }
            stopWatching(region)
        }
    }

    /// The document closes: every watch stops; nothing starts again.
    public func stopAll() {
        closed = true
        for w in watchers.values { w.stop() }
        watchers = [:]
        running = [:]
    }

    private func watch(_ region: UInt64, _ path: String, applied: FileStamp?) {
        watchers[region]?.stop()
        watchers[region] = ExternalEditWatcher(
            path: path, probe: probe, watching: watching, now: now, applied: applied)
    }

    private func stopWatching(_ region: UInt64) {
        watchers.removeValue(forKey: region)?.stop()
    }

    /// Starts importing `s` of `w`. A busy core is retried at the next
    /// tick, without an alert (pre-flight I11); any other refusal skips
    /// `s` (and, from the tick, is an alert).
    private func start(_ region: UInt64, _ w: ExternalEditWatcher, _ s: FileStamp, announce: Bool)
        -> ExternalOutcome
    {
        let path = w.path
        var refused: (any Error)?
        var job: UInt64?
        if announce {
            job = document.startJob(.externalEdit, Self.failedTitle, failure: { refused = $0 }) {
                try $0.applyExternalEdit(event: region, path: path)
            }
        } else {
            do {
                job = try document.session.applyExternalEdit(event: region, path: path)
            } catch {
                refused = error
            }
        }
        if let job {
            running[region] = Running(job: job, stamp: s)
            waiting.remove(region)
            return .importing(job)
        }
        let outcome = refusal(refused)
        if case .busy = outcome {
            // Retried at the next tick; said once (fold check M1).
            if announce, waiting.insert(region).inserted { document.editor.status = Self.waitingMessage }
            return outcome
        }
        waiting.remove(region)
        w.skip(s)
        if announce, case .failed(let why) = outcome {
            document.alert = AlertItem(title: Self.failedTitle, message: why)
        }
        forgetIfGone(region)
        return outcome
    }

    /// Notes ended imports: Done marks the stamp applied; Failed or
    /// Cancelled skips it, the watch kept (pre-flight I7(d), I8); a region
    /// the core forgot stops being watched (pre-flight M10).
    private func poll() {
        for (region, r) in running {
            let j: JobRow
            do {
                j = try document.session.job(id: r.job)
            } catch {
                if case .Busy? = error as? VizError { continue }
                // Not retried every tick (fold check M4).
                watchers[region]?.skip(r.stamp)
                running[region] = nil
                continue
            }
            guard j.state != .running else { continue }
            running[region] = nil
            if j.state == .done {
                watchers[region]?.applied(r.stamp)
            } else {
                watchers[region]?.skip(r.stamp)
                if j.state == .failed { forgetIfGone(region) }
            }
        }
    }

    /// Stops the watch of a region that is gone or whose copy the core
    /// forgot, so later saves in the editor raise nothing (pre-flight M10).
    /// A copy that is only missing stays watched for its return.
    private func forgetIfGone(_ region: UInt64) {
        switch try? document.session.externalEdit(event: region) {
        case .copy?, .gone?: return
        case .noEdit?, nil: stopWatching(region)
        }
    }

    private func refusal(_ error: (any Error)?) -> ExternalOutcome {
        switch error as? VizError {
        case .Busy(let message)?: return .busy(message)
        case let e?: return .failed(e.message)
        case nil: return .failed(error?.localizedDescription ?? "The external edit was refused.")
        }
    }
}
