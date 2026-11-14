import SwiftUI
import SymbioticTokens
import VizDAWAppModel
import VizDAWCore

/// Settings › Plug-ins (plan 6g Task 4, Rulings 8–10): "Put an EQ on new
/// channels" and "Install Multi-Q for other apps". All state lives in
/// `PluginSettings`; the view only reads it.
struct PluginSettingsView: View {
    @Environment(\.symbioticTheme) private var theme
    @Bindable var settings: PluginSettings

    private var eqName: String { settings.bundledEqName ?? "the bundled EQ" }

    var body: some View {
        Form {
            Toggle(
                "Put an EQ on new channels",
                isOn: Binding(get: { settings.newChannelEq }, set: { settings.setNewChannelEq($0) })
            )
            .disabled(settings.bundledEqName == nil)
            .accessibilityLabel("Put an EQ on new channels")
            Text(
                "New audio, instrument and synth channels start with \(eqName) in their EQ slot. Buses and master do not."
            )
            .font(theme.type.caption.font)
            .foregroundStyle(theme.text.secondary.color)
            Button("Install \(eqName) for other apps") { settings.install(replace: false) }
                .disabled(settings.bundledEqName == nil || !settings.bundledAuPresent)
                .accessibilityLabel("Install \(eqName) for other apps")
            Text(
                "Copies the Audio Unit to your Components folder, so projects you take to other apps keep their EQ settings."
            )
            .font(theme.type.caption.font)
            .foregroundStyle(theme.text.secondary.color)
            if settings.bundledEqName == nil {
                Text(PluginSettings.noBundledEq)
                    .foregroundStyle(theme.state.warning.color)
            }
            if let status = settings.status {
                Text(status)
                    .foregroundStyle(theme.text.primary.color)
                    .accessibilityLabel(status)
            }
        }
        .formStyle(.grouped)
        .frame(width: 500)
        .confirmationDialog(
            "Replace the \(eqName) in your Components folder?", isPresented: $settings.confirmReplace
        ) {
            Button("Replace", role: .destructive) { settings.install(replace: true) }
                .accessibilityLabel("Replace")
            Button("Cancel", role: .cancel) {}
                .accessibilityLabel("Cancel")
        } message: {
            Text("Something is already installed under that name.")
        }
    }
}
