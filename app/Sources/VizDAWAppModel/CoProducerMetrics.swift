import Foundation
import Observation
import VizDAWCore

/// The time recorder as the app uses it (plan 8f Task 2's
/// `MetricsRecorder`); tests pass a fake that writes nothing. Every call
/// is made from the main actor.
@MainActor
public protocol MetricsRecording: AnyObject {
    func problem() -> String?
    func sample(clock: MetricsClockRow, category: MetricsCategoryRow?, song: MetricsSongRow?)
    func noteUndo(clock: MetricsClockRow, song: MetricsSongRow?)
    func noteStep(clock: MetricsClockRow, song: MetricsSongRow?, label: String)
    func noteMenu(clock: MetricsClockRow, durationMs: UInt64, chosen: String?)
    func flush() throws
    func close() throws
    func clear()
    /// Delete All Time Records… with counting on: clears and deletes
    /// `<support>/Metrics` under the recorder's own lock.
    func clearAndDeleteAll() throws
}

extension MetricsRecorder: MetricsRecording {}

/// Settings › Co-Producer › Count where my time goes (plan 8f Ruling 1):
/// off unless the composer turned it on, a preference of this Mac, and
/// independent of the Co-Producer switch. While off no recorder exists and
/// `<support>/Metrics` is neither made nor written.
@MainActor
@Observable
public final class CoProducerMetrics {
    public static let key = "VizDAW.coProducerMetrics"
    public static let label = "Count where my time goes"
    public static let caption =
        "Counts, on this Mac only, how long you spend composing, recording, editing and mixing, per song and across all songs, in minute blocks. It also notes bursts of undos, tasks repeated many times and long hunts through the menus. It never records your notes, MIDI or any file name but the song's own. Claude sees it only when you ask through Co-Producer, and never brings it up by itself. Counting works even when Co-Producer is off."

    /// Ruling 4's block and sample lengths.
    public static let blockMs: UInt64 = 60_000
    public static let sampleMs: UInt64 = 5_000

    @ObservationIgnored private let defaults: UserDefaults
    /// The support folder `Metrics/` lives in.
    public let supportDir: URL
    @ObservationIgnored private let environment: [String: String]
    @ObservationIgnored private let makePrivate: @MainActor (URL) -> String?
    @ObservationIgnored private let open: @MainActor (URL, UInt64, UInt64) throws -> any MetricsRecording
    /// The clock the read's "today" comes from.
    @ObservationIgnored public var now: @MainActor () -> Date = { Date() }
    /// Ruling 6's summary (support, today, days, song ID): the core's;
    /// tests give their own.
    @ObservationIgnored public var summary:
        @MainActor (String, String, UInt32?, String?) throws -> MetricsSummaryRow = {
            try metricsSummary(support: $0, today: $1, days: $2, songId: $3)
        }

    /// Counting. False when the key is absent or not a Bool.
    public private(set) var on = false
    /// "Time counting could not start: <reason>", a failed delete, or the
    /// recorder's own problem (a damaged file set aside).
    public private(set) var problem: String?
    /// What Settings shows; nil when there are no records.
    public internal(set) var overview: MetricsOverviewRow?
    /// Delete All Time Records… asks first (the composer's click only).
    public var confirmingDelete = false
    /// The open recorder, while counting.
    public private(set) var recorder: (any MetricsRecording)?

    /// Reads the switch and, when it is on, opens the recorder at once.
    /// `open` makes the recorder; `makePrivate` is Task 4's SupportFolder
    /// check (nil: fine).
    public init(
        defaults: UserDefaults, supportDir: URL, environment: [String: String],
        makePrivate: @escaping @MainActor (URL) -> String?,
        open: @escaping @MainActor (URL, UInt64, UInt64) throws -> any MetricsRecording
    ) {
        self.defaults = defaults
        self.supportDir = supportDir
        self.environment = environment
        self.makePrivate = makePrivate
        self.open = open
        if defaults.object(forKey: Self.key) as? Bool == true { turnOn() }
        refreshOverview()
    }

    /// The switch (Ruling 1): kept, then opened or closed.
    public func setOn(_ want: Bool) {
        if want {
            guard !on else { return }
            turnOn()
        } else {
            problem = nil
            closeRecorder()
            defaults.set(false, forKey: Self.key)
        }
        refreshOverview()
    }

