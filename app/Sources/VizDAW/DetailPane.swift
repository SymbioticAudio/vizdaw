import SwiftUI
import VizDAWAppModel

/// A detail editor's frame: the editor picker, its target, Open in Window,
/// and the editor: the piano roll (4b), the list (4c), else a placeholder.
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

    var body: some View {
        VStack(spacing: 0) {
            HStack {
                Picker("Editor", selection: Binding(get: { state.kind }, set: { onKind($0) })) {
                    ForEach(DetailEditorKind.allCases, id: \.self) { Text($0.title).tag($0) }
                }
                .pickerStyle(.segmented)
                .labelsHidden()
                .frame(width: 380)
                Text(model.editor.title(state.target, sequences: model.snapshot.sequences))
                    .font(.headline)
                    .lineLimit(1)
                Spacer()
                if let onTearOff {
                    Button("Open in Window", action: onTearOff)
                        .help("⌥⌘\(state.kind.key)")
                }
                if let onClose {
                    Button(action: onClose) { Image(systemName: "xmark") }
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
            } else {
                ContentUnavailableView(
                    "\(state.kind.title) arrives in plan \(state.kind.plan)",
                    systemImage: "rectangle.and.pencil.and.ellipsis",
                    description: Text(summary))
            }
        }
    }

    private var summary: String {
        guard let n = model.editor.eventCount(state.target) else {
            return "Showing the whole sequence."
        }
        return "\(n) event\(n == 1 ? "" : "s") in this \(state.target.lane == nil ? "sequence" : "selection")."
    }
}

/// The lower half of the document window (spec §3.7).
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
                onClose: { model.editor.closeDetail() })
        } else {
            ContentUnavailableView(
                "No Editor Open", systemImage: "rectangle.split.1x2",
                description: Text("Double-click a block, or press ⌘1 to ⌘4."))
        }
    }
}

/// Arrangement over the split detail pane.
struct EditorSplit: View {
    let model: DocumentModel
    let docID: UUID

    var body: some View {
        VSplitView {
            ArrangementPane(model: model)
                .frame(minHeight: 160, idealHeight: 380)
            DetailArea(model: model, docID: docID)
                .frame(minHeight: 120, idealHeight: 220)
        }
    }
}
