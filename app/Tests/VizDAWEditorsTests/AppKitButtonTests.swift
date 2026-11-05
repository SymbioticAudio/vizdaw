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
                AppKitButton("Loop", icon: .loop, isOn: true, help: "Loop") {}
                AppKitButton("Left Pane", icon: .leftPane, isOn: false, help: "Left Pane") {}
                AppKitButton("Add Bus", help: "Add a bus") {}
                AppKitButton("Record", icon: .record, enabled: false, help: "Record") {}
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
        let w = hosted(AppKitButton("Loop", icon: .loop, isOn: false, help: "Loop") { pressed += 1 })
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

/// The hold buttons (plan 7d Task B3): press, ticks while held, one release.
@MainActor
@Suite struct HoldButtonTests {
    final class Log {
        var events: [String] = []
    }

    func makeView(_ log: Log) -> HoldButtonView {
        let v = HoldButtonView(frame: NSRect(x: 0, y: 0, width: 30, height: 20))
        v.onPress = { log.events.append("press \($0)") }
        v.onTick = { log.events.append("tick") }
        v.onRelease = { log.events.append("release \($0)") }
        v.onCancel = { log.events.append("cancel") }
        return v
    }

    @Test func aHoldButtonPressesAndReleasesOnce() throws {
        let log = Log()
        let v = makeView(log)
        v.beginHold(shift: false)
        #expect(v.timerIsRunning)
        v.endHold()
        #expect(!v.timerIsRunning)
        #expect(log.events == ["press false", "release true"])
        // A second end is nothing; outside is reported.
        v.endHold()
        v.beginHold(shift: false)
        v.endHold(inside: false)
        #expect(log.events == ["press false", "release true", "press false", "release false"])
        #expect(!log.events.contains("cancel"))
        // A shift in the event's own flags.
        let e = try #require(NSEvent.mouseEvent(
            with: .leftMouseDown, location: .zero, modifierFlags: [.shift], timestamp: 0, windowNumber: 0, context: nil,
            eventNumber: 0, clickCount: 1, pressure: 1))
        #expect(HoldButtonView.shift(of: e))
        v.beginHold(shift: HoldButtonView.shift(of: e))
        v.endHold()
        #expect(log.events.suffix(2) == ["press true", "release true"])
    }

    /// A stalled main thread must not turn a hold into a click: a release
    /// delivers at least one tick first (B2 review M2).
    @Test func aReleaseDeliversAtLeastOneTickFirst() {
        let log = Log()
        let v = makeView(log)
        v.beginHold(shift: false)
        v.endHold()
        #expect(log.events == ["press false", "release true"], "a click: no tick was due")
        log.events = []
        v.beginHold(shift: false)
        v.holdStartedAt -= 1  // the press was long ago, the timer never fired
        v.endHold()
        #expect(log.events == ["press false", "tick", "release true"])
        log.events = []
        v.beginHold(shift: false)
        v.holdStartedAt -= 1
        v.timerFired()
        v.endHold()
        #expect(log.events == ["press false", "tick", "tick", "release true"], "a hold past 0.4 s ticks at its release")
    }

    /// The release is judged by the mouse-up that ended tracking, wherever
    /// the live pointer is (a channel click).
    @Test func aReleaseIsInsideByTheMouseUpEvent() throws {
        let w = NSWindow(
            contentRect: NSRect(x: 0, y: 0, width: 100, height: 100), styleMask: [.titled], backing: .buffered, defer: false)
        w.isReleasedWhenClosed = false
        defer { w.close() }
        let bounds = NSRect(x: 10, y: 10, width: 30, height: 20)
        func up(_ p: NSPoint, type: NSEvent.EventType = .leftMouseUp) throws -> NSEvent {
            try #require(
                NSEvent.mouseEvent(
                    with: type, location: p, modifierFlags: [], timestamp: 0, windowNumber: w.windowNumber, context: nil,
                    eventNumber: 0, clickCount: 1, pressure: 0))
        }
        let id: (NSPoint) -> NSPoint = { $0 }
        #expect(HoldButtonView.endedInside(try up(NSPoint(x: 20, y: 20)), in: w, bounds: bounds, locationInView: id))
        #expect(!HoldButtonView.endedInside(try up(NSPoint(x: 60, y: 20)), in: w, bounds: bounds, locationInView: id))
        // No event, a drag, or another window's event: not an outside.
        #expect(HoldButtonView.endedInside(nil, in: w, bounds: bounds, locationInView: id))
        #expect(HoldButtonView.endedInside(try up(NSPoint(x: 60, y: 20), type: .leftMouseDragged), in: w, bounds: bounds, locationInView: id))
        #expect(HoldButtonView.endedInside(try up(NSPoint(x: 60, y: 20)), in: nil, bounds: bounds, locationInView: id))
    }

    @Test func leavingTheWindowMidHoldCancels() {
        let log = Log()
        let v = makeView(log)
        v.beginHold(shift: false)
        v.cancelHold()
        #expect(!v.timerIsRunning)
        #expect(log.events == ["press false", "cancel"])
        v.cancelHold()
        #expect(log.events.count == 2)
    }

    @Test func anAccessibilityPressIsAClick() throws {
        let log = Log()
        let w = NSWindow(
            contentRect: NSRect(x: 0, y: 0, width: 200, height: 60), styleMask: [.titled], backing: .buffered, defer: false)
        w.isReleasedWhenClosed = false
        defer { w.close() }
        w.contentView = NSHostingView(
            rootView: AppKitHoldButton(
                "Rewind", icon: .rewind, enabled: true, help: "h",
                onPress: { log.events.append("press \($0)") }, onTick: { log.events.append("tick") },
                onRelease: { log.events.append("release \($0)") }, onCancel: { log.events.append("cancel") }))
        w.contentView?.layoutSubtreeIfNeeded()
        func find(_ v: NSView) -> HoldButtonView? {
            (v as? HoldButtonView) ?? v.subviews.lazy.compactMap { find($0) }.first
        }
        let root = try #require(w.contentView)
        let b = try #require(find(root))
        #expect(b.accessibilityPerformPress())
        #expect(log.events == ["press false", "release true"])
        #expect(b.refusesFirstResponder)
        let node = try #require(ControlUI.tree(of: w, maxDepth: 64).nodes.first { $0.label == "Rewind" })
        #expect(node.role == "AXButton" && node.actions == ["press"])
    }
}
