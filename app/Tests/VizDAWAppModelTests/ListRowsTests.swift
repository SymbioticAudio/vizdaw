import Foundation
import Testing
import VizDAWCore
@testable import VizDAWAppModel

private let q: Int64 = 15_360
let listLaneA = LaneKey(3, 31)
let listLaneB = LaneKey(3, 32)

/// An event row of `kind` with `id` at `tick` (other fields blank).
func listEvent(_ id: UInt64, _ tick: Int64, _ kind: EventKindRow) -> EventRow {
    var r = ListInsert.blankRow(kind: kind, tick: tick)
    r.id = id
    return r
}

func listContext() -> ListContext {
    var info = fourFour(3)
    info.tempo.append(TempoPointRow(tick: 16 * q, bpm: 90, ramp: true))
    info.markers = [MarkerRow(tick: q, name: "Verse")]
    return ListContext(
        seq: 3, meter: info.meter, info: info, laneNames: [listLaneA: "Chords", listLaneB: "Bass"],
        destNames: [5: "Piano", 6: "Bass Synth"], seqNames: [2: "Groove"])
}

@Suite struct ListRowsTests {
    @Test func rowsAreSortedWithMapRowsFirst() {
        let a = ListLane(key: listLaneA, events: [listEvent(6, 0, .cc), listEvent(5, q, .note)], dest: 5)
        let b = ListLane(key: listLaneB, events: [listEvent(7, 0, .note)], dest: 6)
        let rows = ListRows.build(ListInput(lanes: [a, b], range: nil, info: listContext().info), filter: ListFilter())
        #expect(rows.map(\.id) == ["t0", "m0", "e6", "e7", "k0", "e5", "t1"])
        #expect(rows[2].lane == listLaneA && rows[3].lane == listLaneB)
    }

    @Test func blockRangesAndFiltersLeaveRowsOut() {
        var text = listEvent(8, 0, .text)
        text.text = "hi"
        text.textKind = .lyric
        let a = ListLane(key: listLaneA, events: [listEvent(6, 0, .cc), listEvent(5, q, .note), text], dest: 5)
        let b = ListLane(key: listLaneB, events: [listEvent(7, 0, .note)], dest: 6)
        let inBlock = ListRows.build(ListInput(lanes: [a], range: 0..<q, info: nil), filter: ListFilter())
        #expect(inBlock.map(\.id) == ["e6", "e8"], "the note at q starts after the block")
        let noCC = ListRows.build(ListInput(lanes: [a, b], range: nil, info: nil), filter: ListFilter(hiddenKinds: [.cc]))
        #expect(!noCC.map(\.id).contains("e6"))
        let piano = ListRows.build(ListInput(lanes: [a, b], range: nil, info: nil), filter: ListFilter(shownDests: [5]))
        #expect(piano.map(\.id) == ["e6", "e8", "e5"], "Bass's note is hidden; text ignores destinations")
        var routed = listEvent(9, 0, .note)
        routed.dest = 6
        let own = ListRows.build(
            ListInput(lanes: [ListLane(key: listLaneA, events: [routed], dest: 5)], range: nil, info: nil),
            filter: ListFilter(shownDests: [5]))
        #expect(own.isEmpty, "an event's own destination wins over its lane's")
        #expect(ListFilter(hiddenKinds: [.cc, .tempo]).viewFilter == ViewFilterRow(hiddenKinds: [.cc], hiddenLanes: []))
        #expect(ListFilter().viewFilter == nil)
    }

