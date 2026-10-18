import Foundation
import Observation
import VizDAWCore

/// The app's key bindings (editors spec §4f, plan 4f Rulings 3–6): every
/// registry command with its keys, kept in the preferences, saved to and
/// loaded from `.vizkeys` files, reset; menus show the keys from here.
/// The bindings themselves (what a key runs, clashes, the file) are Rust's
/// `Keymap`.
@Observable
@MainActor
public final class KeyBindings {
    /// The preferences key of the bindings document.
    public static let key = "VizDAW.keys"
    /// The preferences key of "the number row is for locators".
    public static let reserveKey = "VizDAW.reserveNumberRow"

    @ObservationIgnored public let keymap: Keymap
    @ObservationIgnored private let defaults: UserDefaults
    /// Every command with its keys, in registry order.
    public private(set) var rows: [BindingRow]
    /// The number row jumps to locators even during step entry (whose
    /// note values then come from the keypad; plan 4f Ruling 4).
    public var reserveNumberRow: Bool {
        didSet { defaults.set(reserveNumberRow, forKey: Self.reserveKey) }
    }
    /// What the last load skipped or why it failed, for the Keys page.
    public var message: String?
    /// A recorded key another command has, waiting for Replace.
    public private(set) var pending: PendingKey?

    /// A key the Keys page recorded that another command has.
    public struct PendingKey: Equatable, Sendable {
        public let id: String
        public let key: String
    }

    public init(defaults: UserDefaults = .standard) {
        self.defaults = defaults
        keymap = Keymap()
        if let json = defaults.string(forKey: Self.key) {
            // A damaged preference leaves the defaults.
            _ = try? keymap.loadJson(json: json)
        }
        rows = keymap.rows()
        reserveNumberRow = defaults.bool(forKey: Self.reserveKey)
    }

    /// Command `id`'s row.
    public func row(_ id: String) -> BindingRow? { rows.first { $0.id == id } }

    /// The command a key press runs.
    public func command(keyCode: UInt16, modifiers: ModifiersRow, stepEntry: Bool) -> String? {
        keymap.lookup(keyCode: keyCode, modifiers: modifiers, stepEntry: stepEntry, reserveNumbers: reserveNumberRow)
    }

    /// Gives command `id` exactly `keys` (chord texts); a key another
    /// command has is refused naming it, unless `replace` takes it.
    public func bind(_ id: String, keys: [String], replace: Bool = false) throws {
        try keymap.bind(id: id, keys: keys, replace: replace)
        changed()
    }

    /// The Keys page recorded `key` for `id` (plan 4f Ruling 17): it
    /// becomes the command's only key; one another command has waits in
    /// `pending` for `confirmReplace`.
    public func record(_ id: String, key: String) {
        do {
            try bind(id, keys: [key])
            pending = nil
            message = nil
        } catch {
            pending = PendingKey(id: id, key: key)
            message = AlertItem(error, title: "").message + ". Replace it?"
        }
    }

    /// Takes the pending key from the command that has it.
    public func confirmReplace() {
        guard let p = pending else { return }
        pending = nil
        message = nil
        try? bind(p.id, keys: [p.key], replace: true)
    }

    public func cancelReplace() {
        pending = nil
        message = nil
    }

    public func reset(_ id: String) {
        keymap.resetOne(id: id)
        changed()
    }

    public func resetAll() {
        keymap.reset()
        changed()
    }

    /// Writes the bindings to `path`.
    public func save(to path: String) throws { try keymap.saveFile(path: path) }

    /// Replaces the bindings with the file's; `message` lists what it
    /// skipped. A file that is no bindings file changes nothing.
    public func load(from path: String) throws {
        do {
            let skipped = try keymap.loadFile(path: path)
            message = skipped.isEmpty ? nil : "Skipped: " + skipped.joined(separator: "; ")
        } catch {
            message = AlertItem(error, title: "").message
            throw error
        }
        changed()
    }

    /// The title of the command a keyboard slot would clash with (parent
    /// spec §5: such a slot is refused).
    public func slotClash(_ slot: KeySlotRow) -> String? {
        keymap.slotClash(key: slot.key, modifiers: slot.modifiers).map { row($0)?.title ?? $0 }
    }

    private func changed() {
        rows = keymap.rows()
        defaults.set(keymap.toJson(), forKey: Self.key)
    }
}

