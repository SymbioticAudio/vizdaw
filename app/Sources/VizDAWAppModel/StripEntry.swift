import Foundation

/// The channel strip's typed values (plan 7b Ruling 15): a level or send
/// level in dB, and a pan as L / C / R. The grammar is strict: ASCII
/// digits only, no exponents, no `nan`, and text longer than
/// `maxLength` is refused, never cut.
public enum StripEntry {
    public static let levelHint = "Type a level in dB, like \u{2212}4.2 or \u{2212}inf."
    public static let panHint = "Type a pan like L 24, C or R 10."
    public static let touchedHint = "Release the control first."

    /// Longest text accepted, in Unicode scalars (`NumberEntry`'s rule; this
    /// module does not import `SymbioticControls`).
    static let maxLength = 32

    private static let minus: Character = "\u{2212}"

    /// The text trimmed, U+2212 read as "-"; nil when too long or holding a
    /// control character.
    private static func clean(_ s: String) -> String? {
        let t = s.trimmingCharacters(in: .whitespacesAndNewlines)
            .replacingOccurrences(of: String(minus), with: "-")
        guard t.unicodeScalars.count <= maxLength else { return nil }
        guard !t.unicodeScalars.contains(where: { CharacterSet.controlCharacters.contains($0) }) else {
            return nil
        }
        return t
    }

    /// `digits`, `digits.digits` or `.digits`, ASCII only.
    private static func number(_ s: Substring) -> Double? {
        let parts = s.split(separator: ".", maxSplits: 1, omittingEmptySubsequences: false)
        guard parts.count <= 2, !parts.isEmpty else { return nil }
        func digits(_ p: Substring) -> Bool {
            !p.isEmpty && p.unicodeScalars.allSatisfy { $0.value >= 0x30 && $0.value <= 0x39 }
        }
        if parts.count == 1 {
            guard digits(parts[0]) else { return nil }
        } else {
            guard digits(parts[1]), parts[0].isEmpty || digits(parts[0]) else { return nil }
        }
        return Double(s)
    }

    /// dB from `text`, rounded to 0.1 and clamped to −144…+12 (−144 is −∞);
    /// nil when refused.
    public static func level(_ text: String) -> Double? {
        guard var t = clean(text)?.lowercased() else { return nil }
        if t.hasSuffix("db") {
            t.removeLast(2)
            if t.hasSuffix(" ") { t.removeLast() }
        }
        var sign = 1.0
        var hasSign = false
        var body = Substring(t)
        if let c = body.first, c == "+" || c == "-" {
            hasSign = true
            sign = c == "-" ? -1 : 1
            body = body.dropFirst()
        }
        if body == "inf" || body == "\u{221E}" {
            return hasSign && sign < 0 ? MixerMath.floorDb : nil
        }
        guard let n = number(body) else { return nil }
        let v = (sign * n * 10).rounded(.toNearestOrAwayFromZero) / 10
        if v <= MixerMath.floorDb { return MixerMath.floorDb }
        if v == 0 { return 0 }
        return min(v, MixerMath.maxDb)
    }

    /// −1…1 from `text` (L negative, whole percent); nil when refused.
    public static func pan(_ text: String) -> Double? {
        guard let t = clean(text)?.uppercased() else { return nil }
        if t == "C" { return 0 }
        var body = Substring(t)
        var sign = 1.0
        switch body.first {
        case "L", "R":
            sign = body.first == "L" ? -1 : 1
            body = body.dropFirst()
            if body.first == " " { body = body.dropFirst() }
        case "+", "-":
            sign = body.first == "-" ? -1 : 1
            body = body.dropFirst()
        default:
            break
        }
        guard let n = number(body) else { return nil }
        let pct = min(100, n.rounded(.toNearestOrAwayFromZero))
        return pct == 0 ? 0 : sign * pct / 100
    }

    /// "−4.2 dB", "+3.0 dB", "0.0 dB", "−∞ dB".
    public static func levelText(_ db: Double) -> String {
        if db.isNaN || db <= MixerMath.floorDb { return "\u{2212}\u{221E} dB" }
        let v = (min(db, MixerMath.maxDb) * 10).rounded(.toNearestOrAwayFromZero) / 10
        if v == 0 { return "0.0 dB" }
        let body = String(format: "%.1f", abs(v))
        return (v < 0 ? String(minus) : "+") + body + " dB"
    }

    /// "L 24", "C", "R 10".
    public static func panText(_ pan: Double) -> String {
        let n = pan.isNaN ? 0 : Int((min(1, abs(pan)) * 100).rounded())
        if n == 0 { return "C" }
        return (pan < 0 ? "L " : "R ") + "\(n)"
    }
}
