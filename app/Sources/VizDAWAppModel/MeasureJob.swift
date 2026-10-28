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

    /// The task keeps the job until it ends, so `onDone` runs even when
    /// nobody waits for the answer any more (Task 8 review M1).
    public init(session: any SessionProtocol, dest: UInt64) {
        self.dest = dest
        let hold = SessionHold(session)
        Task.detached {
            let r = hold.measure(dest)
            await self.finish(r, hold)
        }
    }

    private func finish(_ r: Result<MeasureRow, any Error>, _ hold: SessionHold) {
        hold.session = nil
        result = r
        onDone?(r)
    }
}

/// Holds the session for the detached call and lets it go on the main
/// actor (Task 8 review M2): a document closed mid-Measure has its
/// `Session` dropped on the thread that keeps its plugin host. Read off
/// the main actor only by the call, and cleared on it only after.
private final class SessionHold: @unchecked Sendable {
    var session: (any SessionProtocol)?

    init(_ session: any SessionProtocol) { self.session = session }

    func measure(_ dest: UInt64) -> Result<MeasureRow, any Error> {
        guard let s = session else { return .failure(VizError.Internal(message: "the session closed")) }
        return Result { try s.measureReturn(dest: dest) }
    }
}
