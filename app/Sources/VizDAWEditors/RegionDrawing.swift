import AppKit
import SymbioticTokens
import VizDAWAppModel
import VizDAWCore

/// The colours audio regions are drawn in, by meaning (plan 5b Ruling 16),
/// from a theme's editor roles (plan 7b Ruling 13).
public struct RegionPalette {
    public let theme: Theme

    public init(_ theme: Theme) { self.theme = theme }

    public var body: NSColor { theme.regionFill }
    public var outline: NSColor { theme.regionLine }
    public var selectedOutline: NSColor { theme.regionSelected }
    public var waveform: NSColor { theme.regionLine.withAlphaComponent(0.65) }
    public var fadeLine: NSColor { theme.secondaryLabel }
    public var handle: NSColor { theme.graph.handle.nsColor }
    public var gainLine: NSColor { theme.control.gain.nsColor }
    public var offline: NSColor { theme.offlineColour }
    /// The region strip's label: text on the region fill.
    public var name: NSColor { theme.label }
    /// Behind the "Offline" label, so it reads over the hatching (V2).
    public var offlineLabelBackground: NSColor { theme.canvas }
    public var readoutText: NSColor { theme.readout.text.nsColor }
    public var readoutBackground: NSColor { theme.readout.background.nsColor }
}

/// Drawing an audio region (spec §10.1–10.2), shared by the arrangement
/// and the region strip.
@MainActor
public enum RegionDrawing {
    /// The outline's width.
    public static func outlineWidth(selected: Bool) -> CGFloat { selected ? 2 : 1 }

    /// The gain line: inside the gain band, just below the outline's
    /// stroke (which covers `minY - 0.5 … minY + 0.5 + width`), so the
    /// selection outline never hides it (final review V4).
    public static func gainLineRect(in rect: CGRect) -> CGRect {
        let top = rect.minY + 0.5 + outlineWidth(selected: true) / 2 + 0.5
        return CGRect(x: rect.minX + 1, y: top, width: max(rect.width - 2, 0), height: 1.5)
    }

    /// The "Offline" label's font.
    static func offlineFont(_ t: Theme) -> NSFont { t.font(t.type.caption, weight: .semibold) }

    /// Where an arrangement region's "Offline" pill goes: top left, under
    /// the gain band and after the fade-in handle (clear of the fade-in
    /// line, which never reaches right of its handle), for a label of
    /// `size`; nil when the region is too small to hold it (final review
    /// V2).
    public static func offlineLabelRect(_ r: EventRow, in rect: CGRect, size: CGSize) -> CGRect? {
        let box = CGSize(width: size.width + 8, height: size.height + 2)
        var x = rect.minX + 4
        if r.fadeIn.length > 0 { x = max(x, RegionHandles.fadeInX(r, in: rect) + RegionHandles.size / 2 + 2) }
        let label = CGRect(x: x, y: rect.minY + RegionHandles.band + 2, width: box.width, height: box.height)
        guard label.maxX <= rect.maxX - 2, label.maxY <= rect.maxY - 2 else { return nil }
        return label
    }

    /// Draws `r` in `rect` into the current context: its body, its
    /// waveform `rows` in `waveRect` (ready or coarse only), its fade lines,
    /// its outline, a gain line when the gain is not 0 dB and, with
    /// `handles`, its corner handles. An offline region is hatched instead
    /// of showing a waveform and, with `offlineLabel`, says "Offline" when
    /// it has room (the region strip puts it in its own label instead;
    /// final review V1).
    public static func draw(
        _ r: EventRow, in rect: CGRect, rows: OverviewRows?, waveRect: CGRect, offline: Bool, selected: Bool,
        handles: Bool, offlineLabel: Bool = true, theme: Theme
    ) {
        let palette = RegionPalette(theme)
        let body = NSBezierPath(
            roundedRect: rect.insetBy(dx: 0.5, dy: 0.5), xRadius: theme.radius.s, yRadius: theme.radius.s)
        palette.body.withAlphaComponent(palette.body.alphaComponent * (r.mute ? 0.4 : 1)).setFill()
        body.fill()
        NSGraphicsContext.saveGraphicsState()
        body.addClip()
        if offline {
            hatch(rect, palette)
        } else if let rows, rows.state == .ready || rows.state == .coarse {
            palette.waveform.setFill()
            for b in WaveformGeometry.bars(rows, in: waveRect) { b.fill() }
        }
        palette.fadeLine.setStroke()
        if r.fadeIn.length > 0 {
            let p = NSBezierPath()
            p.move(to: CGPoint(x: rect.minX, y: rect.maxY))
            p.line(to: CGPoint(x: RegionHandles.fadeInX(r, in: rect), y: rect.minY))
            p.lineWidth = 1
            p.stroke()
        }
        if r.fadeOut.length > 0 {
            let p = NSBezierPath()
            p.move(to: CGPoint(x: RegionHandles.fadeOutX(r, in: rect), y: rect.minY))
            p.line(to: CGPoint(x: rect.maxX, y: rect.maxY))
            p.lineWidth = 1
            p.stroke()
        }
        NSGraphicsContext.restoreGraphicsState()
        (selected ? palette.selectedOutline : palette.outline).setStroke()
        body.lineWidth = outlineWidth(selected: selected)
        if r.mute { body.setLineDash([3, 2], count: 2, phase: 0) }
        body.stroke()
        // After the outline, below its stroke (V4).
        if r.gainDb != 0 {
            palette.gainLine.setFill()
            gainLineRect(in: rect).fill()
        }
        if handles {
            palette.handle.setFill()
            for (h, box) in RegionHandles.rects(r, in: rect) where h != .gain { NSBezierPath(rect: box).fill() }
        }
        if offline, offlineLabel {
            let attrs: [NSAttributedString.Key: Any] = [
                .font: offlineFont(theme), .foregroundColor: palette.offline,
            ]
            let text = NSString(string: "Offline")
            if let box = offlineLabelRect(r, in: rect, size: text.size(withAttributes: attrs)) {
                pill(text, in: box, attrs: attrs, palette)
            }
        }
    }

