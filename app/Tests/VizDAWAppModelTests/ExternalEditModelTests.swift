import Foundation
import Testing
import VizDAWCore

@testable import VizDAWAppModel

/// Stamps the tests set by path.
final class FakeProbe: FileProbe {
    var stamps: [String: FileStamp] = [:]

    func stamp(_ path: String) -> FileStamp? { stamps[path] }
}

/// One document's external edits (plan 5f-3 Task 4; Rulings 4, 5, 7;
/// pre-flight I7, I8, I10, I11, M9, M10), through the document's own
/// model and tick (`m.external`, `m.idle()`).
@MainActor
@Suite struct ExternalEditModelTests {
    let clock = FakeClock()
    let probe = FakeProbe()
    let opener = FakeOpener()
    let watching = FakeWatching()
    let path = "/tmp/p.vizdaw/Media/Edits/Kick edit 1.wav"

    func model() throws -> (DocumentModel, FakeSession) {
        let f = editorFake()
        f.jobStartState = .running
        let m = try DocumentModel(session: f)
        m.externalTools = ExternalTools(opener: opener, watching: watching, probe: probe, now: { [clock] in clock.t })
        probe.stamps[path] = stamp(1000)
        return (m, f)
    }

    func stamp(_ size: UInt64) -> FileStamp { FileStamp(size: size, modified: clock.t) }

    /// The editor saves: a new stamp and its watch event.
    func save(_ size: UInt64) {
        probe.stamps[path] = stamp(size)
        watching.fire(path)
    }

    @Test func openWritesOpensAndWatches() throws {
        let (m, f) = try model()
        let app = URL(fileURLWithPath: "/Applications/Editor.app")
        #expect(try m.external.open(7, app: app) == path)
        #expect(f.externalOpened == [7])
        #expect(opener.opened == [FakeOpener.Opened(file: URL(fileURLWithPath: path), app: app)])
        #expect(watching.started == [path] && watching.live == 1 && m.external.watched == [7])
        clock.advance(5)
        m.idle()
        #expect(f.externalApplies.isEmpty, "nothing changed yet")
    }

    @Test func aStableChangeStartsOneImport() throws {
        let (m, f) = try model()
        try m.external.open(7, app: nil)
        clock.advance(0.1)
        save(2000)
        clock.advance(0.5)
        m.idle()
        #expect(f.externalApplies.isEmpty, "not stable for a second yet")
        clock.advance(0.6)
        m.idle()
        #expect(f.externalApplies == [FakeSession.ExternalApply(event: 7, path: path)])
        #expect(m.external.importing == [7])
        #expect(m.banners.contains(.job(id: 5001, kind: .externalEdit, progress: 0.4)), "the job's banner")
        clock.advance(2)
        m.idle()
        m.idle()
        #expect(f.externalApplies.count == 1, "one import while it runs")
        f.jobRows[5001]?.state = .done
        m.idle()
        clock.advance(2)
        m.idle()
        #expect(f.externalApplies.count == 1, "one change is applied once")
        #expect(m.external.importing.isEmpty && m.alert == nil)
    }

    @Test func aSaveDuringAnImportIsAppliedAfterIt() throws {
        let (m, f) = try model()
        try m.external.open(7, app: nil)
        save(2000)
        clock.advance(1)
        m.idle()
        #expect(f.externalApplies.count == 1)
        clock.advance(0.2)
        save(3000)
        clock.advance(1.5)
        m.idle()
        #expect(f.externalApplies.count == 1, "deferred while the import runs")
        f.jobRows[5001]?.state = .done
        m.idle()
        #expect(f.externalApplies.count == 2, "then picked up: stable for over a second")
        f.jobRows[5002]?.state = .done
        clock.advance(3)
        m.idle()
        #expect(f.externalApplies.count == 2)
    }

    @Test func reloadReChecks() throws {
        let (m, f) = try model()
        try m.external.open(7, app: nil)
        #expect(m.external.reload(7) == .unchanged)
        probe.stamps[path] = stamp(2000)  // changed, no event, no wait
        #expect(m.external.reload(7) == .importing(5001))
        #expect(f.externalApplies.count == 1)
        #expect(m.external.reload(7) == .busy(ExternalEditModel.importingMessage))
        f.jobRows[5001]?.state = .done
        #expect(m.external.reload(7) == .unchanged, "the import's stamp is applied")
    }

    @Test func reloadRightAfterOpenIsUnchanged() throws {
        let (m, f) = try model()
        try m.external.open(7, app: nil)
        watching.fire(path)  // the editor touched the file, nothing changed
        clock.advance(3)
        m.idle()
        #expect(m.external.reload(7) == .unchanged)
        #expect(f.externalApplies.isEmpty, "the copy Open wrote is not imported back")
    }

    @Test func reloadAfterReopenSaysTheCopyIsGone() throws {
        let (m, f) = try model()
        f.externalStates[7] = .gone
        #expect(m.external.reload(7) == .failed(ExternalEditModel.goneMessage))
        f.externalStates[7] = .noEdit
        #expect(m.external.reload(7) == .unavailable(ExternalEditModel.noEditMessage))
        f.externalStates[7] = .copy(row: ExternalEditRow(event: 7, path: path, from: 1))
        probe.stamps[path] = nil
        #expect(m.external.reload(7) == .failed(ExternalEditModel.goneMessage), "the file is not on disk")
        #expect(f.externalApplies.isEmpty && f.externalOpened.isEmpty)
    }

