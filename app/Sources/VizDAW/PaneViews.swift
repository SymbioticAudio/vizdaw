import SwiftUI
import VizDAWAppModel
import VizDAWCore

/// A placeholder's icon size: about what an SF Symbol drew there before.
private let placeholderIcon: CGFloat = 40

/// A placeholder with an AppIcon (final fix wave B): every icon is a
/// SymbioticAudio one, never an SF Symbol.
extension ContentUnavailableView where Label == SwiftUI.Label<Text, Image>, Actions == EmptyView {
    @MainActor init(_ title: String, icon: AppIcon, description: Description) where Description == Text {
        let label = icon.label(title, size: placeholderIcon)
        self.init {
            label
        } description: {
            description
        }
    }

    @MainActor init(_ title: String, icon: AppIcon) where Description == EmptyView {
        let label = icon.label(title, size: placeholderIcon)
        self.init {
            label
        }
    }
}

/// What re-runs a 30 Hz poll (the document window's, the left strip's, the
/// Mix view's and the bottom pane's mixer): its window turning key, or the
/// document starting or stopping play or record (plan 7a Ruling 8, Task 8
/// review M2: one type for all of them).
struct PollKey: Equatable {
    let key: Bool
    let playing: Bool
}

/// The left pane's "Channel Strip" (plan 7a Rulings 5 and 8): the strip the
/// selected lane plays through, the inspector when it has none, or a note.
struct LeftStripPane: View {
    let model: DocumentModel
    let docID: UUID
    @Environment(\.controlActiveState) var activeState

    var body: some View {
        if let mixer = appStore.mixer(docID) {
            content(mixer)
                .environment(\.mixerPlacement, .strip)
                .sheet(isPresented: mapsShown) { LearnMapsSheet(learn: model.learn) }
                .onAppear { mixer.appeared(.strip) }
                .onDisappear {
                    // The maps sheet goes with this placement; a ride held
                    // in another placement goes on (Ruling 8).
                    model.learn.closeMaps(from: .strip)
                    mixer.disappeared(.strip)
                }
                .task(id: PollKey(key: activeState == .key, playing: model.isPlaying || model.record.isRecording)) {
                    // A background document does not read levels at 30 Hz.
                    guard activeState == .key || model.isPlaying || model.record.isRecording else { return }
                    while !Task.isCancelled {
                        mixer.tick(now: ProcessInfo.processInfo.systemUptime)
                        try? await Task.sleep(for: .milliseconds(33))
                    }
                }
        } else {
            ContentUnavailableView(
                "Mixer Unavailable", icon: .mixer,
                description: Text("The mixer of this document could not be read."))
        }
    }

    @ViewBuilder
    private func content(_ mixer: MixerModel) -> some View {
        let strips = mixer.snapshot.strips
        switch model.leftStrip(strips: strips, destinations: mixer.destinations) {
        case .strip(let id):
            if let strip = strips.first(where: { $0.id == id }) {
                // The strip scrolls itself when the pane is too short
                // (Ruling 14): it needs the pane's height for its throw.
                GeometryReader { g in
                    ChannelStrip(model: mixer, strip: strip, height: max(g.size.height - 16, 0))
                        .padding(8)
                        .frame(maxWidth: .infinity)
                }
            } else {
                PaneNote(text: StripFocus.nothingMessage)
            }
        case .inspector:
            InspectorPane(model: model, docID: docID)
        case .nothing:
            PaneNote(text: StripFocus.nothingMessage)
        }
    }

    /// The MIDI Learn Maps sheet, when this placement opened it.
    private var mapsShown: Binding<Bool> {
        let learn = model.learn
        return Binding(get: { learn.showsMaps(in: .strip) }, set: { if !$0 { learn.closeMaps(from: .strip) } })
    }
}

/// The left pane's read-only inspector (Ruling 6): the followed lane, then
/// the single selected audio region.
struct InspectorPane: View {
    let model: DocumentModel
    let docID: UUID

    var body: some View {
        let mixer = appStore.mixer(docID)
        let rows = model.inspectorRows(
            destinations: mixer?.destinations ?? [], strips: mixer?.snapshot.strips ?? [])
        if rows.isEmpty {
            PaneNote(text: Inspector.emptyMessage)
        } else {
            ScrollView(.vertical) {
                Grid(alignment: .leadingFirstTextBaseline, horizontalSpacing: 8, verticalSpacing: 4) {
                    ForEach(Array(rows.enumerated()), id: \.offset) { _, r in
                        GridRow {
                            Text(r.label)
                                .foregroundStyle(.secondary)
                                .gridColumnAlignment(.trailing)
                            Text(r.value)
                                .lineLimit(1)
                                .truncationMode(.middle)
                                .textSelection(.enabled)
                        }
                    }
                }
                .padding(8)
                .frame(maxWidth: .infinity, alignment: .leading)
            }
        }
    }
}

/// The right pane's markers and locators (Ruling 7): a click moves the
/// playhead there.
struct MarkersPane: View {
    @Environment(\.symbioticTheme) private var theme
    let model: DocumentModel

    var body: some View {
        let rows = MarkerList.rows(
            info: model.selectedSeq.flatMap { model.editor.info($0) }, marks: model.editor.layout.transport)
        if rows.isEmpty {
            PaneNote(text: "No markers or locators.")
        } else {
            List(rows) { row in
                Button {
                    MarkerList.go(row, model)
                } label: {
                    HStack {
                        Text(row.name).lineLimit(1)
                        Spacer(minLength: 6)
                        Text(row.position)
                            .font(theme.type.caption.font.monospacedDigit())
                            .foregroundStyle(.secondary)
                    }
                    .contentShape(Rectangle())
                }
                .buttonStyle(.plain)
                .help("Go to \(row.name)")
            }
        }
    }
}

/// The right pane's Strip Chart of the followed lane (Ruling 7).
struct ControllersPane: View {
    let model: DocumentModel

    var body: some View {
        let chart = model.controllersChart
        if chart.state != nil {
            StripChartEditor(model: chart)
        } else {
            PaneNote(text: "Select a lane to see its controllers.")
        }
    }
}

/// The right pane's effect browser (Ruling 7): a pick goes on the end of the
/// strip the view points at.
struct PluginsPane: View {
    let model: DocumentModel
    let docID: UUID
    /// Its own browser, never `mixer.browserModel(.effect)`, whose cache
    /// `pick` clears. (`State` spelled out: the Command Line Tools cannot
    /// expand `@State`.)
    var browser = State(initialValue: PluginBrowserModel(kind: .effect) { appPlugins.browser(.effect) })

    var body: some View {
        PluginBrowserView(model: browser.wrappedValue, width: nil) { p in
            guard let mixer = appStore.mixer(docID) else {
                model.editor.status = "The mixer of this document could not be read."
                return
            }
            if let refused = PaneContent.insertPlugin(p, document: model, mixer: mixer) {
                model.editor.status = refused
            }
        }
    }
}

/// A pane's one-line note when it has nothing to show.
private struct PaneNote: View {
    let text: String

    var body: some View {
        Text(text)
            .foregroundStyle(.secondary)
            .multilineTextAlignment(.center)
            .padding(12)
            .frame(maxWidth: .infinity, maxHeight: .infinity)
    }
}
