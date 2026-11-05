import AppKit
import Foundation
import Testing
import VizDAWCore
@testable import VizDAWAppModel
@testable import VizDAWEditors

/// The Track menu's commands (plan tracks Task 4; Rulings 4 and 5): the
/// menu, the keys and the control channel share one implementation.
@MainActor
@Suite struct TrackCommandsTests {
    func arg(_ name: String, _ v: String) -> ControlArg { ControlArg(name: name, value: .text(value: v)) }
    func arg(_ name: String, _ v: Int64) -> ControlArg { ControlArg(name: name, value: .int(value: v)) }

    /// Song (3) with tracks A (30) and B (31), shown.
    func demo() throws -> (DocumentModel, FakeSession) {
        let f = editorFake()
        let m = try DocumentModel(session: f)
        m.select(3)
        return (m, f)
    }

    func selectTrack(_ m: DocumentModel, _ lane: UInt64) {
        m.editor.setSelection(SelectionRow(timeRange: nil, lanes: [LaneKey(3, lane).ref], events: []))
    }

    func failure(_ o: ControlOutcome) -> ControlFailure? {
        if case .failed(let f) = o { return f }
        return nil
    }

    @Test func eachNewItemMakesItsKindBelowTheSelection() throws {
        let (m, f) = try demo()
        selectTrack(m, 30)
        let cases: [(String, TrackKindRow)] = [
            (TrackCommands.newAudio, .audio), (TrackCommands.newInstrument, .instrument), (TrackCommands.newMidi, .midi),
        ]
        for (id, kind) in cases {
            selectTrack(m, 30)
            #expect(TrackCommands.perform(id, m) == .done, "\(id)")
            let call = try #require(f.newTracks.last)
            #expect(call.seq == 3 && call.kind == kind && call.after == 30, "\(id)")
            let made = try #require(m.lanes.first { $0.kind == kind && $0.name.hasPrefix("Track") })
            #expect(m.selectedLane?.id == made.id, "\(id): the new track is selected")
            #expect(m.lanes.firstIndex { $0.id == made.id } == 1, "\(id): below A")
        }
        #expect(f.newTracks.count == 3)
    }

