import SwiftUI
import SymbioticTokens
import VizDAWAppModel
import VizDAWCore

/// File › Bounce… (plan 5f-2 Task 6): what to bounce, in which format, the
/// core's warning and the files it would write; progress and Cancel while
/// it runs. All state lives in `BounceModel`.
struct BounceSheet: View {
    @Environment(\.symbioticTheme) private var theme
    @Bindable var bounce: BounceModel

    var body: some View {
        VStack(alignment: .leading, spacing: theme.spacing.m) {
            Text("Bounce").font(theme.type.title.font)
            Picker("Bounce", selection: $bounce.source) {
                Text("Mix").tag(BounceModel.Source.mix)
                Text("Stems").tag(BounceModel.Source.stems)
            }
            .pickerStyle(.segmented)
            .disabled(bounce.job != nil)
            if bounce.source == .stems {
                List(bounce.strips, id: \.id) { s in
                    Toggle(
                        s.name,
                        isOn: Binding(
                            get: { bounce.stems.contains(s.id) },
                            set: { if $0 { bounce.stems.insert(s.id) } else { bounce.stems.remove(s.id) } }))
                }
                .frame(height: 120)
            }
            Text(
                "Files ignore mute and solo. Add to project bounces as heard: muted and solo-silenced strips, and strips routed into a muted bus, are silent."
            )
            .font(theme.type.caption.font)
            .foregroundStyle(.secondary)
            if bounce.source == .stems {
                Text("Files already in the folder are kept; new stems take the next free number.")
                    .font(theme.type.caption.font)
                    .foregroundStyle(.secondary)
            }
            Toggle("Time selection only", isOn: $bounce.useSelection)
                .disabled(!bounce.hasSelection)
            Picker("Format", selection: $bounce.container) {
                Text("WAV").tag(ContainerRow.wav)
                Text("AIFF").tag(ContainerRow.aiff)
                Text("CAF").tag(ContainerRow.caf)
            }
            Picker("Bits", selection: $bounce.depth) {
                Text("16-bit").tag(BitDepthRow.int16)
                Text("24-bit").tag(BitDepthRow.int24)
                Text("32-bit float").tag(BitDepthRow.float32)
            }
            Picker("Rate", selection: $bounce.rateChoice) {
                ForEach(BounceModel.rates, id: \.self) { Text("\($0) Hz").tag($0) }
                Text("Other…").tag(UInt32(0))
            }
            if bounce.otherRate {
                TextField(
                    "Rate (Hz)",
                    value: Binding(get: { Int(bounce.rate) }, set: { bounce.setOtherRate($0) }),
                    format: .number.grouping(.never)
                )
                .accessibilityLabel("Other rate in Hz, 8000 to 384000")
            }
            Toggle("Dither", isOn: $bounce.dither)
                .disabled(bounce.depth == .float32)
            Toggle("Add to project", isOn: $bounce.addToProject)
            if bounce.addToProject {
                Toggle("Also write the files", isOn: $bounce.writeFiles)
            }
            if let w = bounce.warning {
                AppIcon.warning.label(w)
                    .font(theme.type.body.font)
            }
            if !bounce.length.isEmpty {
                Text("Length \(bounce.length)").font(theme.type.caption.font).foregroundStyle(.secondary)
            }
            if let e = bounce.error {
                AppIcon.fault.label(e)
                    .font(theme.type.body.font)
                    .foregroundStyle(theme.state.error.color)
            }
            if let p = bounce.progress {
                ProgressView(value: Double(p))
            }
            HStack {
                Spacer()
                if bounce.job != nil {
                    Button("Cancel") { bounce.cancel() }
                        .keyboardShortcut(.cancelAction)
                } else {
                    Button("Cancel") { bounce.shown = false }
                        .keyboardShortcut(.cancelAction)
                    Button("Bounce") { begin() }
                        .keyboardShortcut(.defaultAction)
                        .disabled(bounce.source == .stems && bounce.stems.isEmpty)
                }
            }
        }
        .padding(theme.spacing.l)
        .frame(width: 440)
        .onChange(of: bounce.running) { was, now in
            if was && !now { bounce.finished() }
        }
    }

    private func begin() {
        guard bounce.needsDestination else {
            bounce.start(path: nil, replace: false)
            return
        }
        if bounce.source == .stems {
            guard let folder = Panels.chooseBounceFolder() else { return }
            bounce.start(path: folder, replace: false)
        } else {
            let ext: String
            switch bounce.container {
            case .wav: ext = "wav"
            case .aiff: ext = "aif"
            case .caf: ext = "caf"
            }
            guard let file = Panels.chooseBounceFile(name: "Bounce", ext: ext) else { return }
            bounce.start(path: file, replace: true)
        }
    }
}
