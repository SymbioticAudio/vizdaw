import AppKit
import Testing
import VizDAWCore
@testable import VizDAWAppModel
@testable import VizDAWEditors

/// Song's rows in the arrangement: Groove (30) is row 0, Chords (31) row 1;
/// the Chords block spans x 0–160 at the default zoom.
private let onChords = CGPoint(x: 50, y: 70)

@MainActor
private func arrangement(_ f: FakeSession) throws -> (DocumentModel, ArrangementView) {
    let m = try DocumentModel(session: f)
    let v = ArrangementView(document: m)
    v.frame = NSRect(x: 0, y: 0, width: 800, height: 300)
    v.layout()
    return (m, v)
}

@MainActor
@Suite struct ArrangementViewTests {
    @Test func escapeAbortsAFinalEditStillPendingOnBusy() throws {
        let f = editorFake()
        let (m, v) = try arrangement(f)
        v.contentMouseDown(onChords, clicks: 1, modifiers: [])
        v.contentMouseDragged(CGPoint(x: 300, y: 70), modifiers: [])
        #expect(m.editor.gesture != nil)
        f.gestureErrors = [.Busy(message: "busy")]
        v.contentMouseUp(CGPoint(x: 600, y: 70), modifiers: [])
        #expect(m.editor.gesture?.finishing == true, "the final edit waits for the plugin timer")
        v.contentCancel()
        #expect(m.editor.gesture == nil)
        #expect(f.calls.last == "abort")
        #expect(!f.calls.contains("end"))
    }

    @Test func aDragWatchesItsSourceAndTargetLanes() throws {
        let f = editorFake()
        let (m, v) = try arrangement(f)
        v.contentMouseDown(onChords, clicks: 1, modifiers: [])
        v.contentMouseDragged(CGPoint(x: 300, y: 20), modifiers: [])
        #expect(m.editor.gesture?.lanes == [chords, grooveLane], "begun straight onto the Groove row")
        v.contentMouseDragged(CGPoint(x: 300, y: 70), modifiers: [])
        #expect(m.editor.gesture?.lanes == [chords])
        v.contentMouseDragged(CGPoint(x: 300, y: 20), modifiers: [])
        #expect(m.editor.gesture?.lanes == [chords, grooveLane])
        var s = f.snap
        s.version += 1
        s.laneRevs.removeAll { $0.lane == 30 }
        f.idleUpdates = [IdleUpdate(snapshot: s, slotsChanged: false, paramsChanged: [], problems: [])]
        m.idle()
        #expect(m.editor.gesture == nil, "the target lane vanished: the drag is aborted")
        #expect(f.calls.last == "abort")
    }

    @Test func locatingWhileStoppedMovesThePlayhead() async throws {
        let f = editorFake()
        f.state.seq = 3
        let (m, v) = try arrangement(f)
        #expect(!v.playheadLayer.isHidden)
        #expect(v.playheadLayer.frame.minX == 0)
        m.locate(61_440)
        await settle()
        #expect(v.playheadLayer.frame.minX == 160)
    }

    @Test func aPlayingArrangementRemovedFromItsWindowStopsAndIsReleased() async throws {
        let f = editorFake()
        let m = try DocumentModel(session: f)
        let w = NSWindow(
            contentRect: NSRect(x: 0, y: 0, width: 800, height: 300), styleMask: [.titled],
            backing: .buffered, defer: true)
        w.isReleasedWhenClosed = false
        weak var released: ArrangementView?
        do {
            let v = ArrangementView(document: m)
            w.contentView?.addSubview(v)
            m.playStop()
            await settle()
            #expect(v.isLinkRunning)
            v.removeFromSuperview()
            #expect(!v.isLinkRunning)
            m.playStop()
            m.playStop()
            await settle()
            #expect(m.playKey.playing)
            #expect(!v.isLinkRunning, "a playKey change must not restart it outside a window")
            released = v
        }
        await settle()
        #expect(released == nil, "nothing keeps the removed arrangement alive")
    }
}
