import SwiftUI
import SymbioticTokens
import VizDAWAppModel
import VizDAWCore

/// Settings › Plug-ins (plan 6g Task 4, Rulings 8–10): "Put an EQ on new
/// channels", the picker of which EQ that is (plan 6g-2 Rulings 5–6) and
/// "Install Multi-Q for other apps". All state lives in `PluginSettings`;
/// the view only reads it.
struct PluginSettingsView: View {
    @Environment(\.symbioticTheme) private var theme
    @Bindable var settings: PluginSettings

    private var eqName: String { settings.bundledEqName ?? "the bundled EQ" }

    /// The picker's selection: nil is Multi-Q; a stored choice still
    /// installed is shown as its item (matched by format and ID).
    private var eqSelection: Binding<DefaultEqRow?> {
        Binding(
            get: {
                guard let c = settings.defaultEq else { return nil }
                return settings.eqChoices.first { $0.format == c.format && $0.id == c.id } ?? c
            },
            set: { settings.setDefaultEq($0) })
    }

    private static func formatLabel(_ f: PluginFormatTag) -> String {
        switch f {
        case .au: "AU"
        case .clap: "CLAP"
        case .vst3: "VST3"
        }
    }

    var body: some View {
        Form {
            Toggle(
                "Put an EQ on new channels",
                isOn: Binding(get: { settings.newChannelEq }, set: { settings.setNewChannelEq($0) })
            )
            .disabled(settings.bundledEqName == nil && settings.eqChoices.isEmpty)
            .accessibilityLabel("Put an EQ on new channels")
            Picker("EQ for new channels", selection: eqSelection) {
                if let name = settings.bundledEqName {
                    Text("\(name) (built in)").tag(DefaultEqRow?.none)
                }
                ForEach(settings.eqChoices, id: \.self) { c in
                    Text("\(c.name) (\(Self.formatLabel(c.format)))").tag(DefaultEqRow?.some(c))
                }
                if let c = settings.defaultEq, !settings.defaultEqAvailable {
                    Text("\(c.name) (not available)").tag(DefaultEqRow?.some(c))
                }
            }
            .disabled(!settings.newChannelEq)
            .accessibilityLabel("EQ for new channels")
            if let note = settings.eqNote {
                Text(note)
                    .foregroundStyle(theme.state.warning.color)
                    .accessibilityLabel(note)
            }
            Text(
                "New channels, new buses and the master of new projects start with \(settings.defaultEqName) in their EQ slot. Projects you already have are not changed."
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
        .onAppear { settings.refreshEqChoices() }
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
