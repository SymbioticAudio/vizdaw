import CoreGraphics
import Foundation
import VizDAWCore

/// An automation target as layouts save it (mirrors `AutomationTargetRow`).
public enum AutoTarget: Codable, Hashable, Sendable {
    case fader(strip: UInt64)
    case pan(strip: UInt64)
    case mute(strip: UInt64)
    case solo(strip: UInt64)
    case send(strip: UInt64, index: UInt16)
    case param(slot: UInt64, param: UInt64)
    case velocityScale(dest: UInt64)

    public init(_ r: AutomationTargetRow) {
        switch r {
        case .stripFader(let s): self = .fader(strip: s)
        case .stripPan(let s): self = .pan(strip: s)
        case .stripMute(let s): self = .mute(strip: s)
        case .stripSolo(let s): self = .solo(strip: s)
        case .sendLevel(let s, let i): self = .send(strip: s, index: i)
        case .pluginParam(let slot, let p): self = .param(slot: slot, param: p)
        case .velocityScale(let d): self = .velocityScale(dest: d)
        }
    }

    public var row: AutomationTargetRow {
        switch self {
        case .fader(let s): .stripFader(strip: s)
        case .pan(let s): .stripPan(strip: s)
        case .mute(let s): .stripMute(strip: s)
        case .solo(let s): .stripSolo(strip: s)
        case .send(let s, let i): .sendLevel(strip: s, send: i)
        case .param(let slot, let p): .pluginParam(slot: slot, param: p)
        case .velocityScale(let d): .velocityScale(dest: d)
        }
    }

    /// Mute and solo hold each value until the next point (Plan 4d Ruling 8).
    public var isSwitch: Bool {
        switch self {
        case .mute, .solo: true
        default: false
        }
    }
}

/// The normalised-value laws (Plan 4d Ruling 4), exactly as the core has
/// them (`core_model::automation`).
public enum AutomationLaw {
    public static let fullScale = Double(UInt32.max)
    public static let floorDb = -144.0

    public static func unit(_ v: Double) -> Double { v.isNaN ? 0 : min(max(v, 0), 1) }

    public static func faderDb(_ v: Double) -> Double {
        let v = unit(v)
        guard v > 0 else { return floorDb }
        let db = 12 + 60 * log10(v)
        return db <= floorDb ? floorDb : db
    }

    public static func faderNorm(_ db: Double) -> Double {
        db.isNaN || db <= floorDb ? 0 : pow(10, (min(db, 12) - 12) / 60)
    }

    public static func pan(_ v: Double) -> Double { unit(v) * 2 - 1 }

    public static func velocityScalePercent(_ v: Double) -> Double {
        let v = unit(v)
        return v <= 0.5 ? 1 + 198 * v : 100 + 300 * (v - 0.5)
    }

    /// A 32-bit-scale value as a normalised value.
    public static func normalized(_ v32: Int64) -> Double {
        Double(min(max(v32, 0), Int64(UInt32.max))) / fullScale
    }

    /// A normalised value on the 32-bit scale.
    public static func v32(_ v: Double) -> Int64 { Int64((unit(v) * fullScale).rounded()) }

    /// A plain parameter value, never in exponent form: precision follows
    /// magnitude and trailing zeros go.
    static func plainText(_ x: Double) -> String {
        guard x.isFinite else { return "0" }
        let a = abs(x)
        let places = a >= 1000 ? 0 : a >= 100 ? 1 : a >= 10 ? 2 : a >= 1 ? 3 : a >= 0.01 ? 4 : 6
        var t = String(format: "%.\(places)f", x)
        if t.contains(".") {
            while t.hasSuffix("0") { t.removeLast() }
            if t.hasSuffix(".") { t.removeLast() }
        }
        return t == "-0" ? "0" : t
    }

    /// The tick after `t`, for an inclusive end as an exclusive one; the
    /// last tick stays (no overflow).
    public static func after(_ t: Int64) -> Int64 { t < .max ? t + 1 : .max }

    /// A normalised value as 0.0–100.0 %.
    public static func percent(_ v: Double) -> String { String(format: "%.1f%%", unit(v) * 100) }

    /// A value as the Strip Chart shows it.
    public static func text(_ t: AutoTarget, _ v: Double, param: ParamRow? = nil) -> String {
        switch t {
        case .fader, .send:
            let db = faderDb(v)
            // A level that would read "-144.0" is the floor: "-∞".
            return db < floorDb + 0.05 ? "-∞ dB" : String(format: "%.1f dB", db)
        case .pan:
            let p = Int((pan(v) * 100).rounded())
            return p == 0 ? "C" : (p < 0 ? "L\(-p)" : "R\(p)")
        case .mute, .solo:
            return unit(v) >= 0.5 ? "On" : "Off"
        case .param:
            guard let p = param else { return percent(v) }
            let plain = p.min + unit(v) * (p.max - p.min)
            // plainText never traps, whatever the range (non-finite, huge).
            return plainText(p.stepped ? plain.rounded() : plain)
        case .velocityScale:
            return String(format: "%.0f%%", velocityScalePercent(v))
        }
    }
}

