import VizDAWCore

/// How the drum grid shows hits (spec §4e).
public enum DrumView: String, Codable, CaseIterable, Sendable {
    case matrix, timeline

    public var title: String {
        switch self {
        case .matrix: "Step Matrix"
        case .timeline: "Timeline"
        }
    }

    public var row: DrumViewRow {
        switch self {
        case .matrix: .matrix
        case .timeline: .timeline
        }
    }
}

/// The drum grid's bar, shared by every drum grid and saved with the
/// project (Plan 4e Ruling 11). Decoding is tolerant like `EditorLayout`'s,
/// and step, spacing and duration are clamped by Rust (`Settings::sane`)
/// so the grid draws what the strokes paint.
public struct DrumGridSettings: Codable, Equatable, Sendable {
    public var view: DrumView = .matrix
    /// A matrix cell.
    public var step: Int64 = MeterMath.ppq / 4
    /// Distance between a stroke's hits.
    public var spacing: Int64 = MeterMath.ppq / 4
    /// Length of a painted hit.
    public var duration: Int64 = MeterMath.ppq / 8
    /// Velocity (1–127) of a painted hit while its row's velocity lane is hidden.
    public var velocity: UInt8 = 100
    public var rowHeight: Double = 20
    public var velocityHeight: Double = 28
    /// The drum grid's own zoom (a 4/4 bar is 640 pt at 96).
    public var ticksPerPoint: Double = 96

    public init() {}

    enum CodingKeys: String, CodingKey {
        case view, step, spacing, duration, velocity, rowHeight, velocityHeight, ticksPerPoint
    }

    public init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        let d = DrumGridSettings()
        view = c.field(.view, d.view)
        step = c.field(.step, d.step)
        spacing = c.field(.spacing, d.spacing)
        duration = c.field(.duration, d.duration)
        velocity = min(max(c.field(.velocity, d.velocity), 1), 127)
        rowHeight = EditorLayout.sane(c.field(.rowHeight, d.rowHeight), 12...64, d.rowHeight)
        velocityHeight = EditorLayout.sane(c.field(.velocityHeight, d.velocityHeight), 16...96, d.velocityHeight)
        ticksPerPoint = EditorLayout.sane(
            c.field(.ticksPerPoint, d.ticksPerPoint), EditorLayout.ticksPerPointRange, d.ticksPerPoint)
        // The grid draws with the lengths Rust paints with (final review I1).
        if let sane = try? drumGridSettingsSane(settings: row) {
            step = sane.step
            spacing = sane.spacing
            duration = sane.duration
        } else {
            step = max(step, 1)
            spacing = max(spacing, 1)
            duration = max(duration, 1)
        }
    }

    /// The settings as Rust takes them.
    public var row: DrumGridSettingsRow {
        DrumGridSettingsRow(
            view: view.row, step: step, spacing: spacing, duration: duration, velocity: velocity,
            rowHeight: rowHeight, velocityHeight: velocityHeight)
    }

    /// Step, spacing and duration choices: title and ticks.
    public static let lengths: [(String, Int64)] = [
        ("1 Bar", 4 * MeterMath.ppq), ("1/2", 2 * MeterMath.ppq), ("1/4", MeterMath.ppq),
        ("1/8", MeterMath.ppq / 2), ("1/8 Triplet", MeterMath.ppq / 3), ("1/16", MeterMath.ppq / 4),
        ("1/16 Triplet", MeterMath.ppq / 6), ("1/32", MeterMath.ppq / 8), ("1/64", MeterMath.ppq / 16),
    ]

    /// `lengths`, plus `current` when it is none of them (a hand-edited
    /// `ui.json`), so a picker never shows blank.
    public static func lengths(including current: Int64) -> [(String, Int64)] {
        lengths.contains { $0.1 == current } ? lengths : lengths + [("\(current) ticks", current)]
    }
}
