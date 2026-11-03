import Foundation

/// A file's size and modification time: the stable rule compares them
/// (spec §11; plan 5f-3 Ruling 4).
public struct FileStamp: Equatable, Sendable {
    public let size: UInt64
    public let modified: Date

    public init(size: UInt64, modified: Date) {
        self.size = size
        self.modified = modified
    }
}

/// Reads a path's stamp; nil when it is missing or not a regular file.
public protocol FileProbe {
    func stamp(_ path: String) -> FileStamp?
}

/// The disk's stamps (`FileManager` attributes).
public struct DiskProbe: FileProbe {
    public init() {}

    public func stamp(_ path: String) -> FileStamp? {
        guard let a = try? FileManager.default.attributesOfItem(atPath: path),
            (a[.type] as? FileAttributeType) == .typeRegular,
            let size = (a[.size] as? NSNumber)?.uint64Value,
            let modified = a[.modificationDate] as? Date
        else { return nil }
        return FileStamp(size: size, modified: modified)
    }
}

/// Watch events for one path (the app: a `DispatchSource`; tests: a fake).
/// The token stops the watch when released. Nil when the path cannot be
/// watched (it is missing): the model then waits for Reload (pre-flight
/// M15).
@MainActor
public protocol FileWatching {
    func watch(_ path: String, _ changed: @escaping @MainActor () -> Void) -> AnyObject?
}

/// Watching for tests and for documents the app gave no tools: records
/// the paths and sends events only when told (`fire`).
@MainActor
public final class FakeWatching: FileWatching {
    @MainActor final class Token {
        let path: String
        let changed: @MainActor () -> Void

        init(path: String, changed: @escaping @MainActor () -> Void) {
            self.path = path
            self.changed = changed
        }
    }

    private struct Weak {
        weak var token: Token?
    }

    private var tokens: [Weak] = []
    /// Every path a watch started on, in order.
    public private(set) var started: [String] = []

    public init() {}

    public func watch(_ path: String, _ changed: @escaping @MainActor () -> Void) -> AnyObject? {
        started.append(path)
        let t = Token(path: path, changed: changed)
        tokens.append(Weak(token: t))
        return t
    }

    /// Watches still held.
    public var live: Int { tokens.filter { $0.token != nil }.count }

    /// A watch event for `path`, to every live watch on it.
    public func fire(_ path: String) {
        for w in tokens {
            if let t = w.token, t.path == path { t.changed() }
        }
    }
}

/// The 1 s stable rule for one edit copy (Ruling 4): a change is due once
/// the file's stamp has stayed the same for `stableSeconds` since the last
/// change, exists, and is neither the stamp last applied nor the one last
/// skipped.
@MainActor
public final class ExternalEditWatcher {
    public static let stableSeconds: TimeInterval = 1

    public let path: String
    private let probe: any FileProbe
    private let now: () -> Date
    private var token: AnyObject?
    /// The last change: when, and the stamp then (nil: missing).
    private var change: (at: Date, stamp: FileStamp?)?
    /// The stamp last imported (or written by Open).
    public private(set) var appliedStamp: FileStamp?
    /// The stamp of the last import that failed or was cancelled: not
    /// retried by itself.
    private var skipped: FileStamp?
    public private(set) var isStopped = false

    /// Watches `path`; `applied` is the stamp already in the project (the
    /// copy Open wrote: pre-flight I10).
    public init(
        path: String, probe: any FileProbe, watching: any FileWatching, now: @escaping () -> Date,
        applied: FileStamp? = nil
    ) {
        self.path = path
        self.probe = probe
        self.now = now
        self.appliedStamp = applied
        token = watching.watch(path) { [weak self] in self?.changed() }
    }

    /// A watch event (Reload calls it too): the stamp now, and when.
    public func changed() {
        guard !isStopped else { return }
        change = (now(), probe.stamp(path))
    }

    /// The stamp to import now, or nil. A stamp that differs from the last
    /// change's is a change of its own (an event lost or coalesced, a file
    /// still growing; pre-flight M1): the second starts again from now.
    public func due() -> FileStamp? {
        guard !isStopped, let c = change else { return nil }
        let s = probe.stamp(path)
        if s != c.stamp {
            change = (now(), s)
            return nil
        }
        guard let s, s != appliedStamp, s != skipped,
            now().timeIntervalSince(c.at) >= Self.stableSeconds
        else { return nil }
        return s
    }

    /// `s` was imported: it is never due again.
    public func applied(_ s: FileStamp) {
        appliedStamp = s
        if change?.stamp == s { change = nil }
    }

    /// `s` failed or was cancelled: not retried until the file changes
    /// (Reload still retries it).
    public func skip(_ s: FileStamp) {
        skipped = s
    }

    /// Reload's check (no second's wait: the user asked): the stamp now
    /// when it exists and differs from the one applied.
    public func changedSinceApplied() -> FileStamp? {
        guard let s = probe.stamp(path), s != appliedStamp else { return nil }
        return s
    }

    /// Stops watching (the document closes, the region is gone).
    public func stop() {
        isStopped = true
        token = nil
        change = nil
    }
}
