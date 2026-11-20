import Foundation
import Observation
import VizDAWCore

/// Customize Transport Bar's model (groups-vcas-transport Task 12,
/// Ruling 15): a draft of the bar that the sheet's palette and bar edit,
/// written to the store only by Done (`apply`). Every edit keeps the
/// draft a layout the store takes: the cluster stays, a built-in item is
/// on the bar once, at most 64 items.
@Observable @MainActor public final class TransportCustomizeModel {
    /// The command a new command button from the palette runs until one
    /// is chosen.
    public static let newCommand = "window.mixer"

    @ObservationIgnored private let store: TransportLayoutStore
    public private(set) var draft: TransportLayout
    /// The bar item the sheet's inspector shows (its size, its command).
    public private(set) var selected: Int?
    /// The command picker's search.
    public var query = ""
    /// The selected command button's label as typed; `commitLabel` takes it.
    public var labelDraft = ""

    public init(store: TransportLayoutStore) {
        self.store = store
        draft = store.layout
    }

    /// Shows bar item `index` in the inspector (nil: none).
    public func select(_ index: Int?) {
        selected = index.flatMap { draft.items.indices.contains($0) ? $0 : nil }
        query = ""
        if let n = selected, case .command(_, let l) = draft.items[n] { labelDraft = l ?? "" } else { labelDraft = "" }
    }

    /// The selected command button takes `labelDraft`; false when it is
    /// too long (or nothing is selected).
    @discardableResult
    public func commitLabel() -> Bool {
        guard let n = selected, case .command(let id, _) = draft.items[n] else { return false }
        return setCommand(at: n, id: id, label: labelDraft)
    }

    /// The selected command button runs `id`.
    public func chooseCommand(_ id: String) {
        guard let n = selected, case .command(_, let l) = draft.items[n] else { return }
        setCommand(at: n, id: id, label: l)
    }

    /// Every built-in item not in the draft, in their order, then a space,
    /// a separator and a command button (these three are always offered).
    public var palette: [TransportItem] {
        let on = Set(draft.items.map(\.id))
        let builtIns = TransportItemID.allCases.filter { !$0.repeats && !on.contains($0) }.map {
            TransportItem.builtIn($0, nil)
        }
        return builtIns + [.builtIn(.space, nil), .builtIn(.separator, nil), .command(id: Self.newCommand, label: nil)]
    }

    /// The draft with `items`, when it is a layout; false (and the draft
    /// unchanged) when not.
    private func take(_ items: [TransportItem]) -> Bool {
        guard case .success(let l) = TransportLayout.checked(items) else { return false }
        if l.items.count != draft.items.count { select(nil) }
        draft = l
        return true
    }

    /// Puts `i` at `index` (clamped: past the end appends); false when the
    /// bar would break a rule (a built-in item twice, more than 64).
    @discardableResult
    public func add(_ i: TransportItem, at index: Int) -> Bool {
        var items = draft.items
        items.insert(i, at: min(max(index, 0), items.count))
        return take(items)
    }

    /// Takes the item at `index` off the bar; false for the cluster or an
    /// index the bar has not.
    @discardableResult
    public func remove(at index: Int) -> Bool {
        guard draft.items.indices.contains(index), draft.items[index].id != .cluster else { return false }
        var items = draft.items
        items.remove(at: index)
        return take(items)
    }

    /// Moves the item at `from` so it ends at `to` (clamped).
    public func move(from: Int, to: Int) {
        guard draft.items.indices.contains(from) else { return }
        var items = draft.items
        let item = items.remove(at: from)
        let at = min(max(to, 0), items.count)
        items.insert(item, at: at)
        let follows = selected == from
        if take(items) { selected = follows ? at : nil }
    }

    /// Bar, Time and Tempo come small or large; other items ignore it.
    public func setSize(at index: Int, _ s: ReadoutSize) {
        guard draft.items.indices.contains(index), case .builtIn(let id, _) = draft.items[index], id.sizes else {
            return
        }
        var items = draft.items
        items[index] = .builtIn(id, s)
        _ = take(items)
    }

    /// The command button at `index` runs `id` with `label` (trimmed,
    /// 1–32 characters; blank: nil, the command's title). False, and the
    /// draft unchanged, for a longer label, a malformed id or an item that
    /// is not a command button.
    @discardableResult
    public func setCommand(at index: Int, id: String, label: String?) -> Bool {
        guard draft.items.indices.contains(index), draft.items[index].id == .command,
            TransportItem.isCommandID(id), case .success(let l) = TransportItem.label(label)
        else { return false }
        var items = draft.items
        items[index] = .command(id: id, label: l)
        return take(items)
    }

    /// The default bar in the draft (Done writes it).
    public func resetToDefault() {
        select(nil)
        draft = .standard
    }

