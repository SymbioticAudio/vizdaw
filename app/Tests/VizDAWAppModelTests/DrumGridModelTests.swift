import Foundation
import Testing
import VizDAWCore
@testable import VizDAWAppModel

private let q: Int64 = 15_360
private let s16 = q / 4

func drumHit(_ id: UInt64, _ tick: Int64, velocity: UInt16 = 100 << 9) -> DrumHitRow {
    DrumHitRow(id: id, tick: tick, duration: s16, velocity: velocity, mute: false)
}

func drumRow(_ id: UInt64, _ name: String, _ note: UInt8, hits: [DrumHitRow] = [], velocity: Bool = false) -> DrumRowRow {
    DrumRowRow(
        id: id, name: name, note: note, dest: nil, destination: "Kit", showVelocity: velocity, hits: hits)
}

/// Sequence 3 of `editorFake()` with drum lanes "Drums" (40: Kick 1 with
/// hits 11 at 0 and 12 at a beat, Snare 2 with hit 13 at a beat) and
/// "Perc" (41: Clap 3).
func drumFake() -> FakeSession {
    let f = editorFake()
    f.laneTable[3] = [lane(30, "Groove"), lane(40, "Drums"), lane(41, "Perc")]
    f.snap.laneRevs += [LaneRev(seq: 3, lane: 40, rev: 4), LaneRev(seq: 3, lane: 41, rev: 5)]
    f.drumTable[3] = [
        DrumLaneRow(
            lane: 40, name: "Drums", destination: "Kit", mute: false, solo: false, playQuantize: nil, playShift: 0,
            rows: [
                drumRow(1, "Kick", 36, hits: [drumHit(11, 0), drumHit(12, q)]),
                drumRow(2, "Snare", 38, hits: [drumHit(13, q)]),
            ]),
        DrumLaneRow(
            lane: 41, name: "Perc", destination: "Kit", mute: false, solo: false, playQuantize: nil, playShift: 0,
            rows: [drumRow(3, "Clap", 39)]),
    ]
    return f
}

@MainActor
@Suite struct DrumGridModelTests {
    func make(_ target: EditorTarget = .lane(seq: 3, lane: 40)) throws -> (FakeSession, DocumentModel, DrumGridModel) {
        let f = drumFake()
        let m = try DocumentModel(session: f)
        m.editor.openDetail(target, kind: .drumGrid)
        f.calls = []
        return (f, m, m.drumGrid(.pane))
    }

    @Test func showsTheDrumLanesOfTheShownSequence() throws {
        let (f, m, g) = try make()
        defer { withExtendedLifetime(m) {} }
        #expect(g.lanes.map(\.name) == ["Drums", "Perc"])
        // Header, Kick, Snare, header, Clap, the new-lane band.
        #expect(g.layout.bands.count == 6)
        #expect(g.spot(30) == .row(lane: 0, row: 0, velocity: nil))
        #expect(g.spot(10) == .header(lane: 0))
        #expect(g.spot(1_000) == .newLane)
        #expect(g.currentLane?.lane == 40)
        _ = g.lanes
        _ = g.layout
        #expect(f.drumLanesCalls == 1, "read once per snapshot")
        #expect(m.drumGrid(.pane) === g, "made once per slot")
        #expect(m.drumGridForCommands() === g)
        // A torn window's grid pressed in is the one the menu acts on.
        let id = try #require(m.editor.tearOff(.drumGrid, shown: 3)?.id)
        let w = m.drumGrid(.window(id))
        w.becameActive()
        #expect(m.drumGridForCommands() === w)
        m.editor.openDetail(.lane(seq: 3, lane: 40), kind: .list)
        m.editor.closeWindow(id)
        #expect(m.drumGridForCommands() == nil, "only while a slot shows a drum grid")
    }

