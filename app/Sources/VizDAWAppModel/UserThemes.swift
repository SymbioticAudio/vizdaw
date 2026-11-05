import CoreGraphics
import Foundation
import Observation
import SymbioticTokens

/// A theme read from a file in the themes folder (controller ruling R-UT).
public struct UserTheme: Equatable, Sendable {
    /// Its ID is `user.` and the file name's slug; its name is the file's.
    public let theme: Theme
    /// The built-in theme its missing or unreadable tokens come from.
    public let base: String
    /// The file's name in the folder.
    public let file: String
}

/// Something wrong in a theme file, as Settings › Appearance lists it.
public struct UserThemeProblem: Equatable, Sendable, Identifiable {
    /// The file's name; empty for a problem of the folder as a whole.
    public let file: String
    public let message: String
    public var id: String { file + "\n" + message }
}

/// A themes folder that cannot be written, or no folder at all.
public struct UserThemeError: LocalizedError, Equatable, Sendable {
    public let message: String
    public var errorDescription: String? { message }
}

/// The user theme file: tokens.json's theme schema (`appearance`,
/// `knobFinish`, `style`, `values`) plus a required `name` and `base` (one
/// of the built-in IDs). Every token is optional; one that is missing keeps
/// the base's, one that does not read keeps the base's and is listed.
/// `id`, `note` and `placeholder` are accepted and ignored: the ID comes
/// from the file name. `brand` is the same in every theme and cannot be
/// changed. The radius tokens decide corners; `style.corners` only
/// describes them (A2 review M2).
public enum UserThemeFile {
    /// A file larger than this is not read (1 MiB).
    public static let maxBytes = 1 << 20
    /// The most theme files read from the folder.
    public static let maxFiles = 64
    /// The longest name, in Unicode scalars.
    public static let maxName = 64
    /// The most problems listed for the whole folder.
    static let maxListed = 200
    /// The most problems listed for one file.
    static let maxProblems = 40
    public static let idPrefix = "user."

    /// `user.` and the file name's slug (lowercase letters, digits and
    /// single hyphens, at most 64), as `ui.theme` accepts it; nil when the
    /// name has no letter or digit.
    public static func id(forFile file: String) -> String? {
        let stem = (file as NSString).deletingPathExtension.lowercased()
        var slug = ""
        for ch in stem.unicodeScalars {
            if ch.isASCII, CharacterSet.alphanumerics.contains(ch) {
                slug.unicodeScalars.append(ch)
            } else if !slug.isEmpty, !slug.hasSuffix("-") {
                slug += "-"
            }
        }
        while slug.hasSuffix("-") { slug.removeLast() }
        if slug.count > 64 {
            slug = String(slug.prefix(64))
            while slug.hasSuffix("-") { slug.removeLast() }
        }
        return slug.isEmpty ? nil : idPrefix + slug
    }

    // MARK: Reading

    /// Reads one file's bytes. The theme is nil when the file cannot be a
    /// theme (not a JSON object, no usable name or base); `problems` lists
    /// what is wrong either way.
    public static func parse(_ data: Data, file: String) -> (theme: UserTheme?, problems: [String]) {
        guard data.count <= maxBytes else { return (nil, ["is larger than 1 MB; it was not read"]) }
        guard let any = try? JSONSerialization.jsonObject(with: data, options: [.fragmentsAllowed]) else {
            return (nil, ["is not JSON"])
        }
        guard let top = any as? [String: Any] else { return (nil, ["is not a JSON object"]) }
        guard let rawName = top["name"] as? String,
            case let name = rawName.trimmingCharacters(in: .whitespacesAndNewlines),
            // Scalars, not graphemes: one letter can carry any number of
            // combining marks (B4 review I2).
            !name.isEmpty, name.unicodeScalars.count <= maxName,
            name.unicodeScalars.allSatisfy({ !CharacterSet.controlCharacters.contains($0) })
        else {
            return (nil, ["needs a \"name\": text of 1 to \(maxName) characters"])
        }
        let ids = Theme.all.map(\.id)
        guard let baseID = top["base"] as? String, let base = Theme.named(baseID) else {
            return (nil, ["needs a \"base\": one of " + ids.joined(separator: ", ")])
        }
        guard let id = id(forFile: file) else {
            return (nil, ["has no letter or digit in its file name, so it has no ID"])
        }
        var r = Reader(base: base)
        for key in top.keys.sorted()
        where !["name", "base", "id", "note", "placeholder", "appearance", "knobFinish", "style", "values"]
            .contains(key)
        {
            r.problem("\(quote(key)) is not a theme key; ignored")
        }
        let theme = r.theme(top, id: id, name: name)
        return (UserTheme(theme: theme, base: baseID, file: file), r.problems)
    }

