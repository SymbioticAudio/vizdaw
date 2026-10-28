import Foundation
import VizDAWCore

/// One Measure, run off the main actor (plan 5d-2 Ruling 6; 5c-2 carry,
/// final review finding 4): the session call (about 2.7 s, 10 s at most)
/// runs on a detached task and never blocks the main thread; the main
/// actor reads the result when it is done. The Track Synth sheet and the
/// control channel's `destination.measure_return` both use it.
@MainActor
public final class MeasureJob {
    public let dest: UInt64
    /// The measurement, or why it failed; nil while it runs.
    public private(set) var result: Result<MeasureRow, any Error>?
    /// Called on the main actor when it ends.
    public var onDone: (@MainActor (Result<MeasureRow, any Error>) -> Void)?

    public var isDone: Bool { result != nil }

    public init(session: any SessionProtocol, dest: UInt64) {
        self.dest = dest
        Task.detached { [weak self] in
            let r: Result<MeasureRow, any Error> = Result { try session.measureReturn(dest: dest) }
            await self?.finish(r)
        }
    }

    private func finish(_ r: Result<MeasureRow, any Error>) {
        result = r
        onDone?(r)
    }
}
