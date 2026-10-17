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
    /// This window's focus token (the timer's note target).
    /// (`State` spelled out: the Command Line Tools cannot expand `@State`.)
    private var focusToken = State(initialValue: UUID())

    /// Spelled out: a private stored property makes the memberwise
    /// initializer private under Xcode's compiler.
    init(id: Binding<UUID?>, store: DocumentStore) {
        _id = id
        self.store = store
    }

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
                        model.selectModify.onPresent = { ModifyPanels.show(store, doc: id) }
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
        .onChange(of: KeyDoc(id: id, key: activeState == .key), initial: true) { old, new in
            KeyDoc.changed(store, window: focusToken.wrappedValue, from: old, to: new)
        }
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

/// Whether a document's own window (document or mixer) is key: the
/// timer's note target, since the timer has no SwiftUI focus.
struct KeyDoc: Equatable {
    let id: UUID?
    let key: Bool

    @MainActor static func changed(_ store: DocumentStore, window: UUID, from old: KeyDoc, to new: KeyDoc) {
        if let o = old.id, old.key, !new.key || new.id != o { store.blurDocument(o, window: window) }
        if let n = new.id, new.key { store.focusDocument(n, window: window) }
    }
}

private struct PollKey: Equatable {
    let key: Bool
    let playing: Bool
}
