import Foundation
import Testing
@testable import VizDAWAppModel

@Suite struct PaneGeometryTests {
    @Test func theDividerCursorAlwaysPopsWhatItPushed() {
        var c = DividerCursor()
        #expect(c.hover(true) == .push)
        #expect(c.hover(true) == nil)
        #expect(c.hover(false) == .pop)
        #expect(c.hover(false) == nil)
        #expect(c.hover(true) == .push)
        #expect(c.disappear() == .pop)
        #expect(c.disappear() == nil)
        // The pointer leaves during a drag, which then ends outside.
        var d = DividerCursor()
        var net = 0
        for op in [d.hover(true), d.hover(false), d.dragEnded(inside: false)] {
            if op == .push { net += 1 } else if op == .pop { net -= 1 }
        }
        #expect(net == 0)
        var pops = 0
        var e = DividerCursor()
        _ = e.hover(true)
        for op in [e.hover(false), e.dragEnded(inside: false)] where op == .pop { pops += 1 }
        #expect(pops == 1)
        // Random sequences (fixed seed).
        var seed: UInt64 = 0x5EED
        func next() -> UInt64 {
            seed = seed &* 6364136223846793005 &+ 1442695040888963407
            return seed >> 33
        }
        for _ in 0..<200 {
            var r = DividerCursor()
            var depth = 0
            for _ in 0..<8 {
                let op: CursorOp?
                switch next() % 4 {
                case 0: op = r.hover(true)
                case 1: op = r.hover(false)
                case 2: op = r.dragEnded(inside: next() % 2 == 0)
                default: op = r.disappear()
                }
                if op == .push { depth += 1 } else if op == .pop { depth -= 1 }
                #expect(depth == 0 || depth == 1)
            }
            if r.disappear() == .pop { depth -= 1 }
            #expect(depth == 0)
        }
    }

    @Test func layoutAPutsTheLeftPaneFullHeightAndTheRightBesideTheBottom() {
        var s = ViewLayouts().arrange; s.right.shown = true   // left 220, bottom 260, right 260
        let f = PaneGeometry.frames(in: CGSize(width: 1400, height: 900), setup: s)
        #expect(f.left == CGRect(x: 0, y: 0, width: 220, height: 900))
        #expect(f.centre == CGRect(x: 221, y: 0, width: 1179, height: 639))
        #expect(f.bottom == CGRect(x: 221, y: 640, width: 918, height: 260))
        #expect(f.right == CGRect(x: 1140, y: 640, width: 260, height: 260))
        #expect(f.hidden.isEmpty)
    }

    @Test func onlyTheRightPaneRunsBesideTheCentre() {
        var s = ViewLayouts().arrange; s.bottom.shown = false; s.right.shown = true
        let f = PaneGeometry.frames(in: CGSize(width: 1400, height: 900), setup: s)
        #expect(f.bottom == nil)
        #expect(f.right == CGRect(x: 1140, y: 0, width: 260, height: 900))
        #expect(f.centre == CGRect(x: 221, y: 0, width: 918, height: 900))
    }

    @Test func hiddenPanesGiveTheCentreTheirSpace() {
        let f = PaneGeometry.frames(in: CGSize(width: 1000, height: 700), setup: ViewLayouts().mix)
        #expect(f.centre == CGRect(x: 0, y: 0, width: 1000, height: 700))
        #expect(f.left == nil && f.bottom == nil && f.right == nil)
    }

    @Test func aSmallWindowShrinksPanesBeforeTheCentre() {
        var s = ViewLayouts().arrange
        s.left.size = 480; s.bottom.size = 900; s.right.shown = true; s.right.size = 520
        let f = PaneGeometry.frames(in: CGSize(width: 700, height: 450), setup: s)
        #expect(f.centre.width >= 240 && f.centre.height >= 160)
        #expect(f.left?.width == 278 && f.right?.width == 180 && f.bottom?.height == 289)
        #expect((f.bottom?.width ?? 0) == PaneGeometry.bottomMinimumWidth)   // 421 − 1 − 180
        #expect(f.hidden.isEmpty)
        let narrow = PaneGeometry.frames(in: CGSize(width: 560, height: 450), setup: s)
        #expect(narrow.hidden == [.right])
        #expect(narrow.left?.width == 319 && narrow.bottom?.height == 289)
        let tiny = PaneGeometry.frames(in: CGSize(width: 300, height: 200), setup: s)
        #expect(tiny.hidden == [.right, .left, .bottom])
    }

    @Test func aDividerDragIsClamped() {
        let s = ViewLayouts().arrange
        let size = CGSize(width: 1400, height: 900)
        #expect(PaneGeometry.drag(s, side: .left, by: 10_000, in: size) == 480)
        #expect(PaneGeometry.drag(s, side: .left, by: -10_000, in: size) == 160)
        #expect(PaneGeometry.drag(s, side: .bottom, by: 40, in: size) == 300)   // dragging up grows it
        #expect(PaneGeometry.drag(s, side: .bottom, by: .nan, in: size) == 260)
    }

