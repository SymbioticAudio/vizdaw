import Foundation
import Testing
import VizDAWCore

@testable import VizDAWAppModel

/// The Region menu (plan 5b Task 8; spec §10.2).
@MainActor
@Suite struct RegionCommandsTests {
    func model() throws -> (DocumentModel, FakeSession) {
        let f = editorFake()
        return (try DocumentModel(session: f), f)
    }

    @Test func everyCommandNeedsSelectedEvents() throws {
        let (m, f) = try model()
        for id in RegionCommands.ids {
            #expect(!RegionCommands.enabled(id, m))
            #expect(RegionCommands.run(id, m) == .unavailable("Select audio regions first."), "\(id)")
        }
        #expect(f.edits.isEmpty && f.normalizations.isEmpty)
    }

    @Test func fadesUseTheTransportPosition() throws {
        let (m, f) = try model()
        m.editor.setSelection(SelectionRow(timeRange: nil, lanes: [], events: [5]))
        f.state.tick = 960
        m.poll()
        #expect(RegionCommands.enabled(RegionCommands.fadeIn, m))
        #expect(RegionCommands.run(RegionCommands.fadeIn, m) == .done)
        #expect(f.edits.last == .audio(edit: .fadeTo(events: [5], at: 960, fadeIn: true)))
        #expect(RegionCommands.run(RegionCommands.fadeOut, m) == .done)
        #expect(f.edits.last == .audio(edit: .fadeTo(events: [5], at: 960, fadeIn: false)))
        #expect(RegionCommands.run(RegionCommands.removeFades, m) == .done)
        #expect(f.edits.last == .audio(edit: .removeFades(events: [5])))
        f.editError = .Model(message: "the cursor is not inside a selected audio region")
        #expect(
            RegionCommands.run(RegionCommands.fadeIn, m)
                == .refused("the cursor is not inside a selected audio region"))
    }

    @Test func normalizeSendsThePeakAndReportsBusy() throws {
        let (m, f) = try model()
        m.editor.setSelection(SelectionRow(timeRange: nil, lanes: [], events: [5, 6]))
        #expect(RegionCommands.normalize(peakDb: -1, m) == .done)
        #expect(f.normalizations.last?.events == [5, 6])
        #expect(f.normalizations.last?.peakDb == -1)
        #expect(RegionCommands.run(RegionCommands.normalize, m) == .done)
        #expect(f.normalizations.last?.peakDb == Float(RegionCommands.defaultPeakDb))
        f.normalizeError = .Busy(message: "still being built")
        #expect(RegionCommands.normalize(peakDb: -1, m) == .busy("still being built"))
        #expect(m.editor.status == "still being built")
        f.normalizeError = nil
        for bad in [0.5, -96.5, Double.nan, Double.infinity] {
            if case .refused = RegionCommands.normalize(peakDb: bad, m) {} else {
                Issue.record("\(bad) was accepted")
            }
        }
        #expect(f.normalizations.count == 2)
    }

    @Test func peakTextParses() {
        #expect(RegionCommands.parsePeak("-0.3") == -0.3)
        #expect(RegionCommands.parsePeak(" −1 ") == -1)
        #expect(RegionCommands.parsePeak("0") == 0)
        for bad in ["", "abc", "nan", "inf", "-inf", "1", "-96.01", "1e999", "0x10", "-0.3dB"] {
            #expect(RegionCommands.parsePeak(bad) == nil, "\(bad)")
        }
    }

    @Test func aGestureBlocksTheCommands() throws {
        let (m, f) = try model()
        m.editor.setSelection(SelectionRow(timeRange: nil, lanes: [], events: [5]))
        #expect(m.editor.beginGesture("Drag", lanes: [], origin: .other))
        #expect(!RegionCommands.enabled(RegionCommands.removeFades, m))
        #expect(RegionCommands.run(RegionCommands.removeFades, m) == .unavailable(EditorModel.gestureOpenMessage))
        #expect(RegionCommands.normalize(peakDb: -1, m) == .unavailable(EditorModel.gestureOpenMessage))
        #expect(f.normalizations.isEmpty)
    }
}