    /// Done: the draft is the bar; the sheet closes.
    public func apply() {
        if selected != nil { commitLabel() }
        store.set(draft)
        store.endCustomizing()
    }

    /// Cancel: the bar stays as it was; the sheet closes.
    public func cancel() { store.endCustomizing() }
}

/// The Command Button's picker (Task 12): the registry's commands that
/// match a search.
public enum CommandPicker {
    /// `all` (registry order) whose title, menu or id contains `query`,
    /// ignoring case; a blank query gives every row.
    public static func rows(_ all: [BindingRow], query: String) -> [BindingRow] {
        let q = query.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !q.isEmpty else { return all }
        return all.filter {
            $0.title.localizedCaseInsensitiveContains(q) || $0.menu.localizedCaseInsensitiveContains(q)
                || $0.id.localizedCaseInsensitiveContains(q)
        }
    }
}

/// What the sheet shows for each item (pre-flight F9: the app target's
/// view stays thin, and tests reach this): its name, its accessibility
/// label and the actions VoiceOver offers.
@MainActor
public enum TransportCustomizeEntries {
    /// One item in the sheet.
    public struct Entry: Equatable, Sendable {
        /// What the item shows.
        public let title: String
        /// Its accessibility label.
        public let accessibilityLabel: String
        /// Its accessibility actions' names.
        public let actions: [String]
    }

    public static let addAction = "Add to Bar"
    public static let removeAction = "Remove"
    public static let leftAction = "Move Left"
    public static let rightAction = "Move Right"

    /// The name of `item` in the palette and the bar: a command button
    /// shows its label, else its command's title, else its id.
    public static func title(_ item: TransportItem, row: BindingRow? = nil) -> String {
        switch item {
        case .builtIn(let id, let s):
            id.sizes && s == .small ? "\(id.title) (Small)" : id.title
        case .command(let cmd, let label):
            label ?? row?.title ?? cmd
        }
    }

    /// The palette's entries.
    public static func palette(_ items: [TransportItem], row: (String) -> BindingRow? = { _ in nil }) -> [Entry] {
        items.map { i in
            let t = i.id == .command ? TransportItemID.command.title : title(i)
            return Entry(title: t, accessibilityLabel: "\(t), not on the bar", actions: [addAction])
        }
    }

    /// The bar's entries: every item can move; all but the cluster can
    /// be removed.
    public static func bar(_ items: [TransportItem], row: (String) -> BindingRow? = { _ in nil }) -> [Entry] {
        items.enumerated().map { n, i in
            let t: String
            if case .command(let cmd, _) = i {
                t = title(i, row: row(cmd))
            } else {
                t = title(i)
            }
            var actions: [String] = []
            if i.id != .cluster { actions.append(removeAction) }
            if n > 0 { actions.append(leftAction) }
            if n + 1 < items.count { actions.append(rightAction) }
            let label = i.id == .command ? "\(TransportItemID.command.title): \(t)" : t
            return Entry(title: t, accessibilityLabel: label, actions: actions)
        }
    }

    /// The drag token of the bar's item at `index` ("bar:3") or of the
    /// palette's ("palette:3"); `parse` reads it back.
    public static func token(bar index: Int) -> String { "bar:\(index)" }
    public static func token(palette index: Int) -> String { "palette:\(index)" }

    public enum Source: Equatable, Sendable {
        case bar(Int)
        case palette(Int)
    }

    /// A drag token's source; nil for any other text (a drop from
    /// elsewhere).
    public static func source(_ token: String) -> Source? {
        let parts = token.split(separator: ":", maxSplits: 1).map(String.init)
        guard parts.count == 2, parts[1].count <= 3, let n = Int(parts[1]), n >= 0 else { return nil }
        switch parts[0] {
        case "bar": return .bar(n)
        case "palette": return .palette(n)
        default: return nil
        }
    }

    /// A drop of `token` on the bar at `index`: a palette item is added, a
    /// bar item moved; false when nothing changed.
    @discardableResult
    public static func drop(_ token: String, onBarAt index: Int, _ m: TransportCustomizeModel) -> Bool {
        switch source(token) {
        case .palette(let n):
            let p = m.palette
            guard p.indices.contains(n) else { return false }
            return m.add(p[n], at: index)
        case .bar(let n):
            guard m.draft.items.indices.contains(n) else { return false }
            let before = m.draft
            m.move(from: n, to: n < index ? index - 1 : index)
            return m.draft != before
        case nil:
            return false
        }
    }

    /// A drop of `token` on the palette: a bar item is removed (never the
    /// cluster).
    @discardableResult
    public static func dropOnPalette(_ token: String, _ m: TransportCustomizeModel) -> Bool {
        guard case .bar(let n) = source(token) else { return false }
        return m.remove(at: n)
    }
}
