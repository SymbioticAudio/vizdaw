import AppKit
import SwiftUI
import VizDAWAppModel
import VizDAWCore
import VizDAWEditors

/// The arrangement with its tool bar and status line.
struct ArrangementPane: View {
    let model: DocumentModel

    var body: some View {
        VStack(spacing: 0) {
            ArrangementBar(model: model, editor: model.editor)
            Divider()
            if model.selectedSeq == nil {
                ContentUnavailableView(
                    "No Sequence", systemImage: "music.note.list",
                    description: Text("Choose File > Create Demo Project to try VizDAW."))
            } else {
                ArrangementHost(document: model)
            }
            StatusLine(editor: model.editor)
        }
    }
}

struct ArrangementHost: NSViewRepresentable {
    let document: DocumentModel

    func makeNSView(context: Context) -> ArrangementContainer {
        ArrangementContainer(document: document)
    }

    func updateNSView(_ view: ArrangementContainer, context: Context) {}
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
                Image(systemName: "minus.magnifyingglass")
            }
            .help("Zoom Out")
            Button {
                editor.updateLayout { $0.ticksPerPoint = max($0.ticksPerPoint / 1.5, 2) }
            } label: {
                Image(systemName: "plus.magnifyingglass")
            }
            .help("Zoom In")
        }
        .padding(.horizontal, 8)
        .padding(.vertical, 4)
    }
}

/// The last failure of an editor action (spec §5: non-modal).
struct StatusLine: View {
    let editor: EditorModel

    var body: some View {
        if let s = editor.status {
            HStack {
                Image(systemName: "exclamationmark.triangle").foregroundStyle(.orange)
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
