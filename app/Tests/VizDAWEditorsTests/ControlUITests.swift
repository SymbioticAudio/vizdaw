import AppKit
import Testing
import VizDAWCore
@testable import VizDAWEditors

/// Records the events it gets, in its own (flipped) coordinates.
@MainActor
final class RecordingView: NSView {
    var got: [String] = []
    override var isFlipped: Bool { true }
    override var acceptsFirstResponder: Bool { true }
    override func acceptsFirstMouse(for event: NSEvent?) -> Bool { true }
    private func note(_ name: String, _ e: NSEvent) {
        let p = convert(e.locationInWindow, from: nil)
        got.append("\(name) \(Int(p.x.rounded())),\(Int(p.y.rounded()))")
    }
    override func mouseDown(with e: NSEvent) { note("down", e) }
    override func mouseDragged(with e: NSEvent) { note("drag", e) }
    override func mouseUp(with e: NSEvent) { note("up", e) }
    override func rightMouseDown(with e: NSEvent) { note("rdown", e) }
    override func keyDown(with e: NSEvent) {
        got.append("key \(e.keyCode) \(e.characters ?? "") \(e.modifierFlags.contains(.command) ? "cmd" : "")")
    }
}

let none = ModifiersRow(shift: false, control: false, option: false, command: false)

@MainActor
@Suite struct ControlUITests {
    /// A 400×300 window: a canvas with two items over the top half, a
    /// recording view over the bottom half.
    func window() -> (NSWindow, ItemsCanvas, RecordingView) {
        let w = NSWindow(
            contentRect: NSRect(x: 100, y: 100, width: 400, height: 300), styleMask: [.titled],
            backing: .buffered, defer: false)
        w.isReleasedWhenClosed = false
        let c = ItemsCanvas(frame: NSRect(x: 0, y: 150, width: 400, height: 150))
        let r = RecordingView(frame: NSRect(x: 0, y: 0, width: 400, height: 150))
        w.contentView?.addSubview(c)
        w.contentView?.addSubview(r)
        c.layout()
        return (w, c, r)
    }

    @Test func theTreeListsTheCanvasItemsWithCheckedPaths() throws {
        let (w, _, _) = window()
        let (nodes, truncated) = ControlUI.tree(of: w, maxDepth: 64)
        #expect(!truncated)
        let chords = try #require(nodes.first { $0.label == "Block Chords, bars 1 to 2" })
        #expect(chords.role == "AXButton" && chords.selected && chords.actions == ["press"])
        let check = controlElementCheck(role: chords.role, label: chords.label, title: chords.title)
        let ref = ElementRefRow(id: "", path: chords.path, check: check)
        #expect(ControlUI.element(ref, in: w) != nil)
        let stale = ElementRefRow(id: "", path: chords.path, check: check ^ 1)
        #expect(ControlUI.element(stale, in: w) == nil, "a changed element is not clicked")
        let gone = ElementRefRow(id: "", path: chords.path + [99], check: check)
        #expect(ControlUI.element(gone, in: w) == nil)
        // The item's frame is in window points, top-left origin: the canvas
        // fills the top half of the content, under the title bar.
        let titleBar = w.frame.height - w.contentLayoutRect.height
        #expect(abs(chords.y - (titleBar + EditorCanvas.rulerHeight)) < 1, "\(chords.y)")
        let p = try #require(ControlUI.point(.element(element: ref), in: w))
        #expect(abs(p.x - 25) < 1)
        #expect(ControlUI.tree(of: w, maxDepth: 1).nodes.allSatisfy { $0.path.count == 1 })
    }

    /// Where `events` land in `view`, as "type x,y" (the window routes a
    /// mouse event by its location; `ControlUI.post` queues them).
    func landing(_ events: [NSEvent], in view: NSView) -> [String] {
        events.map { e in
            let p = view.convert(e.locationInWindow, from: nil)
            let name: String =
                switch e.type {
                case .leftMouseDown: "down"
                case .leftMouseUp: "up"
                case .leftMouseDragged: "drag"
                case .rightMouseDown: "rdown"
                case .rightMouseUp: "rup"
                default: "?"
                }
            return "\(name) \(Int(p.x.rounded())),\(Int(p.y.rounded()))"
        }
    }