    /// Task 2 review M3: a divider's drag starts from the size the pane
    /// shows (shrunk in a small window), so its first change does not jump.
    @Test func aDividerDragStartsFromTheDisplayedSize() {
        var s = ViewLayouts().arrange
        s.left.size = 480; s.bottom.size = 900; s.right.shown = true; s.right.size = 520
        let size = CGSize(width: 700, height: 450)
        let f = PaneGeometry.frames(in: size, setup: s)
        #expect(f.size(.left) == 278 && f.size(.right) == 180 && f.size(.bottom) == 289)
        let shown = f.displayed(s)
        #expect(shown.left.size == 278 && shown.right.size == 180 && shown.bottom.size == 289)
        for side in PaneSide.allCases {
            #expect(PaneGeometry.drag(shown, side: side, by: 0, in: size) == f.size(side), "\(side): no jump")
        }
        #expect(PaneGeometry.drag(shown, side: .left, by: -20, in: size) == 258)
        #expect(PaneGeometry.drag(s, side: .left, by: 0, in: size) == 160, "seeded from the saved sizes it jumps")
        // A pane dropped for this frame counts as hidden; a hidden pane has no size.
        let narrow = PaneGeometry.frames(in: CGSize(width: 560, height: 450), setup: s)
        #expect(!narrow.displayed(s).right.shown && narrow.size(.right) == nil)
        let none = PaneGeometry.frames(in: CGSize(width: 1000, height: 700), setup: ViewLayouts().mix)
        #expect(none.size(.left) == nil && none.size(.bottom) == nil && none.size(.right) == nil)
    }

    /// Owner carry, 2026-11-05: a new document window opens at 1440×900 or
    /// 90% of the screen's visible frame, whichever is smaller, centred.
    @Test func aNewWindowOpensAtMostAt1440By900AndCentred() {
        let big = CGRect(x: 0, y: 0, width: 2560, height: 1415)
        #expect(PaneGeometry.defaultDocumentFrame(visible: big) == CGRect(x: 560, y: 257.5, width: 1440, height: 900))
        // A laptop: 90% of the visible frame (the menu bar and Dock are out of it).
        let laptop = CGRect(x: 0, y: 40, width: 1512, height: 900)
        let l = PaneGeometry.defaultDocumentFrame(visible: laptop)
        #expect(abs(l.width - 1360.8) < 1e-9 && abs(l.height - 810) < 1e-9)
        #expect(abs(l.minX - 75.6) < 1e-9 && abs(l.minY - 85) < 1e-9)
        // Each side is capped alone, on a second display too.
        let wide = CGRect(x: 1600, y: 0, width: 3000, height: 800)
        #expect(
            PaneGeometry.defaultDocumentFrame(visible: wide) == CGRect(x: 2380, y: 40, width: 1440, height: 720))
        // A small screen (1024×768 less the menu bar): not under the old
        // 980×620 default, 90% where that is more.
        let small = PaneGeometry.defaultDocumentFrame(visible: CGRect(x: 0, y: 0, width: 1024, height: 743))
        #expect(small.width == 980 && abs(small.height - 668.7) < 1e-9)
        // A screen under 980 wide: its own width, no more.
        #expect(PaneGeometry.defaultDocumentFrame(visible: CGRect(x: 0, y: 0, width: 800, height: 600)).width == 800)
        // Never under the window's own minimum.
        let tiny = CGRect(x: 0, y: 0, width: 640, height: 400)
        let r = PaneGeometry.defaultDocumentFrame(visible: tiny)
        #expect(r.width >= 700 && r.height >= 450)
        #expect(PaneGeometry.defaultDocumentFrame(visible: .zero).width >= 700)
    }

    @Test func aSavedFrameOffEveryScreenIsIgnored() {
        let screens = [CGRect(x: 0, y: 0, width: 1512, height: 982)]
        #expect(PaneGeometry.sanitizedFrame([100, 100, 1200, 800], screens: screens)
            == CGRect(x: 100, y: 100, width: 1200, height: 800))
        #expect(PaneGeometry.sanitizedFrame([5000, 5000, 1200, 800], screens: screens) == nil)
        #expect(PaneGeometry.sanitizedFrame([0, 0, 300, 200], screens: screens) == nil)
        #expect(PaneGeometry.sanitizedFrame([0, 0, .nan, 800], screens: screens) == nil)
        #expect(PaneGeometry.sanitizedFrame([0, 0, 1200], screens: screens) == nil)
        #expect(PaneGeometry.sanitizedFrame(nil, screens: screens) == nil)
    }
    /// Final review n1: saved on a display above the laptop, reopened
    /// without it: the bottom 100 pt are on screen but the title bar is not.
    @Test func aSavedFrameWithItsTitleBarOffScreenIsIgnored() {
        let screens = [CGRect(x: 0, y: 0, width: 1512, height: 982)]
        #expect(PaneGeometry.sanitizedFrame([100, 870, 1200, 800], screens: screens) == nil)
        #expect(PaneGeometry.sanitizedFrame([100, 170, 1200, 800], screens: screens)
            == CGRect(x: 100, y: 170, width: 1200, height: 800))
    }
}
