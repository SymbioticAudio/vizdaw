import SwiftUI
import VizDAWAppModel
import VizDAWCore

/// The plugin browser popover (spec §9): search, plugins grouped by
/// vendor; choosing one calls `pick`.
struct PluginBrowserView: View {
    @Bindable var model: PluginBrowserModel
    let pick: (PluginRefRow) -> Void

    var body: some View {
        VStack(alignment: .leading, spacing: 6) {
            TextField("Search", text: $model.search)
                .textFieldStyle(.roundedBorder)
            let groups = model.groups
            if groups.isEmpty {
                Text(model.search.isEmpty ? "No plugins found. Scan in Window > Plugin Manager." : "No match.")
                    .foregroundStyle(.secondary)
                    .frame(maxWidth: .infinity, minHeight: 80)
            } else {
                List {
                    ForEach(groups) { g in
                        Section(g.vendor) {
                            ForEach(g.plugins, id: \.self) { p in
                                Button {
                                    pick(p)
                                } label: {
                                    HStack {
                                        Text(p.name)
                                        Spacer()
                                        Text(p.format == .au ? "AU" : "CLAP")
                                            .font(.caption)
                                            .foregroundStyle(.secondary)
                                    }
                                }
                                .buttonStyle(.plain)
                            }
                        }
                    }
                }
                .frame(minHeight: 240)
            }
        }
        .padding(10)
        .frame(width: 320)
    }
}
