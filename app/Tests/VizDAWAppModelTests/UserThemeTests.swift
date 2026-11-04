import Foundation
import SymbioticTokens
import Testing
import VizDAWCore
@testable import VizDAWAppModel

/// User themes (controller ruling R-UT): JSON files in a themes folder with
/// tokens.json's theme schema plus "name" and "base". Every test uses its
/// own temporary folder, never the real one.
@MainActor @Suite struct UserThemeTests {
    /// A fresh temporary themes folder.
    final class Folder {
        let url: URL

        init() throws {
            url = FileManager.default.temporaryDirectory
                .appendingPathComponent("vizdaw-themes-\(UUID().uuidString)", isDirectory: true)
            try FileManager.default.createDirectory(at: url, withIntermediateDirectories: true)
        }

        deinit { try? FileManager.default.removeItem(at: url) }

        func write(_ name: String, _ text: String) throws {
            try Data(text.utf8).write(to: url.appendingPathComponent(name))
        }

        func write(_ name: String, data: Data) throws {
            try data.write(to: url.appendingPathComponent(name))
        }

        func remove(_ name: String) throws {
            try FileManager.default.removeItem(at: url.appendingPathComponent(name))
        }

        var files: [String] {
            ((try? FileManager.default.contentsOfDirectory(atPath: url.path)) ?? []).sorted()
        }
    }

    func parse(_ text: String, file: String = "Mine.json") -> (theme: UserTheme?, problems: [String]) {
        UserThemeFile.parse(Data(text.utf8), file: file)
    }

