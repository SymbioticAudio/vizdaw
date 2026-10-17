import Foundation
import VizDAWCore

/// What a list row is: an event kind or a sequence map entry (Plan 4c
/// Ruling 13).
public enum ListKind: String, Codable, CaseIterable, Hashable, Sendable {
    case note, cc, pitchBend, channelPressure, polyPressure, program, sysEx, text, reference, audioRegion
    case tempo, meter, marker
    case automation

    public init(_ k: EventKindRow) {
        switch k {
        case .note: self = .note
        case .cc: self = .cc
        case .pitchBend: self = .pitchBend
        case .channelPressure: self = .channelPressure
        case .polyPressure: self = .polyPressure
        case .program: self = .program
        case .sysEx: self = .sysEx
        case .text: self = .text
        case .reference: self = .reference
        case .audioRegion: self = .audioRegion
        case .automation: self = .automation
        }
    }

    public var eventKind: EventKindRow? {
        switch self {
        case .note: .note
        case .cc: .cc
        case .pitchBend: .pitchBend
        case .channelPressure: .channelPressure
        case .polyPressure: .polyPressure
        case .program: .program
        case .sysEx: .sysEx
        case .text: .text
        case .reference: .reference
        case .audioRegion: .audioRegion
        case .automation: .automation
        case .tempo, .meter, .marker: nil
        }
    }

    /// Routed to a MIDI destination (the destination filter applies).
    public var isMidi: Bool {
        switch self {
        case .note, .cc, .pitchBend, .channelPressure, .polyPressure, .program, .sysEx: true
        default: false
        }
    }

    public var title: String {
        switch self {
        case .note: "Note"
        case .cc: "Controller"
        case .pitchBend: "Pitch Bend"
        case .channelPressure: "Channel Pressure"
        case .polyPressure: "Poly Pressure"
        case .program: "Program"
        case .sysEx: "SysEx"
        case .text: "Text"
        case .reference: "Reference"
        case .audioRegion: "Audio Region"
        case .tempo: "Tempo"
        case .meter: "Meter and Key"
        case .marker: "Marker"
        case .automation: "Automation"
        }
    }

    /// What Insert offers (Plan 4c Ruling 18).
    public static let insertable: [ListKind] = [
        .note, .cc, .pitchBend, .channelPressure, .polyPressure, .program, .sysEx, .text, .tempo, .meter, .marker,
    ]
}

/// What a list hides (Plan 4c Ruling 19).
public struct ListFilter: Codable, Hashable, Sendable {
    public var hiddenKinds: Set<ListKind>
    /// Destinations shown (nil: all).
    public var shownDests: Set<UInt64>?

    public init(hiddenKinds: Set<ListKind> = [], shownDests: Set<UInt64>? = nil) {
        self.hiddenKinds = hiddenKinds
        self.shownDests = shownDests
    }

    /// Whether a row of `kind` routed to `dest` (its own, else its lane's)
    /// shows.
    public func shows(_ kind: ListKind, dest: UInt64?) -> Bool {
        if hiddenKinds.contains(kind) { return false }
        guard let shownDests, kind.isMidi else { return true }
        return dest.map(shownDests.contains) ?? false
    }

    /// The hidden event kinds as the core's view filter (spec §3.3).
    public var viewFilter: ViewFilterRow? {
        let kinds = ListKind.allCases.filter(hiddenKinds.contains).compactMap(\.eventKind)
        return kinds.isEmpty ? nil : ViewFilterRow(hiddenKinds: kinds, hiddenLanes: [])
    }
}

/// List columns (Plan 4c Ruling 14).
public enum ListColumn: String, CaseIterable, Sendable {
    case position, end, duration, kind, lane, dest, data1, data2, release, smpte, mute

    public var title: String {
        switch self {
        case .position: "Position"
        case .end: "End"
        case .duration: "Length"
        case .kind: "Kind"
        case .lane: "Lane"
        case .dest: "Destination"
        case .data1: "Data 1"
        case .data2: "Data 2"
        case .release: "Release"
        case .smpte: "SMPTE"
        case .mute: "Mute"
        }
    }
}

