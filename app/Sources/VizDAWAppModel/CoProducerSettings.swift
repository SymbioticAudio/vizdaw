import Foundation
import Observation
import VizDAWCore

/// What the Co-Producer switch asks of the app (plan 8a Ruling 10): the
/// live sink opens and closes the Co-Producer socket and writes the off
/// marker; tests pass a fake that does neither.
@MainActor
public protocol CoProducerSink {
    /// Starts listening at `path`: nil when listening, else the reason it
    /// could not.
    func start(path: String) -> String?
    /// Stops: queued calls are answered `shutting_down`, connections drop.
    func stop()
    /// Writes (`true`) or removes (`false`) `<support>/co-producer.off`.
    func markOff(_ off: Bool)
}

/// Settings › Co-Producer (plan 8a Ruling 10): the switch, off unless the
/// composer turned it on, a preference of this Mac. On, the app listens on
/// `<support>/co-producer.sock` for `vizdaw-mcp`; off, there is no socket,
/// and `<support>/co-producer.off` tells `vizdaw-mcp` why it cannot connect.
@Observable
@MainActor
public final class CoProducerSettings {
    public static let key = "VizDAW.coProducer"
    public static let socketName = "co-producer.sock"
    /// A request with no Co-Producer call this long ends (Ruling 3).
    public static let idleMs: UInt64 = 300_000
    public static let onSentence = "Co-Producer is on: Claude can reach VizDAW through vizdaw-mcp."
    public static let offSentence = "Co-Producer is off: Claude cannot reach VizDAW."

    @ObservationIgnored private let defaults: UserDefaults
    @ObservationIgnored private let sink: any CoProducerSink
    /// The support folder the socket and the off marker live in.
    public let supportDir: URL
    /// The app ends every open request here when the switch goes off.
    @ObservationIgnored public var didTurnOff: (@MainActor () -> Void)?

    /// Listening. False when the key is absent or not a Bool.
    public private(set) var on = false
    /// "Co-Producer could not start: <reason>", after a failed start.
    public private(set) var problem: String?
    /// The saved copies of the open saved projects, as `readCopies` found.
    public private(set) var copies = CopiesSizeRow(count: 0, bytes: 0)

    /// Who is asking (plan 8b Ruling 11): `DarwinClientFacts` in the app.
    @ObservationIgnored public var facts: any ClientFacts
    /// Standing approvals: the login Keychain in the app; memory in tests
    /// and with `VIZDAW_TEST_HOOKS=1`.
    @ObservationIgnored public var approvals: any CoProducerApprovalStore
    /// The launch environment (`VIZDAW_TEST_HOOKS`, `VIZDAW_APPROVAL_WAIT_MS`).
    @ObservationIgnored public var environment: [String: String]
    /// The clock approvals are dated and timed by.
    @ObservationIgnored public var now: @MainActor () -> Date = { Date() }
    /// Identities worked out this listening session, by connection and
    /// label (Ruling 11). Connection numbers restart with the server, so
    /// this is emptied whenever the switch turns on or off.
    @ObservationIgnored var identities: [String: ClientIdentity] = [:]
    /// Settings › Co-Producer's list, as `readApprovals` found it.
    public private(set) var approvalRows: [ApprovalRow] = []

    /// Reads the switch and, when it is on, starts at once.
    public init(
        defaults: UserDefaults, supportDir: URL, sink: any CoProducerSink, facts: any ClientFacts = NoClientFacts(),
        approvals: any CoProducerApprovalStore = MemoryApprovalStore(), environment: [String: String] = [:]
    ) {
        self.defaults = defaults
        self.supportDir = supportDir
        self.sink = sink
        self.facts = facts
        self.approvals = approvals
        self.environment = environment
        if defaults.object(forKey: Self.key) as? Bool == true {
            turnOn()
        } else {
            sink.markOff(true)
        }
    }

    /// The socket's path.
    public var socketPath: String { supportDir.appendingPathComponent(Self.socketName).path }

    /// Ruling 10's sentence for the switch as it is.
    public var status: String { on ? Self.onSentence : Self.offSentence }

    /// The switch (Ruling 10): kept, then started or stopped.
    public func setOn(_ want: Bool) {
        if want {
            guard !on else { return }
            turnOn()
        } else {
            problem = nil
            guard on else {
                defaults.set(false, forKey: Self.key)
                return
            }
            sink.stop()
            on = false
            identities = [:]
            didTurnOff?()
            sink.markOff(true)
            defaults.set(false, forKey: Self.key)
        }
    }

