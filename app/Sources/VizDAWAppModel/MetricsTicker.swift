import Foundation

/// The time probe's pure state (plan 8f Task 4): when the next sample is
/// due, how long ago the last input was, and the main menu's tracking
/// session. It holds only times and a menu item's title: never a key, a
/// position or a window.
public struct MetricsTicker: Sendable {
    private let sampleMs: UInt64
    /// The time the next sample is due; nil before the first.
    private var nextDue: UInt64?
    private var lastDue: UInt64?
    private var lastInput: UInt64?
    private var menuStart: UInt64?
    private var chosen: String?

    public init(sampleMs: UInt64) { self.sampleMs = max(sampleMs, 1) }

    /// Only that some input happened at `ms`.
    public mutating func input(at ms: UInt64) { lastInput = ms }

    /// Seconds since the last input; nil when there has been none.
    public func secondsSinceInput(at ms: UInt64) -> Double? {
        guard let lastInput else { return nil }
        return Double(ms > lastInput ? ms - lastInput : 0) / 1000
    }

    /// True once per `sampleMs`. The due times keep to a grid from the
    /// first, so a late tick does not push the next one out; after a gap
    /// (sleep, a stall) it is due once and the grid restarts half an
    /// interval on, between the timer's ticks.
    public mutating func due(at ms: UInt64) -> Bool {
        if let next = nextDue, let last = lastDue, ms >= last {
            guard ms >= next else { return false }
            nextDue = ms >= next + sampleMs ? ms + sampleMs / 2 : next + sampleMs
        } else {
            // The first sample, or the clock was set back.
            nextDue = ms + sampleMs
        }
        lastDue = ms
        return true
    }

    /// The main menu began tracking; a second begin restarts the session.
    public mutating func menuBegan(at ms: UInt64) {
        menuStart = ms
        chosen = nil
    }

    /// The item chosen while the menu tracks; the last one is kept.
    public mutating func menuChose(_ title: String) {
        guard menuStart != nil else { return }
        chosen = title
    }

    /// The session's length and chosen item; nil with no session begun.
    public mutating func menuEnded(at ms: UInt64) -> (durationMs: UInt64, chosen: String?)? {
        guard let start = menuStart else { return nil }
        let made = chosen
        menuStart = nil
        chosen = nil
        return (ms > start ? ms - start : 0, made)
    }
}
