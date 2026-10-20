import AppKit
import VizDAWAppModel
import VizDAWCore

/// The colours audio regions are drawn in, by meaning (plan 5b Ruling 16).
/// Every one is a system colour; the SP7 light and dark themes replace
/// this one place.
@MainActor
public enum RegionPalette {
    public static var body: NSColor { .systemTeal }
    public static var outline: NSColor { .systemTeal }
    public static var waveform: NSColor { .labelColor.withAlphaComponent(0.65) }
    public static var fadeLine: NSColor { .secondaryLabelColor }
    public static var handle: NSColor { .controlAccentColor }
    public static var gainLine: NSColor { .systemOrange }
    public static var offline: NSColor { .systemRed }
    public static var readoutText: NSColor { .labelColor }
    public static var readoutBackground: NSColor { .textBackgroundColor }
}

/// Drawing an audio region (spec §10.1–10.2), shared by the arrangement
/// and the region strip.
@MainActor
public enum RegionDrawing {
    /// Draws `r` in `rect` into the current context: its body, its
    /// waveform `rows` in `waveRect` (ready or coarse only), its fade lines,
    /// a gain line when the gain is not 0 dB, its outline and, with
    /// `handles`, its corner handles. An offline region is hatched instead
    /// of showing a waveform.
    public static func draw(
        _ r: EventRow, in rect: CGRect, rows: OverviewRows?, waveRect: CGRect, offline: Bool, selected: Bool,
        handles: Bool
    ) {
        let body = NSBezierPath(roundedRect: rect.insetBy(dx: 0.5, dy: 0.5), xRadius: 2, yRadius: 2)
        RegionPalette.body.withAlphaComponent(r.mute ? 0.10 : 0.22).setFill()
        body.fill()
        NSGraphicsContext.saveGraphicsState()
        body.addClip()
        if offline {
            hatch(rect)
        } else if let rows, rows.state == .ready || rows.state == .coarse {
            RegionPalette.waveform.setFill()
            for b in WaveformGeometry.bars(rows, in: waveRect) { b.fill() }
        }
        RegionPalette.fadeLine.setStroke()
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
        if r.gainDb != 0 {
            RegionPalette.gainLine.setFill()
            CGRect(x: rect.minX, y: rect.minY + 1, width: rect.width, height: 1).fill()
        }
        NSGraphicsContext.restoreGraphicsState()
        (selected ? NSColor.controlAccentColor : RegionPalette.outline).setStroke()
        body.lineWidth = selected ? 2 : 1
        if r.mute { body.setLineDash([3, 2], count: 2, phase: 0) }
        body.stroke()
        if handles {
            RegionPalette.handle.setFill()
            for (h, box) in RegionHandles.rects(r, in: rect) where h != .gain { NSBezierPath(rect: box).fill() }
        }
        if offline, rect.width > 60 {
            NSString(string: "Offline").draw(
                at: CGPoint(x: rect.minX + 4, y: rect.maxY - 14),
                withAttributes: [
                    .font: NSFont.systemFont(ofSize: 10, weight: .semibold), .foregroundColor: RegionPalette.offline,
                ])
        }
    }

    /// Diagonal hatching for offline media (parent spec §10).
    static func hatch(_ rect: CGRect) {
        RegionPalette.offline.withAlphaComponent(0.55).setStroke()
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
    public static func readout(_ text: String, at p: CGPoint) {
        let attrs: [NSAttributedString.Key: Any] = [
            .font: NSFont.monospacedDigitSystemFont(ofSize: 11, weight: .medium),
            .foregroundColor: RegionPalette.readoutText,
        ]
        let s = NSString(string: text)
        let size = s.size(withAttributes: attrs)
        let box = CGRect(x: p.x, y: p.y, width: size.width + 8, height: size.height + 4)
        RegionPalette.readoutBackground.setFill()
        NSBezierPath(roundedRect: box, xRadius: 3, yRadius: 3).fill()
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