    /// The themes in `folder` and the problems of its files, in file name
    /// order. Hidden files and files not ending in `.json` are skipped. A
    /// name a built-in theme has, or a name or ID an earlier file has, is a
    /// problem and that file is left out.
    public static func load(folder: URL) -> (themes: [UserTheme], problems: [UserThemeProblem], read: [String]) {
        let fm = FileManager.default
        let keys: [URLResourceKey] = [.isRegularFileKey, .fileSizeKey]
        let urls = (try? fm.contentsOfDirectory(at: folder, includingPropertiesForKeys: keys, options: []))
            ?? []
        let files = urls.filter { !$0.lastPathComponent.hasPrefix(".") && $0.pathExtension.lowercased() == "json" }
            .sorted { $0.lastPathComponent < $1.lastPathComponent }
        var themes: [UserTheme] = []
        var problems: [UserThemeProblem] = []
        var read: [String] = []
        var unread = 0
        var unlisted = 0
        let builtIn = Set(Theme.all.map { $0.name.lowercased() })
        for url in files {
            let file = url.lastPathComponent
            let say = { (m: String) in
                if problems.count < maxListed {
                    problems.append(UserThemeProblem(file: file, message: m))
                } else {
                    unlisted += 1
                }
            }
            // At most `maxFiles` are read (and watched: B4 review I3); the
            // rest are one problem.
            guard read.count < maxFiles else {
                unread += 1
                continue
            }
            let v = try? url.resourceValues(forKeys: Set(keys))
            guard v?.isRegularFile == true else { continue }
            if let size = v?.fileSize, size > maxBytes {
                say("is larger than 1 MB; it was not read")
                continue
            }
            guard let data = try? Data(contentsOf: url) else {
                say("could not be read")
                continue
            }
            read.append(file)
            let r = parse(data, file: file)
            r.problems.forEach(say)
            guard let u = r.theme else { continue }
            let lower = u.theme.name.lowercased()
            if builtIn.contains(lower) {
                say("is named \(quote(u.theme.name)), a built-in theme's name; give it another name")
            } else if let other = themes.first(where: { $0.theme.id == u.theme.id }) {
                say("has the ID \(u.theme.id), as \(other.file) has; rename one of the files")
            } else if let other = themes.first(where: { $0.theme.name.lowercased() == lower }) {
                say("is named \(quote(u.theme.name)), as \(other.file) is; give it another name")
            } else {
                themes.append(u)
            }
        }
        if unread > 0 {
            problems.append(
                UserThemeProblem(
                    file: "",
                    message: "\(unread) more theme files were not read: VizDAW reads at most \(maxFiles)"))
        }
        if unlisted > 0 {
            problems.append(UserThemeProblem(file: "", message: "\(unlisted) more problems are not listed"))
        }
        return (themes, problems, read)
    }

    /// `s` in quotes, cut to 40 Unicode scalars (a hostile file's text is never
    /// echoed whole).
    static func quote(_ s: String) -> String {
        // Cut by scalars (B4 review I2), each control character shown as "?".
        var clean = String.UnicodeScalarView()
        for u in s.unicodeScalars.prefix(40) {
            clean.append(CharacterSet.controlCharacters.contains(u) ? "?" : u)
        }
        return "\"" + String(clean) + (s.unicodeScalars.count > 40 ? "…" : "") + "\""
    }

    /// Reads a file's tokens over its base, listing what does not read.
    struct Reader {
        let base: Theme
        var problems: [String] = []
        private var values: [String: Any] = [:]
        private var dropped = 0

