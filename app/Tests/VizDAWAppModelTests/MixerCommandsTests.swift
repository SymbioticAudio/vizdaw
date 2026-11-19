import Foundation
import Testing
import VizDAWCore
@testable import VizDAWAppModel

/// `mixer.new_bus` (plan 6g-2 Task 3, Ruling 7): with `strip`, a bus and a
/// send to it from the strip in one step; without, a bus alone. The name
/// defaults to the next free "Bus N". Nothing here opens a dialog.
@MainActor
@Suite struct MixerCommandsTests {
    func arg(_ name: String, _ v: String) -> ControlArg { ControlArg(name: name, value: .text(value: v)) }
    func arg(_ name: String, _ v: Int64) -> ControlArg { ControlArg(name: name, value: .int(value: v)) }

    /// Synth (1), Bus 1 (2) and Master (0).
    func demo() throws -> (DocumentModel, FakeSession) {
        let f = mixerFake()
        return (try DocumentModel(session: f), f)
    }

    func status(_ o: ControlOutcome) -> String? {
        if case .done(.ran(_, _, _, _, let status)) = o { return status }
        return nil
    }

    func failure(_ o: ControlOutcome) -> ControlFailure? {
        if case .failed(let f) = o { return f }
        return nil
    }

    @Test func newBusCommandRuns() throws {
        #expect(MixerCommands.ids == Set([MixerCommands.newBus] + MixerCommands.coProducerEdits))
        #expect(ControlCommands.handled.contains(MixerCommands.newBus))
        let (m, f) = try demo()
        let doc = UUID()
        let r = MixerCommands.run(MixerCommands.newBus, args: [arg("strip", 1)], docID: doc, m)
        #expect(status(r) == "Bus 2", "\(r)")
        #expect(f.calls == ["newBusFrom 1 Bus 2"])

        f.calls = []
        let alone = MixerCommands.run(MixerCommands.newBus, args: [], docID: doc, m)
        #expect(status(alone) == "Bus 2", "\(alone)")
        #expect(f.calls == ["addBus Bus 2"])

        f.calls = []
        let named = MixerCommands.run(
            MixerCommands.newBus, args: [arg("strip", 1), arg("name", "Drums")], docID: doc, m)
        #expect(status(named) == "Drums", "\(named)")
        #expect(f.calls == ["newBusFrom 1 Drums"])
        #expect(m.alert == nil)
    }

