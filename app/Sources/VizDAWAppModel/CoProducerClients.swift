import Foundation
import VizDAWCore

/// What VizDAW reads about one process (plan 8b Ruling 11): its code
/// signature, as Security checks it, and its process facts. The app reads
/// them through `DarwinClientFacts`; tests make them up.
public struct CodeFacts: Equatable, Sendable {
    public let pid: Int32
    public let parent: Int32
    /// When the process started (`proc_bsdinfo`'s start time, in µs):
    /// read twice, it tells a reused pid apart.
    public let started: UInt64
    /// `SecCodeCheckValidity` passed.
    public let valid: Bool
    /// The signing team ID; nil for unsigned, ad-hoc and platform code.
    public let team: String?
    /// The signing identifier.
    public let identifier: String?
    /// The code directory hash (`kSecCodeInfoUnique`).
    public let cdhash: Data?
    /// Signed with the hardened runtime.
    public let hardened: Bool
    /// Holds the `get-task-allow` entitlement (a debugger may attach).
    public let getTaskAllow: Bool
    /// The executable's path (`proc_pidpath`).
    public let path: String?
    /// The `.app` whose main executable this is.
    public let appBundle: URL?
    /// Its arguments and environment (`KERN_PROCARGS2`); empty when they
    /// cannot be read.
    public let launch: [String]

    public init(
        pid: Int32, parent: Int32, started: UInt64, valid: Bool, team: String?, identifier: String?, cdhash: Data?,
        hardened: Bool, getTaskAllow: Bool, path: String?, appBundle: URL?, launch: [String]
    ) {
        self.pid = pid
        self.parent = parent
        self.started = started
        self.valid = valid
        self.team = team
        self.identifier = identifier
        self.cdhash = cdhash
        self.hardened = hardened
        self.getTaskAllow = getTaskAllow
        self.path = path
        self.appBundle = appBundle
        self.launch = launch
    }
}

/// Where `ClientIdentity.decide` gets its facts: `DarwinClientFacts` in the
/// app, `FakeClientFacts` in tests.
public protocol ClientFacts {
    /// The process an audit token names, found by the token itself (so a
    /// reused pid is never mistaken for it); nil when it is gone.
    func peer(auditToken: [UInt32]) -> CodeFacts?
    /// Process `pid`; nil when it cannot be read.
    func process(pid: Int32) -> CodeFacts?
    /// The cdhash of the `vizdaw-mcp` inside VizDAW's own bundle.
    func bundledMcpHash() -> Data?
    /// `CFBundleDisplayName`, else `CFBundleName`, of `app`.
    func bundleName(_ app: URL) -> String?
}

/// Facts that name nobody: every caller is unknown.
public struct NoClientFacts: ClientFacts {
    public init() {}
    public func peer(auditToken: [UInt32]) -> CodeFacts? { nil }
    public func process(pid: Int32) -> CodeFacts? { nil }
    public func bundledMcpHash() -> Data? { nil }
    public func bundleName(_ app: URL) -> String? { nil }
}

/// Who asks for a change (plan 8b Ruling 11; spec §6.8).
public enum ClientIdentity: Equatable, Sendable {
    /// An app VizDAW identified by its code signature, through its own
    /// `vizdaw-mcp`. `key` is "<team>:<identifier>".
    case known(key: String, team: String, identifier: String, name: String)
    /// Any other host whose path VizDAW can read. `key` is
    /// "<path>\n<label>"; both halves can be copied by any program.
    case unverified(key: String, path: String, label: String?)
    /// No peer token, or no readable path: no key, so no Always allow.
    case unknown

    /// The most processes between `vizdaw-mcp` and `launchd`.
    public static let maxChain = 32

    /// The store key: "signed:<team>:<identifier>", "unverified:<path>\n<label>",
    /// or nil.
    public var key: String? {
        switch self {
        case .known(let k, _, _, _): return "signed:" + k
        case .unverified(let k, _, _): return "unverified:" + k
        case .unknown: return nil
        }
    }

    /// "known", "unverified" or "unknown".
    public var kind: String {
        switch self {
        case .known: return "known"
        case .unverified: return "unverified"
        case .unknown: return "unknown"
        }
    }

    /// The host's path, when unverified.
    public var path: String? {
        if case .unverified(_, let p, _) = self { return p }
        return nil
    }

