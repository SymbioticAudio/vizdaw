import AppKit
import SwiftUI
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

    /// SwiftUI's hosting view takes no first click, and VizDAW under the
    /// channel is not the active app, so its windows are never key: a
    /// channel click on a strip's number box was spent on the window
    /// (plan 7b Task B8). Once the channel has aimed at it, the hosting
    /// view takes the channel's clicks as first clicks; a user's are as
    /// before.
    @Test func aHostingViewTakesTheChannelsFirstClick() throws {
        let w = NSWindow(
            contentRect: NSRect(x: 100, y: 100, width: 200, height: 100), styleMask: [.titled],
            backing: .buffered, defer: false)
        w.isReleasedWhenClosed = false
        let host = NSHostingView(rootView: Color.clear.frame(width: 200, height: 100))
        w.contentView = host
        host.layoutSubtreeIfNeeded()
        let click = ControlUI.clickEvents(w, at: CGPoint(x: 100, y: 60), button: .left, count: 1, modifiers: none)
        let user = try #require(
            NSEvent.mouseEvent(
                with: .leftMouseDown, location: NSPoint(x: 100, y: 40), modifierFlags: [],
                timestamp: click[0].timestamp + 0.5, windowNumber: w.windowNumber, context: nil, eventNumber: 2,
                clickCount: 1, pressure: 1))
        let before = host.acceptsFirstMouse(for: user)
        #expect(!host.acceptsFirstMouse(for: click[0]), "SwiftUI's own answer")
        ControlUI.remember(click)
        #expect(ControlUI.allowFirstMouse(click, in: w), "SwiftUI took no first click there: it may start editing")
        #expect(host.acceptsFirstMouse(for: click[0]))
        #expect(host.acceptsFirstMouse(for: user) == before, "a user's click is unchanged")
        #expect(ControlUI.allowFirstMouse(click, in: w), "taught once, answered alike")
        // A plain AppKit view keeps its own answer.
        let plain = NSView(frame: NSRect(x: 0, y: 0, width: 200, height: 100))
        w.contentView = plain
        #expect(!ControlUI.allowFirstMouse(click, in: w))
        #expect(!plain.acceptsFirstMouse(for: click[0]))
    }

    /// A channel click that starts a number box editing in a window that
    /// is not key leaves SwiftUI's field without the keyboard (plan 7b
    /// Task B8): the channel gives it the keyboard, as a click into a key
    /// window would.
    @Test func aClickedTextFieldGetsTheKeyboard() {
        let w = NSWindow(
            contentRect: NSRect(x: 100, y: 100, width: 200, height: 100), styleMask: [.titled],
            backing: .buffered, defer: false)
        w.isReleasedWhenClosed = false
        let field = NSTextField(frame: NSRect(x: 10, y: 10, width: 80, height: 22))
        let label = NSTextField(labelWithString: "read only")
        label.frame = NSRect(x: 100, y: 10, width: 80, height: 22)
        let plain = NSView(frame: NSRect(x: 0, y: 50, width: 200, height: 50))
        for v in [field, label, plain] { w.contentView?.addSubview(v) }
        let titleBar = w.frame.height - w.contentLayoutRect.height
        // Window points, top-left origin: the field's centre.
        let atField = CGPoint(x: 50, y: titleBar + 100 - 21)
        let atLabel = CGPoint(x: 140, y: titleBar + 100 - 21)
        let atPlain = CGPoint(x: 50, y: titleBar + 20)
        #expect(!ControlUI.focusTextField(at: atPlain, in: w), "no field there")
        #expect(!ControlUI.focusTextField(at: atLabel, in: w), "a label takes no typing")
        #expect(field.currentEditor() == nil)
        #expect(ControlUI.focusTextField(at: atField, in: w))
        #expect(field.currentEditor() != nil && w.firstResponder === field.currentEditor())
        #expect(ControlUI.focusTextField(at: atField, in: w), "it keeps it")
    }

    /// Final review m3: only a click on a number box (marked with
    /// `ControlUI.numberBoxID`) or a text field waits for a field to show;
    /// a fader, a knob, a toggle or a pane background answers at once.
    @Test func onlyANumberBoxOrTextFieldClickMayStartEditing() {
        typealias T = ControlUI.EditTarget
        let box = ControlUI.numberBoxID
        let targets = [
            T(frame: CGRect(x: 0, y: 0, width: 400, height: 300), role: "AXGroup", identifier: nil),
            T(frame: CGRect(x: 10, y: 10, width: 56, height: 24), role: "AXButton", identifier: box),
            T(frame: CGRect(x: 100, y: 10, width: 36, height: 200), role: "AXUnknown", identifier: nil),
            T(frame: CGRect(x: 200, y: 10, width: 24, height: 24), role: "AXButton", identifier: nil),
            T(frame: CGRect(x: 300, y: 10, width: 56, height: 24), role: "AXTextField", identifier: nil),
        ]
        #expect(ControlUI.mayEdit(targets, at: CGPoint(x: 30, y: 20)), "the number box")
        #expect(ControlUI.mayEdit(targets, at: CGPoint(x: 320, y: 20)), "a text field")
        #expect(!ControlUI.mayEdit(targets, at: CGPoint(x: 110, y: 100)), "a fader")
        #expect(!ControlUI.mayEdit(targets, at: CGPoint(x: 210, y: 20)), "a toggle (a button with a value)")
        #expect(!ControlUI.mayEdit(targets, at: CGPoint(x: 380, y: 280)), "the background")
        #expect(!ControlUI.mayEdit(targets, at: CGPoint(x: CGFloat.nan, y: 20)))
    }

    /// The marker reaches the channel's walk on a real SwiftUI element.
    @Test func aMarkedSwiftUIViewIsANumberBoxToTheChannel() {
        let w = NSWindow(
            contentRect: NSRect(x: 100, y: 100, width: 200, height: 100), styleMask: [.titled],
            backing: .buffered, defer: false)
        w.isReleasedWhenClosed = false
        let host = NSHostingView(
            rootView: HStack(spacing: 0) {
                Text("0.0 dB").frame(width: 100, height: 100)
                    .accessibilityElement(children: .ignore).accessibilityLabel("Level")
                    .accessibilityAddTraits(.isButton).accessibilityIdentifier(ControlUI.numberBoxID)
                Text("M").frame(width: 100, height: 100)
                    .accessibilityElement(children: .ignore).accessibilityLabel("Mute")
                    .accessibilityAddTraits(.isButton)
            })
        w.contentView = host
        host.layoutSubtreeIfNeeded()
        let titleBar = w.frame.height - w.contentLayoutRect.height
        #expect(ControlUI.mayEdit(at: CGPoint(x: 50, y: titleBar + 50), in: w), "the marked box")
        #expect(!ControlUI.mayEdit(at: CGPoint(x: 150, y: titleBar + 50), in: w), "the unmarked toggle")
    }

    /// B8 review m1: a focusing click that found no editable field says
    /// which view it hit.
    @Test func aClickWithNoFieldNamesTheViewItHit() {
        let (w, _, _) = window()
        let titleBar = w.frame.height - w.contentLayoutRect.height
        let note = ControlUI.noFieldNote(at: CGPoint(x: 50, y: titleBar + 250), in: w)
        #expect(note == "channel: no editable field under the click (hit: VizDAWEditorsTests.RecordingView)", "\(note)")
        #expect(ControlUI.noFieldNote(at: CGPoint(x: -50, y: -50), in: w).hasSuffix("(hit: none)"))
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

/// An element that answers the accessibility selectors without declaring
/// `NSAccessibilityProtocol`, as SwiftUI's `AccessibilityNode` does (plan
/// 7b Task B7: the strip's knobs, fader and number boxes are such nodes).
@MainActor
final class InformalNode: NSObject {
    let label: String
    let frame: NSRect
    init(label: String, frame: NSRect) {
        self.label = label
        self.frame = frame
    }
    @objc func accessibilityRole() -> String { "AXGroup" }
    @objc func accessibilityLabel() -> String { label }
    @objc func accessibilityValue() -> Any { "0.0 dB" }
    @objc func accessibilityFrame() -> NSRect { frame }
    @objc func accessibilityChildren() -> [Any] { [] }
    @objc func isAccessibilityEnabled() -> Bool { true }
}

/// The same through the attribute API only.
@MainActor
final class AttributeNode: NSObject {
    let label: String
    init(label: String) { self.label = label }
    @objc override func accessibilityAttributeValue(_ attribute: NSAccessibility.Attribute) -> Any? {
        switch attribute.rawValue {
        case "AXRole": return "AXButton"
        case "AXDescription": return label
        default: return nil
        }
    }
}

/// A view whose children are informal nodes.
@MainActor
final class InformalHost: NSView {
    var nodes: [Any] = []
    override func accessibilityChildren() -> [Any]? { nodes }
}

@MainActor
@Suite struct ControlUIInformalTests {
    @Test func theTreeReadsElementsThatDoNotDeclareTheProtocol() throws {
        let w = NSWindow(
            contentRect: NSRect(x: 100, y: 100, width: 400, height: 300), styleMask: [.titled],
            backing: .buffered, defer: false)
        w.isReleasedWhenClosed = false
        let host = InformalHost(frame: NSRect(x: 0, y: 0, width: 400, height: 300))
        w.contentView?.addSubview(host)
        let screen = w.convertToScreen(NSRect(x: 10, y: 20, width: 56, height: 24))
        host.nodes = [InformalNode(label: "Level of Synth", frame: screen), AttributeNode(label: "Old")]
        let nodes = ControlUI.tree(of: w, maxDepth: 64).nodes
        let level = try #require(nodes.first { $0.label == "Level of Synth" })
        #expect(level.role == "AXGroup" && level.value == "0.0 dB" && level.enabled)
        #expect(abs(level.width - 56) < 0.5 && abs(level.height - 24) < 0.5)
        let old = try #require(nodes.first { $0.label == "Old" })
        #expect(old.role == "AXButton" && old.actions == ["press"])
        let check = controlElementCheck(role: level.role, label: level.label, title: level.title)
        let ref = ElementRefRow(id: "", path: level.path, check: check)
        let p = try #require(ControlUI.point(.element(element: ref), in: w))
        let titleBar = w.frame.height - w.contentLayoutRect.height
        #expect(abs(p.x - 38) < 0.5 && abs(p.y - (titleBar + 300 - 32)) < 0.5, "\(p)")
    }
}
