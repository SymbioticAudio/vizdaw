import Foundation
import VizDAWCore

/// Which end of a 7-bit step a typed value stands for.
public enum Edge: Equatable, Sendable {
    /// A range's low end: the step's first 16- or 32-bit value.
    case low
    /// A range's high end: the step's last value.
    case high
    /// A signed change: steps of 512 (or 2²⁵).
    case delta
    /// One value: the MIDI 2.0 upscale.
    case exact
}

/// How a panel field shows and reads its number.
public enum FieldKind: Equatable, Sendable {
    case count
    /// A whole number limited to a range (semitones, controller numbers).
    case bounded(Int64, Int64)
    case percent
    case pitch
    /// A 16-bit velocity as 0–127.
    case velocity(Edge)
    /// A 32-bit value as 0–127.
    case value(Edge)
    /// Ticks as signed beats.units of a quarter note.
    case length
    /// A tick as bar.beat.unit.
    case position
}

/// One numeric field of a condition or modification.
public struct Field: Equatable, Sendable {
    public let label: String
    public let kind: FieldKind

    public init(_ label: String, _ kind: FieldKind) {
        self.label = label
        self.kind = kind
    }
}

public enum FieldText {
    public static func text(_ n: Int64, _ k: FieldKind, meter: [MeterEntryRow]) -> String {
        switch k {
        case .count, .bounded: return "\(n)"
        case .percent: return "\(n)%"
        case .pitch: return PitchName.name(UInt8(min(max(n, 0), 127)))
        case .velocity(let e): return e == .delta ? "\(n / 512)" : "\(n >> 9)"
        case .value(let e): return e == .delta ? "\(n / 33_554_432)" : "\(n >> 25)"
        case .length:
            let m = n == .min ? Int64.max : abs(n)
            return (n < 0 ? "-" : "") + DurationText.text(m, beat: MeterMath.ppq)
        case .position: return Bbu.of(n, meter: meter).text
        }
    }

    public static func parse(_ s: String, _ k: FieldKind, meter: [MeterEntryRow]) -> Int64? {
        let t = s.trimmingCharacters(in: .whitespaces)
        switch k {
        case .count:
            guard let v = Int64(t), abs(v) <= 1_000_000 else { return nil }
            return v
        case .bounded(let lo, let hi):
            guard let v = Int64(t), (lo...hi).contains(v) else { return nil }
            return v
        case .percent:
            let p = t.replacingOccurrences(of: "%", with: "").trimmingCharacters(in: .whitespaces)
            guard let v = Int64(p), (0...10_000).contains(v) else { return nil }
            return v
        case .pitch:
            return PitchName.parse(t).map(Int64.init)
        case .velocity(let e):
            guard let v = Int64(t) else { return nil }
            switch e {
            case .delta: return (-127...127).contains(v) ? v * 512 : nil
            case .low: return (0...127).contains(v) ? v << 9 : nil
            case .high: return (0...127).contains(v) ? (v << 9) | 0x1FF : nil
            case .exact: return (0...127).contains(v) ? Int64(Velocity7.up(Int(v))) : nil
            }
        case .value(let e):
            guard let v = Int64(t) else { return nil }
            switch e {
            case .delta: return (-127...127).contains(v) ? v << 25 : nil
            case .low: return (0...127).contains(v) ? v << 25 : nil
            case .high: return (0...127).contains(v) ? (v << 25) | 0x1FF_FFFF : nil
            case .exact: return (0...127).contains(v) ? Int64(Value7.up(Int(v))) : nil
            }
        case .length:
            let negative = t.hasPrefix("-")
            guard let d = DurationText.parse(negative ? String(t.dropFirst()) : t, current: 0, beat: MeterMath.ppq)
            else { return nil }
            return negative ? -d : d
        case .position:
            return PositionInput.parse(t, current: Bbu(bar: 1, beat: 1, unit: 0)).map { $0.tick(meter: meter) }
        }
    }
}

extension ConditionTest {
    public var fields: [Field] {
        switch self {
        case .timeRange: [Field("from", .position), Field("to", .position)]
        case .pitch, .bracketedBy: [Field("from", .pitch), Field("to", .pitch)]
        case .velocity, .releaseVelocity: [Field("from", .velocity(.low)), Field("to", .velocity(.high))]
        case .duration: [Field("from", .length), Field("to", .length)]
        case .value: [Field("from", .value(.low)), Field("to", .value(.high))]
        case .controller: [Field("from", .bounded(0, 127)), Field("to", .bounded(0, 127))]
        case .metric: [Field("grid", .length), Field("offset", .length), Field("within", .length)]
        case .everyNth: [Field("every", .bounded(1, 1_000_000)), Field("starting at", .count)]
        case .kinds, .dest: []
        }
    }