        init(base: Theme) { self.base = base }

        mutating func problem(_ m: String) {
            if problems.count < UserThemeFile.maxProblems {
                problems.append(m)
            } else {
                dropped += 1
                let more = "… and \(dropped) more"
                if problems.count == UserThemeFile.maxProblems { problems.append(more) } else { problems[problems.count - 1] = more }
            }
        }

        private func describe(_ v: Any) -> String {
            switch v {
            case let s as String: return UserThemeFile.quote(s)
            case is [Any]: return "a list"
            case is [String: Any]: return "an object"
            case let n as NSNumber where CFGetTypeID(n) == CFBooleanGetTypeID(): return n.boolValue ? "true" : "false"
            case let n as NSNumber: return UserThemeFile.quote(n.stringValue)
            default: return "null"
            }
        }

        /// One of `E`'s raw values at `key` of `obj`, else `fallback`.
        private mutating func choice<E: RawRepresentable & CaseIterable>(
            _ obj: [String: Any], _ key: String, _ path: String, _ fallback: E
        ) -> E where E.RawValue == String {
            guard let v = obj[key] else { return fallback }
            if let s = v as? String, let e = E(rawValue: s) { return e }
            let all = E.allCases.map(\.rawValue).joined(separator: ", ")
            problem("\(path): \(describe(v)) is not one of \(all); the base's is kept")
            return fallback
        }

