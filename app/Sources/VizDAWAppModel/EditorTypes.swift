import Foundation
import VizDAWCore

/// A lane of a sequence, as cache key and in saved layouts.
public struct LaneKey: Hashable, Codable, Sendable {
    public let seq: UInt64
    public let lane: UInt64

    public init(_ seq: UInt64, _ lane: UInt64) {
        self.seq = seq
        self.lane = lane
    }

    public var ref: LaneRef { LaneRef(seq: seq, lane: lane) }
}

/// The editing tool (spec §3.5); editor-specific tools join in 4b–4e.
public enum Tool: String, Codable, CaseIterable, Sendable {
    case arrow, pencil, eraser

    public var title: String {
        switch self {
        case .arrow: "Arrow"
        case .pencil: "Pencil"
        case .eraser: "Eraser"
        }
    }
}

/// Light drawing mode (Plan Ruling 17).
public enum LightMode: String, Codable, CaseIterable, Sendable {
    case auto, on, off

    public var title: String {
        switch self {
        case .auto: "Automatic"
        case .on: "On"
        case .off: "Off"
        }
    }
}

/// The arrangement's block rule with its user-set size.
public enum BlockRuleSetting: Codable, Hashable, Sendable {
    case silenceGap(beats: Int)
    case fixedBars(Int)
    case wholeLane

    public static let choices: [BlockRuleSetting] = [
        .silenceGap(beats: 1), .silenceGap(beats: 2), .silenceGap(beats: 4),
        .fixedBars(1), .fixedBars(2), .fixedBars(4), .fixedBars(8), .wholeLane,
    ]

    public var rule: BlockRule {
        switch self {
        case .silenceGap(let beats): .silenceGap(ticks: Int64(max(beats, 1)) * MeterMath.ppq)
        case .fixedBars(let n): .fixedBars(bars: UInt32(clamping: max(n, 1)))
        case .wholeLane: .wholeLane
        }
    }

    public var title: String {
        switch self {
        case .silenceGap(let beats): "Silence of \(beats) Beat\(beats == 1 ? "" : "s")"
        case .fixedBars(let n): "\(n) Bar\(n == 1 ? "" : "s")"
        case .wholeLane: "Whole Lane"
        }
    }
}

/// The detail editors (spec §3.7, Plan Ruling 15); 4a shows placeholders.
public enum DetailEditorKind: String, Codable, CaseIterable, Sendable {
    case pianoRoll, list, stripChart, drumGrid

    public var title: String {
        switch self {
        case .pianoRoll: "Piano Roll"
        case .list: "List"
        case .stripChart: "Strip Chart"
        case .drumGrid: "Drum Grid"
        }
    }

    /// The digit of its ⌘ shortcut (⌥⌘ opens it in a window).
    public var key: String {
        switch self {
        case .pianoRoll: "1"
        case .list: "2"
        case .stripChart: "3"
        case .drumGrid: "4"
        }
    }

    /// The plan that builds it.
    public var plan: String {
        switch self {
        case .pianoRoll: "4b"
        case .list: "4c"
        case .stripChart: "4d"
        case .drumGrid: "4e"
        }
    }
}

/// What a detail editor shows.
public enum EditorTarget: Codable, Hashable, Sendable {
    case block(seq: UInt64, lane: UInt64, start: Int64, end: Int64)
    case lane(seq: UInt64, lane: UInt64)
    case sequence(UInt64)

    public var seq: UInt64 {
        switch self {
        case .block(let s, _, _, _), .lane(let s, _), .sequence(let s): s
        }
    }

    public var lane: UInt64? {
        switch self {
        case .block(_, let l, _, _), .lane(_, let l): l
        case .sequence: nil
        }
    }
}

/// A detail editor and its target.
public struct DetailState: Codable, Hashable, Sendable {
    public var kind: DetailEditorKind
    public var target: EditorTarget
    /// Piano roll: lanes shown besides the target's lane; for a sequence
    /// target exactly the lanes shown (nil: the default set). Optional, so
    /// 4a layouts decode (Plan 4b Rulings 9, 11).
    public var lanes: [UInt64]?
    /// List editor: its kind and destination filter (nil: the default).
    /// Optional, so earlier layouts decode (Plan 4c Ruling 19).
    public var listFilter: ListFilter?

    public init(kind: DetailEditorKind, target: EditorTarget, lanes: [UInt64]? = nil, listFilter: ListFilter? = nil) {
        self.kind = kind
        self.target = target
        self.lanes = lanes
        self.listFilter = listFilter
    }
}

/// A torn-off editor window (spec §3.7).
public struct TornWindow: Codable, Hashable, Sendable, Identifiable {
    public let id: UUID
    public var detail: DetailState
    /// `[x, y, width, height]` in screen points, once known.
    public var frame: [Double]?

    public init(id: UUID, detail: DetailState, frame: [Double]?) {
        self.id = id
        self.detail = detail
        self.frame = frame
    }
}
