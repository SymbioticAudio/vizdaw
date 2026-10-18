import AppKit
import VizDAWCore

/// The control channel's window-side work (control spec §9, §10): the
/// accessibility tree with element IDs, and mouse and key events for
/// VizDAW's own windows. Events are posted to the app's own event queue
/// (`NSApp.postEvent`), never through Quartz: no system-wide events, no
/// real cursor movement, no Accessibility or Screen Recording permission.
/// Posting (not `sendEvent`) lets controls that track the mouse in a loop
/// (buttons, sliders, menus) read the rest of the gesture from the queue.
/// Coordinates are window points with a top-left origin, as in screenshots.
@MainActor
public enum ControlUI {
    /// The most nodes one tree holds.
    public static let maxNodes = 5000

    // MARK: Tree

    private static var accessibilityOn = false

    /// SwiftUI builds its accessibility tree only while an assistive app
    /// asks for it: the channel asks the way VoiceOver does, by setting
    /// AppKit's enhanced-user-interface attribute on the application.
    public static func enableAccessibility() {
        guard !accessibilityOn else { return }
        accessibilityOn = true
        _ = NSApplication.shared.perform(
            NSSelectorFromString("accessibilitySetValue:forAttribute:"), with: NSNumber(value: true),
            with: "AXEnhancedUserInterface")
    }

    /// `window`'s accessibility tree in pre-order, at most `maxDepth` deep.
    public static func tree(of window: NSWindow, maxDepth: Int) -> (nodes: [UiNodeRow], truncated: Bool) {
        enableAccessibility()
        var nodes: [UiNodeRow] = []
        var truncated = false
        var seen: Set<ObjectIdentifier> = [ObjectIdentifier(window)]
        func walk(_ e: any NSAccessibilityProtocol, _ path: [UInt32]) {
            guard path.count < maxDepth else { return }
            for (i, child) in (e.accessibilityChildren() ?? []).enumerated() {
                guard nodes.count < maxNodes else {
                    truncated = true
                    return
                }
                guard let c = child as? (any NSAccessibilityProtocol & NSObjectProtocol),
                    seen.insert(ObjectIdentifier(c)).inserted
                else { continue }
                let p = path + [UInt32(i)]
                nodes.append(row(c, path: p, in: window))
                walk(c, p)
            }
        }
        walk(window, [])
        return (nodes, truncated)
    }

    static func text(_ v: Any?) -> String? {
        switch v {
        case let s as String: s
        case let n as NSNumber: n.stringValue
        case let a as NSAttributedString: a.string
        default: nil
        }
    }

    static let pressable: Set<String> = [
        "AXButton", "AXCheckBox", "AXRadioButton", "AXMenuItem", "AXPopUpButton", "AXMenuButton",
        "AXDisclosureTriangle", "AXCell", "AXRow", "AXLink",
    ]

    static func row(_ e: any NSAccessibilityProtocol, path: [UInt32], in window: NSWindow) -> UiNodeRow {
        let role = e.accessibilityRole()?.rawValue ?? "AXUnknown"
        let f = windowRect(e.accessibilityFrame(), in: window)
        return UiNodeRow(
            path: path, role: role, subrole: e.accessibilitySubrole()?.rawValue,
            label: e.accessibilityLabel(), title: e.accessibilityTitle(), value: text(e.accessibilityValue()),
            x: f.minX, y: f.minY, width: f.width, height: f.height,
            enabled: e.isAccessibilityEnabled(), selected: e.isAccessibilitySelected(),
            focused: e.isAccessibilityFocused(), actions: pressable.contains(role) ? ["press"] : [])
    }

    /// A screen rect (AppKit, bottom-left origin) in window points with a
    /// top-left origin.
    public static func windowRect(_ screen: NSRect, in window: NSWindow) -> CGRect {
        let r = window.convertFromScreen(screen)
        return CGRect(x: r.minX, y: window.frame.height - r.maxY, width: r.width, height: r.height)
    }

    /// The element `ref` names, if it is still there with the same role
    /// and label.
    public static func element(_ ref: ElementRefRow, in window: NSWindow) -> (any NSAccessibilityProtocol)? {
        enableAccessibility()
        var e: any NSAccessibilityProtocol = window
        for i in ref.path {
            let kids = e.accessibilityChildren() ?? []
            guard Int(i) < kids.count, let next = kids[Int(i)] as? any NSAccessibilityProtocol else { return nil }
            e = next
        }
        let role = e.accessibilityRole()?.rawValue ?? "AXUnknown"
        let check = controlElementCheck(role: role, label: e.accessibilityLabel(), title: e.accessibilityTitle())
        return check == ref.check ? e : nil
    }

    /// The window's content area in window points with a top-left origin.
    static func contentRect(of window: NSWindow) -> CGRect {
        let r = window.contentLayoutRect
        return CGRect(x: r.minX, y: window.frame.height - r.maxY, width: r.width, height: r.height)
    }

