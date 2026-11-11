import SwiftUI
import SymbioticControls
import SymbioticTokens
import VizDAWAppModel
import VizDAWCore

/// A MIDI FX slot's settings popover (plan 5g-1 Task 9, Ruling 16): the
/// selected effect of the open track's chain, or nothing when that row is
/// gone. The title, its bypass and Remove, then the effect's controls.
/// Pickers, steppers and toggles edit once each (`update`); a slider drags
/// a draft and edits once at its end. The selection and the draft live in
/// `InputFxModel`.
struct InputFxSettings: View {
    let model: InputFxModel
    @Environment(\.symbioticTheme) private var theme

    var body: some View {
        let rows = model.chain
        if let i = model.selected, rows.indices.contains(i) {
            let row = rows[i]
            VStack(alignment: .leading, spacing: theme.spacing.s) {
                header(i, row)
                controls(i, row.kind)
            }
            .font(theme.type.caption.font)
            .controlSize(.small)
            .padding(theme.spacing.m)
            .frame(width: 260)
        }
    }

    // MARK: The top row

    private func header(_ i: Int, _ row: InputFxRow) -> some View {
        let title = InputFxModel.title(row.kind)
        return HStack(spacing: theme.spacing.s) {
            Text(title)
                .font(theme.type.caption.font.weight(.semibold))
                .foregroundStyle(theme.text.primary.color)
                .lineLimit(1)
            Spacer(minLength: 0)
            Toggle("Bypass", isOn: Binding(get: { row.bypass }, set: { model.setBypass(i, $0) }))
                .toggleStyle(.checkbox)
            Button {
                model.remove(at: i)
                model.closeSettings()
            } label: {
                AppIcon.remove.image
                    .font(theme.type.caption.font)
                    .foregroundStyle(theme.text.secondary.color)
                    .frame(
                        width: ControlMetrics.hitSize(.toggle).width,
                        height: ControlMetrics.hitSize(.toggle).height)
                    .contentShape(Rectangle())
            }
            .buttonStyle(.plain)
            .help("Remove \(title)")
            .accessibilityLabel("Remove \(title)")
        }
    }

    // MARK: The effect's controls

    @ViewBuilder private func controls(_ i: Int, _ kind: InputFxKindRow) -> some View {
        switch kind {
        case let .arpeggiator(mode, division, feel, octaves, gate, velocity, latch):
            arpeggiator(i, mode, division, feel, octaves, gate, velocity, latch)
        case let .transpose(semitones):
            Stepper(
                "Semitones \(InputFxModel.summary(kind))",
                value: Binding(
                    get: { Int(semitones) },
                    set: { model.update(i, .transpose(semitones: Int8(clamping: $0))) }),
                in: -48...48)
        case let .velocity(scale, offset, min, max, fixed):
            velocity(i, scale, offset, min, max, fixed)
        case let .chord(intervals):
            chord(i, intervals)
        case let .inputQuantize(grid, strength):
            inputQuantize(i, grid, strength)
        }
    }

    private static let modes: [ArpModeRow] = [.up, .down, .upDown, .downUp, .asPlayed, .random, .chord]
    private static let divisions: [UInt8] = [1, 2, 4, 8, 16, 32, 64]
    private static let feels: [(FeelRow, String)] = [(.straight, "Straight"), (.dotted, "Dotted"), (.triplet, "Triplet")]

