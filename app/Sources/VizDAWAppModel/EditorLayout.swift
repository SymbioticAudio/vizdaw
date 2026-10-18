import Foundation

/// Everything about the editors that is saved with the project but is not
/// document state (Plan Ruling 12): stored as JSON by the session in the
/// package's `ui.json`, written on Save, never an undo step. Decoding is
/// tolerant: a missing or malformed field keeps its default.
public struct EditorLayout: Codable, Equatable, Sendable {
    public var version = 1
    /// The split detail pane's content (`nil`: empty).
    public var detail: DetailState?
    /// The editor a double-clicked block opens in.
    public var lastKind: DetailEditorKind = .pianoRoll
    public var windows: [TornWindow] = []
    /// The sequence sidebar's order (Plan Ruling 12).
    public var sequenceOrder: [UInt64] = []
    public var blockRule: BlockRuleSetting = .silenceGap(beats: 4)
    public var lightMode: LightMode = .auto
    public var snap = Snap()
    public var laneHeight: Double = 48
    public var ticksPerPoint: Double = 384
    /// Follow playback (page mode, Plan Ruling 18).
    public var follow = true
    /// Every piano roll's view settings (Plan 4b Ruling 11).
    public var pianoRoll = PianoRollSettings()
    /// Every Strip Chart's bar (Plan 4d Ruling 21).
    public var stripChart = StripChartSettings()

    public init() {}

    enum CodingKeys: String, CodingKey {
        case version, detail, lastKind, windows, sequenceOrder, blockRule
        case lightMode, snap, laneHeight, ticksPerPoint, follow, pianoRoll, stripChart
    }

    public init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        let d = EditorLayout()
        version = c.field(.version, d.version)
        detail = c.field(.detail, d.detail)
        lastKind = c.field(.lastKind, d.lastKind)
        windows = c.field(.windows, d.windows)
        sequenceOrder = c.field(.sequenceOrder, d.sequenceOrder)
        blockRule = c.field(.blockRule, d.blockRule)
        lightMode = c.field(.lightMode, d.lightMode)
        snap = c.field(.snap, d.snap)
        laneHeight = Self.sane(c.field(.laneHeight, d.laneHeight), Self.laneHeightRange, d.laneHeight)
        ticksPerPoint = Self.sane(c.field(.ticksPerPoint, d.ticksPerPoint), Self.ticksPerPointRange, d.ticksPerPoint)
        follow = c.field(.follow, d.follow)
        pianoRoll = c.field(.pianoRoll, d.pianoRoll)
        stripChart = c.field(.stripChart, d.stripChart)
    }

    /// Lane heights a hand-edited or corrupt `ui.json` is clamped to.
    public static let laneHeightRange: ClosedRange<Double> = 16...400
    /// Zoom levels (`TimeAxis`'s limits) a decoded layout is clamped to.
    public static let ticksPerPointRange: ClosedRange<Double> = 2...61_440

    /// `v` clamped to `range`; `fallback` when it is not a finite positive number.
    static func sane(_ v: Double, _ range: ClosedRange<Double>, _ fallback: Double) -> Double {
        guard v.isFinite, v > 0 else { return fallback }
        return min(max(v, range.lowerBound), range.upperBound)
    }

    /// The layout in `json`, or the default when it is missing or unreadable.
    public static func decode(_ json: String?) -> EditorLayout {
        guard let data = json?.data(using: .utf8),
            let l = try? JSONDecoder().decode(EditorLayout.self, from: data)
        else { return EditorLayout() }
        return l
    }

    public func encoded() -> String {
        let e = JSONEncoder()
        e.outputFormatting = [.sortedKeys]
        guard let data = try? e.encode(self) else { return "{}" }
        return String(decoding: data, as: UTF8.self)
    }
}

extension KeyedDecodingContainer {
    /// Tolerant decoding: a missing or malformed field keeps `fallback`.
    func field<T: Decodable>(_ key: Key, _ fallback: T) -> T {
        ((try? decodeIfPresent(T.self, forKey: key)) ?? nil) ?? fallback
    }
}
