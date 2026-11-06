import AppKit
import SymbioticTokens
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

    @Test func aTrackOnAnInstrumentOffersReplaceInstrumentByMaker() throws {
        let f = editorFake()
        let m = try DocumentModel(session: f)
        let current = PluginRefRow(format: .clap, id: "u.a", name: "Alpha", vendor: "Acme", version: "1.0")
        f.dests = [
            DestRow(id: 5, name: "Built-in Synth", plugin: nil),
            DestRow(id: 6, name: "Alpha", plugin: current),
            DestRow(id: 7, name: "Juno", plugin: nil, external: true),
        ]
        let mixer = try MixerModel(document: m)
        m.storeMixer = mixer
        let h = LaneHeaderView(document: m)
        var lane = m.lanes[1]
        lane.midiDest = 6
        // No catalog hook (no window): no item.
        #expect(h.destinationMenu(seq: 3, lane: lane).items.map(\.title) == ["No Destination", "Built-in Synth", "Alpha", "Juno"])
        m.instrumentCatalog = {
            [
                catalogRow("u.a", "Alpha", vendor: "Acme", kind: .instrument),
                catalogRow("u.b", "Beta", vendor: "Acme", kind: .instrument),
                catalogRow("u.fx", "Verb", vendor: "Acme", kind: .effect),
                catalogRow("z.c", "Gamma", vendor: "Zed", kind: .instrument),
            ]
        }
        let menu = h.destinationMenu(seq: 3, lane: lane)
        let replace = try #require(menu.items.last)
        #expect(replace.title == "Replace Instrument")
        let makers = try #require(replace.submenu).items
        #expect(makers.map(\.title) == ["Acme", "Zed"])
        let acme = try #require(makers[0].submenu).items
        #expect(acme.map(\.title) == ["Alpha", "Beta"])
        #expect(acme.map(\.state) == [.on, .off], "the instrument it plays is checked")
        let before = f.calls.count
        _ = acme[0].target?.perform(acme[0].action)  // the one already loaded: no change
        #expect(f.calls.count == before)
        _ = acme[1].target?.perform(acme[1].action)
        #expect(Array(f.calls.dropFirst(before)) == ["destPlugin 6 u.b"])
        // The built-in synth's track: nothing checked. An external one: no item.
        lane.midiDest = 5
        let synth = try #require(h.destinationMenu(seq: 3, lane: lane).items.last?.submenu).items
        #expect(synth.flatMap { $0.submenu?.items ?? [] }.allSatisfy { $0.state == .off })
        lane.midiDest = 7
        #expect(h.destinationMenu(seq: 3, lane: lane).items.last?.title == "Juno")
        lane.midiDest = nil
        #expect(h.destinationMenu(seq: 3, lane: lane).items.last?.title == "Juno")
    }

    @Test func theLaneMenuHasNoDrumTrackItem() throws {
        let f = editorFake()
        let m = try DocumentModel(session: f)
        let h = LaneHeaderView(document: m)
        let lane = m.lanes[1]
        let titles = h.laneMenu(seq: 3, lane: lane).items.filter { !$0.isSeparatorItem }.map(\.title)
        // Colour ▸ last (plan 7b Task B7; StripColourTests covers it).
        #expect(titles == ["Track Synth…", "New MIDI Destination…", "Colour"])
        #expect(!h.laneMenu(seq: 3, lane: lane).items[0].isEnabled, "Chords plays no external synth")
        let saved = f.dests
        f.dests = saved + [DestRow(id: 7, name: "Juno", plugin: nil, external: true)]
        var juno = lane
        juno.midiDest = 7
        #expect(h.laneMenu(seq: 3, lane: juno).items[0].isEnabled, "Juno is an external synth")
        f.dests = saved
    }

    /// A header click selects its track; ⇧ or ⌘ adds or removes one, and
    /// the first selected stays the one the Track menu acts on.
    @Test func aHeaderClickSelectsItsTrack() throws {
        let f = editorFake()
        let m = try DocumentModel(session: f)
        let h = LaneHeaderView(document: m)
        let (a, b) = (m.lanes[0].id, m.lanes[1].id)
        m.editor.selectTrack(seq: 3, lane: b, extend: false)
        #expect(m.selectedLane?.id == b)
        #expect(h.selectedLanes == [b])
        m.editor.selectTrack(seq: 3, lane: a, extend: false)
        #expect(m.selectedLane?.id == a && h.selectedLanes == [a], "a plain click replaces it")
        m.editor.selectTrack(seq: 3, lane: b, extend: true)
        #expect(h.selectedLanes == [a, b] && m.selectedLane?.id == a)
        m.editor.selectTrack(seq: 3, lane: a, extend: true)
        #expect(h.selectedLanes == [b] && m.selectedLane?.id == b, "⇧ again removes it")
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

    @Test func escapeInTheLaneRenameFieldCancelsTheRename() throws {
        let f = editorFake()
        let m = try DocumentModel(session: f)
        let h = LaneHeaderView(document: m)
        let n = f.edits.count
        h.beginRename(m.lanes[1], seq: 3, row: 1)
        let field = try #require(h.renaming?.field)
        field.stringValue = "Keys"
        #expect(h.control(field, textView: NSTextView(), doCommandBy: #selector(NSResponder.cancelOperation(_:))))
        #expect(h.renaming == nil && field.superview == nil && !m.editor.textEditing)
        h.controlTextDidEndEditing(Notification(name: NSControl.textDidEndEditingNotification))
        #expect(f.edits.count == n, "no rename sent")
        // Other commands are left to the field; the field editor's own
        // cancel movement does not rename either.
        h.beginRename(m.lanes[1], seq: 3, row: 1)
        let again = try #require(h.renaming?.field)
        #expect(!h.control(again, textView: NSTextView(), doCommandBy: #selector(NSResponder.insertNewline(_:))))
        again.stringValue = "Keys"
        h.controlTextDidEndEditing(
            Notification(
                name: NSControl.textDidEndEditingNotification, object: again,
                userInfo: ["NSTextMovement": NSTextMovement.cancel.rawValue]))
        #expect(f.edits.count == n && h.renaming == nil && !m.editor.textEditing)
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

// MARK: What AppKit draws

private final class Swatch: NSView {
    var colour = NSColor.white
    override func draw(_ dirtyRect: NSRect) {
        colour.setFill()
        bounds.fill()
    }
}

extension ArrangementViewTests {
    /// The sRGB components of the pixel at `x`, `y` (row 0 is the top).
    private func rgb(_ rep: NSBitmapImageRep, _ x: Int, _ y: Int) throws -> [CGFloat] {
        let c = try #require(rep.colorAt(x: x, y: y)?.usingColorSpace(.sRGB))
        return [c.redComponent, c.greenComponent, c.blueComponent]
    }

    /// What `colour` becomes as a pixel of a snapshot (colour management
    /// shifts exact values): a swatch of it, snapshotted and decoded alike.
    private func painted(_ colour: NSColor) throws -> [CGFloat] {
        let swatch = Swatch(frame: NSRect(x: 0, y: 0, width: 4, height: 4))
        swatch.wantsLayer = true
        swatch.colour = colour
        let png = try #require(WindowSnapshot.png(of: swatch, scale: 1))
        return try rgb(try #require(NSBitmapImageRep(data: png.data)), 2, 2)
    }

    private func near(_ a: [CGFloat], _ b: [CGFloat]) -> Bool {
        zip(a, b).allSatisfy { abs($0 - $1) <= 2.0 / 255 }
    }

    @Test func theHeadersAndAddRowShowBesideTheCanvas() throws {
        let f = demoFake()
        f.laneTable[3] = [lane(30, "Groove")]
        let m = try DocumentModel(session: f)
        #expect(m.selectedSeq == 3 && m.lanes.count == 1)
        let container = ArrangementContainer(document: m)
        let w = NSWindow(
            contentRect: NSRect(x: 0, y: 0, width: 800, height: 400), styleMask: [.borderless],
            backing: .buffered, defer: true)
        w.isReleasedWhenClosed = false
        w.contentView = container
        container.frame = NSRect(x: 0, y: 0, width: 800, height: 400)
        container.layoutSubtreeIfNeeded()
        let png = try #require(WindowSnapshot.png(of: w, scale: 1))
        let rep = try #require(NSBitmapImageRep(data: png.data))
        let theme = container.theme
        let canvas = try painted(theme.canvas)
        let headerBackground = try painted(theme.rulerBackground)
        let laneHeight = container.headers.laneHeight
        let header = try rgb(rep, Int(LaneHeaderLayout.width / 2), Int(EditorCanvas.rulerHeight + laneHeight / 2))
        #expect(!near(header, canvas), "the header column shows, the canvas does not paint over it: \(header)")
        // The row's bottom edge is a separator line, which the ruler or
        // the canvas would hide if they painted over the header.
        let rule = try rgb(rep, 50, Int(EditorCanvas.rulerHeight + laneHeight) - 1)
        #expect(!near(rule, headerBackground), "the lane row's separator line shows: \(rule)")
        let addTop = Int(EditorCanvas.rulerHeight + laneHeight)
        let add = try rgb(rep, 8, addTop + 4)
        #expect(near(add, headerBackground), "the add row is the header background: \(add) vs \(headerBackground)")
    }
}
