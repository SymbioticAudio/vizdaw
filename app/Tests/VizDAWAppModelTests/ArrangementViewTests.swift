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

    // MARK: Drag ends

    @Test func aPlainMoveIsOneBeginEndPair() throws {
        let f = editorFake()
        let (m, v) = try arrangement(f)
        f.calls = []
        v.contentMouseDown(onChords, clicks: 1, modifiers: [])
        v.contentMouseDragged(CGPoint(x: 300, y: 70), modifiers: [])
        v.contentMouseUp(CGPoint(x: 300, y: 70), modifiers: [])
        #expect(f.calls == ["begin Move Block", "gesture", "end"])
        guard case .event(.move(let ids, let delta, let toLane))? = f.gestureEdits.last else {
            Issue.record("expected a move, got \(f.gestureEdits)")
            return
        }
        #expect(ids == [1] && delta > 0 && toLane == nil)
        #expect(m.editor.gesture == nil)
    }

    @Test func anOptionDragCopiesIntoTheTargetLane() throws {
        let f = editorFake()
        let (_, v) = try arrangement(f)
        f.calls = []
        v.contentMouseDown(onChords, clicks: 1, modifiers: [.option])
        v.contentMouseDragged(CGPoint(x: 300, y: 20), modifiers: [.option])
        v.contentMouseUp(CGPoint(x: 300, y: 20), modifiers: [.option])
        #expect(f.calls == ["begin Copy Block", "gesture", "end"])
        guard case .event(.insert(let seq, let lane, let events))? = f.gestureEdits.last else {
            Issue.record("expected an insert, got \(f.gestureEdits)")
            return
        }
        #expect((seq, lane) == (3, 30))
        #expect(events.count == 1 && events[0].tick > 0)
    }

    @Test func aSameLaneOptionDragInExactModeOpensMoveToACopy() throws {
        let f = editorFake()
        let (m, v) = try arrangement(f)
        m.editor.exactMode = true
        f.calls = []
        v.contentMouseDown(onChords, clicks: 1, modifiers: [.option])
        v.contentMouseDragged(CGPoint(x: 300, y: 70), modifiers: [.option])
        v.contentMouseUp(CGPoint(x: 300, y: 70), modifiers: [.option])
        #expect(f.calls == ["begin Copy Block", "gesture", "abort"], "previewed, then put back")
        let p = m.selectModify
        #expect(p.isOpen && p.task == .move && p.config.modify.toCopy)
        guard case .move(let delta) = p.config.modify.op else {
            Issue.record("expected a move, got \(p.config.modify.op)")
            return
        }
        #expect(delta > 0)
    }

    @Test func aCrossLaneOptionDragInExactModeStillCopies() throws {
        let f = editorFake()
        let (m, v) = try arrangement(f)
        m.editor.exactMode = true
        f.calls = []
        v.contentMouseDown(onChords, clicks: 1, modifiers: [.option])
        v.contentMouseDragged(CGPoint(x: 300, y: 20), modifiers: [.option])
        v.contentMouseUp(CGPoint(x: 300, y: 20), modifiers: [.option])
        #expect(f.calls == ["begin Copy Block", "gesture", "end"])
        #expect(!m.selectModify.isOpen)
    }

    @Test func aDragOntoAnotherRowMergesIntoItsLane() throws {
        let f = editorFake()
        let (_, v) = try arrangement(f)
        f.calls = []
        v.contentMouseDown(onChords, clicks: 1, modifiers: [])
        v.contentMouseDragged(CGPoint(x: 50, y: 20), modifiers: [])
        v.contentMouseUp(CGPoint(x: 50, y: 20), modifiers: [])
        #expect(f.calls == ["begin Move Block", "gesture", "end"])
        #expect(f.gestureEdits == [.event(edit: .move(events: [1], delta: 0, toLane: LaneRef(seq: 3, lane: 30)))])
    }

    @Test func escapeMidDragLeavesNoChange() throws {
        let f = editorFake()
        let (m, v) = try arrangement(f)
        f.calls = []
        v.contentMouseDown(onChords, clicks: 1, modifiers: [])
        v.contentMouseDragged(CGPoint(x: 300, y: 70), modifiers: [])
        v.contentCancel()
        v.contentMouseUp(CGPoint(x: 300, y: 70), modifiers: [])
        #expect(f.calls == ["begin Move Block", "gesture", "abort"])
        #expect(m.editor.gesture == nil)
    }

    @Test func optionPressedMidDragDoesNotTurnAMoveIntoACopy() throws {
        let f = editorFake()
        let (_, v) = try arrangement(f)
        f.calls = []
        v.contentMouseDown(onChords, clicks: 1, modifiers: [])
        v.contentMouseDragged(CGPoint(x: 300, y: 70), modifiers: [])
        v.contentMouseDragged(CGPoint(x: 300, y: 20), modifiers: [.option])
        v.contentMouseUp(CGPoint(x: 300, y: 20), modifiers: [.option])
        #expect(f.calls.first == "begin Move Block")
        #expect(f.calls.last == "end")
        for e in f.gestureEdits {
            guard case .event(.move) = e else {
                Issue.record("expected only moves, got \(e)")
                return
            }
        }
        guard case .event(.move(_, _, let toLane))? = f.gestureEdits.last else { return }
        #expect(toLane == LaneRef(seq: 3, lane: 30))
    }

    // MARK: Light drawing

    @Test func lightModeCountsTheVisibleBlocksNotTheDirtyRect() throws {
        let f = editorFake()
        f.blocksTable["3/31"] = (0..<1_501).map { blockRow(Int64($0) * 100, Int64($0) * 100, events: [UInt64($0) + 100]) }
        let (_, v) = try arrangement(f)
        #expect(v.visibleBlockCount == 1_502, "1 501 Chords blocks and the Groove reference")
        let image = NSImage(size: NSSize(width: 800, height: 300))
        image.lockFocus()
        v.drawContent(in: CGRect(x: 700, y: 0, width: 10, height: 10))
        image.unlockFocus()
        #expect(v.light, "a small redraw away from the blocks still draws light")
    }

    // MARK: Lane headers

    @Test func unreadableDestinationsSaySoInTheMenu() throws {
        let f = editorFake()
        let m = try DocumentModel(session: f)
        let h = LaneHeaderView(document: m)
        let lane = m.lanes[1]
        f.dests = [DestRow(id: 5, name: "Synth", plugin: nil)]
        #expect(h.destinationMenu(seq: 3, lane: lane).items.map(\.title) == ["No Destination", "Synth"])
        f.destinationsError = .Busy(message: "busy")
        let busy = h.destinationMenu(seq: 3, lane: lane).items
        #expect(busy.map(\.title) == ["No Destination", "Destinations unavailable (busy)"])
        #expect(!busy[1].isEnabled)
        #expect(m.editor.status == nil, "Busy is not a failure to report")
        f.destinationsError = .Model(message: "no destinations")
        let failed = h.destinationMenu(seq: 3, lane: lane).items
        #expect(failed.map(\.title) == ["No Destination", "Destinations unavailable"])
        #expect(!failed[1].isEnabled)
        #expect(m.editor.status == "no destinations")
    }

    @Test func theLaneMenuMarksAndUnmarksADrumLane() throws {
        let f = editorFake()
        let m = try DocumentModel(session: f)
        let h = LaneHeaderView(document: m)
        var lane = m.lanes[1]
        let item = try #require(h.laneMenu(seq: 3, lane: lane).items.first)
        #expect(item.title == "Drum Lane" && item.state == .off)
        _ = item.target?.perform(item.action)
        #expect(f.edits.last == .drum(edit: .markLane(seq: 3, lane: 31, drum: true)))
        lane.isDrum = true
        let on = try #require(h.laneMenu(seq: 3, lane: lane).items.first)
        #expect(on.state == .on)
        _ = on.target?.perform(on.action)
        #expect(f.edits.last == .drum(edit: .markLane(seq: 3, lane: 31, drum: false)))
    }

    @Test func aRenameEndsOnTheLaneItBeganOn() throws {
        let f = editorFake()
        let m = try DocumentModel(session: f)
        let h = LaneHeaderView(document: m)
        h.beginRename(m.lanes[1], seq: 3, row: 1)
        h.renaming?.field.stringValue = "Keys"
        m.choose(2)
        h.controlTextDidEndEditing(Notification(name: NSControl.textDidEndEditingNotification))
        #expect(f.edits.last == .lane(edit: .rename(seq: 3, lane: 31, name: "Keys")))
        #expect(h.renaming == nil)
    }

    @Test func aHeaderTornDownMidRenameReEnablesTransportKeys() throws {
        let m = try DocumentModel(session: editorFake())
        let h = LaneHeaderView(document: m)
        h.frame = CGRect(x: 0, y: 0, width: 200, height: 300)
        let w = NSWindow(
            contentRect: h.frame, styleMask: [.titled], backing: .buffered, defer: true)
        w.contentView?.addSubview(h)
        h.beginRename(m.lanes[1], seq: 3, row: 1)
        #expect(m.editor.textEditors == ["laneName"])
        #expect(!m.transportKeysEnabled)
        h.removeFromSuperview()
        #expect(m.editor.textEditors.isEmpty)
        #expect(m.transportKeysEnabled)
    }
}
