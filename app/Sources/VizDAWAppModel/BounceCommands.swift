import Foundation
import VizDAWCore

/// So `BounceCommands.request` can answer a `Result` (the generated record is
/// already `Sendable`). Not `@retroactive`: VizDAWCore is in this package,
/// and Swift refuses the attribute there. If UniFFI ever generates the
/// conformance, this line fails to compile and goes.
extension ControlFailure: Error {}

/// `file.bounce` over the control channel (plan 5f-2 Task 5; Rulings 2, 3,
/// 10): the Bounce dialog's answers as parameters, checked here before the
/// core sees them; the core checks the destinations (Ruling 3).
public enum BounceCommands {
    public static let id = "file.bounce"
    public static let ids: Set<String> = [id]

    /// The request `args` ask for on `m`'s shown sequence: WAV at the
    /// project's rate and depth by default, dither on at 16 bits (Ruling 11).
    @MainActor
    public static func request(_ args: [ControlArg], _ m: DocumentModel) -> Result<BounceRequestRow, ControlFailure> {
        func bad(_ why: String) -> Result<BounceRequestRow, ControlFailure> {
            .failure(.of(.invalidParams, "file.bounce: \(why)"))
        }
        let path = args.text("path")
        let add = args.flag("add_to_project") ?? false
        // Presence answered the registry's dialog check; `add_to_project:
        // false` alone still needs the dialog (pre-flight M6).
        guard path != nil || add else { return .failure(.dialog("Bounce", "path")) }
        guard let seq = m.selectedSeq else { return .failure(.of(.failed, "file.bounce: no sequence is shown")) }
        let base = (try? m.session.projectInfo().format) ?? ProjectFormatRow(sampleRate: 48_000, depth: .int24)
        let source = args.text("source") ?? "mix"
        if source == "selection", args.text("range") == "song" {
            return bad("`source: selection` bounces the time selection; leave out `range: song`")
        }
        let container: ContainerRow
        switch args.text("format") {
        case "aiff": container = .aiff
        case "caf": container = .caf
        default: container = .wav
        }
        let depth: BitDepthRow
        switch args.text("bits") {
        case "16": depth = .int16
        case "24": depth = .int24
        case "32f": depth = .float32
        default: depth = base.depth
        }
        let rate = args.int("rate").flatMap { UInt32(exactly: $0) } ?? base.sampleRate
        let dither = depth == .float32 ? false : (args.flag("dither") ?? (depth == .int16))
        var src = BounceSourceRow.mix
        if source == "stems" {
            guard let text = args.text("strips") else { return bad("stems need `strips` (strip IDs, e.g. \"1,2\")") }
            var ids: [UInt64] = []
            for part in text.split(separator: ",", omittingEmptySubsequences: false) {
                guard !part.isEmpty, part.unicodeScalars.allSatisfy({ ("0"..."9").contains($0) }), let n = UInt64(part) else {
                    return bad("`strips` is strip IDs separated by commas, e.g. \"1,2\"")
                }
                guard !ids.contains(n) else { return bad("strip \(n) is listed twice") }
                ids.append(n)
            }
            src = .stems(strips: ids)
        } else if let p = path {
            // A pre-check only: the rule is the core's `Container::accepts`
            // (crates/core-media/src/export.rs), which refuses again.
            let ext = (p as NSString).pathExtension.lowercased()
            let fits: Bool
            switch container {
            case .wav: fits = ext == "wav"
            case .aiff: fits = ext == "aif" || ext == "aiff"
            case .caf: fits = ext == "caf"
            }
            guard fits else { return bad("`path` must end in .\(container == .aiff ? "aif" : "\(container)") for this format") }
        }
        let range: BounceRangeRow
        if source == "selection" || args.text("range") == "selection" {
            switch (args.int("start"), args.int("end")) {
            case let (s?, e?):
                guard s < e else { return bad("`start` must be before `end`") }
                range = .ticks(start: s, end: e)
            case (nil, nil):
                guard let t = m.editor.selection.timeRange, t.start < t.end else { return bad("no time selection") }
                range = .ticks(start: t.start, end: t.end)
            default:
                return bad("give both `start` and `end`, or neither")
            }
        } else {
            guard args.int("start") == nil, args.int("end") == nil else {
                return bad("`start` and `end` go with `range: selection`")
            }
            range = .song
        }
        return .success(
            BounceRequestRow(
                seq: seq, source: src, range: range, container: container, depth: depth, rate: rate,
                dither: dither, path: path, addToProject: add, replace: args.flag("replace") ?? false))
    }

    /// Ruling 10's status: each file, the media added, the warnings; nil
    /// when there is nothing to say.
    public static func status(_ r: BounceResultRow, problems: [String]) -> String? {
        var parts = r.files.map { "wrote \($0.path) (\($0.frames) frames, \($0.rate) Hz, \($0.bits))" }
        if !r.media.isEmpty { parts.append("added \(r.media.count) media") }
        if !problems.isEmpty { parts.append("warnings: " + problems.joined(separator: "; ")) }
        return parts.isEmpty ? nil : parts.joined(separator: "; ")
    }
}