    /// Where `p` lands in `window` (an element's centre); nil when the
    /// element is gone, or its frame is not finite, is empty, or has its
    /// centre outside the window's content (hidden, scrolled away).
    public static func point(_ p: PointerRow, in window: NSWindow) -> CGPoint? {
        switch p {
        case .point(let x, let y):
            return x.isFinite && y.isFinite ? CGPoint(x: x, y: y) : nil
        case .element(let ref):
            guard let e = element(ref, in: window) else { return nil }
            let f = windowRect(e.accessibilityFrame(), in: window)
            guard f.minX.isFinite, f.minY.isFinite, f.width.isFinite, f.height.isFinite,
                f.width > 0, f.height > 0
            else { return nil }
            let c = CGPoint(x: f.midX, y: f.midY)
            return contentRect(of: window).contains(c) ? c : nil
        }
    }

    // MARK: Events

    static func flags(_ m: ModifiersRow) -> NSEvent.ModifierFlags {
        var f: NSEvent.ModifierFlags = []
        if m.shift { f.insert(.shift) }
        if m.control { f.insert(.control) }
        if m.option { f.insert(.option) }
        if m.command { f.insert(.command) }
        return f
    }

    /// Channel events carry numbers from here up; the system's own are
    /// small counters. That is how the editors tell them apart.
    static let channelEventBase = 1 << 30
    private static var eventNumber = channelEventBase

    /// Whether `event` was built by the channel.
    public static func isChannel(_ event: NSEvent?) -> Bool {
        guard let e = event else { return false }
        switch e.type {
        case .leftMouseDown, .leftMouseUp, .leftMouseDragged, .rightMouseDown, .rightMouseUp:
            return e.eventNumber >= channelEventBase
        default: return false
        }
    }

    static func mouse(
        _ type: NSEvent.EventType, _ window: NSWindow, _ p: CGPoint, _ flags: NSEvent.ModifierFlags, clicks: Int
    ) -> NSEvent? {
        eventNumber += 1
        let pressure: Float = (type == .leftMouseUp || type == .rightMouseUp) ? 0 : 1
        return NSEvent.mouseEvent(
            with: type, location: NSPoint(x: p.x, y: window.frame.height - p.y), modifierFlags: flags,
            timestamp: ProcessInfo.processInfo.systemUptime, windowNumber: window.windowNumber, context: nil,
            eventNumber: eventNumber, clickCount: clicks, pressure: pressure)
    }

    /// A click (`count` presses, each down and up) at `p`.
    public static func clickEvents(
        _ window: NSWindow, at p: CGPoint, button: MouseButtonRow, count: Int, modifiers: ModifiersRow
    ) -> [NSEvent] {
        let (down, up): (NSEvent.EventType, NSEvent.EventType) =
            button == .right ? (.rightMouseDown, .rightMouseUp) : (.leftMouseDown, .leftMouseUp)
        let f = flags(modifiers)
        return (1...max(1, count)).flatMap { n in
            [mouse(down, window, p, f, clicks: n), mouse(up, window, p, f, clicks: n)].compactMap { $0 }
        }
    }

    /// A press at `from`, `steps` drags along the line, a release at `to`.
    public static func dragEvents(
        _ window: NSWindow, from: CGPoint, to: CGPoint, steps: Int, modifiers: ModifiersRow
    ) -> [NSEvent] {
        let f = flags(modifiers)
        let n = max(1, steps)
        var out = [mouse(.leftMouseDown, window, from, f, clicks: 1)]
        for k in 1...n {
            let t = CGFloat(k) / CGFloat(n)
            let p = CGPoint(x: from.x + (to.x - from.x) * t, y: from.y + (to.y - from.y) * t)
            out.append(mouse(.leftMouseDragged, window, p, f, clicks: 1))
        }
        out.append(mouse(.leftMouseUp, window, to, f, clicks: 1))
        return out.compactMap { $0 }
    }

    /// Down and up for every stroke.
    public static func keyEvents(_ window: NSWindow, _ strokes: [KeyStrokeRow]) -> [NSEvent] {
        strokes.flatMap { s -> [NSEvent] in
            [NSEvent.EventType.keyDown, .keyUp].compactMap { type in
                NSEvent.keyEvent(
                    with: type, location: .zero, modifierFlags: flags(s.modifiers),
                    timestamp: ProcessInfo.processInfo.systemUptime, windowNumber: window.windowNumber,
                    context: nil, characters: s.characters, charactersIgnoringModifiers: s.unmodified,
                    isARepeat: false, keyCode: s.keyCode)
            }
        }
    }

    /// The editors accept channel mouse events (`isChannel`) as first
    /// clicks, since VizDAW may not be the active app (macOS does not let a
    /// background app take focus) and AppKit would otherwise spend the click
    /// on activating the window. A user's own clicks are not affected.
    /// Orders `window` to the front (without stealing focus from the
    /// active app) and queues `events` behind anything already queued.
    public static func post(_ events: [NSEvent], to window: NSWindow) {
        window.orderFront(nil)
        for e in events { NSApp.postEvent(e, atStart: false) }
    }

    /// Sends key `events` straight to `window` (its first responder), which
    /// works whether or not VizDAW is active.
    public static func deliver(_ events: [NSEvent], to window: NSWindow) {
        for e in events { window.sendEvent(e) }
    }

    /// Whether the app's queue still holds posted mouse or key events.
    public static var eventsPending: Bool {
        let mask: NSEvent.EventTypeMask = [
            .leftMouseDown, .leftMouseUp, .leftMouseDragged, .rightMouseDown, .rightMouseUp, .keyDown, .keyUp,
        ]
        return NSApp.nextEvent(matching: mask, until: .distantPast, inMode: .default, dequeue: false) != nil
    }
}
