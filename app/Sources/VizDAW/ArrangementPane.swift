import AppKit
import SwiftUI
import SymbioticTokens
import VizDAWAppModel
import VizDAWCore
import VizDAWEditors

/// The arrangement with its tool bar (the transport bar and the status
/// line are the shell's, plan 7a Ruling 9).
struct ArrangementPane: View {
    let model: DocumentModel

    var body: some View {
        VStack(spacing: 0) {
            // In a narrow centre the bar scrolls rather than wrap its labels.
            ViewThatFits(in: .horizontal) {
                ArrangementBar(model: model, editor: model.editor)
                // With indicators: in a narrow centre Zoom In and Out are
                // off the right edge, and nothing else says so (Task 7 M6).
                ScrollView(.horizontal, showsIndicators: true) {
                    ArrangementBar(model: model, editor: model.editor)
                }
            }
            Divider()
            if model.selectedSeq == nil {
                ContentUnavailableView(
                    "No Sequence", icon: .list,
                    description: Text("Choose File > Create Demo Project to try VizDAW."))
            } else {
                ArrangementHost(document: model)
            }
        }
    }
}

struct ArrangementHost: NSViewRepresentable {
    let document: DocumentModel

    func makeNSView(context: Context) -> ArrangementContainer {
        ArrangementContainer(document: document)
    }

    func updateNSView(_ view: ArrangementContainer, context: Context) {
        view.theme = context.environment.symbioticTheme
    }
}

struct ArrangementBar: View {
    let model: DocumentModel
    @Bindable var editor: EditorModel

    var body: some View {
        HStack(spacing: 10) {
            if let parent = editor.navigation.last,
                let name = model.snapshot.sequences.first(where: { $0.id == parent })?.name
            {
                Button("◀ \(name)") { editor.back() }
                    .help("Back to the sequence this reference sits in")
            }
            Picker("Tool", selection: $editor.tool) {
                ForEach(Tool.allCases, id: \.self) { Text($0.title).tag($0) }
            }
            .pickerStyle(.segmented)
            .labelsHidden()
            .frame(width: 210)
            Toggle(
                "Snap",
                isOn: Binding(
                    get: { editor.layout.snap.enabled },
                    set: { on in editor.updateLayout { $0.snap.enabled = on } })
            )
            .help("Snap to the edit grid (G)")
            Menu(editor.layout.snap.division.title) {
                ForEach(Snap.Division.allCases, id: \.self) { d in
                    Button(d.title) { editor.updateLayout { $0.snap.division = d } }
                }
                Divider()
                Toggle(
                    "Triplet",
                    isOn: Binding(
                        get: { editor.layout.snap.triplet },
                        set: { on in editor.updateLayout { $0.snap.triplet = on } }))
                Toggle(
                    "Dotted",
                    isOn: Binding(
                        get: { editor.layout.snap.dotted },
                        set: { on in editor.updateLayout { $0.snap.dotted = on } }))
            }
            .frame(width: 70)
            Menu("Blocks: \(editor.layout.blockRule.title)") {
                ForEach(BlockRuleSetting.choices, id: \.self) { r in
                    Button(r.title) { editor.updateLayout { $0.blockRule = r } }
                }
            }
            .frame(width: 210)
            Menu("Light: \(editor.layout.lightMode.title)") {
                ForEach(LightMode.allCases, id: \.self) { m in
                    Button(m.title) { editor.updateLayout { $0.lightMode = m } }
                }
            }
            .frame(width: 140)
            Toggle(
                "Follow",
                isOn: Binding(
                    get: { editor.layout.follow },
                    set: { on in editor.updateLayout { $0.follow = on } }))
            Spacer()
            Button {
                editor.updateLayout { $0.ticksPerPoint = min($0.ticksPerPoint * 1.5, 61_440) }
            } label: {
                AppIcon.zoomOut.image
            }
            .help("Zoom Out")
            Button {
                editor.updateLayout { $0.ticksPerPoint = max($0.ticksPerPoint / 1.5, 2) }
            } label: {
                AppIcon.zoomIn.image
            }
            .help("Zoom In")
        }
        .padding(.horizontal, 8)
        .padding(.vertical, 4)
    }
}

/// The transport bar's middle part (plan 4f Rulings 11, 16; plan 7d Ruling
/// 6): step entry's summary when there is one, the bar, time, tempo and
/// signature readouts, then the fields. They share a line when they fit
/// and otherwise each takes a line of its own, so a narrow bar grows a line
/// rather than cover its buttons.
struct TransportBar: View {
    @Environment(\.symbioticTheme) private var theme
    let model: DocumentModel