    @Test func aSmallFileOverridesItsBase() throws {
        let r = parse(
            #"""
            {"name": "Night Owl", "base": "studio-dark",
             "values": {"surface": {"window": "#102030", "panel": "#10203080"},
                        "accent": {"accent": {"system": "systemTeal", "fallback": "#00AAAA"}},
                        "spacing": {"m": 14}, "type": {"title": {"size": 17}}},
             "style": {"labels": "caps"}, "knobFinish": "shaded"}
            """#)
        let u = try #require(r.theme)
        #expect(r.problems == [])
        let t = u.theme
        let base = Theme.studioDark
        #expect(t.id == "user.mine" && t.name == "Night Owl" && u.base == "studio-dark" && u.file == "Mine.json")
        #expect(t.appearance == .dark && t.knobFinish == .shaded && t.style.labels == .caps)
        #expect(t.style.corners == base.style.corners)
        #expect(t.surface.window == ThemeColor(hex: 0x102030FF))
        #expect(t.surface.panel == ThemeColor(hex: 0x10203080))
        #expect(t.surface.raised == base.surface.raised && t.text == base.text && t.meter == base.meter)
        #expect(t.accent.accent == ThemeColor(system: .systemTeal, fallback: 0x00AAAAFF, appearance: .dark))
        #expect(t.spacing.m == 14 && t.spacing.s == base.spacing.s)
        #expect(t.type.title == TypeStyle(size: 17, weight: base.type.title.weight) && t.type.body == base.type.body)
    }

    @Test func everyBuiltInThemeRoundTripsThroughAFile() throws {
        for b in Theme.all {
            let data = UserThemeFile.encode(b, name: "Copy of \(b.id)", base: b.id)
            let r = UserThemeFile.parse(data, file: "Copy.json")
            #expect(r.problems == [], "\(b.id): \(r.problems)")
            let t = try #require(r.theme?.theme, "\(b.id)")
            #expect(t.name == "Copy of \(b.id)" && t.id == "user.copy")
            #expect(t.appearance == b.appearance && t.knobFinish == b.knobFinish && t.style == b.style, "\(b.id)")
            #expect(t.surface == b.surface && t.text == b.text && t.accent == b.accent && t.state == b.state, "\(b.id)")
            #expect(t.meter == b.meter && t.graph == b.graph && t.control == b.control && t.strip == b.strip, "\(b.id)")
            #expect(t.brand == b.brand && t.spacing == b.spacing && t.radius == b.radius, "\(b.id)")
            #expect(t.iconStroke == b.iconStroke && t.type == b.type && t.knob == b.knob && t.fader == b.fader, "\(b.id)")
            #expect(t.relief == b.relief && t.swatch == b.swatch && t.keys == b.keys, "\(b.id)")
            #expect(t.readout == b.readout && t.band == b.band, "\(b.id)")
        }
    }

    /// The parser under hostile files: each problem is listed, nothing
    /// crashes, and a token that does not read keeps the base's.
    @Test func hostileFilesAreListedNotFatal() throws {
        // Not a usable file at all.
        for (text, says) in [
            ("[1, 2]", "not a JSON object"),
            ("\"theme\"", "not a JSON object"),
            ("{", "not JSON"),
            ("", "not JSON"),
            (#"{"base": "flat"}"#, "\"name\""),
            (#"{"name": "", "base": "flat"}"#, "\"name\""),
            (#"{"name": "   ", "base": "flat"}"#, "\"name\""),
            (#"{"name": 7, "base": "flat"}"#, "\"name\""),
            (#"{"name": "A\u0007B", "base": "flat"}"#, "\"name\""),
            (#"{"name": "N"}"#, "\"base\""),
            (#"{"name": "N", "base": "neon"}"#, "\"base\""),
            (#"{"name": "N", "base": "user.other"}"#, "\"base\""),
            (#"{"name": "N", "base": ["flat"]}"#, "\"base\""),
        ] {
            let r = parse(text)
            #expect(r.theme == nil, "\(text)")
            #expect(r.problems.count == 1 && r.problems[0].contains(says), "\(text): \(r.problems)")
        }
        let long = String(repeating: "x", count: 65)
        #expect(parse(#"{"name": "\#(long)", "base": "flat"}"#).theme == nil)

        // Usable, with each bad token listed and the base's kept.
        let r = parse(
            #"""
            {"name": "Rough", "base": "flat", "colour": 1, "appearance": "dim", "knobFinish": 3,
             "style": {"corners": "bevelled", "glow": "on", "labels": true},
             "values": {
               "surface": {"window": "#GGHHII", "panel": "#12345", "raised": 7, "inset": "\#(String(repeating: "#", count: 100_000))",
                           "separator": {"system": "notAColour", "fallback": "#000000"},
                           "selection": {"system": "systemRed"}, "shine": "#FFFFFF"},
               "spacing": {"s": -1, "m": "12", "l": 1e12, "xl": true},
               "iconStroke": {"s16": 400},
               "type": {"body": {"size": 0, "weight": "extraBold"}, "title": "big"},
               "brand": {"gold": "#000000"},
               "sparkle": {"a": "#FFFFFF"},
               "text": ["#FFFFFF"]
             }}
            """#)
        let t = try #require(r.theme?.theme)
        let b = Theme.flat
        #expect(t.name == "Rough")
        #expect(t.appearance == b.appearance && t.knobFinish == b.knobFinish && t.style == b.style)
        #expect(t.surface == b.surface && t.spacing == b.spacing && t.iconStroke == b.iconStroke && t.type == b.type)
        #expect(t.brand == b.brand && t.text == b.text)
        let all = r.problems.joined(separator: "\n")
        for needle in [
            "colour", "appearance", "knobFinish", "style.corners", "style.glow", "style.labels",
            "values.surface.window", "values.surface.panel", "values.surface.raised", "values.surface.inset",
            "values.surface.separator", "values.surface.selection", "values.surface.shine",
            "values.spacing.s", "values.spacing.m", "values.spacing.l", "values.spacing.xl",
            "values.iconStroke.s16", "values.type.body", "values.type.title", "values.brand", "values.sparkle",
            "values.text",
        ] {
            #expect(all.contains(needle), "\(needle) not listed in \(all)")
        }
        // A huge string is not echoed back whole.
        #expect(r.problems.allSatisfy { $0.count < 300 }, "\(r.problems.map(\.count))")
    }

    @Test func aFileOverTheSizeLimitIsNotRead() throws {
        let f = try Folder()
        // 10 MB of valid-looking JSON.
        var big = Data(#"{"name": "Big", "base": "flat", "note": ""#.utf8)
        big.append(Data(repeating: UInt8(ascii: "a"), count: 10_000_000))
        big.append(Data(#""}"#.utf8))
        try f.write("Big.json", data: big)
        let load = UserThemeFile.load(folder: f.url)
        #expect(load.themes.isEmpty)
        #expect(load.problems.count == 1 && load.problems[0].file == "Big.json")
        #expect(load.problems[0].message.contains("larger than"))
    }

    @Test func clashingNamesAndIDsAreListedProblems() throws {
        let f = try Folder()
        try f.write("Console.json", #"{"name": "console", "base": "flat"}"#)  // a built-in's name
        try f.write("A Dark.json", #"{"name": "Dark One", "base": "studio-dark"}"#)
        try f.write("a-dark.json", #"{"name": "Dark Two", "base": "studio-dark"}"#)  // the same ID
        try f.write("Other.json", #"{"name": "Dark One", "base": "flat"}"#)  // the same name
        try f.write("Fine.json", #"{"name": "Fine", "base": "eg13"}"#)
        try f.write("!!!.json", #"{"name": "No ID", "base": "eg13"}"#)
        try f.write("notes.txt", "not a theme")
        try f.write(".hidden.json", "{")
        let load = UserThemeFile.load(folder: f.url)
        #expect(load.themes.map(\.theme.id) == ["user.a-dark", "user.fine"])
        #expect(load.themes.map(\.theme.name) == ["Dark One", "Fine"])
        let byFile = Dictionary(grouping: load.problems, by: \.file).mapValues { $0.map(\.message) }
        #expect(Set(byFile.keys) == ["Console.json", "a-dark.json", "Other.json", "!!!.json"], "\(byFile)")
        #expect(byFile["Console.json"]?.first?.contains("built-in") == true)
        #expect(byFile["a-dark.json"]?.first?.contains("user.a-dark") == true)
        #expect(byFile["Other.json"]?.first?.contains("Dark One") == true)
    }

    /// A2 review M2: the radius tokens win over `style.corners`, which only
    /// describes the theme; a conflict is listed.
    @Test func radiusWinsOverCorners() throws {
        let r = parse(#"{"name": "Soft", "base": "flat", "values": {"radius": {"s": 4, "m": 8, "l": 12}}}"#)
        let t = try #require(r.theme?.theme)
        #expect(t.style.corners == .square && t.radius.s == 4 && t.radius.m == 8)
        #expect(r.problems.count == 1 && r.problems[0].contains("radius"), "\(r.problems)")
        let quiet = parse(#"{"name": "Plain", "base": "flat"}"#)
        #expect(quiet.problems == [])
    }

    @Test func theFolderIsReadAgainWhenItChanges() throws {
        let f = try Folder()
        let watching = FakeWatching()
        try f.write("Mine.json", #"{"name": "Mine", "base": "console"}"#)
        let lib = UserThemes(folder: f.url, watching: watching)
        lib.start()
        #expect(watching.started.contains(f.url.path))
        let p = AppearancePreferences(defaults: MemoryDefaults(), userThemes: lib)
        #expect(p.themes.map(\.id).suffix(1) == ["user.mine"])
        var told: [String] = []
        p.onChange = { told.append($0.name) }
        #expect(p.choose("user.mine") && p.theme.name == "Mine")
        // Edited: the theme in use follows the file.
        try f.write("Mine.json", #"{"name": "Mine Again", "base": "console"}"#)
        watching.fire(f.url.appendingPathComponent("Mine.json").path)
        #expect(p.theme.name == "Mine Again" && told == ["Mine", "Mine Again"])
        // A bad edit is listed; the file's other tokens still apply.
        try f.write("Mine.json", #"{"name": "Mine Again", "base": "console", "values": {"surface": {"window": "red"}}}"#)
        watching.fire(f.url.path)
        #expect(lib.problems.map(\.file) == ["Mine.json"] && p.theme.id == "user.mine")
        // Removed: Classic Light, with the choice kept for the file's return.
        try f.remove("Mine.json")
        watching.fire(f.url.path)
        #expect(p.theme.id == "classic-light" && lib.themes.isEmpty && lib.problems.isEmpty)
        try f.write("Mine.json", #"{"name": "Back", "base": "console"}"#)
        watching.fire(f.url.path)
        #expect(p.theme.name == "Back")
    }

    @Test func aSavedUserThemeIsChosenAtLaunch() throws {
        let f = try Folder()
        try f.write("Mine.json", #"{"name": "Mine", "base": "symbiotic"}"#)
        let d = MemoryDefaults()
        let lib = UserThemes(folder: f.url, watching: FakeWatching())
        lib.start()
        #expect(AppearancePreferences(defaults: d, userThemes: lib).choose("user.mine"))
        let again = UserThemes(folder: f.url, watching: FakeWatching())
        again.start()
        #expect(AppearancePreferences(defaults: d, userThemes: again).theme.name == "Mine")
        // Without its file: Classic Light, and the choice is not rewritten.
        let saved = d.data(forKey: AppearancePreferences.key)
        #expect(AppearancePreferences(defaults: d).theme.id == "classic-light")
        #expect(d.data(forKey: AppearancePreferences.key) == saved)
    }

    @Test func duplicateAsFileNeverOverwrites() throws {
        let f = try Folder()
        let watching = FakeWatching()
        let lib = UserThemes(folder: f.url, watching: watching)
        lib.start()
        let p = AppearancePreferences(defaults: MemoryDefaults(), userThemes: lib)
        #expect(p.choose("console"))
        try f.write("Console Copy 2.json", "keep me")
        let first = try p.duplicateCurrent()
        #expect(first.lastPathComponent == "Console Copy.json")
        let second = try p.duplicateCurrent()
        #expect(second.lastPathComponent == "Console Copy 3.json")
        #expect(f.files == ["Console Copy 2.json", "Console Copy 3.json", "Console Copy.json"])
        #expect(try String(contentsOf: f.url.appendingPathComponent("Console Copy 2.json"), encoding: .utf8) == "keep me")
        // The copies load as themes on the console base, without problems
        // but the unreadable file's.
        lib.reload()
        #expect(lib.themes.map(\.theme.name) == ["Console Copy 3", "Console Copy"])
        #expect(lib.themes.allSatisfy { $0.base == "console" && $0.theme.surface == Theme.console.surface })
        #expect(lib.problems.map(\.file) == ["Console Copy 2.json"])
        // A user theme's copy keeps its base.
        #expect(p.choose("user.console-copy"))
        let third = try p.duplicateCurrent()
        #expect(third.lastPathComponent == "Console Copy Copy.json")
        lib.reload()
        #expect(lib.themes.first { $0.file == "Console Copy Copy.json" }?.base == "console")
    }

    @Test func uiThemeChoosesAUserTheme() throws {
        let f = try Folder()
        try f.write("Mine.json", #"{"name": "Mine", "base": "flat"}"#)
        let lib = UserThemes(folder: f.url, watching: FakeWatching())
        lib.start()
        let p = AppearancePreferences(defaults: MemoryDefaults(), userThemes: lib)
        let r = AppearanceCommands.run([.init(name: "theme", value: .text(value: "user.mine"))], prefs: p)
        guard case .done(.ran(_, _, _, _, let status)) = r else { Issue.record("\(r)"); return }
        #expect(status == "theme user.mine" && p.theme.name == "Mine")
    }
}
