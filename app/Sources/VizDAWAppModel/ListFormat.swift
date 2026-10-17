import Foundation
import VizDAWCore

/// A position as bar·beat·unit, 960 units per beat as the transport shows it;
/// pickup bars count down from 0 (Plan 4c Ruling 15).
public struct Bbu: Equatable, Sendable {
    public var bar: Int64
    public var beat: Int64
    public var unit: Int64

    public static let unitsPerBeat: Int64 = 960
    /// Typed values outside these are refused.
    public static let bars: ClosedRange<Int64> = -1_000_000...1_000_000
    public static let beats: ClosedRange<Int64> = 1...999
    public static let units: ClosedRange<Int64> = 0...959

    public init(bar: Int64, beat: Int64, unit: Int64) {
        self.bar = bar
        self.beat = beat
        self.unit = unit
    }

    /// The position of `tick` (the core's `tick_to_bbu`).
    public static func of(_ tick: Int64, meter: [MeterEntryRow]) -> Bbu {
        let b = MeterMath.bar(at: tick, meter)
        let beatT = MeterMath.beatTicks(b.meter)
        let inBar = tick - b.start
        return Bbu(bar: b.number, beat: inBar / beatT + 1, unit: (inBar % beatT) * unitsPerBeat / beatT)
    }

    /// The first tick of this position (the core's `bbu_to_tick`).
    public func tick(meter: [MeterEntryRow]) -> Int64 {
        let start = MeterMath.barStart(bar, meter)
        let beatT = MeterMath.beatTicks(MeterMath.bar(at: start, meter).meter)
        let units = (unit * beatT + Self.unitsPerBeat - 1) / Self.unitsPerBeat
        return start + (max(beat, 1) - 1) * beatT + units
    }

    public var text: String { "\(bar).\(beat).\(unit)" }
}

/// Typed positions (Plan 4c Ruling 15): "5" is bar 5's downbeat, "5.3" and
/// "5.3.480" name more; an empty part keeps that sub-field of the current
/// position (".3" the beat only, "..480" the unit only, "5.." the bar only).
public enum PositionInput {
    public static func parse(_ text: String, current: Bbu) -> Bbu? {
        let s = text.trimmingCharacters(in: .whitespaces)
        guard !s.isEmpty else { return nil }
        let parts = s.split(separator: ".", omittingEmptySubsequences: false).map(String.init)
        guard parts.count <= 3 else { return nil }
        let subField = parts[0].isEmpty
        var out = subField ? current : Bbu(bar: current.bar, beat: 1, unit: 0)
        for (i, p) in parts.enumerated() {
            if p.isEmpty {
                if !subField, i == 1 { out.beat = current.beat }
                if !subField, i == 2 { out.unit = current.unit }
                continue
            }
            guard let v = Int64(p) else { return nil }
            switch i {
            case 0: out.bar = v
            case 1: out.beat = v
            default: out.unit = v
            }
        }
        guard Bbu.bars.contains(out.bar), Bbu.beats.contains(out.beat), Bbu.units.contains(out.unit) else {
            return nil
        }
        return out
    }
}

/// Lengths as beats.units of a beat of `beat` ticks (Plan 4c Ruling 15).
public enum DurationText {
    public static let maxBeats: Int64 = 1_000_000

    public static func text(_ ticks: Int64, beat: Int64) -> String {
        let b = max(beat, 1)
        return "\(ticks / b).\((ticks % b) * Bbu.unitsPerBeat / b)"
    }

    /// The ticks `text` means; an empty part keeps `current`'s; nil unless a
    /// length of 0 or more.
    public static func parse(_ text: String, current: Int64, beat: Int64) -> Int64? {
        let b = max(beat, 1)
        let s = text.trimmingCharacters(in: .whitespaces)
        guard !s.isEmpty else { return nil }
        let parts = s.split(separator: ".", omittingEmptySubsequences: false).map(String.init)
        guard parts.count <= 2 else { return nil }
        var beats = max(current, 0) / b
        var units = (max(current, 0) % b) * Bbu.unitsPerBeat / b
        if !parts[0].isEmpty {
            guard let v = Int64(parts[0]), (0...maxBeats).contains(v) else { return nil }
            beats = v
            if parts.count == 1 { units = 0 }
        }
        if parts.count == 2, !parts[1].isEmpty {
            guard let u = Int64(parts[1]), Bbu.units.contains(u) else { return nil }
            units = u
        }
        return beats * b + (units * b + Bbu.unitsPerBeat - 1) / Bbu.unitsPerBeat
    }
}

/// `hh:mm:ss:ff` (Plan 4c Ruling 14).
public enum Smpte {
    public static let rates = [24, 25, 30]

    public static func text(seconds: Double, fps: Int) -> String {
        guard seconds.isFinite else { return "--:--:--:--" }
        let f = Int64(max(fps, 1))
        let frames = Int64(min((abs(seconds) * Double(f)).rounded(.down), 9e15))
        let ff = frames % f
        let total = frames / f
        let sign = seconds < 0 && frames > 0 ? "-" : ""
        return sign + String(format: "%02lld:%02lld:%02lld:%02lld", total / 3600, (total / 60) % 60, total % 60, ff)
    }

