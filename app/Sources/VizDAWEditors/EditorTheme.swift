import AppKit
import SymbioticTokens

/// The editor colour roles (plan 7b Ruling 13's table): one of the two
/// files that may name a concrete colour. Every editor colour is one of
/// these, so switching the theme changes everything (spec §4).
public extension Theme {
    // MARK: Surfaces and lines

    /// The canvas background.
    var canvas: NSColor { surface.inset.nsColor }
    /// The ruler and header background.
    var rulerBackground: NSColor { surface.panel.nsColor }
    var barLine: NSColor { graph.grid.nsColor }
    var beatLine: NSColor { graph.grid.nsColor.withAlphaComponent(0.35) }
    var separatorLine: NSColor { surface.separator.nsColor }
    var label: NSColor { text.primary.nsColor }
    var secondaryLabel: NSColor { text.secondary.nsColor }

    // MARK: Selection

    var selectionFill: NSColor { surface.selection.nsColor }
    /// Text on `selectionFill`.
    var selectionText: NSColor { text.inverse.nsColor }

    // MARK: Regions, notes and values

    /// Region and note fill.
    var regionFill: NSColor { graph.fill.nsColor }
    /// Region and note outline.
    var regionLine: NSColor { graph.line.nsColor }
    /// A selected region's or note's outline.
    var regionSelected: NSColor { graph.handleSelected.nsColor }
    /// Velocity and controller bars.
    var valueBar: NSColor { graph.line.nsColor }

    // MARK: States

    var playhead: NSColor { state.record.nsColor }
    var armColour: NSColor { state.record.nsColor }
    var muteColour: NSColor { state.mute.nsColor }
    var soloColour: NSColor { state.solo.nsColor }
    var warningColour: NSColor { state.warning.nsColor }
    var errorColour: NSColor { state.error.nsColor }
    var offlineColour: NSColor { state.offline.nsColor }

    // MARK: Piano keys

    var whiteKey: NSColor { surface.raised.nsColor }
    var blackKey: NSColor { text.primary.nsColor }

    /// Lane `index`'s colour: `swatch`, in order (`red` … `pink`), by
    /// `index % 8` (replaces `PianoRollPalette`'s system colours).
    func laneColour(_ index: Int) -> NSColor {
        let s = swatch
        let all = [s.red, s.orange, s.yellow, s.green, s.teal, s.blue, s.purple, s.pink]
        return all[((index % all.count) + all.count) % all.count].nsColor
    }

    /// Text on a `fill` such as a lit toggle: the theme's text or inverse
    /// text, whichever stands out more from it.
    func legend(on fill: NSColor) -> NSColor {
        let a = text.primary.nsColor
        let b = text.inverse.nsColor
        let f = Self.luminance(fill)
        return abs(Self.luminance(a) - f) >= abs(Self.luminance(b) - f) ? a : b
    }

    /// Relative luminance in sRGB (0 black … 1 white).
    static func luminance(_ c: NSColor) -> CGFloat {
        guard let s = c.usingColorSpace(.sRGB) else { return 0.5 }
        return 0.2126 * s.redComponent + 0.7152 * s.greenComponent + 0.0722 * s.blueComponent
    }

    // MARK: Fonts

    /// `style`'s size in another weight.
    func font(_ style: TypeStyle, weight: NSFont.Weight) -> NSFont {
        .systemFont(ofSize: style.size, weight: weight)
    }

    /// `style`'s size with monospaced digits (rulers, readouts).
    func digitsFont(_ style: TypeStyle, weight: NSFont.Weight = .regular) -> NSFont {
        .monospacedDigitSystemFont(ofSize: style.size, weight: weight)
    }
}

/// The theme a themed view starts in (B4 review M1): the app sets it on
/// every theme change (`ThemeWindows.apply`), so a view made after the
/// change (a list cell scrolled into view, a new editor child, the key
/// recorder) draws in the current theme before any host passes it one.
public enum ThemedViews {
    /// Written and read on the main thread only.
    nonisolated(unsafe) public static var current: Theme = .classicLight
}
