import Foundation
import Testing
import VizDAWCore
@testable import VizDAWAppModel

/// Stands in for the core's switch, the install and the bundled-EQ facts.
@MainActor
final class FakePluginSettingsSink: PluginSettingsSink {
    var pushed: [Bool] = []
    var installs: [(dest: String?, replace: Bool)] = []
    var error: VizError?
    var bundledEqName: String? = "Multi-Q"
    var bundledAuPresent = true

    func setNewChannelEq(_ on: Bool) { pushed.append(on) }

    func installMultiq(dest: String?, replace: Bool) throws -> String {
        installs.append((dest, replace))
        if let e = error { throw e }
        return (dest ?? "/nowhere") + "/Multi-Q.component"
    }
}

/// Settings › Plug-ins (plan 6g Task 4, Rulings 8–10): the "Put an EQ on
/// new channels" switch lives in the preferences and reaches the core at
/// launch and on change; Install asks before replacing, the command never
/// asks. Every install goes to a temp destination.
@MainActor
@Suite struct PluginSettingsTests {
    let dest = FileManager.default.temporaryDirectory
        .appendingPathComponent("vizdaw-plugin-settings-\(UUID().uuidString)").path

    @Test func defaultsToOnAndPushesIt() {
        let sink = FakePluginSettingsSink()
        let s = PluginSettings(defaults: MemoryDefaults(), sink: sink, installDestination: dest)
        #expect(s.newChannelEq == true)
        #expect(sink.pushed == [true])
    }

    @Test func persistsTheSwitch() {
        let defaults = MemoryDefaults()
        let sink = FakePluginSettingsSink()
        let s = PluginSettings(defaults: defaults, sink: sink, installDestination: dest)
        s.setNewChannelEq(false)
        #expect(s.newChannelEq == false)
        #expect(sink.pushed == [true, false])
        let sink2 = FakePluginSettingsSink()
        let again = PluginSettings(defaults: defaults, sink: sink2, installDestination: dest)
        #expect(again.newChannelEq == false)
        #expect(sink2.pushed == [false])
    }

    @Test func existsAsksToReplace() {
        let sink = FakePluginSettingsSink()
        let s = PluginSettings(defaults: MemoryDefaults(), sink: sink, installDestination: dest)
        sink.error = .Exists(path: dest + "/Multi-Q.component")
        s.install(replace: false)
        #expect(s.confirmReplace == true)
        #expect(s.status == nil)
        sink.error = nil
        s.install(replace: true)
        #expect(sink.installs.count == 2)
        #expect(sink.installs.last?.dest == dest)
        #expect(sink.installs.last?.replace == true)
        #expect(s.confirmReplace == false)
        #expect(s.status == "Installed in \(dest).")
    }

    @Test func otherFailuresSayWhy() {
        let sink = FakePluginSettingsSink()
        let s = PluginSettings(defaults: MemoryDefaults(), sink: sink, installDestination: dest)
        sink.error = .Io(message: "Multi-Q could not be installed: Permission denied")
        s.install(replace: false)
        #expect(s.confirmReplace == false)
        #expect(s.status == "Multi-Q could not be installed: Permission denied")
    }

    @Test func commandsNeverAsk() {
        let sink = FakePluginSettingsSink()
        let s = PluginSettings(defaults: MemoryDefaults(), sink: sink, installDestination: dest)
        sink.error = .Exists(path: dest + "/Multi-Q.component")
        let o = PluginSettingsCommands.run("plugins.install_multiq", args: [], settings: s)
        guard case .failed(let f) = o else {
            Issue.record("expected a failure, got \(o)")
            return
        }
        #expect(f.code == .dialogRequired)
        #expect(f.dialog == "Replace")
        #expect(s.confirmReplace == false)
        #expect(sink.installs.first?.dest == dest)
        #expect(sink.installs.first?.replace == false)

        sink.error = nil
        let replace = [ControlArg(name: "replace", value: .flag(value: true))]
        guard case .done(.ran(_, _, _, _, let status)) = PluginSettingsCommands.run(
            "plugins.install_multiq", args: replace, settings: s)
        else {
            Issue.record("expected the install to run")
            return
        }
        #expect(status == dest + "/Multi-Q.component")
        #expect(sink.installs.last?.replace == true)

        sink.error = .Model(message: "Multi-Q is not in this build of VizDAW.")
        if case .failed(let f) = PluginSettingsCommands.run("plugins.install_multiq", args: [], settings: s) {
            #expect(f.code == .unavailable)
        } else {
            Issue.record("expected unavailable")
        }
    }

    @Test func theSwitchCommandSetsAndKeeps() {
        let defaults = MemoryDefaults()
        let sink = FakePluginSettingsSink()
        let s = PluginSettings(defaults: defaults, sink: sink, installDestination: dest)
        let off = [ControlArg(name: "on", value: .flag(value: false))]
        guard case .done(.ran(_, _, _, _, let status)) = PluginSettingsCommands.run(
            "plugins.new_channel_eq", args: off, settings: s)
        else {
            Issue.record("expected the switch to run")
            return
        }
        #expect(status == "new-channel EQ off")
        #expect(s.newChannelEq == false)
        #expect(sink.pushed == [true, false])
        #expect(PluginSettings(defaults: defaults, sink: sink, installDestination: dest).newChannelEq == false)
        #expect(PluginSettingsCommands.ids == ["plugins.new_channel_eq", "plugins.install_multiq"])
    }

    @Test func noBundledEqDisables() {
        let sink = FakePluginSettingsSink()
        sink.bundledEqName = nil
        sink.bundledAuPresent = false
        let s = PluginSettings(defaults: MemoryDefaults(), sink: sink, installDestination: dest)
        #expect(s.bundledEqName == nil)
        s.install(replace: false)
        #expect(sink.installs.isEmpty)
        #expect(s.confirmReplace == false)
        #expect(s.status == PluginSettings.noBundledEq)
        #expect(PluginSettings.noBundledEq == "This build of VizDAW has no bundled EQ.")
        if case .failed(let f) = PluginSettingsCommands.run("plugins.install_multiq", args: [], settings: s) {
            #expect(f.code == .unavailable)
        } else {
            Issue.record("expected unavailable")
        }
        #expect(sink.installs.isEmpty)
    }
}
