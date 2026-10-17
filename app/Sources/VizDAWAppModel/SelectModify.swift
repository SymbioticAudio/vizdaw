import Foundation
import VizDAWCore

/// What a Modify menu command opens the panel for (parent spec §9).
public enum PanelTask: String, CaseIterable, Sendable {
    case selectByRule, transpose, quantize, change, move

    public var title: String {
        switch self {
        case .selectByRule: "Select by Rule"
        case .transpose: "Transpose"
        case .quantize: "Quantize"
        case .change: "Change Values"
        case .move: "Move"
        }
    }
}

/// The select half (Plan 4c Ruling 7).
public struct SelectHalf: Equatable, Sendable {
    public var enabled: Bool
    public var matchAll: Bool
    public var mode: SelectModeRow
    public var conditions: [ConditionRow]

    public init(enabled: Bool = false, matchAll: Bool = true, mode: SelectModeRow = .select, conditions: [ConditionRow] = []) {
        self.enabled = enabled
        self.matchAll = matchAll
        self.mode = mode
        self.conditions = conditions
    }
}

/// The modify half.
public struct ModifyHalf: Equatable, Sendable {
    public var enabled: Bool
    public var op: ModifyRow
    /// To a copy (a harmonizing copy for transposes).
    public var toCopy: Bool

    public init(enabled: Bool = true, op: ModifyRow = .transposeInterval(semitones: 0), toCopy: Bool = false) {
        self.enabled = enabled
        self.op = op
        self.toCopy = toCopy
    }
}

/// Everything the panel holds; a template stores exactly this.
public struct SelectModifyConfig: Equatable, Sendable {
    public var select = SelectHalf()
    public var modify = ModifyHalf()

    public init() {}

    /// The panel set to `task`; Select by Rule starts with Kind is Note and,
    /// when the selection has a range, a Position condition over it.
    public static func preset(_ task: PanelTask, range: TickRange? = nil) -> SelectModifyConfig {
        var c = SelectModifyConfig()
        switch task {
        case .selectByRule:
            c.select.enabled = true
            c.select.conditions = [ConditionRow(test: .kinds(kinds: [.note]), negate: false)]
            if let r = range {
                c.select.conditions.append(ConditionRow(test: .timeRange(start: r.start, end: r.end), negate: false))
            }
            c.modify.enabled = false
        case .transpose:
            c.modify.op = .transposeInterval(semitones: 0)
        case .quantize:
            c.modify.op = .quantize(grid: MeterMath.ppq / 4, strength: 100, offsets: [])
        case .change:
            c.modify.op = .change(target: .velocity, op: .add, amount: .constant(value: 0))
        case .move:
            c.modify.op = .move(delta: 0)
        }
        return c
    }

    public var rule: RuleRow? {
        select.enabled && !select.conditions.isEmpty
            ? RuleRow(conditions: select.conditions, matchAll: select.matchAll) : nil
    }

    public var modification: ModifyRow? { modify.enabled ? modify.op : nil }

    /// What Apply sends; nil when neither half has anything to do.
    public func command() -> EditCommand? {
        let r = rule
        let m = modification
        guard r != nil || m != nil else { return nil }
        return .selectAndModify(rule: r, mode: select.mode, modify: m, toCopy: m != nil && modify.toCopy)
    }

    public init(template t: ModifyTemplateRow) {
        select = SelectHalf(
            enabled: t.rule != nil, matchAll: t.rule?.matchAll ?? true, mode: t.mode, conditions: t.rule?.conditions ?? [])
        modify = ModifyHalf(enabled: t.modify != nil, op: t.modify ?? .transposeInterval(semitones: 0), toCopy: t.toCopy)
    }

    public func template(named name: String) -> ModifyTemplateRow {
        ModifyTemplateRow(name: name, rule: rule, mode: select.mode, modify: modification, toCopy: modify.toCopy)
    }
}

/// The attributes a condition tests (parent spec §9 select half).
public enum ConditionKind: String, CaseIterable, Sendable {
    case time, kind, pitch, velocity, releaseVelocity, duration, value, controller, destination, metric, everyNth
    case bracketed, automation

    public var title: String {
        switch self {
        case .time: "Position"
        case .kind: "Kind"
        case .pitch: "Pitch"
        case .velocity: "Velocity"
        case .releaseVelocity: "Release Velocity"
        case .duration: "Length"
        case .value: "Value"
        case .controller: "Controller"
        case .destination: "Destination"
        case .metric: "Metric Position"
        case .everyNth: "Every Nth"
        case .bracketed: "Inside a Note"
        case .automation: "Automation Target"
        }
    }

