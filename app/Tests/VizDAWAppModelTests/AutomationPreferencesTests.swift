import Foundation
import Testing
import VizDAWCore
@testable import VizDAWAppModel

/// Stands in for the core's automation setter.
@MainActor
final class FakeAutomationSink: AutomationSink {
    var applied: [AutomationSettingsRow] = []
    var error: VizError?
    func apply(_ row: AutomationSettingsRow) throws {
        if let e = error { throw e }
        applied.append(row)
    }
}

/// Settings › Automation (plan 5e-2 Task 1, Ruling 7; spec §7.5): kept in
/// the preferences, handed to the core at launch and on change, kept only
/// once the core took them; damaged values keep their defaults.
@MainActor
@Suite struct AutomationPreferencesTests {
    @Test func launchHandsTheSettingsToTheCore() {
        let sink = FakeAutomationSink()
        let p = AutomationPreferences(defaults: MemoryDefaults(), sink: sink)
        #expect(p.settings == AutomationSettings())
        p.applyAll()
        #expect(sink.applied == [AutomationSettingsRow(afterWrite: .latch, returnMs: 250, thinning: .medium)])
    }

    @Test func settingsAreKeptOnlyOnceTheCoreTookThem() {
        let defaults = MemoryDefaults()
        let sink = FakeAutomationSink()
        let p = AutomationPreferences(defaults: defaults, sink: sink)
        p.setAfterWrite("touch")
        p.setReturnMs(3000)
        p.setThinning("high")
        #expect(p.settings.afterWrite == "touch" && p.settings.returnMs == 2000 && p.settings.thinning == "high")
        #expect(sink.applied.last == AutomationSettingsRow(afterWrite: .touch, returnMs: 2000, thinning: .high))
        let again = AutomationPreferences(defaults: defaults, sink: FakeAutomationSink())
        #expect(again.settings == p.settings, "read back from the preferences")
        p.setAfterWrite("write")
        #expect(p.settings.afterWrite == "touch", "Write is no After Write mode")
        sink.error = .Model(message: "the return time is 0 to 2000 ms")
        p.setReturnMs(100)
        #expect(p.settings.returnMs == 2000)
        #expect(p.problem == "the return time is 0 to 2000 ms")
        sink.error = nil
        p.setReturnMs(100)
        #expect(p.settings.returnMs == 100 && p.problem == nil)
        p.setThinning("extreme")
        #expect(p.settings.thinning == "high", "an unknown name changes nothing")
    }

    @Test func damagedAutomationSettingsKeepTheirDefaults() {
        let defaults = MemoryDefaults()
        let damaged = #"{"afterWrite": "write", "returnMs": 99999, "thinning": 7}"#
        defaults.set(Data(damaged.utf8), forKey: AutomationPreferences.key)
        #expect(AutomationPreferences(defaults: defaults, sink: FakeAutomationSink()).settings == AutomationSettings())
        defaults.set(Data(#"{"returnMs": 500}"#.utf8), forKey: AutomationPreferences.key)
        var want = AutomationSettings()
        want.returnMs = 500
        #expect(AutomationPreferences(defaults: defaults, sink: FakeAutomationSink()).settings == want)
        defaults.set(Data("not json".utf8), forKey: AutomationPreferences.key)
        #expect(AutomationPreferences(defaults: defaults, sink: FakeAutomationSink()).settings == AutomationSettings())
    }
}
