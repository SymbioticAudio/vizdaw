import CoreGraphics
import Foundation
import Observation

/// One built-in item of the transport bar (groups-vcas-transport Ruling
/// 13). The raw value is Swift's; the preference and `ui.set_transport_bar`
/// spell it in snake_case (`token`).
public enum TransportItemID: String, Codable, CaseIterable, Sendable {
    case cluster, loop, click, replaceOverdub, recordOptions, countIn, summary, bar, time, tempo, signature,
        positionField, editFields, punchFields, dspLoad, device, faults, mainView, panes, space, separator

    /// Its name in the palette and in ».
    public var title: String {
        switch self {
        case .cluster: "Transport Controls"
        case .loop: "Loop"
        case .click: "Click"
        case .replaceOverdub: "Replace/Overdub"
        case .recordOptions: "Record Options"
        case .countIn: "Count-in"
        case .summary: "Step Entry"
        case .bar: "Position in Bars"
        case .time: "Position in Time"
        case .tempo: "Tempo"
        case .signature: "Time Signature"
        case .positionField: "Position Field"
        case .editFields: "Edit In and Out"
        case .punchFields: "Punch In and Out"
        case .dspLoad: "DSP Load"
        case .device: "Audio Device"
        case .faults: "Faults"
        case .mainView: "Main View"
        case .panes: "Pane Toggles"
        case .space: "Flexible Space"
        case .separator: "Separator"
        }
    }

    /// The readouts that come small or large.
    public var sizes: Bool { self == .bar || self == .time || self == .tempo }

    /// Items only the document window shows (Ruling 13): the mixer window
    /// neither shows them nor lists them in ».
    public var documentOnly: Bool { self == .faults || self == .mainView || self == .panes }

    /// Items the bar may hold more than once.
    public var repeats: Bool { self == .space || self == .separator }

    /// The snake_case name the preference and the channel use.
    public var token: String {
        var out = ""
        for c in rawValue {
            if c.isUppercase {
                out += "_" + c.lowercased()
            } else {
                out.append(c)
            }
        }
        return out
    }

    /// The item a snake_case name names.
    public init?(token: String) {
        guard let id = Self.allCases.first(where: { $0.token == token }) else { return nil }
        self = id
    }
}

/// A readout's size: large is the theme's title digits (today's bar).
public enum ReadoutSize: String, Codable, Sendable {
    case small, large
}

/// One item of the bar. Task 12 adds command buttons (`command:<id>`).
public enum TransportItem: Codable, Equatable, Sendable {
    /// A built-in item; the size is given only for an item that `sizes`
    /// (nil: large).
    case builtIn(TransportItemID, ReadoutSize?)

    public var id: TransportItemID {
        switch self {
        case .builtIn(let id, _): id
        }
    }

    /// The size it shows at: large unless given small.
    public var size: ReadoutSize {
        switch self {
        case .builtIn(_, let s): s ?? .large
        }
    }

    /// `<id>[:large|:small]`, as the preference and the channel spell it.
    public var token: String {
        switch self {
        case .builtIn(let id, let s): s.map { "\(id.token):\($0.rawValue)" } ?? id.token
        }
    }

    /// The longest piece of hostile text an error quotes.
    static let quoted = 40

    /// The item `token` names, or why not.
    public static func parse(token raw: String) -> Result<TransportItem, TransportLayoutError> {
        let token = raw.trimmingCharacters(in: .whitespaces)
        let parts = token.split(separator: ":", maxSplits: 1, omittingEmptySubsequences: false).map(String.init)
        let name = parts[0]
        guard !name.isEmpty else { return .failure(.init("an empty item")) }
        guard let id = TransportItemID(token: name) else {
            return .failure(.init("unknown item \(name.prefix(quoted))"))
        }
        guard parts.count == 2 else { return .success(.builtIn(id, nil)) }
        guard id.sizes else { return .failure(.init("\(id.token) has no size")) }
        guard let size = ReadoutSize(rawValue: parts[1]) else {
            return .failure(.init("\(id.token) has no size \(parts[1].prefix(quoted))"))
        }
        return .success(.builtIn(id, size))
    }

    public func encode(to encoder: Encoder) throws {
        var c = encoder.singleValueContainer()
        try c.encode(token)
    }

    public init(from decoder: Decoder) throws {
        let c = try decoder.singleValueContainer()
        switch Self.parse(token: try c.decode(String.self)) {
        case .success(let item): self = item
        case .failure(let e): throw DecodingError.dataCorruptedError(in: c, debugDescription: e.message)
        }
    }
}

