import CoreGraphics
import Testing
@testable import VizDAWAppModel

@Suite struct StripLayoutTests {
    @Test func theFaderGetsALongThrowWhenThereIsRoom() {
        let s = StripLayout.split(height: 900, upper: 300)
        #expect(s.throwLength == min(480, 900 - 300 - StripLayout.lowerFixed))
        #expect(!s.scrolls)
    }

    @Test func aShortPaneScrollsTheStripAndKeepsTheMinimumThrow() {
        #expect(StripLayout.split(height: 260, upper: 300) == StripSplit(throwLength: 160, scrolls: true))
        #expect(StripLayout.split(height: 120, upper: 0) == StripSplit(throwLength: 160, scrolls: true))
        let edge = StripLayout.split(height: 300 + StripLayout.lowerFixed + 160, upper: 300)
        #expect(edge == StripSplit(throwLength: 160, scrolls: false))
    }

    @Test func theDbScaleMarksSitWhereTheFaderPutsThem() {
        let m = StripLayout.faderMarks
        #expect(m.map(\.label) == ["+12", "+6", "0", "\u{2212}6", "\u{2212}12", "\u{2212}24", "\u{2212}36", "\u{2212}48", "\u{2212}\u{221E}"])
        for (a, b) in zip(m, m.dropFirst()) { #expect(a.position > b.position) }
        #expect(m[2].position == 60.0 / 72)
        #expect(m.last?.position == 0)
        #expect(m.filter { $0.position == 0 }.count == 1)
        #expect(StripLayout.width == 120 && StripLayout.minimumThrow == 160 && StripLayout.maximumThrow == 480)
    }
}

/// B6 review carries (plan 7b Task B7): the fader's cap and the level box's
/// 24-point hit frame are in the strip's fixed height, so a short pane
/// scrolls rather than overflows.
@Suite struct StripHeightTests {
    @Test func theFixedLowerPartHoldsTheFaderCapAndTheLevelBoxHitFrame() {
        #expect(StripLayout.faderCap == 52)
        #expect(StripLayout.levelBoxHeight == 24)
        #expect(StripLayout.toggleRowHeight == 24)
        #expect(StripLayout.modeMenuHeight == 22)
        let parts = StripLayout.faderCap + StripLayout.levelBoxHeight + StripLayout.toggleRowHeight
            + StripLayout.modeMenuHeight + 4 * StripLayout.gap + 2 * StripLayout.padding
        #expect(StripLayout.lowerFixed == parts)
        #expect(StripLayout.contentHeight(upper: 300, throwLength: 160) == 300 + 160 + parts)
    }

    @Test func atEveryPaneHeightTheStripFitsOrScrolls() {
        for upper in stride(from: CGFloat(0), through: 640, by: 37) {
            for height in stride(from: CGFloat(0), through: 1400, by: 7) {
                let s = StripLayout.split(height: height, upper: upper)
                let content = StripLayout.contentHeight(upper: upper, throwLength: s.throwLength)
                #expect(s.scrolls || content <= height, "height \(height), upper \(upper)")
            }
        }
        // At the shortest pane that does not scroll, the content fills it exactly.
        let upper: CGFloat = 300
        let minimum = upper + StripLayout.lowerFixed + StripLayout.minimumThrow
        let s = StripLayout.split(height: minimum, upper: upper)
        #expect(!s.scrolls)
        #expect(StripLayout.contentHeight(upper: upper, throwLength: s.throwLength) == minimum)
        #expect(StripLayout.split(height: minimum - 1, upper: upper).scrolls)
    }
}
