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
                enabled: TransportCommands.enabled("transport.record", model),
                help: record.isRecording ? "Stop recording (R)" : "Record the armed lanes (R)",
                tint: record.isRecording ? theme.state.record.nsColor : nil
            ) { record.pressRecord() }
            .fixedSize()
            // A press on Record with a punch draft open keeps the draft
            // through the popover's close, so Record refuses and shows it
            // again (plan 7d Ruling 7's bar Record rule).
            .background(RecordPressWatcher(record: record))
            AppKitButton(
                "Record Options", icon: .menuDown,
                help: "Replace or overdub, punch, loop, count-in, pre-roll and takes"
            ) { [host] in record.togglePopover(in: host) }
            .fixedSize()
            .popover(isPresented: popoverShown, arrowEdge: .bottom) {
                RecordPopover(model: model, record: record)
            }
            if record.countingIn {
                Text("Count-in")
                    .font(.caption.weight(.semibold))
                    .foregroundStyle(theme.state.record.color)
            }
            if let p = record.passText {
                Text(p).font(.caption.monospacedDigit())
            }
            if let t = record.remainingText {
                Text(t)
                    .font(.caption)
                    .foregroundStyle(.secondary)
                    .help("Recording time left on the project's disk for the armed audio lanes")
            }
        }
    }

    private var popoverShown: Binding<Bool> {
        let r = record
        let h = host
        // Only this window's dismissal closes it: one that moved to the
        // other window stays open there.
        return Binding(get: { r.showsPopover(in: h) }, set: { if !$0, r.showsPopover(in: h) { r.showsPopover = false } })
    }
}

/// Watches for a press on the Record button it sits behind: with a punch
/// draft open, it calls `recordPressBegins()` from a local mouse-down
/// monitor, before the popover's own outside-click handling closes the
/// popover (which would drop the draft). Inert for the user: clicks and
/// VoiceOver pass through it.
struct RecordPressWatcher: NSViewRepresentable {
    let record: RecordModel

    final class Watcher: NSView {
        var record: RecordModel?
        private var monitor: Any?

        override func hitTest(_ point: NSPoint) -> NSView? { nil }
        override func isAccessibilityElement() -> Bool { false }

        override func viewDidMoveToWindow() {
            super.viewDidMoveToWindow()
            if let m = monitor { NSEvent.removeMonitor(m) }
            monitor = nil
            guard window != nil else { return }
            // An NSView is a `@MainActor` class: the block captures it
            // strongly; the monitor goes when the view leaves its window.
            let me = self
            monitor = NSEvent.addLocalMonitorForEvents(matching: .leftMouseDown) { e in
                // Mouse events arrive on the main thread.
                nonisolated(unsafe) let event = e
                MainActor.assumeIsolated { me.pressed(event) }
                return e
            }
        }

        func pressed(_ event: NSEvent) {
            guard let record, let w = window, event.window === w, record.hasOpenPunchDraft,
                bounds.contains(convert(event.locationInWindow, from: nil))
            else { return }
            record.recordPressBegins()
        }
    }

    func makeNSView(context: Context) -> Watcher {
        let w = Watcher()
        w.record = record
        return w
    }

    func updateNSView(_ w: Watcher, context: Context) { w.record = record }

    static func dismantleNSView(_ w: Watcher, coordinator: ()) { w.record = nil }
}

/// The record options (spec §6.9). Fixed while a pass records: the core
/// took them at Record.
struct RecordPopover: View {
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
                    .font(.caption)
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
            .font(.body.monospacedDigit())
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
