import SwiftUI
import VizDAWAppModel
import VizDAWCore

/// Sidebar of sequences, table of lanes, transport toolbar.
struct DocumentView: View {
    @Bindable var model: DocumentModel
    let docID: UUID

    var body: some View {
        NavigationSplitView {
            SequenceSidebar(model: model)
                .navigationSplitViewColumnWidth(min: 180, ideal: 220)
        } detail: {
            VStack(spacing: 0) {
                ProjectBanners(model: model)
                EditorSplit(model: model, docID: docID)
            }
        }
        .navigationTitle(model.windowTitle)
        .toolbar { TransportToolbar(model: model) }
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
            item: Binding(get: { model.synth.trackSynth }, set: { if $0 == nil { model.synth.closeTrackSynth() } })
        ) { _ in
            TrackSynthSheet(
                sheets: model.synth, editor: model.editor, channels: Int(model.inputs.info?.channels ?? 0) > 0
                    ? Int(model.inputs.info?.channels ?? 0) : 256)
        }
        .sheet(
            item: Binding(
                get: { model.synth.newDestination }, set: { if $0 == nil { model.synth.closeNewDestination() } })
        ) { _ in
            NewMidiDestinationSheet(sheets: model.synth, editor: model.editor)
        }
        .alert(
            "Recovered Recordings",
            isPresented: Binding(
                get: { model.record.showsRecoveryAlert(channelOn: ControlChannel.active) },
                set: { if !$0 { model.record.answerRecovery(import: nil) } })
        ) {
            Button("Import into New Lanes") { model.record.answerRecovery(import: true) }
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

struct TransportToolbar: ToolbarContent {
    @Bindable var model: DocumentModel

    var body: some ToolbarContent {
        ToolbarItemGroup(placement: .navigation) {
            Button {
                model.playStop()
            } label: {
                Label(
                    model.isPlaying ? "Stop" : "Play",
                    systemImage: model.isPlaying ? "stop.fill" : "play.fill")
            }
            .disabled(!model.canPlay)
            Toggle(
                isOn: Binding(get: { model.looping }, set: { _ in model.toggleLoop() })
            ) {
                Label("Loop", systemImage: "repeat")
            }
            RecordControls(model: model, record: model.record)
        }
        ToolbarItemGroup(placement: .principal) {
            Text(Formatting.position(model.transport))
                .font(.system(.title3, design: .monospaced))
                .frame(minWidth: 220)
        }
        ToolbarItemGroup(placement: .primaryAction) {
            Text(Formatting.device(model.transport))
                .font(.caption)
                .foregroundStyle(.secondary)
            if model.hasFaults || model.hasReport {
                Button {
                    model.showsFaults.toggle()
                } label: {
                    Image(systemName: "circle.fill")
                        .foregroundStyle(model.hasFaults ? .red : .orange)
                }
                .help("Faults and compile messages")
                .popover(isPresented: $model.showsFaults) {
                    VStack(alignment: .leading, spacing: 4) {
                        ForEach(Array(model.messages.enumerated()), id: \.offset) {
                            Text($0.element)
                        }
                    }
                    .padding()
                    .frame(minWidth: 240)
                }
            }
        }
    }
}
