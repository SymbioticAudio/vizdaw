import Foundation
import Observation
import VizDAWCore

/// The Bounce sheet's model (plan 5f-2 Task 6; Rulings 3, 5, 8, 11; owner
/// answer 2): the choices, the core's check of them (the external-MIDI
/// warning, the files it would write, the length) and the running job.
@Observable
@MainActor
public final class BounceModel {
    public enum Source: Equatable, Sendable { case mix, stems }

    /// Weak: the sheet can outlive its document.
    @ObservationIgnored private weak var document: DocumentModel?
    @ObservationIgnored private var resetting = false
    /// The destination the check was last asked about.
    @ObservationIgnored private var lastPath: String?

    /// Showing resets the choices to the project's defaults (Ruling 11).
    public var shown = false {
        didSet {
            guard shown, !oldValue else { return }
            reset()
        }
    }
    public var source: Source = .mix { didSet { changed() } }
    public var stems: Set<UInt64> = [] { didSet { changed() } }
    public var useSelection = false { didSet { changed() } }
    public var container: ContainerRow = .wav { didSet { changed() } }
    public var depth: BitDepthRow = .int24 { didSet { changed() } }
    public var rate: UInt32 = 48_000 { didSet { changed() } }
    /// Kept as set; on by default at 16 bits; ignored for float.
    public var dither = false { didSet { changed() } }
    public var addToProject = false { didSet { changed() } }
    /// Write the files. Always so unless "Add to project" is on, which
    /// may be used alone (no panel, no files).
    public var writeFiles = true
    /// "Other…" is the chosen rate (a field from 8,000 to 384,000).
    public var otherRate = false
    /// Ruling 8's sentences (from `bounceCheck`), refreshed on every change.
    public private(set) var warning: String?
    /// The files the bounce would write (`bounceCheck.files`): for stems, the
    /// numbered names it will take in the chosen folder (owner answer 2).
    public private(set) var files: [String] = []
    /// "1:32.4 + tail".
    public private(set) var length = ""
    /// The strips the stems checklist offers (every strip but master).
    public private(set) var strips: [StripRow] = []
    public private(set) var job: UInt64?

    /// The six project rates (Ruling 11); "Other…" takes 8,000–384,000.
    public static let rates: [UInt32] = [44_100, 48_000, 88_200, 96_000, 176_400, 192_000]
    public static let otherRates: ClosedRange<UInt32> = 8_000...384_000

    public init(document: DocumentModel) { self.document = document }

    /// A time selection exists to bounce.
    public var hasSelection: Bool {
        guard let t = document?.editor.selection.timeRange else { return false }
        return t.start < t.end
    }

    /// The job is still listed as running.
    public var running: Bool {
        guard let job, let document else { return false }
        return document.jobs.contains { $0.id == job }
    }

    public var progress: Float? {
        guard let job else { return nil }
        return document?.jobs.first { $0.id == job }?.progress
    }

    /// What the sheet should ask for: a file or folder, or nothing (add only).
    public var needsDestination: Bool { !addToProject || writeFiles }

    /// The picker's value: a listed rate, or 0 for "Other…".
    public var rateChoice: UInt32 {
        get { otherRate ? 0 : rate }
        set {
            if newValue == 0 {
                otherRate = true
            } else {
                otherRate = false
                rate = newValue
            }
        }
    }

    /// The "Other…" field, clamped to 8,000–384,000.
    public func setOtherRate(_ hz: Int) {
        rate = UInt32(min(max(hz, Int(Self.otherRates.lowerBound)), Int(Self.otherRates.upperBound)))
    }

    private func reset() {
        guard let document else { return }
        resetting = true
        let f = (try? document.session.projectInfo().format) ?? ProjectFormatRow(sampleRate: 48_000, depth: .int24)
        source = .mix
        useSelection = false
        container = .wav
        depth = f.depth
        rate = f.sampleRate
        otherRate = !Self.rates.contains(f.sampleRate)
        writeFiles = true
        dither = f.depth == .int16
        addToProject = false
        job = nil
        strips = ((try? document.session.mixer().strips) ?? []).filter { $0.kind != .master }
        stems = Set(strips.map(\.id))
        resetting = false
        check(path: nil)
    }

    private func changed() {
        guard !resetting, shown else { return }
        check(path: lastPath)
    }

    /// The request for `path` (a file, a folder for stems, nil: add only).
    /// `replace` only for a mix (the save panel's confirmation).
    public func request(path: String?, replace: Bool) -> BounceRequestRow? {
        guard let document, let seq = document.selectedSeq else { return nil }
        let range: BounceRangeRow
        if useSelection, let t = document.editor.selection.timeRange, t.start < t.end {
            range = .ticks(start: t.start, end: t.end)
        } else {
            range = .song
        }
        return BounceRequestRow(
            seq: seq, source: source == .mix ? .mix : .stems(strips: stems.sorted()), range: range,
            container: container, depth: depth, rate: rate, dither: depth == .float32 ? false : dither,
            path: path, addToProject: addToProject, replace: source == .mix && replace)
    }

    /// Refreshes `warning`, `files` and `length` for `path`; a failure
    /// clears them (the core's message comes with Bounce).
    public func check(path: String?) {
        lastPath = path
        guard let document, let req = request(path: path, replace: false),
            let row = try? document.session.bounceCheck(req: req)
        else {
            warning = nil
            files = []
            length = ""
            return
        }
        warning = row.warning
        files = row.files
        length = MediaModel.duration(frames: row.frames, rate: req.rate) + " + tail"
    }

    /// Starts the job (`DocumentModel.startJob(.bounce, "Bounce Failed")`).
    public func start(path: String?, replace: Bool) {
        guard let document, let req = request(path: path, replace: replace) else { return }
        job = nil
        var id: UInt64?
        document.startJob(.bounce, "Bounce Failed") {
            let i = try $0.bounce(req: req)
            id = i
            return i
        }
        job = id
        // Done at once: nothing to wait for.
        if id != nil, !running { finished() }
    }

    /// The job ended (the sheet watches `running`): close. A failure is the
    /// document's alert, warnings its Problems banner.
    public func finished() {
        job = nil
        shown = false
    }

    public func cancel() {
        guard let job else { return }
        document?.cancelJob(job)
    }
}