/// What a key press does (plan 4f Ruling 5).
public enum KeyRoute: Equatable, Sendable {
    /// A text field has the keyboard: the key types.
    case typing
    /// Runs this command on the window's document.
    case command(String)
    /// A sequence's keyboard slot.
    case slot(UInt64)
    /// Not VizDAW's: AppKit's menus and the views have it.
    case pass
}

/// Decides what a key press does, the same for the keyboard and the
/// control channel's `ui.key` (plan 4f Ruling 5).
@MainActor
public enum KeyRouter {
    /// No ⌘ and no ⌃: a key that types in a text field.
    public static func plain(_ m: ModifiersRow) -> Bool { !m.command && !m.control }

    /// The route of a key press in a window of a document. `typing`: a
    /// text field has the keyboard. `menus`: AppKit's menus will see the
    /// key next (the keyboard), so a menu item's ⌘ or ⌃ key is left to
    /// its menu, which may open its dialog; the channel has no menus.
    /// Controller ruling F3: only the command's first key, the one the
    /// menu item shows, is left to the menu; any other chord bound to it
    /// runs here, because AppKit's menu does not know it.
    public static func route(
        keyCode: UInt16, modifiers m: ModifiersRow, typing: Bool, stepEntry: Bool, menus: Bool,
        keys: KeyBindings, sequences: [SeqRow]
    ) -> KeyRoute {
        if typing && plain(m) { return .typing }
        if let id = keys.command(keyCode: keyCode, modifiers: m, stepEntry: stepEntry) {
            if menus, !plain(m), let row = keys.row(id), row.menuItem,
                let first = row.keys.first, chord(first, is: keyCode, m)
            {
                return .pass
            }
            return .command(id)
        }
        let bits = UInt8(m.shift ? 1 : 0) | (m.control ? 2 : 0) | (m.option ? 4 : 0) | (m.command ? 8 : 0)
        let slot = sequences.first { s in
            guard let k = s.keyboardSlot else { return false }
            return k.modifiers & 15 == bits && keyCodeFor(name: k.key) == keyCode
        }
        return slot.map { .slot($0.id) } ?? .pass
    }

    /// Whether chord text `text` (`⇧⌘S`) is the press of `keyCode` with `m`.
    private static func chord(_ text: String, is keyCode: UInt16, _ m: ModifiersRow) -> Bool {
        let glyphs: Set<Character> = ["⌃", "⌥", "⇧", "⌘"]
        let name = String(text.drop { glyphs.contains($0) })
        let mods = text.prefix { glyphs.contains($0) }
        return keyCodeFor(name: name) == keyCode
            && mods.contains("⇧") == m.shift && mods.contains("⌃") == m.control
            && mods.contains("⌥") == m.option && mods.contains("⌘") == m.command
    }
}

/// What the key press comes to after its command ran.
public enum KeyAfter: Equatable, Sendable {
    /// Done: the key is used up.
    case handled
    /// The command was unavailable (or needs its dialog): the key goes on
    /// to the menus and the views, as with a disabled menu item.
    case pass
    /// The command made a document; its window should open.
    case opened(UUID)
}

@MainActor
public enum KeyDispatch {
    /// The key's fate after `outcome`; a refusal shows in `doc`'s status
    /// line.
    public static func after(_ outcome: ControlOutcome, doc: DocumentModel?) -> KeyAfter {
        switch outcome {
        case .done: return .handled
        case .opened(_, let d): return .opened(d)
        case .failed(let f):
            switch f.code {
            case .unavailable, .dialogRequired, .unknownId, .noDocument: return .pass
            default:
                doc?.editor.status = f.message
                return .handled
            }
        }
    }
}

/// Where a key press VizDAW did not use goes next (controller rulings F1,
/// F2).
public enum KeyPass: Equatable, Sendable {
    /// AppKit's main menu, then the views: a ⌘ or ⌃ key, a key bound to
    /// nothing.
    case menus
    /// The window alone (its own key equivalents — a sheet's default and
    /// cancel buttons — then its first responder), past the main menu: a
    /// text field's plain keys, and a plain key whose command was
    /// unavailable, which the menu would otherwise match to a different
    /// command's plain-key item (an unset locator's `1` to Step › Whole
    /// Note).
    case window
}

extension KeyDispatch {
    /// Where a key press with `route` goes when it was not used up.
    public static func passed(_ route: KeyRoute, modifiers m: ModifiersRow) -> KeyPass {
        switch route {
        case .typing: return .window
        case .command: return KeyRouter.plain(m) ? .window : .menus
        case .slot, .pass: return .menus
        }
    }
}
