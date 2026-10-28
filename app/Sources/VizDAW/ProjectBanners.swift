import SwiftUI
import VizDAWAppModel
import VizDAWCore

/// The bars above a document's editors (plan 5a Task 13): jobs, media in
/// another format, a device at another rate, disk underruns, problems.
struct ProjectBanners: View {
    @Bindable var model: DocumentModel

    var body: some View {
        VStack(spacing: 0) {
            ForEach(Array(model.banners.enumerated()), id: \.offset) { _, b in
                row(b)
                Divider()
            }
        }
    }

    @ViewBuilder
    private func row(_ b: ProjectBanner) -> some View {
        HStack(spacing: 8) {
            switch b {
            case .job(let id, let kind, let progress):
                ProgressView(value: Double(progress)).frame(width: 120)
                    .accessibilityLabel(ProjectBanner.jobText(kind))
                Text(ProjectBanner.jobText(kind))
                Spacer()
                Button("Cancel") { model.cancelJob(id) }
            case .mismatched(let count, let label):
                Image(systemName: "exclamationmark.triangle")
                Text("\(count) media file\(count == 1 ? " is" : "s are") not in the project format (\(label)) and stay silent.")
                Spacer()
                Button("Convert \(count) File\(count == 1 ? "" : "s")") { model.convertMedia() }
            case .rateMismatch(let device, let project):
                Image(systemName: "speaker.slash")
                Text("The audio device runs at \(Self.khz(device)); this project is \(Self.khz(Double(project))). Audio regions are muted.")
                Spacer()
                Button("Change Project Rate to \(Self.khz(device))") { model.changeRateToDevice() }
            case .diskTooSlow:
                Image(systemName: "externaldrive.badge.exclamationmark")
                Text("Disk too slow: some audio played as silence.")
                Spacer()
                Button("Dismiss") { model.dismissDiskWarning() }
            case .problems(let lines):
                Image(systemName: "exclamationmark.circle")
                Text(lines.joined(separator: "\n")).lineLimit(3)
                Spacer()
                Button("Dismiss") { model.dismissProblems() }
            }
        }
        .font(.callout)
        .padding(.horizontal, 10)
        .padding(.vertical, 5)
        .background(.bar)
    }

    static func khz(_ hz: Double) -> String {
        let k = hz / 1000
        return k.rounded() == k ? "\(Int(k)) kHz" : String(format: "%.1f kHz", k)
    }
}