    @Test func closingStopsEveryWatch() throws {
        let (m, f) = try model()
        let other = "/tmp/p.vizdaw/Media/Edits/Snare edit 1.wav"
        try m.external.open(7, app: nil)
        f.externalRow = ExternalEditRow(event: 8, path: other, from: 2)
        probe.stamps[other] = stamp(500)
        try m.external.open(8, app: nil)
        #expect(watching.live == 2)
        m.stopExternalEdits()
        #expect(watching.live == 0 && m.external.watched.isEmpty)
        save(2000)
        clock.advance(2)
        m.idle()
        #expect(f.externalApplies.isEmpty)
        // Through the store: a document's close stops its watches (pre-flight I7(b)).
        let store = DocumentStore(makeEmpty: { FakeSession([]) }, makeDemo: { demoFake() }, open: { _ in demoFake() })
        store.externalTools = ExternalTools(opener: opener, watching: watching, probe: probe, now: { [clock] in clock.t })
        let id = try #require(store.newDemo())
        let doc = try #require(store.model(id))
        try doc.external.open(5, app: nil)
        #expect(watching.live == 1)
        store.close(id)
        #expect(watching.live == 0)
    }

    @Test func openFailuresAreReported() throws {
        let (m, f) = try model()
        f.externalError = .Model(message: "Event 9 is not an audio region; select one audio region.")
        #expect(throws: VizError.self) { try m.external.open(9, app: nil) }
        f.externalError = nil
        opener.error = .Io(message: "/Applications/Nope.app is not an application.")
        #expect(throws: VizError.self) { try m.external.open(7, app: nil) }
        #expect(watching.started.isEmpty && opener.opened.isEmpty, "nothing is watched or opened")
        #expect(m.external.reload(7) == .unchanged, "a failed launch's copy is not imported back (fold check M6)")
        #expect(f.externalApplies.isEmpty)
        opener.error = nil
        opener.lateFailure = "The application could not open the file."
        try m.external.open(7, app: nil)
        #expect(m.alert?.title == ExternalEditModel.openFailedTitle)
        #expect(m.alert?.message == "The application could not open the file.")
        #expect(watching.live == 1, "the watch stays: the copy can be opened by hand")
    }

    @Test func aCancelledImportStaysArmed() throws {
        let (m, f) = try model()
        try m.external.open(7, app: nil)
        save(2000)
        clock.advance(1)
        m.idle()
        f.jobRows[5001]?.state = .cancelled
        m.idle()
        clock.advance(3)
        m.idle()
        #expect(f.externalApplies.count == 1, "the cancelled change is not retried by itself")
        #expect(watching.live == 1 && m.alert == nil)
        clock.advance(0.1)
        save(3000)
        clock.advance(1)
        m.idle()
        #expect(f.externalApplies.count == 2, "the next save imports")
        f.jobRows[5002]?.state = .cancelled
        m.idle()
        #expect(m.external.reload(7) == .importing(5003), "Reload retries it")
    }

    @Test func aDeletedRegionStopsItsWatch() throws {
        let (m, f) = try model()
        try m.external.open(7, app: nil)
        save(2000)
        clock.advance(1)
        m.idle()
        f.jobRows[5001] = JobRow(
            id: 5001, kind: .externalEdit, progress: 1, state: .failed,
            message: "The region was deleted; the edit was not applied.", problems: [], created: [])
        f.externalStates[7] = .noEdit  // the core forgot the copy
        m.idle()
        #expect(m.alert?.title == "External Edit Failed")
        #expect(m.alert?.message == "The region was deleted; the edit was not applied.")
        #expect(watching.live == 0 && m.external.watched.isEmpty)
        m.alert = nil
        save(3000)
        clock.advance(2)
        m.idle()
        #expect(f.externalApplies.count == 1 && m.alert == nil, "later saves raise nothing")
    }

    @Test func aBusyImportIsRetried() throws {
        let (m, f) = try model()
        try m.external.open(7, app: nil)
        f.applyError = .Busy(message: "Wait for the media job to finish.")
        save(2000)
        clock.advance(1)
        m.idle()
        #expect(f.externalApplies.isEmpty && m.alert == nil, "a busy core is no failure")
        #expect(m.editor.status == ExternalEditModel.waitingMessage, "the wait is said once")
        f.applyError = nil
        m.idle()
        #expect(f.externalApplies.count == 1, "retried at the next tick")
        f.applyError = .Model(message: "This region has no external edit; use Open in External Editor.")
        f.jobRows[5001]?.state = .done
        m.idle()
        save(3000)
        clock.advance(1)
        m.idle()
        #expect(m.alert?.title == "External Edit Failed", "another refusal is an alert")
        m.alert = nil
        m.idle()
        #expect(m.alert == nil, "once")
    }
}
