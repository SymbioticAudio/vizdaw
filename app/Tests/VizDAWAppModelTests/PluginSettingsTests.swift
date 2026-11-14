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
    var pushedEq: [DefaultEqRow?] = []
    var choices: [DefaultEqRow] = []
    var note: String?

    func setNewChannelEq(_ on: Bool) { pushed.append(on) }
    func setDefaultEq(_ choice: DefaultEqRow?) { pushedEq.append(choice) }
    func eqChoices() -> [DefaultEqRow] { choices }
    func defaultEqNote() -> String? { note }

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
        #expect(PluginSettingsCommands.ids == ["plugins.new_channel_eq", "plugins.install_multiq", "plugins.default_eq"])
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

    // MARK: The EQ for new channels (plan 6g-2 Task 2, Rulings 5–7)

    let gain = DefaultEqRow(format: .clap, id: "vizdaw.test.gain", name: "Test Gain")

    private func status(_ o: ControlOutcome) -> String? {
        if case .done(.ran(_, _, _, _, let status)) = o { return status }
        return nil
    }

    private func failure(_ o: ControlOutcome) -> ControlFailure? {
        if case .failed(let f) = o { return f }
        return nil
    }

    @Test func theChoiceDefaultsToMultiQAndPersists() {
        let defaults = MemoryDefaults()
        let sink = FakePluginSettingsSink()
        let s = PluginSettings(defaults: defaults, sink: sink, installDestination: dest)
        #expect(s.defaultEq == nil)
        #expect(sink.pushedEq == [nil])
        #expect(s.defaultEqName == "Multi-Q")
        s.setDefaultEq(gain)
        #expect(s.defaultEq == gain)
        #expect(sink.pushedEq == [nil, gain])
        let stored = defaults.dictionary(forKey: PluginSettings.eqKey) as? [String: String]
        #expect(stored == ["format": "clap", "id": "vizdaw.test.gain", "name": "Test Gain"])
        let sink2 = FakePluginSettingsSink()
        let again = PluginSettings(defaults: defaults, sink: sink2, installDestination: dest)
        #expect(again.defaultEq == gain)
        #expect(again.defaultEqName == "Test Gain")
        #expect(sink2.pushedEq == [gain])
        again.setDefaultEq(nil)
        #expect(defaults.object(forKey: PluginSettings.eqKey) == nil)
        #expect(sink2.pushedEq == [gain, nil])
    }

    @Test func aDamagedChoiceReadsAsMultiQ() {
        let damaged: [Any] = [
            "clap:vizdaw.test.gain",
            ["format": "vst", "id": "vizdaw.test.gain", "name": "Test Gain"],
            ["format": "clap", "name": "Test Gain"],
            ["format": "clap", "id": "vizdaw.test\u{0}gain", "name": "Test Gain"],
        ]
        for value in damaged {
            let defaults = MemoryDefaults()
            defaults.set(value, forKey: PluginSettings.eqKey)
            let sink = FakePluginSettingsSink()
            let s = PluginSettings(defaults: defaults, sink: sink, installDestination: dest)
            #expect(s.defaultEq == nil, "\(value)")
            #expect(sink.pushedEq == [nil])
            #expect(defaults.object(forKey: PluginSettings.eqKey) == nil, "\(value)")
        }
    }

    @Test func aMissingChoiceShowsAndSaysSo() {
        let defaults = MemoryDefaults()
        let sink = FakePluginSettingsSink()
        PluginSettings(defaults: defaults, sink: sink, installDestination: dest).setDefaultEq(gain)
        sink.note = "Test Gain is not available, so new channels get Multi-Q."
        let s = PluginSettings(defaults: defaults, sink: sink, installDestination: dest)
        s.refreshEqChoices()
        #expect(s.defaultEq == gain)
        #expect(s.defaultEqAvailable == false)
        #expect(s.eqNote == "Test Gain is not available, so new channels get Multi-Q.")
        sink.choices = [gain]
        sink.note = nil
        s.refreshEqChoices()
        #expect(s.defaultEqAvailable == true)
        #expect(s.eqNote == nil)
    }

    @Test func defaultEqCommandSetsAndAnswers() {
        let sink = FakePluginSettingsSink()
        sink.choices = [gain]
        let s = PluginSettings(defaults: MemoryDefaults(), sink: sink, installDestination: dest)
        let set = { (plugin: String) in
            PluginSettingsCommands.run(
                "plugins.default_eq", args: [ControlArg(name: "plugin", value: .text(value: plugin))], settings: s)
        }
        #expect(status(set("clap:vizdaw.test.gain")) == "default EQ Test Gain")
        #expect(s.defaultEq == gain)
        #expect(status(set("multi-q")) == "default EQ Multi-Q")
        #expect(s.defaultEq == nil)
        #expect(sink.pushedEq == [nil, gain, nil])
        #expect(PluginSettingsCommands.ids.contains("plugins.default_eq"))
    }

    @Test func defaultEqCommandRefusesBadInput() {
        let sink = FakePluginSettingsSink()
        sink.choices = [gain]
        let s = PluginSettings(defaults: MemoryDefaults(), sink: sink, installDestination: dest)
        s.setDefaultEq(gain)
        let set = { (plugin: String) in
            PluginSettingsCommands.run(
                "plugins.default_eq", args: [ControlArg(name: "plugin", value: .text(value: plugin))], settings: s)
        }
        for bad in ["clap:", "vst:x", "gain", "clap:a\u{0}b", "au:", ""] {
            #expect(failure(set(bad))?.code == .invalidParams, "\(bad)")
            #expect(s.defaultEq == gain)
        }
        let unknown = failure(set("clap:vizdaw.test.synth4"))
        #expect(unknown?.code == .unknownId)
        #expect(unknown?.message == "No installed effect is clap:vizdaw.test.synth4.")
        #expect(failure(PluginSettingsCommands.run("plugins.default_eq", args: [], settings: s))?.code == .invalidParams)
        #expect(s.defaultEq == gain)
        #expect(sink.pushedEq == [nil, gain])
    }

    @Test func eqChoicesAreUsableEffectsButMultiQsClap() {
        let rows = [
            catalogRow("vizdaw.test.gain", "Test Gain", vendor: "VizDAW"),
            catalogRow(PluginSettings.multiqClapId, "Multi-Q", vendor: "VizDAW"),
            catalogRow("aufx/Gain/Test", "AU Gain", vendor: "VizDAW", format: .au, status: .notValidated),
            catalogRow("vizdaw.test.off", "Off", vendor: "VizDAW", status: .disabled),
            catalogRow("vizdaw.test.gone", "Gone", vendor: "VizDAW", status: .missing),
            catalogRow("vizdaw.test.synth4", "Synth", vendor: "VizDAW", kind: .instrument),
        ]
        #expect(
            PluginSettings.eqChoices(from: rows) == [
                gain, DefaultEqRow(format: .au, id: "aufx/Gain/Test", name: "AU Gain"),
            ])
    }
}
