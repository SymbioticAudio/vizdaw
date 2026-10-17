import Foundation
import VizDAWCore

/// Why typed text was refused; `message` goes to the status line.
public struct ListEditError: Error, Equatable {
    public let message: String

    public init(_ message: String) { self.message = message }
}

/// What a committed cell does.
public enum ListChange: Equatable {
    case unchanged
    /// Send the command; the row then starts at `tick` (the list follows it).
    case edit(EditCommand, tick: Int64)
}

/// Typed cell text to commands (Plan 4c Rulings 15–17).
public enum ListEdit {
    public static func commit(_ text: String, column: ListColumn, row r: ListRow, context c: ListContext) throws
        -> ListChange
    {
        // Free text (text data, marker names) keeps its spaces; every other
        // cell is trimmed before it is parsed.
        let free: Bool
        switch r.source {
        case .event(_, let e): free = e.kind == .text && column == .data2
        case .marker: free = column == .data2
        default: free = false
        }
        let t = free ? text : text.trimmingCharacters(in: .whitespaces)
        // Text equal to what the cell shows changes nothing, so an off-grid
        // event never moves when its unchanged position is committed.
        let shown = ListText.cell(r, column, c)
        if t == (free ? shown : shown.trimmingCharacters(in: .whitespaces)) { return .unchanged }
        switch r.source {
        case .event(_, let e): return try event(t, column, e, c)
        case .tempo(let i, let p): return try tempo(t, column, i, p, c)
        case .meter(let i, let m): return try meter(t, column, i, m, c)
        case .marker(let i, let m): return try marker(t, column, i, m, c)
        }
    }

    static func position(_ t: String, from tick: Int64, _ c: ListContext) throws -> Int64 {
        guard let b = PositionInput.parse(t, current: Bbu.of(tick, meter: c.meter)) else {
            throw ListEditError("\"\(t)\" is not a position: type a bar, bar.beat or bar.beat.unit.")
        }
        // The same position written differently keeps the exact tick.
        return b == Bbu.of(tick, meter: c.meter) ? tick : b.tick(meter: c.meter)
    }

    /// A typed SMPTE time as a tick, rounded to the nearest display unit.
    static func smpte(_ t: String, _ c: ListContext) throws -> Int64 {
        guard let s = Smpte.parse(t, fps: c.fps), let tick = c.tickAtSeconds(s) else {
            throw ListEditError("\"\(t)\" is not an SMPTE time (hh:mm:ss:ff).")
        }
        let unit = max(c.beat(at: tick) / Bbu.unitsPerBeat, 1)
        let start = MeterMath.bar(at: tick, c.meter).start
        return start + ((tick - start + unit / 2) / unit) * unit
    }

    static func number(_ t: String, _ range: ClosedRange<Int>, _ what: String) throws -> Int {
        guard let n = Int(t), range.contains(n) else {
            throw ListEditError("\(what) must be \(range.lowerBound) to \(range.upperBound).")
        }
        return n
    }

    static func event(_ t: String, _ col: ListColumn, _ e: EventRow, _ c: ListContext) throws -> ListChange {
        func replace(_ change: (inout EventRow) -> Void) -> ListChange {
            var n = e
            change(&n)
            return n == e ? .unchanged : .edit(.event(edit: .replace(event: e.id, row: n)), tick: n.tick)
        }
        switch col {
        case .position, .smpte:
            let tick = try (col == .position ? position(t, from: e.tick, c) : smpte(t, c))
            return tick == e.tick
                ? .unchanged : .edit(.event(edit: .move(events: [e.id], delta: tick - e.tick, toLane: nil)), tick: tick)
        case .end:
            let end = try position(t, from: e.tick + e.duration, c)
            guard end >= e.tick else { throw ListEditError("The end is before the start.") }
            return replace { $0.duration = end - e.tick }
        case .duration:
            guard let d = DurationText.parse(t, current: e.duration, beat: c.beat(at: e.tick)) else {
                throw ListEditError("\"\(t)\" is not a length: type beats.units, such as 1.480.")
            }
            let beat = c.beat(at: e.tick)
            if DurationText.text(d, beat: beat) == DurationText.text(e.duration, beat: beat) { return .unchanged }
            return replace { $0.duration = d }
        case .release:
            let v = try number(t, 0...127, "Release velocity")
            return Velocity7.of(e.releaseVelocity) == v ? .unchanged : replace { $0.releaseVelocity = Velocity7.up(v) }
        case .mute:
            let on: Bool
            switch t.lowercased() {
            case "m", "y", "yes", "1", "x", "on": on = true
            case "", "n", "no", "0", "off": on = false
            default: throw ListEditError("Type M to mute, or clear the cell.")
            }
            return on == e.mute ? .unchanged : .edit(.event(edit: .setMute(events: [e.id], mute: on)), tick: e.tick)
        case .dest:
            let dest: UInt64?
            if t.isEmpty || t == "-" || t == "—" || t.lowercased() == "lane" {
                dest = nil
            } else if let id = c.destNames.first(where: { $0.value.caseInsensitiveCompare(t) == .orderedSame })?.key {
                dest = id
            } else {
                throw ListEditError("No destination is named \"\(t)\".")
            }
            return dest == e.dest ? .unchanged : .edit(.event(edit: .setDest(events: [e.id], dest: dest)), tick: e.tick)
        case .data1:
            return try data1(t, e, replace)
        case .data2:
            return try data2(t, e, replace)
        case .kind, .lane:
            return .unchanged
        }
    }