/// The optional columns shown (an app preference, Plan 4c Ruling 19).
public struct ListColumns: Codable, Hashable, Sendable {
    public var end = false
    public var duration = true
    public var release = false
    public var smpte = false
    /// SMPTE frames per second: 24, 25 or 30.
    public var fps = 30

    public init() {}

    public func visible(lanes: Int) -> [ListColumn] {
        var c: [ListColumn] = [.position]
        if end { c.append(.end) }
        if duration { c.append(.duration) }
        c.append(.kind)
        if lanes > 1 { c.append(.lane) }
        c += [.dest, .data1, .data2]
        if release { c.append(.release) }
        if smpte { c.append(.smpte) }
        c.append(.mute)
        return c
    }
}

/// One row of a list editor.
public struct ListRow: Equatable, Identifiable, Sendable {
    public enum Source: Equatable, Sendable {
        case event(lane: LaneKey, row: EventRow)
        case tempo(index: Int, point: TempoPointRow)
        case meter(index: Int, entry: MeterEntryRow)
        case marker(index: Int, marker: MarkerRow)
    }

    public let source: Source

    public init(_ source: Source) { self.source = source }

    /// Events keep their ID across edits; map rows are numbered.
    public var id: String {
        switch source {
        case .event(_, let e): "e\(e.id)"
        case .tempo(let i, _): "t\(i)"
        case .meter(let i, _): "m\(i)"
        case .marker(let i, _): "k\(i)"
        }
    }

    public var tick: Int64 {
        switch source {
        case .event(_, let e): e.tick
        case .tempo(_, let p): p.tick
        case .meter(_, let m): m.tick
        case .marker(_, let m): m.tick
        }
    }

    public var kind: ListKind {
        switch source {
        case .event(_, let e): ListKind(e.kind)
        case .tempo: .tempo
        case .meter: .meter
        case .marker: .marker
        }
    }

    public var event: EventRow? {
        if case .event(_, let e) = source { return e }
        return nil
    }

    public var lane: LaneKey? {
        if case .event(let l, _) = source { return l }
        return nil
    }
}

/// A lane's events for a list.
public struct ListLane: Sendable {
    public var key: LaneKey
    public var events: [EventRow]
    /// The lane's MIDI destination.
    public var dest: UInt64?

    public init(key: LaneKey, events: [EventRow], dest: UInt64?) {
        self.key = key
        self.events = events
        self.dest = dest
    }
}

/// What a list shows.
public struct ListInput: Sendable {
    public var lanes: [ListLane]
    /// A block's range (events starting inside it).
    public var range: Range<Int64>?
    /// The sequence's maps: tempo, meter and marker rows (sequence targets).
    public var info: SequenceInfo?

    public init(lanes: [ListLane], range: Range<Int64>?, info: SequenceInfo?) {
        self.lanes = lanes
        self.range = range
        self.info = info
    }
}

public enum ListRows {
    /// Rows by tick, then tempo, meter, marker, events, then lane order, then
    /// ID (Plan 4c Ruling 13).
    public static func build(_ input: ListInput, filter: ListFilter) -> [ListRow] {
        typealias Keyed = (tick: Int64, group: Int, lane: Int, id: UInt64, row: ListRow)
        var keyed: [Keyed] = []
        func inRange(_ t: Int64) -> Bool { input.range.map { $0.contains(t) } ?? true }
        if let info = input.info {
            if filter.shows(.tempo, dest: nil) {
                for (i, p) in info.tempo.enumerated() where inRange(p.tick) {
                    keyed.append((p.tick, 0, 0, UInt64(i), ListRow(.tempo(index: i, point: p))))
                }
            }
            if filter.shows(.meter, dest: nil) {
                for (i, m) in info.meter.enumerated() where inRange(m.tick) {
                    keyed.append((m.tick, 1, 0, UInt64(i), ListRow(.meter(index: i, entry: m))))
                }
            }
            if filter.shows(.marker, dest: nil) {
                for (i, m) in info.markers.enumerated() where inRange(m.tick) {
                    keyed.append((m.tick, 2, 0, UInt64(i), ListRow(.marker(index: i, marker: m))))
                }
            }
        }
        for (li, lane) in input.lanes.enumerated() {
            for e in lane.events where inRange(e.tick) {
                guard filter.shows(ListKind(e.kind), dest: e.dest ?? lane.dest) else { continue }
                keyed.append((e.tick, 3, li, e.id, ListRow(.event(lane: lane.key, row: e))))
            }
        }
        keyed.sort { ($0.tick, $0.group, $0.lane, $0.id) < ($1.tick, $1.group, $1.lane, $1.id) }
        return keyed.map(\.row)
    }
}

