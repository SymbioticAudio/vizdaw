import SwiftUI
import SymbioticTokens
import VizDAWAppModel
import VizDAWCore

/// File › Export › DAWproject… (spec §9; plan 6d Task 13): the region gain
/// choice and "What won't transfer" from the core's check as it changes.
/// The sheet comes first and Export… asks where, with the core's file name.
/// The Save panel confirms a replace, so a chosen path is sent with
/// `replace`. All state lives in `DawprojectModel`.
struct DawprojectSheet: View {
    @Environment(\.symbioticTheme) private var theme
    @Bindable var dawproject: DawprojectModel
    let document: DocumentModel

    /// A kept or running export has its choices already: they stay as sent.
    private var busy: Bool { dawproject.waiting || dawproject.running }

    var body: some View {
        VStack(alignment: .leading, spacing: theme.spacing.m) {
            Text("Export DAWproject").font(theme.type.title.font)
            Text("Most complete: tracks, mix, plug-ins, automation")
                .font(theme.type.caption.font)
                .foregroundStyle(theme.text.secondary.color)
            Picker("Region gain", selection: $dawproject.regionGain) {
                Text("Apply to the audio").tag(RegionGainRow.apply)
                Text("Leave out").tag(RegionGainRow.leaveOut)
            }
            .accessibilityLabel("Region gain")
            .disabled(busy)
            Text("What won't transfer").font(theme.type.body.font)
            WontTransfer(report: dawproject.wontTransfer, checking: dawproject.checkError == nil)
            if let e = dawproject.checkError {
                AppIcon.fault.label(e)
                    .font(theme.type.body.font)
                    .foregroundStyle(theme.state.error.color)
                    .fixedSize(horizontal: false, vertical: true)
            }
            if dawproject.waiting {
                HStack(spacing: theme.spacing.s) {
                    ProgressView().controlSize(.small)
                        .accessibilityLabel("Waiting")
                    Text("Waiting for the running job to finish…")
                        .font(theme.type.caption.font)
                        .foregroundStyle(theme.text.secondary.color)
                }
            } else if dawproject.running, let p = dawproject.progress {
                ProgressView(value: Double(p))
                    .accessibilityLabel("Export progress")
            }
            HStack {
                Spacer()
                if dawproject.waiting {
                    Button("Cancel") { dawproject.cancelWaiting() }
                        .keyboardShortcut(.cancelAction)
                        .accessibilityLabel("Cancel the waiting export")
                } else if dawproject.running, let job = dawproject.job {
                    Button("Cancel") { document.cancelJob(job) }
                        .keyboardShortcut(.cancelAction)
                        .accessibilityLabel("Cancel the export")
                } else {
                    Button("Cancel") { dawproject.shown = false }
                        .keyboardShortcut(.cancelAction)
                        .accessibilityLabel("Cancel")
                    Button("Export…") { begin() }
                        .keyboardShortcut(.defaultAction)
                        .disabled(dawproject.wontTransfer == nil || dawproject.defaultName.isEmpty)
                        .accessibilityLabel("Export, then choose where")
                }
            }
        }
        .padding(theme.spacing.l)
        .frame(width: 480)
    }

    /// The Save panel with the core's name, then the job (or the wait for a
    /// running one). The panel has confirmed any replace.
    private func begin() {
        guard let path = Panels.chooseDawprojectFile(name: dawproject.defaultName) else { return }
        dawproject.start(path: path, replace: true)
    }
}
