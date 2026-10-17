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

    public init() {}

    enum CodingKeys: String, CodingKey {
        case version, detail, lastKind, windows, sequenceOrder, blockRule
        case lightMode, snap, laneHeight, ticksPerPoint, follow
    }

    public init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        func field<T: Decodable>(_ key: CodingKeys, _ fallback: T) -> T {
            ((try? c.decodeIfPresent(T.self, forKey: key)) ?? nil) ?? fallback
        }
        let d = EditorLayout()
        version = field(.version, d.version)
        detail = field(.detail, d.detail)
        lastKind = field(.lastKind, d.lastKind)
        windows = field(.windows, d.windows)
        sequenceOrder = field(.sequenceOrder, d.sequenceOrder)
        blockRule = field(.blockRule, d.blockRule)
        lightMode = field(.lightMode, d.lightMode)
        snap = field(.snap, d.snap)
        laneHeight = field(.laneHeight, d.laneHeight)
        ticksPerPoint = field(.ticksPerPoint, d.ticksPerPoint)
        follow = field(.follow, d.follow)
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
