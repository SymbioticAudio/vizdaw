import Foundation
import Testing
@testable import VizDAWAppModel

@Suite struct PaneGeometryTests {
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
}
