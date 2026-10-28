import SwiftUI
import VizDAWAppModel
import VizDAWCore

/// Settings › Audio (spec §6.6, §6.9; plan 5d-2 Task 7): the input device,
/// Software Monitoring, the I/O buffer size with the round trip, the
/// latency trim, and the streamer's RAM cache and read-ahead.
struct AudioSettingsView: View {
    @Bindable var prefs: AudioPreferences
    let store: DocumentStore

    var body: some View {
        Form {
            Section("Input") {
                Picker(
                    "Input device",
                    selection: Binding(get: { prefs.settings.inputDevice }, set: { prefs.setInputDevice($0) })
                ) {
                    ForEach(prefs.inputDeviceChoices(), id: \.title) { c in Text(c.title).tag(c.name) }
                }
                Text("A new device takes effect at once; while recording or measuring, once that stops.")
                    .font(.caption)
                    .foregroundStyle(.secondary)
                Toggle(
                    "Software Monitoring",
                    isOn: Binding(
                        get: { prefs.settings.softwareMonitoring }, set: { prefs.setSoftwareMonitoring($0) }))
                Text(
                    "Off: VizDAW never plays an input back; listen through your interface's direct monitoring. "
                        + "Recording and MIDI thru are unaffected."
                )
                .font(.caption)
                .foregroundStyle(.secondary)
            }
            Section("Latency") {
                Picker(
                    "I/O Buffer Size",
                    selection: Binding(
                        get: { prefs.settings.bufferFrames },
                        set: {
                            prefs.setBufferFrames($0)
                            readDevice()
                        })
                ) {
                    ForEach(AudioCommands.bufferSizes, id: \.self) { Text("\($0) frames").tag($0) }
                }
                Text(AudioPreferences.roundTripText(prefs.device, bufferFrames: prefs.settings.bufferFrames))
                    .font(.caption)
                    .foregroundStyle(.secondary)
                Stepper(
                    value: Binding(
                        get: { Int(prefs.settings.latencyTrim) }, set: { prefs.setLatencyTrim(Int32(clamping: $0)) }),
                    in: -192_000...192_000
                ) {
                    LabeledContent(
                        "Latency trim",
                        value: AudioPreferences.trimText(
                            prefs.settings.latencyTrim, sampleRate: prefs.device?.sampleRate ?? 0))
                }
                Button("Reset Trim") { prefs.setLatencyTrim(0) }
                    .disabled(prefs.settings.latencyTrim == 0)
            }
            Section("Streaming") {
                Picker(
                    "RAM cache",
                    selection: Binding(
                        get: { prefs.settings.cacheMb },
                        set: { prefs.setStream(cacheMb: $0, readAheadSeconds: prefs.settings.readAheadSeconds) })
                ) {
                    ForEach(AudioPreferences.cacheChoices, id: \.self) {
                        Text($0 >= 1024 ? "\($0 / 1024) GB" : "\($0) MB").tag($0)
                    }
                }
                Stepper(
                    "Disk read-ahead: \(prefs.settings.readAheadSeconds) s",
                    value: Binding(
                        get: { prefs.settings.readAheadSeconds },
                        set: { prefs.setStream(cacheMb: prefs.settings.cacheMb, readAheadSeconds: $0) }),
                    in: 2...8)
                Text("Cache and read-ahead apply from the next playback.")
                    .font(.caption)
                    .foregroundStyle(.secondary)
            }
            if let p = prefs.problem {
                Text(p).foregroundStyle(.red)
            }
        }
        .formStyle(.grouped)
        .frame(width: 500)
        .padding()
        .onAppear { readDevice() }
    }

    /// Any open document reads the one device.
    private func readDevice() {
        let holder = store.documents.values.first { ((try? $0.session.inputInfo())?.device ?? "") != "" }
        prefs.readDevice(holder?.session)
    }
}

/// Settings › Recording (spec §6.9): the format of new projects, the
/// record options new documents start with, the monitor mode Add Lane
/// gives, and the low-disk thresholds.
struct RecordingSettingsView: View {
    @Bindable var prefs: AudioPreferences
    let store: DocumentStore

    var body: some View {
        Form {
            Section("New Projects") {
                Picker(
                    "Sample rate",
                    selection: Binding(
                        get: { store.defaultFormat.sampleRate },
                        set: { store.defaultFormat = ProjectFormatRow(sampleRate: $0, depth: store.defaultFormat.depth) })
                ) {
                    ForEach(projectRates(), id: \.self) { Text(ProjectBanners.khz(Double($0))).tag($0) }
                }
                Picker(
                    "Bit depth",
                    selection: Binding(
                        get: { store.defaultFormat.depth },
                        set: {
                            store.defaultFormat = ProjectFormatRow(sampleRate: store.defaultFormat.sampleRate, depth: $0)
                        })
                ) {
                    ForEach(FormatPanel.depths, id: \.1) { Text($0.1).tag($0.0) }
                }
            }
            Section("New Documents") {
                Stepper(
                    "Count-in: \(RecordPopover.bars(prefs.settings.countInBars))",
                    value: Binding(
                        get: { prefs.settings.countInBars },
                        set: {
                            prefs.setRecordDefaults(
                                countIn: $0, preRoll: prefs.settings.preRollBars,
                                passesAsTakes: prefs.settings.passesAsTakes)
                        }),
                    in: 0...4)
                Stepper(
                    "Pre-roll: \(RecordPopover.bars(prefs.settings.preRollBars))",
                    value: Binding(
                        get: { prefs.settings.preRollBars },
                        set: {
                            prefs.setRecordDefaults(
                                countIn: prefs.settings.countInBars, preRoll: $0,
                                passesAsTakes: prefs.settings.passesAsTakes)
                        }),
                    in: 0...8)
                Toggle(
                    "Passes as takes",
                    isOn: Binding(
                        get: { prefs.settings.passesAsTakes },
                        set: {
                            prefs.setRecordDefaults(
                                countIn: prefs.settings.countInBars, preRoll: prefs.settings.preRollBars,
                                passesAsTakes: $0)
                        }))
                Picker(
                    "Monitoring for new lanes",
                    selection: Binding(get: { prefs.settings.defaultMonitor }, set: { prefs.setDefaultMonitor($0) })
                ) {
                    Text("Auto").tag("auto")
                    Text("Input").tag("input")
                    Text("Off").tag("off")
                }
                Text("Open documents keep their own options: change those in the toolbar's record popover.")
                    .font(.caption)
                    .foregroundStyle(.secondary)
            }
            Section("Disk") {
                Text(AudioPreferences.lowDiskText)
                    .foregroundStyle(.secondary)
            }
        }
        .formStyle(.grouped)
        .frame(width: 500)
        .padding()
    }
}
