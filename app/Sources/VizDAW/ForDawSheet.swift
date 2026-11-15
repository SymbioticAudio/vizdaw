import SwiftUI
import SymbioticTokens
import VizDAWAppModel
import VizDAWCore

/// File › Export › For Another DAW… (spec §9; plan 6b Task 5): the stems,
/// their format, "Add the stems to this project", where every file starts,
/// and "What won't transfer" from the core's check as the choices change.
/// The sheet comes first and Export asks where (Ruling 11). All state lives
/// in `ForDawModel`.
struct ForDawSheet: View {
    @Environment(\.symbioticTheme) private var theme
    @Bindable var forDaw: ForDawModel
    let document: DocumentModel

    /// A kept or running export has its choices already: they stay as sent.
    private var busy: Bool { forDaw.waiting || forDaw.running }

    var body: some View {
        VStack(alignment: .leading, spacing: theme.spacing.m) {
            Text("Export for Another DAW").font(theme.type.title.font)
            Text("Audio stems and MIDI files for any DAW")
                .font(theme.type.caption.font)
                .foregroundStyle(theme.text.secondary.color)
            Text("Stems").font(theme.type.body.font)
            List(forDaw.strips, id: \.id) { s in
                Toggle(
                    s.name,
                    isOn: Binding(
                        get: { forDaw.stems.contains(s.id) },
                        set: { if $0 { forDaw.stems.insert(s.id) } else { forDaw.stems.remove(s.id) } })
                )
                .accessibilityLabel("Stem for \(s.name)")
            }
            .frame(height: 120)
            .disabled(busy)
            AudioFormatRows(choices: forDaw)
                .disabled(busy)
            Toggle("Add the stems to this project", isOn: $forDaw.addToProject)
                .accessibilityLabel("Add the stems to this project")
                .disabled(busy)
            if let check = forDaw.check {
                Text(check.start)
                    .font(theme.type.caption.font)
                    .foregroundStyle(theme.text.secondary.color)
                    .fixedSize(horizontal: false, vertical: true)
                Text("Length \(forDaw.length)")
                    .font(theme.type.caption.font)
                    .foregroundStyle(theme.text.secondary.color)
                if let w = check.warning {
                    AppIcon.warning.label(w)
                        .font(theme.type.body.font)
                        .fixedSize(horizontal: false, vertical: true)
                }
            }
            Text("What won't transfer").font(theme.type.body.font)
            WontTransfer(report: forDaw.check?.report, checking: forDaw.error == nil)
            if let e = forDaw.error {
                AppIcon.fault.label(e)
                    .font(theme.type.body.font)
                    .foregroundStyle(theme.state.error.color)
                    .fixedSize(horizontal: false, vertical: true)
            }
            if forDaw.waiting {
                HStack(spacing: theme.spacing.s) {
                    ProgressView().controlSize(.small)
                        .accessibilityLabel("Waiting")
                    Text("Waiting for the running import, export or bounce to finish…")
                        .font(theme.type.caption.font)
                        .foregroundStyle(theme.text.secondary.color)
                }
            } else if forDaw.running, let p = forDaw.progress {
                ProgressView(value: Double(p))
                    .accessibilityLabel("Export progress")
            }
            HStack {
                Spacer()
                if forDaw.waiting {
                    Button("Cancel") { forDaw.cancelWaiting() }
                        .keyboardShortcut(.cancelAction)
                        .accessibilityLabel("Cancel the waiting export")
                } else if forDaw.running, let job = forDaw.job {
                    Button("Cancel") { document.cancelJob(job) }
                        .keyboardShortcut(.cancelAction)
                        .accessibilityLabel("Cancel the export")
                } else {
                    Button("Cancel") { forDaw.shown = false }
                        .keyboardShortcut(.cancelAction)
                        .accessibilityLabel("Cancel")
                    Button("Export…") { begin() }
                        .keyboardShortcut(.defaultAction)
                        .disabled(forDaw.check == nil)
                        .accessibilityLabel("Export, then choose where")
                }
            }
        }
        .padding(theme.spacing.l)
        .frame(width: 480)
    }

    /// The folder panel, then the job (or the wait for a running one).
    private func begin() {
        guard let folder = Panels.chooseExportFolder() else { return }
        forDaw.start(folder: folder)
    }
}
