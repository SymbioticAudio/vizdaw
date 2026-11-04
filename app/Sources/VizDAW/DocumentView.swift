import AppKit
import SwiftUI
import VizDAWAppModel
import VizDAWCore

/// One document's window: the shell (transport bar, layout A's panes,
/// status line) with the document's sheets and alerts (plan 7a Task 7).
struct DocumentView: View {
    @Bindable var model: DocumentModel
    let docID: UUID

    var body: some View {
        DocumentShell(model: model, docID: docID)
            .navigationTitle(model.windowTitle)
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

/// The transport bar along the top of the window (plan 7a Ruling 9): an
/// ordinary view, not a toolbar. Play, Loop and Record, the position, the
/// transport fields, the device and (document window only) the faults
/// button, the main-view tabs and the pane toggles. The mixer window
/// shows it with `showsViews` false.
struct DocumentTransportBar: View {
    @Bindable var model: DocumentModel
    let docID: UUID
    let showsViews: Bool

    var body: some View {
        HStack(spacing: 10) {
            HStack(spacing: 6) {
                PlayStopButton(playing: model.isPlaying, enabled: model.canPlay) { model.playStop() }
                    .fixedSize()
                Toggle(
                    isOn: Binding(get: { model.looping }, set: { _ in model.toggleLoop() })
                ) {
                    Label("Loop", systemImage: "repeat")
                }
                .toggleStyle(.button)
                .labelStyle(.iconOnly)
                .help("Loop")
                RecordControls(model: model, record: model.record)
            }
            .fixedSize()
            Text(Formatting.position(model.transport))
                .font(.system(.title3, design: .monospaced))
                .lineLimit(1)
                .fixedSize()
            // The fields take what is left, cut at the right in a narrow window.
            TransportBar(model: model)
                .frame(minWidth: 0, maxWidth: .infinity, alignment: .leading)
                .clipped()
                .layoutPriority(-1)
            Text(Formatting.device(model.transport))
                .font(.caption)
                .foregroundStyle(.secondary)
                .lineLimit(1)
                .truncationMode(.middle)
            if showsViews {
                faults
                views
                    .fixedSize()
            }
        }
        .padding(.horizontal, 8)
        .padding(.vertical, 4)
        .background(.bar)
    }

    /// Attached only in the document window, so `showsFaults` opens one
    /// popover (Ruling 8).
    @ViewBuilder private var faults: some View {
        if model.hasFaults || model.hasReport {
            Button {
                model.showsFaults.toggle()
            } label: {
                Image(systemName: "circle.fill")
                    .foregroundStyle(model.hasFaults ? .red : .orange)
            }
            .help("Faults and compile messages")
            .accessibilityLabel("Faults")
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

    /// The main-view tabs and the pane toggles, through the View menu's
    /// commands (`ui.*`).
    private var views: some View {
        HStack(spacing: 6) {
            Picker(
                "Main View",
                selection: Binding(
                    get: { model.editor.layout.views.main },
                    set: { ViewCommands.run($0 == .arrange ? "ui.view_arrange" : "ui.view_mix", on: nil, model) })
            ) {
                ForEach(MainView.allCases, id: \.self) { Text($0.title).tag($0) }
            }
            .pickerStyle(.segmented)
            .labelsHidden()
            .fixedSize()
            paneToggle("ui.toggle_left", .left, symbol: "sidebar.left", key: "⌃⌘←")
            paneToggle("ui.toggle_bottom", .bottom, symbol: "rectangle.bottomthird.inset.filled", key: "⌃⌘↓")
            paneToggle("ui.toggle_right", .right, symbol: "sidebar.right", key: "⌃⌘→")
        }
    }

    private func paneToggle(_ id: String, _ side: PaneSide, symbol: String, key: String) -> some View {
        Toggle(
            isOn: Binding(get: { ViewCommands.isOn(id, model) }, set: { ViewCommands.run(id, on: $0, model) })
        ) {
            Image(systemName: symbol)
        }
        .toggleStyle(.button)
        .help("\(side.title) (\(key))")
        .accessibilityLabel(side.title)
    }
}

/// Play/Stop as an AppKit button: an `AXButton` named "Play" or "Stop" in
/// the window's accessibility tree, as the toolbar item was, so the control
/// channel's tree (which walks AppKit's elements) finds it.
struct PlayStopButton: NSViewRepresentable {
    let playing: Bool
    let enabled: Bool
    let action: @MainActor () -> Void

    @MainActor
    final class Coordinator: NSObject {
        var action: @MainActor () -> Void

        init(action: @escaping @MainActor () -> Void) { self.action = action }

        @objc func press() { action() }
    }

    func makeCoordinator() -> Coordinator { Coordinator(action: action) }

    func makeNSView(context: Context) -> NSButton {
        let b = NSButton(title: "", target: context.coordinator, action: #selector(Coordinator.press))
        b.bezelStyle = .push
        b.imagePosition = .imageOnly
        update(b)
        return b
    }

    func updateNSView(_ b: NSButton, context: Context) {
        context.coordinator.action = action
        update(b)
    }

    private func update(_ b: NSButton) {
        let title = playing ? "Stop" : "Play"
        b.image = NSImage(systemSymbolName: playing ? "stop.fill" : "play.fill", accessibilityDescription: title)
        b.setAccessibilityLabel(title)
        b.toolTip = "\(title) (Space)"
        b.isEnabled = enabled
    }
}
