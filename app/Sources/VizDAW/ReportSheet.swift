import AppKit
import SwiftUI
import SymbioticTokens
import VizDAWAppModel
import VizDAWCore

/// The import, export and merge report (spec §6.3, §9; plan 6a Task 10):
/// the summary line, then Conflicts, Changed, Left out and Notes with counts and
/// disclosure rows naming tracks and bars; Copy, Save as Text… and, for an
/// export, Show in Finder. Wording from `ReportText`, shared with Copy.
struct ReportSheet: View {
    @Environment(\.symbioticTheme) private var theme
    @Bindable var interchange: InterchangeModel

    var body: some View {
        VStack(alignment: .leading, spacing: theme.spacing.m) {
            Text(title).font(theme.type.title.font)
            if let report = interchange.report {
                Text(report.summary)
                    .font(theme.type.body.font)
                    .fixedSize(horizontal: false, vertical: true)
                    .textSelection(.enabled)
                let groups = ReportText.groups(report)
                if groups.isEmpty {
                    Text("Everything transferred.")
                        .font(theme.type.body.font)
                        .foregroundStyle(theme.text.secondary.color)
                } else {
                    ScrollView(.vertical) {
                        VStack(alignment: .leading, spacing: theme.spacing.s) {
                            ForEach(Array(groups.enumerated()), id: \.offset) { _, g in
                                DisclosureGroup {
                                    VStack(alignment: .leading, spacing: theme.spacing.xs) {
                                        ForEach(Array(g.items.enumerated()), id: \.offset) { _, item in
                                            ReportItemView(item: item)
                                        }
                                    }
                                    .padding(.leading, theme.spacing.m)
                                } label: {
                                    GroupTitle(title: g.title, count: g.items.reduce(0) { $0 + $1.count })
                                }
                                .accessibilityLabel("\(g.title), \(g.items.reduce(0) { $0 + $1.count })")
                            }
                        }
                        .frame(maxWidth: .infinity, alignment: .leading)
                    }
                    .frame(minHeight: 120, maxHeight: 360)
                }
            }
            HStack {
                Button("Copy") { copy() }
                    .accessibilityLabel("Copy the report")
                Button("Save as Text…") { save() }
                    .accessibilityLabel("Save the report as text")
                if !interchange.written.isEmpty {
                    Button("Show in Finder") {
                        NSWorkspace.shared.activateFileViewerSelecting(
                            interchange.written.map { URL(fileURLWithPath: $0) })
                    }
                    .accessibilityLabel("Show the exported files in Finder")
                }
                Spacer()
                // As the Track Synth sheet's Done: Escape closes it.
                Button("Done") { interchange.reportShown = false }
                    .keyboardShortcut(.cancelAction)
            }
        }
        .padding(theme.spacing.l)
        .frame(width: 520)
    }

    /// The core's summary starts "Imported", "Exported" or "Merged", or
    /// is "Nothing to merge…" (plan 6e Task 11).
    private var title: String {
        let summary = interchange.report?.summary ?? ""
        if !interchange.written.isEmpty || summary.hasPrefix("Exported") { return "Export Report" }
        if summary.hasPrefix("Merged") || summary.hasPrefix("Nothing to merge") { return "Merge Report" }
        return summary.hasPrefix("Imported") ? "Import Report" : "Report"
    }

    private var text: String { interchange.report.map(ReportText.plain) ?? "" }

    private func copy() {
        NSPasteboard.general.clearContents()
        NSPasteboard.general.setString(text, forType: .string)
    }

    private func save() {
        guard let path = Panels.chooseReportText(name: "MIDI Report") else { return }
        do {
            try text.write(toFile: path, atomically: true, encoding: .utf8)
        } catch {
            NSAlert(error: error).runModal()
        }
    }
}

/// A group's title and count. A merge's "Conflicts" (plan 6e Ruling 15),
/// listed first, carries the warning icon and colour: VizDAW's version
/// was kept there.
private struct GroupTitle: View {
    @Environment(\.symbioticTheme) private var theme
    let title: String
    let count: UInt64

    var body: some View {
        if title == "Conflicts" {
            AppIcon.warning.label("\(title) (\(count))")
                .font(theme.type.body.font.weight(.semibold))
                .foregroundStyle(theme.state.warning.color)
        } else {
            Text("\(title) (\(count))")
                .font(theme.type.body.font.weight(.semibold))
        }
    }
}

/// One report line: its places in a disclosure row when it has any.
private struct ReportItemView: View {
    @Environment(\.symbioticTheme) private var theme
    let item: ReportItemRow

    var body: some View {
        let details = ReportText.details(item)
        if details.isEmpty {
            Text(ReportText.line(item))
                .font(theme.type.body.font)
                .fixedSize(horizontal: false, vertical: true)
        } else {
            DisclosureGroup {
                VStack(alignment: .leading, spacing: theme.spacing.xxs) {
                    ForEach(Array(details.enumerated()), id: \.offset) { _, d in
                        Text(d)
                            .font(theme.type.caption.font)
                            .foregroundStyle(theme.text.secondary.color)
                            .textSelection(.enabled)
                    }
                }
                .padding(.leading, theme.spacing.m)
            } label: {
                Text(ReportText.line(item))
                    .font(theme.type.body.font)
                    .fixedSize(horizontal: false, vertical: true)
            }
        }
    }
}