        mutating func theme(_ top: [String: Any], id: String, name: String) -> Theme {
            let appearance = choice(top, "appearance", "appearance", base.appearance)
            let finish = choice(top, "knobFinish", "knobFinish", base.knobFinish)
            var style = base.style
            var cornersGiven = false
            if let raw = top["style"] {
                if let s = raw as? [String: Any] {
                    style = readStyle(s)
                    cornersGiven = s["corners"] != nil
                } else {
                    problem("style: \(describe(raw)) is not an object; the base's is kept")
                }
            }
            if let raw = top["values"] {
                if let v = raw as? [String: Any] {
                    values = v
                } else {
                    problem("values: \(describe(raw)) is not an object; the base's are kept")
                }
            }
            for key in values.keys.sorted() where !UserThemeFile.groups.keys.contains(key) {
                problem("values.\(key) is not a token group; ignored")
            }
            if values["brand"] != nil {
                problem("values.brand is the same in every theme and cannot be changed; ignored")
            }
            for (group, items) in UserThemeFile.groups where group != "brand" {
                guard let raw = values[group] else { continue }
                guard let obj = raw as? [String: Any] else {
                    problem("values.\(group): \(describe(raw)) is not an object; the base's are kept")
                    continue
                }
                for key in obj.keys.sorted() where !items.contains(key) {
                    problem("values.\(group).\(key) is not a \(group) token; ignored")
                }
            }
            let b = base
            let t = Theme(
                id: id, name: name, appearance: appearance, knobFinish: finish, style: style, isPlaceholder: false,
                surface: .init(
                    window: color("surface", "window", b.surface.window),
                    panel: color("surface", "panel", b.surface.panel),
                    raised: color("surface", "raised", b.surface.raised),
                    inset: color("surface", "inset", b.surface.inset),
                    separator: color("surface", "separator", b.surface.separator),
                    selection: color("surface", "selection", b.surface.selection),
                    focusRing: color("surface", "focusRing", b.surface.focusRing)),
                text: .init(
                    primary: color("text", "primary", b.text.primary),
                    secondary: color("text", "secondary", b.text.secondary),
                    disabled: color("text", "disabled", b.text.disabled),
                    inverse: color("text", "inverse", b.text.inverse)),
                accent: .init(
                    accent: color("accent", "accent", b.accent.accent),
                    onAccent: color("accent", "onAccent", b.accent.onAccent)),
                state: .init(
                    record: color("state", "record", b.state.record), play: color("state", "play", b.state.play),
                    solo: color("state", "solo", b.state.solo), mute: color("state", "mute", b.state.mute),
                    warning: color("state", "warning", b.state.warning),
                    error: color("state", "error", b.state.error),
                    offline: color("state", "offline", b.state.offline)),
                meter: .init(
                    background: color("meter", "background", b.meter.background),
                    low: color("meter", "low", b.meter.low), mid: color("meter", "mid", b.meter.mid),
                    high: color("meter", "high", b.meter.high), clip: color("meter", "clip", b.meter.clip)),
                graph: .init(
                    line: color("graph", "line", b.graph.line), fill: color("graph", "fill", b.graph.fill),
                    grid: color("graph", "grid", b.graph.grid), handle: color("graph", "handle", b.graph.handle),
                    handleSelected: color("graph", "handleSelected", b.graph.handleSelected)),
                control: .init(
                    gain: color("control", "gain", b.control.gain),
                    eqLow: color("control", "eqLow", b.control.eqLow),
                    eqLowMid: color("control", "eqLowMid", b.control.eqLowMid),
                    eqHighMid: color("control", "eqHighMid", b.control.eqHighMid),
                    eqHigh: color("control", "eqHigh", b.control.eqHigh),
                    send: color("control", "send", b.control.send), pan: color("control", "pan", b.control.pan),
                    dynamics: color("control", "dynamics", b.control.dynamics),
                    filter: color("control", "filter", b.control.filter)),
                strip: .init(
                    channel: color("strip", "channel", b.strip.channel),
                    instrument: color("strip", "instrument", b.strip.instrument),
                    auxReturn: color("strip", "auxReturn", b.strip.auxReturn),
                    bus: color("strip", "bus", b.strip.bus), master: color("strip", "master", b.strip.master)),
                brand: b.brand,
                spacing: .init(
                    xxs: points("spacing", "xxs", b.spacing.xxs), xs: points("spacing", "xs", b.spacing.xs),
                    s: points("spacing", "s", b.spacing.s), m: points("spacing", "m", b.spacing.m),
                    l: points("spacing", "l", b.spacing.l), xl: points("spacing", "xl", b.spacing.xl)),
                radius: .init(
                    none: points("radius", "none", b.radius.none), s: points("radius", "s", b.radius.s),
                    m: points("radius", "m", b.radius.m), l: points("radius", "l", b.radius.l)),
                iconStroke: .init(
                    s16: points("iconStroke", "s16", b.iconStroke.s16, max: 16),
                    s24: points("iconStroke", "s24", b.iconStroke.s24, max: 16),
                    s32: points("iconStroke", "s32", b.iconStroke.s32, max: 16)),
                type: .init(
                    caption: font("caption", b.type.caption), body: font("body", b.type.body),
                    label: font("label", b.type.label), title: font("title", b.type.title),
                    display: font("display", b.type.display)),
                knob: .init(
                    skirt: color("knob", "skirt", b.knob.skirt), cap: color("knob", "cap", b.knob.cap),
                    track: color("knob", "track", b.knob.track),
                    indicator: color("knob", "indicator", b.knob.indicator)),
                fader: .init(
                    well: color("fader", "well", b.fader.well), scale: color("fader", "scale", b.fader.scale),
                    capLine: color("fader", "capLine", b.fader.capLine)),
                relief: .init(
                    highlight: color("relief", "highlight", b.relief.highlight),
                    shadow: color("relief", "shadow", b.relief.shadow),
                    capTop: color("relief", "capTop", b.relief.capTop),
                    capBottom: color("relief", "capBottom", b.relief.capBottom),
                    panelTop: color("relief", "panelTop", b.relief.panelTop),
                    panelBottom: color("relief", "panelBottom", b.relief.panelBottom)),
                swatch: .init(
                    red: color("swatch", "red", b.swatch.red), orange: color("swatch", "orange", b.swatch.orange),
                    yellow: color("swatch", "yellow", b.swatch.yellow),
                    green: color("swatch", "green", b.swatch.green), teal: color("swatch", "teal", b.swatch.teal),
                    blue: color("swatch", "blue", b.swatch.blue),
                    purple: color("swatch", "purple", b.swatch.purple),
                    pink: color("swatch", "pink", b.swatch.pink)),
                keys: .init(white: color("keys", "white", b.keys.white), black: color("keys", "black", b.keys.black)),
                readout: .init(
                    background: color("readout", "background", b.readout.background),
                    text: color("readout", "text", b.readout.text)),
                band: .init(light: color("band", "light", b.band.light), dark: color("band", "dark", b.band.dark)))
            let rounds = [t.radius.s, t.radius.m, t.radius.l].contains { $0 > 0 }
            if (cornersGiven || values["radius"] != nil), rounds == (t.style.corners == .square) {
                problem(
                    "style.corners is \(t.style.corners.rawValue) but the radius tokens are "
                        + (rounds ? "not all 0" : "all 0") + "; the radius tokens win (corners only describes them)")
            }
            return t
        }