/// Names for automation targets, from the mixer (Plan 4d Ruling 21).
public struct StripNames: Equatable, Sendable {
    public var strips: [UInt64: String] = [:]
    /// Per strip, its sends' target names in send order.
    public var sends: [UInt64: [String]] = [:]
    public var slots: [UInt64: String] = [:]
    public var params: [UInt64: [ParamRow]] = [:]
    public var dests: [UInt64: String] = [:]

    public init() {}

    public func param(_ slot: UInt64, _ id: UInt64) -> ParamRow? { params[slot]?.first { $0.id == id } }

    /// Whether `t`'s strip, plugin slot or destination still exists (names
    /// read from a mixer with no strips know nothing: every target passes).
    public func knows(_ t: AutoTarget) -> Bool {
        guard !strips.isEmpty else { return true }
        switch t {
        case .fader(let s), .pan(let s), .mute(let s), .solo(let s), .send(let s, _): return strips[s] != nil
        case .param(let slot, _): return slots[slot] != nil
        case .velocityScale(let d): return dests[d] != nil
        }
    }

    func strip(_ s: UInt64) -> String { strips[s] ?? "Strip \(s)" }

    public func title(_ t: AutoTarget) -> String {
        switch t {
        case .fader(let s): return "Fader · \(strip(s))"
        case .pan(let s): return "Pan · \(strip(s))"
        case .mute(let s): return "Mute · \(strip(s))"
        case .solo(let s): return "Solo · \(strip(s))"
        case .send(let s, let i):
            let to = sends[s].flatMap { $0.indices.contains(Int(i)) ? " to \($0[Int(i)])" : nil } ?? ""
            return "Send \(Int(i) + 1)\(to) · \(strip(s))"
        case .param(let slot, let p):
            return "\(param(slot, p)?.name ?? "Parameter \(p)") · \(slots[slot] ?? "Plugin \(slot)")"
        case .velocityScale(let d):
            return "Velocity Scale · \(dests[d] ?? "Destination \(d)")"
        }
    }
}

/// One value a Strip Chart row draws.
public struct StripValue: Equatable, Sendable {
    /// The event (`nil` for a tempo point).
    public let id: UInt64?
    public let tick: Int64
    /// A note's length (bars are this wide); 0 for points.
    public let length: Int64
    public let value: Int64
    /// The value ramps to the next point (continuous automation, a
    /// ramping tempo point).
    public let ramp: Bool

    public init(id: UInt64?, tick: Int64, length: Int64, value: Int64, ramp: Bool) {
        self.id = id
        self.tick = tick
        self.length = length
        self.value = value
        self.ramp = ramp
    }
}

/// What a Strip Chart row shows (Plan 4d Ruling 21).
public enum StripLane: Codable, Hashable, Sendable {
    case velocity, releaseVelocity, duration
    case controller(UInt8)
    case pitchBend, channelPressure
    case polyPressure(UInt8)
    case program, tempo
    case automation(AutoTarget)

    /// How values are drawn: bars at note starts, held steps, or ramps.
    public enum Style: Sendable { case bars, steps, ramps }

    /// The controllers the row menu lists first.
    public static let controllers: [UInt8] = [1, 2, 4, 7, 10, 11, 64, 74]
    static let controllerNames: [UInt8: String] = [
        1: "Modulation", 2: "Breath", 4: "Foot", 7: "Volume", 10: "Pan", 11: "Expression", 64: "Sustain",
        74: "Brightness",
    ]

    /// The row of a line target (the panel's target picker).
    public init?(_ t: LineTargetRow) {
        switch t {
        case .value(.velocity): self = .velocity
        case .value(.releaseVelocity): self = .releaseVelocity
        case .value(.duration): self = .duration
        case .value(.value): return nil
        case .series(.controller(let i)): self = .controller(i)
        case .series(.pitchBend): self = .pitchBend
        case .series(.channelPressure): self = .channelPressure
        case .series(.polyPressure(let p)): self = .polyPressure(p)
        case .series(.program): self = .program
        case .series(.automation(let t)): self = .automation(AutoTarget(t))
        case .tempo: self = .tempo
        }
    }

