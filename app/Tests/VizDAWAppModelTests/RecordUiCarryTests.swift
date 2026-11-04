import AppKit
import Testing
import VizDAWCore
@testable import VizDAWAppModel
@testable import VizDAWEditors

/// The 5d-2 record UI's carries (plan 5e-2 Tasks 7–8): Settings › Audio's
/// holder and Software Monitoring, the monitor button's tooltip, and the
/// test gaps the 5d-2 reviews parked.
@MainActor
@Suite(.serialized) struct RecordUiCarryTests {
    /// T7 Minor 3: the device is read from the first session whose input
    /// names one.
    @Test func theInputHolderIsTheFirstSessionWithADevice() throws {
        let quiet = FakeSession([])
        let holder = FakeSession([])
        holder.inputRow = InputInfoRow(
            device: "Fake", channels: 2, names: ["L", "R"], inputLatencyFrames: 0, outputLatencyFrames: 0)
        #expect(AudioPreferences.inputHolder([quiet, holder]) === holder)
        #expect(AudioPreferences.inputHolder([quiet]) == nil)
        #expect(AudioPreferences.inputHolder([]) == nil)
    }

    /// T7 Minor 4: Software Monitoring reaches the meters at once, not at
    /// the next poll.
    @Test func softwareMonitoringReachesTheMetersAtOnce() throws {
        let saved = LaneInputs.softwareMonitoring
        defer { LaneInputs.softwareMonitoring = saved }
        var on = true
        LaneInputs.softwareMonitoring = { on }
        let m = try DocumentModel(session: editorFake())
        #expect(m.inputs.monitoringOn)
        on = false
        m.inputs.refreshMonitoring()
        #expect(!m.inputs.monitoringOn)
    }

    /// T5 M3 (Ruling 14): the monitor button explains its mode.
    @Test func theMonitorButtonHasItsTooltip() throws {
        let f = editorFake()
        f.laneTable[3]?[1].monitor = .input
        let m = try DocumentModel(session: f)
        let h = LaneHeaderView(document: m)
        let r = try #require(LaneHeaderLayout.controls(rowTop: h.rowTop(1)).first { $0.0 == .monitor }?.1)
        #expect(h.toolTipText(at: CGPoint(x: r.midX, y: r.midY)) == LaneInputs.help(.input))
        let name = try #require(LaneHeaderLayout.controls(rowTop: h.rowTop(1)).first { $0.0 == .name }?.1)
        #expect(h.toolTipText(at: CGPoint(x: name.midX, y: name.midY)) == nil)
    }

    /// 5d-2 T4 M4: disarming clears "opening" and its status line.
    @Test func openingIsClearedOnDisarm() throws {
        let f = editorFake()
        f.laneTable[3]?[1].arm = true
        f.laneTable[3]?[1].input = .audio(first: 0, stereo: false, returnOf: nil)
        f.inputRow = InputInfoRow(
            device: "Fake", channels: 2, names: ["L", "R"], inputLatencyFrames: 0, outputLatencyFrames: 0)
        f.inputRow.opening = true
        let m = try DocumentModel(session: f)
        m.inputs.pollMeters()
        #expect(m.inputs.opening && m.editor.status == LaneInputs.openingNote)
        f.laneTable[3]?[1].arm = false
        f.snap.version += 1
        m.refreshAfterJob()
        m.inputs.pollMeters()
        #expect(!m.inputs.opening && m.editor.status == nil)
    }

    /// 5d-2 T5 M5: the meter hides under 46 points; a click on the
    /// monitor button goes through `mouseDown` to one undo step.
    @Test func theMeterHidesUnder46PointsAndTheMonitorClickCycles() throws {
        #expect(!LaneHeaderLayout.showsMeter(laneHeight: 45))
        for h in [46.0, 47, 48, 96] { #expect(LaneHeaderLayout.showsMeter(laneHeight: h), "\(h)") }
        let f = editorFake()
        let m = try DocumentModel(session: f)
        m.select(3)
        let h = LaneHeaderView(document: m)
        let r = try #require(LaneHeaderLayout.controls(rowTop: h.rowTop(1)).first { $0.0 == .monitor }?.1)
        let at = h.convert(NSPoint(x: r.midX, y: r.midY), to: nil)
        let e = try #require(
            NSEvent.mouseEvent(
                with: .leftMouseDown, location: at, modifierFlags: [], timestamp: 0, windowNumber: 0,
                context: nil, eventNumber: 1, clickCount: 1, pressure: 1))
        h.mouseDown(with: e)
        #expect(f.edits.last == .lane(edit: .setMonitor(seq: 3, lane: 31, monitor: .input)))
    }

    /// Final review m9: the monitor tooltips cover the visible rows only,
    /// are rebuilt only when what they cover changes, and follow a scroll.
    @Test func monitorTooltipsCoverTheVisibleRowsAndFollowAScroll() throws {
        let f = editorFake()
        let base = try #require(f.laneTable[3]?.first)
        f.laneTable[3] = (0..<200).map { i in
            var l = base
            l.id = 1000 + UInt64(i)
            l.monitor = i == 10 ? .input : .off
            return l
        }
        let m = try DocumentModel(session: f)
        m.select(3)
        let h = LaneHeaderView(document: m)
        h.frame = NSRect(x: 0, y: 0, width: LaneHeaderLayout.width, height: 300)
        func monitorRect(_ i: Int) -> CGRect? { LaneHeaderLayout.controls(rowTop: h.rowTop(i)).first { $0.0 == .monitor }?.1 }
        h.layout()
        let visible = Int((300 / h.laneHeight).rounded(.up)) + 1
        #expect(!h.toolTipRects.isEmpty && h.toolTipRects.count <= visible, "\(h.toolTipRects.count) rects")
        #expect(h.toolTipRects.first == monitorRect(0))
        let rebuilds = h.toolTipRebuilds
        h.layout()
        #expect(h.toolTipRebuilds == rebuilds, "nothing moved: no rebuild")
        // Scrolled by ten rows: row 10's button is where row 0's was, and
        // its tooltip names row 10's mode.
        h.verticalOffset = 10 * h.laneHeight
        h.layout()
        #expect(h.toolTipRects.count <= visible)
        let r = try #require(monitorRect(10))
        #expect(h.toolTipRects.contains(r))
        #expect(h.toolTipText(at: CGPoint(x: r.midX, y: r.midY)) == LaneInputs.help(.input))
        #expect(h.toolTipRects.allSatisfy { $0.maxY > 0 && $0.minY < 300 }, "only rows in view")
    }
}
