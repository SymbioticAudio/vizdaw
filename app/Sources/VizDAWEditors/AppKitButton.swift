import AppKit
import SwiftUI
import SymbioticTokens
import VizDAWAppModel

/// A bar button as an AppKit `NSButton` (plan 7a final review I1): an
/// `AXButton` (or, with `isOn`, a toggle) named `title` in the window's
/// accessibility tree, which the control channel walks. SwiftUI's own
/// `Button` and `Toggle` are not in that tree. With `icon` it shows the
/// icon only (its SymbioticAudio icon, chosen in AppIcon: plan 7b Task B3
/// and the final fix wave, as a template image); without,
/// the title. It never takes the keyboard (Full Keyboard Access stays on the editors;
/// Task 7 review M4).
public struct AppKitButton: View {
    let button: BarButton

    public init(
        _ title: String, icon: AppIcon? = nil, isOn: Bool? = nil, enabled: Bool = true, help: String,
        tint: NSColor? = nil, action: @escaping @MainActor () -> Void
    ) {
        button = BarButton(
            title: title, icon: icon, isOn: isOn, enabled: enabled, help: help, tint: tint, action: action)
    }

    // SwiftUI sets a hosted control's `isEnabled` from the environment, so
    // the state goes there.
    public var body: some View { button.disabled(!button.enabled) }
}

/// `AppKitButton`'s `NSButton`.
struct BarButton: NSViewRepresentable {
    let title: String
    let icon: AppIcon?
    let isOn: Bool?
    let enabled: Bool
    let help: String
    let tint: NSColor?
    let action: @MainActor () -> Void

    @MainActor
    final class Coordinator: NSObject {
        var action: @MainActor () -> Void
        var isOn: Bool?

        init(action: @escaping @MainActor () -> Void) { self.action = action }

        @objc func press(_ sender: NSButton) {
            // A toggle shows the model, not its own flip: the action
            // changes the model, which updates the button.
            if let on = isOn { sender.state = on ? .on : .off }
            action()
        }
    }

    func makeCoordinator() -> Coordinator { Coordinator(action: action) }

    func makeNSView(context: Context) -> NSButton {
        let b = NSButton(title: "", target: context.coordinator, action: #selector(Coordinator.press(_:)))
        b.bezelStyle = .push
        b.refusesFirstResponder = true
        if isOn != nil { b.setButtonType(.pushOnPushOff) }
        update(b, context.coordinator, context.environment.symbioticTheme)
        return b
    }

    func updateNSView(_ b: NSButton, context: Context) {
        context.coordinator.action = action
        update(b, context.coordinator, context.environment.symbioticTheme)
    }

    private func update(_ b: NSButton, _ c: Coordinator, _ theme: Theme) {
        c.isOn = isOn
        if let icon {
            b.image = icon.nsImage(accessibilityDescription: title, theme: ThemedViews.current)
            b.imagePosition = .imageOnly
            b.title = ""
        } else {
            b.image = nil
            b.imagePosition = .noImage
            b.title = title
        }
        b.setAccessibilityLabel(title)
        b.toolTip = help
        b.isEnabled = enabled
        if let isOn { b.state = isOn ? .on : .off }
        // The theme's colours only (plan 7b Ruling 13); an "on" toggle
        // takes its accent.
        b.contentTintColor = tint ?? (isOn == true ? theme.accent.accent.nsColor : nil)
    }
}

/// Bar text as an AppKit label (`NSTextField(labelWithString:)`): an
/// `AXStaticText` whose value is the text, in the channel's tree (I1).
public struct AppKitLabel: NSViewRepresentable {
    let text: String
    let font: NSFont
    let secondary: Bool
    let truncatesMiddle: Bool

    public init(_ text: String, font: NSFont, secondary: Bool = false, truncatesMiddle: Bool = false) {
        self.text = text
        self.font = font
        self.secondary = secondary
        self.truncatesMiddle = truncatesMiddle
    }

    /// The width a label needs to show `text` in `font` on one line.
    @MainActor
    public static func width(of text: String, font: NSFont) -> CGFloat {
        let f = NSTextField(labelWithString: text)
        f.font = font
        return f.intrinsicContentSize.width
    }

    public func makeNSView(context: Context) -> NSTextField {
        let f = NSTextField(labelWithString: text)
        f.maximumNumberOfLines = 1
        f.cell?.usesSingleLineMode = true
        f.refusesFirstResponder = true
        update(f, context.environment.symbioticTheme)
        return f
    }

    public func updateNSView(_ f: NSTextField, context: Context) { update(f, context.environment.symbioticTheme) }

    /// In the theme's text colours, never the window's (plan 7b Ruling 13).
    private func update(_ f: NSTextField, _ theme: Theme) {
        if f.stringValue != text { f.stringValue = text }
        f.font = font
        f.textColor = secondary ? theme.secondaryLabel : theme.label
        f.lineBreakMode = truncatesMiddle ? .byTruncatingMiddle : .byClipping
    }

