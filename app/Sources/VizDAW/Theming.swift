import AppKit
import SwiftUI
import SymbioticTokens
import VizDAWAppModel
import VizDAWEditors

/// Every SwiftUI root of the app (plan 7b Ruling 12): each scene's root view
/// and every `NSHostingView` the app makes. Below it `\.symbioticTheme`,
/// the colour scheme, `.primary`/`.secondary`/`.tertiary`, the tint and
/// the background are the theme's, and its window is styled as soon as the
/// root is in it and on every change (B4 review I1), however late the
/// window was made.
struct ThemedRoot: ViewModifier {
    let prefs: AppearancePreferences

    func body(content: Content) -> some View {
        let t = prefs.theme
        content
            .symbioticTheme(t)
            .foregroundStyle(t.text.primary.color, t.text.secondary.color, t.text.disabled.color)
            .tint(t.accent.accent.color)
            .background(WindowStyler(theme: t))
            .background(t.surface.window.color)
    }
}

/// Styles the window its marker is in (`WindowThemeMarker`).
private struct WindowStyler: NSViewRepresentable {
    let theme: Theme

    func makeNSView(context: Context) -> WindowThemeMarker {
        let m = WindowThemeMarker()
        m.theme = theme
        return m
    }

    func updateNSView(_ m: WindowThemeMarker, context: Context) {
        m.theme = theme
    }
}

extension View {
    /// `ThemedRoot` on the app's theme preference.
    func themedRoot(_ prefs: AppearancePreferences = appearancePreferences) -> some View {
        modifier(ThemedRoot(prefs: prefs))
    }
}

/// The theme in AppKit (Ruling 12): the app's appearance, so windows,
/// menus, alerts and open panels made later follow it, and each VizDAW
/// window's appearance and background. Only VizDAW's own windows (those
/// `ControlWindows` knows) are restyled; alerts and open panels follow
/// `NSApp.appearance` (B4 review M8).
@MainActor
enum ThemeWindows {
    /// The app and every window it has now. At launch (`AppDelegate`) and on
    /// every change (`AppearancePreferences.onChange`).
    static func apply(_ t: Theme) {
        // Themed AppKit views made from now on start in it (B4 review M1).
        ThemedViews.current = t
        NSApp.appearance = NSAppearance(named: t.appearance.nsAppearanceName)
        for w in NSApp.windows where ControlWindows.tagOf(w).kind != .other {
            WindowTheming.style(w, t)
        }
    }

    /// A window as it is made (plug-in windows, torn editors, the Modify
    /// panel), in the current theme.
    static func style(_ w: NSWindow) {
        WindowTheming.style(w, appearancePreferences.theme)
    }
}
