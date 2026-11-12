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

    /// Plan 6a Task 10: the sheet opens loaded, with what a format change
    /// would convert; the control channel never shows it.
    @Test func showLoadsAndSaysWhatAFormatChangeConverts() throws {
        let f = demoFake()
        f.song.title = "Song"
        let m = try DocumentModel(session: f)
        let p = m.projectSettings
        #expect(!p.shown)
        p.show()
        #expect(p.shown && p.song.title == "Song")
        #expect(p.mediaNote == "The project has no audio files yet.")
        f.mediaRows = [mediaRow(1, "a", frames: 48_000 * 90), mediaRow(2, "b", frames: 48_000 * 30)]
        p.show()
        #expect(p.mediaNote == "Changing the format converts 2 audio files (2 min of audio) into new files (one undo step).")
        f.mediaRows = [mediaRow(1, "a", frames: 48_000 * 5)]
        p.show()
        #expect(p.mediaNote == "Changing the format converts 1 audio file (5 s of audio) into new files (one undo step).")
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

    /// Plan 6a Task 10: the inspector's Region name field. Typing keeps a
    /// draft; Return sends one rename, only when the name changed.
    @Test func theInspectorsNameFieldCommitsOnce() throws {
        let (m, f) = try document(name: "Take 3")
        let before = f.edits.count
        #expect(m.regionNameText(5) == "Take 3")
        m.editRegionName(5, "Take 3")
        m.commitRegionName()
        #expect(f.edits.count == before, "unchanged")
        m.editRegionName(5, "  Lead  ")
        #expect(m.regionNameText(5) == "  Lead  " && m.regionNameText(6) == "", "a draft is its region's own")
        #expect(f.edits.count == before, "typing sends nothing")
        m.commitRegionName()
        #expect(f.edits.count == before + 1 && f.edits.last == .audio(edit: .setName(event: 5, name: "Lead")))
        m.commitRegionName()
        #expect(f.edits.count == before + 1, "committed once")
        m.editRegionName(5, "")
        m.cancelRegionName()
        m.commitRegionName()
        #expect(f.edits.count == before + 1, "cancelled")
        m.editRegionName(5, " ")
        m.commitRegionName()
        #expect(f.edits.last == .audio(edit: .setName(event: 5, name: nil)), "cleared: the file's name shows")
    }
}