    @Test func trackItemsNeedASequence() throws {
        let none = try DocumentModel(session: FakeSession([]))
        #expect(none.selectedSeq == nil)
        for id in TrackCommands.ids.sorted() { #expect(!TrackCommands.enabled(id, none), "\(id)") }
        let id = UUID()
        let refused = TrackCommands.run(TrackCommands.newAudio, args: [], doc: id, none)
        #expect(failure(refused)?.message == "Select a sequence first.")

        let f = FakeSession([seq(3, "Song", topLevel: true)])
        f.laneTable = [3: []]
        let m = try DocumentModel(session: f)
        m.select(3)
        for id in [TrackCommands.newAudio, TrackCommands.newInstrument, TrackCommands.newMidi] {
            #expect(TrackCommands.enabled(id, m), "\(id)")
        }
        for id in [TrackCommands.duplicate, TrackCommands.delete, TrackCommands.rename] {
            #expect(!TrackCommands.enabled(id, m), "\(id)")
        }
        let (full, _) = try demo()
        selectTrack(full, 31)
        for id in TrackCommands.ids.sorted() { #expect(TrackCommands.enabled(id, full), "\(id)") }
    }

    @Test func aMidiTrackWithoutDestinationSaysSo() throws {
        let (m, _) = try demo()
        #expect(TrackCommands.perform(TrackCommands.newMidi, m) == .done)
        #expect(
            m.editor.status
                == "No external MIDI destination yet: choose one in the track's destination menu.")
        m.editor.clearStatus()
        #expect(TrackCommands.perform(TrackCommands.newInstrument, m) == .done)
        #expect(m.editor.status == nil, "an instrument track has its synth")
    }

    @Test func deleteTheOnlyTrackLeavesTheAddRow() throws {
        let f = FakeSession([seq(3, "Song", topLevel: true)])
        f.laneTable = [3: [lane(30, "Only")]]
        let m = try DocumentModel(session: f)
        m.select(3)
        selectTrack(m, 30)
        f.laneTable[3] = []  // what the core's Remove leaves
        #expect(TrackCommands.perform(TrackCommands.delete, m) == .done)
        #expect(f.edits.last == .lane(edit: .remove(seq: 3, lane: 30)))
        #expect(m.lanes.isEmpty)
        let h = LaneHeaderView(document: m)
        let labels = (h.accessibilityChildren() ?? []).compactMap { ($0 as? NSAccessibilityElement)?.accessibilityLabel() }
        #expect(labels == ["Add Track"])
    }

    @Test func trackCommandsRefuseHostileInput() throws {
        let (m, f) = try demo()
        let id = UUID()
        let run = { (cmd: String, args: [ControlArg]) in TrackCommands.run(cmd, args: args, doc: id, m) }
        selectTrack(m, 30)
        let before = f.edits.count
        #expect(failure(run(TrackCommands.delete, [arg("lane", 999)]))?.message == "There is no track 999 in this sequence.")
        #expect(failure(run(TrackCommands.duplicate, [arg("lane", 999)]))?.message == "There is no track 999 in this sequence.")
        #expect(failure(run(TrackCommands.newAudio, [arg("after", 999)]))?.message == "There is no track 999 in this sequence.")
        #expect(failure(run(TrackCommands.newAudio, [arg("seq", 999)]))?.message == "There is no sequence 999.")
        #expect(failure(run(TrackCommands.delete, [arg("seq", 999)]))?.message == "There is no sequence 999.")
        #expect(failure(run(TrackCommands.rename, [arg("seq", 999), arg("name", "x")]))?.message == "There is no sequence 999.")
        // Wrong types and out-of-range ids are named, never sent.
        #expect(failure(run(TrackCommands.delete, [arg("lane", "x")]))?.code == .invalidParams)
        #expect(failure(run(TrackCommands.delete, [arg("lane", -1)]))?.code == .invalidParams)
        #expect(failure(run(TrackCommands.rename, [arg("name", 5)]))?.code == .invalidParams)
        #expect(f.edits.count == before && f.newTracks.isEmpty && f.duplicated.isEmpty)

        // A blank name is the core's refusal, passed through.
        f.editError = .Model(message: "a track name cannot be empty")
        let blank = run(TrackCommands.rename, [arg("name", "   ")])
        #expect(failure(blank)?.message == "a track name cannot be empty")
        #expect(f.edits.isEmpty && m.lanes.map(\.name) == ["Groove", "Chords"])
        f.editError = nil
        // No name over the channel is the dialog it would have opened.
        #expect(failure(run(TrackCommands.rename, []))?.dialog == "Rename Track")

        // A duplicate the core refuses (a return track) shows why.
        let why = "Duplicate Track does not copy a Track Synth return track: duplicate its MIDI track and run Track Synth again."
        f.duplicateError = .Model(message: why)
        #expect(TrackCommands.perform(TrackCommands.duplicate, m) == .refused(why))
        #expect(m.editor.status == why)
        f.duplicateError = nil
        m.editor.clearStatus()

        // While recording: Busy's message, and nothing is sent.
        f.recordRow.recording = true
        m.record.refresh()
        #expect(m.record.isRecording)
        let busy = "Stop recording to change which tracks record."
        for cmd in [
            TrackCommands.delete, TrackCommands.duplicate, TrackCommands.newAudio, TrackCommands.newInstrument,
            TrackCommands.newMidi,
        ] {
            #expect(TrackCommands.perform(cmd, m) == .busy(busy), "\(cmd)")
            #expect(failure(run(cmd, []))?.code == .busy, "\(cmd)")
        }
        #expect(m.editor.status == busy)
        #expect(f.edits.isEmpty && f.newTracks.isEmpty && f.duplicated.isEmpty)
    }
}
