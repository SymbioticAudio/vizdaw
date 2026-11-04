import AppKit
import SwiftUI
import SymbioticTokens
import VizDAWAppModel

/// Settings › Appearance (plan 7b Ruling 12; user themes by controller
/// ruling R-UT): the theme, the channel colour bars, and the user themes'
/// folder with any problems its files have.
struct AppearanceSettingsView: View {
    @Bindable var prefs: AppearancePreferences
    @Environment(\.symbioticTheme) private var current

    var body: some View {
        Form {
            Picker("Theme", selection: Binding(get: { prefs.theme.id }, set: { _ = prefs.choose($0) })) {
                ForEach(prefs.themes) { t in
                    HStack(spacing: current.spacing.s) {
                        Text(t.name)
                        Spacer(minLength: 12)
                        ThemeSwatches(theme: t)
                    }
                    .tag(t.id)
                }
            }
            .pickerStyle(.radioGroup)
            Toggle("Show channel colour bars", isOn: Binding(get: { prefs.colourBars }, set: { prefs.setColourBars($0) }))
            Text("The theme applies to every window. It is a preference of this Mac, not saved in projects.")
                .font(.caption)
                .foregroundStyle(.secondary)
            if let lib = prefs.userThemes {
                Section("Your Themes") {
                    Text(
                        "A theme file is JSON in the themes folder: a \"name\", a \"base\" (one of "
                            + Theme.all.map(\.id).joined(separator: ", ")
                            + ") and any tokens to change. Files are read again when they change."
                    )
                    .font(.caption)
                    .foregroundStyle(.secondary)
                    HStack {
                        Button("Duplicate as File…") { duplicate(lib) }
                        Button("Show Themes Folder") {
                            try? FileManager.default.createDirectory(at: lib.folder, withIntermediateDirectories: true)
                            NSWorkspace.shared.open(lib.folder)
                        }
                    }
                    if let said = lib.lastDuplicate {
                        Text(said).font(.caption).foregroundStyle(.secondary)
                    }
                    ForEach(lib.problems) { p in
                        Text(p.file.isEmpty ? "\(p.message)." : "\(p.file) \(p.message).")
                            .font(.caption)
                            .foregroundStyle(current.state.warning.color)
                    }
                }
            }
        }
        .formStyle(.grouped)
        .frame(width: 500)
    }

    /// Writes the current theme as a new file and shows it in the Finder.
    /// Only from the button: the channel never opens anything.
    private func duplicate(_ lib: UserThemes) {
        do {
            let url = try prefs.duplicateCurrent()
            NSWorkspace.shared.activateFileViewerSelecting([url])
        } catch {
            lib.lastDuplicate = error.localizedDescription
        }
    }
}

/// A theme's colours at a glance: window, panel, accent and the five strip
/// kinds, drawn from its tokens.
struct ThemeSwatches: View {
    let theme: Theme

    var body: some View {
        let s = theme.strip
        let colours = [
            theme.surface.window, theme.surface.panel, theme.accent.accent,
            s.channel, s.instrument, s.auxReturn, s.bus, s.master,
        ]
        HStack(spacing: theme.spacing.xxs) {
            ForEach(colours.indices, id: \.self) { i in
                Rectangle()
                    .fill(colours[i].color)
                    .frame(width: 12, height: 12)
                    .overlay(Rectangle().stroke(theme.surface.separator.color, lineWidth: 0.5))
            }
        }
        .accessibilityHidden(true)
    }
}
