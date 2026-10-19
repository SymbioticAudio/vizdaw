import SwiftUI
import VizDAWAppModel
import VizDAWCore
import VizDAWEditors

/// The drum grid (spec §4e) with its bar; a sequence without drum lanes
/// says how to make one.
struct DrumGridEditor: View {
    let model: DrumGridModel

    var body: some View {
        // Observed, so the empty state follows edits and undo.
        let _ = model.editor.drawVersion
        VStack(spacing: 0) {
            DrumGridBar(model: model, editor: model.editor)
            Divider()
            if model.lanes.isEmpty {
                ContentUnavailableView(
                    "No Drum Lanes", systemImage: "square.grid.3x3",
                    description: Text(
                        "Make a lane a drum lane with Drum > Drum Lane or its header's menu, or add a Drum Kit sequence."))
            } else {
                DrumGridHost(model: model)
            }
        }
    }
}

struct DrumGridHost: NSViewRepresentable {
    let model: DrumGridModel

    func makeNSView(context: Context) -> DrumGridContainer { DrumGridContainer(model: model) }

    func updateNSView(_ view: DrumGridContainer, context: Context) {}
}

/// Tool, view, step, spacing, length, velocity, Add Row, and a menu with
/// the current lane's play quantize and nudge (Plan 4e Rulings 10, 11):
/// one line that fits the detail pane of a default-sized window.
struct DrumGridBar: View {
    let model: DrumGridModel
    @Bindable var editor: EditorModel

    private func binding<T>(_ key: WritableKeyPath<DrumGridSettings, T>) -> Binding<T> {
        Binding(
            get: { editor.layout.drumGrid[keyPath: key] },
            set: { v in editor.updateLayout { $0.drumGrid[keyPath: key] = v } })
    }

    private func lengths(_ title: String, _ key: WritableKeyPath<DrumGridSettings, Int64>, help: String) -> some View {
        Picker(title, selection: binding(key)) {
            ForEach(DrumGridSettings.lengths(including: editor.layout.drumGrid[keyPath: key]), id: \.1) {
                Text($0.0).tag($0.1)
            }
        }
        .labelsHidden()
        .frame(width: 70)
        .help(help)
        .accessibilityLabel(title)
    }

    private var erasing: Binding<Bool> {
        Binding(get: { editor.tool == .eraser }, set: { editor.tool = $0 ? .eraser : .pencil })
    }

    var body: some View {
        let s = editor.layout.drumGrid
        HStack(spacing: 6) {
            Picker("Tool", selection: erasing) {
                Text("Paint").tag(false)
                Text("Erase").tag(true)
            }
            .pickerStyle(.segmented)
            .labelsHidden()
            .frame(width: 100)
            .accessibilityLabel("Tool")
            Picker("View", selection: binding(\.view)) {
                ForEach(DrumView.allCases, id: \.self) { Text($0.title).tag($0) }
            }
            .labelsHidden()
            .frame(width: 100)
            .accessibilityLabel("View")
            lengths("Step", \.step, help: "Step: the width of a step matrix cell")
            lengths("Spacing", \.spacing, help: "Spacing: the distance between the hits a drag paints")
            lengths("Length", \.duration, help: "Length: the length of a painted hit")
            Stepper(
                "Vel \(s.velocity)",
                onIncrement: { editor.updateLayout { $0.drumGrid.velocity = min($0.drumGrid.velocity + 4, 127) } },
                onDecrement: { editor.updateLayout { $0.drumGrid.velocity = max($0.drumGrid.velocity, 5) - 4 } }
            )
            .help("The velocity of painted hits in rows whose velocity lane is hidden")
            .accessibilityLabel("Velocity")
            Button {
                model.addRow()
            } label: {
                Image(systemName: "plus")
            }
            .help("Add Drum Row")
            .accessibilityLabel("Add Row")
            Spacer(minLength: 0)
            if let lane = model.currentLane {
                laneMenu(lane)
            }
        }
        .padding(.horizontal, 8)
        .padding(.vertical, 4)
    }

    /// Play quantize choices (the current one checked) and nudge.
    private func laneMenu(_ lane: DrumLaneRow) -> some View {
        let title = model.quantizeTitle(lane)
        let shift = lane.playShift
        return Menu("\(lane.name): \(title)") {
            Section("Play Quantize") {
                ForEach(DrumGridModel.quantizeChoices, id: \.title) { c in
                    Toggle(
                        c.title,
                        isOn: Binding(
                            get: { c.title == title }, set: { _ in model.setQuantize(lane: lane.lane, c.quantize) }))
                }
            }
            Section("Nudge: \(shift > 0 ? "+" : "")\(shift) ticks") {
                Button("Nudge Earlier") { model.nudge(lane: lane.lane, steps: -1) }
                Button("Nudge Later") { model.nudge(lane: lane.lane, steps: 1) }
            }
        }
        .frame(width: 160)
        .help("The lane's play quantize (grid or swing) and nudge, applied as it plays")
        .accessibilityLabel("Lane")
    }
}
