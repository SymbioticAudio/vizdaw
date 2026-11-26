import SwiftUI
import SymbioticTokens
import VizDAWAppModel
import VizDAWCore

/// The Track Synth sheet (spec §6.7; plan 5d-2 Task 8): the return
/// channels, arming the MIDI lane too, muting it after recording; then
/// Measure, which runs off the main thread.
struct TrackSynthSheet: View {
    @Environment(\.symbioticTheme) private var theme
    @Bindable var sheets: SynthSheets
    let editor: EditorModel
    /// The device's input channels (the core accepts 1–256).
    let channels: Int

    var body: some View {
        if let d = sheets.trackSynth {
            VStack(alignment: .leading, spacing: theme.spacing.m) {
                Text("Track \(d.synthName)").font(theme.type.title.font)
                Text("Records \(d.synthName)'s audio return into a track below this one, armed and monitored, in the same pass as its MIDI.")
                    .font(theme.type.body.font)
                    .foregroundStyle(.secondary)
                    .fixedSize(horizontal: false, vertical: true)
                Form {
                    // With Stereo, the last channel has no partner (final review m5).
                    Stepper(
                        "Return input: \(channelText(d))", value: draft(\.channel),
                        in: 1...max(d.stereo ? channels - 1 : channels, 1))
                    Toggle("Stereo (two channels)", isOn: draft(\.stereo))
                    Toggle("Also record the MIDI track", isOn: draft(\.armMidi))
                    Toggle("Mute the MIDI track after recording", isOn: draft(\.muteAfter))
                }
                .disabled(d.tracked)
                HStack {
                    Text(sheets.returnLatency(d.dest).map { String(format: "Return latency: %.1f ms", $0) } ?? "")
                    Spacer()
                    if sheets.measuring != nil { ProgressView().controlSize(.small) }
                    Button("Measure") { sheets.measure(d.dest) }
                        .disabled(sheets.measuring != nil)
                        .help("Plays five notes to the synth with the transport stopped and times its return")
                }
                if let msg = sheets.message {
                    Text(msg)
                        .font(theme.type.body.font)
                        .fixedSize(horizontal: false, vertical: true)
                }
                HStack {
                    Spacer()
                    Button(d.tracked ? "Done" : "Cancel") { sheets.closeTrackSynth() }
                        .keyboardShortcut(.cancelAction)
                    if !d.tracked {
                        Button("Track Synth") { sheets.commitTrackSynth() }
                            .keyboardShortcut(.defaultAction)
                    }
                }
            }
            .padding(theme.spacing.xl)
            .frame(width: 440)
            .onAppear { editor.beginTextEditing("synthSheet") }
            .onDisappear { editor.endTextEditing("synthSheet") }
        }
    }

    private func channelText(_ d: TrackSynthDraft) -> String {
        d.stereo ? "In \(d.channel)+\(d.channel + 1)" : "In \(d.channel)"
    }

    private func draft<T>(_ key: WritableKeyPath<TrackSynthDraft, T>) -> Binding<T> {
        Binding(
            get: { sheets.trackSynth.map { $0[keyPath: key] } ?? TrackSynthDraft.placeholder[keyPath: key] },
            set: { v in sheets.trackSynth?[keyPath: key] = v })
    }
}

extension TrackSynthDraft {
    /// What a binding reads after the sheet closed (never shown).
    static let placeholder = TrackSynthDraft(seq: 0, lane: 0, dest: 0, synthName: "")
}

/// The New MIDI Destination sheet (plan 5d-1 Ruling 25, plan 5d-2 Task 8):
/// a name, one of the system's MIDI destinations and a channel; the lane
/// then plays to it.
struct NewMidiDestinationSheet: View {
    @Environment(\.symbioticTheme) private var theme
    @Bindable var sheets: SynthSheets
    let editor: EditorModel