    @Test func aClickOnAnEmptyCellPaintsOneHit() throws {
        let (f, m, g) = try make()
        defer { withExtendedLifetime(m) {} }
        g.strokeBegan(lane: 0, row: 1, at: 2 * s16 + 5, snapped: 2 * s16, grab: 1, velocity: nil, erase: false)
        g.strokeEnded(at: 2 * s16 + 5, grab: 1, velocity: nil)
        #expect(f.calls == ["begin Paint Drum Hits", "gesture", "end"])
        #expect(
            f.gestureEdits.last
                == .drum(
                    edit: .paint(
                        seq: 3, lane: 40, row: 2, hits: [DrumPaintHitRow(tick: 2 * s16, velocity: 100 << 9)],
                        window: s16, duration: q / 8)))
        #expect(g.selectedRow == DrumRowKey(lane: 40, row: 2))
        #expect(g.stroke == nil)
    }

    @Test func aClickOnAHitErasesIt() throws {
        let (f, m, g) = try make()
        defer { withExtendedLifetime(m) {} }
        g.strokeBegan(lane: 0, row: 0, at: q + 9, snapped: q, grab: 1, velocity: nil, erase: false)
        g.strokeEnded(at: q + 9, grab: 1, velocity: nil)
        #expect(f.calls.first == "begin Erase Drum Hits")
        #expect(f.gestureEdits.last == .event(edit: .delete(events: [12])))
    }

    @Test func aDragPaintsARunAndTheVelocityLaneSetsEachHit() throws {
        let (f, m, g) = try make()
        defer { withExtendedLifetime(m) {} }
        g.updateSettings { $0.spacing = q / 2 }
        g.strokeBegan(lane: 0, row: 1, at: 0, snapped: 0, grab: 1, velocity: 20, erase: false)
        g.strokeMoved(at: 2 * q + 1, grab: 1, velocity: 90)
        let hits: [DrumPaintHitRow] = [
            DrumPaintHitRow(tick: 0, velocity: 20 << 9), DrumPaintHitRow(tick: q / 2, velocity: 90 << 9),
            DrumPaintHitRow(tick: q, velocity: 90 << 9), DrumPaintHitRow(tick: 3 * q / 2, velocity: 90 << 9),
            DrumPaintHitRow(tick: 2 * q, velocity: 90 << 9),
        ]
        #expect(
            f.gestureEdits.last
                == .drum(edit: .paint(seq: 3, lane: 40, row: 2, hits: hits, window: s16, duration: q / 8)))
        g.strokeEnded(at: q / 2 + 3, grab: 1, velocity: 127)
        #expect(
            f.gestureEdits.last
                == .drum(edit: .paint(seq: 3, lane: 40, row: 2, hits: Array(hits.prefix(2)), window: s16, duration: q / 8)),
            "back towards the start: the run shrinks, each hit keeps its velocity")
        #expect(f.calls.last == "end")
        #expect(m.editor.gesture == nil)
    }

    @Test func escapeAndRefusalsLeaveNoUndoStep() throws {
        let (f, m, g) = try make()
        defer { withExtendedLifetime(m) {} }
        g.strokeBegan(lane: 0, row: 1, at: 0, snapped: 0, grab: 1, velocity: nil, erase: false)
        g.strokeMoved(at: q, grab: 1, velocity: nil)
        g.strokeCancelled()
        #expect(f.calls == ["begin Paint Drum Hits", "gesture", "abort"])
        #expect(g.stroke == nil && m.editor.gesture == nil)
        // The eraser on an empty cell erases nothing: no gesture at all.
        f.calls = []
        g.strokeBegan(lane: 1, row: 0, at: 0, snapped: 0, grab: 1, velocity: nil, erase: true)
        g.strokeEnded(at: 0, grab: 1, velocity: nil)
        #expect(f.calls.isEmpty)
        // Another drag still finishing: the stroke is refused and says so.
        let sent = f.gestureEdits.count
        #expect(m.editor.beginGesture("Move", lanes: [LaneKey(3, 40)], origin: .other))
        g.strokeBegan(lane: 0, row: 1, at: 0, snapped: 0, grab: 1, velocity: nil, erase: false)
        g.strokeEnded(at: 0, grab: 1, velocity: nil)
        #expect(m.editor.status == EditorModel.gestureOpenMessage)
        #expect(f.gestureEdits.count == sent)
    }

    @Test func rowCommandsActOnTheSelectedRow() throws {
        let (f, m, g) = try make()
        defer { withExtendedLifetime(m) {} }
        #expect(!g.removeSelectedRow())
        #expect(m.editor.status == DrumGridModel.noRowMessage)
        #expect(g.addRow())
        #expect(f.edits.last == .drum(edit: .addRow(seq: 3, lane: 40, name: "Closed Hat", note: 42, dest: nil)))
        g.select(lane: 0, row: 1)
        #expect(g.selected?.row.name == "Snare")
        #expect(g.showVelocity(nil))
        #expect(f.edits.last == .drum(edit: .showVelocity(seq: 3, lane: 40, row: 2, show: true)))
        #expect(g.splitSelectedRow())
        #expect(f.edits.last == .drum(edit: .splitRow(seq: 3, lane: 40, row: 2)))
        #expect(g.removeSelectedRow())
        #expect(f.edits.last == .drum(edit: .removeRow(seq: 3, lane: 40, row: 2)))
        #expect(g.rename(DrumRowKey(lane: 40, row: 1), to: "  Bass Drum "))
        #expect(f.edits.last == .drum(edit: .renameRow(seq: 3, lane: 40, row: 1, name: "Bass Drum")))
        #expect(!g.rename(DrumRowKey(lane: 40, row: 1), to: "Kick"), "unchanged: no undo step")
        // Kick dropped on Clap's band moves it into Perc; far below splits it.
        #expect(g.drop(lane: 0, row: 0, atY: 90))
        #expect(f.edits.last == .drum(edit: .moveRowToLane(seq: 3, lane: 40, row: 1, toLane: 41)))
        #expect(g.drop(lane: 0, row: 0, atY: 500))
        #expect(f.edits.last == .drum(edit: .splitRow(seq: 3, lane: 40, row: 1)))
        let n = f.edits.count
        #expect(!g.drop(lane: 0, row: 0, atY: 30), "its own place")
        #expect(f.edits.count == n)
    }

    @Test func laneQuantizeNudgeAndDrumLaneMarking() throws {
        let (f, m, g) = try make()
        defer { withExtendedLifetime(m) {} }
        let swing = DrumGridModel.quantizeChoices.first { $0.title == "1/16 Swing 62%" }
        #expect(swing != nil)
        #expect(g.setQuantize(lane: 40, swing?.quantize))
        #expect(f.edits.last == .lane(edit: .setPlayQuantize(seq: 3, lane: 40, quantize: swing?.quantize)))
        #expect(g.quantizeTitle(g.lanes[0]) == "Off")
        #expect(g.nudge(lane: 40, steps: -2))
        #expect(f.edits.last == .lane(edit: .setPlayShift(seq: 3, lane: 40, shift: -240)))
        #expect(m.editor.markDrumLane(seq: 3, lane: 30, on: true))
        #expect(f.edits.last == .drum(edit: .markLane(seq: 3, lane: 30, drum: true)))
    }

    @Test func busyKeepsTheLanesAndAFailureShowsOnce() async throws {
        let (f, m, g) = try make()
        defer { withExtendedLifetime(m) {} }
        #expect(g.lanes.count == 2)
        f.readError = .Busy(message: "busy")
        nextSnapshot(f) { _ in }
        m.editor.edit(.drum(edit: .markLane(seq: 3, lane: 30, drum: true)))
        #expect(g.lanes.count == 2, "Busy keeps what was read")
        f.readError = .Model(message: "gone")
        nextSnapshot(f) { _ in }
        m.editor.edit(.drum(edit: .markLane(seq: 3, lane: 30, drum: false)))
        #expect(g.lanes.isEmpty)
        let calls = f.drumLanesCalls
        _ = g.lanes
        #expect(f.drumLanesCalls == calls, "not retried at the same version")
        await settle()
        #expect(m.editor.status == "gone")
    }

    @Test func aFailedExtendOnMouseUpAbortsTheGesture() throws {
        let (f, m, g) = try make()
        defer { withExtendedLifetime(m) {} }
        g.strokeBegan(lane: 0, row: 1, at: 0, snapped: 0, grab: 1, velocity: nil, erase: false)
        g.strokeMoved(at: q, grab: 1, velocity: nil)
        g.extendStroke = { _, _, _, _, _, _ in throw VizError.Model(message: "bad") }
        g.strokeEnded(at: q, grab: 1, velocity: nil)
        #expect(f.calls.last == "abort")
        #expect(g.stroke == nil && m.editor.gesture == nil)
    }

    @Test func aBusyReadThenAFreshOneDoesNotTrap() throws {
        let (f, m, g) = try make()
        defer { withExtendedLifetime(m) {} }
        let two = g.lanes
        #expect(two.count == 2)
        nextSnapshot(f) { _ in }
        m.editor.edit(.drum(edit: .markLane(seq: 3, lane: 30, drum: true)))
        var reads = 0
        f.drumReadHook = {
            reads += 1
            if reads == 1 { throw VizError.Busy(message: "busy") }
            return [two[0]]
        }
        // The first read is Busy (the two old lanes); every later one has one lane.
        g.select(lane: 1, row: 0)
        #expect(g.selectedRow == DrumRowKey(lane: 41, row: 3))
        // Fresh data has no lane 1 any more: refused, not a trap.
        g.strokeBegan(lane: 1, row: 0, at: 0, snapped: 0, grab: 1, velocity: nil, erase: false)
        #expect(g.stroke == nil)
        #expect(!g.drop(lane: 1, row: 0, atY: 0))
        #expect(g.selected == nil)
    }

    @Test func aFailureForgetsTheRowsReadBeforeIt() throws {
        let (f, m, g) = try make()
        defer { withExtendedLifetime(m) {} }
        #expect(g.lanes.count == 2)
        f.readError = .Model(message: "gone")
        nextSnapshot(f) { _ in }
        m.editor.edit(.drum(edit: .markLane(seq: 3, lane: 30, drum: true)))
        #expect(g.lanes.isEmpty)
        f.readError = .Busy(message: "busy")
        nextSnapshot(f) { _ in }
        m.editor.edit(.drum(edit: .markLane(seq: 3, lane: 30, drum: false)))
        #expect(g.lanes.isEmpty, "Busy after a failure must not bring the old rows back")
    }

    @Test func settingsDecodeTolerantly() throws {
        let json = #"{"drumGrid":{"view":"timeline","step":-5,"velocity":200,"rowHeight":1e9,"spacing":"x"}}"#
        let l = EditorLayout.decode(json)
        #expect(l.drumGrid.view == .timeline)
        #expect(l.drumGrid.step == 3_840 && l.drumGrid.velocity == 127 && l.drumGrid.rowHeight == 64)
        #expect(l.drumGrid.spacing == DrumGridSettings().spacing)
        // Step, spacing and duration are what Rust paints with (`Settings::sane`).
        let tiny = EditorLayout.decode(#"{"drumGrid":{"step":1,"spacing":-3,"duration":0}}"#).drumGrid
        #expect(tiny.step == 480 && tiny.spacing == 480 && tiny.duration == q / 8)
        let huge = EditorLayout.decode(#"{"drumGrid":{"step":1000000000000,"spacing":1000000000000000}}"#).drumGrid
        #expect(huge.step == 61_440 && huge.spacing == 16 * q)
        #expect(EditorLayout.decode("{}").drumGrid == DrumGridSettings())
        #expect(DrumGridSettings.lengths(including: 999).last?.1 == 999)
        #expect(DrumGridSettings.lengths(including: q).count == DrumGridSettings.lengths.count)
    }
}
