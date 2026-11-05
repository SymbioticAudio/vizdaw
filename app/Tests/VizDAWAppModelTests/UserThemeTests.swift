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
        // A huge string is not echoed back whole (bytes, not graphemes).
        #expect(r.problems.allSatisfy { $0.utf8.count < 300 }, "\(r.problems.map(\.utf8.count))")
    }

    /// Review I2: one letter with 200 000 combining marks is one grapheme
    /// but 400 001 bytes: too long for a name, and never echoed whole.
    @Test func combiningMarksCountAgainstTheLimits() throws {
        let heavy = "A" + String(repeating: "\u{0301}", count: 200_000)
        #expect(heavy.count == 1)
        let named = parse(#"{"name": "\#(heavy)", "base": "flat"}"#)
        #expect(named.theme == nil)
        #expect(named.problems.allSatisfy { $0.utf8.count < 300 }, "\(named.problems.map(\.utf8.count))")
        let token = parse(#"{"name": "Heavy", "base": "flat", "values": {"surface": {"window": "\#(heavy)"}}}"#)
        #expect(token.theme?.theme.surface == Theme.flat.surface)
        #expect(token.problems.count == 1 && token.problems[0].contains("values.surface.window"))
        #expect(token.problems.allSatisfy { $0.utf8.count < 300 }, "\(token.problems.map(\.utf8.count))")
        // A name of 64 scalars is the longest.
        let mark = "e" + String(repeating: "\u{0301}", count: 31)
        #expect(parse(#"{"name": "\#(mark)\#(mark)", "base": "flat"}"#).theme != nil)
        #expect(parse(#"{"name": "\#(mark)\#(mark)x", "base": "flat"}"#).theme == nil)
    }

    /// Review M7: JSON nested far deeper than the parser's limit is refused
    /// as not JSON, without a crash.
    @Test func deeplyNestedJSONIsRefused() {
        let deep = String(repeating: "[", count: 500_000) + String(repeating: "]", count: 500_000)
        let r = parse(deep)
        #expect(r.theme == nil && r.problems == ["is not JSON"])
        let inValues = #"{"name": "Deep", "base": "flat", "values": "#
            + String(repeating: "{\"a\":", count: 100_000) + "1" + String(repeating: "}", count: 100_001)
        #expect(parse(inValues).theme == nil)
    }

    /// Review I3: a folder of 300 files opens at most `maxFiles` + 1
    /// watches (the folder and the files read), lists the rest as one
    /// problem, and a reload never holds two sets.
    @Test func aCrowdedFolderOpensBoundedWatches() throws {
        let f = try Folder()
        for i in 0..<300 {
            try f.write(String(format: "T%03d.json", i), #"{"name": "T\#(i)", "base": "flat"}"#)
        }
        // Over 1 MB: listed, not read, not watched.
        try f.write("A big.json", data: Data(repeating: 0x20, count: UserThemeFile.maxBytes + 1))
        let watching = CountingWatching()
        let lib = UserThemes(folder: f.url, watching: watching)
        lib.start()
        #expect(lib.themes.count == UserThemeFile.maxFiles)
        #expect(watching.live <= UserThemeFile.maxFiles + 1, "\(watching.live)")
        #expect(!watching.watched.contains(f.url.appendingPathComponent("A big.json").path))
        #expect(watching.peak <= UserThemeFile.maxFiles + 1, "\(watching.peak)")
        lib.reload()
        #expect(watching.peak <= UserThemeFile.maxFiles + 1, "\(watching.peak)")
        #expect(lib.problems.count == 2, "\(lib.problems.map(\.message))")
        #expect(lib.problems.contains { $0.message.contains("236 more") })
    }

    /// Review M4: every ID a file name gives is one the channel accepts
    /// (core-control's `is_slug` after `user.`).
    @Test func everyFileIDIsInTheChannelsGrammar() throws {
        let grammar = try NSRegularExpression(pattern: #"^user\.[a-z0-9]([a-z0-9-]{0,62}[a-z0-9])?$"#)
        let names = [
            "Mine.json", "my dark.json", "--a--b--.json", "Café Noir.json", "ＦＵＬＬ ｗｉｄｔｈ x.json",
            String(repeating: "ab-", count: 40) + ".json", String(repeating: "a", count: 63) + "-b.json",
            "A" + String(repeating: "\u{0301}", count: 1000) + "b.json", "2026.json", "x.y.z.json",
        ]
        for n in names {
            guard let id = UserThemeFile.id(forFile: n) else { continue }
            let range = NSRange(id.startIndex..., in: id)
            #expect(grammar.firstMatch(in: id, range: range) != nil, "\(n) gave \(id)")
        }
        #expect(UserThemeFile.id(forFile: "!!!.json") == nil)
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

    /// B4 rereview N4: a save that lands after the folder is read but before
    /// its watch opens is not missed (the read is repeated once armed).
    @Test func aSaveBetweenTheReadAndTheWatchIsNotMissed() throws {
        let f = try Folder()
        try f.write("Mine.json", #"{"name": "Mine", "base": "console"}"#)
        // Saves on the first arming of the folder: after the read, before
        // any event could be delivered.
        let watching = SavingWatching(folder: f.url.path) {
            try? f.write("Late.json", #"{"name": "Late", "base": "flat"}"#)
        }
        let lib = UserThemes(folder: f.url, watching: watching)
        lib.start()
        #expect(lib.themes.map(\.theme.name).sorted() == ["Late", "Mine"])
        #expect(watching.saved == 1)
        #expect(watching.watched.contains(f.url.appendingPathComponent("Late.json").path), "the late file is watched")
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

/// Counts live watches and the most held at once (review I3).
@MainActor final class CountingWatching: FileWatching {
    final class Token {
        let owner: CountingWatching
        init(owner: CountingWatching) { self.owner = owner }
        deinit {
            // A strong `let`, not a weak capture, into the main actor (Swift 6.1).
            let owner = self.owner
            MainActor.assumeIsolated { owner.live -= 1 }
        }
    }

    fileprivate(set) var live = 0
    private(set) var peak = 0
    private(set) var watched: [String] = []

    func watch(_ path: String, _ changed: @escaping @MainActor () -> Void) -> AnyObject? {
        live += 1
        peak = max(peak, live)
        watched.append(path)
        return Token(owner: self)
    }
}

/// Runs `save` once, the first time the folder is armed: a save racing
/// the read (B4 rereview N4). It never fires an event itself.
@MainActor final class SavingWatching: FileWatching {
    let folder: String
    let save: () -> Void
    private(set) var saved = 0
    private(set) var watched: [String] = []

    init(folder: String, save: @escaping () -> Void) {
        self.folder = folder
        self.save = save
    }

    func watch(_ path: String, _ changed: @escaping @MainActor () -> Void) -> AnyObject? {
        watched.append(path)
        if path == folder && saved == 0 {
            saved += 1
            save()
        }
        return NSObject()
    }
}