    static func data1(_ t: String, _ e: EventRow, _ replace: ((inout EventRow) -> Void) -> ListChange) throws
        -> ListChange
    {
        switch e.kind {
        case .note, .polyPressure:
            guard let p = PitchName.parse(t) else {
                throw ListEditError("\"\(t)\" is not a pitch: type a name such as C4 or a number 0 to 127.")
            }
            return replace { $0.pitch = p }
        case .cc:
            let n = try number(t, 0...127, "The controller number")
            return replace { $0.value2 = Int64(n) }
        case .program:
            let n = try number(t, 0...127, "The program")
            return replace { $0.value = Int64(n) }
        case .text:
            guard let k = ListText.textKinds.first(where: { ListText.title($0).caseInsensitiveCompare(t) == .orderedSame })
            else { throw ListEditError("Type Text, Lyric, Cue, Marker Text or Copyright.") }
            return replace { $0.textKind = k }
        default:
            return .unchanged
        }
    }

    static func data2(_ t: String, _ e: EventRow, _ replace: ((inout EventRow) -> Void) -> ListChange) throws
        -> ListChange
    {
        switch e.kind {
        case .note:
            let v = try number(t, 0...127, "Velocity")
            return Velocity7.of(e.velocity) == v ? .unchanged : replace { $0.velocity = Velocity7.up(v) }
        case .cc, .channelPressure, .polyPressure:
            let v = try number(t, 0...127, "The value")
            return Value7.of(e.value) == v ? .unchanged : replace { $0.value = Int64(Value7.up(v)) }
        case .pitchBend:
            let v = try number(t, -8192...8191, "Pitch bend")
            return Bend14.of(e.value) == v ? .unchanged : replace { $0.value = Int64(Bend14.up(v)) }
        case .program:
            if t.isEmpty || t == "-" { return replace { $0.value2 = -1 } }
            let parts = t.split(separator: ":").map { Int($0.trimmingCharacters(in: .whitespaces)) }
            guard parts.count == 2, let m = parts[0], let l = parts[1], (0...127).contains(m), (0...127).contains(l)
            else { throw ListEditError("Type the bank as msb:lsb, such as 0:5, or clear it.") }
            return replace { $0.value2 = Int64(m * 128 + l) }
        case .text:
            return replace { $0.text = t }
        case .reference:
            let refusal = ListEditError("Type repeats and transpose, such as ×3 +2 or fill 0.")
            let words = t.split(separator: " ").map(String.init)
            guard (1...2).contains(words.count) else { throw refusal }
            let first = words[0].lowercased()
            let loops: UInt32
            if first == "fill" {
                loops = 0
            } else if let n = UInt32(first.replacingOccurrences(of: "×", with: "").replacingOccurrences(of: "x", with: "")),
                (1...10_000).contains(n)
            {
                loops = n
            } else {
                throw refusal
            }
            var transpose = e.refTranspose
            if words.count == 2 {
                guard let tr = Int(words[1].replacingOccurrences(of: "+", with: "")), (-127...127).contains(tr) else {
                    throw refusal
                }
                transpose = Int8(tr)
            }
            return replace {
                $0.refLoops = loops
                $0.refTranspose = transpose
            }
        case .audioRegion:
            let s = t.lowercased().replacingOccurrences(of: "db", with: "").trimmingCharacters(in: .whitespaces)
            guard let g = Float(s), g.isFinite, (-144...24).contains(g) else {
                throw ListEditError("Gain must be -144 to 24 dB.")
            }
            return replace { $0.gainDb = g }
        default:
            return .unchanged
        }
    }