        private mutating func readStyle(_ s: [String: Any]) -> ThemeStyle {
            let b = base.style
            for key in s.keys.sorted() where !UserThemeFile.styleKeys.contains(key) {
                problem("style.\(key) is not a style; ignored")
            }
            return ThemeStyle(
                labels: choice(s, "labels", "style.labels", b.labels),
                legendFamily: choice(s, "legendFamily", "style.legendFamily", b.legendFamily),
                numberFamily: choice(s, "numberFamily", "style.numberFamily", b.numberFamily),
                corners: choice(s, "corners", "style.corners", b.corners),
                panelBands: choice(s, "panelBands", "style.panelBands", b.panelBands),
                knobSkirt: choice(s, "knobSkirt", "style.knobSkirt", b.knobSkirt),
                capStyle: choice(s, "capStyle", "style.capStyle", b.capStyle),
                tile: choice(s, "tile", "style.tile", b.tile))
        }

        private func item(_ group: String, _ key: String) -> Any? {
            (values[group] as? [String: Any])?[key]
        }

        private mutating func color(_ group: String, _ key: String, _ fallback: ThemeColor) -> ThemeColor {
            guard let v = item(group, key) else { return fallback }
            let path = "values.\(group).\(key)"
            if let s = v as? String {
                if let hex = UserThemeFile.hex(s) { return ThemeColor(hex: hex) }
                problem("\(path): \(describe(v)) is not #RRGGBB or #RRGGBBAA; the base's is kept")
                return fallback
            }
            if let o = v as? [String: Any], let sys = o["system"] {
                guard Set(o.keys) == ["system", "fallback"] else {
                    problem("\(path): a system colour needs exactly \"system\" and \"fallback\"; the base's is kept")
                    return fallback
                }
                guard let name = (sys as? String).flatMap(SystemColorName.init(rawValue:)) else {
                    problem("\(path): \(describe(sys)) is not a macOS system colour; the base's is kept")
                    return fallback
                }
                guard let fb = (o["fallback"] as? String).flatMap(UserThemeFile.hex) else {
                    problem("\(path): the fallback is not #RRGGBB or #RRGGBBAA; the base's is kept")
                    return fallback
                }
                // The theme pins it to its own appearance.
                return ThemeColor(system: name, fallback: fb, appearance: .light)
            }
            problem("\(path): \(describe(v)) is not a colour (\"#RRGGBB\", or a system colour); the base's is kept")
            return fallback
        }

        private mutating func number(_ v: Any, _ path: String, _ range: ClosedRange<Double>) -> Double? {
            guard let n = v as? NSNumber, CFGetTypeID(n) != CFBooleanGetTypeID(), case let d = n.doubleValue,
                d.isFinite, range.contains(d)
            else {
                problem("\(path): \(describe(v)) is not a number from \(fmt(range.lowerBound)) to \(fmt(range.upperBound)); the base's is kept")
                return nil
            }
            return d
        }

        private func fmt(_ d: Double) -> String { d == d.rounded() ? String(Int(d)) : String(d) }

        private mutating func points(_ group: String, _ key: String, _ fallback: CGFloat, max: Double = 1000)
            -> CGFloat
        {
            guard let v = item(group, key) else { return fallback }
            return number(v, "values.\(group).\(key)", 0...max).map { CGFloat($0) } ?? fallback
        }

