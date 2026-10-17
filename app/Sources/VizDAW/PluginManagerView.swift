import AppKit
import SwiftUI
import VizDAWAppModel
import VizDAWCore

/// The Plugin Manager window (spec §9).
struct PluginManagerView: View {
    @Bindable var model: PluginManagerModel

    var body: some View {
        VStack(spacing: 0) {
        Table(model.rows, selection: $model.selection) {
            TableColumn("On") { r in
                Toggle("Enabled", isOn: Binding(get: { r.isEnabled }, set: { model.setEnabled(r, $0) }))
                    .labelsHidden()
                    .disabled(!r.hasPlugin)
            }
            .width(30)
            TableColumn("Name", value: \.name)
            TableColumn("Vendor", value: \.vendor)
            TableColumn("Format", value: \.formatText).width(50)
            TableColumn("Version", value: \.version).width(70)
            TableColumn("Type", value: \.kindText).width(80)
            TableColumn("Status") { r in
                Text(r.statusText)
                    .foregroundStyle(r.isBlocklisted ? Color.red : (r.row.status == .ok ? Color.primary : Color.orange))
                    .help(r.row.path)
            }
        }
        Text("Turning a plugin off unloads it from all open projects; its settings are kept.")
            .font(.caption).foregroundStyle(.secondary)
            .padding(6)
        }
        .searchable(text: $model.search)
        .toolbar {
            ToolbarItemGroup {
                Picker("Format", selection: $model.format) {
                    ForEach(PluginManagerModel.FormatFilter.allCases) { Text($0.rawValue).tag($0) }
                }
                .pickerStyle(.segmented)
                Text(model.progressText).font(.caption).foregroundStyle(.secondary)
                Button("Rescan Selected") { model.rescanSelected() }
                    .disabled(!model.canRescan)
                Button("Rescan All") { model.rescanAll() }
                    .disabled(model.plugins.isScanning)
                Button("Clear Blocklist") { model.clearBlocklist() }
                    .disabled(!model.canClearBlocklist)
                Button("Reveal in Finder") {
                    if let p = model.revealPath {
                        NSWorkspace.shared.activateFileViewerSelecting([URL(fileURLWithPath: p)])
                    }
                }
                .disabled(model.revealPath == nil)
            }
        }
        .frame(minWidth: 760, minHeight: 360)
    }
}
