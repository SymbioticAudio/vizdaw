import Foundation
import Testing
import VizDAWCore
@testable import VizDAWAppModel

private func ext(_ id: UInt64, _ name: String = "Juno", mix: MidiMixRow = MidiMixRow(volume: nil, pan: nil, mute: false)) -> DestRow {
    var d = DestRow(id: id, name: name, plugin: nil, external: true)
    d.midiMix = mix
    return d
}

@MainActor
@Suite struct MidiStripTests {
    func model(_ f: FakeSession) throws -> MixerModel {
        try MixerModel(document: DocumentModel(session: f))
    }

    func fake() -> FakeSession {
        let f = mixerFake()
        f.dests = [ext(7)]
        return f
    }

    @Test func labelsAndParsing() {
        #expect(MidiStripMath.volumeLabel(nil) == "100")
        #expect(MidiStripMath.volumeLabel(90) == "90")
        #expect(MidiStripMath.panLabel(64) == "C")
        #expect(MidiStripMath.panLabel(0) == "L 64")
        #expect(MidiStripMath.panLabel(127) == "R 63")
        #expect(MidiStripMath.panLabel(nil) == "C")
        #expect(MidiStripMath.parseVolume("0") == 0)
        #expect(MidiStripMath.parseVolume(" 127 ") == 127)
        #expect(MidiStripMath.parseVolume("128") == nil)
        #expect(MidiStripMath.parseVolume("-1") == nil)
        #expect(MidiStripMath.parseVolume("x") == nil)
        #expect(MidiStripMath.parsePan("C") == 64)
        #expect(MidiStripMath.parsePan("c") == 64)
        #expect(MidiStripMath.parsePan("L 64") == 0)
        #expect(MidiStripMath.parsePan("L 1") == 63)
        #expect(MidiStripMath.parsePan("R 63") == 127)
        #expect(MidiStripMath.parsePan("R 10") == 74)
        #expect(MidiStripMath.parsePan("L 65") == nil)
        #expect(MidiStripMath.parsePan("R 64") == nil)
        #expect(MidiStripMath.parsePan("L 0") == nil)
        #expect(MidiStripMath.parsePan("R") == nil)
        #expect(MidiStripMath.parsePan("0") == 0)
        #expect(MidiStripMath.parsePan("127") == 127)
        #expect(MidiStripMath.parsePan("128") == nil)
        #expect(MidiStripMath.position(nil) == 100.0 / 127)
        #expect(MidiStripMath.position(127) == 1)
        #expect(MidiStripMath.value(at: 0) == 0)
        #expect(MidiStripMath.value(at: 1) == 127)
        #expect(MidiStripMath.value(at: 2) == 127)
        #expect(MidiStripMath.value(at: -1) == 0)
        #expect(MidiStripMath.value(at: .nan) == 0)
        for v: UInt8 in [0, 1, 64, 100, 127] {
            #expect(MidiStripMath.value(at: MidiStripMath.position(v)) == v)
        }
    }

    @Test func aDragPreviewsAndCommitsOnce() throws {
        let f = fake()
        let m = try model(f)
        m.midiDrag(7, volume: 10, pan: nil, now: 1.000)
        m.midiDrag(7, volume: 11, pan: nil, now: 1.005)
        m.midiDrag(7, volume: 12, pan: nil, now: 1.010)
        #expect(f.midiPreviews.count == 1)
        m.midiDrag(7, volume: 13, pan: nil, now: 1.035)
        #expect(f.midiPreviews.count == 2)
        #expect(f.midiPreviews.last?.1 == 13)
        #expect(f.midiMixes.isEmpty)
        m.midiRelease(7)
        #expect(f.midiMixes.count == 1)
        #expect(f.midiMixes[0].0 == 7)
        #expect(f.midiMixes[0].1 == MidiMixRow(volume: 13, pan: nil, mute: false))
        m.midiRelease(7)
        #expect(f.midiMixes.count == 1)
    }

    @Test func aClickCommitsOnce() throws {
        let f = fake()
        let m = try model(f)
        m.setMidiVolume(7, 55)
        #expect(f.midiMixes.count == 1)
        #expect(f.midiMixes[0].1 == MidiMixRow(volume: 55, pan: nil, mute: false))
        m.setMidiPan(7, 20)
        #expect(f.midiMixes.last?.1 == MidiMixRow(volume: 55, pan: 20, mute: false))
        m.toggleMidiMute(7)
        #expect(f.midiMixes.last?.1 == MidiMixRow(volume: 55, pan: 20, mute: true))
        #expect(f.midiMixes.count == 3)
    }