    /// Ruling 11's identity of the peer that sent `label`:
    /// - known only when the peer is VizDAW's own `vizdaw-mcp` (its cdhash
    ///   the bundled one's) and its parent chain up to `launchd` is valid
    ///   code of one team, every process hardened, without
    ///   `get-task-allow` and launched without injection or debugging,
    ///   topped by an `.app`'s main executable whose parent is pid 1, and
    ///   no pid was reused while it was read. The label never counts.
    /// - else unverified, keyed by the host's path and the label: the
    ///   `vizdaw-mcp`'s parent, or for any other peer the peer itself.
    /// - else unknown.
    public static func decide(peer: PeerRow?, label: String?, facts: any ClientFacts) -> ClientIdentity {
        guard let peer, peer.auditToken.count == 8, let me = facts.peer(auditToken: peer.auditToken) else {
            return .unknown
        }
        let ours = me.valid && me.cdhash != nil && me.cdhash == facts.bundledMcpHash()
        if ours, let k = known(me, peer.auditToken, facts) { return k }
        let host = ours ? facts.process(pid: me.parent) : me
        guard let path = host?.path, !path.isEmpty else { return .unknown }
        return .unverified(key: path + "\n" + (label ?? ""), path: path, label: label)
    }

    /// The known identity of `mcp`'s host chain, or nil.
    private static func known(_ mcp: CodeFacts, _ token: [UInt32], _ facts: any ClientFacts) -> ClientIdentity? {
        var chain: [CodeFacts] = []
        var pid = mcp.parent
        while pid != 1 {
            guard pid > 1, chain.count < maxChain, let f = facts.process(pid: pid), f.pid == pid else { return nil }
            chain.append(f)
            pid = f.parent
        }
        guard let top = chain.last, let app = top.appBundle, let identifier = top.identifier, !identifier.isEmpty,
            let team = top.team, !team.isEmpty
        else { return nil }
        for f in chain {
            guard f.valid, f.team == team, f.hardened, !f.getTaskAllow, cleanLaunch(f.launch) else { return nil }
        }
        // Pid reuse: every process is read again and must be the same one.
        for f in chain {
            guard let again = facts.process(pid: f.pid), again.started == f.started, again.parent == f.parent else {
                return nil
            }
        }
        guard let again = facts.peer(auditToken: token), again.started == mcp.started, again.parent == mcp.parent
        else { return nil }
        let name = facts.bundleName(app) ?? app.deletingPathExtension().lastPathComponent
        return .known(key: team + ":" + identifier, team: team, identifier: identifier, name: name)
    }

    /// Launched as itself: its launch list was read and holds no library
    /// injection, Node or Electron override, or debugger port.
    static func cleanLaunch(_ launch: [String]) -> Bool {
        guard !launch.isEmpty else { return false }
        let refused = ["DYLD_", "ELECTRON_RUN_AS_NODE", "NODE_OPTIONS", "--inspect", "--remote-debugging"]
        return !launch.contains { entry in refused.contains { entry.hasPrefix($0) } }
    }
}

/// One app the composer always allows (plan 8b Ruling 11).
public struct StandingApproval: Codable, Equatable, Sendable {
    /// `ClientIdentity.key`: "signed:…" or "unverified:…".
    public let key: String
    /// True for a known app, false for an unverified host.
    public let verified: Bool
    /// The app's name, or an unverified host's label or path.
    public let name: String
    public let team: String?
    public let identifier: String?
    public let path: String?
    public let label: String?
    public let granted: Date

    public init(
        key: String, verified: Bool, name: String, team: String?, identifier: String?, path: String?,
        label: String?, granted: Date
    ) {
        self.key = key
        self.verified = verified
        self.name = name
        self.team = team
        self.identifier = identifier
        self.path = path
        self.label = label
        self.granted = granted
    }

    /// The store's JSON; anything unreadable is no approvals, so VizDAW asks.
    public static func decodeAll(_ data: Data) -> [StandingApproval] {
        (try? JSONDecoder().decode([StandingApproval].self, from: data)) ?? []
    }

    public static func encodeAll(_ all: [StandingApproval]) throws -> Data {
        let e = JSONEncoder()
        e.outputFormatting = .sortedKeys
        return try e.encode(all)
    }
}

/// The approval for `identity`, if any: a known identity matches only
/// verified approvals and an unverified one only unverified approvals, by
/// exact key.
public func standingApproval(for identity: ClientIdentity, in all: [StandingApproval]) -> StandingApproval? {
    guard let key = identity.key else { return nil }
    switch identity {
    case .known: return all.first { $0.verified && $0.key.hasPrefix("signed:") && $0.key == key }
    case .unverified: return all.first { !$0.verified && $0.key.hasPrefix("unverified:") && $0.key == key }
    case .unknown: return nil
    }
}

/// Where standing approvals are kept: the login Keychain in the app
/// (`KeychainApprovalStore`), memory in tests and with `VIZDAW_TEST_HOOKS=1`.
public protocol CoProducerApprovalStore {
    /// Every approval; an unreadable store is empty.
    func load() -> [StandingApproval]
    func save(_ all: [StandingApproval]) throws
}

/// Approvals in memory, as the store's JSON (so bad bytes read as none).
public final class MemoryApprovalStore: CoProducerApprovalStore {
    /// The stored JSON.
    public var data: Data?

    public init() {}

    public func load() -> [StandingApproval] { data.map(StandingApproval.decodeAll) ?? [] }

    public func save(_ all: [StandingApproval]) throws { data = try StandingApproval.encodeAll(all) }
}
