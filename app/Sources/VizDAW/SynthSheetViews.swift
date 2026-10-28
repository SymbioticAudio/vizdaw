import SwiftUI
import VizDAWAppModel
import VizDAWCore

/// The Track Synth sheet (spec §6.7; plan 5d-2 Task 8): the return
/// channels, arming the MIDI lane too, muting it after recording; then
/// Measure, which runs off the main thread.
struct TrackSynthSheet: View {
    @Bindable var sheets: SynthSheets
    let editor: EditorModel
    /// The device's input channels (the core accepts 1–256).
    let channels: Int

    var body: some View {
        if let d = sheets.trackSynth {
            VStack(alignment: .leading, spacing: 12) {
                Text("Track \(d.synthName)").font(.headline)
                Text("Records \(d.synthName)'s audio return into a lane below this one, armed and monitored, in the same pass as its MIDI.")
                    .font(.callout)
                    .foregroundStyle(.secondary)
                    .fixedSize(horizontal: false, vertical: true)
                Form {
                    // With Stereo, the last channel has no partner (final review m5).
                    Stepper(
                        "Return input: \(channelText(d))", value: draft(\.channel),
                        in: 1...max(d.stereo ? channels - 1 : channels, 1))
                    Toggle("Stereo (two channels)", isOn: draft(\.stereo))
                    Toggle("Also record the MIDI lane", isOn: draft(\.armMidi))
                    Toggle("Mute the MIDI lane after recording", isOn: draft(\.muteAfter))
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
                        .font(.callout)
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
            .padding(20)
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
    @Bindable var sheets: SynthSheets
    let editor: EditorModel

    var body: some View {
        if let d = sheets.newDestination {
            VStack(alignment: .leading, spacing: 12) {
                Text("New MIDI Destination").font(.headline)
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
                    Text(msg).font(.callout).foregroundStyle(.red)
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
            .padding(20)
            .frame(width: 400)
            .onAppear { editor.beginTextEditing("destinationSheet") }
            .onDisappear { editor.endTextEditing("destinationSheet") }
        }
    }
}
