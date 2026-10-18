import SwiftUI
import VizDAWAppModel
import VizDAWCore
import VizDAWEditors

/// The Strip Chart (spec §4d) with its bar.
struct StripChartEditor: View {
    let model: StripChartModel

    var body: some View {
        VStack(spacing: 0) {
            StripChartBar(model: model, editor: model.editor)
            Divider()
            StripChartHost(model: model)
        }
    }
}

struct StripChartHost: NSViewRepresentable {
    let model: StripChartModel

    func makeNSView(context: Context) -> StripChartView { StripChartView(model: model) }

    func updateNSView(_ view: StripChartView, context: Context) {}
}

/// Mode, operation, shape and its parameters, step, the lane shown and the
/// number of rows (Plan 4d Rulings 17, 21, 22).
struct StripChartBar: View {
    let model: StripChartModel
    @Bindable var editor: EditorModel

    private func binding<T>(_ key: WritableKeyPath<StripChartSettings, T>) -> Binding<T> {
        Binding(
            get: { editor.layout.stripChart[keyPath: key] },
            set: { v in editor.updateLayout { $0.stripChart[keyPath: key] = v } })
    }

    private var laneName: String {
        guard let l = model.lane else { return "—" }
        return editor.laneRows(l.seq).first { $0.id == l.lane }?.name ?? "—"
    }

    /// Operations that draw no line shape.
    private var shapeless: Bool {
        let s = editor.layout.stripChart
        return s.mode == .erase || (s.mode == .modify && [.compress, .thin, .thicken, .legato].contains(s.op))
    }

    var body: some View {
        let s = editor.layout.stripChart
        HStack(spacing: 10) {
            Picker("Mode", selection: binding(\.mode)) {
                ForEach(StripMode.allCases, id: \.self) { Text($0.title).tag($0) }
            }
            .pickerStyle(.segmented)
            .labelsHidden()
            .frame(width: 190)
            .accessibilityLabel("Mode")
            if s.mode == .modify {
                Picker("Operation", selection: binding(\.op)) {
                    ForEach(StripOp.allCases, id: \.self) { Text($0.title).tag($0) }
                }
                .labelsHidden()
                .frame(width: 120)
                .accessibilityLabel("Operation")
            }
            if !shapeless {
                Picker("Shape", selection: binding(\.shape)) {
                    ForEach(ShapeKind.allCases, id: \.self) { Text($0.title).tag($0) }
                }
                .labelsHidden()
                .frame(width: 110)
                .accessibilityLabel("Shape")
                if s.shape == .curved {
                    Picker("Curve", selection: binding(\.curve)) {
                        ForEach(CurveKind.allCases, id: \.self) { Text($0.title).tag($0) }
                    }
                    .labelsHidden()
                    .frame(width: 100)
                    .accessibilityLabel("Curve")
                }
                if s.shape == .oscillator {
                    oscillator(s)
                }
            }
            Picker("Step", selection: binding(\.step)) {
                ForEach(StripBarChoices.steps(s.step), id: \.1) { Text($0.0).tag($0.1) }
            }
            .frame(width: 110)
            .help("Distance between drawn points; the spacing of Thin and Thicken")
            .accessibilityLabel("Step")
            if model.target?.lane == nil, let seq = model.seq {
                Menu("Lane: \(laneName)") {
                    ForEach(editor.laneRows(seq), id: \.id) { l in
                        Button(l.name) { model.chosenLane = LaneKey(seq, l.id) }
                    }
                }
                .frame(width: 150)
            }
            Spacer()
            Stepper(
                "Rows: \(model.rows.count)", onIncrement: { model.addRow() },
                onDecrement: { model.removeRow(model.rows.count - 1) }
            )
            .accessibilityLabel("Rows")
        }
        .padding(.horizontal, 8)
        .padding(.vertical, 4)
    }

    @ViewBuilder
    private func oscillator(_ s: StripChartSettings) -> some View {
        Picker("Wave", selection: binding(\.wave)) {
            ForEach(WaveKind.allCases, id: \.self) { Text($0.title).tag($0) }
        }
        .labelsHidden()
        .frame(width: 90)
        .accessibilityLabel("Wave")
        Picker("Period", selection: binding(\.period)) {
            ForEach(PeriodKind.allCases, id: \.self) { Text($0.title).tag($0) }
        }
        .labelsHidden()
        .frame(width: 150)
        .accessibilityLabel("Period Kind")
        switch s.period {
        case .musical:
            Picker("Every", selection: binding(\.periodTicks)) {
                ForEach(StripBarChoices.musicalPeriods(s.periodTicks), id: \.1) { Text($0.0).tag($0.1) }
            }
            .frame(width: 140)
            .accessibilityLabel("Period")
        case .clock:
            Picker("Every", selection: binding(\.periodMs)) {
                ForEach(StripBarChoices.clockPeriods(s.periodMs), id: \.1) { Text($0.0).tag($0.1) }
            }
            .frame(width: 120)
            .accessibilityLabel("Period")
        case .bars:
            Picker("Every", selection: binding(\.periodBars)) {
                ForEach(StripBarChoices.barPeriods(s.periodBars), id: \.1) { Text($0.0).tag($0.1) }
            }
            .frame(width: 120)
            .accessibilityLabel("Period")
        }
        Picker("Phase", selection: binding(\.phase)) {
            ForEach(StripBarChoices.phases(s.phase), id: \.1) { Text($0.0).tag($0.1) }
        }
        .frame(width: 100)
        .accessibilityLabel("Phase")
    }
}