    public var numbers: [Int64] {
        switch self {
        case let .timeRange(s, e): [s, e]
        case let .pitch(l, h), let .bracketedBy(l, h): [Int64(l), Int64(h)]
        case let .velocity(l, h), let .releaseVelocity(l, h): [Int64(l), Int64(h)]
        case let .duration(a, b): [a, b]
        case let .value(l, h): [Int64(l), Int64(h)]
        case let .controller(l, h): [Int64(l), Int64(h)]
        case let .metric(g, o, t): [g, o, t]
        case let .everyNth(n, p): [Int64(n), Int64(p)]
        case .kinds, .dest: []
        }
    }

    /// The test with its numbers replaced (clamped to their types).
    public func with(_ n: [Int64]) -> ConditionTest {
        let old = numbers
        func at(_ i: Int) -> Int64 { n.indices.contains(i) ? n[i] : (old.indices.contains(i) ? old[i] : 0) }
        func pitch(_ v: Int64) -> UInt8 { UInt8(min(max(v, 0), 127)) }
        switch self {
        case .timeRange: return .timeRange(start: at(0), end: at(1))
        case .pitch: return .pitch(low: pitch(at(0)), high: pitch(at(1)))
        case .bracketedBy: return .bracketedBy(low: pitch(at(0)), high: pitch(at(1)))
        case .velocity: return .velocity(low: UInt16(clamping: at(0)), high: UInt16(clamping: at(1)))
        case .releaseVelocity: return .releaseVelocity(low: UInt16(clamping: at(0)), high: UInt16(clamping: at(1)))
        case .duration: return .duration(min: at(0), max: at(1))
        case .value: return .value(low: UInt32(clamping: at(0)), high: UInt32(clamping: at(1)))
        case .controller: return .controller(low: pitch(at(0)), high: pitch(at(1)))
        case .metric: return .metric(grid: max(at(0), 1), offset: at(1), tolerance: max(at(2), 0))
        case .everyNth: return .everyNth(n: UInt32(clamping: max(at(0), 1)), phase: UInt32(clamping: at(1)))
        case .kinds, .dest: return self
        }
    }
}

extension ModifyRow {
    static func unit(_ t: ValueTargetRow, _ e: Edge) -> FieldKind {
        switch t {
        case .velocity, .releaseVelocity: .velocity(e)
        case .duration: .length
        case .value: .value(e)
        }
    }

    static func changeFields(_ t: ValueTargetRow, _ op: ChangeOpRow, _ a: AmountRow) -> [Field] {
        switch op {
        case .limit: return [Field("lowest", unit(t, .low)), Field("highest", unit(t, .high))]
        case .compress: return [Field("lowest", unit(t, .exact)), Field("highest", unit(t, .exact))]
        case .legato: return []
        case .set, .scale, .add:
            let k: FieldKind = op == .scale ? .percent : unit(t, op == .add ? .delta : .exact)
            switch a {
            case .constant: return [Field("amount", k)]
            case .line: return [Field("from", k), Field("to", k)]
            case .oscillator: return [Field("low", k), Field("high", k), Field("period", .length), Field("phase", .length)]
            case .random: return [Field("low", k), Field("high", k)]
            }
        }
    }

    public var fields: [Field] {
        switch self {
        case .transposeInterval: [Field("semitones", .bounded(-127, 127))]
        case .transposeDiatonic: [Field("steps", .bounded(-127, 127))]
        case .transposeInvert: [Field("around", .pitch)]
        case .quantize: [Field("grid", .length), Field("strength", .percent)]
        case let .change(t, op, a): Self.changeFields(t, op, a)
        case .move: [Field("by", .length)]
        case .scaleTime: [Field("times", .bounded(1, 1_000_000)), Field("divided by", .bounded(1, 1_000_000)), Field("around", .position)]
        case .trimStart: [Field("by", .length)]
        case .substitute(.controller): [Field("controller", .bounded(0, 127))]
        case .substitute(.pitch): [Field("pitch", .pitch)]
        case .thin, .thicken: [Field("spacing", .length)]
        case .transposeToScale, .noteMap, .setDest, .substitute, .delete: []
        }
    }

