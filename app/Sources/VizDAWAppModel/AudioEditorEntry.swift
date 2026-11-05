import Foundation

/// The audio editor's typed values (plan 7c Ruling 7): gain, fades, time
/// and peak. Strict like `StripEntry`: ASCII digits only, no sign on a
/// time, no exponent, `nan` or `inf`, and text over 32 scalars is refused.
public enum AudioEditorEntry {
    public enum Unit: Sendable { case ms, s }

    public static let gainHint = "Type a gain in dB, like \u{2212}4.2 or +6."
    public static let fadeHint = "Type a fade like 250 ms or 1.5 s."
    public static let timeHint = "Type a time like 0:01.250, 1.25 s or 1250 ms."
    public static let peakHint = "Type a peak from \u{2212}96 to 0 dB, like \u{2212}0.3."
    public static let noMediaHint = "This region's audio file is unknown."

    /// Lowest peak target, in dB.
    static let peakFloorDb = -96.0

    /// Gain in dB, clamped to −144…+24 (−144 is −∞); nil when refused.
    public static func gain(_ text: String) -> Double? {
        StripEntry.level(text, max: 24)
    }

    /// Peak target in dB, clamped to −96…0; `-inf` is refused.
    public static func peak(_ text: String) -> Double? {
        guard let t = StripEntry.clean(text)?.lowercased() else { return nil }
        if t.contains("inf") || t.contains("\u{221E}") { return nil }
        guard let v = StripEntry.level(text, max: 0) else { return nil }
        return max(v, peakFloorDb)
    }

    private static func ascii(_ p: Substring, maxCount: Int? = nil, exactly: Int? = nil) -> Bool {
        let n = p.unicodeScalars.count
        guard n > 0, p.unicodeScalars.allSatisfy({ $0.value >= 0x30 && $0.value <= 0x39 }) else {
            return false
        }
        if let m = maxCount, n > m { return false }
        if let e = exactly, n != e { return false }
        return true
    }

    /// Seconds from `N`, `N ms`, `N s`, `M:SS` or `M:SS.fff`; a bare `N` is in
    /// `bare`. nil when refused.
    public static func duration(_ text: String, bare: Unit) -> Double? {
        guard var t = StripEntry.clean(text)?.lowercased() else { return nil }
        if t.contains(":") {
            let parts = t.split(separator: ":", omittingEmptySubsequences: false)
            guard parts.count == 2, ascii(parts[0], maxCount: 4) else { return nil }
            let sec = parts[1].split(separator: ".", maxSplits: 1, omittingEmptySubsequences: false)
            guard ascii(sec[0], exactly: 2), let ss = Double(sec[0]), ss < 60 else { return nil }
            var frac = 0.0
            if sec.count == 2 {
                guard ascii(sec[1], maxCount: 3), let f = Double("0." + sec[1]) else { return nil }
                frac = f
            }
            return Double(parts[0])! * 60 + ss + frac
        }
        var unit = bare
        if t.hasSuffix("ms") {
            t.removeLast(2)
            unit = .ms
        } else if t.hasSuffix("s") {
            t.removeLast()
            unit = .s
        } else {
            return number(t, unit)
        }
        if t.hasSuffix(" ") { t.removeLast() }
        return number(t, unit)
    }

    private static func number(_ t: String, _ unit: Unit) -> Double? {
        guard let n = StripEntry.number(Substring(t)) else { return nil }
        return unit == .ms ? n / 1000 : n
    }

    /// Whole frames for `seconds` at `rate`, clamped to 0…`Int64.max / 2`;
    /// 0 for a non-finite product or a zero rate.
    public static func frames(_ seconds: Double, rate: UInt32) -> Int64 {
        guard rate > 0 else { return 0 }
        let v = (seconds * Double(rate)).rounded()
        guard v.isFinite else { return 0 }
        // `Double(Int64.max / 2)` rounds up to 2^62: clamp the integer too.
        return min(Int64(min(max(v, 0), Double(Int64.max / 2))), Int64.max / 2)
    }

    /// "−4.2 dB", "+24.0 dB", "−∞ dB".
    public static func gainText(_ db: Double) -> String {
        StripEntry.levelText(db, max: 24)
    }

    /// "−0.3 dB", "0.0 dB".
    public static func peakText(_ db: Double) -> String {
        StripEntry.levelText(db, max: 0)
    }

    /// Whole milliseconds: "250 ms", "1500 ms"; "–" without a rate.
    public static func fadeText(frames: Int64, rate: UInt32) -> String {
        guard rate > 0 else { return "\u{2013}" }
        let ms = (Double(max(frames, 0)) * 1000 / Double(rate)).rounded(.toNearestOrAwayFromZero)
        return "\(Int64(min(ms, 9e18))) ms"
    }
}