    /// Its own width, or (when it truncates) narrower if the proposal is.
    public func sizeThatFits(_ proposal: ProposedViewSize, nsView f: NSTextField, context: Context) -> CGSize? {
        let ideal = f.intrinsicContentSize
        guard truncatesMiddle, let w = proposal.width, w.isFinite else { return ideal }
        return CGSize(width: max(min(w, ideal.width), 0), height: ideal.height)
    }
}

/// A bar button that is held (plan 7d Task B3): `onPress` at mouse-down
/// (with ⇧), `onTick` every `TransportShuttle.tickEvery` while it is held,
/// `onRelease` once at the end (with whether the pointer ended inside), and
/// `onCancel` if the button leaves its window mid-hold. An accessibility
/// press is a click: `onPress(false)` then `onRelease(true)`. Like
/// `AppKitButton` it is an `AXButton` named `title` and never takes the
/// keyboard.
public struct AppKitHoldButton: View {
    let button: HoldButton

    public init(
        _ title: String, icon: AppIcon, enabled: Bool, help: String,
        onPress: @escaping @MainActor (Bool) -> Void, onTick: @escaping @MainActor () -> Void,
        onRelease: @escaping @MainActor (Bool) -> Void, onCancel: @escaping @MainActor () -> Void
    ) {
        button = HoldButton(
            title: title, icon: icon, enabled: enabled, help: help, onPress: onPress, onTick: onTick,
            onRelease: onRelease, onCancel: onCancel)
    }

    public var body: some View { button.disabled(!button.enabled) }
}

/// `AppKitHoldButton`'s representable.
struct HoldButton: NSViewRepresentable {
    let title: String
    let icon: AppIcon
    let enabled: Bool
    let help: String
    let onPress: @MainActor (Bool) -> Void
    let onTick: @MainActor () -> Void
    let onRelease: @MainActor (Bool) -> Void
    let onCancel: @MainActor () -> Void

    func makeNSView(context: Context) -> HoldButtonView {
        let b = HoldButtonView(frame: .zero)
        b.bezelStyle = .push
        b.refusesFirstResponder = true
        b.title = ""
        update(b)
        return b
    }

    func updateNSView(_ b: HoldButtonView, context: Context) { update(b) }

    private func update(_ b: HoldButtonView) {
        b.onPress = onPress
        b.onTick = onTick
        b.onRelease = onRelease
        b.onCancel = onCancel
        b.image = icon.nsImage(accessibilityDescription: title, theme: ThemedViews.current)
        b.imagePosition = .imageOnly
        b.setAccessibilityLabel(title)
        b.toolTip = help
        b.isEnabled = enabled
    }
}

/// Holds the view for the timer's `@Sendable` block (Swift 6.1: the block
/// captures this `@MainActor` class, never the view).
@MainActor
private final class HoldTimerBox {
    weak var view: HoldButtonView?
}

/// The hold button's `NSButton`.
final class HoldButtonView: NSButton {
    var onPress: @MainActor (Bool) -> Void = { _ in }
    var onTick: @MainActor () -> Void = {}
    var onRelease: @MainActor (Bool) -> Void = { _ in }
    var onCancel: @MainActor () -> Void = {}

    private var timer: Timer?
    private var holding = false
    private var ticks = 0
    /// `systemUptime` at the press (internal: tests age it).
    var holdStartedAt: TimeInterval = 0

    var timerIsRunning: Bool { timer?.isValid == true }

    /// ⇧ in the event's own flags, so a synthesized ⇧ counts.
    static func shift(of event: NSEvent) -> Bool { event.modifierFlags.contains(.shift) }

    override func mouseDown(with event: NSEvent) {
        guard isEnabled else { return super.mouseDown(with: event) }
        beginHold(shift: Self.shift(of: event))
        super.mouseDown(with: event)  // the tracking loop
        var inside = true
        if let w = window { inside = bounds.contains(convert(w.mouseLocationOutsideOfEventStream, from: nil)) }
        endHold(inside: inside)
    }

    override func accessibilityPerformPress() -> Bool {
        guard isEnabled else { return false }
        onPress(false)
        onRelease(true)
        return true
    }

    override func viewDidMoveToWindow() {
        super.viewDidMoveToWindow()
        if window == nil { cancelHold() }
    }

    func beginHold(shift: Bool) {
        if holding { cancelHold() }
        holding = true
        ticks = 0
        holdStartedAt = ProcessInfo.processInfo.systemUptime
        onPress(shift)
        // `onPress` may have ended the hold (a view torn down).
        guard holding else { return }
        let box = HoldTimerBox()
        box.view = self
        let t = Timer(timeInterval: TransportShuttle.tickEvery, repeats: true) { _ in
            MainActor.assumeIsolated { box.view?.timerFired() }
        }
        RunLoop.main.add(t, forMode: .common)
        timer = t
    }

    func timerFired() {
        guard holding else { return }
        ticks += 1
        onTick()
    }

    /// The hold ends. A release always follows at least one tick when one
    /// was due, so a stalled main thread (whose timer never fired) does not
    /// turn a hold into a click.
    func endHold(inside: Bool = true) {
        guard holding else { return }
        stopTimer()
        if ticks == 0, ProcessInfo.processInfo.systemUptime - holdStartedAt >= TransportShuttle.tickEvery {
            ticks = 1
            onTick()
        }
        holding = false
        onRelease(inside)
    }

    func cancelHold() {
        guard holding else { return }
        holding = false
        stopTimer()
        onCancel()
    }

    private func stopTimer() {
        timer?.invalidate()
        timer = nil
    }
}