    @Test func anUndoDuringADraftDropsTheDraft() throws {
        let f = fake()
        let m = try model(f)
        m.midiDrag(7, volume: 30, pan: nil, now: 1)
        #expect(m.midiMix(7)?.volume == 30)
        f.dests[0].midiMix = MidiMixRow(volume: 80, pan: nil, mute: false)
        f.snap.version += 1
        m.document.refreshAfterJob()
        m.refreshIfNeeded()
        #expect(m.midiMix(7)?.volume == 80)
        m.toggleMidiMute(7)
        #expect(f.midiMixes.last?.1 == MidiMixRow(volume: 80, pan: nil, mute: true))
        m.midiRelease(7)
        #expect(f.midiMixes.count == 1, "the dropped draft does not commit")
    }

    @Test func aGoneDestinationDropsItsDraft() throws {
        let f = fake()
        let m = try model(f)
        m.midiDrag(7, volume: 30, pan: nil, now: 1)
        f.dests = []
        f.snap.version += 1
        m.document.refreshAfterJob()
        m.refreshIfNeeded()
        #expect(m.midiDraft.isEmpty)
        m.midiRelease(7)
        #expect(f.midiMixes.isEmpty)
        #expect(m.document.alert == nil)
    }

    @Test func aCancelledDragCommitsNothing() throws {
        let f = fake()
        let m = try model(f)
        m.midiDrag(7, volume: 30, pan: nil, now: 1)
        m.midiCancel(7)
        #expect(m.midiMix(7)?.volume == nil)
        m.midiRelease(7)
        m.toggleMidiMute(7)
        #expect(f.midiMixes.count == 1)
        #expect(f.midiMixes[0].1 == MidiMixRow(volume: nil, pan: nil, mute: true))
    }

    @Test func aDragThatEndsWhereItBeganCommitsNothing() throws {
        let f = fake()
        f.dests = [ext(7, mix: MidiMixRow(volume: 50, pan: nil, mute: false))]
        let m = try model(f)
        m.midiDrag(7, volume: nil, pan: nil, now: 1)
        m.midiDrag(7, volume: 50, pan: nil, now: 2)
        m.midiRelease(7)
        #expect(f.midiMixes.isEmpty)
    }

    @Test func aFailedCommitRaisesAnAlert() throws {
        let f = fake()
        let m = try model(f)
        f.synthError = .Busy(message: "x")
        m.setMidiVolume(7, 55)
        #expect(m.document.alert != nil)
        m.setMidiVolume(99, 55)
        #expect(f.midiMixes.isEmpty, "an unknown destination is ignored")
    }

    @Test func typedValuesAreParsedAndRangeChecked() throws {
        let f = fake()
        let m = try model(f)
        #expect(!m.typeMidiVolume(7, "200"))
        #expect(!m.typeMidiPan(7, "L 70"))
        #expect(f.midiMixes.isEmpty)
        #expect(m.typeMidiVolume(7, "90"))
        #expect(m.typeMidiPan(7, "R 10"))
        #expect(f.midiMixes.count == 2)
        #expect(f.midiMixes[1].1 == MidiMixRow(volume: 90, pan: 74, mute: false))
    }

    @Test func columnsPutMidiStripsBeforeBuses() throws {
        let f = mixerFake()
        f.dests = [ext(7), DestRow(id: 2, name: "Built-in Synth", plugin: nil), ext(8, "Moog")]
        let m = try model(f)
        let ids = m.columns.map(\.id)
        #expect(ids == ["s1", "d7", "d8", "s2"])
        #expect(Set(ids).count == ids.count)
        #expect(!ids.contains("s0"), "master is excluded")
    }

