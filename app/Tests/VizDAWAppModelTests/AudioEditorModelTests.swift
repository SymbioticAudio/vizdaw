import Foundation
import Testing
import VizDAWCore
@testable import VizDAWAppModel

private let toneLane = LaneKey(3, 30)

/// Region 71 on lane (3, 30): media 5 ("tone", 48 kHz, 96 000 frames,
/// stereo), offset 24 000, 48 000 frames, at tick 48 000 (1.5625 s in the
/// fake, so 75 000 frames of room before tick 0). Region 72 is at tick 0,
/// offset 0, 24 000 frames; region 73 at tick 0, offset 24 000, 24 000
/// frames. The IDs avoid `editorFake()`'s reference block 7.
@MainActor func audioEditorDocument(
    gainDb: Float = 0, fadeIn: Int64 = 0, fadeOut: Int64 = 0, media: Bool = true
) -> (FakeSession, DocumentModel) {
    let f = editorFake()
    var r71 = regionRow(
        71, tick: 48_000, duration: 61_440, frames: 48_000, media: 5, gainDb: gainDb, fadeIn: fadeIn,
        fadeOut: fadeOut)
    r71.sourceOffset = 24_000
    let r72 = regionRow(72, tick: 0, duration: 30_720, frames: 24_000, media: 5)
    var r73 = regionRow(73, tick: 0, duration: 30_720, frames: 24_000, media: 5)
    r73.sourceOffset = 24_000
    f.laneEventTable["3/30"] = LaneEvents(seq: 3, lane: 30, rev: 1, events: [r71, r72, r73])
    f.mediaRows = media ? [mediaRow(5, "tone")] : []
    // swiftlint:disable:next force_try
    return (f, try! DocumentModel(session: f))
}

@MainActor func selectEvents(_ events: [UInt64], in m: DocumentModel) {
    m.editor.setSelection(SelectionRow(timeRange: nil, lanes: [], events: events))
}

@MainActor
@Suite struct AudioEditorModelTests {
    @Test func theEditorFollowsTheSelectedRegion() {
        let (_, m) = audioEditorDocument()
        selectEvents([71], in: m)
        // A first sync before the pane has a width must not leave a sliver
        // (Task 4 review M1).
        m.audioEditor.sync(width: 0)
        m.audioEditor.sync(width: 600)
        #expect(m.audioEditor.axis == FrameAxis.fit(offset: 24_000, length: 48_000, width: 600, mediaFrames: 96_000))
        #expect(m.editedRegion?.row.id == 71)
        #expect(m.editedRegion?.lane == toneLane)
        #expect(m.editedRegion?.media?.id == 5)
        let a = m.audioEditor
        a.sync(width: 600)
        #expect(a.axis == FrameAxis.fit(offset: 24_000, length: 48_000, width: 600, mediaFrames: 96_000))
        #expect(a.shownRegion == 71)
        a.zoomIn()
        let zoomed = a.axis
        a.sync(width: 600)
        #expect(a.axis == zoomed)
        #expect(a.axis != FrameAxis.fit(offset: 24_000, length: 48_000, width: 600, mediaFrames: 96_000))
        selectEvents([1], in: m)
        #expect(m.editedRegion == nil)
        a.sync(width: 600)
        #expect(a.shownRegion == nil)
        selectEvents([71, 72], in: m)
        #expect(m.editedRegion == nil)
        a.sync(width: 600)
        #expect(a.shownRegion == nil)
        selectEvents([71], in: m)
        a.sync(width: 600)
        #expect(a.axis == FrameAxis.fit(offset: 24_000, length: 48_000, width: 600, mediaFrames: 96_000))
        #expect(a.shownRegion == 71)
        #expect(m.inspectorRegion?.id == 71)
    }

    @Test func aVanishedRegionShowsTheMessage() {
        let (f, m) = audioEditorDocument()
        selectEvents([71], in: m)
        m.audioEditor.sync(width: 600)
        f.laneEventTable["3/30"] = LaneEvents(seq: 3, lane: 30, rev: 9, events: [])
        nextSnapshot(f) { $0.laneRevs[0].rev = 9 }
        m.undo()
        #expect(m.editedRegion == nil)
        let before = f.edits.count
        #expect(m.audioEditor.type(.gain, "-3") == false)
        #expect(f.edits.count == before)
        #expect(m.editor.status == AudioEditorModel.emptyMessage)
        m.audioEditor.sync(width: 600)
        #expect(m.audioEditor.shownRegion == nil)
        // No region: every box shows the dash, the Peak box too (Task 4 review M2).
        for box in AudioEditorBox.allCases { #expect(m.audioEditor.text(box) == "\u{2013}", "\(box)") }
    }

    @Test func typedGainIsOneUndoStepAndARefusalSendsNothing() {
        let (f, m) = audioEditorDocument()
        selectEvents([71], in: m)
        let a = m.audioEditor
        #expect(a.type(.gain, "-4.2"))
        #expect(f.edits == [.audio(edit: .setGain(event: 71, gainDb: -4.2))])
        #expect(a.type(.gain, "abc") == false)
        #expect(f.edits.count == 1)
        #expect(m.editor.status == AudioEditorEntry.gainHint)
        #expect(a.type(.gain, "0"))
        #expect(f.edits.count == 1, "equal to the row's gain: nothing sent")
    }

    @Test func typedFadesClampToWhatTheOtherLeaves() {
        let (f, m) = audioEditorDocument(fadeOut: 12_000)
        selectEvents([71], in: m)
        let a = m.audioEditor
        #expect(a.type(.fadeIn, "10 s"))
        #expect(
            f.edits == [
                .audio(
                    edit: .setFades(
                        event: 71, fadeIn: FadeRow(length: 36_000, shape: .linear),
                        fadeOut: FadeRow(length: 12_000, shape: .linear)))
            ])
        #expect(a.type(.fadeOut, "250"))
        #expect(f.edits.count == 1, "the fade out is 12 000 already")
        #expect(a.type(.fadeIn, "-5") == false)
        #expect(m.editor.status == AudioEditorEntry.fadeHint)
        #expect(f.edits.count == 1)
        // A fade that is not a whole ms shows rounded; committing the
        // unchanged text moves nothing (Task 2 review M1).
        let (g, n) = audioEditorDocument(fadeIn: 100)
        selectEvents([71], in: n)
        #expect(n.audioEditor.text(.fadeIn) == "2 ms")
        #expect(n.audioEditor.type(.fadeIn, "2 ms"))
        #expect(g.edits.isEmpty)
    }