    @ViewBuilder private func arpeggiator(
        _ i: Int, _ mode: ArpModeRow, _ division: UInt8, _ feel: FeelRow, _ octaves: UInt8, _ gate: UInt16,
        _ velocity: UInt8?, _ latch: Bool
    ) -> some View {
        let make = { (m: ArpModeRow, d: UInt8, f: FeelRow, o: UInt8, g: UInt16, v: UInt8?, l: Bool) in
            InputFxKindRow.arpeggiator(mode: m, division: d, feel: f, octaves: o, gate: g, velocity: v, latch: l)
        }
        Picker(
            "Mode",
            selection: Binding(
                get: { mode },
                set: { model.update(i, make($0, division, feel, octaves, gate, velocity, latch)) })
        ) {
            ForEach(Self.modes, id: \.self) { m in Text(InputFxModel.modeTitle(m)).tag(m) }
        }
        HStack(spacing: theme.spacing.s) {
            Picker(
                "Rate",
                selection: Binding(
                    get: { division },
                    set: { model.update(i, make(mode, $0, feel, octaves, gate, velocity, latch)) })
            ) {
                ForEach(Self.divisions, id: \.self) { d in Text("1/\(d)").tag(d) }
            }
            Picker(
                "Feel",
                selection: Binding(
                    get: { feel },
                    set: { model.update(i, make(mode, division, $0, octaves, gate, velocity, latch)) })
            ) {
                ForEach(Self.feels, id: \.0) { f in Text(f.1).tag(f.0) }
            }
            .labelsHidden()
        }
        Stepper(
            "Octaves \(octaves)",
            value: Binding(
                get: { Int(octaves) },
                set: { model.update(i, make(mode, division, feel, UInt8(clamping: $0), gate, velocity, latch)) }),
            in: 1...4)
        VStack(alignment: .leading, spacing: theme.spacing.xxs) {
            Text("Gate \(gate)%").foregroundStyle(theme.text.secondary.color)
            Slider(
                value: Binding(
                    get: { Double(gate) },
                    set: {
                        model.setDraft(
                            make(mode, division, feel, octaves, UInt16(clamping: Int($0.rounded())), velocity, latch))
                    }),
                in: 5...200, step: 1,
                onEditingChanged: edits)
            .accessibilityLabel("Gate")
        }
        HStack(spacing: theme.spacing.s) {
            Picker(
                "Velocity",
                selection: Binding(
                    get: { velocity != nil },
                    set: { model.update(i, make(mode, division, feel, octaves, gate, $0 ? 100 : nil, latch)) })
            ) {
                Text("As Played").tag(false)
                Text("Fixed").tag(true)
            }
            if let v = velocity {
                Stepper(
                    "\(v)",
                    value: Binding(
                        get: { Int(v) },
                        set: { model.update(i, make(mode, division, feel, octaves, gate, UInt8(clamping: $0), latch)) }),
                    in: 1...127)
                .accessibilityLabel("Fixed velocity")
            }
        }
        Toggle(
            "Latch",
            isOn: Binding(
                get: { latch },
                set: { model.update(i, make(mode, division, feel, octaves, gate, velocity, $0)) }))
        .toggleStyle(.checkbox)
    }

    @ViewBuilder private func velocity(
        _ i: Int, _ scale: UInt16, _ offset: Int16, _ min: UInt8, _ max: UInt8, _ fixed: UInt8?
    ) -> some View {
        let make = { (s: UInt16, o: Int16, lo: UInt8, hi: UInt8, f: UInt8?) in
            InputFxKindRow.velocity(scale: s, offset: o, min: lo, max: hi, fixed: f)
        }
        VStack(alignment: .leading, spacing: theme.spacing.xxs) {
            Text("Scale \(scale)%").foregroundStyle(theme.text.secondary.color)
            Slider(
                value: Binding(
                    get: { Double(scale) },
                    set: { model.setDraft(make(UInt16(clamping: Int($0.rounded())), offset, min, max, fixed)) }),
                in: 0...400, step: 1,
                onEditingChanged: edits)
            .accessibilityLabel("Scale")
        }
        VStack(alignment: .leading, spacing: theme.spacing.xxs) {
            Text("Offset \(offset < 0 ? "\u{2212}\(-Int(offset))" : "+\(offset)")")
                .foregroundStyle(theme.text.secondary.color)
            Slider(
                value: Binding(
                    get: { Double(offset) },
                    set: { model.setDraft(make(scale, Int16(clamping: Int($0.rounded())), min, max, fixed)) }),
                in: -127...127, step: 1,
                onEditingChanged: edits)
            .accessibilityLabel("Offset")
        }
        Stepper(
            "Min \(min)",
            value: Binding(
                get: { Int(min) },
                set: { model.update(i, make(scale, offset, UInt8(clamping: $0), max, fixed)) }),
            in: 1...Int(max))
        Stepper(
            "Max \(max)",
            value: Binding(
                get: { Int(max) },
                set: { model.update(i, make(scale, offset, min, UInt8(clamping: $0), fixed)) }),
            in: Int(min)...127)
        HStack(spacing: theme.spacing.s) {
            Toggle(
                "Fixed",
                isOn: Binding(
                    get: { fixed != nil },
                    set: { model.update(i, make(scale, offset, min, max, $0 ? 100 : nil)) }))
            .toggleStyle(.checkbox)
            if let f = fixed {
                Stepper(
                    "\(f)",
                    value: Binding(
                        get: { Int(f) },
                        set: { model.update(i, make(scale, offset, min, max, UInt8(clamping: $0))) }),
                    in: 1...127)
                .accessibilityLabel("Fixed velocity")
            }
        }
    }

