import AppKit
import ObjectiveC
import QuartzCore

/// A window as a PNG, rendered by the app itself (control spec §9), with
/// no Screen Recording permission:
///
/// 1. the window's layer tree is drawn with `CALayer.render(in:)`, which
///    brings in SwiftUI and Core Animation content;
/// 2. every view that draws itself (`draw(_:)` into its layer) is drawn
///    again, in place, back to front, with its own Core Animation
///    sublayers (the playhead) on top. AppKit does not redraw an occluded
///    window (behind the agent's terminal, or on a locked screen), so its
///    layers can hold a stale picture; drawing the views shows what they
///    hold now. A view that has never drawn (opened while its window
///    was occluded) is drawn too, though its layer has no contents.
///
/// Content the window server composites alone (vibrancy materials, Metal
/// or video layers, plugin views of other processes) renders flat or empty.
@MainActor
public enum WindowSnapshot {
    /// The view whose layer covers the whole window, title bar included.
    public static func root(of window: NSWindow) -> NSView? {
        window.contentView?.superview ?? window.contentView
    }

    /// `view` as PNG data at `scale` pixels per point, with its size in pixels.
    public static func png(of view: NSView, scale: CGFloat) -> (data: Data, width: Int, height: Int)? {
        guard scale.isFinite, scale > 0 else { return nil }
        view.layoutSubtreeIfNeeded()
        let size = view.bounds.size
        let dw = max(1, (Double(size.width) * Double(scale)).rounded())
        let dh = max(1, (Double(size.height) * Double(scale)).rounded())
        // Checked in Double, before any Int conversion can trap.
        guard dw.isFinite, dh.isFinite, dw <= 16_384, dh <= 16_384, dw * dh <= 64_000_000 else { return nil }
        let (w, h) = (Int(dw), Int(dh))
        guard let space = CGColorSpace(name: CGColorSpace.sRGB),
            let ctx = CGContext(
                data: nil, width: w, height: h, bitsPerComponent: 8, bytesPerRow: 0, space: space,
                bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue)
        else { return nil }
        ctx.scaleBy(x: CGFloat(w) / max(size.width, 1), y: CGFloat(h) / max(size.height, 1))
        if let layer = view.layer {
            ctx.saveGState()
            // A flipped view's own layer is geometry-flipped: rendered on
            // its own (not inside its window) it would come out upside down.
            if layer.isGeometryFlipped {
                ctx.translateBy(x: 0, y: size.height)
                ctx.scaleBy(x: 1, y: -1)
            }
            layer.render(in: ctx)
            ctx.restoreGState()
            redrawSelfDrawn(under: view, ctx: ctx)
        } else {
            let g = NSGraphicsContext(cgContext: ctx, flipped: false)
            NSGraphicsContext.saveGraphicsState()
            NSGraphicsContext.current = g
            view.displayIgnoringOpacity(view.bounds, in: g)
            NSGraphicsContext.restoreGraphicsState()
        }
        guard let image = ctx.makeImage(),
            let data = NSBitmapImageRep(cgImage: image).representation(using: .png, properties: [:])
        else { return nil }
        return (data, w, h)
    }

    /// `window` as PNG data; `scale` nil uses the window's backing scale.
    public static func png(of window: NSWindow, scale: CGFloat?) -> (data: Data, width: Int, height: Int)? {
        guard let v = root(of: window) else { return nil }
        return png(of: v, scale: scale ?? window.backingScaleFactor)
    }

    /// Whether `v`'s class draws in `draw(_:)`.
    static func drawsItself(_ v: NSView) -> Bool {
        let sel = #selector(NSView.draw(_:))
        return class_getMethodImplementation(type(of: v), sel) != class_getMethodImplementation(NSView.self, sel)
    }

    /// Step 2: the self-drawn views under `root`, back to front.
    static func redrawSelfDrawn(under root: NSView, ctx: CGContext) {
        func walk(_ v: NSView) {
            for s in v.subviews where !s.isHidden && s.alphaValue > 0 {
                // Also a view that never drew (made while its window was
                // occluded): its layer has no contents for step 1 to render.
                if s.layer != nil, !s.wantsUpdateLayer, !s.bounds.isEmpty, drawsItself(s),
                    !(s is NSVisualEffectView)
                {
                    redraw(s, root: root, ctx: ctx)
                }
                walk(s)
            }
        }
        walk(root)
    }

    /// Draws `s` where it is in `root` (bottom-left based context).
    static func redraw(_ s: NSView, root: NSView, ctx: CGContext) {
        // Only what ancestors (scroll views) let show, within its own
        // bounds: views do not clip to bounds by default (macOS 14+), so
        // `visibleRect` can reach past them, and a view that fills its
        // dirty rect (the ruler) would paint over its siblings.
        let visible = s.visibleRect.intersection(s.bounds)
        guard !visible.isEmpty else { return }
        func inContext(_ r: CGRect) -> CGRect {
            root.isFlipped ? CGRect(x: r.minX, y: root.bounds.height - r.maxY, width: r.width, height: r.height) : r
        }
        let rect = inContext(s.convert(s.bounds, to: root))
        ctx.saveGState()
        defer { ctx.restoreGState() }
        ctx.clip(to: inContext(s.convert(visible, to: root)))
        ctx.translateBy(x: rect.minX, y: rect.minY)
        if s.isFlipped {
            ctx.translateBy(x: 0, y: rect.height)
            ctx.scaleBy(x: 1, y: -1)
        }
        ctx.translateBy(x: -s.bounds.minX, y: -s.bounds.minY)
        NSGraphicsContext.saveGraphicsState()
        NSGraphicsContext.current = NSGraphicsContext(cgContext: ctx, flipped: s.isFlipped)
        s.draw(visible)
        NSGraphicsContext.restoreGraphicsState()
        // Its own Core Animation sublayers (not its subviews') on top.
        guard let layer = s.layer else { return }
        let views = Set(s.subviews.compactMap { $0.layer.map(ObjectIdentifier.init) })
        for sub in layer.sublayers ?? [] where !sub.isHidden && !views.contains(ObjectIdentifier(sub)) {
            ctx.saveGState()
            ctx.translateBy(x: sub.frame.minX, y: sub.frame.minY)
            sub.render(in: ctx)
            ctx.restoreGState()
        }
    }
}
