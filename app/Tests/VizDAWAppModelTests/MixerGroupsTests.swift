import Foundation
import Testing
import VizDAWCore
@testable import VizDAWAppModel

/// The mixer's groups and VCAs (groups-vcas-transport Task 9): the Mixer
/// menu's edits, ⌥-drags, followers moving with a group drag, the VCA
/// offset readout, the VCA columns, the key path's selection and the VCA
/// strip's labels.
@MainActor
@Suite struct MixerGroupsTests {
    init() { _ = TestSupportDir.shared }

    /// Strips 2, 3 and 4, Bus 5 and Master; VCAs as given.
    static func groupsFake(vcas: [VcaRow] = [], groups: [GroupRow] = []) -> FakeSession {
        let f = demoFake()
        var strips = [
            stripRow(2, "Kick", kind: .audioPath(path: 1)),
            stripRow(3, "Snare", kind: .audioPath(path: 2)),
            stripRow(4, "Bass", kind: .audioPath(path: 3)),
            stripRow(5, "Bus 1"),
            stripRow(0, "Master", kind: .master),
        ]
        for i in strips.indices {
            strips[i].vcas = vcas.filter { $0.members.contains(strips[i].id) }.map(\.id)
            strips[i].group = groups.first { $0.members.contains(strips[i].id) }?.id
        }
        f.mixerSnap = MixerSnapshot(
            version: f.snap.version, strips: strips, buses: [BusRow(id: 5, name: "Bus 1")], groups: groups,
            vcas: vcas)
        return f
    }

    static func vca(_ id: UInt64, _ name: String, members: [UInt64], db: Double = 0) -> VcaRow {
        VcaRow(
            id: id, name: name, colour: .blue, members: members, gainDb: db, mute: false, solo: false,
            automation: .read)
    }

    func model(_ f: FakeSession) throws -> MixerModel { try MixerModel(document: DocumentModel(session: f)) }

    func args(_ a: [ControlArg]) -> [String] { a.map { "\($0.name)=\($0.value)" } }

    func strip(_ m: MixerModel, _ id: UInt64) throws -> StripRow { try #require(m.strips.first { $0.id == id }) }

    @Test func groupingTheSelectionRunsNewGroup() throws {
        let f = Self.groupsFake()
        let m = try model(f)
        m.document.selectedStrips = [3, 2, 0]
        #expect(m.canGroupSelection)
        m.groupSelection()
        #expect(f.mixerRuns.map(\.id) == ["mixer.new_group"])
        #expect(
            f.mixerRuns.map { args($0.args) } == [["strips=\(ControlValue.text(value: "2,3"))"]],
            "in ID order, master left out")
    }

    @Test func theMixerMenusRulesFollowTheCommands() throws {
        let g = GroupRow(id: 1, name: "Drums", colour: .red, members: [2, 3], linkMute: true, linkSolo: true)
        let m = try model(Self.groupsFake(groups: [g]))
        m.document.selectedStrips = [0]
        #expect(!m.canGroupSelection, "master alone: no strip to group")
        #expect(m.selectionGroup == nil)
        m.document.selectedStrips = [2, 3]
        #expect(m.selectionGroup == g, "the selection shares one group")
        #expect(!m.canGroupSelection, "a strip in a group is refused")
        m.document.selectedStrips = [2, 4]
        #expect(m.selectionGroup == nil, "Bass is in no group")
        m.document.selectedStrips = [4]
        #expect(m.canGroupSelection)
        m.document.selectedStrips = []
        #expect(!m.canGroupSelection && m.selectionGroup == nil)
    }

    @Test func ungroupDeletesTheSelectionsGroup() throws {
        let g = GroupRow(id: 7, name: "Drums", colour: .red, members: [2, 3], linkMute: true, linkSolo: true)
        let f = Self.groupsFake(groups: [g])
        let m = try model(f)
        m.document.selectedStrips = [2, 3]
        m.ungroupSelection()
        #expect(f.mixerRuns.map(\.id) == ["mixer.delete_group"])
        #expect(f.mixerRuns.map { args($0.args) } == [["group=\(ControlValue.int(value: 7))"]])
    }

    @Test func assignAndRemoveTakeOnlyWhatTheCoreWouldDo() throws {
        let v = Self.vca(1, "Drums", members: [2])
        let f = Self.groupsFake(vcas: [v])
        let m = try model(f)
        #expect(m.assignable([2, 3], to: v) == [3], "a member is not assigned again")
        #expect(m.vcasHolding([3]).isEmpty)
        #expect(m.vcasHolding([2, 3]) == [v])
        m.assign([3], toVca: 1, true)
        m.assign([2], toVca: 1, false)
        m.newVca(fromSelection: false)
        #expect(f.mixerRuns.map(\.id) == ["mixer.assign_vca", "mixer.unassign_vca", "mixer.new_vca"])
        #expect(f.mixerRuns.last.map { args($0.args) } == [], "a new VCA without the selection has no strips")
    }

