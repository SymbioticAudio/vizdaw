import SwiftUI
import VizDAWAppModel
import VizDAWCore
import VizDAWEditors

/// The list editor (spec §4c) with its bar.
struct ListEditorPane: View {
    let model: ListEditorModel

    var body: some View {
        VStack(spacing: 0) {
            ListBar(model: model)
            Divider()
            // A new model (the slot's list was dropped and made again)
            // gets a new view: the view keeps the model it was made with.
            ListHost(model: model)
                .id(ObjectIdentifier(model))
        }
    }
}

struct ListHost: NSViewRepresentable {
    let model: ListEditorModel

    func makeNSView(context: Context) -> ListEditorView { ListEditorView(model: model) }

    func updateNSView(_ view: ListEditorView, context: Context) {}
}

/// Insert, columns, what is shown, and MIDI In (Plan 4c Rulings 14, 18, 19).
struct ListBar: View {
    @Bindable var model: ListEditorModel

    private func column(_ key: WritableKeyPath<ListColumns, Bool>) -> Binding<Bool> {
        Binding(
            get: { model.columnSettings[keyPath: key] },
            set: { on in
                var c = model.columnSettings
                c[keyPath: key] = on
                model.setColumns(c)
            })
    }

    private var fps: Binding<Int> {
        Binding(
            get: { model.columnSettings.fps },
            set: { f in
                var c = model.columnSettings
                c.fps = f
                model.setColumns(c)
            })
    }

    var body: some View {
        HStack(spacing: 10) {
            Menu {
                ForEach(model.insertKinds, id: \.self) { k in
                    Button(k.title) { model.insert(k) }
                }
            } label: {
                Text("Insert \(model.lastInsertKind.title)")
            } primaryAction: {
                model.insert()
            }
            .frame(width: 190)
            .help("Insert the last inserted kind (⌘I); the menu chooses another")
            Menu("Columns") {
                Toggle("End", isOn: column(\.end))
                Toggle("Length", isOn: column(\.duration))
                Toggle("Release Velocity", isOn: column(\.release))
                Toggle("SMPTE", isOn: column(\.smpte))
                Picker("SMPTE Frame Rate", selection: fps) {
                    ForEach(Smpte.rates, id: \.self) { Text("\($0) fps").tag($0) }
                }
            }
            .frame(width: 100)
            Menu("Show") {
                ForEach(ListKind.allCases, id: \.self) { k in
                    Toggle(
                        k.title,
                        isOn: Binding(get: { !model.filter.hiddenKinds.contains(k) }, set: { _ in model.toggle(k) }))
                }
                Divider()
                ForEach(model.destinations, id: \.id) { d in
                    Toggle(
                        d.name,
                        isOn: Binding(get: { model.shows(dest: d.id) }, set: { _ in model.toggle(dest: d.id) }))
                }
                Divider()
                Button("Show Everything") { model.setFilter(ListFilter()) }
                Button("Apply to All Lists") { model.applyFilterToAll() }
                Button("Save as Default") { model.saveFilterAsDefault() }
            }
            .frame(width: 80)
            .help("The kinds and destinations this list shows")
            Toggle("MIDI In", isOn: $model.midiIn)
                .help("Play a note to enter its pitch and velocity")
            Spacer()
            Text("\(model.rows.count) rows").foregroundStyle(.secondary).font(.caption)
        }
        .padding(.horizontal, 8)
        .padding(.vertical, 4)
    }
}
