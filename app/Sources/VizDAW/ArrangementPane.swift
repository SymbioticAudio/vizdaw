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
                    "No Sequence", systemImage: AppIcon.list.symbol,
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

/// The transport fields and step entry's line (plan 4f Rulings 11, 16), in
/// the transport bar.
struct TransportBar: View {
    let model: DocumentModel

    var body: some View {
        // Step entry's summary in front of the fields, or above them when
        // both do not fit on one line, so a narrow bar never cuts either
        // (Task 7 review I1).
        SummaryAndFields(spacing: 12) {
            if let s = model.stepEntry.summary {
                Text(s)
                    .font(.caption)
                    .padding(.horizontal, 6)
                    .padding(.vertical, 2)
                    .background(Color.accentColor.opacity(0.18), in: RoundedRectangle(cornerRadius: 4))
                    .accessibilityLabel(s)
                    .fixedSize()
            }
            TransportFieldsHost(document: model)
                .frame(height: 22)
        }
    }
}

/// The summary (when there is one) then the fields on one line, or the
/// summary above the fields when the line is narrower than both at their
/// ideal widths. The last subview is the fields, which take what is left.
struct SummaryAndFields: Layout {
    var spacing: CGFloat
    var lineSpacing: CGFloat = 2

    private func oneLine(_ width: CGFloat, _ s: [CGSize]) -> Bool {
        s.count < 2 || width >= s[0].width + spacing + s[1].width
    }

    func sizeThatFits(proposal: ProposedViewSize, subviews: Subviews, cache: inout ()) -> CGSize {
        let s = subviews.map { $0.sizeThatFits(.unspecified) }
        guard let fields = s.last else { return .zero }
        let ideal = s.count < 2 ? fields.width : s[0].width + spacing + fields.width
        let width = proposal.width.flatMap { $0.isFinite ? $0 : nil } ?? ideal
        let height = oneLine(width, s) ? (s.map(\.height).max() ?? 0) : s[0].height + lineSpacing + fields.height
        return CGSize(width: width, height: height)
    }

    func placeSubviews(in bounds: CGRect, proposal: ProposedViewSize, subviews: Subviews, cache: inout ()) {
        let s = subviews.map { $0.sizeThatFits(.unspecified) }
        guard let fields = subviews.last, let fs = s.last else { return }
        if s.count < 2 {
            fields.place(
                at: CGPoint(x: bounds.minX, y: bounds.midY), anchor: .leading,
                proposal: ProposedViewSize(width: bounds.width, height: fs.height))
        } else if oneLine(bounds.width, s) {
            subviews[0].place(at: CGPoint(x: bounds.minX, y: bounds.midY), anchor: .leading, proposal: ProposedViewSize(s[0]))
            fields.place(
                at: CGPoint(x: bounds.minX + s[0].width + spacing, y: bounds.midY), anchor: .leading,
                proposal: ProposedViewSize(width: max(bounds.width - s[0].width - spacing, 0), height: fs.height))
        } else {
            subviews[0].place(at: CGPoint(x: bounds.minX, y: bounds.minY), anchor: .topLeading, proposal: ProposedViewSize(s[0]))
            fields.place(
                at: CGPoint(x: bounds.minX, y: bounds.minY + s[0].height + lineSpacing), anchor: .topLeading,
                proposal: ProposedViewSize(width: bounds.width, height: fs.height))
        }
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
        let w = proposal.width.flatMap { $0.isFinite ? $0 : nil } ?? fit
        return CGSize(width: w, height: 22)
    }
}

/// The last failure of an editor action (spec §5: non-modal).
struct StatusLine: View {
    let editor: EditorModel

    var body: some View {
        if let s = editor.status {
            HStack {
                AppIcon.warning.image.foregroundStyle(.orange)
                Text(s).lineLimit(1)
                Spacer()
                Button("Dismiss") { editor.clearStatus() }.buttonStyle(.borderless)
            }
            .font(.caption)
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
