import AppKit
import Testing
import VizDAWCore
@testable import VizDAWAppModel
@testable import VizDAWEditors

/// The lane header's input controls (plan 5d-2 Task 5; spec §6.9): where
/// they sit, the input menu, and what VoiceOver says.
@MainActor
@Suite struct LaneHeaderInputTests {
    /// Song's Chords lane (31) plays to an audio path; the device has two
    /// named channels.
    func header(armed: Bool = false) throws -> (DocumentModel, FakeSession, LaneHeaderView) {
        let f = editorFake()
        f.laneTable[3]?[1].audioOut = 7
        if armed {
            f.laneTable[3]?[1].arm = true
            f.laneTable[3]?[1].input = .audio(first: 0, stereo: false, returnOf: nil)
        }
        f.inputRow = InputInfoRow(
            device: "Fake", channels: 2, names: ["L", "R"], inputLatencyFrames: 0, outputLatencyFrames: 0)
        let m = try DocumentModel(session: f)
        return (m, f, LaneHeaderView(document: m))
    }

    @Test func theInputMenuListsTheChoicesAndSetsTheInput() throws {
        let (m, f, h) = try header()
        let items = h.inputMenu(seq: 3, lane: m.lanes[1]).items.filter { !$0.isSeparatorItem }
        #expect(items.map(\.title) == ["No Input", "Audio", "In 1 (L)", "In 2 (R)", "In 1+2"])
        #expect(items[0].state == .on, "the lane has no input yet")
        #expect(!items[1].isEnabled, "a section title")
        let pair = items[4]
        _ = pair.target?.perform(pair.action)
        #expect(
            f.edits.last == .lane(edit: .setInput(seq: 3, lane: 31, input: .audio(first: 0, stereo: true, returnOf: nil))))
    }

    @Test func theControlsShareTheRowWithoutOverlapping() {
        let controls = LaneHeaderLayout.controls(rowTop: 0)
        for (i, a) in controls.enumerated() {
            for b in controls.dropFirst(i + 1) {
                #expect(!a.1.intersects(b.1), "\(a.0) and \(b.0)")
            }
            #expect(a.1.maxX <= LaneHeaderLayout.width)
        }
        let meter = LaneHeaderLayout.meterRect(rowTop: 0)
        #expect(controls.allSatisfy { !$0.1.intersects(meter) })
        #expect(meter.maxY <= 48, "inside the default lane height")
        #expect(LaneHeaderLayout.clipRect(rowTop: 0).minX >= meter.maxX)
    }

    @Test func voiceOverSaysTheInputAndMonitoring() throws {
        var l = lane(31, "Vocal")
        l.arm = true
        l.input = .audio(first: 0, stereo: true, returnOf: nil)
        l.monitor = .input
        #expect(ArrangementA11y.header(l) == "Vocal, no destination, armed, input In 1+2, monitoring input")
        l.monitor = .off
        #expect(ArrangementA11y.header(l).hasSuffix("monitoring off"))
        l.monitor = .auto
        #expect(ArrangementA11y.header(l) == "Vocal, no destination, armed, input In 1+2")
        // A return lane says whose return it records (Task 5 review M3).
        #expect(
            ArrangementA11y.header(l, returnOf: "Moog")
                == "Vocal, no destination, armed, input In 1+2, recording the return of Moog")
    }

    /// Pre-flight I5: the header says so while the input opens.
    @Test func voiceOverSaysTheInputIsOpening() throws {
        let (m, f, h) = try header(armed: true)
        f.inputRow.opening = true
        m.inputs.pollMeters()
        #expect(m.inputs.opening)
        let labels = (h.accessibilityChildren() ?? []).compactMap { ($0 as? NSAccessibilityElement)?.accessibilityLabel() }
        #expect(labels.contains { $0.hasSuffix(", input opening") })
    }

    /// Task 4 review M2: no empty meter frame before the first poll.
    @Test func monitoringStartsFromTheCoresSetting() throws {
        let saved = LaneInputs.softwareMonitoring
        defer { LaneInputs.softwareMonitoring = saved }
        LaneInputs.softwareMonitoring = { false }
        let (m, _, _) = try header()
        #expect(!m.inputs.monitoringOn)
    }
}