    var body: some View {
        TransportFlowLayout(spacing: theme.spacing.m, lineSpacing: theme.spacing.xxs) {
            if let s = model.stepEntry.summary {
                Text(s)
                    .font(theme.type.caption.font)
                    .padding(.horizontal, theme.spacing.s)
                    .padding(.vertical, theme.spacing.xxs)
                    .background(
                        theme.accent.accent.color.opacity(0.18), in: RoundedRectangle(cornerRadius: theme.radius.s))
                    .accessibilityLabel(s)
                    .fixedSize()
            }
            TransportReadoutsHost(document: model)
            TransportFieldsHost(document: model)
                .frame(height: 22)
        }
    }
}

/// The summary (when there is one), the readouts, then the fields, in
/// lines by `TransportFlow`. The last subview is the fields, which take
/// what is left of a line, down to their minimum width.
struct TransportFlowLayout: Layout {
    var spacing: CGFloat
    var lineSpacing: CGFloat

    /// The fields' least width: a subview asked for no width reports it.
    private func minimum(_ subviews: Subviews) -> CGFloat {
        subviews.last?.sizeThatFits(.zero).width ?? 0
    }

    private func lines(_ width: CGFloat, _ s: [CGSize], _ subviews: Subviews) -> [[Int]] {
        TransportFlow.lines(
            widths: s.map(\.width), lastMinimum: minimum(subviews), width: width, spacing: spacing)
    }

    func sizeThatFits(proposal: ProposedViewSize, subviews: Subviews, cache: inout ()) -> CGSize {
        let s = subviews.map { $0.sizeThatFits(.unspecified) }
        guard !s.isEmpty else { return .zero }
        // Its ideal width is every item on one line.
        let ideal = s.map(\.width).reduce(0, +) + spacing * CGFloat(s.count - 1)
        let width = proposal.width.flatMap { $0.isFinite ? $0 : nil } ?? ideal
        let rows = lines(width, s, subviews)
        let heights = rows.map { row in row.map { s[$0].height }.max() ?? 0 }
        return CGSize(width: width, height: heights.reduce(0, +) + lineSpacing * CGFloat(max(rows.count - 1, 0)))
    }

    func placeSubviews(in bounds: CGRect, proposal: ProposedViewSize, subviews: Subviews, cache: inout ()) {
        let s = subviews.map { $0.sizeThatFits(.unspecified) }
        guard !s.isEmpty else { return }
        var y = bounds.minY
        for row in lines(bounds.width, s, subviews) {
            let height = row.map { s[$0].height }.max() ?? 0
            var x = bounds.minX
            for i in row {
                // The fields take what is left of their line.
                let w = i == s.count - 1 ? max(bounds.maxX - x, 0) : s[i].width
                subviews[i].place(
                    at: CGPoint(x: x, y: y + height / 2), anchor: .leading,
                    proposal: ProposedViewSize(width: w, height: s[i].height))
                x += w + spacing
            }
            y += height + lineSpacing
        }
    }
}

struct TransportReadoutsHost: NSViewRepresentable {
    let document: DocumentModel

    func makeNSView(context: Context) -> TransportReadoutsView { TransportReadoutsView(document: document) }

    func updateNSView(_ view: TransportReadoutsView, context: Context) {
        view.theme = context.environment.symbioticTheme
    }

    /// Its own fixed size: the boxes keep their widths.
    func sizeThatFits(_ proposal: ProposedViewSize, nsView: TransportReadoutsView, context: Context) -> CGSize? {
        nsView.intrinsicContentSize
    }
}

struct TransportFieldsHost: NSViewRepresentable {
    let document: DocumentModel

    func makeNSView(context: Context) -> TransportFieldsView { TransportFieldsView(document: document) }

    func updateNSView(_ view: TransportFieldsView, context: Context) {
        view.theme = context.environment.symbioticTheme
    }

    /// Its fitting width when asked for its ideal size, so the transport
    /// bar can tell whether one row holds it; else what it is offered.
    func sizeThatFits(_ proposal: ProposedViewSize, nsView: TransportFieldsView, context: Context) -> CGSize? {
        let fit = nsView.fittingWidth
        // Asked for no width, it reports its least (the bar's flow keeps it
        // at least this wide).
        let w = proposal.width.flatMap { $0.isFinite ? max($0, nsView.minimumWidth) : nil } ?? fit
        return CGSize(width: w, height: 22)
    }
}

/// The last failure of an editor action (spec §5: non-modal).
struct StatusLine: View {
    @Environment(\.symbioticTheme) private var theme
    let editor: EditorModel

    var body: some View {
        if let s = editor.status {
            HStack {
                AppIcon.warning.image.foregroundStyle(theme.state.warning.color)
                Text(s).lineLimit(1)
                Spacer()
                Button("Dismiss") { editor.clearStatus() }.buttonStyle(.borderless)
            }
            .font(theme.type.caption.font)
            .padding(.horizontal, 8)
            .padding(.vertical, 3)
            .background(.bar)
            .task(id: s) {
                try? await Task.sleep(for: .seconds(6))
                if editor.status == s { editor.clearStatus() }
            }
        }
    }
}
