import VizDAWCore

/// Extrapolates the playhead between transport polls (spec §3.4, Plan
/// Ruling 11): `tick + min(elapsed, pollInterval) × ticksPerSecond`. While
/// playing it never steps back by less than `holdSeconds` (poll jitter);
/// it snaps on bigger backward jumps (locate, loop wrap), on start, stop
/// and a change of sequence. Owned by `DocumentModel` outside observation:
/// the canvas reads it on every display frame.
@MainActor
public final class TransportClock {
    public struct Sample: Equatable, Sendable {
        public let tick: Int64
        public let hostTimeNs: UInt64
        public let ticksPerSecond: Double
        public let playing: Bool
        public let seq: UInt64?
    }

    public static let holdSeconds = 0.1

    public private(set) var sample: Sample?
    /// The tick drawn last.
    public private(set) var shown: Double = 0
    /// How far (seconds) the extrapolation may run past the last sample.
    public private(set) var pollInterval: Double = 0.033

    public init() {}

    public var isPlaying: Bool { sample?.playing ?? false }

    /// A new transport reading.
    public func update(_ t: TransportState, pollInterval: Double) {
        let new = Sample(
            tick: t.tick, hostTimeNs: t.hostTimeNs, ticksPerSecond: max(t.ticksPerSecond, 0),
            playing: t.playing, seq: t.seq)
        self.pollInterval = pollInterval
        let old = sample
        sample = new
        let continuing = (old?.playing ?? false) && new.playing && old?.seq == new.seq
        let jumpedBack = Double(new.tick) < shown - Self.holdSeconds * new.ticksPerSecond
        if !continuing || jumpedBack {
            shown = Double(new.tick)
        }
    }

    /// The tick to draw at host time `now` (ns, `CACurrentMediaTime`'s clock).
    public func tick(atHostTimeNs now: UInt64) -> Double {
        guard let s = sample else { return 0 }
        guard s.playing else {
            shown = Double(s.tick)
            return shown
        }
        let elapsed = now > s.hostTimeNs ? Double(now - s.hostTimeNs) / 1e9 : 0
        let t = Double(s.tick) + min(elapsed, pollInterval) * s.ticksPerSecond
        shown = max(shown, t)
        return shown
    }
}