/// What cell text needs besides the row.
public struct ListContext {
    public var seq: UInt64
    public var meter: [MeterEntryRow]
    public var info: SequenceInfo?
    public var laneNames: [LaneKey: String]
    public var destNames: [UInt64: String]
    public var seqNames: [UInt64: String]
    /// Seconds by row ID (the SMPTE column).
    public var seconds: [String: Double]
    public var fps: Int
    /// The tick at a time in seconds (typed SMPTE).
    public var tickAtSeconds: (Double) -> Int64?

    public init(
        seq: UInt64, meter: [MeterEntryRow] = [], info: SequenceInfo? = nil, laneNames: [LaneKey: String] = [:],
        destNames: [UInt64: String] = [:], seqNames: [UInt64: String] = [:], seconds: [String: Double] = [:],
        fps: Int = 30, tickAtSeconds: @escaping (Double) -> Int64? = { _ in nil }
    ) {
        self.seq = seq
        self.meter = meter
        self.info = info
        self.laneNames = laneNames
        self.destNames = destNames
        self.seqNames = seqNames
        self.seconds = seconds
        self.fps = fps
        self.tickAtSeconds = tickAtSeconds
    }

    /// The beat length at `tick`.
    public func beat(at tick: Int64) -> Int64 { MeterMath.beatTicks(MeterMath.bar(at: tick, meter).meter) }
}

/// Cell text (Plan 4c Ruling 16).
public enum ListText {
    static let textKinds: [TextKindRow] = [.text, .lyric, .cue, .marker, .copyright]

    static func title(_ k: TextKindRow?) -> String {
        switch k {
        case .text, nil: "Text"
        case .lyric: "Lyric"
        case .cue: "Cue"
        case .marker: "Marker Text"
        case .copyright: "Copyright"
        }
    }

    static func kindTitle(_ r: ListRow) -> String {
        if let e = r.event, e.kind == .text { return title(e.textKind) }
        return r.kind.title
    }

    /// A note's length, a reference's span or a region's length.
    static func span(_ r: ListRow) -> Int64? {
        guard let e = r.event, [.note, .reference, .audioRegion].contains(e.kind) else { return nil }
        return e.duration
    }

    public static func cell(_ r: ListRow, _ col: ListColumn, _ c: ListContext) -> String {
        switch col {
        case .position:
            return Bbu.of(r.tick, meter: c.meter).text
        case .end:
            guard let len = span(r) else { return "" }
            return Bbu.of(r.tick + len, meter: c.meter).text
        case .duration:
            guard let len = span(r) else { return "" }
            return DurationText.text(len, beat: c.beat(at: r.tick))
        case .kind:
            return kindTitle(r)
        case .lane:
            return r.lane.flatMap { c.laneNames[$0] } ?? ""
        case .dest:
            guard let e = r.event, r.kind.isMidi else { return "" }
            return e.dest.map { c.destNames[$0] ?? "Destination \($0)" } ?? "Lane"
        case .data1:
            return data1(r, c)
        case .data2:
            return data2(r, c)
        case .release:
            guard let e = r.event, e.kind == .note else { return "" }
            return "\(Velocity7.of(e.releaseVelocity))"
        case .smpte:
            return c.seconds[r.id].map { Smpte.text(seconds: $0, fps: c.fps) } ?? ""
        case .mute:
            return r.event?.mute == true ? "M" : ""
        }
    }

