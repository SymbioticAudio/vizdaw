import Foundation
import Testing
import VizDAWCore
@testable import VizDAWAppModel

/// The automation commands over the control channel (plan 5e-1 Task 10):
/// arguments checked before the core is called; the core's refusals are
/// the call's failures; nothing opens a dialog.
@MainActor
@Suite(.serialized) struct AutomationCommandsTests {
    func doc() throws -> (UUID, DocumentModel, FakeSession) {
        let f = mixerFake()
        let s = DocumentStore(makeEmpty: { FakeSession([]) }, makeDemo: { f }, open: { _ in demoFake() })
        let id = try #require(s.newDemo())
        return (id, try #require(s.model(id)), f)
    }

    func arg(_ name: String, _ v: String) -> ControlArg { ControlArg(name: name, value: .text(value: v)) }

    func code(_ o: ControlOutcome) -> ControlErrorCode? {
        if case .failed(let f) = o { return f.code }
        return nil
    }

    @Test func automationCommandsCheckTheirArguments() throws {
        let (id, m, f) = try doc()
        let run = { (cmd: String, args: [ControlArg]) in AutomationCommands.run(cmd, args: args, doc: id, m) }
        // set_mode: the strips and the mode.
        #expect(code(run(AutomationCommands.setMode, [arg("strips", "all"), arg("mode", "touch")])) == nil)
        #expect(f.modeCalls.last?.0 == [1, 2, 0] && f.modeCalls.last?.1 == .touch)
        #expect(code(run(AutomationCommands.setMode, [arg("strips", "2,1"), arg("mode", "off")])) == nil)
        #expect(f.modeCalls.last?.0 == [2, 1])
        #expect(code(run(AutomationCommands.setMode, [arg("strips", "1,1"), arg("mode", "read")])) == nil)
        #expect(f.modeCalls.last?.0 == [1], "a strip listed twice is set once")
        for bad in ["", "1,,2", "1,x", "-1", "1.5", " 1", "99999999999999999999999"] {
            #expect(
                code(run(AutomationCommands.setMode, [arg("strips", bad), arg("mode", "read")])) == .invalidParams,
                "\(bad)")
        }
        // gesture: the target, the phase, the value.
        #expect(code(run(AutomationCommands.gesture, [arg("target", "fader:1"), arg("phase", "touch")])) == nil)
        #expect(f.gestures.last?.1 == .touch(value: nil))
        #expect(
            code(run(AutomationCommands.gesture, [arg("target", "fader:1"), arg("phase", "value"), arg("value", "0.25")]))
                == nil)
        #expect(f.gestures.last?.1 == .value(value: 0.25))
        for (target, phase, value) in [
            ("fader:x", "touch", nil), ("velocity:1", "touch", nil), ("fader:1", "value", nil),
            ("fader:1", "value", "nan"), ("fader:1", "value", "1e400"), ("fader:1", "value", "-0.5"),
            ("fader:1", "value", "1.5"), ("fader:1", "value", "0,5"), ("fader:1", "touch", "inf"),
            ("fader:1", "value", "1e-1"), ("fader:1", "value", "0x1p-2"), ("fader:1", "value", " 0.5"),
        ] as [(String, String, String?)] {
            var args = [arg("target", target), arg("phase", phase)]
            if let value { args.append(arg("value", value)) }
            #expect(code(run(AutomationCommands.gesture, args)) == .invalidParams, "\(target) \(phase) \(value ?? "-")")
        }
        // The core's refusal is the call's failure.
        f.gestureError = .Model(message: "unknown id strip9")
        #expect(code(run(AutomationCommands.gesture, [arg("target", "fader:9"), arg("phase", "touch")])) == .failed)
        f.gestureError = .Busy(message: "Finish the open edit first.")
        #expect(code(run(AutomationCommands.gesture, [arg("target", "fader:1"), arg("phase", "touch")])) == .busy)
    }

    /// 5e-1 Task 10 review M3: a mode change while an editor drag is open
    /// is unavailable, and the core is not called.
    @Test func setModeIsUnavailableDuringAnEditorDrag() throws {
        let (id, m, f) = try doc()
        #expect(m.editor.beginGesture("Move", lanes: [], origin: .other))
        let calls = f.modeCalls.count
        let o = AutomationCommands.run(
            AutomationCommands.setMode, args: [arg("strips", "all"), arg("mode", "touch")], doc: id, m)
        #expect(code(o) == .unavailable)
        #expect(f.modeCalls.count == calls)
    }

    @Test func suspendTogglesTheGlobalSwitch() throws {
        setAutomationSuspended(on: false)
        defer { setAutomationSuspended(on: false) }
        _ = AutomationCommands.runSuspend([])
        #expect(automationSuspended())
        _ = AutomationCommands.runSuspend([ControlArg(name: "on", value: .flag(value: true))])
        #expect(automationSuspended())
        _ = AutomationCommands.runSuspend([])
        #expect(!automationSuspended())
        #expect(ControlCommands.handled.isSuperset(of: AutomationCommands.ids))
    }

    /// A gesture begun over the channel always ends over it (task 10
    /// dispatch): the release takes the touch's own spelling, needs no
    /// value, is never refused by an app-side check that the touch passed,
    /// and is passed on for a target that went meanwhile (the core does
    /// nothing then, Ruling 20).
    @Test func aChannelGestureAlwaysEndsOverTheChannel() throws {
        let (id, m, f) = try doc()
        let run = { (args: [ControlArg]) in AutomationCommands.run(AutomationCommands.gesture, args: args, doc: id, m) }
        for target in ["fader:1", "pan:2", "mute:0", "solo:1", "send:1:1", "param:10:4294967295", "fader:99"] {
            #expect(code(run([arg("target", target), arg("phase", "touch")])) == nil, "\(target)")
            #expect(code(run([arg("target", target), arg("phase", "release")])) == nil, "\(target)")
            #expect(f.gestures.last?.0 == parseAutomationTarget(text: target) && f.gestures.last?.1 == .release)
        }
        // A release with a value is a script's slip: named, nothing sent.
        let before = f.gestures.count
        #expect(
            code(run([arg("target", "fader:1"), arg("phase", "release"), arg("value", "0.5")])) == .invalidParams)
        #expect(f.gestures.count == before)
    }

    /// Missing, wrongly typed and out-of-enum arguments are named before
    /// the core is called (the registry checks them first; the app does
    /// not rely on it).
    @Test func hostileArgumentsNeverReachTheCore() throws {
        let (id, m, f) = try doc()
        let run = { (cmd: String, args: [ControlArg]) in AutomationCommands.run(cmd, args: args, doc: id, m) }
        let flag = ControlArg(name: "mode", value: .flag(value: true))
        for args in [
            [arg("strips", "all")], [arg("mode", "read")], [arg("strips", "all"), arg("mode", "loud")],
            [arg("strips", "all"), arg("mode", "READ")], [arg("strips", "all"), flag],
            [ControlArg(name: "strips", value: .int(value: 1)), arg("mode", "read")],
            [arg("strips", String(repeating: "1,", count: 50_000) + "1"), arg("mode", "read")],
            [arg("strips", "all,1"), arg("mode", "read")], [arg("strips", "1,"), arg("mode", "read")],
        ] {
            #expect(code(run(AutomationCommands.setMode, args)) == .invalidParams, "\(args)")
        }
        #expect(f.modeCalls.isEmpty)
        for args in [
            [arg("phase", "touch")], [arg("target", "fader:1")], [arg("target", "fader:1"), arg("phase", "wiggle")],
            [ControlArg(name: "target", value: .int(value: 1)), arg("phase", "touch")],
            [arg("target", "fader:1"), arg("phase", "value"), ControlArg(name: "value", value: .int(value: 0))],
            [arg("target", String(repeating: "9", count: 100_000)), arg("phase", "touch")],
            [arg("target", "fader:1"), arg("phase", "value"), arg("value", String(repeating: "0", count: 100_000))],
            [arg("target", "fader:1"), arg("phase", "value"), arg("value", "inf")],
            [arg("target", "fader:1"), arg("phase", "value"), arg("value", ".")],
            [arg("target", "fader:1"), arg("phase", "value"), arg("value", "0.5.1")],
            [arg("target", "fader:1"), arg("phase", "value"), arg("value", "")],
            [arg("target", "fader:1"), arg("phase", "value"), arg("value", "٠.٥")],
        ] {
            #expect(code(run(AutomationCommands.gesture, args)) == .invalidParams, "\(args)")
        }
        #expect(f.gestures.isEmpty)
        // Edges of the range are values; a strip the mixer does not have is
        // the core's refusal (unknown ids are `failed`, not checked here).
        #expect(code(run(AutomationCommands.gesture, [arg("target", "fader:1"), arg("phase", "value"), arg("value", "0")])) == nil)
        #expect(code(run(AutomationCommands.gesture, [arg("target", "fader:1"), arg("phase", "value"), arg("value", "1.0")])) == nil)
        #expect(code(run(AutomationCommands.gesture, [arg("target", "fader:1"), arg("phase", "value"), arg("value", ".5")])) == nil)
        #expect(f.gestures.map(\.1) == [.value(value: 0), .value(value: 1), .value(value: 0.5)])
        // The suspend flag of the wrong type is named too.
        let was = automationSuspended()
        defer { setAutomationSuspended(on: was) }
        #expect(code(AutomationCommands.runSuspend([arg("on", "yes")])) == .invalidParams)
        #expect(automationSuspended() == was, "a refused call does not toggle")
    }

    @Test func modeCommandsSetTheSelectedStrips() throws {
        let (_, m, f) = try doc()
        #expect(!AutomationCommands.enabled("automation.mode.write", m))
        #expect(AutomationCommands.run("automation.mode.write", on: nil, m) == .unavailable("Select strips in the mixer first."))
        m.selectedStrips = [2, 0]
        #expect(AutomationCommands.enabled("automation.mode.write", m))
        #expect(AutomationCommands.run("automation.mode.write", on: nil, m) == .done)
        #expect(f.modeCalls.last?.0 == [0, 2] && f.modeCalls.last?.1 == .write)
        #expect(AutomationCommands.enabled(AutomationCommands.suspend, m))
        #expect(Set(AutomationCommands.modeIDs.keys).isSubset(of: ControlCommands.handled))
    }

    /// Fix round 1 (M1, M3): a drag open answers like `automation.set_mode`.
    @Test func aModeCommandWithADragOpenIsUnavailableLikeSetMode() throws {
        let (id, m, f) = try doc()
        m.selectedStrips = [1]
        #expect(m.editor.beginGesture("drag", lanes: [], origin: .other))
        let before = f.modeCalls.count
        #expect(AutomationCommands.run("automation.mode.read", on: nil, m) == .unavailable(EditorModel.gestureOpenMessage))
        let viaChannel = AutomationCommands.run("automation.mode.read", args: [], doc: id, m)
        let viaSetMode = AutomationCommands.run(
            AutomationCommands.setMode, args: [arg("strips", "1"), arg("mode", "read")], doc: id, m)
        #expect(code(viaChannel) == .unavailable && code(viaChannel) == code(viaSetMode))
        #expect(f.modeCalls.count == before)
    }

    /// Fix round 1 (M2): the channel branch.
    @Test func theChannelRunsModeCommandsAndIgnoresStrayArguments() throws {
        let (id, m, f) = try doc()
        let none = AutomationCommands.run("automation.mode.latch", args: [], doc: id, m)
        #expect(code(none) == .unavailable && f.modeCalls.isEmpty)
        m.selectedStrips = [2, 1]
        #expect(code(AutomationCommands.run("automation.mode.latch", args: [], doc: id, m)) == nil)
        #expect(f.modeCalls.last?.0 == [1, 2] && f.modeCalls.last?.1 == .latch)
        let n = f.modeCalls.count
        let hostile = [
            arg("strips", "all"), arg("mode", "off"), ControlArg(name: "strips", value: .flag(value: true)),
            ControlArg(name: "mode", value: .flag(value: false)),
        ]
        #expect(code(AutomationCommands.run("automation.mode.trim", args: hostile, doc: id, m)) == nil)
        #expect(f.modeCalls.count == n + 1)
        #expect(f.modeCalls.last?.0 == [1, 2] && f.modeCalls.last?.1 == .trim, "the selection and the ID decide")
    }

    /// Fix round 1 (M3): Suspend Automation from the menu run, and `isOn`.
    @Test func theMenuTogglesSuspendAndIsOnReadsIt() throws {
        let (_, m, _) = try doc()
        defer { setAutomationSuspended(on: false) }
        setAutomationSuspended(on: false)
        #expect(!AutomationCommands.isOn(AutomationCommands.suspend, m))
        #expect(AutomationCommands.run(AutomationCommands.suspend, on: true, m) == .done)
        #expect(AutomationCommands.isOn(AutomationCommands.suspend, m) && automationSuspended())
        #expect(AutomationCommands.run(AutomationCommands.suspend, on: false, m) == .done)
        #expect(!AutomationCommands.isOn(AutomationCommands.suspend, m))
        #expect(AutomationCommands.run(AutomationCommands.suspend, on: nil, m) == .done && automationSuspended())
        #expect(!AutomationCommands.isOn("automation.mode.read", m))
    }

    /// Final review I1: a selected strip that went while the mixer was
    /// closed (no refetch prunes it) is left out; the strips that remain
    /// get the mode; with none left the command is unavailable. Menu and
    /// channel alike.
    @Test func modeCommandsSkipStripsThatWentWithTheMixerClosed() throws {
        let (id, m, f) = try doc()
        m.selectedStrips = [1, 2]
        f.mixerSnap.strips.removeAll { $0.id == 2 }  // Undo "Add Bus", mixer closed
        #expect(AutomationCommands.run("automation.mode.write", on: nil, m) == .done)
        #expect(f.modeCalls.last?.0 == [1] && f.modeCalls.last?.1 == .write)
        #expect(m.selectedStrips == [1], "the selection is pruned")
        // The channel path.
        m.selectedStrips = [1, 2]
        #expect(code(AutomationCommands.run("automation.mode.latch", args: [], doc: id, m)) == nil)
        #expect(f.modeCalls.last?.0 == [1] && f.modeCalls.last?.1 == .latch)
        // None left: unavailable, the core not called, the menu dimmed.
        f.mixerSnap.strips.removeAll { $0.id == 1 }
        let n = f.modeCalls.count
        m.selectedStrips = [1, 2]
        #expect(AutomationCommands.run("automation.mode.read", on: nil, m) == .unavailable("Select strips in the mixer first."))
        #expect(m.selectedStrips.isEmpty)
        m.selectedStrips = [1, 2]
        #expect(code(AutomationCommands.run("automation.mode.read", args: [], doc: id, m)) == .unavailable)
        #expect(f.modeCalls.count == n)
        // `enabled` dims once the document moves on (the Undo's snapshot).
        m.selectedStrips = [1, 2]
        f.snap.version += 1
        m.apply(f.snap)
        #expect(m.selectedStrips.isEmpty)
        #expect(!AutomationCommands.enabled("automation.mode.read", m))
    }

    /// Final review m1: the core's Busy answers `busy`, as set_mode does.
    @Test func aBusyModeCommandAnswersBusyLikeSetMode() throws {
        let (id, m, f) = try doc()
        m.selectedStrips = [1]
        f.modeError = .Busy(message: "Release the touched control first.")
        #expect(AutomationCommands.run("automation.mode.read", on: nil, m) == .busy("Release the touched control first."))
        let viaChannel = AutomationCommands.run("automation.mode.read", args: [], doc: id, m)
        let viaSetMode = AutomationCommands.run(
            AutomationCommands.setMode, args: [arg("strips", "1"), arg("mode", "read")], doc: id, m)
        #expect(code(viaChannel) == .busy && code(viaSetMode) == .busy)
    }

    /// Final review m2: one failure title for a mode change, wherever it
    /// comes from (the mixer's popup and Set All use it too).
    @Test func aModeChangeFailsUnderOneTitle() throws {
        let (id, m, f) = try doc()
        m.selectedStrips = [1]
        f.modeError = .Model(message: "boom")
        func message(_ o: ControlOutcome) -> String? {
            if case .failed(let x) = o { return x.message }
            return nil
        }
        let viaChannel = message(AutomationCommands.run("automation.mode.read", args: [], doc: id, m))
        let viaSetMode = message(
            AutomationCommands.run(AutomationCommands.setMode, args: [arg("strips", "1"), arg("mode", "read")], doc: id, m))
        #expect(viaChannel == "Automation Mode Failed: boom")
        #expect(viaSetMode == "Automation Mode Failed: boom")
        let mixer = try MixerModel(document: m)
        mixer.setMode(1, .read)
        #expect(m.alert?.title == "Automation Mode Failed")
        m.alert = nil
        mixer.setAllModes(.read)
        #expect(m.alert?.title == "Automation Mode Failed")
    }
}
