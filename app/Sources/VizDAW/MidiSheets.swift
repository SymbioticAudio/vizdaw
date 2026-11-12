import SwiftUI
import SymbioticTokens
import VizDAWAppModel
import VizDAWCore

/// File › Import MIDI File… (spec §9; plan 6a Task 10): where the tracks
/// go, each channel's destination, splitting, MPE and the tempo map. All
/// state lives in `InterchangeModel`.
struct MidiImportSheet: View {
    @Environment(\.symbioticTheme) private var theme
    @Environment(\.openWindow) private var openWindow
    @Bindable var interchange: InterchangeModel
    /// The project's destinations a channel can play to.
    let destinations: [DestRow]

    var body: some View {
        VStack(alignment: .leading, spacing: theme.spacing.m) {
            Text("Import MIDI File").font(theme.type.title.font)
            if let info = interchange.info {
                Text(Self.describe(info, path: interchange.importPath))
                    .font(theme.type.caption.font)
                    .foregroundStyle(theme.text.secondary.color)
                    .fixedSize(horizontal: false, vertical: true)
            }
            Picker("Import into", selection: $interchange.intoNewProject) {
                Text("This project, at the playhead").tag(false)
                Text("A new project").tag(true)
            }
            .pickerStyle(.radioGroup)
            .accessibilityLabel("Import into")
            if interchange.choices.isEmpty {
                Text("The file plays no notes on any channel.")
                    .font(theme.type.body.font)
                    .foregroundStyle(theme.text.secondary.color)
            } else {
                Text(interchange.intoNewProject ? "In a new project each channel plays a new built-in synth." : "Where each channel plays")
                    .font(theme.type.body.font)
                List {
                    ForEach($interchange.choices) { $c in
                        HStack(spacing: theme.spacing.s) {
                            Text(Self.channelName(c))
                                .frame(minWidth: 120, alignment: .leading)
                            Text(c.notes == 1 ? "1 note" : "\(c.notes) notes")
                                .font(theme.type.caption.font.monospacedDigit())
                                .foregroundStyle(theme.text.secondary.color)
                            Spacer(minLength: theme.spacing.s)
                            Picker("Destination", selection: $c.dest) {
                                Text("New built-in synth").tag(UInt64?.none)
                                ForEach(destinations, id: \.id) { d in Text(d.name).tag(UInt64?.some(d.id)) }
                            }
                            .labelsHidden()
                            .frame(width: 200)
                            .disabled(interchange.intoNewProject)
                            .accessibilityLabel("Destination for \(Self.channelName(c).lowercased())")
                        }
                    }
                }
                .frame(height: 160)
            }
            Toggle("Split by channel", isOn: $interchange.splitChannels)
                .accessibilityLabel("Split tracks by channel")
            Toggle("Treat as MPE", isOn: $interchange.mpe)
                .accessibilityLabel("Treat as MPE")
                .help("Member-channel notes, bend, CC 74 and pressure become per-note expression")
            Toggle(
                "Use the file's tempo map",
                isOn: Binding(
                    get: { interchange.useTempo || interchange.intoNewProject }, set: { interchange.useTempo = $0 }))
                .disabled(interchange.useTempoLocked || interchange.intoNewProject)
                .accessibilityLabel("Use the file's tempo map")
                .help(
                    interchange.useTempoLocked || interchange.intoNewProject
                        ? "A sequence without events always takes the file's tempo and meter"
                        : "Replace this sequence's tempo and meter from the playhead with the file's")
            HStack {
                Spacer()
                Button("Cancel") { interchange.importShown = false }
                    .keyboardShortcut(.cancelAction)
                Button("Import") { begin() }
                    .keyboardShortcut(.defaultAction)
            }
        }
        .padding(theme.spacing.l)
        .frame(width: 480)
    }

    private func begin() {
        guard interchange.intoNewProject else {
            interchange.startImport()
            return
        }
        if let id = appStore.importAsNewProject(interchange) { openWindow(value: id) }
    }

    /// "Channel 10", or "Group 2, channel 10" past the first group (1-based,
    /// as people count them).
    static func channelName(_ c: ChannelChoice) -> String {
        c.group == 0 ? "Channel \(c.channel + 1)" : "Group \(c.group + 1), channel \(c.channel + 1)"
    }

    /// The file's name, kind and track count.
    static func describe(_ info: MidiFileInfoRow, path: String) -> String {
        let kind: String
        switch info.kind {
        case .smf0: kind = "Standard MIDI File, type 0"
        case .smf1: kind = "Standard MIDI File, type 1"
        case .clip: kind = "MIDI Clip File"
        }
        let tracks = info.tracks == 1 ? "1 track" : "\(info.tracks) tracks"
        return "\((path as NSString).lastPathComponent): \(kind), \(tracks)"
    }
}

/// File › Export › MIDI File… and MIDI Clip File… (spec §9; plan 6a
/// Ruling 18): the options, then "What won't transfer" from the core's dry
/// run as they change; Export asks where.
struct MidiExportSheet: View {
    @Environment(\.symbioticTheme) private var theme
    @Bindable var interchange: InterchangeModel
    let document: DocumentModel

    private var isSmf: Bool { interchange.exportKind == .smf }

