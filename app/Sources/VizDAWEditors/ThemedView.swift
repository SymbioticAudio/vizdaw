import AppKit
import SymbioticTokens

/// An editor view that draws in a theme (plan 7b Ruling 12): each host's
/// `updateNSView` sets its root view's `theme` from `\.symbioticTheme`, and
/// the root passes it to the themed views below.
@MainActor
public protocol ThemedView: NSView {
    var theme: Theme { get set }
}

extension ThemedView {
    /// A view's `theme` `didSet`: when it really changed, redraw and pass it
    /// down.
    func themeDidChange(from old: Theme) {
        guard theme != old else { return }
        needsDisplay = true
        passTheme(theme)
    }
}

extension NSView {
    /// Sets `t` on the nearest themed views below this one (each passes it on).
    public func passTheme(_ t: Theme) {
        for v in subviews {
            if let themed = v as? any ThemedView {
                themed.theme = t
            } else {
                v.passTheme(t)
            }
        }
    }
}
