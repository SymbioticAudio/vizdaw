import Foundation

/// The transport's marks and record modes, saved with the project in
/// `ui.json` (plan 4f Rulings 7–8): ten numbered locators, the punch range
/// and the punch and overdub switches (recording, sub-project 5, will
/// read them). Decoding is tolerant: a missing or malformed field keeps
/// its default; ticks are kept within ±2^40.
public struct TransportMarks: Codable, Equatable, Sendable {
    /// How many locators (keys 0–9).
    public static let locatorCount = 10
    /// The model's time limit.
    public static let tickLimit: Int64 = 1 << 40

    /// Locator `n`'s tick, or nil when it is not set.
    public var locators: [Int64?] = Array(repeating: nil, count: locatorCount)
    public var punchIn: Int64 = 0
    /// After `punchIn`.
    public var punchOut: Int64 = 4 * 61_440
    public var punch = false
    public var overdub = false

    public init() {}

    enum CodingKeys: String, CodingKey {
        case locators, punchIn, punchOut, punch, overdub
    }

    public init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        let d = TransportMarks()
        let raw: [Int64?] = c.field(.locators, d.locators)
        locators = (0..<Self.locatorCount).map { i in
            i < raw.count ? raw[i].flatMap(Self.sane) : nil
        }
        let a = c.field(.punchIn, d.punchIn)
        let b = c.field(.punchOut, d.punchOut)
        if let a = Self.sane(a), let b = Self.sane(b), b > a {
            punchIn = a
            punchOut = b
        }
        punch = c.field(.punch, d.punch)
        overdub = c.field(.overdub, d.overdub)
    }

    /// `t` when it is within the model's limit.
    static func sane(_ t: Int64) -> Int64? { (-tickLimit...tickLimit).contains(t) ? t : nil }

    /// Locator `n` (0–9), if set.
    public func locator(_ n: Int) -> Int64? {
        locators.indices.contains(n) ? locators[n] : nil
    }

    /// Sets (a tick) or clears (nil) locator `n`; false for a number
    /// outside 0–9 or a tick past the limit.
    @discardableResult
    public mutating func setLocator(_ n: Int, _ tick: Int64?) -> Bool {
        guard locators.indices.contains(n) else { return false }
        if let t = tick, Self.sane(t) == nil { return false }
        locators[n] = tick
        return true
    }

    /// Sets the punch range; false (nothing changed) unless `out > in`
    /// and both are within the limit.
    @discardableResult
    public mutating func setPunch(in a: Int64, out b: Int64) -> Bool {
        guard Self.sane(a) != nil, Self.sane(b) != nil, b > a else { return false }
        punchIn = a
        punchOut = b
        return true
    }
}
