import SwiftUI
import VizDAWAppModel

/// One window: adopts a queued document or makes an empty one, shows it,
/// polls the transport while key and opens windows other code requested.
struct DocumentWindow: View {
    @Binding var id: UUID?
    let store: DocumentStore
    @Environment(\.openWindow) private var openWindow
    @Environment(\.controlActiveState) private var activeState

    var body: some View {
        Group {
            if let id, let model = store.model(id) {
                DocumentView(model: model)
                    .focusedSceneValue(\.documentModel, model)
                    .task(id: PollKey(key: activeState == .key, playing: model.isPlaying)) {
                        guard activeState == .key else { return }
                        while !Task.isCancelled {
                            model.poll()
                            try? await Task.sleep(for: model.pollInterval)
                        }
                    }
                    .onAppear { SmokeRun.start(model) }
                    .onDisappear { store.close(id) }
            } else {
                ProgressView()
            }
        }
        .onAppear {
            if id == nil {
                id = store.takePending() ?? store.newEmpty()
            }
            openQueued()
        }
        .onChange(of: store.pending) { openQueued() }
        .alert(
            store.alert?.title ?? "",
            isPresented: Binding(
                get: { store.alert != nil && activeState == .key },
                set: { if !$0 { store.alert = nil } }),
            presenting: store.alert
        ) { _ in
            Button("OK") { store.alert = nil }
        } message: { item in
            Text(item.message)
        }
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
