import SwiftUI
import VizDAWAppModel
import VizDAWCore

/// The toolbar's record controls (spec §6.9; plan 5d-2 Task 3): Record
/// (the R key's `transport.record`, red while recording), the options
/// popover, and the readouts: count-in, the loop pass and the time left.
struct RecordControls: View {
    let model: DocumentModel
    @Bindable var record: RecordModel

    var body: some View {
        HStack(spacing: 6) {
            Button {
                record.pressRecord()
            } label: {
                Image(systemName: record.isRecording ? "record.circle.fill" : "record.circle")
                    .foregroundStyle(record.isRecording ? Color.red : Color.primary)
            }
            .accessibilityLabel(record.isRecording ? "Stop Recording" : "Record")
            .help(record.isRecording ? "Stop recording (R)" : "Record the armed lanes (R)")
            .disabled(!TransportCommands.enabled("transport.record", model))
            Button {
                record.showsPopover.toggle()
            } label: {
                Image(systemName: "chevron.down")
            }
            .accessibilityLabel("Record Options")
            .help("Replace or overdub, punch, loop, count-in, pre-roll and takes")
            .popover(isPresented: $record.showsPopover, arrowEdge: .bottom) {
                RecordPopover(model: model, record: record)
            }
            if record.countingIn {
                Text("Count-in")
                    .font(.caption.weight(.semibold))
                    .foregroundStyle(.red)
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
}

/// The record options (spec §6.9). Fixed while a pass records: the core
/// took them at Record.
struct RecordPopover: View {
    let model: DocumentModel
    @Bindable var record: RecordModel
    /// The punch field being edited: leaving it commits its draft, as a
    /// macOS text field does (5d-2 final review I3).
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
        .onChange(of: focusedPunch) { old, _ in
            if let old { record.commitPunch(old) }
        }
        .onAppear { model.editor.beginTextEditing("recordPopover") }
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
