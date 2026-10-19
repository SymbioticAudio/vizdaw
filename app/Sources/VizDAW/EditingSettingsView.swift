import SwiftUI
import VizDAWAppModel

/// Settings > Editing: both nudge steps on one page (parent spec §9, Plan
/// 4b Ruling 16).
struct EditingSettingsView: View {
    @Bindable var prefs: EditingPreferences

    private enum TimeChoice: Hashable { case grid, division, ticks }

    private var timeChoice: Binding<TimeChoice> {
        Binding(
            get: {
                switch prefs.nudge.time {
                case .grid: .grid
                case .division: .division
                case .ticks: .ticks
                }
            },
            set: { choice in
                switch choice {
                case .grid: prefs.nudge.time = .grid
                case .division: prefs.nudge.time = .division(.sixteenth, triplet: false)
                case .ticks: prefs.nudge.time = .ticks(240)
                }
            })
    }

    private var scaleSteps: Binding<Bool> {
        Binding(
            get: { if case .scaleSteps = prefs.nudge.pitch { true } else { false } },
            set: { on in prefs.nudge.pitch = on ? .scaleSteps(amount) : .semitones(amount) })
    }

    private var amount: Int {
        switch prefs.nudge.pitch {
        case .semitones(let n), .scaleSteps(let n): n
        }
    }

    private var amountBinding: Binding<Int> {
        Binding(
            get: { amount },
            set: { n in
                if case .scaleSteps = prefs.nudge.pitch {
                    prefs.nudge.pitch = .scaleSteps(n)
                } else {
                    prefs.nudge.pitch = .semitones(n)
                }
            })
    }

    var body: some View {
        Form {
            Section("Nudge in Time") {
                Picker("Step", selection: timeChoice) {
                    Text("Edit Grid").tag(TimeChoice.grid)
                    Text("Note Value").tag(TimeChoice.division)
                    Text("Ticks").tag(TimeChoice.ticks)
                }
                if case let .division(d, triplet) = prefs.nudge.time {
                    Picker(
                        "Note Value",
                        selection: Binding(get: { d }, set: { prefs.nudge.time = .division($0, triplet: triplet) })
                    ) {
                        ForEach(Snap.Division.allCases, id: \.self) { Text($0.title).tag($0) }
                    }
                    Toggle(
                        "Triplet", isOn: Binding(get: { triplet }, set: { prefs.nudge.time = .division(d, triplet: $0) }))
                }
                if case let .ticks(n) = prefs.nudge.time {
                    Stepper(
                        "\(n) ticks (\(MeterMath.ppq) per quarter note)",
                        value: Binding(get: { Int(n) }, set: { prefs.nudge.time = .ticks(Int64(max($0, 1))) }),
                        in: 1...61_440, step: 60)
                }
            }
            Section("Nudge in Pitch") {
                Picker("Step", selection: scaleSteps) {
                    Text("Semitones").tag(false)
                    Text("Scale Steps").tag(true)
                }
                Stepper("Amount: \(amount)", value: amountBinding, in: 1...12)
                Toggle("Follow the sequence's key signature", isOn: $prefs.nudge.followKeySignature)
                    .disabled(!scaleSteps.wrappedValue)
                Picker("Root", selection: $prefs.nudge.root) {
                    ForEach(0..<12, id: \.self) { Text(ScaleMath.rootNames[$0]).tag(UInt8($0)) }
                }
                .disabled(!scaleSteps.wrappedValue || prefs.nudge.followKeySignature)
                Picker("Scale", selection: $prefs.nudge.scale) {
                    ForEach(Scale.allCases, id: \.self) { Text($0.title).tag($0) }
                }
                .disabled(!scaleSteps.wrappedValue || prefs.nudge.followKeySignature)
            }
        }
        .formStyle(.grouped)
        .frame(width: 460)
        .padding()
    }
}