    public var lineTarget: LineTargetRow {
        switch self {
        case .velocity: .value(target: .velocity)
        case .releaseVelocity: .value(target: .releaseVelocity)
        case .duration: .value(target: .duration)
        case .controller(let i): .series(series: .controller(index: i))
        case .pitchBend: .series(series: .pitchBend)
        case .channelPressure: .series(series: .channelPressure)
        case .polyPressure(let p): .series(series: .polyPressure(pitch: p))
        case .program: .series(series: .program)
        case .tempo: .tempo
        case .automation(let t): .series(series: .automation(target: t.row))
        }
    }

    public var isNoteValue: Bool {
        switch self {
        case .velocity, .releaseVelocity, .duration: true
        default: false
        }
    }

    public var style: Style {
        switch self {
        case .velocity, .releaseVelocity, .duration: .bars
        case .tempo: .ramps
        case .automation(let t): t.isSwitch ? .steps : .ramps
        default: .steps
        }
    }

    /// The values a row spans, bottom to top (Plan 4d Ruling 18).
    public var range: ClosedRange<Int64> {
        switch self {
        case .velocity, .releaseVelocity: 0...0xFFFF
        case .duration: 0...(4 * MeterMath.ppq)
        case .tempo: 30_000...300_000
        default: 0...Int64(UInt32.max)
        }
    }

    /// A centre line (pitch bend, pan).
    public var center: Int64? {
        switch self {
        case .pitchBend, .automation(.pan): 0x8000_0000
        default: nil
        }
    }

    /// The row's values among `events` (or the tempo map), in time order.
    public func values(_ events: [EventRow], tempo: [TempoPointRow]) -> [StripValue] {
        switch self {
        case .tempo:
            // A non-finite bpm is skipped rather than trapping.
            return tempo.filter { $0.bpm.isFinite }.map {
                let milli = min(max(($0.bpm * 1000).rounded(), 0), 1e9)
                return StripValue(id: nil, tick: $0.tick, length: 0, value: Int64(milli), ramp: $0.ramp)
            }
        case .velocity, .releaseVelocity, .duration:
            return events.filter { $0.kind == .note }.map { e in
                let v: Int64
                switch self {
                case .velocity: v = Int64(e.velocity)
                case .releaseVelocity: v = Int64(e.releaseVelocity)
                default: v = e.duration
                }
                return StripValue(id: e.id, tick: e.tick, length: max(e.duration, 0), value: v, ramp: false)
            }
        default:
            let ramps = style == .ramps
            return events.compactMap { e in
                seriesValue(e).map { StripValue(id: e.id, tick: e.tick, length: 0, value: $0, ramp: ramps) }
            }
        }
    }

    /// `e`'s value when it belongs to this row's series (32-bit scale).
    func seriesValue(_ e: EventRow) -> Int64? {
        switch self {
        case .controller(let i): e.kind == .cc && e.value2 == Int64(i) ? e.value : nil
        case .pitchBend: e.kind == .pitchBend ? e.value : nil
        case .channelPressure: e.kind == .channelPressure ? e.value : nil
        case .polyPressure(let p): e.kind == .polyPressure && e.pitch == p ? e.value : nil
        case .program: e.kind == .program ? e.value << 25 : nil
        case .automation(let t):
            e.kind == .automation && e.automation?.target == t.row ? AutomationLaw.v32(e.automation?.value ?? 0) : nil
        default: nil
        }
    }

    /// What the eraser deletes between two ticks (either order); nil for
    /// note values and tempo, which are not erased here.
    public func eraseRule(from: Int64, to: Int64) -> RuleRow? {
        let lo = min(from, to)
        let hi = max(from, to)
        func rule(_ tests: [ConditionTest]) -> RuleRow {
            RuleRow(
                conditions: ([.timeRange(start: lo, end: AutomationLaw.after(hi))] + tests).map {
                    ConditionRow(test: $0, negate: false)
                },
                matchAll: true)
        }
        switch self {
        case .velocity, .releaseVelocity, .duration, .tempo: return nil
        case .controller(let i): return rule([.kinds(kinds: [.cc]), .controller(low: i, high: i)])
        case .pitchBend: return rule([.kinds(kinds: [.pitchBend])])
        case .channelPressure: return rule([.kinds(kinds: [.channelPressure])])
        case .polyPressure(let p): return rule([.kinds(kinds: [.polyPressure]), .pitch(low: p, high: p)])
        case .program: return rule([.kinds(kinds: [.program])])
        case .automation(let t): return rule([.automation(target: t.row)])
        }
    }

    public func title(_ names: StripNames) -> String {
        switch self {
        case .velocity: "Velocity"
        case .releaseVelocity: "Release Velocity"
        case .duration: "Length"
        case .controller(let i): "Controller \(i)" + (Self.controllerNames[i].map { " (\($0))" } ?? "")
        case .pitchBend: "Pitch Bend"
        case .channelPressure: "Channel Pressure"
        case .polyPressure(let p): "Poly Pressure \(PitchName.name(p))"
        case .program: "Program"
        case .tempo: "Tempo"
        case .automation(let t): names.title(t)
        }
    }

