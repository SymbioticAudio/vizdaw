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
                    description: Text("Add a sequence to arrange tracks in.")
                ) {
                    Button("New Sequence") { model.newSequence(.empty) }
                }
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
    @Environment(\.symbioticTheme) private var theme
    let model: DocumentModel
    @Bindable var editor: EditorModel

    var body: some View {
        HStack(spacing: theme.spacing.m) {
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
        .padding(.horizontal, theme.spacing.s)
        .padding(.vertical, theme.spacing.xs)
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
            .padding(.horizontal, theme.spacing.s)
            .padding(.vertical, theme.spacing.xs)
            .background(.bar)
            .task(id: s) {
                try? await Task.sleep(for: .seconds(6))
                if editor.status == s { editor.clearStatus() }
            }
        }
    }
}
