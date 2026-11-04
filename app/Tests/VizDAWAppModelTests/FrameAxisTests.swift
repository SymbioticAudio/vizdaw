import CoreGraphics
import Foundation
import Testing

@testable import VizDAWAppModel

/// Plan 7c Ruling 3: the audio editor's time axis, in media frames.
@Suite struct FrameAxisTests {
    let media: UInt64 = 96_000
    let width: CGFloat = 600

    private func fitted() -> FrameAxis {
        FrameAxis.fit(offset: 24_000, length: 48_000, width: width, mediaFrames: media)
    }

    @Test func zoomStaysInRange() {
        var axis = fitted()
        for _ in 0..<10 { axis = axis.zoomed(by: 1000, aroundX: 300, width: width, mediaFrames: media) }
        #expect(axis.framesPerPoint == 1)
        for _ in 0..<10 { axis = axis.zoomed(by: 0.001, aroundX: 300, width: width, mediaFrames: media) }
        #expect(axis.framesPerPoint == FrameAxis.maxFramesPerPoint(mediaFrames: media, width: width))
        #expect(axis.framesPerPoint == 320)
        let start = fitted()
        for bad in [Double.nan, 0, -2, .infinity] {
            #expect(start.zoomed(by: bad, aroundX: 300, width: width, mediaFrames: media) == start)
        }
    }

    @Test func fitShowsTheRegionWithMargins() {
        let axis = fitted()
        #expect(axis.framesPerPoint == 88)
        #expect(axis.originFrame == 21_600)
        #expect(abs(axis.x(24_000) - 27.27) < 0.01)
        #expect(abs(axis.x(72_000) - 572.73) < 0.01)
        let tiny = FrameAxis.fit(offset: 10, length: 1, width: width, mediaFrames: media)
        #expect(tiny.framesPerPoint == 1)
        #expect(tiny.originFrame == 9)
    }

    @Test func zoomKeepsTheFrameUnderThePointer() {
        let before = fitted()
        let after = before.zoomed(by: 2, aroundX: 200, width: width, mediaFrames: media)
        #expect(abs(before.frame(atX: 200) - after.frame(atX: 200)) < 1e-9)
        #expect(after.framesPerPoint == 44)
    }

    @Test func scrollStopsAtTheFileEdges() {
        let axis = fitted()
        #expect(axis.scrolled(byPoints: -1e9, width: width, mediaFrames: media).originFrame == -300 * 88)
        #expect(axis.scrolled(byPoints: 1e9, width: width, mediaFrames: media).originFrame == 96_000 - 300 * 88)
    }
}
