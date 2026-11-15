import SwiftUI
import SymbioticTokens
import VizDAWAppModel
import VizDAWCore

/// File › Export › AAF… (spec §9; plan 6f Task 8): the options and "What
/// won't transfer" from the core's check as they change. The sheet comes
/// first and Export… asks for the folder to put the core's "<Song> AAF"
/// folder in. All state lives in `AafModel`.
struct AafExportSheet: View {
    @Environment(\.symbioticTheme) private var theme
    @Bindable var aaf: AafModel
    let document: DocumentModel

    /// A kept or running export has its choices already: they stay as sent.
    private var busy: Bool { aaf.waiting || aaf.running }

    var body: some View {
        VStack(alignment: .leading, spacing: theme.spacing.m) {
            Text("Export AAF").font(theme.type.title.font)
            Text("Audio, fades and volume; MIDI as MIDI files beside it")
                .font(theme.type.caption.font)
                .foregroundStyle(theme.text.secondary.color)
            Group {
                Toggle("Embed audio in the AAF file", isOn: $aaf.embed)
                    .accessibilityLabel("Embed audio in the AAF file")
                Picker("Bit depth", selection: $aaf.depth) {
                    Text("16-bit").tag(AafDepthRow.int16)
                    Text("24-bit").tag(AafDepthRow.int24)
                }
                .accessibilityLabel("Bit depth")
                Picker("Stereo tracks", selection: $aaf.stereo) {
                    Text("As left and right mono tracks").tag(AafStereoRow.monoPairs)
                    Text("As stereo tracks").tag(AafStereoRow.stereo)
                }
                .accessibilityLabel("Stereo tracks")
                Picker("Start", selection: $aaf.hourStart) {
                    Text("00:00:00:00").tag(false)
                    Text("01:00:00:00").tag(true)
                }
                .accessibilityLabel("Start")
                Picker("Frame rate", selection: $aaf.fps) {
                    Text("24").tag(AafFpsRow.fps24)
                    Text("25").tag(AafFpsRow.fps25)
                    Text("29.97 drop frame").tag(AafFpsRow.fps2997Drop)
                    Text("30").tag(AafFpsRow.fps30)
                }
                .accessibilityLabel("Frame rate")
                Toggle("Clip gain", isOn: $aaf.clipGain)
                    .accessibilityLabel("Clip gain")
                Toggle("Track volume and pan", isOn: $aaf.automation)
                    .accessibilityLabel("Track volume and pan")
            }
            .disabled(busy)
            if !aaf.defaultName.isEmpty {
                Text("Writes the folder \u{201C}\(aaf.defaultName)\u{201D}.")
                    .font(theme.type.caption.font)
                    .foregroundStyle(theme.text.secondary.color)
            }
            Text("What won't transfer").font(theme.type.body.font)
            WontTransfer(report: aaf.wontTransfer, checking: aaf.checkError == nil)
            if let e = aaf.checkError {
                AppIcon.fault.label(e)
                    .font(theme.type.body.font)
                    .foregroundStyle(theme.state.error.color)
                    .fixedSize(horizontal: false, vertical: true)
            }
            if aaf.waiting {
                HStack(spacing: theme.spacing.s) {
                    ProgressView().controlSize(.small)
                        .accessibilityLabel("Waiting")
                    Text("Waiting for the running job to finish…")
                        .font(theme.type.caption.font)
                        .foregroundStyle(theme.text.secondary.color)
                }
            } else if aaf.running, let p = aaf.progress {
                ProgressView(value: Double(p))
                    .accessibilityLabel("Export progress")
            }
            HStack {
                Spacer()
                if aaf.waiting {
                    Button("Cancel") { aaf.cancelWaiting() }
                        .keyboardShortcut(.cancelAction)
                        .accessibilityLabel("Cancel the waiting export")
                } else if aaf.running, let job = aaf.job {
                    Button("Cancel") { document.cancelJob(job) }
                        .keyboardShortcut(.cancelAction)
                        .accessibilityLabel("Cancel the export")
                } else {
                    Button("Cancel") { aaf.exportShown = false }
                        .keyboardShortcut(.cancelAction)
                        .accessibilityLabel("Cancel")
                    Button("Export…") { begin() }
                        .keyboardShortcut(.defaultAction)
                        .disabled(aaf.wontTransfer == nil || aaf.defaultName.isEmpty)
                        .accessibilityLabel("Export, then choose where")
                }
            }
        }
        .padding(theme.spacing.l)
        .frame(width: 480)
    }