    var body: some View {
        if let d = sheets.newDestination {
            VStack(alignment: .leading, spacing: theme.spacing.m) {
                Text("New MIDI Destination").font(theme.type.title.font)
                Form {
                    TextField(
                        "Name", text: Binding(get: { d.name }, set: { sheets.newDestination?.name = $0 }))
                    if d.ports.isEmpty {
                        Text("No MIDI destination is connected.").foregroundStyle(.secondary)
                    } else {
                        Picker("MIDI port", selection: Binding(get: { d.port }, set: { sheets.newDestination?.port = $0 })) {
                            ForEach(d.ports, id: \.self) { Text($0).tag($0) }
                        }
                    }
                    Stepper(
                        "Channel \(d.channel)",
                        value: Binding(get: { d.channel }, set: { sheets.newDestination?.channel = $0 }), in: 1...16)
                }
                if let msg = sheets.destinationMessage {
                    Text(msg).font(theme.type.body.font).foregroundStyle(theme.state.error.color)
                }
                HStack {
                    Spacer()
                    Button("Cancel") { sheets.closeNewDestination() }
                        .keyboardShortcut(.cancelAction)
                    Button("Add") { sheets.commitNewDestination() }
                        .keyboardShortcut(.defaultAction)
                        .disabled(!sheets.canAddDestination)
                }
            }
            .padding(theme.spacing.xl)
            .frame(width: 400)
            .onAppear { editor.beginTextEditing("destinationSheet") }
            .onDisappear { editor.endTextEditing("destinationSheet") }
        }
    }
}

/// The destination editor (plan release-fixes Task 8): an external MIDI
/// destination's name, MIDI port and channel (small-fixes Task 4) and its
/// return latency, typed (committed on Return or Done) or measured.
struct DestinationEditorSheet: View {
    @Environment(\.symbioticTheme) private var theme
    @Bindable var sheets: SynthSheets
    let editor: EditorModel

    var body: some View {
        if let d = sheets.editDestination {
            VStack(alignment: .leading, spacing: theme.spacing.m) {
                Text(d.title).font(theme.type.title.font).foregroundStyle(theme.text.primary.color)
                Form {
                    TextField("Name", text: Binding(get: { d.name }, set: { sheets.editDestination?.name = $0 }))
                        .onSubmit { sheets.commitEdit() }
                        .accessibilityLabel("Destination name")
                    if d.portChoices.isEmpty {
                        Text("No MIDI destination is connected.").foregroundStyle(theme.text.secondary.color)
                    } else {
                        Picker("MIDI port", selection: Binding(get: { d.port }, set: { sheets.editDestination?.port = $0 })) {
                            ForEach(d.portChoices, id: \.self) { Text(d.portTitle($0)).tag($0) }
                        }
                        .accessibilityLabel("MIDI port")
                    }
                    Picker("Channel", selection: Binding(get: { d.channel }, set: { sheets.editDestination?.channel = $0 })) {
                        ForEach(1...16, id: \.self) { Text("\($0)").tag($0) }
                    }
                    .accessibilityLabel("MIDI channel")
                    TextField(
                        "Return latency (ms)",
                        text: Binding(get: { d.latencyText }, set: { sheets.editDestination?.latencyText = $0 })
                    )
                    .onSubmit { sheets.commitEdit() }
                    .accessibilityLabel("Return latency (ms)")
                }
                HStack {
                    Spacer()
                    if sheets.measuring != nil { ProgressView().controlSize(.small).accessibilityLabel("Measuring") }
                    Button("Measure") { sheets.measure(d.dest) }
                        .disabled(sheets.measuring != nil)
                        .help("Plays five notes to the synth with the transport stopped and times its return")
                        .accessibilityLabel("Measure")
                }
                if let msg = sheets.message {
                    Text(msg)
                        .font(theme.type.body.font)
                        .foregroundStyle(theme.text.primary.color)
                        .fixedSize(horizontal: false, vertical: true)
                }
                HStack {
                    Spacer()
                    // Return commits the field (and keeps the sheet); Done
                    // commits and closes; Cancel (and Escape) closes without
                    // committing, so text that does not parse never traps it.
                    Button("Cancel") { sheets.closeEditDestination() }
                        .keyboardShortcut(.cancelAction)
                        .accessibilityLabel("Cancel")
                    Button("Done") { if sheets.commitEdit() { sheets.closeEditDestination() } }
                        .accessibilityLabel("Done")
                }
            }
            .padding(theme.spacing.xl)
            .frame(width: 400)
            .onAppear { editor.beginTextEditing("destinationEditor") }
            .onDisappear { editor.endTextEditing("destinationEditor") }
        }
    }
}