/// Why a transport layout was refused: the first bad token, named.
public struct TransportLayoutError: Error, Equatable, Sendable {
    public let message: String

    public init(_ message: String) { self.message = message }
}

/// The bar's items in order (Ruling 17): stored as
/// `{"version": 1, "items": ["cluster", "loop", "bar:large", …]}`.
public struct TransportLayout: Codable, Equatable, Sendable {
    public static let version = 1
    /// The most items a bar holds.
    public static let maxItems = 64

    public var items: [TransportItem]

    public init(items: [TransportItem]) { self.items = items }

    /// Ruling 13's default: today's bar, with Loop just after the cluster
    /// and Click after Loop.
    public static let standard = TransportLayout(
        items: ([
            .cluster, .loop, .click, .replaceOverdub, .recordOptions, .summary, .bar, .time, .tempo, .signature,
            .positionField, .editFields, .punchFields, .space, .device, .faults, .mainView, .panes,
        ] as [TransportItemID]).map { .builtIn($0, nil) })

    /// `ui.set_transport_bar`'s text: tokens split by ",", each
    /// `<id in snake_case>[:large|:small]`; exactly one `cluster`, at most
    /// 64 tokens; the error names the first bad token.
    public static func parse(_ text: String) -> Result<TransportLayout, TransportLayoutError> {
        let tokens = text.split(separator: ",", omittingEmptySubsequences: false).map(String.init)
        guard tokens.count <= maxItems else { return .failure(.init("at most \(maxItems) items")) }
        var items: [TransportItem] = []
        for t in tokens {
            switch TransportItem.parse(token: t) {
            case .success(let item): items.append(item)
            case .failure(let e): return .failure(e)
            }
        }
        return checked(items)
    }

    /// `items` as a layout, or why it is not one: exactly one cluster, at
    /// most 64 items, and no built-in item twice but spaces and separators.
    public static func checked(_ items: [TransportItem]) -> Result<TransportLayout, TransportLayoutError> {
        guard items.count <= maxItems else { return .failure(.init("at most \(maxItems) items")) }
        guard items.filter({ $0.id == .cluster }).count == 1 else {
            return .failure(.init("the bar needs cluster once"))
        }
        var seen: Set<TransportItemID> = []
        for item in items where !item.id.repeats {
            guard seen.insert(item.id).inserted else { return .failure(.init("\(item.id.token) is in the bar twice")) }
        }
        return .success(TransportLayout(items: items))
    }

    /// Its text for `ui.set_transport_bar`.
    public var text: String { items.map(\.token).joined(separator: ",") }

    /// The items a window shows: the mixer window skips the document-only
    /// ones (Ruling 13).
    public func items(documentWindow: Bool) -> [TransportItem] {
        documentWindow ? items : items.filter { !$0.id.documentOnly }
    }

    enum CodingKeys: String, CodingKey { case version, items }

    public func encode(to encoder: Encoder) throws {
        var c = encoder.container(keyedBy: CodingKeys.self)
        try c.encode(Self.version, forKey: .version)
        try c.encode(items, forKey: .items)
    }

    /// A version this build does not know, a bad item or a broken rule
    /// throws (Ruling 17: the store then shows the default).
    public init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        let v = try c.decode(Int.self, forKey: .version)
        guard v == Self.version else {
            throw DecodingError.dataCorruptedError(
                forKey: .version, in: c, debugDescription: "transport bar version \(v) is not \(Self.version)")
        }
        switch Self.checked(try c.decode([TransportItem].self, forKey: .items)) {
        case .success(let l): self = l
        case .failure(let e):
            throw DecodingError.dataCorruptedError(forKey: .items, in: c, debugDescription: e.message)
        }
    }
}

/// The app's transport bar preference (Ruling 17). A damaged or newer
/// value gives the default and is left as it is until `set` or `reset`.
@Observable @MainActor public final class TransportLayoutStore {
    public static let key = "VizDAW.transportBar"

    @ObservationIgnored private let defaults: UserDefaults
    public private(set) var layout: TransportLayout

    public init(defaults: UserDefaults) {
        self.defaults = defaults
        layout =
            defaults.string(forKey: Self.key).flatMap {
                try? JSONDecoder().decode(TransportLayout.self, from: Data($0.utf8))
            } ?? .standard
    }