    @Test func clicksAndDragsLandWhereTheyAreAimed() {
        let (w, _, r) = window()
        let titleBar = w.frame.height - w.contentLayoutRect.height
        let top = titleBar + 150  // the recording view's top edge, window points
        let clicks = ControlUI.clickEvents(w, at: CGPoint(x: 30, y: top + 20), button: .left, count: 2, modifiers: none)
        #expect(landing(clicks, in: r) == ["down 30,20", "up 30,20", "down 30,20", "up 30,20"])
        #expect(clicks.map(\.clickCount) == [1, 1, 2, 2], "the second press is a double click")
        #expect(clicks.allSatisfy { $0.windowNumber == w.windowNumber })
        let right = ControlUI.clickEvents(w, at: CGPoint(x: 5, y: top + 5), button: .right, count: 1, modifiers: none)
        #expect(landing(right, in: r) == ["rdown 5,5", "rup 5,5"])
        let drag = ControlUI.dragEvents(
            w, from: CGPoint(x: 10, y: top + 10), to: CGPoint(x: 50, y: top + 30), steps: 2,
            modifiers: ModifiersRow(shift: false, control: false, option: true, command: false))
        #expect(drag.allSatisfy { $0.modifierFlags.contains(.option) })
        #expect(landing(drag, in: r) == ["down 10,10", "drag 30,20", "drag 50,30", "up 50,30"])
        // Delivered, the recording view gets them where they were aimed.
        w.makeFirstResponder(r)
        r.mouseDown(with: drag[0])
        #expect(r.got == ["down 10,10"])
    }

    @Test func keyStrokesReachTheFirstResponder() {
        let (w, _, r) = window()
        w.makeFirstResponder(r)
        let cmdS = KeyStrokeRow(
            keyCode: 1, characters: "s", unmodified: "s",
            modifiers: ModifiersRow(shift: false, control: false, option: false, command: true))
        let space = KeyStrokeRow(keyCode: 49, characters: " ", unmodified: " ", modifiers: none)
        let events = ControlUI.keyEvents(w, [space, cmdS])
        #expect(events.map(\.type) == [NSEvent.EventType.keyDown, .keyUp, .keyDown, .keyUp])
        ControlUI.deliver(events, to: w)
        #expect(r.got == ["key 49   ", "key 1 s cmd"])
    }

