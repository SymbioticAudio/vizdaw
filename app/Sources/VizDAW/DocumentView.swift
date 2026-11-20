import AppKit
import SwiftUI
import SymbioticTokens
import VizDAWAppModel
import VizDAWCore
import VizDAWEditors

/// One document's window: the shell (transport bar, layout A's panes,
/// status line) with the document's sheets and alerts (plan 7a Task 7).
struct DocumentView: View {
    @Bindable var model: DocumentModel
    let docID: UUID

    var body: some View {
        DocumentShell(model: model, docID: docID)
            .navigationTitle(model.windowTitle)
            .modifier(DocumentSheets(model: model, docID: docID))
            .modifier(InterchangeSheets(model: model, docID: docID))
            .modifier(DocumentAlerts(model: model))
            // The Takes menu's Rename… asks here (plan 5c-2 takes Ruling 5);
            // the header installs its own rename field for Rename Track.
            .onAppear { model.askTakeName = Self.askTakeName }
    }

    /// The Rename Take question; nil when cancelled.
    static func askTakeName(_ current: String) -> String? {
        TextPanel.run(
            title: "Rename Take", message: "A new name for “\(current)”:", initial: current, ok: "Rename",
            label: "Take name")
    }
}

/// The document's sheets, apart from `DocumentView.body` so the type
/// checker can handle the chain (one `.sheet` per sheet).
private struct DocumentSheets: ViewModifier {
    @Bindable var model: DocumentModel
    let docID: UUID

    func body(content: Content) -> some View {
        content
            .sheet(
                item: Binding(
                    get: { model.editor.referenceDraft }, set: { model.editor.referenceDraft = $0 })
            ) { _ in
                ReferenceSheet(editor: model.editor)
            }
            .sheet(
                isPresented: Binding(
                    get: { model.media.showsRelinkSheet(channelOn: ControlChannel.active) },
                    set: { if !$0 { model.media.leaveOffline() } })
            ) {
                RelinkSheet(media: model.media)
            }
            .sheet(
                isPresented: Binding(
                    get: { model.bounce.shown && !ControlChannel.active }, set: { model.bounce.shown = $0 })
            ) {
                BounceSheet(bounce: model.bounce)
            }
            .sheet(
                isPresented: Binding(
                    get: { model.forDaw.shown && !ControlChannel.active }, set: { model.forDaw.shown = $0 })
            ) {
                ForDawSheet(forDaw: model.forDaw, document: model)
            }
            .sheet(
                isPresented: Binding(
                    get: { model.dawproject.shown && !ControlChannel.active }, set: { model.dawproject.shown = $0 })
            ) {
                DawprojectSheet(dawproject: model.dawproject, document: model)
            }
            .sheet(
                isPresented: Binding(
                    get: { model.aaf.exportShown && !ControlChannel.active }, set: { model.aaf.exportShown = $0 })
            ) {
                AafExportSheet(aaf: model.aaf, document: model)
            }
            .sheet(
                isPresented: Binding(
                    get: { model.aaf.importShown && !ControlChannel.active }, set: { model.aaf.importShown = $0 })
            ) {
                AafImportSheet(aaf: model.aaf)
            }
            .sheet(
                item: Binding(get: { model.synth.trackSynth }, set: { if $0 == nil { model.synth.closeTrackSynth() } })
            ) { _ in
                TrackSynthSheet(
                    sheets: model.synth, editor: model.editor, channels: model.inputs.knownChannels ?? 256)
            }
            .sheet(
                item: Binding(
                    get: { model.synth.newDestination }, set: { if $0 == nil { model.synth.closeNewDestination() } })
            ) { _ in
                NewMidiDestinationSheet(sheets: model.synth, editor: model.editor)
            }
            // VizDAW's own approval of a Co-Producer change (plan 8b Ruling
            // 11): the one sheet a Co-Producer call may open, and only for an
            // apply waiting for a yes. Under the developer channel the e2e
            // answers it with `settings.co_producer_answer` instead.
            .sheet(
                item: Binding(
                    get: { ControlChannel.active ? nil : model.coProducerApproval },
                    set: { if $0 == nil { model.coProducerApproval?.deny() } })
            ) { approval in
                CoProducerApprovalSheet(model: approval)
            }
            .sheet(
                isPresented: Binding(
                    get: { model.synth.editDestination != nil },
                    set: { if !$0 { model.synth.closeEditDestination() } })
            ) {
                DestinationEditorSheet(sheets: model.synth, editor: model.editor)
            }
    }
}

/// The MIDI and project sheets (plan 6a), split from `DocumentSheets` to
/// keep each chain short for the type checker.
private struct InterchangeSheets: ViewModifier {
    @Bindable var model: DocumentModel
    let docID: UUID

    func body(content: Content) -> some View {
        content
            // The MIDI sheets and the report open only for what the user
            // started here, never under the control channel (plan 6a).
            .sheet(
                isPresented: Binding(
                    get: { model.interchange.importShown && !ControlChannel.active },
                    set: { model.interchange.importShown = $0 })
            ) {
                MidiImportSheet(interchange: model.interchange, destinations: appStore.mixer(docID)?.destinations ?? [])
            }
            .sheet(
                isPresented: Binding(
                    get: { model.interchange.exportShown && !ControlChannel.active },
                    set: { model.interchange.exportShown = $0 })
            ) {
                MidiExportSheet(interchange: model.interchange, document: model)
            }
            .sheet(
                isPresented: Binding(
                    get: { model.interchange.reportShown && !ControlChannel.active },
                    set: { model.interchange.reportShown = $0 })
            ) {
                ReportSheet(interchange: model.interchange)
            }
            .sheet(
                isPresented: Binding(
                    get: { model.projectSettings.shown && !ControlChannel.active },
                    set: { model.projectSettings.shown = $0 })
            ) {
                ProjectSettingsSheet(settings: model.projectSettings, editor: model.editor)
            }
            // The return prompt's second page (plan 6e Task 11), as the
            // DAWproject sheet: only for what the user opened, never under
            // the control channel. It waits while another sheet is up
            // (`MergeModel.presented`), so a queued merge is never
            // cancelled by a sheet SwiftUI did not show.
            .sheet(
                isPresented: Binding(
                    get: { model.merge.presented && !ControlChannel.active },
                    set: { model.merge.setPresented($0, channelActive: ControlChannel.active) })
            ) {
                MergeSheet(merge: model.merge)
            }
    }
}