        private mutating func font(_ key: String, _ fallback: TypeStyle) -> TypeStyle {
            guard let v = item("type", key) else { return fallback }
            let path = "values.type.\(key)"
            guard let o = v as? [String: Any] else {
                problem("\(path): \(describe(v)) is not {\"size\": …, \"weight\": …}; the base's is kept")
                return fallback
            }
            for k in o.keys.sorted() where k != "size" && k != "weight" {
                problem("\(path).\(k) is not a font field; ignored")
            }
            var size = fallback.size
            if let s = o["size"], let d = number(s, path + ".size", 1...200) { size = CGFloat(d) }
            let weight = choice(o, "weight", path + ".weight", fallback.weight)
            return TypeStyle(size: size, weight: weight)
        }
    }

    /// `#RRGGBB` (opaque) or `#RRGGBBAA` as 0xRRGGBBAA.
    static func hex(_ s: String) -> UInt32? {
        guard s.hasPrefix("#"), s.count == 7 || s.count == 9 else { return nil }
        let digits = s.dropFirst()
        guard digits.allSatisfy(\.isHexDigit), digits.allSatisfy(\.isASCII), let v = UInt32(digits, radix: 16)
        else { return nil }
        return digits.count == 6 ? v << 8 | 0xFF : v
    }

    /// tokens.json's groups and their items (symbiotic-design 0.2.0).
    static let groups: [String: [String]] = [
        "surface": ["window", "panel", "raised", "inset", "separator", "selection", "focusRing"],
        "text": ["primary", "secondary", "disabled", "inverse"],
        "accent": ["accent", "onAccent"],
        "state": ["record", "play", "solo", "mute", "warning", "error", "offline"],
        "meter": ["background", "low", "mid", "high", "clip"],
        "graph": ["line", "fill", "grid", "handle", "handleSelected"],
        "control": ["gain", "eqLow", "eqLowMid", "eqHighMid", "eqHigh", "send", "pan", "dynamics", "filter"],
        "strip": ["channel", "instrument", "auxReturn", "bus", "master"],
        "brand": ["gold", "cyan", "cyanInk", "ink", "paper", "rule"],
        "spacing": ["xxs", "xs", "s", "m", "l", "xl"],
        "radius": ["none", "s", "m", "l"],
        "iconStroke": ["s16", "s24", "s32"],
        "type": ["caption", "body", "label", "title", "display"],
        "knob": ["skirt", "cap", "track", "indicator"],
        "fader": ["well", "scale", "capLine"],
        "relief": ["highlight", "shadow", "capTop", "capBottom", "panelTop", "panelBottom"],
        "swatch": ["red", "orange", "yellow", "green", "teal", "blue", "purple", "pink"],
        "keys": ["white", "black"],
        "readout": ["background", "text"],
        "band": ["light", "dark"],
    ]

    static let styleKeys: Set<String> = [
        "labels", "legendFamily", "numberFamily", "corners", "panelBands", "knobSkirt", "capStyle", "tile",
    ]

    // MARK: Writing

