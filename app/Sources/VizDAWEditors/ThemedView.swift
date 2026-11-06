import AppKit
import SymbioticTokens
import VizDAWAppModel

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

/// A window in a theme (Ruling 12): its appearance and its background.
@MainActor
public enum WindowTheming {
    public static func style(_ w: NSWindow, _ t: Theme) {
        w.appearance = NSAppearance(named: t.appearance.nsAppearanceName)
        w.backgroundColor = t.surface.window.nsColor
    }
}

/// An empty view in a themed root that styles the window it is in: when
/// it arrives in a window and whenever its theme changes, so a window made
/// after the last theme change is styled at once (B4 review I1).
public final class WindowThemeMarker: NSView {
    public override init(frame: NSRect) {
        super.init(frame: frame)
        clipsToBounds = true
    }

    public required init?(coder: NSCoder) { nil }

    public var theme: Theme = ThemedViews.current {
        didSet { if theme != oldValue { styleWindow() } }
    }

    public override func viewDidMoveToWindow() {
        super.viewDidMoveToWindow()
        styleWindow()
    }

    public override func hitTest(_ point: NSPoint) -> NSView? { nil }

    /// On the next turn of the run loop: styling a window while SwiftUI is
    /// still building it (a Settings window's toolbar) left its title bar,
    /// tabs and window buttons undrawn.
    private func styleWindow() {
        DispatchQueue.main.async { [self] in
            guard let w = window else { return }
            WindowTheming.style(w, theme)
        }
    }
}
