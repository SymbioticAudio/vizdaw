import Foundation
import SymbioticTokens
import Testing
import VizDAWCore
@testable import VizDAWAppModel

/// Settings › Appearance's preference and `ui.theme` (plan 7b Task B4,
/// Rulings 11 and 12).
@MainActor @Suite struct AppearanceTests {
    @Test func theDefaultThemeIsClassicLight() {
        let p = AppearancePreferences(defaults: MemoryDefaults())
        #expect(p.theme.id == "classic-light" && p.theme.name == "Classic Light" && p.colourBars)
    }
    @Test func choosingAThemeSavesItAndTellsTheWindows() {
        let d = MemoryDefaults()
        let p = AppearancePreferences(defaults: d)
        var told: [String] = []
        p.onChange = { told.append($0.id) }
        #expect(p.choose("console"))
        #expect(told == ["console"])
        #expect(AppearancePreferences(defaults: d).theme.id == "console")
    }
    @Test func anUnknownOrDamagedSavedThemeFallsBack() {
        for bytes in [Data(#"{"theme":"neon","colourBars":false}"#.utf8), Data([0xFF, 0x00])] {
            let d = MemoryDefaults()
            d.set(bytes, forKey: AppearancePreferences.key)
            let p = AppearancePreferences(defaults: d)
            #expect(p.theme.id == "classic-light")
            #expect(d.data(forKey: AppearancePreferences.key) == bytes)  // not rewritten
        }
        // Per field: a readable colourBars is kept; garbage gives the default.
        let kept = MemoryDefaults()
        kept.set(Data(#"{"theme":"neon","colourBars":false}"#.utf8), forKey: AppearancePreferences.key)
        #expect(!AppearancePreferences(defaults: kept).colourBars)
        let junk = MemoryDefaults()
        junk.set(Data([0xFF, 0x00]), forKey: AppearancePreferences.key)
        #expect(AppearancePreferences(defaults: junk).colourBars)
    }
    @Test func chooseRefusesAnUnknownTheme() {
        let p = AppearancePreferences(defaults: MemoryDefaults())
        var told = 0
        p.onChange = { _ in told += 1 }
        #expect(!p.choose("svp-light") && !p.choose("") && p.theme.id == "classic-light" && told == 0)
    }
    @Test func themeNamesKeepTheCleanRoom() {
        let banned = ["svp", "studio vision", "opcode", "logic", "apple", "midas", "soundcraft",
                      "steinberg", "cubase", "metro", "zune", "microsoft"]
        for t in Theme.all { #expect(!banned.contains { t.name.lowercased().contains($0) }, "\(t.name)") }
    }
    @Test func uiThemeChecksItsArguments() {
        let p = AppearancePreferences(defaults: MemoryDefaults())
        for bad: [ControlArg] in [[], [.init(name: "theme", value: .text(value: "neon"))]] {
            guard case .failed(let f) = AppearanceCommands.run(bad, prefs: p) else { Issue.record("ran"); continue }
            #expect(f.code == .invalidParams)
        }
        #expect(p.theme.id == "classic-light")
    }
    @Test func uiThemeAnswersItsStatusAndNeedsNoDocument() {
        let p = AppearancePreferences(defaults: MemoryDefaults())
        let r = AppearanceCommands.run([.init(name: "theme", value: .text(value: "flat"))], prefs: p)
        guard case .done(.ran(_, let doc, _, _, let status)) = r else { Issue.record("\(r)"); return }
        #expect(status == "theme flat" && p.theme.id == "flat" && doc == nil)
    }

    // Beyond the brief's seven: the channel's dispatch, the colour bars and
    // the message.

    /// `ui.theme` runs through `ControlCommands` with no document open, on
    /// the host's preferences; an unknown ID names itself.
    @Test func uiThemeRunsWithoutADocumentThroughTheHost() {
        let host = FakeControlHost()
        let store = DocumentStore(makeEmpty: { FakeSession([]) }, makeDemo: { FakeSession([]) }, open: { _ in FakeSession([]) })
        #expect(ControlCommands.handled.contains("ui.theme"))
        let ok = ControlCommands.run(
            "ui.theme", args: [.init(name: "theme", value: .text(value: "eg13"))], doc: nil, store: store, host: host)
        guard case .done(.ran(let id, _, _, _, let status)) = ok else { Issue.record("\(ok)"); return }
        #expect(id == "ui.theme" && status == "theme eg13" && host.appearance.theme.id == "eg13")
        let bad = ControlCommands.run(
            "ui.theme", args: [.init(name: "theme", value: .text(value: "user.nope"))], doc: nil, store: store,
            host: host)
        guard case .failed(let f) = bad else { Issue.record("\(bad)"); return }
        #expect(f.code == .invalidParams && f.message == "user.nope is not a theme.")
        #expect(host.appearance.theme.id == "eg13")
    }

    @Test func theColourBarsAreSavedAndTellNobodyAboutTheTheme() {
        let d = MemoryDefaults()
        let p = AppearancePreferences(defaults: d)
        var told = 0
        p.onChange = { _ in told += 1 }
        p.setColourBars(false)
        #expect(!p.colourBars && told == 0)
        let again = AppearancePreferences(defaults: d)
        #expect(!again.colourBars && again.theme.id == "classic-light")
    }

    @Test func everyBuiltInThemeCanBeChosen() {
        let p = AppearancePreferences(defaults: MemoryDefaults())
        #expect(p.themes.map(\.id) == ["classic-light", "studio-dark", "flat", "console", "symbiotic", "eg13"])
        for t in Theme.all {
            #expect(p.choose(t.id) && p.theme == t)
        }
    }
}