    @Test func newBusCommandRefusesBadInput() throws {
        let (m, f) = try demo()
        let doc = UUID()
        let cases: [([ControlArg], ControlErrorCode, String?)] = [
            ([arg("strip", 1), arg("name", "a\u{0}b")], .invalidParams, nil),
            ([arg("name", "  ")], .invalidParams, nil),
            ([arg("strip", 0)], .invalidParams, "Master has no sends."),
            ([arg("strip", 999)], .unknownId, nil),
            ([arg("strip", -1)], .invalidParams, nil),
        ]
        for (args, code, message) in cases {
            let r = MixerCommands.run(MixerCommands.newBus, args: args, docID: doc, m)
            let fail = try #require(failure(r), "\(args)")
            #expect(fail.code == code, "\(args): \(fail.message)")
            if let message { #expect(fail.message == message, "\(args)") }
        }
        #expect(f.calls.isEmpty)
        #expect(m.alert == nil)
    }

    /// Plan 8d Task 5: the thirteen ids, the seven group ids and the seven
    /// VCA ids, each with arguments the registry takes, and the status each
    /// answers (its title; a new track's name).
    static let edits: [(String, [ControlArg], String)] = {
        func t(_ n: String, _ v: String) -> ControlArg { ControlArg(name: n, value: .text(value: v)) }
        func i(_ n: String, _ v: Int64) -> ControlArg { ControlArg(name: n, value: .int(value: v)) }
        func b(_ n: String, _ v: Bool) -> ControlArg { ControlArg(name: n, value: .flag(value: v)) }
        return [
            ("mixer.set_strip", [i("strip", 1), t("gain_db", "-3"), b("mute", true)], "Set Strip"),
            ("mixer.set_output", [i("strip", 1), i("bus", 2)], "Set Strip Output"),
            ("mixer.add_send", [i("strip", 1), i("target", 2)], "Add Send"),
            ("mixer.set_send", [i("strip", 1), i("index", 1), t("level_db", "-6")], "Set Send"),
            ("mixer.remove_send", [i("strip", 1), i("index", 1)], "Remove Send"),
            ("mixer.add_insert", [i("strip", 1), t("plugin", "multi-q")], "Add Insert"),
            ("mixer.remove_insert", [i("slot", 10)], "Remove Insert"),
            ("mixer.move_insert", [i("slot", 10), i("strip", 2), i("index", 1)], "Move Insert"),
            ("mixer.set_bypass", [i("slot", 10), b("bypass", true)], "Bypass Insert"),
            ("mixer.set_eq", [i("strip", 1), t("plugin", "none")], "Set EQ"),
            ("mixer.set_param", [i("slot", 10), i("param", 3), t("value", "0.5")], "Set Plug-in Parameter"),
            ("mixer.remove_bus", [i("strip", 2)], "Remove Bus"),
            ("mixer.new_group", [t("strips", "1,2"), t("name", "Drums")], "Group"),
            ("mixer.rename_group", [i("group", 1), t("name", "Kit")], "Rename Group"),
            ("mixer.set_group_colour", [i("group", 1), t("colour", "teal")], "Group Colour"),
            ("mixer.add_to_group", [i("group", 1), t("strips", "2")], "Add to Group"),
            ("mixer.remove_from_group", [i("group", 1), t("strips", "2")], "Remove from Group"),
            ("mixer.set_group_links", [i("group", 1), b("mute", false)], "Group Links"),
            ("mixer.delete_group", [i("group", 1)], "Ungroup"),
            ("mixer.new_vca", [t("strips", "1,2"), t("name", "Kit")], "New VCA"),
            ("mixer.rename_vca", [i("vca", 1), t("name", "Drums")], "Rename VCA"),
            ("mixer.set_vca_colour", [i("vca", 1), t("colour", "blue")], "VCA Colour"),
            ("mixer.assign_vca", [i("vca", 1), t("strips", "2")], "Assign to VCA"),
            ("mixer.unassign_vca", [i("vca", 1), t("strips", "2")], "Remove from VCA"),
            ("mixer.set_vca", [i("vca", 1), t("gain_db", "-3"), b("mute", true)], "Set VCA"),
            ("mixer.delete_vca", [i("vca", 1)], "Delete VCA"),
            ("track.new_plugin_instrument", [i("seq", 3), i("after", 30), t("plugin", "built-in-synth")], "Inst 3"),
        ]
    }()

    func args(_ a: [ControlArg]) -> [String] { a.map { "\($0.name)=\($0.value)" } }

    /// Each mixer edit is one `runMixer` call with its id and arguments
    /// unchanged; its snapshot is applied, and it answers its title (a new
    /// track: the track's name) with no sheet.
    @Test func mixerEditsRunThroughOneCall() throws {
        #expect(Self.edits.map(\.0) == MixerCommands.coProducerEdits)
        for (id, given, title) in Self.edits {
            let (m, f) = try demo()
            let doc = UUID()
            let o = MixerCommands.run(id, args: given, docID: doc, m)
            #expect(status(o) == title, "\(id): \(o)")
            guard case .done(.ran(let command, let d, _, _, _)) = o else { continue }
            #expect(command == id)
            #expect(d == doc.uuidString)
            #expect(f.mixerRuns.map(\.id) == [id])
            #expect(f.mixerRuns.map { args($0.args) } == [args(given)], "\(id)")
            #expect(m.snapshot.version == f.snap.version, "\(id): the snapshot is applied")
            #expect(m.alert == nil)
        }
    }

    /// The session's refusal is `failed` in its words; its Busy is `busy`.
    @Test func aRefusedMixerEditIsFailedOrBusy() throws {
        for (id, given, _) in Self.edits {
            let (m, f) = try demo()
            f.mixerError = .Model(message: "Strip 9 is not in this song.")
            let refused = try #require(failure(MixerCommands.run(id, args: given, docID: UUID(), m)), "\(id)")
            #expect(refused.code == .failed, "\(id)")
            #expect(refused.message == "Strip 9 is not in this song.", "\(id)")
            f.mixerError = .Busy(message: "Finish the edit first.")
            let busy = try #require(failure(MixerCommands.run(id, args: given, docID: UUID(), m)), "\(id)")
            #expect(busy.code == .busy, "\(id)")
            #expect(busy.message == "Finish the edit first.", "\(id)")
            #expect(m.alert == nil)
        }
    }

    /// With an editor gesture open, each is unavailable and the session is
    /// not called (`mixer.new_bus`'s guard order).
    @Test func aGestureStopsEveryMixerEdit() throws {
        for (id, given, _) in Self.edits {
            let (m, f) = try demo()
            #expect(m.editor.beginGesture("drag", lanes: [], origin: .other))
            let o = try #require(failure(MixerCommands.run(id, args: given, docID: UUID(), m)), "\(id)")
            #expect(o.code == .unavailable, "\(id)")
            #expect(o.message == EditorModel.gestureOpenMessage, "\(id)")
            #expect(f.mixerRuns.isEmpty, "\(id)")
        }
    }

    /// `track.new_plugin_instrument` without `seq` and `after` gets the
    /// selected sequence and track, as New Track does; the new track is
    /// selected and named in the status.
    @Test func aPluginInstrumentTrackGoesBelowTheSelection() throws {
        let (m, f) = try demo()
        let seq = try #require(m.selectedSeq)
        let lane = try #require(m.lanes.first)
        m.editor.setSelection(SelectionRow(timeRange: nil, lanes: [LaneKey(seq, lane.id).ref], events: []))
        let plugin = arg("plugin", "built-in-synth")
        let o = MixerCommands.run("track.new_plugin_instrument", args: [plugin], docID: UUID(), m)
        let count = f.laneTable[seq]?.count ?? 0
        #expect(status(o) == "Inst \(count)", "\(o)")
        #expect(
            f.mixerRuns.map { args($0.args) }
                == [args([plugin, arg("seq", Int64(seq)), arg("after", Int64(lane.id))])])
        let made = try #require(f.laneTable[seq]?.first { $0.name == "Inst \(count)" })
        #expect(m.selectedLane?.id == made.id)

        // A `seq` given is kept, and no track of another sequence is `after`.
        f.mixerRuns = []
        let other = MixerCommands.run(
            "track.new_plugin_instrument", args: [plugin, arg("seq", 2)], docID: UUID(), m)
        #expect(status(other) == "Inst 3", "\(other)")
        #expect(f.mixerRuns.map { args($0.args) } == [args([plugin, arg("seq", 2)])])
    }
}

