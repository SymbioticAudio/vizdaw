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
    /// A region's fill on a lane whose strip has colour `tint` (owner G1
    /// Q2): the tint, as translucent as the theme's region fill and at most
    /// 0.45, so the outline, waveform and text keep reading over it.
    func regionFill(tint: NSColor?) -> NSColor {
        guard let tint else { return regionFill }
        return tint.withAlphaComponent(min(regionFill.alphaComponent, 0.45))
    }
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

    // White and black in every theme (owner decision 2026-11-03, overruling
    // Ruling 13's surface.raised / text.primary): the `keys` group.
    var whiteKey: NSColor { keys.white.nsColor }
    var blackKey: NSColor { keys.black.nsColor }
    /// The octave labels, drawn on the white keys.
    var keyLabel: NSColor { keys.black.nsColor }

    /// Lane `index`'s colour: `swatch`, in order (`red` … `pink`), by
    /// `index % 8` (replaces `PianoRollPalette`'s system colours).
    func laneColour(_ index: Int) -> NSColor {
        let s = swatch
        let all = [s.red, s.orange, s.yellow, s.green, s.teal, s.blue, s.purple, s.pink]
        return all[((index % all.count) + all.count) % all.count].nsColor
    }

    /// Text on a `fill` such as a lit toggle: the black or white key ink
    /// (near-black and near-white in every theme), whichever has the higher
    /// WCAG contrast with it (B5 review I1).
    func legend(on fill: NSColor) -> NSColor {
        let dark = keys.black.nsColor
        let light = keys.white.nsColor
        return Self.contrast(dark, fill) >= Self.contrast(light, fill) ? dark : light
    }

    /// The first of `candidates` with at least `ratio` contrast on
    /// `background`; if none has, the one with the most.
    func firstReadable(_ candidates: [ThemeColor], on background: NSColor, ratio: CGFloat) -> NSColor {
        let inks = candidates.map(\.nsColor)
        if let ok = inks.first(where: { Self.contrast($0, background) >= ratio }) { return ok }
        return inks.max { Self.contrast($0, background) < Self.contrast($1, background) } ?? label
    }

    /// WCAG relative luminance (0 black … 1 white), from linearised sRGB.
    static func relativeLuminance(_ c: NSColor) -> CGFloat {
        guard let s = c.usingColorSpace(.sRGB) else { return 0.5 }
        func lin(_ v: CGFloat) -> CGFloat { v <= 0.04045 ? v / 12.92 : pow((v + 0.055) / 1.055, 2.4) }
        return 0.2126 * lin(s.redComponent) + 0.7152 * lin(s.greenComponent) + 0.0722 * lin(s.blueComponent)
    }

    /// The WCAG contrast ratio of two colours (1 … 21).
    static func contrast(_ a: NSColor, _ b: NSColor) -> CGFloat {
        let la = relativeLuminance(a)
        let lb = relativeLuminance(b)
        return (max(la, lb) + 0.05) / (min(la, lb) + 0.05)
    }

    // MARK: Status inks (B5 review M4, M5)

    /// "Offline" text and icons on the window (the media pool, the relink
    /// sheet): the warning colour where it reads (4.5:1), else the
    /// secondary text.
    var offlineText: NSColor {
        firstReadable([state.warning, text.secondary], on: surface.window.nsColor, ratio: 4.5)
    }
    /// The key recorder's ring while it records: the focus ring where it
    /// reads on the window (3:1, non-text), else the accent or the text.
    var recordingRing: NSColor {
        firstReadable([surface.focusRing, accent.accent, text.primary], on: surface.window.nsColor, ratio: 3)
    }
    /// A tool's overlay on the canvas (the strip chart's compress box): the
    /// accent, else the selected handle or the text.
    var toolOverlay: NSColor {
        firstReadable([accent.accent, graph.handleSelected, text.primary], on: canvas, ratio: 3)
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

