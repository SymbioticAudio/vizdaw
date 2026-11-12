import Foundation
import Testing
import VizDAWCore

@testable import VizDAWAppModel

/// Project Settings with Song info (plan 6a Task 9) and the region's name.
@MainActor
@Suite struct ProjectSettingsModelTests {
    @Test func applySendsOnlyWhatChanged() throws {
        let f = demoFake()
        f.song.title = "Old"
        let m = try DocumentModel(session: f)
        let p = m.projectSettings
        p.load()
        #expect(p.song.title == "Old" && p.song.comment == "" && p.rate == 48_000 && p.depth == .int24)
        p.apply()
        #expect(f.songInfoSets.isEmpty && f.formatChanges.isEmpty)
        p.song.title = "New"
        p.apply()
        #expect(f.songInfoSets.count == 1 && f.songInfoSets[0].title == "New" && f.songInfoSets[0].artist == nil)
        #expect(f.formatChanges.isEmpty)
        p.apply()
        #expect(f.songInfoSets.count == 1, "applied once")
        p.rate = 96_000
        p.apply()
        #expect(f.formatChanges == [ProjectFormatRow(sampleRate: 96_000, depth: .int24)] && f.songInfoSets.count == 1)
        p.load()
        p.song.composer = "C"
        p.depth = .int16
        p.apply()
        #expect(f.songInfoSets.count == 2 && f.formatChanges.count == 2)
        // Emptying a field clears it.
        p.load()
        p.song.composer = ""
        p.apply()
        #expect(f.songInfoSets.last?.composer == nil)
    }

    func document(name: String?) throws -> (DocumentModel, FakeSession) {
        let f = editorFake()
        f.laneTable[3] = [lane(30, "A"), lane(31, "B")]
        var r = regionRow(5, tick: 0, duration: 61_440, frames: 96_000)
        r.regionName = name
        f.laneEventTable["3/31"] = LaneEvents(seq: 3, lane: 31, rev: 2, events: [r])
        f.mediaRows = [mediaRow(7, "loop")]
        let m = try DocumentModel(session: f)
        m.editor.setSelection(SelectionRow(timeRange: nil, lanes: [], events: [5]))
        return (m, f)
    }

    @Test func regionNameShowsInTheInspector() throws {
        let (named, _) = try document(name: "Take 3")
        #expect(named.inspectorRows(destinations: [], strips: []).contains(InspectorRow(label: "Region", value: "Take 3")))
        let (plain, _) = try document(name: nil)
        let media = plain.media.row(7)?.name ?? "?"
        #expect(plain.inspectorRows(destinations: [], strips: []).contains(InspectorRow(label: "Region", value: media)))
    }

    @Test func renamingARegionIsOneUndoStep() throws {
        let (m, f) = try document(name: nil)
        m.renameRegion(5, to: "Take 4")
        #expect(f.edits.last == .audio(edit: .setName(event: 5, name: "Take 4")))
        m.renameRegion(5, to: "")
        #expect(f.edits.last == .audio(edit: .setName(event: 5, name: nil)))
    }
}