    public init(_ t: ConditionTest) {
        switch t {
        case .timeRange: self = .time
        case .kinds: self = .kind
        case .pitch: self = .pitch
        case .velocity: self = .velocity
        case .releaseVelocity: self = .releaseVelocity
        case .duration: self = .duration
        case .value: self = .value
        case .controller: self = .controller
        case .dest: self = .destination
        case .metric: self = .metric
        case .everyNth: self = .everyNth
        case .bracketedBy: self = .bracketed
        case .automation: self = .automation
        }
    }

    /// A new condition of this kind; Position covers `range` (else the first
    /// four 4/4 bars).
    public func test(range: TickRange?) -> ConditionTest {
        let q = MeterMath.ppq
        switch self {
        case .time:
            let r = range ?? TickRange(start: 0, end: 16 * q)
            return .timeRange(start: r.start, end: r.end)
        case .kind: return .kinds(kinds: [.note])
        case .pitch: return .pitch(low: 60, high: 72)
        case .velocity: return .velocity(low: 0, high: 0xFFFF)
        case .releaseVelocity: return .releaseVelocity(low: 0, high: 0xFFFF)
        case .duration: return .duration(min: 0, max: q)
        case .value: return .value(low: 0, high: UInt32.max)
        case .controller: return .controller(low: 7, high: 7)
        case .destination: return .dest(dest: nil)
        case .metric: return .metric(grid: q, offset: 0, tolerance: q / 16)
        case .everyNth: return .everyNth(n: 2, phase: 0)
        case .bracketed: return .bracketedBy(low: 0, high: 127)
        case .automation: return .automation(target: .stripFader(strip: 0))
        }
    }
}

/// How a condition compares (parent spec §9: equal, not equal, inside or
/// outside a range); "not" is the condition's `negate`.
public enum Comparison: String, CaseIterable, Sendable {
    case equal, notEqual, inside, outside

    public var title: String {
        switch self {
        case .equal: "is"
        case .notEqual: "is not"
        case .inside: "inside"
        case .outside: "outside"
        }
    }

    public var negates: Bool { self == .notEqual || self == .outside }

    public static func choices(for t: ConditionTest) -> [Comparison] {
        switch ConditionKind(t) {
        case .time: [.inside, .outside]
        case .pitch, .velocity, .releaseVelocity, .duration, .value, .controller, .bracketed: allCases
        default: [.equal, .notEqual]
        }
    }

    /// A range reads as "is" when both ends show the same text.
    public static func of(_ c: ConditionRow) -> Comparison {
        let choices = choices(for: c.test)
        guard choices.contains(.inside) else { return c.negate ? .notEqual : .equal }
        let f = c.test.fields
        let n = c.test.numbers
        if choices.contains(.equal), f.count == 2, n.count == 2,
            FieldText.text(n[0], f[0].kind, meter: []) == FieldText.text(n[1], f[1].kind, meter: [])
        {
            return c.negate ? .notEqual : .equal
        }
        return c.negate ? .outside : .inside
    }

    /// The condition compared this way; "is" and "is not" on a range make
    /// its high end the same step as its low end.
    public func applied(to c: ConditionRow) -> ConditionRow {
        var out = c
        out.negate = negates
        let f = c.test.fields
        let n = c.test.numbers
        if self == .equal || self == .notEqual, Comparison.choices(for: c.test).contains(.inside), f.count == 2,
            n.count == 2
        {
            let low = FieldText.text(n[0], f[0].kind, meter: [])
            out.test = c.test.with([n[0], FieldText.parse(low, f[1].kind, meter: []) ?? n[0]])
        }
        return out
    }
}

/// The panel's modify choices (parent spec §9 modify half).
public enum ModifyKind: String, CaseIterable, Sendable {
    case transpose, diatonic, toScale, invert, noteMap, quantize, change, move, scaleTime, trimStart, setDest
    case substitute, thin, thicken, delete

    public var title: String {
        switch self {
        case .transpose: "Transpose"
        case .diatonic: "Transpose in Scale"
        case .toScale: "Snap to Scale"
        case .invert: "Invert"
        case .noteMap: "Note Map"
        case .quantize: "Quantize"
        case .change: "Change Values"
        case .move: "Move"
        case .scaleTime: "Scale Time"
        case .trimStart: "Trim Start"
        case .setDest: "Set Destination"
        case .substitute: "Substitute"
        case .thin: "Thin Out"
        case .thicken: "Thicken"
        case .delete: "Delete"
        }
    }

