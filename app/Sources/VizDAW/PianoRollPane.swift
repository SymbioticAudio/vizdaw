import SwiftUI
import VizDAWAppModel
import VizDAWCore
import VizDAWEditors

/// The piano roll (spec §4b) with its bar.
struct PianoRollEditor: View {
    let model: PianoRollModel

    var body: some View {
        VStack(spacing: 0) {
            PianoRollBar(model: model, editor: model.editor)
            Divider()
            PianoRollHost(model: model)
        }
    }
}

struct PianoRollHost: NSViewRepresentable {
    let model: PianoRollModel

    func makeNSView(context: Context) -> PianoRollContainer { PianoRollContainer(model: model) }

    func updateNSView(_ view: PianoRollContainer, context: Context) {}
}

/// Tool, keyboard, shown lanes, edit lane, panes, row height and zoom.
struct PianoRollBar: View {
    let model: PianoRollModel
    @Bindable var editor: EditorModel

    private func setting(_ key: WritableKeyPath<PianoRollSettings, Bool>) -> Binding<Bool> {
        Binding(
            get: { editor.layout.pianoRoll[keyPath: key] },
            set: { on in editor.updateLayout { $0.pianoRoll[keyPath: key] = on } })
    }

    private var editLaneName: String { model.editLane.flatMap { model.row($0)?.name } ?? "—" }

    var body: some View {
        HStack(spacing: 10) {
            Picker("Tool", selection: $editor.tool) {
                ForEach(Tool.allCases, id: \.self) { Text($0.title).tag($0) }
            }
            .pickerStyle(.segmented)
            .labelsHidden()
            .frame(width: 210)
            Toggle("Keyboard", isOn: setting(\.showKeyboard))
            Menu("Lanes") {
                ForEach(model.sequenceLanes, id: \.id) { l in
                    Toggle(l.name, isOn: Binding(get: { model.isShown(l.id) }, set: { _ in model.toggleShown(l.id) }))
                        .disabled(l.id == model.target?.lane)
                }
            }
            .frame(width: 80)
            .help("Lanes shown in this piano roll")
            Menu("Insert Into: \(editLaneName)") {
                ForEach(model.shownLanes, id: \.self) { key in
                    Button(model.row(key)?.name ?? "Lane") { model.chosenLane = key }
                }
            }
            .frame(width: 190)
            .help("The lane the pencil adds notes to")
            Toggle("Exact", isOn: $editor.exactMode)
                .help("Exact mode: a drag opens Select and Modify with its amounts (⌃⌘E)")
            Toggle("Audio", isOn: setting(\.showRegions))
            Toggle("Expression", isOn: setting(\.showExpression))
            Spacer()
            Button {
                editor.updateLayout { $0.pianoRoll.rowHeight = max($0.pianoRoll.rowHeight - 2, 4) }
            } label: {
                Image(systemName: "arrow.down.right.and.arrow.up.left")
            }
            .help("Shorter Rows")
            Button {
                editor.updateLayout { $0.pianoRoll.rowHeight = min($0.pianoRoll.rowHeight + 2, 40) }
            } label: {
                Image(systemName: "arrow.up.left.and.arrow.down.right")
            }
            .help("Taller Rows")
            Button {
                editor.updateLayout { $0.pianoRoll.ticksPerPoint = min($0.pianoRoll.ticksPerPoint * 1.5, 61_440) }
            } label: {
                Image(systemName: "minus.magnifyingglass")
            }
            .help("Zoom Out")
            Button {
                editor.updateLayout { $0.pianoRoll.ticksPerPoint = max($0.pianoRoll.ticksPerPoint / 1.5, 2) }
            } label: {
                Image(systemName: "plus.magnifyingglass")
            }
            .help("Zoom In")
        }
        .padding(.horizontal, 8)
        .padding(.vertical, 4)
    }
}