    @Test func optionalColumnsAndTheLaneColumn() {
        #expect(ListColumns().visible(lanes: 1) == [.position, .duration, .kind, .dest, .data1, .data2, .mute])
        var all = ListColumns()
        all.end = true
        all.release = true
        all.smpte = true
        #expect(
            all.visible(lanes: 2)
                == [.position, .end, .duration, .kind, .lane, .dest, .data1, .data2, .release, .smpte, .mute])
    }

    @Test func cellsShowEveryKind() {
        let c = listContext()
        func cells(_ e: EventRow) -> [String] {
            let r = ListRow(.event(lane: listLaneA, row: e))
            return [ListText.cell(r, .kind, c), ListText.cell(r, .data1, c), ListText.cell(r, .data2, c)]
        }
        var note = listEvent(1, q, .note)
        note.pitch = 60
        note.velocity = Velocity7.up(100)
        note.duration = q + q / 4
        let nr = ListRow(.event(lane: listLaneA, row: note))
        #expect(cells(note) == ["Note", "C4", "100"])
        #expect(ListText.cell(nr, .position, c) == "1.2.0")
        #expect(ListText.cell(nr, .duration, c) == "1.240")
        #expect(ListText.cell(nr, .end, c) == "1.3.240")
        #expect(ListText.cell(nr, .dest, c) == "Lane")
        #expect(ListText.cell(nr, .lane, c) == "Chords")
        #expect(ListText.summary(nr, c) == "Note C4, bar 1 beat 2, velocity 100")
        var cc = listEvent(2, 0, .cc)
        cc.value2 = 7
        cc.value = Int64(Value7.up(64))
        cc.dest = 6
        #expect(cells(cc) == ["Controller", "7", "64"])
        #expect(ListText.cell(ListRow(.event(lane: listLaneA, row: cc)), .dest, c) == "Bass Synth")
        var pb = listEvent(3, 0, .pitchBend)
        pb.value = Int64(Bend14.up(-100))
        #expect(cells(pb) == ["Pitch Bend", "", "-100"])
        var program = listEvent(4, 0, .program)
        program.value = 9
        program.value2 = 0 * 128 + 5
        #expect(cells(program) == ["Program", "9", "0:5"])
        var sysex = listEvent(5, 0, .sysEx)
        sysex.data = Data([0x7E, 0x7F, 0x09, 0x01])
        #expect(cells(sysex) == ["SysEx", "4 bytes", "7E 7F 09 01"])
        var text = listEvent(6, 0, .text)
        text.text = "verse"
        text.textKind = .lyric
        #expect(cells(text) == ["Lyric", "Lyric", "verse"])
        var reference = listEvent(7, 0, .reference)
        reference.refSeq = 2
        reference.refLoops = 3
        reference.refTranspose = 2
        #expect(cells(reference) == ["Reference", "Groove", "×3 +2"])
        reference.refLoops = 0
        reference.refTranspose = -1
        #expect(cells(reference)[2] == "fill -1")
        var region = listEvent(8, 0, .audioRegion)
        region.gainDb = -3.5
        #expect(cells(region) == ["Audio Region", "Audio", "-3.5 dB"])
        let info = c.info!
        let tempo = ListRow(.tempo(index: 1, point: info.tempo[1]))
        #expect([ListText.cell(tempo, .data1, c), ListText.cell(tempo, .data2, c)] == ["90.00", "ramp"])
        let meter = ListRow(.meter(index: 0, entry: MeterEntryRow(tick: 0, num: 6, den: 8, sharps: 2, minor: false)))
        #expect([ListText.cell(meter, .data1, c), ListText.cell(meter, .data2, c)] == ["6/8", "D major"])
        #expect(ListText.cell(ListRow(.marker(index: 0, marker: info.markers[0])), .data2, c) == "Verse")
        #expect(!ListText.editable(tempo, .kind) && ListText.editable(tempo, .position))
        #expect(!ListText.editable(ListRow(.tempo(index: 0, point: info.tempo[0])), .position), "the first tempo stays at 0")
        #expect(!ListText.editable(ListRow(.event(lane: listLaneA, row: sysex)), .data2))
    }

    @Test func buildingRowsForAHundredThousandEventsIsQuick() {
        let events = (0..<100_000).map { i in listEvent(UInt64(i), Int64(i) * 240, .note) }
        let start = Date()
        let rows = ListRows.build(
            ListInput(lanes: [ListLane(key: listLaneA, events: events, dest: 5)], range: nil, info: nil),
            filter: ListFilter())
        #expect(rows.count == 100_000)
        #expect(Date().timeIntervalSince(start) < 3, "debug build bound")
    }
}
