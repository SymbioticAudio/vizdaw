import Foundation
import Testing
@testable import VizDAWAppModel
import VizDAWCore

/// Who is asking (plan 8b Task 6, Ruling 11; spec §6.8): an app VizDAW can
/// name by its code signature, an unverified host, or nobody it can name;
/// and standing approvals, which never cross kinds. No Keychain, no real
/// process: `FakeClientFacts` and `MemoryApprovalStore` only.
@MainActor
@Suite struct CoProducerClientsTests {
    typealias F = FakeClientFacts
    let label = "Helper Chat"
    var known: ClientIdentity {
        .known(key: "ABCDE12345:com.example.chat", team: F.team, identifier: "com.example.chat", name: "Helper Chat")
    }

    func decide(_ f: FakeClientFacts, peer: PeerRow? = F.peerRow(), label: String? = "Helper Chat") -> ClientIdentity {
        ClientIdentity.decide(peer: peer, label: label, facts: f)
    }

    func unverified(_ path: String, _ label: String? = "Helper Chat") -> ClientIdentity {
        .unverified(key: "\(path)\n\(label ?? "")", path: path, label: label)
    }

    @Test func aSignedHostIsKnown() {
        let id = decide(F.goodChain())
        #expect(id == known)
        #expect(id.key == "signed:ABCDE12345:com.example.chat")
        // The label never keys or names a known app.
        #expect(decide(F.goodChain(), label: "Something Else") == known)
        #expect(decide(F.goodChain(), label: nil) == known)
        // Without a display name: the bundle's own name.
        let f = F.goodChain()
        f.names = [:]
        #expect(decide(f) == .known(
            key: "ABCDE12345:com.example.chat", team: F.team, identifier: "com.example.chat", name: "Helper Chat"))
    }

    @Test func unverifiedParentsAreNeverKnown() {
        let helperPath = F.goodChain().processes[F.helperPid]!.path!
        let mcpPath = F.mcp().path!
        func expect(_ why: String, host: String = helperPath, _ change: (FakeClientFacts) -> Void) {
            let f = F.goodChain()
            change(f)
            let id = decide(f)
            #expect(id == unverified(host), "\(why): \(id)")
            #expect(id.key == "unverified:\(host)\nHelper Chat", "\(why)")
        }
        // A peer that is not VizDAW's own vizdaw-mcp: the host is the peer.
        expect("another cdhash", host: mcpPath) { $0.peers[F.mcpPid] = F.mcp().with { $0.cdhash = F.otherHash } }
        expect("a raw client", host: "/usr/local/bin/raw-client") {
            $0.peers[F.mcpPid] = CodeFacts.fake(
                pid: F.mcpPid, parent: F.appPid, team: F.team, identifier: "com.example.chat",
                path: "/usr/local/bin/raw-client", cdhash: F.otherHash, launch: ["raw-client"])
        }
        expect("no code", host: mcpPath) {
            $0.peers[F.mcpPid] = F.mcp().with { $0.valid = false; $0.cdhash = nil }
        }
        expect("an invalid signature with our hash", host: mcpPath) {
            $0.peers[F.mcpPid] = F.mcp().with { $0.valid = false }
        }
        expect("VizDAW has no hash of its own", host: mcpPath) { $0.mcpHash = nil }
        // The parent chain.
        expect("an unsigned parent") {
            $0.processes[F.helperPid] = $0.processes[F.helperPid]!.with { $0.valid = false; $0.team = nil; $0.cdhash = nil }
        }
        expect("an ad-hoc parent") { $0.processes[F.helperPid] = $0.processes[F.helperPid]!.with { $0.team = nil } }
        expect("a platform shell", host: "/bin/zsh") {
            $0.processes[F.helperPid] = $0.processes[F.helperPid]!.with {
                $0.team = nil
                $0.identifier = "com.example.zsh"
                $0.path = "/bin/zsh"
            }
        }
        expect("an invalid signature in the chain") {
            $0.processes[F.appPid] = $0.processes[F.appPid]!.with { $0.valid = false }
        }
        expect("reparented to launchd", host: "/sbin/launchd") { $0.peers[F.mcpPid] = F.mcp(parent: 1) }
        expect("mixed teams") {
            // The app, then a shell, then the same team's helper.
            $0.processes[350] = CodeFacts.fake(
                pid: 350, parent: F.appPid, team: nil, identifier: "com.example.zsh", path: "/bin/zsh",
                launch: ["-zsh"])
            $0.processes[F.helperPid] = $0.processes[F.helperPid]!.with { $0.parent = 350 }
        }
        expect("another team in the chain") {
            $0.processes[F.helperPid] = $0.processes[F.helperPid]!.with { $0.team = "ZZZZZ99999" }
        }
        expect("a top that is no app") { $0.processes[F.appPid] = $0.processes[F.appPid]!.with { $0.appBundle = nil } }
        expect("a top whose parent is not 1") {
            $0.processes[F.appPid] = $0.processes[F.appPid]!.with { $0.parent = 2 }
        }
        expect("a top with no identifier") {
            $0.processes[F.appPid] = $0.processes[F.appPid]!.with { $0.identifier = nil }
        }
        expect("not hardened") { $0.processes[F.appPid] = $0.processes[F.appPid]!.with { $0.hardened = false } }
        expect("a helper not hardened") {
            $0.processes[F.helperPid] = $0.processes[F.helperPid]!.with { $0.hardened = false }
        }
        expect("get-task-allow") {
            $0.processes[F.helperPid] = $0.processes[F.helperPid]!.with { $0.getTaskAllow = true }
        }
        for bad in [
            "DYLD_INSERT_LIBRARIES=/tmp/x.dylib", "ELECTRON_RUN_AS_NODE=1", "NODE_OPTIONS=--require /tmp/x.js",
            "--inspect=9229", "--remote-debugging-port=1", "--inspect-brk", "DYLD_LIBRARY_PATH=/tmp",
        ] {
            expect("launched with \(bad)") {
                $0.processes[F.appPid] = $0.processes[F.appPid]!.with { $0.launch.append(bad) }
            }
            expect("a helper launched with \(bad)") {
                $0.processes[F.helperPid] = $0.processes[F.helperPid]!.with { $0.launch.append(bad) }
            }
        }
        expect("a launch list that cannot be read") {
            $0.processes[F.appPid] = $0.processes[F.appPid]!.with { $0.launch = [] }
        }
        expect("the app's pid was reused") { $0.laterStart[F.appPid] = 9 }
        expect("the helper's pid was reused") { $0.laterStart[F.helperPid] = 9 }
        expect("the peer restarted") { $0.laterStart[F.mcpPid] = 9 }
        expect("a chain longer than 32") { f in
            // 40 helpers of the same team between vizdaw-mcp and the app.
            var parent = F.appPid
            for i in 0..<40 {
                let pid = Int32(1_000 + i)
                f.processes[pid] = f.processes[F.helperPid]!.with {
                    $0.pid = pid
                    $0.parent = parent
                }
                parent = pid
            }
            f.processes[F.helperPid] = f.processes[F.helperPid]!.with { $0.parent = parent }
        }
        expect("a chain that loops") {
            $0.processes[F.appPid] = $0.processes[F.appPid]!.with { $0.parent = F.helperPid }
        }
        // The label is half the key.
        let soft = F.goodChain().with { $0.processes[F.appPid] = $0.processes[F.appPid]!.with { $0.hardened = false } }
        #expect(decide(soft, label: nil) == unverified(helperPath, nil))
        #expect(decide(soft, label: nil).key == "unverified:\(helperPath)\n")
    }

    @Test func nobodyItCanNameIsUnknown() {
        #expect(decide(F.goodChain(), peer: nil) == .unknown)
        #expect(ClientIdentity.unknown.key == nil)
        #expect(decide(F.goodChain(), peer: PeerRow(conn: 1, auditToken: [1, 2, 3])) == .unknown)
        #expect(decide(F.goodChain(), peer: F.peerRow(4_242)) == .unknown, "a peer that is gone")
        let f = F.goodChain()
        f.peers[F.mcpPid] = F.mcp().with { $0.cdhash = F.otherHash; $0.path = nil }
        #expect(decide(f) == .unknown, "a raw client whose path cannot be read")
        let g = F.goodChain()
        g.processes[F.helperPid] = g.processes[F.helperPid]!.with { $0.team = nil; $0.path = nil }
        #expect(decide(g) == .unknown, "a host whose path cannot be read")
        let h = F.goodChain()
        h.processes[F.helperPid] = nil
        #expect(decide(h) == .unknown, "a parent that is gone")
    }

    @Test func forgedClientInfoIsOnlyALabel() throws {
        let rig = try ApprovalRig(
            facts: F.goodChain().with { $0.processes[F.helperPid] = $0.processes[F.helperPid]!.with { $0.team = nil } })
        rig.store(StandingApproval(
            key: "signed:ABCDE12345:com.example.chat", verified: true, name: "Helper Chat", team: F.team,
            identifier: "com.example.chat", path: nil, label: nil, granted: rig.now))
        let o = rig.apply(peer: F.peerRow(), client: "Helper Chat")
        #expect(rig.isAwaiting(o))
        let asked = try #require(rig.approver.asked.first)
        let helperPath = F.goodChain().processes[F.helperPid]!.path!
        #expect(asked.identity == .unverified(key: "\(helperPath)\nHelper Chat", path: helperPath, label: "Helper Chat"))
        #expect(asked.label == "Helper Chat")
        #expect(rig.f.coTakeApprovals.isEmpty, "nothing applied")
        let sheet = try #require(rig.m.coProducerApproval)
        #expect(sheet.message
            == "An app VizDAW cannot identify (it calls itself \"Helper Chat\") asks to make these changes to \"\(rig.song)\":")
    }

    @Test func theKindsNeverCross() throws {
        // An unverified approval naming exactly the good chain's app.
        let rig = try ApprovalRig()
        rig.store(StandingApproval(
            key: "unverified:\(F.appPath)\nHelper Chat", verified: false, name: "Helper Chat", team: nil,
            identifier: nil, path: F.appPath, label: "Helper Chat", granted: rig.now))
        #expect(rig.isAwaiting(rig.apply(peer: F.peerRow(), client: "Helper Chat")))
        #expect(rig.approver.asked.count == 1, "a verified app never picks up an unverified approval")

        // A verified approval for the app, when the same app is started
        // from a shell and so does not verify.
        let shell = F.goodChain()
        shell.processes[350] = CodeFacts.fake(
            pid: 350, parent: 1, team: nil, identifier: "com.example.zsh", path: "/bin/zsh", launch: ["-zsh"])
        shell.processes[F.appPid] = shell.processes[F.appPid]!.with { $0.parent = 350 }
        let rig2 = try ApprovalRig(facts: shell)
        rig2.store(StandingApproval(
            key: "signed:ABCDE12345:com.example.chat", verified: true, name: "Helper Chat", team: F.team,
            identifier: "com.example.chat", path: nil, label: nil, granted: rig2.now))
        #expect(rig2.isAwaiting(rig2.apply(peer: F.peerRow(), client: "Helper Chat")))
        #expect(rig2.approver.asked.count == 1)

        // Path and label both match, or nothing does.
        func approval(_ key: String, verified: Bool) -> StandingApproval {
            StandingApproval(
                key: key, verified: verified, name: "x", team: nil, identifier: nil, path: nil, label: nil,
                granted: rig.now)
        }
        let pa = approval("unverified:/p\nA", verified: false)
        #expect(standingApproval(for: unverified("/p", "A"), in: [pa]) == pa)
        #expect(standingApproval(for: unverified("/p", "B"), in: [pa]) == nil)
        #expect(standingApproval(for: unverified("/q", "A"), in: [pa]) == nil)
        #expect(standingApproval(for: unverified("/p", nil), in: [pa]) == nil)
        #expect(standingApproval(for: .unknown, in: [pa]) == nil)
        // Hand-made entries whose flag and key disagree match nothing.
        #expect(standingApproval(for: unverified("/p", "A"), in: [approval("unverified:/p\nA", verified: true)]) == nil)
        #expect(standingApproval(for: known, in: [approval("signed:ABCDE12345:com.example.chat", verified: false)])
            == nil)
        #expect(standingApproval(for: known, in: [approval("ABCDE12345:com.example.chat", verified: true)]) == nil)
        let good = approval("signed:ABCDE12345:com.example.chat", verified: true)
        #expect(standingApproval(for: known, in: [pa, good]) == good)
        // An unverified identity whose path holds a forged "signed:" text.
        #expect(standingApproval(
            for: .unverified(key: "x", path: "x", label: nil), in: [approval("signed:x", verified: true)]) == nil)
    }

    @Test func revokedApprovalAsksAgain() throws {
        let rig = try ApprovalRig()
        let cliKey = "unverified:\(F.cliPath)\nHelper CLI"
        rig.store(StandingApproval(
            key: "signed:ABCDE12345:com.example.chat", verified: true, name: "Helper Chat", team: F.team,
            identifier: "com.example.chat", path: nil, label: nil, granted: rig.now))
        rig.store(StandingApproval(
            key: cliKey, verified: false, name: "Helper CLI", team: nil, identifier: nil, path: F.cliPath,
            label: "Helper CLI", granted: rig.now))
        // Both apply with no sheet, as a standing approval.
        #expect(rig.isApplied(rig.apply(peer: F.peerRow(conn: 1), client: "Helper Chat")))
        #expect(rig.isApplied(rig.apply(peer: F.peerRow(F.cliPid, conn: 2), client: "Helper CLI")))
        #expect(rig.approver.asked.isEmpty)
        #expect(rig.f.coTakeApprovals == [.standing, .standing])

        // Settings lists both; only the unverified one says so.
        let settings = rig.host.coProducer
        settings.readApprovals()
        #expect(settings.approvalRows.map(\.text) == [
            "Helper Chat — team ABCDE12345 · com.example.chat · since 2026-11-16",
            "Helper CLI — unverified · \(F.cliPath) · since 2026-11-16",
        ])
        #expect(!settings.approvalRows[0].accessibility.contains("unverified"))
        #expect(settings.approvalRows[1].accessibility.contains("unverified"))

        // Revoke the verified one: it asks; the other still applies.
        settings.revoke("signed:ABCDE12345:com.example.chat")
        #expect(rig.isAwaiting(rig.apply(peer: F.peerRow(conn: 1), client: "Helper Chat")))
        rig.approver.answer(.deny)
        #expect(rig.isApplied(rig.apply(peer: F.peerRow(F.cliPid, conn: 2), client: "Helper CLI")))
        #expect(rig.approver.asked.count == 1)

        // Revoke the unverified one too.
        settings.revoke(cliKey)
        #expect(rig.isAwaiting(rig.apply(peer: F.peerRow(F.cliPid, conn: 2), client: "Helper CLI")))
        rig.approver.answer(.deny)
        #expect(rig.approver.asked.count == 2)
        #expect(settings.approvalRows.isEmpty)
        #expect(settings.approvalsEmptyLine == "None. VizDAW asks before every change.")

        // Revoke All empties the store.
        rig.store(StandingApproval(
            key: cliKey, verified: false, name: "Helper CLI", team: nil, identifier: nil, path: F.cliPath,
            label: "Helper CLI", granted: rig.now))
        settings.readApprovals()
        #expect(settings.approvalRows.count == 1)
        settings.revokeAll()
        #expect(rig.approvals.load().isEmpty)
        #expect(settings.approvalRows.isEmpty)
    }

    @Test func anUnreadableStoreAsks() throws {
        let rig = try ApprovalRig()
        rig.approvals.data = Data("{not json".utf8)
        #expect(rig.approvals.load().isEmpty)
        #expect(rig.isAwaiting(rig.apply(peer: F.peerRow(), client: "Helper Chat")))
        #expect(rig.approver.asked.count == 1)
    }

    @Test func approvalsRoundTripAsJson() throws {
        let a = StandingApproval(
            key: "unverified:/p\nA", verified: false, name: "A", team: nil, identifier: nil, path: "/p", label: "A",
            granted: Date(timeIntervalSince1970: 1_000))
        let store = MemoryApprovalStore()
        try store.save([a])
        #expect(store.load() == [a])
        #expect(StandingApproval.decodeAll(Data()) == [])
        #expect(StandingApproval.decodeAll(Data("[{\"key\": 1}]".utf8)) == [])
    }
}

extension FakeClientFacts {
    /// A command-line client of the socket: not VizDAW's `vizdaw-mcp`.
    static let cliPid: Int32 = 600
    static let cliPath = "/usr/local/bin/helper-cli"

    /// These facts with `change` made.
    func with(_ change: (FakeClientFacts) -> Void) -> FakeClientFacts {
        change(self)
        return self
    }
}
