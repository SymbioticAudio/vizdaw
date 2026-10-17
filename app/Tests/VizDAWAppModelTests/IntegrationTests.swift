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