    @Test func optionDragTouchesAlone() throws {
        let f = Self.groupsFake()
        let m = try model(f)
        let t = AutomationTargetRow.stripFader(strip: 2)
        m.touch(t, alone: true)
        m.move(t, to: 0.5)
        m.release(t)
        #expect(f.alones == [true, true, true], "touch, value and release, each alone")
        #expect(f.holders == [.mouse, .mouse, .mouse])
        m.touch(t)
        m.release(t)
        #expect(f.alones.count == 3, "a plain drag is the group's: no `alone` call")
        #expect(f.gestures.count == 5)
    }

    @Test func followersShowTheCoresValues() throws {
        let f = Self.groupsFake()
        let m = try model(f)
        let lead = AutomationTargetRow.stripFader(strip: 2)
        let follower = AutomationTargetRow.stripFader(strip: 3)
        m.touch(lead)
        m.move(lead, to: AutomationLaw.faderNorm(-6))
        f.automationRow = AutomationStateRow(
            modes: [], touched: [lead, follower],
            touchedValues: [AutomationLaw.faderNorm(-1), AutomationLaw.faderNorm(-6)], suspended: false,
            writing: false)
        m.pollAutomation()
        #expect(abs(m.gainDb(try strip(m, 3)) + 6) < 1e-6, "the follower moves with the drag")
        #expect(abs(m.gainDb(try strip(m, 2)) + 6) < 1e-6, "the hand here wins over the core's value")
        #expect(m.gainDb(try strip(m, 4)) == 0)
        f.automationRow = AutomationStateRow(modes: [], touched: [], suspended: false, writing: false)
        m.release(lead)
        m.pollAutomation()
        #expect(m.gainDb(try strip(m, 3)) == 0, "the snapshot's again")
    }

    @Test func vcaOffsetReadoutSumsShownValues() throws {
        let m = try model(
            Self.groupsFake(vcas: [
                Self.vca(1, "A", members: [2, 3], db: -3), Self.vca(2, "B", members: [2], db: 1),
                Self.vca(3, "Off", members: [4], db: MixerMath.floorDb),
            ]))
        #expect(m.vcaOffsetDb(try strip(m, 2)) == -2)
        #expect(MixerModel.vcaOffsetText(m.vcaOffsetDb(try strip(m, 2))) == "VCA \u{2212}2.0")
        #expect(MixerModel.vcaOffsetText(m.vcaOffsetDb(try strip(m, 5))) == nil, "in no VCA: 0")
        #expect(MixerModel.vcaOffsetText(0) == nil)
        #expect(MixerModel.vcaOffsetText(2) == "VCA +2.0")
        #expect(MixerModel.vcaOffsetText(m.vcaOffsetDb(try strip(m, 4))) == "VCA \u{2212}\u{221E}")
        // A drag on VCA A moves the readout before the snapshot does.
        let t = AutomationTargetRow.vcaFader(vca: 1)
        m.touch(t)
        m.move(t, to: AutomationLaw.faderNorm(-10))
        #expect(abs(m.vcaOffsetDb(try strip(m, 3)) + 10) < 1e-6)
    }

    /// Pre-flight F25: at rest, the readout's sum is the core's.
    @Test func theOffsetIsTheCoresAtRest() throws {
        setenv("VIZDAW_AUDIO", "offline", 1)
        let session = try Session.newDemo()
        defer { try? session.close() }
        let m = try MixerModel(document: DocumentModel(session: session))
        let member = try #require(m.strips.first { $0.kind != .bus })
        func run(_ id: String, _ args: [ControlArg]) throws { m.document.apply(try session.runMixer(id: id, args: args)) }
        try run("mixer.new_vca", DocumentModel.stripsArg([member.id]))
        try run("mixer.new_vca", DocumentModel.stripsArg([member.id]))
        let ids = try session.mixer().vcas.map(\.id)
        try #require(ids.count == 2)
        func level(_ vca: UInt64, _ db: String) throws {
            try run(
                "mixer.set_vca",
                [.init(name: "vca", value: .int(value: Int64(vca))), .init(name: "gain_db", value: .text(value: db))])
        }
        try level(ids[0], "-4.5")
        try level(ids[1], "1")
        m.refreshIfNeeded()
        let row = try strip(m, member.id)
        #expect(row.vcaOffsetDb == -3.5)
        #expect(m.vcaOffsetDb(row) == row.vcaOffsetDb)
    }

    /// Pre-flight F10: the VCA modes are the registry's; Write and Trim
    /// are refused.
    @Test func theVcaModesAreTheCoresChoice() throws {
        setenv("VIZDAW_AUDIO", "offline", 1)
        let session = try Session.newDemo()
        defer { try? session.close() }
        _ = try session.runMixer(id: "mixer.new_vca", args: [])
        let vca = try #require(try session.mixer().vcas.first)
        func set(_ m: AutomationModeRow) throws {
            _ = try session.runMixer(
                id: "mixer.set_vca",
                args: [
                    .init(name: "vca", value: .int(value: Int64(vca.id))),
                    .init(name: "mode", value: .text(value: AutomationModes.name(m))),
                ])
        }
        for m in AutomationModes.vca {
            try set(m)
            #expect(try session.mixer().vcas.first?.automation == m)
        }
        for m in AutomationModes.all where !AutomationModes.vca.contains(m) {
            #expect(throws: (any Error).self) { try set(m) }
        }
    }

