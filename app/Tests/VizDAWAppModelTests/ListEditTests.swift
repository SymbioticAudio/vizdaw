import Foundation
import Testing
import VizDAWCore
@testable import VizDAWAppModel

private let q: Int64 = 15_360
private let bar: Int64 = 4 * q

private func noteRowAt(_ tick: Int64) -> ListRow {
    var n = listEvent(1, tick, .note)
    n.pitch = 60
    n.velocity = Velocity7.up(64)
    n.duration = q
    return ListRow(.event(lane: listLaneA, row: n))
}

private func commit(_ text: String, _ col: ListColumn, _ r: ListRow, _ c: ListContext = listContext()) throws
    -> ListChange
{
    try ListEdit.commit(text, column: col, row: r, context: c)
}

private func replaced(_ r: ListRow, _ change: (inout EventRow) -> Void) -> ListChange {
    var n = r.event!
    change(&n)
    return .edit(.event(edit: .replace(event: n.id, row: n)), tick: n.tick)
}

private func message(_ body: () throws -> ListChange) -> String? {
    do {
        _ = try body()
        return nil
    } catch let e as ListEditError {
        return e.message
    } catch {
        return "\(error)"
    }
}

@Suite struct ListEditTests {
    @Test func typingABarNumberMovesToItsDownbeat() throws {
        let r = noteRowAt(q + 100)
        #expect(
            try commit("5", .position, r)
                == .edit(.event(edit: .move(events: [1], delta: 4 * bar - (q + 100), toLane: nil)), tick: 4 * bar))
        // q + 100 is 1.2.6; ".3" changes the beat only: 1.3.6 = 2q + 96.
        #expect(
            try commit(".3", .position, r)
                == .edit(.event(edit: .move(events: [1], delta: q - 4, toLane: nil)), tick: 2 * q + 96))
        #expect(try commit("1.2.6", .position, r) == .unchanged)
    }

    @Test func committingShownTextSendsNoEdit() throws {
        let r = noteRowAt(q + 100)
        let c = listContext()
        // The position shows 1.2.6 (the 4 ticks off the unit grid are not shown); typing it back must not move.
        for col in [ListColumn.position, .end, .duration, .data1, .data2, .release, .kind] {
            #expect(try commit(ListText.cell(r, col, c), col, r, c) == .unchanged, "\(col)")
        }
    }

    @Test func noteCellsReplaceTheEvent() throws {
        let r = noteRowAt(0)
        #expect(try commit("D4", .data1, r) == replaced(r) { $0.pitch = 62 })
        #expect(try commit("100", .data2, r) == replaced(r) { $0.velocity = Velocity7.up(100) })
        #expect(try commit("64", .data2, r) == .unchanged, "the shown value keeps its 16 bits")
        #expect(try commit("2.0", .duration, r) == replaced(r) { $0.duration = 2 * q })
        #expect(try commit("1.3", .end, r) == replaced(r) { $0.duration = 2 * q })
        #expect(try commit("90", .release, r) == replaced(r) { $0.releaseVelocity = Velocity7.up(90) })
        #expect(try commit("M", .mute, r) == .edit(.event(edit: .setMute(events: [1], mute: true)), tick: 0))
        #expect(try commit("bass synth", .dest, r) == .edit(.event(edit: .setDest(events: [1], dest: 6)), tick: 0))
        #expect(try commit("", .dest, r) == .unchanged, "already the lane's")
    }

    @Test func otherKindsEditTheirFields() throws {
        let cc = ListRow(.event(lane: listLaneA, row: listEvent(2, 0, .cc)))
        #expect(try commit("11", .data1, cc) == replaced(cc) { $0.value2 = 11 })
        #expect(try commit("127", .data2, cc) == replaced(cc) { $0.value = Int64(Value7.up(127)) })
        let pb = ListRow(.event(lane: listLaneA, row: listEvent(3, 0, .pitchBend)))
        #expect(try commit("-8192", .data2, pb) == .unchanged, "0 is the lowest bend")
        #expect(try commit("0", .data2, pb) == replaced(pb) { $0.value = Int64(Bend14.up(0)) })
        var p = listEvent(4, 0, .program)
        p.value2 = -1
        let program = ListRow(.event(lane: listLaneA, row: p))
        #expect(try commit("1:2", .data2, program) == replaced(program) { $0.value2 = 130 })
        var t = listEvent(5, 0, .text)
        t.text = "a"
        t.textKind = .text
        let text = ListRow(.event(lane: listLaneA, row: t))
        #expect(try commit("Cue", .data1, text) == replaced(text) { $0.textKind = .cue })
        #expect(try commit("b", .data2, text) == replaced(text) { $0.text = "b" })
        var rf = listEvent(6, 0, .reference)
        rf.refLoops = 1
        let reference = ListRow(.event(lane: listLaneA, row: rf))
        #expect(
            try commit("×3 +2", .data2, reference)
                == replaced(reference) {
                    $0.refLoops = 3
                    $0.refTranspose = 2
                })
        #expect(try commit("fill", .data2, reference) == replaced(reference) { $0.refLoops = 0 })
        let region = ListRow(.event(lane: listLaneA, row: listEvent(7, 0, .audioRegion)))
        #expect(try commit("-6 dB", .data2, region) == replaced(region) { $0.gainDb = -6 })
    }

    @Test func badTextIsRefusedWithAMessage() {
        let r = noteRowAt(0)
        #expect(message { try commit("abc", .position, r) }?.contains("not a position") == true)
        #expect(message { try commit("300", .data2, r) } == "Velocity must be 0 to 127.")
        #expect(message { try commit("H9", .data1, r) }?.contains("not a pitch") == true)
        #expect(message { try commit("0.4.0", .end, r) } == "The end is before the start.")
        #expect(message { try commit("Organ", .dest, r) } == "No destination is named \"Organ\".")
        #expect(message { try commit("maybe", .mute, r) } == "Type M to mute, or clear the cell.")
        let c = listContext()
        let tempo = ListRow(.tempo(index: 1, point: c.info!.tempo[1]))
        #expect(message { try commit("0", .data1, tempo) } == "Tempo must be 1 to 999 BPM.")
        let region = ListRow(.event(lane: listLaneA, row: listEvent(7, 0, .audioRegion)))
        #expect(message { try commit("+30 dB", .data2, region) } == "Gain must be -144 to 24 dB.")
    }

    @Test func mapRowsReplaceTheirWholeMap() throws {
        let c = listContext()
        let info = c.info!
        let tempo = ListRow(.tempo(index: 1, point: info.tempo[1]))
        var t = info.tempo
        t[1].bpm = 100
        #expect(try commit("100", .data1, tempo) == .edit(.sequence(edit: .setTempo(seq: 3, tempo: t)), tick: 16 * q))
        let meter = ListRow(.meter(index: 0, entry: info.meter[0]))
        var m = info.meter
        m[0].num = 6
        m[0].den = 8
        #expect(try commit("6/8", .data1, meter) == .edit(.sequence(edit: .setMeter(seq: 3, meter: m)), tick: 0))
        m = info.meter
        m[0].sharps = 3
        m[0].minor = true
        #expect(try commit("F# minor", .data2, meter) == .edit(.sequence(edit: .setMeter(seq: 3, meter: m)), tick: 0))
        let marker = ListRow(.marker(index: 0, marker: info.markers[0]))
        #expect(
            try commit("3", .position, marker)
                == .edit(.sequence(edit: .setMarkers(seq: 3, markers: [MarkerRow(tick: 2 * bar, name: "Verse")])), tick: 2 * bar))
        let first = ListRow(.tempo(index: 0, point: info.tempo[0]))
        #expect(try commit("2", .position, first) == .unchanged, "the first tempo stays at 0")
    }

    @Test func typedSmpteRoundsToADisplayUnit() throws {
        var c = listContext()
        c.fps = 25
        // 1.48 s at 120 BPM is 45 465.6 ticks; a unit is 16 ticks: 45 472 is 2 841.75 units → 45 472.
        c.tickAtSeconds = { s in Int64((s * 30_720).rounded()) }
        let r = noteRowAt(0)
        #expect(
            try commit("00:00:01:12", .smpte, r, c)
                == .edit(.event(edit: .move(events: [1], delta: 45_472, toLane: nil)), tick: 45_472))
    }

    @Test func insertDefaultsAndDeleteCommands() {
        let c = listContext()
        guard case .event(.insert(_, _, let rows))? = ListInsert.command(.note, tick: q, lane: listLaneA, seq: 3, info: c.info)
        else { Issue.record("no insert"); return }
        #expect(rows.count == 1 && rows[0].pitch == 60 && rows[0].velocity == Velocity7.up(100) && rows[0].duration == q)
        #expect(ListInsert.command(.reference, tick: 0, lane: listLaneA, seq: 3, info: c.info) == nil)
        #expect(ListInsert.command(.note, tick: 0, lane: nil, seq: 3, info: c.info) == nil)
        guard case .sequence(.setMeter(_, let meter))? = ListInsert.command(.meter, tick: q, lane: nil, seq: 3, info: c.info)
        else { Issue.record("no meter"); return }
        #expect(meter.map(\.tick) == [0, bar], "the next barline")
        guard case .sequence(.setMarkers(_, let markers))? = ListInsert.command(.marker, tick: 0, lane: nil, seq: 3, info: c.info)
        else { Issue.record("no marker"); return }
        #expect(markers.map(\.name) == ["Marker 2", "Verse"])
        let doomed = [
            noteRowAt(0), ListRow(.tempo(index: 0, point: c.info!.tempo[0])),
            ListRow(.tempo(index: 1, point: c.info!.tempo[1])), ListRow(.marker(index: 0, marker: c.info!.markers[0])),
        ]
        #expect(
            ListDelete.commands(doomed, seq: 3, info: c.info) == [
                .event(edit: .delete(events: [1])),
                .sequence(edit: .setTempo(seq: 3, tempo: [c.info!.tempo[0]])),
                .sequence(edit: .setMarkers(seq: 3, markers: [])),
            ])
    }
}