    /// The app quits: the server stops and requests end; the off marker
    /// is left as it is (Ruling 10).
    public func quit() {
        guard on else { return }
        sink.stop()
        on = false
        identities = [:]
        didTurnOff?()
    }

    private func turnOn() {
        identities = [:]
        sink.markOff(false)
        if let why = sink.start(path: socketPath) {
            problem = "Co-Producer could not start: \(why)"
            on = false
            sink.markOff(true)
            defaults.set(false, forKey: Self.key)
            return
        }
        problem = nil
        on = true
        defaults.set(true, forKey: Self.key)
    }

    /// Counts the saved copies of `store`'s open saved projects.
    public func readCopies(_ store: DocumentStore) {
        var count: UInt64 = 0
        var bytes: UInt64 = 0
        for m in store.documents.values where !m.needsSaveAs {
            guard let c = try? m.session.coProducerCopiesSize() else { continue }
            count += c.count
            bytes += c.bytes
        }
        copies = CopiesSizeRow(count: count, bytes: bytes)
    }

    /// "Saved copies in open projects: N (X MB)".
    public var copiesLine: String {
        let mb = String(format: "%.1f", Double(copies.bytes) / 1_000_000)
        return "Saved copies in open projects: \(copies.count) (\(mb) MB)"
    }

    // MARK: VizDAW's own approval (plan 8b Ruling 11)

    /// One row of "Apps that may change songs without asking".
    public struct ApprovalRow: Identifiable, Equatable, Sendable {
        /// The approval's key.
        public let id: String
        public let text: String
        public let accessibility: String
    }

    public static let approvalsTitle = "Apps that may change songs without asking"
    public let approvalsEmptyLine = "None. VizDAW asks before every change."

    /// `settings.co_producer_answer` and the shortened wait work only with
    /// `VIZDAW_TEST_HOOKS=1` at launch.
    public var testHooks: Bool { environment["VIZDAW_TEST_HOOKS"] == "1" }

    /// How long the sheet waits for the composer: 10 minutes, or with test
    /// hooks `VIZDAW_APPROVAL_WAIT_MS`, at least a second and never longer.
    public var approvalWait: TimeInterval {
        let full = CoProducerCommands.approvalWait
        guard testHooks, let ms = environment["VIZDAW_APPROVAL_WAIT_MS"].flatMap({ Int($0) }),
            ms <= Int(full * 1000)
        else { return full }
        return TimeInterval(max(ms, 1000)) / 1000
    }

    /// Reads the standing approvals for the list.
    public func readApprovals() {
        let day = DateFormatter()
        day.locale = Locale(identifier: "en_US_POSIX")
        day.calendar = Calendar(identifier: .gregorian)
        day.timeZone = .current
        day.dateFormat = "yyyy-MM-dd"
        approvalRows = approvals.load().map { a in
            let since = "since \(day.string(from: a.granted))"
            if a.verified {
                let text = "\(a.name) — team \(a.team ?? "?") · \(a.identifier ?? "?") · \(since)"
                return ApprovalRow(id: a.key, text: text, accessibility: text)
            }
            let path = a.path ?? "?"
            let text = "\(a.label ?? path) — unverified · \(path) · \(since)"
            return ApprovalRow(id: a.key, text: text, accessibility: "\(a.label ?? path), unverified, \(path), \(since)")
        }
    }

    /// Revoke: the approval stops at once; the next request asks again.
    public func revoke(_ key: String) {
        try? approvals.save(approvals.load().filter { $0.key != key })
        readApprovals()
    }

    /// Revoke All.
    public func revokeAll() {
        try? approvals.save([])
        readApprovals()
    }

    /// Keeps a new standing approval, replacing one with the same key.
    func grant(_ a: StandingApproval) {
        try? approvals.save(approvals.load().filter { $0.key != a.key } + [a])
    }

    /// The support folder as `vizdaw-mcp` finds it (Task 5's
    /// `support_dir_from_env`), so the socket and the screenshots it
    /// reads are where it looks: `VIZDAW_SUPPORT_DIR` when set (nil
    /// unless absolute), else `$HOME/Library/Application Support/VizDAW`
    /// (nil unless `HOME` is absolute).
    public nonisolated static func supportPath(environment: [String: String]) -> String? {
        if let d = environment["VIZDAW_SUPPORT_DIR"] {
            return d.hasPrefix("/") ? d : nil
        }
        guard let home = environment["HOME"], home.hasPrefix("/") else { return nil }
        return URL(fileURLWithPath: home).appendingPathComponent("Library/Application Support/VizDAW").path
    }
}
