import SwiftUI
import VizDAWAppModel

/// One window: shows its document (adopting a queued one or making an
/// empty one when it has none, or only a stale id), polls the transport
/// while key or playing and opens windows other code requested.
struct DocumentWindow: View {
    @Binding var id: UUID?
    let store: DocumentStore
    @Environment(\.openWindow) private var openWindow
    @Environment(\.controlActiveState) private var activeState

    var body: some View {
        Group {
            if let id, let model = store.model(id) {
                DocumentView(model: model, docID: id)
                    .focusedSceneValue(\.documentModel, model)
                    .focusedSceneValue(\.documentID, id)
                    .task(id: PollKey(key: activeState == .key, playing: model.isPlaying)) {
                        // Background playback keeps polling so device
                        // events are handled (O27).
                        guard activeState == .key || model.isPlaying else { return }
                        while !Task.isCancelled {
                            model.poll()
                            try? await Task.sleep(for: model.pollInterval)
                        }
                    }
                    .onAppear {
                        SmokeRun.start(model) { openWindow(id: "mixer", value: id) }
                        PluginWindows.reopen(store, doc: id)
                        EditorWindows.reopen(store, doc: id)
                    }
                    .onDisappear { store.close(id) }
            } else {
                ProgressView()
                    .onAppear { adopt() }
            }
        }
        .onAppear {
            adopt()
            openQueued()
        }
        .onChange(of: store.pending) { openQueued() }
        .onChange(of: activeState) { openQueued() }
    }

    /// Gives the window a live document when it has none (new window,
    /// stale restored id, or its document was closed).
    private func adopt() {
        guard id.flatMap(store.model) == nil else { return }
        id = store.documentForWindow(id)
    }

    /// Opens a window for every queued document (only the key window
    /// does it, so each opens once).
    private func openQueued() {
        guard activeState == .key || store.documents.count <= 1 else { return }
        while let next = store.takePending() {
            openWindow(value: next)
        }
    }
}

private struct PollKey: Equatable {
    let key: Bool
    let playing: Bool
}