    public init(_ r: ModifyRow) {
        switch r {
        case .transposeInterval: self = .transpose
        case .transposeDiatonic: self = .diatonic
        case .transposeToScale: self = .toScale
        case .transposeInvert: self = .invert
        case .noteMap: self = .noteMap
        case .quantize: self = .quantize
        case .change: self = .change
        case .move: self = .move
        case .scaleTime: self = .scaleTime
        case .trimStart: self = .trimStart
        case .setDest: self = .setDest
        case .substitute: self = .substitute
        case .thin: self = .thin
        case .thicken: self = .thicken
        case .delete: self = .delete
        }
    }

    /// A new modification of this kind.
    public var row: ModifyRow {
        let q = MeterMath.ppq
        switch self {
        case .transpose: return .transposeInterval(semitones: 0)
        case .diatonic: return .transposeDiatonic(steps: 1, root: 0, mask: Scale.major.mask)
        case .toScale: return .transposeToScale(root: 0, mask: Scale.major.mask)
        case .invert: return .transposeInvert(axis: 60)
        case .noteMap: return .noteMap(map: [])
        case .quantize: return .quantize(grid: q / 4, strength: 100, offsets: [])
        case .change: return .change(target: .velocity, op: .add, amount: .constant(value: 0))
        case .move: return .move(delta: 0)
        case .scaleTime: return .scaleTime(num: 1, den: 1, anchor: 0)
        case .trimStart: return .trimStart(delta: 0)
        case .setDest: return .setDest(dest: nil)
        case .substitute: return .substitute(to: .controller(index: 1))
        case .thin: return .thin(spacing: q / 16)
        case .thicken: return .thicken(spacing: q / 16)
        case .delete: return .delete
        }
    }

    /// The step's name as the undo menu shows it ("Modify Again: …").
    public static func title(_ r: ModifyRow) -> String {
        switch r {
        case .transposeInterval, .transposeDiatonic, .transposeToScale, .transposeInvert, .noteMap: "Transpose"
        case .quantize: "Quantize"
        case .change(let t, _, _):
            switch t {
            case .velocity: "Change Velocity"
            case .releaseVelocity: "Change Release Velocity"
            case .duration: "Change Duration"
            case .value: "Change Values"
            }
        case .move: "Move"
        case .scaleTime: "Scale Time"
        case .trimStart: "Trim Start"
        case .setDest: "Reassign Destination"
        case .substitute: "Substitute"
        case .thin: "Thin Out"
        case .thicken: "Thicken"
        case .delete: "Delete Events"
        }
    }
}

/// The change operations, for a picker.
public enum ChangeOpKind: String, CaseIterable, Sendable {
    case set, scale, add, limit, compress, legato

    public var title: String {
        switch self {
        case .set: "Set to"
        case .scale: "Scale by"
        case .add: "Add"
        case .limit: "Limit to"
        case .compress: "Compress to"
        case .legato: "Legato"
        }
    }

    public init(_ op: ChangeOpRow) {
        switch op {
        case .set: self = .set
        case .scale: self = .scale
        case .add: self = .add
        case .limit: self = .limit
        case .compress: self = .compress
        case .legato: self = .legato
        }
    }

    /// The operation with the target's full range for limits.
    public func row(for t: ValueTargetRow) -> ChangeOpRow {
        let top: Int64 =
            switch t {
            case .velocity, .releaseVelocity: 0xFFFF
            case .duration: 4 * MeterMath.ppq
            case .value: Int64(UInt32.max)
            }
        switch self {
        case .set: return .set
        case .scale: return .scale
        case .add: return .add
        case .limit: return .limit(min: 0, max: top)
        case .compress: return .compress(low: 0, high: top)
        case .legato: return .legato
        }
    }
}

/// The amount shapes, for a picker (Plan 4c Ruling 3).
public enum AmountKind: String, CaseIterable, Sendable {
    case constant, line, oscillator, random

    public var title: String {
        switch self {
        case .constant: "Constant"
        case .line: "Along a Line"
        case .oscillator: "Oscillating"
        case .random: "Random"
        }
    }

    public init(_ a: AmountRow) {
        switch a {
        case .constant: self = .constant
        case .line: self = .line
        case .oscillator: self = .oscillator
        case .random: self = .random
        }
    }

    public func row(seed: UInt64) -> AmountRow {
        switch self {
        case .constant: .constant(value: 0)
        case .line: .line(from: 0, to: 0, curve: .straight)
        case .oscillator: .oscillator(wave: .sine, low: 0, high: 0, period: MeterMath.ppq, phase: 0)
        case .random: .random(low: 0, high: 0, seed: seed)
        }
    }
}

extension Scale {
    /// The named scale with this mask, if any.
    public static func of(mask: UInt16) -> Scale? { allCases.first { $0.mask == mask } }
}
