import SwiftUI
import SymbioticTokens
import VizDAWAppModel
import VizDAWCore

/// File › Project Settings… (plan 6a Task 10): the project's format and its
/// song info. OK sends only what changed (`ProjectSettingsModel.apply`):
/// a song-info change is one undo step ("Set Song Info"), a format change
/// a conversion job.
struct ProjectSettingsSheet: View {
    @Environment(\.symbioticTheme) private var theme
    @Bindable var settings: ProjectSettingsModel
    let editor: EditorModel

    /// The 13 song-info fields: label, accessibility label, text.
    private var fields: [(String, String, WritableKeyPath<SongText, String>)] {
        [
            ("Title", "Song title", \.title), ("Artist", "Artist", \.artist), ("Album", "Album", \.album),
            ("Original artist", "Original artist", \.originalArtist), ("Composer", "Composer", \.composer),
            ("Songwriter", "Songwriter", \.songwriter), ("Producer", "Producer", \.producer),
            ("Arranger", "Arranger", \.arranger), ("Year", "Year", \.year), ("Genre", "Genre", \.genre),
            ("Copyright", "Copyright", \.copyright), ("Website", "Website", \.website),
        ]
    }

    var body: some View {
        VStack(alignment: .leading, spacing: theme.spacing.m) {
            Text("Project Settings").font(theme.type.title.font)
            Form {
                Section("Format") {
                    Picker("Sample rate", selection: $settings.rate) {
                        ForEach(projectRates(), id: \.self) { Text(ProjectBanners.khz(Double($0))).tag($0) }
                    }
                    .accessibilityLabel("Sample rate")
                    Picker("Bit depth", selection: $settings.depth) {
                        ForEach(FormatPanel.depths, id: \.1) { Text($0.1).tag($0.0) }
                    }
                    .accessibilityLabel("Bit depth")
                    Text(settings.mediaNote)
                        .font(theme.type.caption.font)
                        .foregroundStyle(theme.text.secondary.color)
                        .fixedSize(horizontal: false, vertical: true)
                }
                Section("Song info") {
                    ForEach(fields, id: \.0) { label, a11y, key in
                        TextField(label, text: $settings.song[dynamicMember: key])
                            .accessibilityLabel(a11y)
                    }
                    TextField("Comment", text: $settings.song.comment, axis: .vertical)
                        .lineLimit(3...6)
                        .accessibilityLabel("Comment")
                }
            }
            HStack {
                Spacer()
                Button("Cancel") { settings.shown = false }
                    .keyboardShortcut(.cancelAction)
                Button("OK") {
                    settings.shown = false
                    settings.apply()
                }
                .keyboardShortcut(.defaultAction)
                // A held merge holds a format change back (plan 6e Task 11).
                .disabled(settings.formatBlocked)
                .help(settings.formatBlocked ? MergeModel.heldHelp : "")
            }
        }
        .padding(theme.spacing.l)
        .frame(width: 480)
        .onAppear { editor.beginTextEditing("projectSettings") }
        .onDisappear { editor.endTextEditing("projectSettings") }
    }
}