    /// `t` as a theme file named `name` on `base` (Duplicate as File):
    /// every token but the brand's, which no file changes.
    public static func encode(_ t: Theme, name: String, base: String) -> Data {
        func c(_ x: ThemeColor) -> Any {
            switch x.source {
            case .rgba(let v): return hexText(v)
            case .system(let n, let fb): return ["system": n.rawValue, "fallback": hexText(fb)]
            }
        }
        func p(_ x: CGFloat) -> Any { Double(x) }
        func f(_ x: TypeStyle) -> Any { ["size": Double(x.size), "weight": x.weight.rawValue] }
        let s = t.style
        let values: [String: Any] = [
            "surface": [
                "window": c(t.surface.window), "panel": c(t.surface.panel), "raised": c(t.surface.raised),
                "inset": c(t.surface.inset), "separator": c(t.surface.separator),
                "selection": c(t.surface.selection), "focusRing": c(t.surface.focusRing),
            ],
            "text": [
                "primary": c(t.text.primary), "secondary": c(t.text.secondary), "disabled": c(t.text.disabled),
                "inverse": c(t.text.inverse),
            ],
            "accent": ["accent": c(t.accent.accent), "onAccent": c(t.accent.onAccent)],
            "state": [
                "record": c(t.state.record), "play": c(t.state.play), "solo": c(t.state.solo),
                "mute": c(t.state.mute), "warning": c(t.state.warning), "error": c(t.state.error),
                "offline": c(t.state.offline),
            ],
            "meter": [
                "background": c(t.meter.background), "low": c(t.meter.low), "mid": c(t.meter.mid),
                "high": c(t.meter.high), "clip": c(t.meter.clip),
            ],
            "graph": [
                "line": c(t.graph.line), "fill": c(t.graph.fill), "grid": c(t.graph.grid),
                "handle": c(t.graph.handle), "handleSelected": c(t.graph.handleSelected),
            ],
            "control": [
                "gain": c(t.control.gain), "eqLow": c(t.control.eqLow), "eqLowMid": c(t.control.eqLowMid),
                "eqHighMid": c(t.control.eqHighMid), "eqHigh": c(t.control.eqHigh), "send": c(t.control.send),
                "pan": c(t.control.pan), "dynamics": c(t.control.dynamics), "filter": c(t.control.filter),
            ],
            "strip": [
                "channel": c(t.strip.channel), "instrument": c(t.strip.instrument),
                "auxReturn": c(t.strip.auxReturn), "bus": c(t.strip.bus), "master": c(t.strip.master),
            ],
            "spacing": [
                "xxs": p(t.spacing.xxs), "xs": p(t.spacing.xs), "s": p(t.spacing.s), "m": p(t.spacing.m),
                "l": p(t.spacing.l), "xl": p(t.spacing.xl),
            ],
            "radius": ["none": p(t.radius.none), "s": p(t.radius.s), "m": p(t.radius.m), "l": p(t.radius.l)],
            "iconStroke": ["s16": p(t.iconStroke.s16), "s24": p(t.iconStroke.s24), "s32": p(t.iconStroke.s32)],
            "type": [
                "caption": f(t.type.caption), "body": f(t.type.body), "label": f(t.type.label),
                "title": f(t.type.title), "display": f(t.type.display),
            ],
            "knob": [
                "skirt": c(t.knob.skirt), "cap": c(t.knob.cap), "track": c(t.knob.track),
                "indicator": c(t.knob.indicator),
            ],
            "fader": ["well": c(t.fader.well), "scale": c(t.fader.scale), "capLine": c(t.fader.capLine)],
            "relief": [
                "highlight": c(t.relief.highlight), "shadow": c(t.relief.shadow), "capTop": c(t.relief.capTop),
                "capBottom": c(t.relief.capBottom), "panelTop": c(t.relief.panelTop),
                "panelBottom": c(t.relief.panelBottom),
            ],
            "swatch": [
                "red": c(t.swatch.red), "orange": c(t.swatch.orange), "yellow": c(t.swatch.yellow),
                "green": c(t.swatch.green), "teal": c(t.swatch.teal), "blue": c(t.swatch.blue),
                "purple": c(t.swatch.purple), "pink": c(t.swatch.pink),
            ],
            "keys": ["white": c(t.keys.white), "black": c(t.keys.black)],
            "readout": ["background": c(t.readout.background), "text": c(t.readout.text)],
            "band": ["light": c(t.band.light), "dark": c(t.band.dark)],
        ]
        let top: [String: Any] = [
            "name": name, "base": base, "appearance": t.appearance.rawValue, "knobFinish": t.knobFinish.rawValue,
            "style": [
                "labels": s.labels.rawValue, "legendFamily": s.legendFamily.rawValue,
                "numberFamily": s.numberFamily.rawValue, "corners": s.corners.rawValue,
                "panelBands": s.panelBands.rawValue, "knobSkirt": s.knobSkirt.rawValue,
                "capStyle": s.capStyle.rawValue, "tile": s.tile.rawValue,
            ],
            "values": values,
        ]
        return (try? JSONSerialization.data(withJSONObject: top, options: [.prettyPrinted, .sortedKeys]))
            ?? Data()
    }

    /// 0xRRGGBBAA as `#RRGGBB` when opaque, else `#RRGGBBAA`.
    static func hexText(_ v: UInt32) -> String {
        v & 0xFF == 0xFF ? String(format: "#%06X", v >> 8) : String(format: "#%08X", v)
    }
}

