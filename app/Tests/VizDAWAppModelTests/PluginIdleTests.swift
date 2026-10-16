import Foundation
import Testing
import VizDAWCore
@testable import VizDAWAppModel

/// The plugin timer's step: gestures become undo steps, slot changes bump
/// the slot version, problems go to the fault popover (never an alert).
@MainActor
@Suite struct PluginIdleTests {
    @Test func aGestureSnapshotIsApplied() throws {
        let f = demoFake()
        let m = try DocumentModel(session: f)
        var next = f.snap
        next.version += 1
        next.undoLabel = "Change Plugin Parameter"
        f.idleUpdates = [
            IdleUpdate(snapshot: next, slotsChanged: false, paramsChanged: [7], problems: [])
        ]
        m.idle()
        #expect(m.undoTitle == "Undo Change Plugin Parameter")
        #expect(m.paramsChanged == [7])
        #expect(m.slotsVersion == 0)
    }

    @Test func slotChangesAndProblemsAreKept() throws {
        let f = demoFake()
        let m = try DocumentModel(session: f)
        f.idleUpdates = [
            IdleUpdate(
                snapshot: nil, slotsChanged: true, paramsChanged: [],
                problems: ["Strip — Diva: failed: load failed"])
        ]
        m.idle()
        m.idle()
        #expect(f.idleCalls == 2)
        #expect(m.slotsVersion == 1)
        #expect(m.alert == nil)
        #expect(m.hasReport)
        #expect(m.messages == ["Strip — Diva: failed: load failed"])
    }
}
