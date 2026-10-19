import SwiftUI
import VizDAWAppModel
import VizDAWCore

/// The panel's Strip Chart Line (Plan 4d Ruling 24): target, operation and
/// shape pickers; the numbers are the panel's fields.
struct LineModifyEditor: View {
    let model: SelectModifyModel
    let target: LineTargetRow
    let op: LineOpRow
    let line: LineRow

    static let ops: [(String, LineOpRow)] = [
        ("Draw", .draw), ("Set", .set), ("Scale", .scale), ("Add", .add), ("Max Limit", .limitMax),
        ("Min Limit", .limitMin), ("Compress", .compress), ("Thin", .thin), ("Thicken", .thicken), ("Legato", .legato),
    ]

    private var targetTitle: String {
        StripLane(target).map { $0.title(model.automationNames) } ?? "Any Value"
    }

    var body: some View {
        Menu(targetTitle) {
            ForEach(LineTargetChoices.basics, id: \.self) { lane in
                Button(lane.title(model.automationNames)) { model.setLineTarget(lane) }
            }
            Menu("Controller") {
                ForEach(LineTargetChoices.controllers, id: \.self) { lane in
                    Button(lane.title(model.automationNames)) { model.setLineTarget(lane) }
                }
            }
            Menu("Poly Pressure") {
                ForEach(LineTargetChoices.polyPressure, id: \.title) { octave in
                    Menu(octave.title) {
                        ForEach(octave.lanes, id: \.self) { lane in
                            Button(lane.title(model.automationNames)) { model.setLineTarget(lane) }
                        }
                    }
                }
            }
            Menu("Automation") {
                ForEach(model.automationMenus, id: \.title) { g in
                    Menu(g.title) {
                        ForEach(g.lanes, id: \.self) { lane in
                            Button(lane.title(model.automationNames)) { model.setLineTarget(lane) }
                        }
                    }
                }
            }
        }
        .frame(width: 210)
        .accessibilityLabel("Line Target")
        Picker("", selection: Binding(get: { op }, set: { model.setLineOp($0) })) {
            ForEach(Self.ops, id: \.0) { Text($0.0).tag($0.1) }
        }
        .labelsHidden()
        .frame(width: 110)
        .accessibilityLabel("Operation")
        Picker("", selection: Binding(get: { ShapeKind(line.shape) }, set: { model.setLineShape($0) })) {
            ForEach(ShapeKind.allCases, id: \.self) { Text($0.title).tag($0) }
        }
        .labelsHidden()
        .frame(width: 110)
        .accessibilityLabel("Shape")
        switch line.shape {
        case .curved(let curve):
            Picker("", selection: Binding(get: { curve }, set: { model.setLineCurve($0) })) {
                ForEach(CurveKind.allCases, id: \.self) { Text($0.title).tag($0.row) }
            }
            .labelsHidden()
            .frame(width: 100)
            .accessibilityLabel("Curve")
        case let .oscillator(wave, period, _):
            Picker("", selection: Binding(get: { wave }, set: { model.setLineWave($0) })) {
                ForEach(WaveKind.allCases, id: \.self) { Text($0.title).tag($0.row) }
            }
            .labelsHidden()
            .frame(width: 90)
            .accessibilityLabel("Wave")
            Picker("", selection: Binding(get: { Self.kind(period) }, set: { model.setLinePeriodKind($0) })) {
                ForEach(PeriodKind.allCases, id: \.self) { Text($0.title).tag($0) }
            }
            .labelsHidden()
            .frame(width: 150)
            .accessibilityLabel("Period Kind")
        case .free(let points):
            Text("Drawn line (\(points.count) points)")
                .foregroundStyle(.secondary)
        default:
            EmptyView()
        }
    }

    static func kind(_ p: PeriodRow) -> PeriodKind {
        switch p {
        case .ticks: .musical
        case .millis: .clock
        case .bars: .bars
        }
    }
}

/// An Automation Target condition's target.
struct AutomationTargetPicker: View {
    let model: SelectModifyModel
    let selection: AutoTarget
    let choose: (AutoTarget) -> Void

    var body: some View {
        Menu(model.automationNames.title(selection)) {
            ForEach(model.automationMenus, id: \.title) { g in
                Menu(g.title) {
                    ForEach(g.lanes, id: \.self) { lane in
                        if case .automation(let t) = lane {
                            Button(model.automationNames.title(t)) { choose(t) }
                        }
                    }
                }
            }
        }
        .frame(width: 220)
        .accessibilityLabel("Automation Target")
    }
}
