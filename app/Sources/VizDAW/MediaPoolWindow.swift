import AppKit
import SwiftUI
import VizDAWAppModel
import VizDAWCore
import VizDAWEditors

/// The media pool window of document `id` (spec §10.3).
struct MediaPoolWindow: View {
    @Binding var id: UUID?
    let store: DocumentStore

    /// Spelled out: a private stored property makes the memberwise
    /// initializer private under Xcode's compiler.
    init(id: Binding<UUID?>, store: DocumentStore) {
        _id = id
        self.store = store
    }

    var body: some View {
        content.background(WindowTag(kind: .mediaPool, doc: id))
    }

    @ViewBuilder private var content: some View {
        if let id, let doc = store.model(id) {
            MediaPoolView(media: doc.media)
                .focusedSceneValue(\.documentModel, doc)
                .focusedSceneValue(\.documentID, id)
                .navigationTitle("Media Pool — \(doc.snapshot.title)")
        } else {
            ContentUnavailableView(
                "No Document", systemImage: "waveform",
                description: Text("The document of this media pool is closed."))
        }
    }
}

/// The pool's table and actions (rows and actions are `MediaModel`'s).
struct MediaPoolView: View {
    @Bindable var media: MediaModel

    private var selected: UInt64? { media.selection.count == 1 ? media.selection.first : nil }

    var body: some View {
        VStack(spacing: 0) {
            Table(media.poolRows, selection: $media.selection) {
                TableColumn("Name", value: \.name)
                TableColumn("Duration", value: \.duration).width(min: 60, ideal: 72)
                TableColumn("Channels", value: \.channels).width(min: 56, ideal: 64)
                TableColumn("Origin", value: \.origin).width(min: 64, ideal: 88)
                TableColumn("Uses") { r in Text("\(r.uses)").monospacedDigit() }.width(min: 36, ideal: 44)
                TableColumn("Used", value: \.used).width(min: 44, ideal: 52)
                TableColumn("Offline") { r in
                    if r.offline.isEmpty {
                        Text("")
                    } else {
                        Label(r.offline, systemImage: "exclamationmark.triangle")
                            .foregroundStyle(Color(nsColor: RegionPalette.offline))
                            .accessibilityLabel("Offline: \(r.offline)")
                    }
                }
            }
            .contextMenu(forSelectionType: UInt64.self) { ids in
                if let id = ids.first, ids.count == 1 { rowMenu(id) }
            } primaryAction: { ids in
                if let id = ids.first { media.selectUsers(id) }
            }
            // The Delete key (and Edit > Delete) deletes the selected item,
            // asking first as the button does (Task 12 review M3).
            .onDeleteCommand { if let id = selected { delete(id) } }
            .accessibilityLabel("Media items")
            Divider()
            HStack(spacing: 8) {
                Button("Select Regions") { if let id = selected { media.selectUsers(id) } }
                    .disabled(selected == nil)
                Button("Select Unused") { media.selectUnused() }
                Button("Compact") { media.compact() }
                    .help("Remove every item no region uses (one undo step). Their files go to the Trash on save.")
                Spacer()
                Button("Rename…") { if let id = selected { rename(id) } }.disabled(selected == nil)
                Button("Relink…") { if let id = selected { relink(id) } }.disabled(selected == nil)
                Button("Reveal in Finder") { if let id = selected { reveal(id) } }.disabled(selected == nil)
                Button("Import…") { media.importIntoPool(Panels.chooseAudio()) }
                Button("Delete", role: .destructive) { if let id = selected { delete(id) } }
                    .disabled(selected == nil)
            }
            .padding(8)
        }
        .frame(minWidth: 620, minHeight: 280)
    }

    @ViewBuilder private func rowMenu(_ id: UInt64) -> some View {
        Button("Select Regions") { media.selectUsers(id) }
        Button("Rename…") { rename(id) }
        Button("Relink…") { relink(id) }
        Button("Reveal in Finder") { reveal(id) }
        Divider()
        Button("Delete", role: .destructive) { delete(id) }
    }

    private func rename(_ id: UInt64) {
        guard let r = media.row(id),
            let name = TextPanel.run(
                title: "Rename Media", message: "A new name for “\(r.name)”:", initial: r.name, ok: "Rename",
                label: "Media name")
        else { return }
        media.rename(id, to: name)
    }

    private func relink(_ id: UInt64) {
        guard let r = media.row(id), let path = Panels.chooseRelink(name: r.name) else { return }
        media.relink(id, path: path)
    }

    private func reveal(_ id: UInt64) {
        guard let f = media.file(id) else { return }
        NSWorkspace.shared.activateFileViewerSelecting([URL(fileURLWithPath: f)])
    }

    private func delete(_ id: UInt64) {
        if let q = media.deleteQuestion(id) {
            guard ConfirmPanel.run(title: "Delete Media", message: q, ok: "Delete") else { return }
        }
        media.remove(id)
    }
}
