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
