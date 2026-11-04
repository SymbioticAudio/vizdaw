import SwiftUI
import SymbioticIcons
import VizDAWAppModel

/// A detail editor's frame: the editor picker, its target, Open in Window,
/// and the editor: the piano roll (4b), the list (4c), the Strip Chart
/// (4d) or the drum grid (4e).
struct DetailPane: View {
    let model: DocumentModel
    let state: DetailState
    /// This frame's piano roll (the pane's or the torn window's).
    let roll: PianoRollModel
    /// This frame's slot: its list editor is made only while it shows a
    /// list (one made otherwise would linger until the next layout change).
    let slot: DetailSlot
    let onKind: (DetailEditorKind) -> Void
    let onTearOff: (() -> Void)?
    let onClose: (() -> Void)?
    /// The kind picker: torn windows show it; the bottom pane leaves it to
    /// its pane picker (plan 7a Ruling 3).
    var showsKindPicker = true

    var body: some View {
        VStack(spacing: 0) {
            HStack {
                if showsKindPicker {
                    Picker("Editor", selection: Binding(get: { state.kind }, set: { onKind($0) })) {
                        ForEach(DetailEditorKind.allCases, id: \.self) { Text($0.title).tag($0) }
                    }
                    .pickerStyle(.segmented)
                    .labelsHidden()
                    .frame(width: 380)
                }
                Text(model.editor.title(state.target, sequences: model.snapshot.sequences))
                    .font(.headline)
                    .lineLimit(1)
                Spacer()
                if let onTearOff {
                    Button("Open in Window", action: onTearOff)
                        .help("⌥⌘\(state.kind.key)")
                }
                if let onClose {
                    Button(action: onClose) { Icon(AppIcon.close.role, size: 16) }
                        .buttonStyle(.borderless)
                        .help("Close Detail Editor")
                }
            }
            .padding(.horizontal, 8)
            .padding(.vertical, 4)
            Divider()
            if state.kind == .pianoRoll {
                PianoRollEditor(model: roll)
            } else if state.kind == .list {
                ListEditorPane(model: model.list(slot))
            } else if state.kind == .stripChart {
                StripChartEditor(model: model.stripChart(slot))
            } else {
                DrumGridEditor(model: model.drumGrid(slot))
            }
        }
    }
}

/// The bottom pane's editor (spec §3.7; plan 7a Ruling 3).
struct DetailArea: View {
    let model: DocumentModel
    let docID: UUID

    var body: some View {
        if let d = model.editor.layout.detail {
            DetailPane(
                model: model, state: d, roll: model.pianoRoll(.pane), slot: .pane,
                onKind: { k in model.editor.openDetail(d.target, kind: k) },
                onTearOff: {
                    EditorWindows.tearOff(appStore, doc: docID, kind: d.kind)
                    model.editor.closeDetail()
                },
                onClose: { model.editor.closeDetail() }, showsKindPicker: false)
        } else {
            ContentUnavailableView {
                Label { Text("No Editor Open") } icon: { Icon(AppIcon.split.role, size: 32) }
            } description: {
                Text("Double-click a block, or press ⌘1 to ⌘4.")
            }
            .frame(maxWidth: .infinity, maxHeight: .infinity)
        }
    }
}
