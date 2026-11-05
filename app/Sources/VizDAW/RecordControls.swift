import SwiftUI
import SymbioticTokens
import VizDAWAppModel
import VizDAWCore
import VizDAWEditors

/// The toolbar's record controls (spec §6.9; plan 5d-2 Task 3): Record
/// (the R key's `transport.record`, red while recording), the options
/// popover, and the readouts: count-in, the loop pass and the time left.
struct RecordControls: View {
    @Environment(\.symbioticTheme) private var theme
    let model: DocumentModel
    @Bindable var record: RecordModel
    /// The window this bar is in: the popover shows under this chevron
    /// only (plan 7a final review I2).
    let host: RecordPopoverHost

    var body: some View {
        HStack(spacing: 6) {
            // AppKit buttons, so the channel's tree finds them (final review I1).
            AppKitButton(
                record.isRecording ? "Stop Recording" : "Record",
                icon: record.isRecording ? .recording : .record,
                // Enabled while a text edit holds the keyboard: a click with
                // a punch draft open reaches Record, which refuses and says
                // why (fix round 2 N2).
                enabled: record.buttonEnabled,
                help: record.isRecording ? "Stop recording (R)" : "Record the armed lanes (R)",
                tint: record.isRecording ? theme.state.record.nsColor : nil
            ) { record.pressRecord() }
            .fixedSize()
            // Marks the Record button, so the popover's close veto and the
            // bar's punch fields know a press on it (plan 7d Ruling 7's bar
            // Record rule, fix round 1 I1).
            .background(RecordButtonMarker())
            AppKitButton(
                "Record Options", icon: .menuDown,
                help: "Replace or overdub, punch, loop, count-in, pre-roll and takes"
            ) { [host] in record.togglePopover(in: host) }
            .fixedSize()
            // An AppKit popover, whose close veto reads the press (I1).
            .background(
                RecordPopoverAnchor(model: model, record: record, host: host, shown: record.showsPopover(in: host)))
            if record.countingIn {
                Text("Count-in")
                    .font(theme.type.label.font)
                    .foregroundStyle(theme.state.record.color)
            }
            if let p = record.passText {
                Text(p).font(theme.type.caption.font.monospacedDigit())
            }
            if let t = record.remainingText {
                Text(t)
                    .font(theme.type.caption.font)
                    .foregroundStyle(.secondary)
                    .help("Recording time left on the project's disk for the armed audio lanes")
            }
        }
    }
}

/// Marks the Record button it sits behind in `RecordButtons`. Inert for
/// the user: clicks and VoiceOver pass through it.
struct RecordButtonMarker: NSViewRepresentable {
    final class Marker: NSView {
        override func hitTest(_ point: NSPoint) -> NSView? { nil }
        override func isAccessibilityElement() -> Bool { false }

        override func viewDidMoveToWindow() {
            super.viewDidMoveToWindow()
            if window != nil { RecordButtons.register(self) } else { RecordButtons.unregister(self) }
        }
    }

    func makeNSView(context: Context) -> Marker { Marker() }

    func updateNSView(_ m: Marker, context: Context) {}
}

/// Shows the record popover under the chevron it sits behind, through a
/// `RecordPopoverController` (fix round 1 I1). Inert for the user.
struct RecordPopoverAnchor: NSViewRepresentable {
    let model: DocumentModel
    let record: RecordModel
    let host: RecordPopoverHost
    /// The model's `showsPopover(in: host)`, read in the body so a change
    /// updates this view.
    let shown: Bool

    final class Anchor: NSView {
        var controller: RecordPopoverController?
        var shown = false
        override func hitTest(_ point: NSPoint) -> NSView? { nil }
        override func isAccessibilityElement() -> Bool { false }

        override func viewDidMoveToWindow() {
            super.viewDidMoveToWindow()
            if window != nil {
                controller?.sync(shown: shown)
            } else if let c = controller, c.popover.isShown {
                c.popover.close()
            }
        }
    }

    func makeNSView(context: Context) -> Anchor {
        let a = Anchor()
        // Themed like every other hosting view (plan 7b Ruling 12; fix
        // round 2 N1): it follows the appearance preferences live.
        let hosting = NSHostingController(rootView: RecordPopover(model: model, record: record).themedRoot())
        hosting.sizingOptions = [.preferredContentSize]
        let c = RecordPopoverController(record: record, host: host, content: hosting)
        c.anchor = a
        a.controller = c
        return a
    }

    func updateNSView(_ a: Anchor, context: Context) {
        a.shown = shown
        // Next turn: the anchor is laid out, and a close and reopen in the
        // other window's bar happen in order.
        let c = a.controller
        let s = shown
        DispatchQueue.main.async { c?.sync(shown: s) }
    }

    static func dismantleNSView(_ a: Anchor, coordinator: ()) {
        if let c = a.controller, c.popover.isShown { c.popover.close() }
    }
}

/// The record options (spec §6.9). Fixed while a pass records: the core
/// took them at Record.
struct RecordPopover: View {
    @Environment(\.symbioticTheme) private var theme
    let model: DocumentModel
    @Bindable var record: RecordModel
    /// The punch field being edited: moving to the other one commits its
    /// draft; leaving the pair drops it (plan 7d Ruling 7).
    @FocusState private var focusedPunch: TransportField?

    var body: some View {
        Form {
            Picker("Mode", selection: Binding(get: { record.replaces }, set: { record.setReplace($0) })) {
                Text("Replace").tag(true)
                Text("Overdub").tag(false)
            }
            .pickerStyle(.segmented)
            if let note = record.overdubNote {
                Text(note)
                    .font(theme.type.caption.font)
                    .foregroundStyle(.secondary)
                    .fixedSize(horizontal: false, vertical: true)
            }
            Toggle("Punch", isOn: Binding(get: { record.punches }, set: { record.setPunch($0) }))
            HStack {
                punchField(.punchIn)
                punchField(.punchOut)
            }
            .disabled(!record.punches)
            Toggle("Loop", isOn: Binding(get: { model.looping }, set: { _ in model.toggleLoop() }))
            Stepper("Count-in: \(Self.bars(record.countInBars))", value: $record.countInBars, in: 0...4)
            Stepper("Pre-roll: \(Self.bars(record.preRollBars))", value: $record.preRollBars, in: 0...8)
            Toggle("Passes as takes", isOn: $record.passesAsTakes)
            if let t = record.remainingText {
                LabeledContent("Disk", value: t)
            }
        }
        .formStyle(.grouped)
        .disabled(record.isRecording)
        .frame(width: 340)
        // The punch fields take letters and digits, not transport keys.
        .onChange(of: focusedPunch) { old, new in
            record.punchFocusChanged(from: old, to: new)
        }
        .onDisappear { model.editor.endTextEditing("recordPopover") }
    }

    private func punchField(_ f: TransportField) -> some View {
        TextField(f.title, text: Binding(get: { record.punchText(f) }, set: { record.setPunchText(f, $0) }))
            .font(theme.type.body.font.monospacedDigit())
            .focused($focusedPunch, equals: f)
            .onSubmit { record.commitPunch(f) }
    }

    static func bars(_ n: UInt8) -> String {
        switch n {
        case 0: "off"
        case 1: "1 bar"
        default: "\(n) bars"
        }
    }
}