    /// Seconds of a typed SMPTE time; fewer fields count from the right
    /// ("12" is 12 frames, "1:12" one second and 12 frames).
    public static func parse(_ text: String, fps: Int) -> Double? {
        var s = Substring(text.trimmingCharacters(in: .whitespaces))
        let negative = s.hasPrefix("-")
        if negative { s = s.dropFirst() }
        let parts = s.split(separator: ":", omittingEmptySubsequences: false)
        guard (1...4).contains(parts.count) else { return nil }
        var values: [Int64] = []
        for p in parts {
            guard let v = Int64(p), (0...1_000_000).contains(v) else { return nil }
            values.append(v)
        }
        let f = Int64(max(fps, 1))
        let v = Array(repeating: Int64(0), count: 4 - values.count) + values
        guard v[3] < f, values.count < 3 || v[2] < 60, values.count < 4 || v[1] < 60 else { return nil }
        let secs = Double(v[0] * 3600 + v[1] * 60 + v[2]) + Double(v[3]) / Double(f)
        return negative ? -secs : secs
    }
}

/// 16-bit velocities shown and typed as MIDI 1.0 values (Plan 4c Ruling 16).
public enum Velocity7 {
    public static func of(_ v: UInt16) -> Int { Int(v >> 9) }

    public static func up(_ v: Int) -> UInt16 {
        let c = UInt16(min(max(v, 0), 127))
        let shifted = c << 9
        guard c > 64 else { return shifted }
        let rep = c & 0x3F
        return shifted | (rep << 3) | (rep >> 3)
    }
}

/// 32-bit controller and pressure values shown and typed as 0–127.
public enum Value7 {
    public static func of(_ v: Int64) -> Int { Int(UInt32(clamping: v) >> 25) }

    public static func up(_ v: Int) -> UInt32 {
        let c = UInt32(min(max(v, 0), 127))
        let shifted = c << 25
        guard c > 64 else { return shifted }
        let rep = c & 0x3F
        return shifted | (rep << 19) | (rep << 13) | (rep << 7) | (rep << 1) | (rep >> 5)
    }
}

/// 32-bit pitch bend shown and typed as −8192…8191.
public enum Bend14 {
    public static func of(_ v: Int64) -> Int { Int(UInt32(clamping: v) >> 18) - 8192 }

    public static func up(_ signed: Int) -> UInt32 {
        let u = UInt32(min(max(signed + 8192, 0), 16_383))
        let shifted = u << 18
        guard u > 8192 else { return shifted }
        let rep = u & 0x1FFF
        return shifted | (rep << 5) | (rep >> 8)
    }
}

extension PitchName {
    /// "C4", "c#4", "Db4", "C-1", or a number 0–127.
    public static func parse(_ text: String) -> UInt8? {
        let s = text.trimmingCharacters(in: .whitespaces)
        if let n = Int(s) { return (0...127).contains(n) ? UInt8(n) : nil }
        let bases: [String: Int] = ["C": 0, "D": 2, "E": 4, "F": 5, "G": 7, "A": 9, "B": 11]
        guard let first = s.first, let base = bases[String(first).uppercased()] else { return nil }
        var rest = s.dropFirst()
        var pc = base
        if let a = rest.first, a == "#" || a == "♯" {
            pc += 1
            rest = rest.dropFirst()
        } else if let a = rest.first, a == "b" || a == "♭" {
            pc -= 1
            rest = rest.dropFirst()
        }
        guard let octave = Int(rest) else { return nil }
        let n = (octave + 1) * 12 + pc
        return (0...127).contains(n) ? UInt8(n) : nil
    }
}

/// Key signatures by name ("D major", "F# minor").
public enum KeyName {
    /// By sharps −7…7.
    static let majors = ["Cb", "Gb", "Db", "Ab", "Eb", "Bb", "F", "C", "G", "D", "A", "E", "B", "F#", "C#"]
    static let minors = ["Ab", "Eb", "Bb", "F", "C", "G", "D", "A", "E", "B", "F#", "C#", "G#", "D#", "A#"]

    public static func name(sharps: Int8, minor: Bool) -> String {
        let i = Int(sharps) + 7
        guard (0..<15).contains(i) else { return "\(sharps) sharps" }
        return minor ? "\(minors[i]) minor" : "\(majors[i]) major"
    }

    public static func parse(_ text: String) -> (sharps: Int8, minor: Bool)? {
        let words = text.trimmingCharacters(in: .whitespaces).split(separator: " ")
        guard words.count == 2, let f = words[0].first else { return nil }
        let isMinor: Bool
        switch words[1].lowercased() {
        case "major": isMinor = false
        case "minor": isMinor = true
        default: return nil
        }
        let root = String(f).uppercased() + words[0].dropFirst().lowercased()
        guard let i = (isMinor ? minors : majors).firstIndex(of: root) else { return nil }
        return (Int8(i - 7), isMinor)
    }
}
