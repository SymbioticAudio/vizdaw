import SymbioticTokens

/// The theme a themed view starts in (B4 review M1): the app sets it on
/// every theme change (`ThemeWindows.apply`), so a view made after the
/// change (a list cell scrolled into view, a new editor child, the key
/// recorder) draws in the current theme before any host passes it one.
/// AppIcon draws its images in it too (final fix wave B), so it lives here
/// rather than in VizDAWEditors.
public enum ThemedViews {
    @MainActor public static var current: Theme = .classicLight
}
