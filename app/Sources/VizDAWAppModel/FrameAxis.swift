import CoreGraphics
import Foundation

/// The audio editor's time axis (plan 7c Ruling 3): media frames to points.
/// `x` is relative to the waveform rect's left edge. The operations clamp;
/// `init` stores what it is given.
public struct FrameAxis: Equatable, Sendable {
    public private(set) var framesPerPoint: Double
    public private(set) var originFrame: Double

    /// The closest zoom: one frame per point.
    public static let minFramesPerPoint: Double = 1

    public init(framesPerPoint: Double = 512, originFrame: Double = 0) {
        self.framesPerPoint = framesPerPoint
        self.originFrame = originFrame
    }

    public func x(_ frame: Double) -> CGFloat { CGFloat((frame - originFrame) / framesPerPoint) }

    public func frame(atX x: CGFloat) -> Double { originFrame + Double(x) * framesPerPoint }

    /// The farthest zoom: the whole file in half the width.
    public static func maxFramesPerPoint(mediaFrames: UInt64, width: CGFloat) -> Double {
        max(1, 2 * Double(max(mediaFrames, 1)) / Double(max(width, 1)))
    }

    /// The region plus 5% of its length each side.
    public static func fit(offset: UInt64, length: UInt64, width: CGFloat, mediaFrames: UInt64) -> FrameAxis {
        let margin = Double(max(length / 20, 1))
        let visible = Double(length) + 2 * margin
        let fpp = min(
            max(visible / Double(max(width, 1)), minFramesPerPoint),
            maxFramesPerPoint(mediaFrames: mediaFrames, width: width))
        let axis = FrameAxis(framesPerPoint: fpp, originFrame: Double(offset) - margin)
        return axis.clamped(width: width, mediaFrames: mediaFrames)
    }

    /// `framesPerPoint / factor` (above 1 zooms in) keeping the frame under
    /// `x`; a factor that is not finite or ≤ 0 gives `self`.
    public func zoomed(by factor: Double, aroundX x: CGFloat, width: CGFloat, mediaFrames: UInt64) -> FrameAxis {
        guard factor.isFinite, factor > 0 else { return self }
        let fpp = min(
            max(framesPerPoint / factor, Self.minFramesPerPoint),
            Self.maxFramesPerPoint(mediaFrames: mediaFrames, width: width))
        let under = frame(atX: x)
        return FrameAxis(framesPerPoint: fpp, originFrame: under - Double(x) * fpp)
            .clamped(width: width, mediaFrames: mediaFrames)
    }

    /// Moves the view `dx` points right, the origin held within
    /// `−w·fpp/2 … mediaFrames − w·fpp/2`.
    public func scrolled(byPoints dx: CGFloat, width: CGFloat, mediaFrames: UInt64) -> FrameAxis {
        FrameAxis(framesPerPoint: framesPerPoint, originFrame: originFrame + Double(dx) * framesPerPoint)
            .clamped(width: width, mediaFrames: mediaFrames)
    }

    private func clamped(width: CGFloat, mediaFrames: UInt64) -> FrameAxis {
        let half = Double(max(width, 1)) * framesPerPoint / 2
        var out = self
        out.originFrame = min(max(originFrame, -half), Double(mediaFrames) - half)
        return out
    }
}
