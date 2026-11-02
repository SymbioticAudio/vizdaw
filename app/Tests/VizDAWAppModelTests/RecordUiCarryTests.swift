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
}
