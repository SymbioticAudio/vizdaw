import AppKit
import Testing
@testable import VizDAWEditors

@MainActor
final class ItemsCanvas: EditorCanvas {
    var pressed = 0
    override func accessibilityItems() -> [CanvasAccessibilityItem] {
        [
            CanvasAccessibilityItem(
                label: "Block Chords, bars 1 to 2", frame: CGRect(x: 0, y: 0, width: 50, height: 20),
                selected: true, press: { [unowned self] in self.pressed += 1 }),
            CanvasAccessibilityItem(
                label: "Block Bass, bar 3", frame: CGRect(x: 60, y: 0, width: 50, height: 20),
                selected: false, press: nil),
        ]
    }
}

@MainActor
final class AxisItemsCanvas: EditorCanvas {
    override func accessibilityItems() -> [CanvasAccessibilityItem] {
        [CanvasAccessibilityItem(
            label: "Block", frame: CGRect(x: axis.x(38_400), y: 0, width: 10, height: 10),
            selected: false, press: nil)]
    }
}

@MainActor
@Suite struct EditorCanvasTests {
    @Test func theRulerSitsAboveTheContentLayers() {
        let c = EditorCanvas(frame: NSRect(x: 0, y: 0, width: 400, height: 300))
        c.layout()
        #expect(c.ruler.frame == NSRect(x: 0, y: 0, width: 400, height: EditorCanvas.rulerHeight))
        #expect(c.content.frame == NSRect(x: 0, y: 22, width: 400, height: 278))
        #expect(c.grid.frame == c.content.frame && c.overlay.frame == c.content.frame)
        #expect(c.content.hitTest(NSPoint(x: 10, y: 10)) == nil, "events go to the canvas")
    }

    @Test func itIsAnAccessibilityContainerOfItsItems() {
        let c = ItemsCanvas(frame: NSRect(x: 0, y: 0, width: 400, height: 300))
        c.layout()
        #expect(c.accessibilityRole() == .group)
        let kids = (c.accessibilityChildren() ?? []).compactMap { $0 as? NSAccessibilityElement }
        #expect(kids.map { $0.accessibilityLabel() ?? "" } == ["Block Chords, bars 1 to 2", "Block Bass, bar 3"])
        #expect(kids[0].isAccessibilitySelected())
        #expect(kids[0].accessibilityPerformPress())
        #expect(c.pressed == 1)
        #expect(!kids[1].accessibilityPerformPress())
    }

    @Test func thePlayheadFollowsTheSourceWithoutAnimation() {
        let c = EditorCanvas(frame: NSRect(x: 0, y: 0, width: 400, height: 300))
        c.layout()
        c.axis = TimeAxis(ticksPerPoint: 100, originTick: 0)
        c.playheadSource = { _ in 10_000 }
        c.setPlaying(false)
        #expect(!c.playheadLayer.isHidden)
        #expect(c.playheadLayer.frame.minX == 100)
        c.playheadSource = { _ in nil }
        c.setPlaying(false)
        #expect(c.playheadLayer.isHidden)
    }

    @Test func theDisplayLinkRunsOnlyWhileTheCanvasIsInAWindow() {
        let w = NSWindow(
            contentRect: NSRect(x: 0, y: 0, width: 400, height: 300), styleMask: [.titled],
            backing: .buffered, defer: true)
        w.isReleasedWhenClosed = false
        let c = EditorCanvas(frame: NSRect(x: 0, y: 0, width: 400, height: 300))
        c.setPlaying(true)
        #expect(!c.isLinkRunning, "no window, no link")
        w.contentView?.addSubview(c)
        #expect(c.isLinkRunning)
        c.removeFromSuperview()
        #expect(!c.isLinkRunning, "it must not keep ticking or retained after leaving the window")
        w.contentView?.addSubview(c)
        #expect(c.isLinkRunning, "playback still on: it restarts")
        c.setPlaying(false)
        #expect(!c.isLinkRunning)
    }

    @Test func accessibilityElementsKeepTheirIdentityAndFollowTheAxis() {
        let c = AxisItemsCanvas(frame: NSRect(x: 0, y: 0, width: 400, height: 300))
        c.layout()
        c.axis = TimeAxis(ticksPerPoint: 384, originTick: 0)
        let a = c.accessibilityChildren() as? [NSAccessibilityElement] ?? []
        let b = c.accessibilityChildren() as? [NSAccessibilityElement] ?? []
        #expect(a.count == 1 && a.first === b.first)
        #expect(a[0].accessibilityFrameInParentSpace().minX == 100)
        c.axis = TimeAxis(ticksPerPoint: 384, originTick: 38_400 - 384 * 20)
        let d = c.accessibilityChildren() as? [NSAccessibilityElement] ?? []
        #expect(d.first === a.first)
        #expect(d[0].accessibilityFrameInParentSpace().minX == 20)
    }

    @Test func aCanvasLeavingItsWindowIsReleased() {
        let w = NSWindow(
            contentRect: NSRect(x: 0, y: 0, width: 400, height: 300), styleMask: [.titled],
            backing: .buffered, defer: true)
        w.isReleasedWhenClosed = false
        weak var weakCanvas: EditorCanvas?
        var stopped = false
        autoreleasepool {
            let c = EditorCanvas(frame: NSRect(x: 0, y: 0, width: 400, height: 300))
            weakCanvas = c
            w.contentView?.addSubview(c)
            c.setPlaying(true)
            c.removeFromSuperview()
            stopped = !c.isLinkRunning
        }
        #expect(stopped)
        #expect(weakCanvas == nil, "no display link retains it")
    }
}
