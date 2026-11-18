import AppKit
import SwiftUI
import SymbioticTokens
import VizDAWAppModel

/// Settings › Co-Producer (plan 8a Ruling 10): the switch, what it means,
/// where `vizdaw-mcp` is, and how many saved copies the open projects
/// hold; then Time (plan 8f Ruling 1). All state lives in
/// `CoProducerSettings` and `CoProducerMetrics`; the view only reads it.
struct CoProducerSettingsView: View {
    @Environment(\.symbioticTheme) private var theme
    @Bindable var settings: CoProducerSettings
    @Bindable var metrics: CoProducerMetrics
    let store: DocumentStore

    /// `vizdaw-mcp`, next to the app's executable in the bundle.
    static var serverPath: String {
        let exe = Bundle.main.executableURL ?? URL(fileURLWithPath: CommandLine.arguments[0])
        return exe.deletingLastPathComponent().appendingPathComponent("vizdaw-mcp").path
    }

    var body: some View {
        Form {
            Toggle("Co-Producer", isOn: Binding(get: { settings.on }, set: { settings.setOn($0) }))
                .accessibilityLabel("Co-Producer")
            Text(
                "Lets Claude work on your songs through VizDAW's Co-Producer connection. Each request is one undo step, a copy of the song is saved first, and nothing that changes or deletes your own work happens without your yes."
            )
            .font(theme.type.caption.font)
            .foregroundStyle(theme.text.secondary.color)
            Text(settings.status)
                .foregroundStyle(theme.text.primary.color)
                .accessibilityLabel(settings.status)
            if settings.on {
                HStack(spacing: theme.spacing.s) {
                    Text(Self.serverPath)
                        .font(theme.type.caption.font)
                        .foregroundStyle(theme.text.primary.color)
                        .textSelection(.enabled)
                        .accessibilityLabel("The Co-Producer server's path: \(Self.serverPath)")
                    Spacer(minLength: theme.spacing.m)
                    Button("Copy Path") {
                        NSPasteboard.general.clearContents()
                        NSPasteboard.general.setString(Self.serverPath, forType: .string)
                    }
                    .accessibilityLabel("Copy the Co-Producer server's path")
                }
            }
            Text(settings.copiesLine)
                .font(theme.type.caption.font)
                .foregroundStyle(theme.text.secondary.color)
                .accessibilityLabel(settings.copiesLine)
            if let problem = settings.problem {
                Text(problem)
                    .foregroundStyle(theme.state.warning.color)
                    .accessibilityLabel(problem)
            }
            approvals
            time
        }
        .formStyle(.grouped)
        .frame(width: 500)
        .onAppear {
            settings.readCopies(store)
            settings.readApprovals()
            metrics.refreshOverview()
        }
        // Only the composer's own click asks; the control channel's delete
        // opens nothing.
        .alert("Delete all time records?", isPresented: $metrics.confirmingDelete) {
            Button("Delete", role: .destructive) { metrics.deleteAll() }
                .accessibilityLabel("Delete all time records")
            Button("Cancel", role: .cancel) { metrics.confirmingDelete = false }
                .accessibilityLabel("Cancel")
        } message: {
            Text("This deletes everything VizDAW has counted about where your time goes, for every song. It cannot be undone.")
        }
    }

    /// Time (plan 8f Ruling 1): Count where my time goes, what it means,
    /// what is kept, and Delete All Time Records….
    private var time: some View {
        Section {
            Toggle(CoProducerMetrics.label, isOn: Binding(get: { metrics.on }, set: { metrics.setOn($0) }))
                .accessibilityLabel("Count where my time goes")
            Text(CoProducerMetrics.caption)
                .font(theme.type.caption.font)
                .foregroundStyle(theme.text.secondary.color)
            Text(metrics.overviewLine)
                .foregroundStyle(theme.text.primary.color)
                .accessibilityLabel(metrics.overviewLine)
            HStack {
                Spacer()
                Button("Delete All Time Records…") { metrics.confirmingDelete = true }
                    .disabled(metrics.overview == nil)
                    .accessibilityLabel("Delete all time records")
            }
            if let problem = metrics.problem {
                Text(problem)
                    .foregroundStyle(theme.state.warning.color)
                    .accessibilityLabel(problem)
            }
        } header: {
            Text("Time")
                .font(theme.type.body.font)
                .foregroundStyle(theme.text.primary.color)
                .accessibilityLabel("Time")
        }
    }

    /// "Apps that may change songs without asking" (plan 8b Ruling 11):
    /// each standing approval with Revoke, and Revoke All.
    private var approvals: some View {
        Section {
            if settings.approvalRows.isEmpty {
                Text(settings.approvalsEmptyLine)
                    .font(theme.type.caption.font)
                    .foregroundStyle(theme.text.secondary.color)
                    .accessibilityLabel(settings.approvalsEmptyLine)
            } else {
                ForEach(settings.approvalRows) { row in
                    HStack(spacing: theme.spacing.s) {
                        Text(row.text)
                            .font(theme.type.caption.font)
                            .foregroundStyle(theme.text.primary.color)
                            .textSelection(.enabled)
                            .accessibilityLabel(row.accessibility)
                        Spacer(minLength: theme.spacing.m)
                        Button("Revoke") { settings.revoke(row.id) }
                            .accessibilityLabel("Revoke: \(row.accessibility)")
                    }
                }
                HStack {
                    Spacer()
                    Button("Revoke All") { settings.revokeAll() }
                        .accessibilityLabel("Revoke all approvals")
                }
            }
        } header: {
            Text(CoProducerSettings.approvalsTitle)
                .font(theme.type.body.font)
                .foregroundStyle(theme.text.primary.color)
                .accessibilityLabel(CoProducerSettings.approvalsTitle)
        }
    }
}
