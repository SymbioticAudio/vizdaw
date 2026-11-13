import SwiftUI
import SymbioticTokens
import VizDAWAppModel
import VizDAWCore

/// The return prompt's second page (plan 6e Ruling 14; spec §9): Merge
/// DAWproject on the window of the project it changes. While the Hold job
/// reads the file, "Reading the file…" with its progress; when the file lost
/// its export ID, the project's exports to choose from; then the core's
/// counts line and its conflict lines; Merge or Cancel. Every sentence the
/// core owns (the summary, the conflicts, a refusal) is shown as it comes.
/// All state lives in `MergeModel`.
struct MergeSheet: View {
    @Environment(\.symbioticTheme) private var theme
    @Bindable var merge: MergeModel

    /// The snapshot list shows until a choice starts the job.
    private var choosing: Bool {
        !merge.snapshots.isEmpty && merge.chosenSnapshot == nil && merge.job == nil && !merge.waiting
    }

    var body: some View {
        VStack(alignment: .leading, spacing: theme.spacing.m) {
            Text("Merge DAWproject").font(theme.type.title.font)
            Text((merge.path as NSString).lastPathComponent)
                .font(theme.type.caption.font)
                .foregroundStyle(theme.text.secondary.color)
                .lineLimit(1)
                .truncationMode(.middle)
            if merge.waiting {
                HStack(spacing: theme.spacing.s) {
                    ProgressView().controlSize(.small)
                        .accessibilityLabel("Waiting")
                    Text("Waiting for the running job to finish…")
                        .font(theme.type.caption.font)
                        .foregroundStyle(theme.text.secondary.color)
                }
            } else if merge.running {
                Text("Reading the file…").font(theme.type.body.font)
                ProgressView(value: Double(merge.progress ?? 0))
                    .accessibilityLabel("Merge progress")
            } else if choosing {
                SnapshotList(merge: merge)
            } else if let preview = merge.preview {
                PreviewView(preview: preview)
            }
            if let e = merge.error {
                AppIcon.warning.label(e)
                    .font(theme.type.body.font)
                    .foregroundStyle(theme.state.warning.color)
                    .fixedSize(horizontal: false, vertical: true)
                    .textSelection(.enabled)
            }
            HStack {
                Spacer()
                Button("Cancel") { merge.cancel() }
                    .keyboardShortcut(.cancelAction)
                    .accessibilityLabel(merge.held ? "Cancel the merge; nothing changes" : "Cancel")
                if choosing {
                    Button("Preview") { merge.previewSelected() }
                        .keyboardShortcut(.defaultAction)
                        .disabled(merge.selectedSnapshot == nil)
                        .accessibilityLabel("Preview the merge with the chosen export")
                } else {
                    if merge.canPreviewAgain {
                        Button("Preview Again") { merge.previewAgain() }
                            .accessibilityLabel("Preview the merge again")
                    }
                    Button("Merge") { merge.confirm() }
                        .keyboardShortcut(.defaultAction)
                        .disabled(!merge.canMerge)
                        .accessibilityLabel("Merge into this project")
                }
            }
        }
        .padding(theme.spacing.l)
        .frame(width: 520)
    }
}

/// "Which export did this file come from?": the project's exports, newest
/// first, as their date and time and track count.
private struct SnapshotList: View {
    @Environment(\.symbioticTheme) private var theme
    @Bindable var merge: MergeModel

    var body: some View {
        VStack(alignment: .leading, spacing: theme.spacing.s) {
            Text("Which export did this file come from?").font(theme.type.body.font)
            List(merge.snapshots, id: \.exportId, selection: $merge.selectedSnapshot) { row in
                HStack(spacing: theme.spacing.s) {
                    Text(Self.when(row.exportedAt))
                    Spacer()
                    Text(Self.tracks(row.tracks))
                        .foregroundStyle(theme.text.secondary.color)
                        .monospacedDigit()
                }
                .font(theme.type.body.font)
                .accessibilityElement(children: .combine)
                .accessibilityLabel("Export of \(Self.when(row.exportedAt)), \(Self.tracks(row.tracks))")
            }
            .frame(minHeight: 120, maxHeight: 220)
            .accessibilityLabel("Exports of this project")
        }
    }

    /// The export's local date and time.
    static func when(_ secs: UInt64) -> String {
        Date(timeIntervalSince1970: TimeInterval(secs)).formatted(date: .abbreviated, time: .shortened)
    }

    static func tracks(_ n: UInt32) -> String { n == 1 ? "1 track" : "\(n) tracks" }
}

/// The preview: the core's counts line, then the conflicts it lists.
private struct PreviewView: View {
    @Environment(\.symbioticTheme) private var theme
    let preview: MergePreviewRow

    var body: some View {
        VStack(alignment: .leading, spacing: theme.spacing.s) {
            Text(preview.summary)
                .font(theme.type.body.font)
                .fixedSize(horizontal: false, vertical: true)
                .textSelection(.enabled)
            if preview.conflictLines.isEmpty {
                Text("No conflicts.")
                    .font(theme.type.body.font)
                    .foregroundStyle(theme.text.secondary.color)
            } else {
                Text("Conflicts (VizDAW's version is kept)")
                    .font(theme.type.body.font.weight(.semibold))
                    .foregroundStyle(theme.state.warning.color)
                ScrollView(.vertical) {
                    VStack(alignment: .leading, spacing: theme.spacing.xs) {
                        ForEach(Array(preview.conflictLines.enumerated()), id: \.offset) { _, line in
                            AppIcon.warning.label(line)
                                .font(theme.type.body.font)
                                .fixedSize(horizontal: false, vertical: true)
                                .textSelection(.enabled)
                        }
                    }
                    .frame(maxWidth: .infinity, alignment: .leading)
                }
                .frame(minHeight: 60, maxHeight: 240)
                .accessibilityLabel("Conflicts, \(preview.conflictLines.count)")
            }
        }
    }
}