    static func data1(_ r: ListRow, _ c: ListContext) -> String {
        switch r.source {
        case .tempo(_, let p): return String(format: "%.2f", p.bpm)
        case .meter(_, let m): return "\(m.num)/\(m.den)"
        case .marker: return ""
        case .event(_, let e):
            switch e.kind {
            case .note, .polyPressure: return PitchName.name(e.pitch)
            case .cc: return "\(e.value2)"
            case .program: return "\(e.value)"
            case .sysEx: return "\(e.data.count) bytes"
            case .text: return title(e.textKind)
            case .reference: return e.refSeq.flatMap { c.seqNames[$0] } ?? "Sequence"
            case .audioRegion: return "Audio"
            case .automation: return e.automation.map { AutomationText.short($0.target) } ?? ""
            case .pitchBend, .channelPressure: return ""
            }
        }
    }

    static func data2(_ r: ListRow, _ c: ListContext) -> String {
        switch r.source {
        case .tempo(_, let p): return p.ramp ? "ramp" : "jump"
        case .meter(_, let m): return KeyName.name(sharps: m.sharps, minor: m.minor)
        case .marker(_, let m): return m.name
        case .event(_, let e):
            switch e.kind {
            case .note: return "\(Velocity7.of(e.velocity))"
            case .cc, .channelPressure, .polyPressure: return "\(Value7.of(e.value))"
            case .pitchBend: return "\(Bend14.of(e.value))"
            case .program: return e.value2 < 0 ? "" : "\(e.value2 / 128):\(e.value2 % 128)"
            case .sysEx:
                let head = e.data.prefix(8).map { String(format: "%02X", $0) }.joined(separator: " ")
                return e.data.count > 8 ? head + " …" : head
            case .text: return e.text ?? ""
            case .reference:
                let loops = e.refLoops == 0 ? "fill" : "×\(e.refLoops)"
                return "\(loops) \(e.refTranspose > 0 ? "+" : "")\(e.refTranspose)"
            case .audioRegion: return String(format: "%.1f dB", e.gainDb)
            case .automation: return e.automation.map { AutomationText.percent($0.value) } ?? ""
            }
        }
    }

    public static func editable(_ r: ListRow, _ col: ListColumn) -> Bool {
        switch r.source {
        case .tempo(let i, _), .meter(let i, _):
            return col == .data1 || col == .data2 || (col == .position && i > 0)
        case .marker:
            return col == .position || col == .data2
        case .event(_, let e):
            switch col {
            case .position, .smpte, .mute: return true
            case .end, .duration, .release: return e.kind == .note
            case .dest: return ListKind(e.kind).isMidi
            case .kind, .lane: return false
            case .data1: return [.note, .polyPressure, .cc, .program, .text].contains(e.kind)
            case .data2:
                return [
                    .note, .cc, .pitchBend, .channelPressure, .polyPressure, .program, .text, .reference, .audioRegion,
                    .automation,
                ]
                .contains(e.kind)
            }
        }
    }

    /// The row as VoiceOver reads it ("Note C4, bar 3 beat 1, velocity 100").
    public static func summary(_ r: ListRow, _ c: ListContext) -> String {
        let b = Bbu.of(r.tick, meter: c.meter)
        let at = "bar \(b.bar) beat \(b.beat)"
        if r.kind == .note { return "Note \(data1(r, c)), \(at), velocity \(data2(r, c))" }
        let d = [data1(r, c), data2(r, c)].filter { !$0.isEmpty }.joined(separator: ", ")
        return "\(kindTitle(r)), \(at)" + (d.isEmpty ? "" : ", \(d)")
    }
}