/// The user themes' folder (R-UT), read again whenever it or one of its
/// files changes. The app's is `~/Library/Application Support/VizDAW/Themes`;
/// tests pass a temporary folder and a fake watcher.
@MainActor @Observable
public final class UserThemes {
    public let folder: URL
    public private(set) var themes: [UserTheme] = []
    public private(set) var problems: [UserThemeProblem] = []
    /// What the last Duplicate as File did, for Settings to show.
    public var lastDuplicate: String?
    /// Called after every read (the preferences follow the theme in use).
    @ObservationIgnored public var onReload: (() -> Void)?
    @ObservationIgnored private let watching: (any FileWatching)?
    @ObservationIgnored private var tokens: [AnyObject] = []

    public init(folder: URL, watching: (any FileWatching)?) {
        self.folder = folder
        self.watching = watching
    }

    /// Makes the folder if it is missing, reads it, and watches it.
    public func start() {
        try? FileManager.default.createDirectory(at: folder, withIntermediateDirectories: true)
        reload()
    }

    /// Reads the folder again, and watches it and each theme file (an edit
    /// in place changes the file, not the folder).
    public func reload() {
        var r = UserThemeFile.load(folder: folder)
        if let watching {
            // Arm, then read once more (B4 rereview N4): a save that landed
            // between the read and the arm is seen now, and any later one
            // fires a watch. When that read finds other files, they are
            // armed in turn (at most twice: a folder still churning fires
            // its folder watch).
            for _ in 0..<2 {
                // The folder and the files read, no more (B4 review I3); the
                // old watches go before the new ones open.
                tokens = []
                let paths = [folder.path] + r.read.map { folder.appendingPathComponent($0).path }
                tokens = paths.compactMap { watching.watch($0) { [weak self] in self?.reload() } }
                let again = UserThemeFile.load(folder: folder)
                let same = again.read == r.read
                r = again
                if same { break }
            }
        }
        if r.themes != themes { themes = r.themes }
        if r.problems != problems { problems = r.problems }
        onReload?()
    }

    /// Writes `t` as a new file, `‹name› Copy.json` (then ` 2`, ` 3` …),
    /// never over an existing file; the name in the file matches.
    public func duplicate(_ t: Theme, base: String) throws -> URL {
        let stem = Self.fileSafe(String(String.UnicodeScalarView(t.name.unicodeScalars.prefix(UserThemeFile.maxName - 9)))) + " Copy"
        let taken = Set((themes.map(\.theme.name) + Theme.all.map(\.name)).map { $0.lowercased() })
        let ids = Set(themes.map(\.theme.id))
        try FileManager.default.createDirectory(at: folder, withIntermediateDirectories: true)
        for n in 1...999 {
            let name = n == 1 ? stem : "\(stem) \(n)"
            let file = name + ".json"
            guard !taken.contains(name.lowercased()), let id = UserThemeFile.id(forFile: file), !ids.contains(id)
            else { continue }
            let url = folder.appendingPathComponent(file)
            guard !FileManager.default.fileExists(atPath: url.path) else { continue }
            do {
                try UserThemeFile.encode(t, name: name, base: base).write(to: url, options: [.withoutOverwriting])
            } catch CocoaError.fileWriteFileExists {
                continue
            } catch {
                throw UserThemeError(message: "\(file) could not be written: \(error.localizedDescription)")
            }
            lastDuplicate = "Wrote \(file)."
            return url
        }
        throw UserThemeError(message: "The themes folder has too many copies of \(t.name).")
    }

    /// `s` without the characters a file name cannot hold.
    static func fileSafe(_ s: String) -> String {
        let bad = CharacterSet(charactersIn: "/:\\").union(.controlCharacters)
        let out = String(String.UnicodeScalarView(s.unicodeScalars.filter { !bad.contains($0) }))
        let trimmed = out.trimmingCharacters(in: .whitespaces)
        return trimmed.hasPrefix(".") ? String(trimmed.drop { $0 == "." }) : trimmed
    }
}
