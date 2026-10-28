import SwiftUI
import VizDAWAppModel
import VizDAWCore

/// An AudioPath strip's input picker and monitor button, above its inserts
/// (spec §6.9; plan 5d-2 Task 6): the same controls as the lane header,
/// for the lane that plays through the strip (Ruling 4).
struct StripInputView: View {
    let inputs: LaneInputs
    let lane: LaneRow

    var body: some View {
        HStack(spacing: 4) {
            Menu(LaneInputs.title(lane.input)) {
                ForEach(Array(inputs.groups(for: lane).enumerated()), id: \.offset) { _, group in
                    Section(group.title) {
                        ForEach(Array(group.choices.enumerated()), id: \.offset) { _, choice in
                            Button {
                                inputs.choose(choice, lane: lane)
                            } label: {
                                if choice.input == lane.input {
                                    Label(choice.title, systemImage: "checkmark")
                                } else {
                                    Text(choice.title)
                                }
                            }
                        }
                    }
                }
            }
            .controlSize(.small)
            .help(inputs.badge(lane).map { "Input of \(lane.name), recording \($0)" } ?? "Input of \(lane.name)")
            Button(LaneInputs.label(lane.monitor)) { inputs.cycleMonitor(lane) }
                .controlSize(.small)
                .frame(width: 26)
                .help(LaneInputs.help(lane.monitor))
                .accessibilityLabel(LaneInputs.help(lane.monitor))
        }
        .onAppear { inputs.refreshInfo() }
    }
}
