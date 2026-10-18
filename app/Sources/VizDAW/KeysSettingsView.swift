import AppKit
import SwiftUI
import VizDAWAppModel
import VizDAWCore
import VizDAWEditors

/// Settings > Keys (plan 4f Ruling 17): every command by menu with its
/// keys; record a new key, reset one or all, save and load a bindings
/// file, and keep the number row for locators.
struct KeysSettingsView: View {
    @Bindable var keys: KeyBindings

    private var groups: [(String, [BindingRow])] {
        var order: [String] = []
        var by: [String: [BindingRow]] = [:]
        for r in keys.rows {
            if by[r.menu] == nil { order.append(r.menu) }
            by[r.menu, default: []].append(r)
        }
        return order.map { ($0, by[$0] ?? []) }
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            Toggle("Number keys always go to locators (step entry uses the keypad)", isOn: $keys.reserveNumberRow)
            if let m = keys.message {
                HStack {
                    Text(m).font(.caption).foregroundStyle(.secondary).lineLimit(3)
                    if keys.pending != nil {
                        Button("Replace") { keys.confirmReplace() }
                        Button("Cancel") { keys.cancelReplace() }
                    }
                }
            }
            List {
                ForEach(groups, id: \.0) { group in
                    Section(group.0) {
                        ForEach(group.1, id: \.id) { row in
                            HStack {
                                Text(row.title)
                                if row.stepEntry {
                                    Text("step entry").font(.caption2).foregroundStyle(.secondary)
                                }
                                Spacer()
                                Text(row.keys.joined(separator: "  "))
                                    .font(.system(.body, design: .monospaced))
                                    .foregroundStyle(row.keys == row.defaults ? .primary : Color.accentColor)
                                KeyRecorder(title: row.title) { keys.record(row.id, key: $0) }
                                    .frame(width: 96, height: 20)
                                Button("Reset") { keys.reset(row.id) }
                                    .disabled(row.keys == row.defaults)
                            }
                        }
                    }
                }
            }
            HStack {
                Button("Save…") { save() }
                Button("Load…") { load() }
                Spacer()
                Button("Reset All") { keys.resetAll() }
            }
        }
        .padding()
        .frame(width: 620, height: 520)
    }

    private func save() {
        let panel = NSSavePanel()
        panel.nameFieldStringValue = "Key Bindings.vizkeys"
        guard panel.runModal() == .OK, let path = panel.url?.path else { return }
        do {
            try keys.save(to: path)
        } catch {
            keys.message = AlertItem(error, title: "").message
        }
    }

    private func load() {
        let panel = NSOpenPanel()
        panel.allowsMultipleSelection = false
        panel.canChooseDirectories = false
        guard panel.runModal() == .OK, let path = panel.url?.path else { return }
        try? keys.load(from: path)
    }
}

/// The AppKit key recorder in a row.
struct KeyRecorder: NSViewRepresentable {
    let title: String
    let onKey: (String) -> Void

    func makeNSView(context: Context) -> KeyRecorderView {
        let v = KeyRecorderView()
        v.title = title
        v.onKey = onKey
        return v
    }

    func updateNSView(_ v: KeyRecorderView, context: Context) {
        v.title = title
        v.onKey = onKey
    }
}