    /// Delete All Time Records… (Ruling 1): with counting on, the recorder
    /// clears and deletes under its own lock and counting goes on into a
    /// fresh file; with it off, the folder goes. False when the delete
    /// failed, and `problem` says why.
    @discardableResult
    public func deleteAll() -> Bool {
        confirmingDelete = false
        defer { refreshOverview() }
        do {
            if let recorder {
                try recorder.clearAndDeleteAll()
            } else {
                try metricsDeleteAll(support: supportDir.path)
            }
        } catch {
            problem = AlertItem(error, title: "Time records").message
            return false
        }
        problem = recorder?.problem()
        return true
    }

    /// The app quits: pending counts are written, the open block dropped;
    /// the setting is kept.
    public func quit() { closeRecorder() }

    /// Reads the overview from disk; nil when there are no records or they
    /// cannot be read.
    public func refreshOverview() {
        guard let row = try? metricsOverview(support: supportDir.path), row.songs > 0 else {
            overview = nil
            return
        }
        overview = row
    }

    /// "Time records: N songs since D, X in all." or "No time records."
    public var overviewLine: String {
        guard let o = overview else { return "No time records." }
        let songs = o.songs == 1 ? "1 song" : "\(o.songs) songs"
        return "Time records: \(songs) since \(o.since ?? "?"), \(Self.duration(o.totalSeconds)) in all."
    }

    /// "<h> h <m> min", "<m> min", or "under a minute" (Ruling 6).
    static func duration(_ seconds: UInt64) -> String {
        let minutes = seconds / 60
        if minutes == 0 { return "under a minute" }
        if minutes < 60 { return "\(minutes) min" }
        return "\(minutes / 60) h \(minutes % 60) min"
    }

    /// Ruling 4's block and sample lengths; the e2e's fast ones only when
    /// `VIZDAW_METRICS_FAST` is "1" and `VIZDAW_SUPPORT_DIR` is set.
    public nonisolated static func timing(environment: [String: String]) -> (blockMs: UInt64, sampleMs: UInt64) {
        if environment["VIZDAW_METRICS_FAST"] == "1", environment["VIZDAW_SUPPORT_DIR"] != nil {
            return (2_000, 200)
        }
        return (60_000, 5_000)
    }

    /// The recorder's clock at `date`: wall-clock milliseconds, so a sleep
    /// shows as a gap, and the local day "yyyy-MM-dd".
    public static func clock(_ date: Date) -> MetricsClockRow {
        let f = DateFormatter()
        f.locale = Locale(identifier: "en_US_POSIX")
        f.calendar = Calendar(identifier: .gregorian)
        f.timeZone = .current
        f.dateFormat = "yyyy-MM-dd"
        let ms = max(date.timeIntervalSince1970, 0) * 1000
        return MetricsClockRow(ms: UInt64(ms), day: f.string(from: date))
    }

    /// `co_producer.metrics` (Ruling 6): flushed first when counting, then
    /// `days` days ending today, with `songID`'s song as "this song".
    func read(days: UInt32?, songID: String?) throws -> (on: Bool, row: MetricsSummaryRow) {
        // A failed flush still answers what is on disk.
        try? recorder?.flush()
        let row = try summary(supportDir.path, Self.clock(now()).day, days, songID)
        return (on, row)
    }

    private func turnOn() {
        let fail = { (why: String) in
            self.problem = "Time counting could not start: \(why)"
            self.on = false
            self.recorder = nil
            self.defaults.set(false, forKey: Self.key)
        }
        if let why = makePrivate(supportDir) { return fail(why) }
        let (block, sample) = Self.timing(environment: environment)
        do {
            let r = try open(supportDir, block, sample)
            recorder = r
            on = true
            problem = r.problem()
            defaults.set(true, forKey: Self.key)
        } catch {
            fail(AlertItem(error, title: "Time records").message)
        }
    }

    /// Writes pending counts and drops the open block; nothing is sampled
    /// from then on.
    private func closeRecorder() {
        if let r = recorder {
            do {
                try r.close()
            } catch {
                problem = AlertItem(error, title: "Time records").message
            }
        }
        recorder = nil
        on = false
    }
}
