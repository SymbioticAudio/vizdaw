import Foundation
import Observation
import SymbioticTokens

/// Settings › Appearance (plan 7b Ruling 12): the theme and the channel
/// colour bars. A preference of this Mac, never saved in a project.
/// Decoding is tolerant per field: a missing or damaged field keeps its
/// default; an ID that names no theme is kept here and resolved by
/// `AppearancePreferences`.
public struct AppearanceSettings: Codable, Equatable, Sendable {
    public var theme = "classic-light"
    public var colourBars = true

    public init() {}

    enum CodingKeys: String, CodingKey {
        case theme, colourBars
    }

    public init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        let d = AppearanceSettings()
        theme = c.field(.theme, d.theme)
        colourBars = c.field(.colourBars, d.colourBars)
    }
}

/// The app's theme (plan 7b Ruling 12; user themes by controller ruling
/// R-UT). Every window follows `theme`; `onChange` tells the AppKit
/// windows. Nothing is written until `choose` or `setColourBars`: a saved
/// ID that names no theme (a newer VizDAW's, a user theme whose file is
/// gone) gives Classic Light and is kept for when it reads again.
@MainActor @Observable
public final class AppearancePreferences {
    public static let defaultID = "classic-light"
    public static let key = "appearance"

    @ObservationIgnored private let defaults: UserDefaults
    /// The user themes' folder, if the app reads one.
    @ObservationIgnored public let userThemes: UserThemes?
    /// The settings as saved (the theme ID as chosen, even when it names
    /// no theme now).
    @ObservationIgnored private var settings: AppearanceSettings

    public private(set) var theme: Theme
    public private(set) var colourBars: Bool
    /// Called with the new theme whenever `theme` changes. Not
    /// `@MainActor`-typed, so a test closure may mutate a captured `var`
    /// (as `KeyRecorderView.onKey`).
    @ObservationIgnored public var onChange: ((Theme) -> Void)?

    public init(defaults: UserDefaults, userThemes: UserThemes? = nil) {
        self.defaults = defaults
        self.userThemes = userThemes
        let s =
            defaults.data(forKey: Self.key).flatMap { try? JSONDecoder().decode(AppearanceSettings.self, from: $0) }
            ?? AppearanceSettings()
        settings = s
        colourBars = s.colourBars
        theme = Self.resolve(s.theme, userThemes?.themes ?? []) ?? .classicLight
        userThemes?.onReload = { [weak self] in self?.refresh() }
    }

    /// The themes Settings offers: the built-in ones, then the user's.
    public var themes: [Theme] { Theme.all + (userThemes?.themes.map(\.theme) ?? []) }

    /// The built-in theme `id` is, or is made from.
    public func baseID(of id: String) -> String? {
        if Theme.named(id) != nil { return id }
        return userThemes?.themes.first { $0.theme.id == id }?.base
    }

    private static func resolve(_ id: String, _ user: [UserTheme]) -> Theme? {
        Theme.named(id) ?? user.first { $0.theme.id == id }?.theme
    }

    /// Makes `id` the theme and saves it; false (nothing changed) when no
    /// theme has that ID.
    public func choose(_ id: String) -> Bool {
        guard let t = Self.resolve(id, userThemes?.themes ?? []) else { return false }
        settings.theme = id
        save()
        theme = t
        onChange?(t)
        return true
    }

    public func setColourBars(_ on: Bool) {
        settings.colourBars = on
        colourBars = on
        save()
    }

    /// Writes the current theme as a new file in the themes folder, a
    /// starting point to edit (R-UT): never over an existing file.
    public func duplicateCurrent() throws -> URL {
        guard let lib = userThemes else { throw UserThemeError(message: "There is no themes folder.") }
        let base = baseID(of: theme.id) ?? Self.defaultID
        return try lib.duplicate(theme, base: base)
    }

    /// The user themes were read again: the saved choice may now read, or
    /// no longer read, or read differently.
    private func refresh() {
        let t = Self.resolve(settings.theme, userThemes?.themes ?? []) ?? .classicLight
        guard t != theme else { return }
        theme = t
        onChange?(t)
    }

    private func save() {
        if let data = try? JSONEncoder().encode(settings) { defaults.set(data, forKey: Self.key) }
    }
}