    /// The folder to put the export folder in, then the job (or the wait
    /// for a running one). The core never replaces a folder that is there.
    private func begin() {
        guard let folder = Panels.chooseAafExportFolder(name: aaf.defaultName) else { return }
        aaf.confirmed(folder: folder)
    }
}

/// File › Import Project › AAF… (spec §9; plan 6f Task 8): where the file
/// goes, the MIDI file beside it, a folder to look for missing audio in,
/// and joining left and right tracks. All state lives in `AafModel`.
struct AafImportSheet: View {
    @Environment(\.symbioticTheme) private var theme
    @Bindable var aaf: AafModel

    var body: some View {
        VStack(alignment: .leading, spacing: theme.spacing.m) {
            Text("Import AAF").font(theme.type.title.font)
            if let info = aaf.info {
                Text(Self.describe(info, path: aaf.importPath))
                    .font(theme.type.caption.font)
                    .foregroundStyle(theme.text.secondary.color)
                    .fixedSize(horizontal: false, vertical: true)
            }
            Picker("Import into", selection: $aaf.newProject) {
                Text("Open as a new project").tag(true)
                Text("Add to this project").tag(false)
            }
            .pickerStyle(.radioGroup)
            .accessibilityLabel("Import into")
            HStack(spacing: theme.spacing.s) {
                TextField(
                    "MIDI file beside it",
                    text: Binding(get: { aaf.midiPath ?? "" }, set: { aaf.midiPath = $0.isEmpty ? nil : $0 }))
                    .font(theme.type.body.font)
                    .accessibilityLabel("MIDI file beside it")
                Button("Choose…") {
                    if let path = Panels.chooseMidiBesideAaf() { aaf.midiPath = path }
                }
                .accessibilityLabel("Choose the MIDI file")
            }
            HStack(spacing: theme.spacing.s) {
                Text("Look for missing audio in…").font(theme.type.body.font)
                Text(aaf.mediaFolder.map { ($0 as NSString).lastPathComponent } ?? "Nowhere else")
                    .font(theme.type.caption.font)
                    .foregroundStyle(theme.text.secondary.color)
                    .lineLimit(1)
                    .truncationMode(.middle)
                    .help(aaf.mediaFolder ?? "")
                Spacer(minLength: theme.spacing.s)
                Button("Choose…") {
                    if let folder = Panels.chooseAafMediaFolder() { aaf.mediaFolder = folder }
                }
                .accessibilityLabel("Choose a folder to look for missing audio in")
                if aaf.mediaFolder != nil {
                    Button("Clear") { aaf.mediaFolder = nil }
                        .accessibilityLabel("Clear the folder to look for missing audio in")
                }
            }
            Toggle("Join left and right tracks into stereo", isOn: $aaf.joinPairs)
                .accessibilityLabel("Join left and right tracks into stereo")
            HStack {
                Spacer()
                Button("Cancel") { aaf.importShown = false }
                    .keyboardShortcut(.cancelAction)
                    .accessibilityLabel("Cancel")
                Button("Import") { aaf.startImport(store: appStore) }
                    .keyboardShortcut(.defaultAction)
                    .accessibilityLabel("Import")
            }
        }
        .padding(theme.spacing.l)
        .frame(width: 480)
    }

    /// The file's name, rate, tracks and audio files.
    static func describe(_ info: AafInfoRow, path: String) -> String {
        let tracks = info.tracks == 1 ? "1 track" : "\(info.tracks) tracks"
        let files = info.audioFiles == 1 ? "1 audio file" : "\(info.audioFiles) audio files"
        let offline = info.offline == 0 ? "" : ", \(info.offline) missing"
        return "\((path as NSString).lastPathComponent): \(info.sampleRate) Hz, \(tracks), \(files)\(offline)"
    }
}
