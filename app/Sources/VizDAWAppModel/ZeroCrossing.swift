import Foundation
import VizDAWCore

/// Zero-crossing snapping for audio trims (plan release-fixes Task 7): a
/// trimmed start or end moves to the nearest frame where the channel-summed
/// signal crosses zero, so the cut does not click.
@MainActor public enum ZeroCrossing {
    /// How far each side of a trim the core looks.
    public static let radiusSeconds = 0.003

    /// The preferences the snapper reads: the app's, set at launch (nil:
    /// the default, on).
    public static var preferences: EditingPreferences?

    /// Identity when the preference is off; else session.zeroCrossing(media:frame:radius:)
    /// with radius = radiusSeconds × the media's rate; an error keeps the frame.
    public static func snapper(_ m: DocumentModel, media: UInt64) -> (UInt64) -> UInt64 {
        guard preferences?.zeroCrossings ?? true, let rate = m.media.row(media)?.sampleRate, rate > 0 else {
            return { $0 }
        }
        let radius = UInt64((radiusSeconds * Double(rate)).rounded())
        let session = m.session
        return { frame in (try? session.zeroCrossing(media: media, frame: frame, radius: radius)) ?? frame }
    }
}
