import Foundation
import Testing
import VizDAWAppModel

/// The probe's pure timing (plan 8f Task 4): samples once per interval,
/// only the age of the last input, and main-menu sessions.
@Suite struct MetricsTickerTests {
    private func dueAt(_ t: inout MetricsTicker, _ ms: UInt64) -> Bool { t.due(at: ms) }

    @Test func samplesAreDueOncePerInterval() {
        var t = MetricsTicker(sampleMs: 5_000)
        #expect(dueAt(&t, 0))
        #expect(!dueAt(&t, 0))
        #expect(!dueAt(&t, 4_999))
        #expect(dueAt(&t, 5_000))
        #expect(!dueAt(&t, 5_001))
        // A 3-minute gap (sleep, a stall): due once, not once per missed tick.
        #expect(dueAt(&t, 185_000))
        #expect(!dueAt(&t, 185_000))
        #expect(!dueAt(&t, 186_000))
    }

    @Test func aLateTickDoesNotShiftTheNextOne() {
        var t = MetricsTicker(sampleMs: 5_000)
        #expect(dueAt(&t, 0))
        // The timer fired 3 ms late; the next on-time tick is still due.
        #expect(dueAt(&t, 5_003))
        #expect(dueAt(&t, 10_000))
    }

    @Test func inputAgeOnly() {
        var t = MetricsTicker(sampleMs: 5_000)
        #expect(t.secondsSinceInput(at: 1_000) == nil)
        t.input(at: 1_000)
        #expect(t.secondsSinceInput(at: 91_000) == 90)
        // A clock set back reads as just now, never negative.
        #expect(t.secondsSinceInput(at: 500) == 0)
    }

    @Test func menuSessionsAreTimed() {
        var t = MetricsTicker(sampleMs: 5_000)
        // An end with no begin is nothing.
        let empty = t.menuEnded(at: 1_000)
        #expect(empty == nil)
        // A choice outside a session is not kept.
        t.menuChose("Bounce…")
        t.menuBegan(at: 0)
        t.menuChose("Quantize…")
        let r = t.menuEnded(at: 9_000)
        #expect(r?.durationMs == 9_000)
        #expect(r?.chosen == "Quantize…")
        let over = t.menuEnded(at: 10_000)
        #expect(over == nil)
        // Nothing chosen.
        t.menuBegan(at: 20_000)
        let none = t.menuEnded(at: 21_000)
        #expect(none?.durationMs == 1_000)
        #expect(none?.chosen == nil)
        // A second begin restarts the session and forgets the choice.
        t.menuBegan(at: 30_000)
        t.menuChose("Quantize…")
        t.menuBegan(at: 35_000)
        let again = t.menuEnded(at: 36_000)
        #expect(again?.durationMs == 1_000)
        #expect(again?.chosen == nil)
    }
}