    static func tempo(_ t: String, _ col: ListColumn, _ i: Int, _ p: TempoPointRow, _ c: ListContext) throws
        -> ListChange
    {
        guard var points = c.info?.tempo, points.indices.contains(i) else { return .unchanged }
        var n = p
        switch col {
        case .position:
            guard i > 0 else { return .unchanged }
            n.tick = try position(t, from: p.tick, c)
            if n.tick <= 0 || points.enumerated().contains(where: { $0.offset != i && $0.element.tick == n.tick }) {
                throw ListEditError("A tempo change is already at that position.")
            }
        case .data1:
            let s = t.lowercased().replacingOccurrences(of: "bpm", with: "").trimmingCharacters(in: .whitespaces)
            guard let bpm = Double(s), bpm.isFinite, (1...999).contains(bpm) else {
                throw ListEditError("Tempo must be 1 to 999 BPM.")
            }
            n.bpm = bpm
        case .data2:
            switch t.lowercased() {
            case "ramp": n.ramp = true
            case "jump", "": n.ramp = false
            default: throw ListEditError("Type ramp or jump.")
            }
        default:
            return .unchanged
        }
        guard n != p else { return .unchanged }
        points[i] = n
        points.sort { $0.tick < $1.tick }
        return .edit(.sequence(edit: .setTempo(seq: c.seq, tempo: points)), tick: n.tick)
    }

    static func meter(_ t: String, _ col: ListColumn, _ i: Int, _ m: MeterEntryRow, _ c: ListContext) throws
        -> ListChange
    {
        guard var entries = c.info?.meter, entries.indices.contains(i) else { return .unchanged }
        var n = m
        switch col {
        case .position:
            guard i > 0 else { return .unchanged }
            n.tick = try position(t, from: m.tick, c)
            if n.tick <= 0 || entries.enumerated().contains(where: { $0.offset != i && $0.element.tick == n.tick }) {
                throw ListEditError("A meter change is already at that position.")
            }
        case .data1:
            let parts = t.split(separator: "/").compactMap { UInt8($0.trimmingCharacters(in: .whitespaces)) }
            guard parts.count == 2, (1...64).contains(parts[0]), [1, 2, 4, 8, 16, 32, 64].contains(parts[1]) else {
                throw ListEditError("Type the meter as beats/unit, such as 6/8.")
            }
            n.num = parts[0]
            n.den = parts[1]
        case .data2:
            guard let k = KeyName.parse(t) else { throw ListEditError("Type a key such as D major or F# minor.") }
            n.sharps = k.sharps
            n.minor = k.minor
        default:
            return .unchanged
        }
        guard n != m else { return .unchanged }
        entries[i] = n
        entries.sort { $0.tick < $1.tick }
        // The core needs every meter change on a barline of the meters before it.
        for j in entries.indices.dropFirst() where MeterMath.bar(at: entries[j].tick, Array(entries[..<j])).start != entries[j].tick {
            throw ListEditError(
                col == .position
                    ? "Meter changes must start on a barline."
                    : "That meter would move a later meter change off its barline.")
        }
        return .edit(.sequence(edit: .setMeter(seq: c.seq, meter: entries)), tick: n.tick)
    }

    static func marker(_ t: String, _ col: ListColumn, _ i: Int, _ m: MarkerRow, _ c: ListContext) throws
        -> ListChange
    {
        guard var markers = c.info?.markers, markers.indices.contains(i) else { return .unchanged }
        var n = m
        switch col {
        case .position:
            n.tick = try position(t, from: m.tick, c)
        case .data2:
            guard !t.trimmingCharacters(in: .whitespaces).isEmpty else { throw ListEditError("A marker needs a name.") }
            n.name = t
        default:
            return .unchanged
        }
        guard n != m else { return .unchanged }
        markers[i] = n
        markers.sort { $0.tick < $1.tick }
        return .edit(.sequence(edit: .setMarkers(seq: c.seq, markers: markers)), tick: n.tick)
    }
}

/// New rows (Plan 4c Ruling 18).
public enum ListInsert {
    public static func blankRow(kind: EventKindRow, tick: Int64) -> EventRow {
        EventRow(
            id: 0, tick: tick, kind: kind, mute: false, dest: nil, groupChannel: nil, pitch: 0, velocity: 0,
            releaseVelocity: 0, duration: 0, value: 0, value2: 0, attribute: nil, perNote: [], refSeq: nil,
            refLength: nil, refLoops: 0, refTranspose: 0, media: nil, sourceOffset: 0, frames: 0, gainDb: 0,
            fadeIn: FadeRow(length: 0, shape: .linear), fadeOut: FadeRow(length: 0, shape: .linear), text: nil,
            textKind: nil, data: Data(), eightBit: false)
    }

