import SwiftUI
import VizDAWAppModel
import VizDAWCore
import VizDAWEditors

/// The offline/relink dialog (spec §10.3; plan 5b Ruling 14): the items
/// that are missing or changed, each with Locate…, plus Search Folder…
/// and Leave Offline. Relinks run as jobs; the window's banners show them.
struct RelinkSheet: View {
    @Bindable var media: MediaModel

    var body: some View {
        VStack(alignment: .leading, spacing: 10) {
            Text("Some audio files of this project are missing or changed")
                .font(.headline)
            Text("Their regions stay silent until each item is relinked to its file. Each relink is one undo step.")
                .font(.callout)
                .foregroundStyle(.secondary)
            List(media.relinkable, id: \.id) { r in
                HStack(spacing: 8) {
                    Image(systemName: "exclamationmark.triangle")
                        .foregroundStyle(Color(nsColor: RegionPalette.offline))
                        .accessibilityHidden(true)
                    VStack(alignment: .leading, spacing: 2) {
                        Text(r.name)
                        Text("\(r.offline.map(MediaModel.reasonText) ?? "") · \(r.path)")
                            .font(.caption)
                            .foregroundStyle(.secondary)
                    }
                    Spacer()
                    Button("Locate…") {
                        if let p = Panels.chooseRelink(name: r.name) { media.relink(r.id, path: p) }
                    }
                    .accessibilityLabel("Locate \(r.name)")
                }
                .accessibilityElement(children: .contain)
            }
            .frame(minHeight: 160)
            HStack {
                Button("Search Folder…") {
                    if let d = Panels.chooseFolder() { media.searchFolder(d) }
                }
                Spacer()
                Button("Leave Offline") { media.leaveOffline() }
                    .keyboardShortcut(.cancelAction)
            }
        }
        .padding(16)
        .frame(width: 540)
    }
}