    @Test func vcaColumnsSitBeforeMaster() throws {
        let m = try model(Self.groupsFake(vcas: [Self.vca(1, "A", members: []), Self.vca(2, "B", members: [])]))
        #expect(m.columns.map(\.id) == ["s2", "s3", "s4", "s5", "v1", "v2"], "the buses, then the VCAs")
        #expect(m.master?.id == 0, "master follows, after the divider")
    }

    @Test func keysFillTheSelection() throws {
        let f = Self.groupsFake()
        let s = DocumentStore(makeEmpty: { FakeSession([]) }, makeDemo: { f }, open: { _ in demoFake() })
        let id = try #require(s.newDemo())
        let m = try #require(s.model(id))
        m.selectedStrips = [4, 2, 0]
        #expect(KeyDispatch.command("mixer.new_vca", doc: (id, m), store: s, host: FakeControlHost()) == .handled)
        #expect(f.mixerRuns.map(\.id) == ["mixer.new_vca"])
        #expect(f.mixerRuns.map { args($0.args) } == [["strips=\(ControlValue.text(value: "2,4"))"]])
        _ = KeyDispatch.command("mixer.new_group", doc: (id, m), store: s, host: FakeControlHost())
        #expect(f.mixerRuns.last.map { args($0.args) } == ["strips=\(ControlValue.text(value: "2,4"))"])
        _ = KeyDispatch.command("mixer.rename_vca", doc: (id, m), store: s, host: FakeControlHost())
        #expect(f.mixerRuns.last?.id == "mixer.rename_vca")
        #expect(f.mixerRuns.last?.args.isEmpty == true, "only new group and new VCA take the selection")
    }

    /// Ruling 11: the channel's call never acts on the selection.
    @Test func theChannelNeverFillsTheSelection() throws {
        let f = Self.groupsFake()
        let s = DocumentStore(makeEmpty: { FakeSession([]) }, makeDemo: { f }, open: { _ in demoFake() })
        let id = try #require(s.newDemo())
        let m = try #require(s.model(id))
        m.selectedStrips = [2, 3]
        for c in ["mixer.new_group", "mixer.new_vca"] {
            _ = ControlCommands.run(c, args: [], doc: (id, m), store: s, host: FakeControlHost())
            #expect(f.mixerRuns.last?.id == c)
            #expect(f.mixerRuns.last?.args.isEmpty == true, "\(c)")
        }
    }

    @Test func vcaStripControlsAreLabelled() {
        #expect(VcaStripLabels.group("Drums") == "Group Drums")
        #expect(VcaStripLabels.offset("VCA \u{2212}3.0") == "VCA offset VCA \u{2212}3.0")
        #expect(VcaStripLabels.fader("Drums") == "Fader of VCA Drums")
        #expect(VcaStripLabels.mute("Drums") == "Mute VCA Drums")
        #expect(VcaStripLabels.solo("Drums") == "Solo VCA Drums")
        #expect(VcaStripLabels.mode == "VCA automation mode")
        #expect(VcaStripLabels.fader("VCA 1") == "Fader of VCA 1")
        #expect(VcaStripLabels.level("VCA 1") == "Level of VCA 1")
        #expect(VcaStripLabels.mute("VCA 1") == "Mute VCA 1")
        #expect(VcaStripLabels.solo("VCA 1") == "Solo VCA 1")
        #expect(VcaStripLabels.fader("Strings") == "Fader of VCA Strings")
        #expect(VcaStripLabels.title("VCA") == "VCA")
        #expect(VcaStripLabels.title("VCAs and more") == "VCA VCAs and more")
    }

    @Test func aVcaStripsEditsAreSetVca() throws {
        let v = Self.vca(3, "Drums", members: [2])
        let f = Self.groupsFake(vcas: [v])
        let m = try model(f)
        m.toggleVcaMute(v)
        m.toggleVcaSolo(v)
        m.setVcaMode(v, .latch)
        m.setVcaMode(v, .read)
        #expect(f.mixerRuns.map(\.id) == ["mixer.set_vca", "mixer.set_vca", "mixer.set_vca"], "Read is no change")
        #expect(f.mixerRuns.map { args($0.args).last } == [
            "mute=\(ControlValue.flag(value: true))", "solo=\(ControlValue.flag(value: true))",
            "mode=\(ControlValue.text(value: "latch"))",
        ])
        m.beginVcaRename(v)
        m.renameDraft = "  Kit "
        m.commitVcaRename()
        #expect(m.renamingVca == nil)
        #expect(f.mixerRuns.last.map { args($0.args) } == [
            "vca=\(ControlValue.int(value: 3))", "name=\(ControlValue.text(value: "Kit"))",
        ])
        m.deleteVca(3)
        #expect(f.mixerRuns.last?.id == "mixer.delete_vca")
    }
}
