import AppKit
import QuartzCore
import Testing
@testable import VizDAWEditors

/// A flipped view that fills itself green in `draw(_:)`, with a blue strip
/// along its bottom edge.
@MainActor
final class FilledView: NSView {
    var fill = NSColor(srgbRed: 0, green: 1, blue: 0, alpha: 1)
    override var isFlipped: Bool { true }
    override func draw(_ dirtyRect: NSRect) {
        fill.setFill()
        bounds.fill()
        NSColor(srgbRed: 0, green: 0, blue: 1, alpha: 1).setFill()
        NSRect(x: 0, y: bounds.height - 5, width: bounds.width, height: 5).fill()
    }
}

@MainActor
@Suite struct WindowSnapshotTests {
    /// The dominant channel at `x`, `y` (row 0 is the top): "r", "g", "b"
    /// or "?" (colour management shifts exact values a little).
    func pixel(_ data: Data, _ x: Int, _ y: Int) -> String {
        guard let rep = NSBitmapImageRep(data: data), let c = rep.colorAt(x: x, y: y)?.usingColorSpace(.sRGB) else {
            return "none"
        }
        let (r, g, b) = (c.redComponent, c.greenComponent, c.blueComponent)
        if r > 0.8 && g < 0.4 && b < 0.4 { return "r" }
        if g > 0.8 && r < 0.4 && b < 0.4 { return "g" }
        if b > 0.8 && r < 0.4 && g < 0.4 { return "b" }
        return "?"
    }

    @Test func layerContentIsRenderedTheRightWayUp() throws {
        let v = NSView(frame: NSRect(x: 0, y: 0, width: 40, height: 20))
        v.wantsLayer = true
        let top = CALayer()
        top.frame = CGRect(x: 0, y: 10, width: 40, height: 10)  // y up: the top half
        top.backgroundColor = NSColor(srgbRed: 1, green: 0, blue: 0, alpha: 1).cgColor
        let bottom = CALayer()
        bottom.frame = CGRect(x: 0, y: 0, width: 40, height: 10)
        bottom.backgroundColor = NSColor(srgbRed: 0, green: 0, blue: 1, alpha: 1).cgColor
        v.layer?.addSublayer(top)
        v.layer?.addSublayer(bottom)
        let png = try #require(WindowSnapshot.png(of: v, scale: 2))
        #expect(png.width == 80 && png.height == 40)
        #expect(pixel(png.data, 5, 5) == "r", "row 0 is the top")
        #expect(pixel(png.data, 5, 35) == "b")
    }

    @Test func drawnFlippedViewsAndTheirSublayersAreIncluded() throws {
        let v = FilledView(frame: NSRect(x: 0, y: 0, width: 30, height: 30))
        v.wantsLayer = true
        let mark = CALayer()
        mark.frame = CGRect(x: 0, y: 0, width: 30, height: 5)  // flipped: the top strip
        mark.backgroundColor = NSColor(srgbRed: 1, green: 0, blue: 0, alpha: 1).cgColor
        v.layer?.addSublayer(mark)
        let png = try #require(WindowSnapshot.png(of: v, scale: 1))
        #expect(pixel(png.data, 15, 15) == "g", "draw(_:) content")
        #expect(pixel(png.data, 15, 28) == "b", "drawn at the bottom, shown at the bottom")
        #expect(pixel(png.data, 15, 1) == "r", "a flipped view's sublayer at y 0 is at its top")
    }

    @Test func aWindowIsRenderedWholeAtItsScale() throws {
        let w = NSWindow(
            contentRect: NSRect(x: 0, y: 0, width: 200, height: 100), styleMask: [.titled],
            backing: .buffered, defer: false)
        w.isReleasedWhenClosed = false
        let content = FilledView(frame: NSRect(x: 0, y: 0, width: 200, height: 100))
        w.contentView = content
        let png = try #require(WindowSnapshot.png(of: w, scale: 1))
        #expect(png.width == Int(w.frame.width) && png.height == Int(w.frame.height), "title bar included")
        #expect(pixel(png.data, 100, png.height - 50) == "g", "the content is in it")
    }

    @Test func aStaleLayerShowsWhatItsViewDrawsNow() throws {
        // AppKit does not redraw occluded windows: a layer can hold an old
        // picture. The snapshot draws self-drawing views again.
        let parent = NSView(frame: NSRect(x: 0, y: 0, width: 40, height: 40))
        parent.wantsLayer = true
        let v = FilledView(frame: NSRect(x: 0, y: 0, width: 40, height: 40))
        v.wantsLayer = true
        parent.addSubview(v)
        v.layer?.display()
        #expect(v.layer?.contents != nil, "drawn once into its layer")
        v.fill = NSColor(srgbRed: 1, green: 0, blue: 0, alpha: 1)  // no needsDisplay: the layer is stale
        let png = try #require(WindowSnapshot.png(of: parent, scale: 1))
        #expect(pixel(png.data, 20, 10) == "r", "what the view draws now")
        #expect(pixel(png.data, 20, 38) == "b", "the right way up")
    }

    @Test func aViewThatNeverDrewShowsWhatItDrawsNow() throws {
        // A view made while its window was occluded has never drawn: its
        // layer has no contents, yet the snapshot shows it (final review I2).
        let parent = NSView(frame: NSRect(x: 0, y: 0, width: 40, height: 40))
        parent.wantsLayer = true
        let v = FilledView(frame: NSRect(x: 0, y: 0, width: 40, height: 40))
        v.wantsLayer = true
        parent.addSubview(v)
        #expect(v.layer?.contents == nil, "never drawn")
        let png = try #require(WindowSnapshot.png(of: parent, scale: 1))
        #expect(pixel(png.data, 20, 10) == "g", "what the view draws")
        #expect(pixel(png.data, 20, 38) == "b", "the right way up")
    }

    @Test func aSelfDrawnViewInAScrollViewStaysInsideIt() throws {
        let root = NSView(frame: NSRect(x: 0, y: 0, width: 100, height: 100))
        root.wantsLayer = true
        let scroll = NSScrollView(frame: NSRect(x: 0, y: 60, width: 40, height: 40))  // top-left
        let doc = FilledView(frame: NSRect(x: 0, y: 0, width: 300, height: 300))
        doc.wantsLayer = true
        scroll.documentView = doc
        root.addSubview(scroll)
        doc.layer?.display()
        let png = try #require(WindowSnapshot.png(of: root, scale: 1))
        #expect(pixel(png.data, 20, 20) == "g", "inside the scroll view")
        #expect(pixel(png.data, 70, 20) != "g", "right of it")
        #expect(pixel(png.data, 20, 70) != "g", "below it")
        #expect(pixel(png.data, 70, 70) != "g")
    }

    @Test func badScalesReturnNil() {
        let v = NSView(frame: NSRect(x: 0, y: 0, width: 40, height: 20))
        v.wantsLayer = true
        for s in [CGFloat.nan, .infinity, -.infinity, 0, -1, 1e9, 1e300] {
            #expect(WindowSnapshot.png(of: v, scale: s) == nil, "scale \(s)")
        }
        let big = NSView(frame: NSRect(x: 0, y: 0, width: 10_000, height: 10_000))
        #expect(WindowSnapshot.png(of: big, scale: 1) == nil, "over 64M pixels")
    }
}
