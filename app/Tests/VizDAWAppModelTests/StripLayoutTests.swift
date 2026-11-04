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
