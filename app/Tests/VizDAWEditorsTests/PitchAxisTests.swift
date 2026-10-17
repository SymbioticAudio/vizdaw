import CoreGraphics
import Testing
@testable import VizDAWEditors

@Suite struct PitchAxisTests {
    @Test func rowsRunFromTheTop() {
        let a = PitchAxis(rowHeight: 10, offset: 0)
        #expect(a.top(127) == 0)
        #expect(a.top(0) == 1_270)
        #expect(a.center(60) == 675)
        #expect(a.contentHeight == 1_280)
    }

    @Test func pitchAtYClampsAndFollowsTheOffset() {
        let a = PitchAxis(rowHeight: 10, offset: 600)
        #expect(a.pitch(atY: 75) == 60)
        #expect(a.pitch(atY: -1_000) == 127)
        #expect(a.pitch(atY: 9_999) == 0)
        #expect(a.pitch(atY: .nan) == 60)
    }

    @Test func semitonesAreWholeRowsUpPositive() {
        let a = PitchAxis(rowHeight: 10, offset: 3)
        #expect(a.semitones(fromY: 100, toY: 81) == 2)
        #expect(a.semitones(fromY: 100, toY: 104) == 0)
        #expect(a.semitones(fromY: 100, toY: 135) == -3)
        #expect(a.semitones(fromY: 0, toY: -5_000) > 127, "unclamped: the drag clamps the group")
    }

    @Test func zoomKeepsThePitchUnderTheAnchor() {
        let a = PitchAxis(rowHeight: 10, offset: 600)
        let z = a.withRowHeight(20, keepingY: 75)
        #expect(z.rowHeight == 20)
        #expect(z.pitch(atY: 75) == 60)
        #expect(PitchAxis(rowHeight: 999).rowHeight == PitchAxis.maxRowHeight)
        #expect(PitchAxis(rowHeight: .nan).rowHeight == 10)
    }

    @Test func centeringClampsToTheContent() {
        let a = PitchAxis(rowHeight: 10)
        let c = a.centering(60, height: 200)
        #expect(abs(c.center(60) - 100) < 0.001)
        #expect(a.centering(127, height: 200).offset == 0)
        #expect(a.centering(0, height: 200).offset == 1_080)
        #expect(a.clamped(height: 2_000).offset == 0, "taller than the content")
    }

    @Test func visiblePitches() {
        let a = PitchAxis(rowHeight: 10, offset: 600)
        #expect(a.visible(height: 100) == 58...67)
    }

    @Test func valueAxisRoundTripsAndClamps() {
        let v = ValueAxis(height: 96, inset: 3)
        #expect(v.y(UInt32.max) == 3)
        #expect(v.y(0) == 93)
        #expect(v.value(atY: 3) == UInt32.max)
        #expect(v.value(atY: 93) == 0)
        #expect(v.value(atY: -50) == UInt32.max)
        #expect(v.value(atY: 500) == 0)
        let mid = v.value(atY: v.y(0x8000_0000))
        #expect(mid > 0x7F00_0000 && mid < 0x8100_0000)
    }
}
