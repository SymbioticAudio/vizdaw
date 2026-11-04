import AppKit
import SwiftUI
import Testing
@testable import VizDAWEditors

/// The transport and mixer bars' AppKit controls (plan 7a final review I1):
/// the channel's tree walks AppKit's accessibility elements and sees no
/// SwiftUI `Button`, `Toggle` or `Text`, so the bars' controls are AppKit.
@MainActor
@Suite struct AppKitButtonTests {
    func hosted<V: View>(_ v: V) -> NSWindow {
        let w = NSWindow(
            contentRect: NSRect(x: 100, y: 100, width: 500, height: 100), styleMask: [.titled],
            backing: .buffered, defer: false)
        w.isReleasedWhenClosed = false
        w.contentView = NSHostingView(rootView: v)
        w.contentView?.layoutSubtreeIfNeeded()
        return w
    }

    func buttons(in v: NSView?) -> [NSButton] {
        guard let v else { return [] }
        return (v as? NSButton).map { [$0] } ?? v.subviews.flatMap { buttons(in: $0) }
    }

    @Test func barButtonsAreInTheTreeWithTheirLabelsAndState() throws {
        let w = hosted(
            HStack {
                AppKitButton("Loop", symbol: "repeat", isOn: true, help: "Loop") {}
                AppKitButton("Left Pane", symbol: "sidebar.left", isOn: false, help: "Left Pane") {}
                AppKitButton("Add Bus", help: "Add a bus") {}
                AppKitButton("Record", symbol: "record.circle", enabled: false, help: "Record") {}
            })
        defer { w.close() }
        let nodes = ControlUI.tree(of: w, maxDepth: 64).nodes
        for name in ["Loop", "Left Pane", "Add Bus", "Record"] {
            let n = try #require(nodes.first { $0.label == name || $0.title == name }, "\(name)")
            #expect(n.actions == ["press"], "\(name): \(n.role)")
        }
        #expect(nodes.first { $0.label == "Record" }?.enabled == false)
        let b = buttons(in: w.contentView)
        #expect(b.count == 4)
        #expect(b.first { $0.accessibilityLabel() == "Loop" }?.state == .on)
        #expect(b.first { $0.accessibilityLabel() == "Left Pane" }?.state == .off)
        // Full Keyboard Access does not move the keyboard to them (Task 7 M4).
        #expect(b.allSatisfy { $0.refusesFirstResponder })
        #expect(b.first { $0.accessibilityLabel() == "Loop" }?.toolTip == "Loop")
    }

    @Test func aPressRunsTheActionAndAToggleShowsTheModel() throws {
        var pressed = 0
        let w = hosted(AppKitButton("Loop", symbol: "repeat", isOn: false, help: "Loop") { pressed += 1 })
        defer { w.close() }
        let b = try #require(buttons(in: w.contentView).first)
        b.performClick(nil)
        #expect(pressed == 1)
        // The model did not change (a refused toggle): the button shows the model.
        #expect(b.state == .off)
    }

    @Test func barTextIsStaticTextWithItsValue() throws {
        let w = hosted(AppKitLabel("DSP 12%", font: .systemFont(ofSize: 11)))
        defer { w.close() }
        let nodes = ControlUI.tree(of: w, maxDepth: 64).nodes
        let n = try #require(nodes.first { $0.value == "DSP 12%" })
        #expect(n.role == "AXStaticText")
        // The width a label needs for a text, for a steady minimum width (Task 7 M5).
        let f = NSFont.monospacedSystemFont(ofSize: 15, weight: .regular)
        #expect(
            AppKitLabel.width(of: "999.4.960  00:00.000", font: f) > AppKitLabel.width(of: "1.1.0  0:00.000", font: f))
        let probe = NSTextField(labelWithString: "abc")
        probe.font = f
        #expect(AppKitLabel.width(of: "abc", font: f) == probe.intrinsicContentSize.width)
    }
}
