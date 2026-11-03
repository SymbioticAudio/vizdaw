import SwiftUI
import VizDAWAppModel
import VizDAWCore

/// Settings › Surfaces (plan 5f-1 Rulings 9–11): the configured units with
/// their connection state, Add and Remove, and the commands learned to
/// MIDI controls. All state lives in `SurfaceSettings`.
struct SurfacesSettingsView: View {
    @Bindable var settings: SurfaceSettings
    @Bindable var keys: KeyBindings

    private static let kinds: [SurfaceKindRow] = [.mcu, .mcuXt, .hui]

    var body: some View {
        Form {
            Section("Surfaces") {
                if settings.rows.isEmpty {
                    Text("No surfaces. Add an MCU-compatible, MCU XT-compatible or HUI-compatible unit.")
                        .foregroundStyle(.secondary)
                }
                ForEach(settings.rows, id: \.id) { row in
                    HStack {
                        VStack(alignment: .leading) {
                            Text(title(row))
                            Text("\(row.input) → \(row.output)")
                                .font(.caption)
                                .foregroundStyle(.secondary)
                        }
                        Spacer()
                        Text(SurfaceSettings.status(row))
                            .foregroundStyle(row.connected && row.online ? Color.primary : Color.secondary)
                        Button("Remove") { settings.remove(row.id) }
                    }
                }
                Button("Add Surface…") { settings.beginAdd() }
            }
            Section("Commands Learned to MIDI") {
                if settings.commandMaps.isEmpty {
                    Text("None. Learn MIDI for a command in Settings › Keys.")
                        .foregroundStyle(.secondary)
                }
                ForEach(settings.commandMaps, id: \.self) { map in
                    HStack {
                        Text(map.text)
                        Spacer()
                        Text(keys.row(map.command)?.title ?? map.command)
                            .foregroundStyle(.secondary)
                        Button("Remove") { settings.removeCommandMap(map.command) }
                    }
                }
            }
        }
        .formStyle(.grouped)
        .frame(width: 560)
        .padding()
        .task {
            // Connection states change on their own (Ruling 10's 2 s check).
            while !Task.isCancelled {
                settings.refresh()
                try? await Task.sleep(for: .milliseconds(500))
            }
        }
        .sheet(
            isPresented: Binding(get: { settings.draft != nil }, set: { if !$0 { settings.cancelAdd() } })
        ) {
            addSheet
        }
        .alert(
            settings.alert?.title ?? "",
            isPresented: Binding(get: { settings.alert != nil }, set: { if !$0 { settings.alert = nil } })
        ) {
            Button("OK") { settings.alert = nil }
        } message: {
            Text(settings.alert?.message ?? "")
        }
    }

    private func title(_ row: SurfaceRow) -> String {
        let t = SurfaceSettings.title(row.kind)
        return row.kind == .mcuXt ? "\(t) · position \(row.position)" : t
    }

    /// A binding to one field of the open sheet's draft.
    private func field<V>(_ path: WritableKeyPath<SurfaceDraft, V>, _ empty: V) -> Binding<V> {
        Binding(
            get: { settings.draft?[keyPath: path] ?? empty },
            set: { settings.draft?[keyPath: path] = $0 })
    }

    private var addSheet: some View {
        VStack(alignment: .leading, spacing: 12) {
            Text("Add Surface").font(.headline)
            Form {
                Picker("Type", selection: field(\.kind, .mcu)) {
                    ForEach(Self.kinds, id: \.self) { Text(SurfaceSettings.title($0)).tag($0) }
                }
                Picker("Input", selection: field(\.input, "")) {
                    if settings.ports.sources.isEmpty { Text("No MIDI sources").tag("") }
                    ForEach(settings.ports.sources, id: \.self) { Text($0).tag($0) }
                }
                Picker("Output", selection: field(\.output, "")) {
                    if settings.ports.destinations.isEmpty { Text("No MIDI destinations").tag("") }
                    ForEach(settings.ports.destinations, id: \.self) { Text($0).tag($0) }
                }
                if settings.draft?.kind == .mcuXt {
                    Stepper(
                        "Position: \(settings.draft?.position ?? 1)", value: field(\.position, 1), in: 0...3)
                    Text("Units are banked left to right by position; the MCU-compatible main unit takes a free one.")
                        .font(.caption)
                        .foregroundStyle(.secondary)
                }
            }
            if let problem = settings.draftProblem {
                Text(problem).foregroundStyle(.red)
            }
            HStack {
                Spacer()
                Button("Cancel", role: .cancel) { settings.cancelAdd() }
                    .keyboardShortcut(.cancelAction)
                Button("Add") { settings.commitAdd() }
                    .keyboardShortcut(.defaultAction)
                    .disabled((settings.draft?.input ?? "").isEmpty || (settings.draft?.output ?? "").isEmpty)
            }
        }
        .padding()
        .frame(width: 420)
    }
}