    /// A value as the row shows it.
    public func text(_ v: Int64, _ names: StripNames) -> String {
        switch self {
        case .velocity, .releaseVelocity: return "\(Velocity7.of(UInt16(clamping: v)))"
        case .duration: return DurationText.text(max(v, 0), beat: MeterMath.ppq)
        case .controller, .channelPressure, .polyPressure: return "\(Value7.of(v))"
        case .pitchBend: return "\(Bend14.of(v))"
        case .program: return "\(min(max(v, 0), Int64(UInt32.max)) >> 25)"
        case .tempo: return String(format: "%.1f BPM", Double(v) / 1000)
        case .automation(let t):
            let p: ParamRow?
            if case .param(let slot, let id) = t { p = names.param(slot, id) } else { p = nil }
            return AutomationLaw.text(t, AutomationLaw.normalized(v), param: p)
        }
    }
}

/// The Strip Chart's modes (Plan 4d Ruling 22).
public enum StripMode: String, Codable, CaseIterable, Sendable {
    case draw, modify, erase

    public var title: String {
        switch self {
        case .draw: "Draw"
        case .modify: "Modify"
        case .erase: "Erase"
        }
    }
}

/// What a Modify line does (Plan 4d Rulings 15, 19).
public enum StripOp: String, Codable, CaseIterable, Sendable {
    case set, scale, add, limitMax, limitMin, compress, thin, thicken, legato

    public var title: String {
        switch self {
        case .set: "Set"
        case .scale: "Scale"
        case .add: "Add"
        case .limitMax: "Max Limit"
        case .limitMin: "Min Limit"
        case .compress: "Compress"
        case .thin: "Thin"
        case .thicken: "Thicken"
        case .legato: "Legato"
        }
    }

    public var row: LineOpRow {
        switch self {
        case .set: .set
        case .scale: .scale
        case .add: .add
        case .limitMax: .limitMax
        case .limitMin: .limitMin
        case .compress: .compress
        case .thin: .thin
        case .thicken: .thicken
        case .legato: .legato
        }
    }

    /// What a row offers: legato for lengths only; thin and thicken for
    /// series and tempo.
    public static func choices(for lane: StripLane) -> [StripOp] {
        let common: [StripOp] = [.set, .scale, .add, .limitMax, .limitMin, .compress]
        if lane == .duration { return common + [.legato] }
        return lane.isNoteValue ? common : common + [.thin, .thicken]
    }
}

/// Value ↔ y inside one row (`top` is the row's top in the view; `inset`
/// points stay free above and below).
public struct StripScale: Equatable, Sendable {
    public var range: ClosedRange<Int64>
    public var top: CGFloat
    public var height: CGFloat
    public var inset: CGFloat

    public init(range: ClosedRange<Int64>, top: CGFloat, height: CGFloat, inset: CGFloat = 3) {
        self.range = range
        self.top = top.isFinite ? top : 0
        self.height = height.isFinite ? max(height, 0) : 0
        self.inset = inset
    }

    var span: CGFloat { max(height - 2 * inset, 1) }

    public func y(_ v: Int64) -> CGFloat {
        let lo = Double(range.lowerBound)
        let hi = Double(range.upperBound)
        let c = Double(min(max(v, range.lowerBound), range.upperBound))
        let f = hi > lo ? (c - lo) / (hi - lo) : 0
        return top + inset + span * CGFloat(1 - f)
    }

    /// 0 at the bottom of the row, 1 at its top; clamped.
    public func fraction(atY y: CGFloat) -> Double {
        guard y.isFinite else { return 0 }
        let f = 1 - Double((y - top - inset) / span)
        return f.isFinite ? min(max(f, 0), 1) : 0
    }

    public func value(atY y: CGFloat) -> Int64 {
        let lo = Double(range.lowerBound)
        let hi = Double(range.upperBound)
        return Int64((lo + fraction(atY: y) * (hi - lo)).rounded())
    }

    /// What a point at `y` stands for under `op` (Plan 4d Ruling 18): a
    /// percentage for Scale (0 at the bottom, 100 in the middle, 200 at the
    /// top), a signed change for Add (± half the range at the edges), else a
    /// value.
    public func amount(atY y: CGFloat, op: StripOp) -> Int64 {
        switch op {
        case .scale:
            return Int64((fraction(atY: y) * 200).rounded())
        case .add:
            let half = Double(range.upperBound - range.lowerBound)
            return Int64(((fraction(atY: y) - 0.5) * half).rounded())
        default:
            return value(atY: y)
        }
    }
}