    @Test func aMidiTrackFocusesItsMidiStrip() {
        func lane(_ dest: UInt64?, kind: TrackKindRow) -> LaneRow {
            var l = FakeLane.make(9, kind: kind)
            l.midiDest = dest
            return l
        }
        let dests = [ext(3), DestRow(id: 4, name: "Piano", plugin: PluginRefRow(format: .clap, id: "x", name: "P", vendor: "V", version: "1"))]
        let strips = [stripRow(12, "Piano", kind: .instrument(dest: 4))]
        #expect(StripFocus.strip(selected: [lane(3, kind: .midi)], armed: [], destinations: dests, strips: strips)
            == .midi(dest: 3, lane: 9))
        #expect(StripFocus.strip(selected: [lane(nil, kind: .midi)], armed: [], destinations: dests, strips: strips)
            == .midi(dest: nil, lane: 9))
        #expect(StripFocus.strip(selected: [lane(4, kind: .instrument)], armed: [], destinations: dests, strips: strips)
            == .strip(12))
        #expect(StripFocus.strip(selected: [lane(nil, kind: .audio)], armed: [], destinations: dests, strips: strips)
            == .inspector)
    }

    @Test func midiLaneFollowsTheFxLaneRule() throws {
        let f = fake()
        var a = lane(30, "A")
        a.midiDest = 7
        var b = lane(31, "B")
        b.midiDest = 7
        var other = lane(32, "C")
        other.midiDest = 8
        f.laneTable[3] = [other, a, b]
        let m = try model(f)
        m.document.select(3)
        #expect(m.midiLane(for: 7, in: .window)?.id == 30)
        b.arm = true
        f.laneTable[3] = [other, a, b]
        f.snap.version += 1
        m.document.refreshAfterJob()
        #expect(m.midiLane(for: 7, in: .window)?.id == 31)
        #expect(m.midiLane(for: 7, in: .strip)?.id == 31, "the left pane follows the focused (armed) track")
        #expect(m.midiLane(for: 8, in: .strip) == nil, "...and only when it plays that destination")
        #expect(m.midiLane(for: 9, in: .window) == nil)
    }

    @Test func midiActivityDecays() throws {
        let f = fake()
        let m = try model(f)
        f.levelsValue.midi = [MidiLevelRow(dest: 7, velocity: 127)]
        m.pollLevels()
        #expect(m.midiActivity[7] == 1)
        f.levelsValue.midi = []
        m.pollLevels()
        let second = try #require(m.midiActivity[7])
        #expect(abs(second - Double(MixerMath.meterDecay)) < 1e-6)
        for _ in 0..<200 { m.pollLevels() }
        #expect(m.midiActivity[7] == nil, "a spent activity is dropped")
        f.levelsValue.midi = [MidiLevelRow(dest: 7, velocity: 64)]
        m.pollLevels()
        #expect(abs((m.midiActivity[7] ?? 0) - 64.0 / 127) < 1e-6)
    }

    // MARK: What the views ask (Task 6)

    @Test func faderScaleAndLabelsAreTheCcValues() {
        #expect(MidiStripMath.faderMarks.map(\.label) == ["127", "100", "64", "32", "0"])
        let positions: [Double] = [127, 100, 64, 32, 0].map { $0 / 127 }
        #expect(MidiStripMath.faderMarks.map(\.position) == positions)
        #expect(MidiStripMath.panPosition(nil) == 64.0 / 127, "an unset pan sits at the centre")
        #expect(MidiStripMath.panPosition(0) == 0)
        #expect(MidiStripMath.panPosition(127) == 1)
        #expect(MidiStripMath.kindLabel(channel: 0) == "MIDI \u{00B7} Ch 1")
        #expect(MidiStripMath.kindLabel(channel: 15) == "MIDI \u{00B7} Ch 16")
        #expect(MidiStripMath.kindLabel(channel: nil) == "MIDI")
        #expect(MidiStripMath.noDestination == "No destination")
        #expect(!MidiStripMath.volumeHint.lowercased().contains("lane"))
        #expect(!MidiStripMath.panHint.lowercased().contains("lane"))
    }

    @Test func midiFxKeyNeverMeetsAStripID() {
        #expect(MixerModel.midiFxKey(dest: 7, lane: 30) != 7)
        #expect(MixerModel.midiFxKey(dest: 7, lane: 30) == MixerModel.midiFxKey(dest: 7, lane: 31),
                "a destination's strip keeps its key whichever track it edits")
        #expect(MixerModel.midiFxKey(dest: nil, lane: 30) != 30)
        #expect(MixerModel.midiFxKey(dest: nil, lane: 30) != MixerModel.midiFxKey(dest: 30, lane: 1))
    }

    @Test func armAndDestinationEditTheTrack() throws {
        let f = fake()
        var a = lane(30, "A")
        a.midiDest = 7
        f.laneTable[3] = [a]
        let m = try model(f)
        m.document.select(3)
        m.toggleMidiArm(a)
        #expect(f.edits.last == .lane(edit: .setFlags(seq: 3, lane: 30, mute: nil, solo: nil, arm: true)))
        m.setMidiDest(a, nil)
        #expect(f.edits.last == .lane(edit: .setDest(seq: 3, lane: 30, dest: nil)))
        let n = f.edits.count
        m.setMidiDest(a, 7)
        #expect(f.edits.count == n, "the destination it has: no edit")
    }
}

private enum FakeLane {
    static func make(_ id: UInt64, kind: TrackKindRow) -> LaneRow { lane(id, "L\(id)", kind: kind) }
}