/// Plan 8d Task 5 through the real library: `mixer.new_bus {strip}` with
/// no name, applied through Co-Producer, makes the bus its preview named
/// (Rust's `next_bus_name` and the app's `MixerModel.nextBusName`).
@MainActor
@Suite struct MixerCommandsIntegrationTests {
    init() { _ = TestSupportDir.shared }

    @Test func theNewBusIsTheOneThePreviewNamed() throws {
        setenv("VIZDAW_AUDIO", "offline", 1)
        let session = try Session.newDemo()
        _ = try session.addBus(name: "Bus 1")
        _ = try session.addBus(name: "Bus 3")
        let store = DocumentStore(makeEmpty: { session }, makeDemo: { session }, open: { _ in session })
        let id = try #require(store.newDemo())
        let m = try #require(store.model(id))
        defer { try? m.session.close() }
        let host = FakeControlHost()
        let stamp = "2026-11-17 10-00"
        func run(_ r: ControlRequest) -> ControlOutcome {
            CoProducerCommands.run(
                r, doc: (id, m), store: store, host: host, stamp: stamp, peer: nil, approver: FakeApprover(.allow))
        }
        let synth = try #require(try session.mixer().strips.first { $0.kind != .master && $0.kind != .bus })
        let plan = [PlannedRow(id: "mixer.new_bus", args: [ControlArg(name: "strip", value: .int(value: Int64(synth.id)))])]
        _ = run(.coProducerBegin(name: "a bus", asked: nil, window: nil))
        guard case .done(.coProducerPreview(_, let preview)) = run(.coProducerPreview(plan: plan, window: nil)) else {
            Issue.record("no preview")
            return
        }
        let named = preview.items.map { "\($0.thing) \($0.change)" }
        let before = Set(try session.mixer().buses.map(\.name))
        #expect(MixerModel.nextBusName(try session.mixer().buses) == "Bus 2")
        let o = run(.coProducerApply(token: preview.token, plan: plan, window: nil, client: nil))
        guard case .done(.coProducerApplied(_, let results, _)) = o else {
            Issue.record("not applied: \(o)")
            return
        }
        #expect(results == ["mixer.new_bus: done"])
        // The bus made is the one the preview listed, by name.
        let made = try session.mixer().buses.map(\.name).filter { !before.contains($0) }
        #expect(made == ["Bus 2 (Co-Producer)"])
        #expect(named.contains("Strip \(made.first ?? "-") added"), "\(named)")
        #expect(m.alert == nil)
    }
}
