import AppKit
import SwiftUI

/// A bar button as an AppKit `NSButton` (plan 7a final review I1): an
/// `AXButton` (or, with `isOn`, a toggle) named `title` in the window's
/// accessibility tree, which the control channel walks. SwiftUI's own
/// `Button` and `Toggle` are not in that tree. With `symbol` it shows the
/// symbol only; without, the title. It never takes the keyboard (Full
/// Keyboard Access stays on the editors; Task 7 review M4).
public struct AppKitButton: View {
    let button: BarButton

    public init(
        _ title: String, symbol: String? = nil, isOn: Bool? = nil, enabled: Bool = true, help: String,
        tint: NSColor? = nil, action: @escaping @MainActor () -> Void
    ) {
        button = BarButton(
            title: title, symbol: symbol, isOn: isOn, enabled: enabled, help: help, tint: tint, action: action)
    }

    // SwiftUI sets a hosted control's `isEnabled` from the environment, so
    // the state goes there.
    public var body: some View { button.disabled(!button.enabled) }
}

/// `AppKitButton`'s `NSButton`.
struct BarButton: NSViewRepresentable {
    let title: String
    let symbol: String?
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
        update(b, context.coordinator)
        return b
    }

    func updateNSView(_ b: NSButton, context: Context) {
        context.coordinator.action = action
        update(b, context.coordinator)
    }

    private func update(_ b: NSButton, _ c: Coordinator) {
        c.isOn = isOn
        if let symbol {
            b.image = NSImage(systemSymbolName: symbol, accessibilityDescription: title)
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
        // Semantic colours only; an "on" toggle also takes the accent.
        b.contentTintColor = tint ?? (isOn == true ? .controlAccentColor : nil)
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
        update(f)
        return f
    }

    public func updateNSView(_ f: NSTextField, context: Context) { update(f) }

    private func update(_ f: NSTextField) {
        if f.stringValue != text { f.stringValue = text }
        f.font = font
        f.textColor = secondary ? .secondaryLabelColor : .labelColor
        f.lineBreakMode = truncatesMiddle ? .byTruncatingMiddle : .byClipping
    }

    /// Its own width, or (when it truncates) narrower if the proposal is.
    public func sizeThatFits(_ proposal: ProposedViewSize, nsView f: NSTextField, context: Context) -> CGSize? {
        let ideal = f.intrinsicContentSize
        guard truncatesMiddle, let w = proposal.width, w.isFinite else { return ideal }
        return CGSize(width: max(min(w, ideal.width), 0), height: ideal.height)
    }
}
