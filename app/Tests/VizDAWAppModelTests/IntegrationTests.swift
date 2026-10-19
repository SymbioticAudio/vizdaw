import Foundation
import Testing
import VizDAWCore
@testable import VizDAWAppModel

/// Real FFI, offline audio backend (no hardware needed).
@MainActor
@Suite struct IntegrationTests {
    @Test func demoPlaysThroughTheRealLibrary() async throws {
        setenv("VIZDAW_AUDIO", "offline", 1)
        let m = try DocumentModel(session: Session.newDemo())
        #expect(m.snapshot.sequences.count == 3)
        #expect(m.topLevel.map(\.name) == ["Song"])
        #expect(m.lanes.map(\.name) == ["Groove", "Chords"])
        m.playStop()
        #expect(m.alert == nil)
        try await Task.sleep(for: .milliseconds(200))
        m.poll()
        #expect(m.isPlaying)
        #expect(m.transport.tick > 0)
        #expect(m.transport.device == "Offline")
        m.playStop()
        // The audio thread applies Stop on its next block.
        try await Task.sleep(for: .milliseconds(50))
        m.poll()
        #expect(!m.isPlaying)
    }
}

/// The list editor through the real library.
@MainActor
@Suite struct ListIntegrationTests {
    @Test func aMixedListDeleteIsOneUndoStepThroughTheRealLibrary() throws {
        setenv("VIZDAW_AUDIO", "offline", 1)
        let m = try DocumentModel(session: Session.newDemo())
        let seq = try #require(m.selectedSeq ?? m.topLevel.first?.id)
        #expect(m.editor.edit(.sequence(edit: .setMarkers(seq: seq, markers: [MarkerRow(tick: 0, name: "A")]))))
        m.editor.openDetail(.sequence(seq), kind: .list)
        let l = m.list(.pane)
        let before = l.rows.map(\.id)
        let event = try #require(before.first { $0.hasPrefix("e") })
        #expect(before.contains("k0"))
        l.delete([event, "k0"])
        #expect(m.editor.status == nil)
        #expect(!l.rows.map(\.id).contains(event) && !l.rows.map(\.id).contains("k0"))
        #expect(m.undoTitle == "Undo Delete")
        m.undo()
        #expect(l.rows.map(\.id) == before, "one undo restores the events and the marker")
        try m.session.close()
    }
}

/// The mixer and recovery through the real library (offline audio; a
/// temporary support folder, never the user's).
@MainActor
@Suite struct MixerIntegrationTests {
    @Test func aBusAndASaveThroughTheRealLibrary() throws {
        setenv("VIZDAW_AUDIO", "offline", 1)
        let support = FileManager.default.temporaryDirectory
            .appendingPathComponent("vizdaw-swift-test-\(ProcessInfo.processInfo.processIdentifier)")
        try setSupportDir(path: support.path)
        let doc = try DocumentModel(session: Session.newDemo())
        let mixer = try MixerModel(document: doc)
        #expect(mixer.strips.map(\.name) == ["Built-in Synth"])
        #expect(mixer.master?.name == "Master")
        mixer.addBus()
        #expect(mixer.strips.map(\.name) == ["Built-in Synth", "Bus 1"])
        #expect(doc.undoTitle == "Undo Add Bus")
        let path = support.appendingPathComponent("mix.vizdaw").path
        doc.saveAs(path: path)
        #expect(doc.alert == nil)
        let info = try recoveryCheck(path: path)
        #expect(!info.crashed, "open here, not a crash")
        #expect(info.autosave == nil)
        try doc.session.close()
        try? FileManager.default.removeItem(at: support)
    }
}

/// Step entry through the real library: `state.get editor`'s step row
/// carries dotted, triplet and the steps taken (final review item 13).
@MainActor
@Suite struct StepIntegrationTests {
    @Test func theStepRowShowsDotTripletAndARealStep() throws {
        setenv("VIZDAW_AUDIO", "offline", 1)
        let m = try DocumentModel(session: Session.newDemo())
        #expect(StepCommands.run(StepCommands.entry, on: true, m) == .done)
        #expect(StepCommands.run("step.eighth", on: nil, m) == .done)
        #expect(StepCommands.run("step.dot", on: nil, m) == .done)
        #expect(StepCommands.run("step.triplet", on: nil, m) == .done)
        #expect(StepCommands.run(StepCommands.notes, on: nil, pitches: "C4 E4", m) == .done)
        let row = try #require(ControlStates.stepRow(m))
        #expect(row.value == "eighth" && row.dotted && row.triplet)
        #expect(row.steps == 1, "one chord entered")
        #expect(row.tick == stepLength(value: .eighth, dotted: true, triplet: true), "moved by its length")
        #expect(m.undoTitle == "Undo Step Entry")
        try m.session.close()
    }
}