    /// The next interval from `value` in direction `step` (±1) that is
    /// within ±48, not 0 and not already in `others`; nil when none is.
    private static func nextInterval(_ value: Int, _ step: Int, others: [Int8]) -> Int8? {
        var v = value + step
        while (-48...48).contains(v) {
            if v != 0, !others.contains(Int8(v)) { return Int8(v) }
            v += step
        }
        return nil
    }

    @ViewBuilder private func chord(_ i: Int, _ intervals: [Int8]) -> some View {
        ForEach(Array(intervals.enumerated()), id: \.offset) { n, interval in
            let others = intervals.enumerated().filter { $0.offset != n }.map(\.element)
            let set = { (v: Int8?) in
                guard let v else { return }
                var next = intervals
                next[n] = v
                model.update(i, .chord(intervals: next))
            }
            HStack(spacing: theme.spacing.s) {
                Stepper(
                    "Interval \(n + 1): \(interval < 0 ? "\u{2212}\(-Int(interval))" : "+\(interval)")",
                    onIncrement: { set(Self.nextInterval(Int(interval), 1, others: others)) },
                    onDecrement: { set(Self.nextInterval(Int(interval), -1, others: others)) })
                Spacer(minLength: 0)
                Button {
                    var next = intervals
                    next.remove(at: n)
                    model.update(i, .chord(intervals: next))
                } label: {
                    AppIcon.remove.image
                        .font(theme.type.caption.font)
                        .frame(
                            width: ControlMetrics.hitSize(.toggle).width,
                            height: ControlMetrics.hitSize(.toggle).height)
                        .contentShape(Rectangle())
                }
                .buttonStyle(.plain)
                .disabled(intervals.count <= 1)
                .help("Remove this interval")
                .accessibilityLabel("Remove interval \(n + 1)")
            }
        }
        Button {
            if let v = Self.nextInterval(Int(intervals.max() ?? 0), 1, others: intervals)
                ?? Self.nextInterval(0, -1, others: intervals)
            {
                model.update(i, .chord(intervals: intervals + [v]))
            }
        } label: {
            AppIcon.add.image
                .font(theme.type.caption.font)
                .frame(
                    width: ControlMetrics.hitSize(.toggle).width,
                    height: ControlMetrics.hitSize(.toggle).height)
                .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .disabled(intervals.count >= 6)
        .help("Add an interval")
        .accessibilityLabel("Add an interval")
    }

    @ViewBuilder private func inputQuantize(_ i: Int, _ grid: Int64, _ strength: UInt8) -> some View {
        Picker(
            "Grid",
            selection: Binding(
                get: { grid },
                set: { model.update(i, .inputQuantize(grid: $0, strength: strength)) })
        ) {
            ForEach(InputFxModel.quantizeGrids, id: \.ticks) { g in Text(g.title).tag(g.ticks) }
        }
        VStack(alignment: .leading, spacing: theme.spacing.xxs) {
            Text("Strength \(strength)%").foregroundStyle(theme.text.secondary.color)
            Slider(
                value: Binding(
                    get: { Double(strength) },
                    set: { model.setDraft(.inputQuantize(grid: grid, strength: UInt8(clamping: Int($0.rounded())))) }),
                in: 0...100, step: 1,
                onEditingChanged: edits)
            .accessibilityLabel("Strength")
        }
    }

    /// A slider grabbed starts the draft; released, it edits once.
    private func edits(_ editing: Bool) {
        if editing { model.beginDraft() } else { model.commitDraft() }
    }
}
