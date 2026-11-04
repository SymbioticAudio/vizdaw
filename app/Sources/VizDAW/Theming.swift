import AppKit
import SwiftUI
import SymbioticTokens
import VizDAWAppModel

/// Every SwiftUI root of the app (plan 7b Ruling 12): each scene's root view
/// and every `NSHostingView` the app makes. Below it `\.symbioticTheme`,
/// the colour scheme, `.primary`/`.secondary`/`.tertiary`, the tint and
/// the background are the theme's.
struct ThemedRoot: ViewModifier {
    let prefs: AppearancePreferences

    func body(content: Content) -> some View {
        let t = prefs.theme
        content
            .symbioticTheme(t)
            .foregroundStyle(t.text.primary.color, t.text.secondary.color, t.text.disabled.color)
            .tint(t.accent.accent.color)
            .background(t.surface.window.color)
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
/// window's appearance and background.
@MainActor
enum ThemeWindows {
    /// The app and every window it has now. At launch (`AppDelegate`) and on
    /// every change (`AppearancePreferences.onChange`).
    static func apply(_ t: Theme) {
        NSApp.appearance = NSAppearance(named: t.appearance.nsAppearanceName)
        for w in NSApp.windows where w.styleMask.contains(.titled) {
            style(w, t)
        }
    }

    /// A window as it is made (plug-in windows, torn editors, the Modify
    /// panel), in the current theme.
    static func style(_ w: NSWindow) {
        style(w, appearancePreferences.theme)
    }

    private static func style(_ w: NSWindow, _ t: Theme) {
        w.appearance = NSAppearance(named: t.appearance.nsAppearanceName)
        w.backgroundColor = t.surface.window.nsColor
    }
}