    @Test func typedStartAndLengthClamp() {
        let (f, m) = audioEditorDocument()
        selectEvents([71], in: m)
        let a = m.audioEditor
        func trims(_ box: AudioEditorBox, _ text: String) -> EditCommand? {
            let before = f.edits.count
            #expect(a.type(box, text), "\(box) \(text)")
            return f.edits.count > before ? f.edits.last : nil
        }
        #expect(trims(.start, "0.25") == .audio(edit: .trim(event: 71, frontFrames: -12_000, backFrames: 0)))
        #expect(trims(.start, "0") == .audio(edit: .trim(event: 71, frontFrames: -24_000, backFrames: 0)))
        #expect(trims(.start, "10") == .audio(edit: .trim(event: 71, frontFrames: 47_999, backFrames: 0)))
        #expect(trims(.length, "1.25") == .audio(edit: .trim(event: 71, frontFrames: 0, backFrames: -12_000)))
        #expect(trims(.length, "10") == .audio(edit: .trim(event: 71, frontFrames: 0, backFrames: -24_000)))
        #expect(trims(.length, "0") == .audio(edit: .trim(event: 71, frontFrames: 0, backFrames: 47_999)))
        #expect(a.startRoom == 75_000)

        // The sequence start (Ruling 13.8): region 73 is at tick 0.
        selectEvents([73], in: m)
        #expect(a.startRoom == 0)
        #expect(trims(.start, "0") == nil)
        #expect(trims(.start, "0.25") == nil)

        let (g, n) = audioEditorDocument(media: false)
        selectEvents([71], in: n)
        #expect(n.audioEditor.type(.start, "1") == false)
        #expect(n.editor.status == AudioEditorEntry.noMediaHint)
        #expect(n.audioEditor.text(.start) == "\u{2013}")
        #expect(n.audioEditor.startRoom == 0)
        #expect(g.edits.isEmpty)
    }

    @Test func shapesKeepTheirLength() {
        let (f, m) = audioEditorDocument(fadeIn: 12_000)
        selectEvents([71], in: m)
        let a = m.audioEditor
        a.setShape(.fadeIn, .sCurve)
        #expect(
            f.edits == [
                .audio(
                    edit: .setFades(
                        event: 71, fadeIn: FadeRow(length: 12_000, shape: .sCurve),
                        fadeOut: FadeRow(length: 0, shape: .linear)))
            ])
        a.setShape(.fadeIn, .linear)
        #expect(f.edits.count == 1)
    }

    @Test func normalizeUsesTheTypedPeakAndNeverADialog() {
        let (f, m) = audioEditorDocument()
        selectEvents([71], in: m)
        let a = m.audioEditor
        #expect(a.type(.peak, "-1"))
        #expect(f.edits.isEmpty)
        #expect(a.text(.peak) == "\u{2212}1.0 dB")
        a.normalize()
        #expect(f.normalizations.count == 1)
        #expect(f.normalizations.first?.events == [71])
        #expect(f.normalizations.first?.peakDb == -1)
        let busy = "The waveform of tone is still being built; try again in a moment."
        f.normalizeError = .Busy(message: busy)
        a.normalize()
        #expect(m.editor.status == busy)
        #expect(m.alert == nil)
    }

    @Test func theZoomButtonsStayInRange() {
        let (_, m) = audioEditorDocument()
        selectEvents([71], in: m)
        let a = m.audioEditor
        a.sync(width: 600)
        for _ in 0..<20 { a.zoomIn() }
        #expect(a.axis.framesPerPoint == 1)
        for _ in 0..<20 { a.zoomOut() }
        #expect(a.axis.framesPerPoint == FrameAxis.maxFramesPerPoint(mediaFrames: 96_000, width: 600))
        for _ in 0..<10 { a.taller() }
        #expect(a.amplitude == 16)
        for _ in 0..<10 { a.shorter() }
        #expect(a.amplitude == 1)
    }
}
