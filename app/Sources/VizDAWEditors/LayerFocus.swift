import AppKit

/// Ends the keyboard focus of a layer the window is about to hide (plan
/// 7a Rulings 3 and 9): a hidden piano roll must not take Delete, and a
/// hidden list cell or lane name ends its edit through its own end path.
public enum LayerFocus {
    /// When `window`'s first responder is a view whose frame lies inside
    /// `rect` (window coordinates), or that the layer collapsed to nothing
    /// (Task 7 review M2), resigns it (`makeFirstResponder(nil)`) and
    /// returns true. A cell's field editor counts as its cell.
    @MainActor @discardableResult
    public static func resign(_ window: NSWindow?, inside rect: NSRect) -> Bool {
        guard let window, !rect.isEmpty, let view = window.firstResponder as? NSView, view.window === window
        else { return false }
        let r = view.convert(view.bounds, to: nil)
        // A zero-sized view's `visibleRect` is not empty, so its own
        // rect is checked too.
        let collapsed = r.isEmpty || view.visibleRect.isEmpty
        guard rect.insetBy(dx: -1, dy: -1).contains(NSPoint(x: r.midX, y: r.midY)) || collapsed else { return false }
        return window.makeFirstResponder(nil)
    }
}