    @Test func onlyChannelEventsAreFirstClicks() throws {
        let (w, c, _) = window()
        let channel = ControlUI.clickEvents(w, at: CGPoint(x: 5, y: 5), button: .left, count: 1, modifiers: none)
        #expect(c.acceptsFirstMouse(for: channel[0]))
        // Delivered (nextEvent needs a running app, so this goes straight to the window).
        ControlUI.deliver(channel, to: w)
        // After the channel's events are delivered, a user's click is not one.
        let user = try #require(
            NSEvent.mouseEvent(
                with: .leftMouseDown, location: NSPoint(x: 5, y: 5), modifierFlags: [], timestamp: 0,
                windowNumber: w.windowNumber, context: nil, eventNumber: 7, clickCount: 1, pressure: 1))
        #expect(!ControlUI.isChannel(user))
        #expect(!c.acceptsFirstMouse(for: user))
        #expect(!c.acceptsFirstMouse(for: nil))
    }

    @Test func aPostedEventRenumberedByAppKitIsStillTheChannels() throws {
        let (w, c, _) = window()
        let channel = ControlUI.clickEvents(w, at: CGPoint(x: 5, y: 5), button: .left, count: 1, modifiers: none)
        ControlUI.remember(channel)
        // What AppKit dispatches after `postEvent`: same timestamp, a small number.
        let dispatched = try #require(
            NSEvent.mouseEvent(
                with: .leftMouseDown, location: NSPoint(x: 5, y: 5), modifierFlags: [],
                timestamp: channel[0].timestamp, windowNumber: w.windowNumber, context: nil, eventNumber: 1,
                clickCount: 1, pressure: 1))
        #expect(ControlUI.isChannel(dispatched))
        #expect(c.acceptsFirstMouse(for: dispatched))
        let user = try #require(
            NSEvent.mouseEvent(
                with: .leftMouseDown, location: NSPoint(x: 5, y: 5), modifierFlags: [],
                timestamp: channel[0].timestamp + 0.5, windowNumber: w.windowNumber, context: nil, eventNumber: 2,
                clickCount: 1, pressure: 1))
        #expect(!ControlUI.isChannel(user), "a user's click at another time is not")
        // Only the newest are kept.
        for _ in 0..<(ControlUI.postedLimit / 2 + 1) {
            ControlUI.remember(ControlUI.clickEvents(w, at: .zero, button: .left, count: 1, modifiers: none))
        }
        #expect(!ControlUI.isChannel(dispatched))
    }

    @Test func theLargestChannelGestureFitsWhatIsRemembered() throws {
        let (w, _, _) = window()
        #expect(ControlUI.postedLimit >= ControlUI.maxGestureEvents)
        #expect(ControlUI.maxGestureEvents >= 3 * 2, "a triple click")
        let drag = ControlUI.dragEvents(
            w, from: CGPoint(x: 5, y: 5), to: CGPoint(x: 50, y: 50), steps: 200, modifiers: none)
        #expect(drag.count == ControlUI.maxGestureEvents)
        ControlUI.remember(drag)
        // The press as AppKit dispatches it after `postEvent` (renumbered).
        let down = try #require(
            NSEvent.mouseEvent(
                with: .leftMouseDown, location: NSPoint(x: 5, y: 5), modifierFlags: [],
                timestamp: drag[0].timestamp, windowNumber: w.windowNumber, context: nil, eventNumber: 3,
                clickCount: 1, pressure: 1))
        #expect(ControlUI.isChannel(down))
        // A larger request degrades to a coarser drag that still fits.
        let huge = ControlUI.dragEvents(
            w, from: CGPoint(x: 5, y: 5), to: CGPoint(x: 50, y: 50), steps: 100_000, modifiers: none)
        #expect(huge.count == ControlUI.postedLimit)
        #expect(huge.first?.type == .leftMouseDown && huge.last?.type == .leftMouseUp)
    }

    @Test func aClickPointNeedsAVisibleFiniteElement() throws {
        let (w, _, _) = window()
        #expect(ControlUI.point(.point(x: .nan, y: 3), in: w) == nil)
        #expect(ControlUI.point(.point(x: 3, y: .infinity), in: w) == nil)
        #expect(ControlUI.point(.point(x: 3, y: 4), in: w) == CGPoint(x: 3, y: 4))
        let item = try #require(ControlUI.tree(of: w, maxDepth: 64).nodes.first { $0.label == "Block Chords, bars 1 to 2" })
        let ref = ElementRefRow(
            id: "", path: item.path, check: controlElementCheck(role: item.role, label: item.label, title: item.title))
        let e = try #require(ControlUI.element(ref, in: w) as? NSAccessibilityElement)
        let original = e.accessibilityFrameInParentSpace()
        func at(_ r: NSRect) -> CGPoint? {
            e.setAccessibilityFrameInParentSpace(r)
            return ControlUI.point(.element(element: ref), in: w)
        }
        defer { e.setAccessibilityFrameInParentSpace(original) }
        #expect(at(original) != nil)
        #expect(at(NSRect(x: original.minX, y: original.minY, width: 0, height: 10)) == nil, "empty")
        #expect(at(NSRect(x: original.minX, y: original.minY, width: .nan, height: 10)) == nil, "not finite")
        #expect(at(NSRect(x: original.minX + 5000, y: original.minY, width: 20, height: 20)) == nil, "outside")
        #expect(at(NSRect(x: original.minX, y: original.minY - 5000, width: 20, height: 20)) == nil, "above")
    }
}
