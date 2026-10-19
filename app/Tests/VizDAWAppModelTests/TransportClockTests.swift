import Testing
import VizDAWCore
@testable import VizDAWAppModel

private let t0: UInt64 = 1_000_000_000

private func state(
    tick: Int64, at ns: UInt64, playing: Bool = true, seq: UInt64? = 1, tps: Double = 30_720
) -> TransportState {
    TransportState(
        playing: playing, seq: seq, tick: tick, positionBbu: "", seconds: 0, sampleRate: 48_000,
        device: "", faults: [], report: [], hostTimeNs: ns, ticksPerSecond: tps)
}

@MainActor
@Suite struct TransportClockTests {
    @Test func noSampleIsTickZero() {
        #expect(TransportClock().tick(atHostTimeNs: t0) == 0)
    }

    @Test func extrapolatesBetweenPolls() {
        let c = TransportClock()
        c.update(state(tick: 1_000, at: t0), pollInterval: 0.033)
        #expect(c.tick(atHostTimeNs: t0 + 10_000_000) == 1_000 + 0.01 * 30_720)
    }

    @Test func neverRunsMoreThanOnePollPastTheSample() {
        let c = TransportClock()
        c.update(state(tick: 1_000, at: t0), pollInterval: 0.033)
        #expect(c.tick(atHostTimeNs: t0 + 1_000_000_000) == 1_000 + 0.033 * 30_720)
    }

    @Test func holdsInsteadOfTwitchingBack() {
        let c = TransportClock()
        c.update(state(tick: 1_000, at: t0), pollInterval: 0.033)
        let ahead = c.tick(atHostTimeNs: t0 + 30_000_000)
        // The next poll reads slightly less than was drawn: hold.
        c.update(state(tick: 1_900, at: t0 + 30_000_000), pollInterval: 0.033)
        #expect(c.tick(atHostTimeNs: t0 + 30_000_000) == ahead)
        #expect(c.tick(atHostTimeNs: t0 + 40_000_000) == 1_900 + 0.01 * 30_720)
    }

    @Test func snapsBackOnALocateOrALoopWrap() {
        let c = TransportClock()
        c.update(state(tick: 100_000, at: t0), pollInterval: 0.033)
        _ = c.tick(atHostTimeNs: t0 + 20_000_000)
        c.update(state(tick: 0, at: t0 + 33_000_000), pollInterval: 0.033)
        #expect(c.tick(atHostTimeNs: t0 + 33_000_000) == 0)
    }

    @Test func stopAndSequenceChangesSnap() {
        let c = TransportClock()
        c.update(state(tick: 9_000, at: t0), pollInterval: 0.033)
        _ = c.tick(atHostTimeNs: t0 + 30_000_000)
        c.update(state(tick: 8_000, at: t0 + 33_000_000, playing: false), pollInterval: 0.25)
        #expect(c.tick(atHostTimeNs: t0 + 900_000_000) == 8_000)
        c.update(state(tick: 9_000, at: t0, seq: 1), pollInterval: 0.033)
        _ = c.tick(atHostTimeNs: t0 + 30_000_000)
        c.update(state(tick: 8_990, at: t0 + 33_000_000, seq: 2), pollInterval: 0.033)
        #expect(c.tick(atHostTimeNs: t0 + 33_000_000) == 8_990)
    }

    @Test func theDocumentFeedsTheClockOnEveryPoll() throws {
        let f = demoFake()
        let m = try DocumentModel(session: f)
        f.state.playing = true
        f.state.tick = 5_000
        f.state.hostTimeNs = 77
        m.poll()
        #expect(m.clock.sample?.tick == 5_000)
        #expect(m.clock.sample?.hostTimeNs == 77)
        #expect(m.clock.pollInterval == 0.033)
    }
}