    var body: some View {
        VStack(alignment: .leading, spacing: theme.spacing.m) {
            Text(isSmf ? "Export MIDI File" : "Export MIDI Clip File").font(theme.type.title.font)
            Text(
                isSmf
                    ? "Notes, controllers, tempo and markers as a Standard MIDI File"
                    : "One track, lossless, as a MIDI 2.0 Clip File"
            )
            .font(theme.type.caption.font)
            .foregroundStyle(theme.text.secondary.color)
            if isSmf {
                Picker("Type", selection: $interchange.type0) {
                    Text("Type 1 (one MIDI track per track)").tag(false)
                    Text("Type 0 (everything on one MIDI track)").tag(true)
                }
                .accessibilityLabel("MIDI file type")
                Picker("Resolution", selection: $interchange.ppq) {
                    Text("480 PPQ").tag(UInt16(480))
                    Text("960 PPQ").tag(UInt16(960))
                    Text("15 360 PPQ").tag(UInt16(15_360))
                }
                .accessibilityLabel("Resolution in ticks per quarter note")
            }
            Text("Tracks").font(theme.type.body.font)
            List(document.lanes, id: \.id) { lane in
                Toggle(
                    lane.name,
                    isOn: Binding(
                        get: { interchange.lanes.contains(lane.id) },
                        set: { if $0 { interchange.lanes.insert(lane.id) } else { interchange.lanes.remove(lane.id) } })
                )
                .accessibilityLabel("Export track \(lane.name)")
            }
            .frame(height: 120)
            if isSmf {
                Toggle("Write per-note expression as MPE", isOn: $interchange.exportMpe)
                    .accessibilityLabel("Write per-note expression as MPE")
            } else {
                Toggle("Time selection only", isOn: $interchange.useSelection)
                    .disabled(!interchange.hasSelection)
                    .accessibilityLabel("Time selection only")
                if interchange.lanes.count > 1 {
                    Text("Each track becomes one file in the folder you choose.")
                        .font(theme.type.caption.font)
                        .foregroundStyle(theme.text.secondary.color)
                }
            }
            Text("What won't transfer").font(theme.type.body.font)
            WontTransfer(report: interchange.wontTransfer, checking: interchange.exportError == nil)
            if let e = interchange.exportError {
                AppIcon.fault.label(e)
                    .font(theme.type.body.font)
                    .foregroundStyle(theme.state.error.color)
                    .fixedSize(horizontal: false, vertical: true)
            }
            HStack {
                Spacer()
                Button("Cancel") { interchange.exportShown = false }
                    .keyboardShortcut(.cancelAction)
                Button("Export…") { begin() }
                    .keyboardShortcut(.defaultAction)
                    .disabled(interchange.lanes.isEmpty)
            }
        }
        .padding(theme.spacing.l)
        .frame(width: 480)
        .alert(
            interchange.replaceQuestion?.message ?? "",
            isPresented: Binding(
                get: { interchange.replaceQuestion != nil },
                set: { if !$0 { interchange.answerReplace(false) } })
        ) {
            Button("Replace", role: .destructive) { interchange.answerReplace(true) }
            Button("Cancel", role: .cancel) { interchange.answerReplace(false) }
        }
    }

    /// The Save panel (a folder for several clip tracks), then the job.
    private func begin() {
        let path: String?
        if isSmf {
            path = Panels.chooseMidiExportFile(name: document.snapshot.title, ext: "mid")
        } else if interchange.lanes.count == 1, let id = interchange.lanes.first {
            let name = document.lanes.first { $0.id == id }?.name ?? document.snapshot.title
            path = Panels.chooseMidiExportFile(name: name, ext: "midi2")
        } else {
            // A folder: names already in it are asked about first.
            if let folder = Panels.chooseMidiExportFolder() { interchange.exportToFolder(folder) }
            return
        }
        guard let path else { return }
        interchange.startExport(path: path)
    }
}

/// The dry run's report, in the export sheet.
private struct WontTransfer: View {
    @Environment(\.symbioticTheme) private var theme
    let report: ReportRow?
    /// No answer yet and no refusal: the dry run is running.
    let checking: Bool

    var body: some View {
        if let report {
            let groups = ReportText.groups(report)
            if groups.isEmpty {
                Text("Everything transfers.")
                    .font(theme.type.caption.font)
                    .foregroundStyle(theme.text.secondary.color)
            } else {
                ScrollView(.vertical) {
                    VStack(alignment: .leading, spacing: theme.spacing.xs) {
                        ForEach(Array(groups.enumerated()), id: \.offset) { _, g in
                            Text(g.title).font(theme.type.caption.font.weight(.semibold))
                            ForEach(Array(g.items.enumerated()), id: \.offset) { _, item in
                                Text(ReportText.line(item))
                                    .font(theme.type.caption.font)
                                    .fixedSize(horizontal: false, vertical: true)
                            }
                        }
                    }
                    .frame(maxWidth: .infinity, alignment: .leading)
                }
                .frame(maxHeight: 120)
            }
        } else if checking {
            ProgressView().controlSize(.small)
                .accessibilityLabel("Checking what won't transfer")
        }
    }
}
