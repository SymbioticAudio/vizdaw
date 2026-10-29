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
}
