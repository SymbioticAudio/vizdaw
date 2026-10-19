import Testing
import VizDAWCore
@testable import VizDAWAppModel

private let q: Int64 = 15_360
private let meter44 = [MeterEntryRow(tick: 0, num: 4, den: 4, sharps: 0, minor: false)]

@Suite struct SelectModifyTests {
    @Test func presetsMakeTheirTasksCommand() {
        #expect(
            SelectModifyConfig.preset(.transpose).command()
                == .selectAndModify(rule: nil, mode: .select, modify: .transposeInterval(semitones: 0), toCopy: false))
        #expect(SelectModifyConfig.preset(.quantize).modify.op == .quantize(grid: q / 4, strength: 100, offsets: []))
        #expect(SelectModifyConfig.preset(.move).modify.op == .move(delta: 0))
        let rule = SelectModifyConfig.preset(.selectByRule, range: TickRange(start: 0, end: 4 * q))
        #expect(
            rule.command()
                == .selectAndModify(
                    rule: RuleRow(
                        conditions: [
                            ConditionRow(test: .kinds(kinds: [.note]), negate: false),
                            ConditionRow(test: .timeRange(start: 0, end: 4 * q), negate: false),
                        ], matchAll: true),
                    mode: .select, modify: nil, toCopy: false))
    }

    @Test func nothingTurnedOnIsNoCommand() {
        var c = SelectModifyConfig.preset(.change)
        c.modify.enabled = false
        #expect(c.command() == nil)
        c.select.enabled = true
        #expect(c.command() == nil, "a select half without conditions does nothing")
    }

    @Test func templatesRoundTrip() {
        var c = SelectModifyConfig.preset(.selectByRule, range: TickRange(start: 0, end: q))
        c.select.mode = .refine
        c.select.matchAll = false
        c.modify.enabled = true
        c.modify.op = .change(target: .velocity, op: .add, amount: .line(from: 0, to: 2048, curve: .easeIn))
        c.modify.toCopy = true
        let t = c.template(named: "Swell")
        #expect(t.name == "Swell" && t.toCopy && t.mode == .refine)
        #expect(SelectModifyConfig(template: t) == c)
    }

    @Test func comparisonsReadAndSetRanges() {
        let hundred = ConditionRow(test: .velocity(low: 100 << 9, high: (100 << 9) | 511), negate: false)
        #expect(Comparison.of(hundred) == .equal)
        #expect(Comparison.of(Comparison.outside.applied(to: hundred)) == .notEqual, "still one step")
        let wide = ConditionRow(test: .velocity(low: 50 << 9, high: 0xFFFF), negate: false)
        #expect(Comparison.of(wide) == .inside)
        #expect(Comparison.equal.applied(to: wide).test == .velocity(low: 50 << 9, high: (50 << 9) | 511))
        #expect(Comparison.outside.applied(to: wide).negate)
        #expect(Comparison.choices(for: .timeRange(start: 0, end: 1)) == [.inside, .outside])
        #expect(Comparison.choices(for: .kinds(kinds: [.note])) == [.equal, .notEqual])
        #expect(Comparison.of(ConditionRow(test: .dest(dest: 5), negate: true)) == .notEqual)
    }

    @Test func kindsRoundTripThroughTheirDefaults() {
        for k in ConditionKind.allCases { #expect(ConditionKind(k.test(range: nil)) == k, "\(k)") }
        for k in ModifyKind.allCases { #expect(ModifyKind(k.row) == k, "\(k)") }
        #expect(ModifyKind.title(.change(target: .duration, op: .add, amount: .constant(value: 1))) == "Change Duration")
        #expect(ModifyKind.title(.transposeDiatonic(steps: 1, root: 0, mask: 1)) == "Transpose")
        #expect(ModifyKind.title(.setDest(dest: nil)) == "Reassign Destination")
        for k in ChangeOpKind.allCases { #expect(ChangeOpKind(k.row(for: .velocity)) == k) }
        for k in AmountKind.allCases { #expect(AmountKind(k.row(seed: 1)) == k) }
        #expect(Scale.of(mask: Scale.dorian.mask) == .dorian && Scale.of(mask: 1) == nil)
    }

    @Test func fieldsAndNumbersAgree() {
        var tests = ConditionKind.allCases.map { $0.test(range: nil) }
        tests.append(.metric(grid: q, offset: 5, tolerance: 7))
        for t in tests {
            #expect(t.fields.count == t.numbers.count, "\(t)")
            #expect(t.with(t.numbers) == t, "\(t)")
        }
        var rows = ModifyKind.allCases.map(\.row)
        for op in ChangeOpKind.allCases {
            for a in AmountKind.allCases {
                rows.append(.change(target: .value, op: op.row(for: .value), amount: a.row(seed: 3)))
            }
        }
        for r in rows {
            #expect(r.fields.count == r.numbers.count, "\(r)")
            #expect(r.with(r.numbers) == r, "\(r)")
        }
        #expect(
            ModifyRow.change(target: .velocity, op: .scale, amount: .constant(value: 100)).fields
                == [Field("amount", .percent)])
        #expect(
            ModifyRow.change(target: .velocity, op: .add, amount: .constant(value: 0)).fields
                == [Field("amount", .velocity(.delta))])
    }

    @Test func fieldTextRoundTrips() {
        func parse(_ s: String, _ k: FieldKind) -> Int64? { FieldText.parse(s, k, meter: meter44) }
        func text(_ n: Int64, _ k: FieldKind) -> String { FieldText.text(n, k, meter: meter44) }
        #expect(parse("100", .velocity(.exact)) == 0xC924 && text(0xC924, .velocity(.exact)) == "100")
        #expect(parse("100", .velocity(.high)) == 51_711)
        #expect(parse("-10", .velocity(.delta)) == -5120 && text(-5120, .velocity(.delta)) == "-10")
        #expect(parse("127", .value(.exact)) == 0xFFFF_FFFF)
        #expect(parse("-1.480", .length) == -(q + q / 2) && text(-(q + q / 2), .length) == "-1.480")
        #expect(parse("2", .position) == 4 * q && text(4 * q, .position) == "2.1.0")
        #expect(parse("80%", .percent) == 80 && text(80, .percent) == "80%")
        #expect(parse("C4", .pitch) == 60 && text(60, .pitch) == "C4")
        #expect(parse("abc", .count) == nil && parse("200", .velocity(.exact)) == nil)
    }

    @Test func noteMapAndGrooveText() {
        let m = NoteMapText.parse("C2>D2, E2>F#2")
        #expect(m == [NoteMapEntry(from: 36, to: 38), NoteMapEntry(from: 40, to: 42)])
        #expect(NoteMapText.text(m ?? []) == "C2>D2, E2>F#2")
        #expect(NoteMapText.parse("C2") == nil)
        #expect(NoteMapText.parse("") == [])
        #expect(OffsetsText.parse("0 120 -60") == [0, 120, -60] && OffsetsText.text([0, 120, -60]) == "0 120 -60")
        #expect(OffsetsText.parse("0 x") == nil)
    }

    @Test func typedNumbersOutsideTheirRangeAreRejected() {
        func parse(_ s: String, _ k: FieldKind) -> Int64? { FieldText.parse(s, k, meter: meter44) }
        let semis = ModifyRow.transposeInterval(semitones: 0).fields[0].kind
        #expect(parse("127", semis) == 127 && parse("-127", semis) == -127)
        #expect(parse("128", semis) == nil && parse("-200", semis) == nil)
        #expect(parse("128", ConditionKind.controller.test(range: nil).fields[0].kind) == nil)
        #expect(parse("0", ConditionKind.everyNth.test(range: nil).fields[0].kind) == nil)
        #expect(parse("0", ModifyRow.scaleTime(num: 1, den: 1, anchor: 0).fields[0].kind) == nil)
    }

    @Test func everyKindRoundTripsItsTextOnGrid() {
        let cases: [(FieldKind, [Int64])] = [
            (.count, [0, 7, 1000]), (.bounded(-127, 127), [-127, 0, 127]), (.percent, [0, 80, 100, 200]),
            (.pitch, [0, 60, 127]),
            (.velocity(.exact), [Int64(Velocity7.up(0)), Int64(Velocity7.up(100)), Int64(Velocity7.up(127))]),
            (.velocity(.low), [0, 100 << 9, 127 << 9]), (.velocity(.high), [511, (100 << 9) | 511, 0xFFFF]),
            (.velocity(.delta), [-512, 0, 5120]),
            (.value(.exact), [Int64(Value7.up(0)), Int64(Value7.up(64)), Int64(Value7.up(127))]),
            (.value(.low), [0, 64 << 25]), (.value(.high), [0x1FF_FFFF, 0xFFFF_FFFF]), (.value(.delta), [-(1 << 25), 3 << 25]),
            (.length, [0, q / 4, q, -q, -(q + q / 2)]), (.position, [0, q, 4 * q, 9 * q + q / 2]),
        ]
        for (k, ns) in cases {
            for n in ns {
                let t = FieldText.text(n, k, meter: meter44)
                #expect(FieldText.parse(t, k, meter: meter44) == n, "\(k) \(n) -> \(t)")
            }
        }
    }
}

@Suite struct LineRowFieldTests {
    private let q: Int64 = 15_360

    @Test func lineFieldsRoundTripAndNameTheStep() {
        let osc = ModifyRow.line(
            target: .series(series: .controller(index: 7)), op: .add,
            line: LineRow(
                from: 0, to: 4 * q, start: -5, end: 5, step: q / 8,
                shape: .oscillator(wave: .sine, period: .millis(ms: 250), phase: 125)))
        #expect(osc.fields.map(\.label) == ["from", "to", "start", "end", "step", "period (ms)", "phase (‰)"])
        #expect(osc.fields[2].kind == .value(.delta))
        #expect(osc.numbers == [0, 4 * q, -5, 5, q / 8, 250, 125])
        #expect(osc.with(osc.numbers) == osc)
        // A typed `to` before `from` is held at `from`.
        if case .line(_, _, let l) = osc.with([4 * q, 0]) {
            #expect(l.from == 4 * q && l.to == 4 * q)
        } else {
            Issue.record("with() did not return a line")
        }
        #expect(ModifyKind.title(osc) == "Change Values")
        #expect(ModifyKind.lineTitle(.series(series: .program), .draw) == "Draw Program Changes")
        #expect(ModifyKind.lineTitle(.value(target: .releaseVelocity), .scale) == "Change Release Velocity")
        var c = SelectModifyConfig()
        c.modify = ModifyHalf(enabled: true, op: osc, toCopy: true)
        #expect(c.command() == .selectAndModify(rule: nil, mode: .select, modify: osc, toCopy: false))
    }
}

@Suite struct TempoLineFieldTests {
    @Test func tempoLinesTypeBeatsPerMinute() {
        let row = ModifyRow.line(
            target: .tempo, op: .draw,
            line: LineRow(from: 0, to: 61_440, start: 120_000, end: 90_500, step: 1_920, shape: .straight))
        #expect(row.fields[2].kind == .bpm)
        #expect(FieldText.text(120_000, .bpm, meter: []) == "120.00")
        #expect(FieldText.parse("90.5", .bpm, meter: []) == 90_500)
        #expect(FieldText.parse("-12", .bpm, meter: []) == -12_000)
        #expect(FieldText.parse("1200", .bpm, meter: []) == nil)
        #expect(ModifyKind.title(row) == "Draw Tempo")
        #expect(ModifyKind.lineTitle(.tempo, .limitMax) == "Change Tempo")
    }
}
