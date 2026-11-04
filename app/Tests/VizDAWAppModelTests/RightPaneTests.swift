import Foundation
import Testing
import VizDAWCore
@testable import VizDAWAppModel

/// Song's first lane (Groove, ID 30) plays through audio path 7, whose strip is 10.
func laneOnStrip10() -> FakeSession {
    let f = demoFake()
    f.laneTable[3]?[0].audioOut = 7
    f.mixerSnap = MixerSnapshot(
        version: f.snap.version,
        strips: [stripRow(10, "Out 1", kind: .audioPath(path: 7)), stripRow(0, "Master", kind: .master)],
        buses: [])
    return f
}

@MainActor private func fakeDocumentAndSession(_ f: FakeSession = demoFake()) -> (DocumentModel, FakeSession) {
    (try! DocumentModel(session: f), f)
}

@MainActor private func selectFirstLane(_ m: DocumentModel) {
    m.editor.setSelection(SelectionRow(timeRange: nil, lanes: [LaneKey(m.selectedSeq!, m.lanes[0].id).ref], events: []))
}

private func info(markers: [MarkerRow]) -> SequenceInfo {
    var i = fourFour(3)
    i.markers = markers
    return i
}

private let delay = PluginRefRow(format: .clap, id: "x.Delay", name: "Delay", vendor: "V", version: "1")

@MainActor
@Suite struct RightPaneTests {
    @Test func markersListTheSequencesMarkersAndSetLocators() {
        var marks = TransportMarks()
        marks.setLocator(2, 1920)
        marks.setLocator(0, 0)
        let rows = MarkerList.rows(
            info: info(markers: [MarkerRow(tick: 1920, name: "Chorus"), MarkerRow(tick: 0, name: "Intro")]), marks: marks)
        #expect(rows.map(\.name) == ["Intro", "Locator 0", "Chorus", "Locator 2"])
        #expect(rows.first?.position == "1.1.0")
    }
    @Test func choosingAMarkerMovesThePlayhead() {
        let (m, session) = fakeDocumentAndSession()
        MarkerList.go(MarkerListRow(id: "m0", name: "Chorus", tick: 1920, position: "1.2.0"), m)
        #expect(session.locates.last == 1920)
    }
    @Test func aPickedPluginGoesOnTheSelectedLanesStrip() throws {
        let (m, session) = fakeDocumentAndSession(laneOnStrip10())
        selectFirstLane(m)
        let mixer = try MixerModel(document: m)
        #expect(PaneContent.insertPlugin(delay, document: m, mixer: mixer) == nil)
        #expect(session.calls.last == "addInsert 10 end \(delay.id)")
    }
    @Test func aPickedPluginGoesOnTheSelectedMixerStrip() throws {
        let (m, session) = fakeDocumentAndSession(laneOnStrip10())
        let mixer = try MixerModel(document: m)
        m.selectedStrips = [10]
        #expect(PaneContent.insertPlugin(delay, document: m, mixer: mixer) == nil)
        #expect(session.calls.last == "addInsert 10 end \(delay.id)")
        m.selectedStrips = [0]
        #expect(PaneContent.insertPlugin(delay, document: m, mixer: mixer) == PaneContent.noStripMessage)
    }
    @Test func aPluginWithNoStripIsRefusedInTheStatusLine() throws {
        let (m, _) = fakeDocumentAndSession()
        let mixer = try MixerModel(document: m)
        #expect(PaneContent.insertPlugin(delay, document: m, mixer: mixer) == PaneContent.noStripMessage)
    }
    @Test func theControllersChartFollowsTheSelectedLane() {
        let (m, _) = fakeDocumentAndSession()
        #expect(m.controllersChart.state == nil)
        selectFirstLane(m)
        #expect(m.controllersChart.state?.kind == .stripChart)
        #expect(m.controllersChart.state?.target == .lane(seq: m.selectedSeq!, lane: m.lanes[0].id))
    }
    @Test func theControllersRowsAreKeptPerLane() {
        let (m, _) = fakeDocumentAndSession()
        selectFirstLane(m)
        var d = m.controllersChart.state!
        d.stripRows = [.velocity, .controller(1)]
        m.controllersChart.write(d)
        #expect(
            EditorLayout.decode(m.editor.layout.encoded()).controllers["\(m.selectedSeq!):\(m.lanes[0].id)"]
                == [.velocity, .controller(1)])
    }
}
