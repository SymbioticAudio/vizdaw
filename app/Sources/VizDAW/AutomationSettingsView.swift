import SwiftUI
import VizDAWAppModel
import VizDAWCore

/// Settings › Automation (spec §7.5; plan 5e-2 Task 1): the mode a Write
/// strip takes after its pass, the return time, and the thinning.
struct AutomationSettingsView: View {
    @Bindable var prefs: AutomationPreferences

    private static let modeTitles = [
        ("off", "Off"), ("read", "Read"), ("touch", "Touch"), ("latch", "Latch"), ("trim", "Trim"),
    ]

    var body: some View {
        Form {
            Picker(
                "After Write",
                selection: Binding(get: { prefs.settings.afterWrite }, set: { prefs.setAfterWrite($0) })
            ) {
                ForEach(Self.modeTitles, id: \.0) { Text($0.1).tag($0.0) }
            }
            Stepper(
                "Return time: \(prefs.settings.returnMs) ms",
                value: Binding(get: { Int(prefs.settings.returnMs) }, set: { prefs.setReturnMs(UInt32(clamping: $0)) }),
                in: 0...Int(AutomationSettings.maxReturnMs), step: 50)
            Picker(
                "Thinning",
                selection: Binding(get: { prefs.settings.thinning }, set: { prefs.setThinning($0) })
            ) {
                Text("Low").tag("low")
                Text("Medium").tag("medium")
                Text("High").tag("high")
            }
            Text("How closely written automation follows the move: 0.1 dB on faders and sends, 0.25 % elsewhere at Medium.")
                .font(.caption)
                .foregroundStyle(.secondary)
            Toggle("Write plugin parameters only when touched", isOn: .constant(true))
                .disabled(true)
            Text("Always on: writing every parameter of every plugin would bury the lanes.")
                .font(.caption)
                .foregroundStyle(.secondary)
            if let problem = prefs.problem {
                Text(problem).foregroundStyle(.red)
            }
        }
        .formStyle(.grouped)
        .frame(width: 500)
    }
}