    public var numbers: [Int64] {
        switch self {
        case .transposeInterval(let s): return [Int64(s)]
        case .transposeDiatonic(let s, _, _): return [Int64(s)]
        case .transposeInvert(let a): return [Int64(a)]
        case let .quantize(g, s, _): return [g, Int64(s)]
        case let .change(_, op, amount):
            switch op {
            case let .limit(a, b), let .compress(a, b): return [a, b]
            case .legato: return []
            case .set, .scale, .add:
                switch amount {
                case .constant(let v): return [v]
                case let .line(f, t, _): return [f, t]
                case let .oscillator(_, l, h, p, ph): return [l, h, p, ph]
                case let .random(l, h, _): return [l, h]
                }
            }
        case .move(let d), .trimStart(let d): return [d]
        case let .scaleTime(n, d, a): return [Int64(n), Int64(d), a]
        case .substitute(.controller(let i)): return [Int64(i)]
        case .substitute(.pitch(let p)): return [Int64(p)]
        case .thin(let s), .thicken(let s): return [s]
        case .transposeToScale, .noteMap, .setDest, .substitute, .delete: return []
        }
    }

    /// The modification with its numbers replaced (clamped to their types).
    public func with(_ n: [Int64]) -> ModifyRow {
        let old = numbers
        func at(_ i: Int) -> Int64 { n.indices.contains(i) ? n[i] : (old.indices.contains(i) ? old[i] : 0) }
        func pitch(_ v: Int64) -> UInt8 { UInt8(min(max(v, 0), 127)) }
        switch self {
        case .transposeInterval:
            return .transposeInterval(semitones: Int8(clamping: at(0)))
        case let .transposeDiatonic(_, r, m):
            return .transposeDiatonic(steps: Int8(clamping: at(0)), root: r, mask: m)
        case .transposeInvert:
            return .transposeInvert(axis: pitch(at(0)))
        case let .quantize(_, _, o):
            return .quantize(grid: max(at(0), 1), strength: UInt8(min(max(at(1), 0), 100)), offsets: o)
        case let .change(t, op, amount):
            switch op {
            case .limit: return .change(target: t, op: .limit(min: at(0), max: at(1)), amount: amount)
            case .compress: return .change(target: t, op: .compress(low: at(0), high: at(1)), amount: amount)
            case .legato: return self
            case .set, .scale, .add:
                let a: AmountRow
                switch amount {
                case .constant: a = .constant(value: at(0))
                case let .line(_, _, c): a = .line(from: at(0), to: at(1), curve: c)
                case let .oscillator(w, _, _, _, _):
                    a = .oscillator(wave: w, low: at(0), high: at(1), period: max(at(2), 1), phase: at(3))
                case let .random(_, _, s): a = .random(low: at(0), high: at(1), seed: s)
                }
                return .change(target: t, op: op, amount: a)
            }
        case .move: return .move(delta: at(0))
        case .trimStart: return .trimStart(delta: at(0))
        case .scaleTime:
            return .scaleTime(
                num: UInt32(clamping: max(at(0), 1)), den: UInt32(clamping: max(at(1), 1)), anchor: at(2))
        case .substitute(.controller): return .substitute(to: .controller(index: pitch(at(0))))
        case .substitute(.pitch): return .substitute(to: .pitch(pitch: pitch(at(0))))
        case .thin: return .thin(spacing: max(at(0), 1))
        case .thicken: return .thicken(spacing: max(at(0), 1))
        case .transposeToScale, .noteMap, .setDest, .substitute, .delete: return self
        }
    }
}

/// A note map as "C2>D2, E2>F#2".
public enum NoteMapText {
    public static func text(_ m: [NoteMapEntry]) -> String {
        m.map { "\(PitchName.name($0.from))>\(PitchName.name($0.to))" }.joined(separator: ", ")
    }

    public static func parse(_ s: String) -> [NoteMapEntry]? {
        var out: [NoteMapEntry] = []
        for item in s.split(separator: ",") {
            let p = item.split(separator: ">")
            guard p.count == 2, let a = PitchName.parse(String(p[0])), let b = PitchName.parse(String(p[1])) else {
                return nil
            }
            out.append(NoteMapEntry(from: a, to: b))
        }
        return out
    }
}

/// Groove offsets in ticks as "0 120 -60".
public enum OffsetsText {
    public static func text(_ o: [Int64]) -> String { o.map(String.init).joined(separator: " ") }

    public static func parse(_ s: String) -> [Int64]? {
        var out: [Int64] = []
        for w in s.split(separator: " ") {
            guard let v = Int64(w), abs(v) <= 1_000_000 else { return nil }
            out.append(v)
        }
        return out
    }
}