    /// `text` on the offline label background in `box`.
    static func pill(
        _ text: NSString, in box: CGRect, attrs: [NSAttributedString.Key: Any], _ palette: RegionPalette
    ) {
        palette.offlineLabelBackground.setFill()
        let radius = palette.theme.radius.s
        NSBezierPath(roundedRect: box, xRadius: radius, yRadius: radius).fill()
        text.draw(at: CGPoint(x: box.minX + 4, y: box.minY + 1), withAttributes: attrs)
    }

    /// The region strip's label for a region of lane `name`: the name, and
    /// "— Offline" when its media is offline (V1).
    public static func stripLabel(_ name: String, offline: Bool) -> String {
        offline ? (name.isEmpty ? "Offline" : "\(name) — Offline") : name
    }

    /// Where the region strip draws a region's label: under the gain band
    /// (and its gain line), inside the 18-point region (V1).
    public static func stripLabelOrigin(in rect: CGRect) -> CGPoint {
        CGPoint(x: rect.minX + 4, y: rect.minY + RegionHandles.band + 0.5)
    }

    /// Draws the region strip's label `text` for a region in `rect`: on the
    /// offline label background in the offline colour when offline.
    public static func drawStripLabel(_ text: String, in rect: CGRect, offline: Bool, font: NSFont, theme: Theme) {
        let palette = RegionPalette(theme)
        let s = NSString(string: text)
        let attrs = stripLabelAttributes(font: font, offline: offline, theme: theme)
        let at = stripLabelOrigin(in: rect)
        if offline {
            let size = s.size(withAttributes: attrs)
            pill(
                s, in: CGRect(x: at.x - 4, y: at.y - 0.5, width: size.width + 8, height: size.height + 1),
                attrs: attrs, palette)
        } else {
            s.draw(at: at, withAttributes: attrs)
        }
    }

    /// The region strip label's attributes: the theme's text colour on the
    /// region fill (never a fixed white), or the offline colour.
    public static func stripLabelAttributes(font: NSFont, offline: Bool, theme: Theme)
        -> [NSAttributedString.Key: Any]
    {
        let palette = RegionPalette(theme)
        return [.font: font, .foregroundColor: offline ? palette.offline : palette.name]
    }

    /// Diagonal hatching for offline media (parent spec §10).
    static func hatch(_ rect: CGRect, _ palette: RegionPalette) {
        palette.offline.withAlphaComponent(0.55).setStroke()
        let p = NSBezierPath()
        var x = rect.minX - rect.height
        while x < rect.maxX {
            p.move(to: CGPoint(x: x, y: rect.maxY))
            p.line(to: CGPoint(x: x + rect.height, y: rect.minY))
            x += 6
        }
        p.lineWidth = 1
        p.stroke()
    }

    /// The ±dB readout while the gain band is dragged.
    public static func readout(_ text: String, at p: CGPoint, theme: Theme) {
        let palette = RegionPalette(theme)
        let attrs: [NSAttributedString.Key: Any] = [
            .font: theme.digitsFont(theme.type.label, weight: .medium),
            .foregroundColor: palette.readoutText,
        ]
        let s = NSString(string: text)
        let size = s.size(withAttributes: attrs)
        let box = CGRect(x: p.x, y: p.y, width: size.width + 8, height: size.height + 4)
        palette.readoutBackground.setFill()
        let radius = theme.radius.s
        NSBezierPath(roundedRect: box, xRadius: radius, yRadius: radius).fill()
        s.draw(at: CGPoint(x: box.minX + 4, y: box.minY + 2), withAttributes: attrs)
    }

    /// The context menu of a fade's area: its four shapes, the current one
    /// checked; `apply` sends the edit.
    public static func fadeShapeMenu(
        _ r: EventRow, _ h: RegionHandle, apply: @escaping @MainActor (EditCommand) -> Void
    ) -> NSMenu {
        let menu = NSMenu()
        let current = h == .fadeOut ? r.fadeOut.shape : r.fadeIn.shape
        let fade = RegionHandles.label(h)
        for (shape, title) in RegionHandles.shapes {
            menu.addItem(
                MenuAction(title: "\(fade): \(title)") {
                    if let c = RegionHandles.shapeCommand(r, h, shape) { apply(c) }
                }.item(checked: shape == current))
        }
        return menu
    }
}