    /// Shows `l` and writes the preference.
    public func set(_ l: TransportLayout) {
        layout = l
        if let data = try? JSONEncoder().encode(l), let s = String(data: data, encoding: .utf8) {
            defaults.set(s, forKey: Self.key)
        }
    }

    /// The default bar, written.
    public func reset() { set(.standard) }
}

/// How the one row fits (Ruling 14): which pieces show and which fold
/// into ».
public enum TransportRowFit {
    public enum Kind: Sendable { case fixed, content, space, separator }

    public struct Piece: Sendable {
        public var width: CGFloat
        public var kind: Kind

        public init(width: CGFloat, kind: Kind) {
            self.width = width
            self.kind = kind
        }
    }

    /// Ruling 14: content folds last first until the shown pieces, their
    /// spacing and (when anything folds) the chevron fit; fixed never
    /// folds; spaces and separators are never in `folded`. NaN or ≤ 0
    /// width: everything but fixed folds; +inf: nothing folds. A separator
    /// next to an edge, the » or another separator is not shown.
    public static func fit(_ pieces: [Piece], width: CGFloat, spacing: CGFloat, chevron: CGFloat)
        -> (shown: [Int], folded: [Int])
    {
        let content = pieces.indices.filter { pieces[$0].kind == .content }
        let fixed = pieces.indices.filter { pieces[$0].kind == .fixed }
        if width.isNaN || width <= 0 { return (fixed, content) }
        if width == .infinity { return (tidied(Array(pieces.indices), pieces), []) }
        let gap = clean(spacing)
        for k in 0...content.count {
            let folded = Set(content.suffix(k))
            let shown = tidied(pieces.indices.filter { !folded.contains($0) }, pieces)
            let need = used(shown, pieces, spacing: gap) + (k > 0 ? gap + clean(chevron) : 0)
            if need <= width { return (shown, content.suffix(k).map { $0 }) }
        }
        return (fixed, content)
    }

    /// The width `shown` takes with its spacing.
    public static func used(_ shown: [Int], _ pieces: [Piece], spacing: CGFloat) -> CGFloat {
        shown.map { clean(pieces[$0].width) }.reduce(0, +) + clean(spacing) * CGFloat(max(shown.count - 1, 0))
    }

    /// A width that is not finite and positive counts as 0.
    public static func clean(_ w: CGFloat) -> CGFloat { w.isFinite && w > 0 ? w : 0 }

    /// `shown` without the separators next to an edge, the » or another
    /// separator.
    static func tidied(_ shown: [Int], _ pieces: [Piece]) -> [Int] {
        shown.enumerated().compactMap { n, i in
            guard pieces[i].kind == .separator else { return i }
            let before = n > 0 ? pieces[shown[n - 1]].kind : nil
            let after = n + 1 < shown.count ? pieces[shown[n + 1]].kind : nil
            guard let before, let after, before != .separator, after != .separator else { return nil }
            return i
        }
    }
}

/// What a folded item is in » (Ruling 14), and what an item's own menu
/// holds: a button is an action (checked for a toggle), a readout its
/// text (disabled), a field, Record Options or the main-view tabs a
/// popover of the item's own view, and Click or the pane toggles a
/// submenu.
public enum TransportMenuEntry {
    case action(title: String, isOn: Bool?, enabled: Bool, run: @MainActor () -> Void)
    case text(String)
    case popover(title: String)
    case submenu(title: String, isOn: Bool?, enabled: Bool, entries: [TransportMenuEntry])

    /// The menu item's title.
    public var title: String {
        switch self {
        case .action(let t, _, _, _), .popover(let t), .submenu(let t, _, _, _): t
        case .text(let t): t
        }
    }

    /// Its check mark: nil when it is not a toggle.
    public var isOn: Bool? {
        switch self {
        case .action(_, let on, _, _), .submenu(_, let on, _, _): on
        case .text, .popover: nil
        }
    }

    /// Whether it can be chosen.
    public var enabled: Bool {
        switch self {
        case .action(_, _, let e, _), .submenu(_, _, let e, _): e
        case .text: false
        case .popover: true
        }
    }

    /// The entries below it.
    public var entries: [TransportMenuEntry] {
        if case .submenu(_, _, _, let e) = self { return e }
        return []
    }

    /// The entry titled `title` below it.
    public func entry(_ title: String) -> TransportMenuEntry? { entries.first { $0.title == title } }

    /// Runs it, as choosing it does; false when it is not an enabled action.
    @MainActor @discardableResult
    public func choose() -> Bool {
        guard case .action(_, _, true, let run) = self else { return false }
        run()
        return true
    }
}
