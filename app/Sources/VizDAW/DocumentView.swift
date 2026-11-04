import AppKit
import SwiftUI
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
        // One row when it fits, else the fields on a second row (Task 7
        // review I1): never cut at the default window or the minimum.
        TransportBarLayout(spacing: 10) {
            HStack(spacing: 10) {
                // AppKit controls and text: the channel's tree walks AppKit's
                // elements and sees no SwiftUI button or text (final review I1).
                HStack(spacing: 6) {
                    AppKitButton(
                        model.isPlaying ? "Stop" : "Play", icon: model.isPlaying ? .stop : .play,
                        enabled: model.canPlay, help: model.isPlaying ? "Stop (Space)" : "Play (Space)"
                    ) { model.playStop() }
                    .fixedSize()
                    AppKitButton("Loop", icon: .loop, isOn: model.looping, help: "Loop") { model.toggleLoop() }
                        .fixedSize()
                    RecordControls(model: model, record: model.record, host: showsViews ? .document : .mixerWindow)
                }
                .fixedSize()
                // A steady width, so the bar does not jitter as the digits
                // change (Task 7 review M5).
                AppKitLabel(Formatting.position(model.transport), font: Self.positionFont)
                    .fixedSize()
                    .frame(minWidth: Self.positionWidth, alignment: .leading)
            }
            // In a row narrower than they like, the fields narrow (to 72 pt).
            TransportBar(model: model)
                .clipped()
            HStack(spacing: 10) {
                AppKitLabel(
                    Formatting.device(model.transport), font: .preferredFont(forTextStyle: .caption1), secondary: true,
                    truncatesMiddle: true)
                if showsViews {
                    faults
                    views
                        .fixedSize()
                }
            }
        }
        .padding(.horizontal, TransportBarRows.padding)
        .padding(.vertical, 4)
        .background(.bar)
    }

    /// The position's font, and the width of its widest usual text.
    @MainActor static let positionFont = NSFont.monospacedSystemFont(
        ofSize: NSFont.preferredFont(forTextStyle: .title3).pointSize, weight: .regular)
    @MainActor static let positionWidth = AppKitLabel.width(of: "999.4.960  00:00.000", font: positionFont)

    /// Attached only in the document window, so `showsFaults` opens one
    /// popover (Ruling 8).
    @ViewBuilder private var faults: some View {
        if model.hasFaults || model.hasReport {
            AppKitButton(
                "Faults", icon: .fault, help: "Faults and compile messages",
                tint: model.hasFaults ? .systemRed : .systemOrange
            ) { model.showsFaults.toggle() }
            .fixedSize()
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
            paneToggle("ui.toggle_left", .left, icon: .leftPane, key: "⌃⌘←")
            paneToggle("ui.toggle_bottom", .bottom, icon: .bottomPane, key: "⌃⌘↓")
            paneToggle("ui.toggle_right", .right, icon: .rightPane, key: "⌃⌘→")
        }
    }

    private func paneToggle(_ id: String, _ side: PaneSide, icon: AppIcon, key: String) -> some View {
        AppKitButton(side.title, icon: icon, isOn: ViewCommands.isOn(id, model), help: "\(side.title) (\(key))") {
            ViewCommands.run(id, on: nil, model)
        }
        .fixedSize()
    }
}

/// The transport bar's three parts (leading controls, the fields, trailing
/// controls) on one row, or, when `TransportBarRows` says it does not fit,
/// the leading and trailing on the first row and the fields on the second.
/// One layout, so the fields view is never remade when the window resizes.
struct TransportBarLayout: Layout {
    var spacing: CGFloat
    var rowSpacing: CGFloat = 4

    private func ideal(_ subviews: Subviews) -> [CGSize] {
        subviews.map { $0.sizeThatFits(.unspecified) }
    }

    private func rows(_ width: CGFloat, _ s: [CGSize]) -> Int {
        guard s.count == 3 else { return 1 }
        // The width here is inside the bar's padding: add it back for the rule.
        return TransportBarRows.count(
            width: width + 2 * TransportBarRows.padding, leading: s[0].width, fields: s[1].width,
            trailing: s[2].width, spacing: spacing)
    }

    func sizeThatFits(proposal: ProposedViewSize, subviews: Subviews, cache: inout ()) -> CGSize {
        let s = ideal(subviews)
        guard s.count == 3 else { return .zero }
        let one = s[0].width + spacing + s[1].width + spacing + s[2].width
        let width = proposal.width.flatMap { $0.isFinite ? $0 : nil } ?? one
        let top = max(s[0].height, s[2].height)
        guard rows(width, s) == 2 else { return CGSize(width: width, height: max(top, s[1].height)) }
        // On a row of its own the fields part may wrap (the summary above).
        let fields = subviews[1].sizeThatFits(ProposedViewSize(width: width, height: nil))
        return CGSize(width: width, height: top + rowSpacing + fields.height)
    }

    func placeSubviews(in bounds: CGRect, proposal: ProposedViewSize, subviews: Subviews, cache: inout ()) {
        let s = ideal(subviews)
        guard s.count == 3 else { return }
        let w = bounds.width
        if rows(w, s) == 1 {
            let top = max(s[0].height, s[1].height, s[2].height)
            subviews[0].place(
                at: CGPoint(x: bounds.minX, y: bounds.minY + top / 2), anchor: .leading,
                proposal: ProposedViewSize(s[0]))
            let fieldsW = max(w - s[0].width - s[2].width - 2 * spacing, 0)
            subviews[1].place(
                at: CGPoint(x: bounds.minX + s[0].width + spacing, y: bounds.minY + top / 2), anchor: .leading,
                proposal: ProposedViewSize(width: fieldsW, height: s[1].height))
            subviews[2].place(
                at: CGPoint(x: bounds.maxX, y: bounds.minY + top / 2), anchor: .trailing,
                proposal: ProposedViewSize(s[2]))
        } else {
            let top = max(s[0].height, s[2].height)
            subviews[0].place(
                at: CGPoint(x: bounds.minX, y: bounds.minY + top / 2), anchor: .leading,
                proposal: ProposedViewSize(s[0]))
            // The trailing part gives way (the device text truncates) before
            // it covers the leading part.
            let trailingW = max(min(s[2].width, w - s[0].width - spacing), 0)
            subviews[2].place(
                at: CGPoint(x: bounds.maxX, y: bounds.minY + top / 2), anchor: .trailing,
                proposal: ProposedViewSize(width: trailingW, height: s[2].height))
            let fields = subviews[1].sizeThatFits(ProposedViewSize(width: w, height: nil))
            subviews[1].place(
                at: CGPoint(x: bounds.minX, y: bounds.minY + top + rowSpacing), anchor: .topLeading,
                proposal: ProposedViewSize(width: w, height: fields.height))
        }
    }
}