/// The document's alerts (recovered recordings and the document alert).
private struct DocumentAlerts: ViewModifier {
    @Bindable var model: DocumentModel

    func body(content: Content) -> some View {
        content
            .alert(
                "Recovered Recordings",
                isPresented: Binding(
                    get: { model.record.showsRecoveryAlert(channelOn: ControlChannel.active) },
                    set: { if !$0 { model.record.answerRecovery(import: nil) } })
            ) {
                Button("Import into New Tracks") { model.record.answerRecovery(import: true) }
                Button("Move to Trash", role: .destructive) { model.record.answerRecovery(import: false) }
                Button("Later", role: .cancel) { model.record.answerRecovery(import: nil) }
            } message: {
                Text(model.record.recoveryMessage)
            }
            .alert(
                model.alert?.title ?? "",
                // With the control channel on no sheet opens (control spec §7):
                // the channel reports the alert and it stays on the document.
                isPresented: Binding(
                    get: { ControlAlertLog.showsSheet(model.alert, channelOn: ControlChannel.active) },
                    set: { if !$0 { model.alert = nil } }),
                presenting: model.alert
            ) { _ in
                Button("OK") { model.alert = nil }
            } message: { item in
                Text(item.message)
            }
    }
}

/// The transport bar along the top of the window (plan 7a Ruling 9;
/// groups-vcas-transport Rulings 13 and 14): an ordinary view, not a
/// toolbar. One row of the app's transport items (`TransportRowView`),
/// from the transport bar preference; what does not fit folds into ».
/// The mixer window shows it with `showsViews` false, without the
/// document-only items.
struct DocumentTransportBar: View {
    let model: DocumentModel
    let docID: UUID
    let showsViews: Bool
    /// The window's token: Customize Transport Bar opens on the window
    /// that asked (Task 12).
    @Environment(\.transportWindowToken) private var window

    var body: some View {
        TransportRowHost(
            model: model, docID: docID, documentWindow: showsViews, layout: appStore.transportLayout.layout,
            onCustomize: customize
        )
        .background(.bar)
        .sheet(isPresented: sheetShown) { sheet }
    }

    /// The row's "Customize Transport Bar…": the sheet on this window.
    private var customize: (@MainActor () -> Void)? {
        guard let t = window else { return nil }
        let layouts = appStore.transportLayout
        return { layouts.beginCustomizing(window: t) }
    }

    private var sheetShown: Binding<Bool> {
        let layouts = appStore.transportLayout
        let token = window
        return Binding(
            get: { token != nil && layouts.customizing == token && layouts.customizer != nil },
            set: { shown in
                if !shown, token != nil, layouts.customizing == token { layouts.endCustomizing() }
            })
    }

    @ViewBuilder private var sheet: some View {
        if let c = appStore.transportLayout.customizer {
            TransportCustomizeSheet(model: c, keys: keyBindings).themedRoot()
        }
    }
}

/// A window's token in its views (Task 12: the sheet's window).
struct TransportWindowTokenKey: EnvironmentKey {
    static let defaultValue: UUID? = nil
}

extension EnvironmentValues {
    var transportWindowToken: UUID? {
        get { self[TransportWindowTokenKey.self] }
        set { self[TransportWindowTokenKey.self] = newValue }
    }
}

/// Hosts the row; a new layout (Settings, the channel) updates it in place.
struct TransportRowHost: NSViewRepresentable {
    let model: DocumentModel
    let docID: UUID
    let documentWindow: Bool
    let layout: TransportLayout
    /// The row's "Customize Transport Bar…" (nil: off).
    var onCustomize: (@MainActor () -> Void)?

    func makeNSView(context: Context) -> TransportRowView {
        let m = model
        let id = docID
        let doc = documentWindow
        let row = TransportRowView(document: m, layout: layout, documentWindow: doc) { item in
            TransportItemViews.make(item, model: m, docID: id, documentWindow: doc)
        }
        row.dspLoad = { appStore.mixer(id)?.dspLoad ?? 0 }
        row.commandRow = { keyBindings.row($0) }
        row.runCommand = { cmd in
            CommandButtonAction.run(
                cmd, doc: appStore.model(id).map { (id, $0) }, store: appStore, host: AppControlHost.shared,
                menu: AppMenuPerformer())
        }
        row.onCustomize = onCustomize
        return row
    }

    func updateNSView(_ row: TransportRowView, context: Context) {
        row.onCustomize = onCustomize
        row.theme = context.environment.symbioticTheme
        row.setLayout(layout)
    }

    /// The width it is offered; its own height.
    func sizeThatFits(_ proposal: ProposedViewSize, nsView row: TransportRowView, context: Context) -> CGSize? {
        let w = proposal.width.flatMap { $0.isFinite ? $0 : nil } ?? 700
        return CGSize(width: w, height: row.fittingHeight)
    }
}
