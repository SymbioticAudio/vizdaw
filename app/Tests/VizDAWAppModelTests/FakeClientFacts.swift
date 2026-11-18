import Foundation
@testable import VizDAWAppModel
import VizDAWCore

/// The facts `ClientIdentity.decide` reads, made up per test (plan 8b
/// Task 6): no Security or Darwin call, no real process. Every app name
/// here is invented.
final class FakeClientFacts: ClientFacts {
    /// The peer, by the pid in its audit token's word 5.
    var peers: [Int32: CodeFacts] = [:]
    /// Every other process, by pid.
    var processes: [Int32: CodeFacts] = [:]
    /// A start time a pid reports from its second read on (pid reuse).
    var laterStart: [Int32: UInt64] = [:]
    var mcpHash: Data? = FakeClientFacts.ourHash
    var names: [URL: String] = [:]
    private var reads: [Int32: Int] = [:]

    static let ourHash = Data([0xAB, 0xCD, 0x01])
    static let otherHash = Data([0x12, 0x34])
    static let team = "ABCDE12345"
    static let appPath = "/Applications/Helper Chat.app/Contents/MacOS/Helper Chat"
    static let app = URL(fileURLWithPath: "/Applications/Helper Chat.app")
    static let mcpPid: Int32 = 500
    static let helperPid: Int32 = 400
    static let appPid: Int32 = 300

    func peer(auditToken: [UInt32]) -> CodeFacts? {
        guard auditToken.count == 8 else { return nil }
        return read(peers, Int32(bitPattern: auditToken[5]))
    }
    func process(pid: Int32) -> CodeFacts? { read(processes, pid) }
    func bundledMcpHash() -> Data? { mcpHash }
    func bundleName(_ app: URL) -> String? { names[app] }

    private func read(_ table: [Int32: CodeFacts], _ pid: Int32) -> CodeFacts? {
        guard let f = table[pid] else { return nil }
        reads[pid, default: 0] += 1
        guard reads[pid, default: 0] > 1, let later = laterStart[pid] else { return f }
        return f.with { $0.started = later }
    }

    /// The good chain (Task 6 brief): VizDAW's own `vizdaw-mcp`, under a
    /// helper of team ABCDE12345, under `/Applications/Helper Chat.app`
    /// (the same team, "com.example.chat", hardened), whose parent is 1.
    static func goodChain() -> FakeClientFacts {
        let f = FakeClientFacts()
        f.peers[mcpPid] = mcp()
        f.processes[helperPid] = CodeFacts.fake(
            pid: helperPid, parent: appPid, team: team, identifier: "com.example.chat.helper",
            path: "/Applications/Helper Chat.app/Contents/Frameworks/Helper Chat Helper.app/Contents/MacOS/Helper Chat Helper",
            launch: ["Helper Chat Helper", "--type=utility"])
        f.processes[appPid] = CodeFacts.fake(
            pid: appPid, parent: 1, team: team, identifier: "com.example.chat", path: appPath,
            appBundle: app, launch: [appPath, "HOME=/Users/composer"])
        f.processes[1] = CodeFacts.fake(
            pid: 1, parent: 0, team: nil, identifier: "com.example.launchd", path: "/sbin/launchd",
            launch: ["/sbin/launchd"])
        f.names[app] = "Helper Chat"
        // A command-line client on the socket itself (not vizdaw-mcp).
        f.peers[cliPid] = CodeFacts.fake(
            pid: cliPid, parent: 350, team: nil, identifier: "helper-cli", path: cliPath, cdhash: otherHash,
            launch: ["helper-cli"])
        return f
    }

    /// VizDAW's own `vizdaw-mcp`, child of the helper.
    static func mcp(parent: Int32 = helperPid) -> CodeFacts {
        CodeFacts.fake(
            pid: mcpPid, parent: parent, team: nil, identifier: "vizdaw-mcp",
            path: "/Applications/VizDAW.app/Contents/MacOS/vizdaw-mcp", cdhash: ourHash,
            launch: ["vizdaw-mcp"])
    }

    /// The audit token of `pid`, as the socket reads it.
    static func token(_ pid: Int32) -> [UInt32] { [1, 2, 3, 4, 5, UInt32(bitPattern: pid), 7, 8] }

    /// The peer row of `pid` on connection `conn`.
    static func peerRow(_ pid: Int32 = mcpPid, conn: UInt64 = 1) -> PeerRow {
        PeerRow(conn: conn, auditToken: token(pid))
    }
}

extension CodeFacts {
    /// A valid, signed, hardened process with the given facts.
    static func fake(
        pid: Int32, parent: Int32, team: String?, identifier: String?, path: String?,
        appBundle: URL? = nil, cdhash: Data? = Data([0x01]), launch: [String]
    ) -> CodeFacts {
        CodeFacts(
            pid: pid, parent: parent, started: 1_000 + UInt64(pid), valid: true, team: team,
            identifier: identifier, cdhash: cdhash, hardened: true, getTaskAllow: false, path: path,
            appBundle: appBundle, launch: launch)
    }

    /// These facts with `change` made.
    func with(_ change: (inout Draft) -> Void) -> CodeFacts {
        var d = Draft(self)
        change(&d)
        return d.facts
    }

    /// A mutable copy for tests.
    struct Draft {
        var pid: Int32, parent: Int32, started: UInt64, valid: Bool, team: String?, identifier: String?
        var cdhash: Data?, hardened: Bool, getTaskAllow: Bool, path: String?, appBundle: URL?, launch: [String]
        init(_ f: CodeFacts) {
            (pid, parent, started, valid, team, identifier) = (f.pid, f.parent, f.started, f.valid, f.team, f.identifier)
            (cdhash, hardened, getTaskAllow, path, appBundle, launch) =
                (f.cdhash, f.hardened, f.getTaskAllow, f.path, f.appBundle, f.launch)
        }
        var facts: CodeFacts {
            CodeFacts(
                pid: pid, parent: parent, started: started, valid: valid, team: team, identifier: identifier,
                cdhash: cdhash, hardened: hardened, getTaskAllow: getTaskAllow, path: path, appBundle: appBundle,
                launch: launch)
        }
    }
}