    /// A new event row of `kind` with its defaults; nil for references,
    /// regions and map kinds.
    public static func row(_ kind: ListKind, tick: Int64) -> EventRow? {
        guard let k = kind.eventKind else { return nil }
        var r = blankRow(kind: k, tick: tick)
        switch kind {
        case .note:
            r.pitch = 60
            r.velocity = Velocity7.up(100)
            r.duration = MeterMath.ppq
        case .cc:
            r.value2 = 7
            r.value = Int64(Value7.up(64))
        case .pitchBend:
            r.value = Int64(Bend14.up(0))
        case .channelPressure:
            r.value = 0
        case .polyPressure:
            r.pitch = 60
        case .program:
            r.value2 = -1
        case .sysEx:
            r.data = Data([0x7E, 0x7F, 0x09, 0x01])
        case .text:
            r.text = "Text"
            r.textKind = .text
        default:
            return nil
        }
        return r
    }

    /// The command inserting `kind` at `tick`: into `lane` for events, into
    /// sequence `seq`'s maps otherwise; nil when it cannot be inserted here.
    public static func command(_ kind: ListKind, tick: Int64, lane: LaneKey?, seq: UInt64, info: SequenceInfo?)
        -> EditCommand?
    {
        switch kind {
        case .reference, .audioRegion:
            return nil
        case .tempo:
            guard var points = info?.tempo else { return nil }
            let bpm = points.last(where: { $0.tick <= tick })?.bpm ?? 120
            points.removeAll { $0.tick == tick }
            points.append(TempoPointRow(tick: tick, bpm: bpm, ramp: false))
            points.sort { $0.tick < $1.tick }
            return .sequence(edit: .setTempo(seq: seq, tempo: points))
        case .meter:
            guard var entries = info?.meter else { return nil }
            let bar = MeterMath.bar(at: tick, entries)
            let at = tick == bar.start ? tick : bar.start + bar.length
            guard !entries.contains(where: { $0.tick == at }) else { return nil }
            let m = bar.meter
            entries.append(MeterEntryRow(tick: at, num: m.num, den: m.den, sharps: m.sharps, minor: m.minor))
            entries.sort { $0.tick < $1.tick }
            return .sequence(edit: .setMeter(seq: seq, meter: entries))
        case .marker:
            var markers = info?.markers ?? []
            markers.append(MarkerRow(tick: tick, name: "Marker \(markers.count + 1)"))
            markers.sort { $0.tick < $1.tick }
            return .sequence(edit: .setMarkers(seq: seq, markers: markers))
        default:
            guard let lane, let r = row(kind, tick: tick) else { return nil }
            return .event(edit: .insert(seq: lane.seq, lane: lane.lane, events: [r]))
        }
    }
}

/// Deleting rows: one command per kind of row (events, tempo, meter,
/// markers); the first tempo point and meter entry stay.
public enum ListDelete {
    public static func commands(_ rows: [ListRow], seq: UInt64, info: SequenceInfo?) -> [EditCommand] {
        var out: [EditCommand] = []
        let events = rows.compactMap(\.event).map(\.id)
        if !events.isEmpty { out.append(.event(edit: .delete(events: events))) }
        guard let info else { return out }
        var tempo = Set<Int>()
        var meter = Set<Int>()
        var markers = Set<Int>()
        for r in rows {
            switch r.source {
            case .tempo(let i, _) where i > 0: tempo.insert(i)
            case .meter(let i, _) where i > 0: meter.insert(i)
            case .marker(let i, _): markers.insert(i)
            default: break
            }
        }
        if !tempo.isEmpty {
            let kept = info.tempo.enumerated().filter { !tempo.contains($0.offset) }.map(\.element)
            out.append(.sequence(edit: .setTempo(seq: seq, tempo: kept)))
        }
        if !meter.isEmpty {
            let kept = info.meter.enumerated().filter { !meter.contains($0.offset) }.map(\.element)
            out.append(.sequence(edit: .setMeter(seq: seq, meter: kept)))
        }
        if !markers.isEmpty {
            let kept = info.markers.enumerated().filter { !markers.contains($0.offset) }.map(\.element)
            out.append(.sequence(edit: .setMarkers(seq: seq, markers: kept)))
        }
        return out
    }
}
