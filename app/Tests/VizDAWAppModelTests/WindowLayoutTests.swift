import Foundation
import Testing
import VizDAWCore
@testable import VizDAWAppModel

@MainActor private func editor() -> EditorModel {
    // swiftlint:disable:next force_try
    try! DocumentModel(session: editorFake()).editor
}

@MainActor
@Suite struct WindowLayoutTests {
    @Test func aNewLayoutOpensArrangeWithTheStripLeftAndTheEditorBelow() {
        let v = EditorLayout().views
        #expect(v.main == .arrange)
        #expect(v.arrange.left == PaneState(shown: true, view: .strip, size: 220))
        #expect(v.arrange.bottom == PaneState(shown: true, view: .editor, size: 260))
        #expect(v.arrange.right == PaneState(shown: false, view: .controllers, size: 260))
        #expect([v.mix.left.shown, v.mix.bottom.shown, v.mix.right.shown] == [false, false, false])
        #expect(v.mix.right.view == .plugins)
    }
    @Test func eachPaneOffersOnlyItsOwnList() {
        #expect(ViewLayouts.views(.left, in: .arrange) == [.strip, .sequences, .inspector])
        #expect(ViewLayouts.views(.bottom, in: .arrange) == [.editor, .audioEditor, .mixer])
        #expect(ViewLayouts.views(.right, in: .arrange) == [.controllers, .mediaPool, .plugins, .markers])
    }
    @Test func theMixViewOffersNoMixerBelow() {
        #expect(ViewLayouts.views(.bottom, in: .mix) == [.editor, .audioEditor])
        let e = editor()
        #expect(e.setPane(.bottom, view: .mixer, shown: nil, size: nil, in: .mix)
            == "mixer is not on the bottom pane's list in the mix view.")
        #expect(e.layout.views.mix.bottom.view == .editor)
    }
    @Test func theBottomPaneListsTheAudioEditorInBothViews() {
        #expect(ViewLayouts.views(.bottom, in: .arrange) == [.editor, .audioEditor, .mixer])
        #expect(ViewLayouts.views(.bottom, in: .mix) == [.editor, .audioEditor])
        #expect(PaneView.audioEditor.rawValue == "audio_editor")
        #expect(PaneView.audioEditor.title == "Audio Editor")
        let bottom = try? JSONDecoder().decode(
            ViewLayouts.self, from: Data(#"{"arrange":{"bottom":{"view":"audio_editor"}}}"#.utf8))
        #expect(bottom?.arrange.bottom.view == .audioEditor)
        let left = try? JSONDecoder().decode(
            ViewLayouts.self, from: Data(#"{"arrange":{"left":{"view":"audio_editor"}}}"#.utf8))
        #expect(left?.arrange.left.view == .strip)
    }
    @Test func switchingViewsRestoresEachSetup() {
        let e = editor()
        _ = e.setPane(.right, view: .markers, shown: true, size: 300, in: nil)
        e.showMainView(.mix)
        _ = e.setPane(.left, view: nil, shown: true, size: 400, in: nil)
        e.showMainView(.arrange)
        #expect(e.layout.views.current.right == PaneState(shown: true, view: .markers, size: 300))
        #expect(e.layout.views.mix.left == PaneState(shown: true, view: .strip, size: 400))
    }
    @Test func togglingAPaneKeepsItsViewAndSize() {
        let e = editor()
        _ = e.setPane(.left, view: .inspector, shown: nil, size: 300, in: nil)
        e.setPaneShown(.left, nil)
        e.setPaneShown(.left, nil)
        #expect(e.layout.views.arrange.left == PaneState(shown: true, view: .inspector, size: 300))
    }
    @Test func aCorruptViewsFieldDecodesToDefaults() {
        let json = #"{"views":{"main":"edit","arrange":{"left":{"shown":true,"view":"mixer","size":-5},"#
            + #""bottom":{"shown":"yes","view":"editor","size":1e9},"right":{"view":"nope"}},"#
            + #""mix":{"bottom":{"shown":true,"view":"mixer","size":200}}},"laneHeight":60}"#
        let l = EditorLayout.decode(json)
        #expect(l.views.main == .arrange)
        #expect(l.views.arrange.left == PaneState(shown: true, view: .strip, size: 160))
        #expect(l.views.arrange.bottom == PaneState(shown: true, view: .editor, size: 900))
        #expect(l.views.arrange.right == ViewLayouts().arrange.right)
        #expect(l.views.mix.bottom == PaneState(shown: true, view: .editor, size: 200))
        #expect(l.views.mix.left == ViewLayouts().mix.left && l.views.mix.right == ViewLayouts().mix.right)
        #expect(l.laneHeight == 60)
        // A partial Mix patch keeps the Mix defaults for what it leaves out;
        // unknown keys and a pane that is not an object change nothing
        // (Task 1 review M1, M2).
        let partial = EditorLayout.decode(#"{"views":{"mix":{"bottom":{"size":200,"colour":"red"},"left":5}}}"#)
        #expect(partial.views.mix.bottom == PaneState(shown: false, view: .editor, size: 200))
        #expect(partial.views.mix.left == ViewLayouts().mix.left)
        #expect(partial.views.arrange == ViewLayouts().arrange)
        let notAnObject = EditorLayout.decode(#"{"views":7,"laneHeight":70}"#)
        #expect(notAnObject.views == ViewLayouts() && notAnObject.laneHeight == 70)
    }
    /// Final review n2: a window move that ends where it began writes nothing.
    @Test func anUnchangedFrameWritesNothing() {
        let e = editor()
        e.setDocumentFrame([10, 20, 1200, 800])
        let drawn = e.drawVersion
        e.setDocumentFrame([10, 20, 1200, 800])
        #expect(e.drawVersion == drawn)
        e.setDocumentFrame([11, 20, 1200, 800])
        #expect(e.drawVersion == drawn + 1 && e.layout.views.frame == [11, 20, 1200, 800])
    }
    @Test func anOlderLayoutWithoutViewsDecodes() {
        let l = EditorLayout.decode(#"{"version":1,"laneHeight":80}"#)
        #expect(l.views == ViewLayouts())
        #expect(l.laneHeight == 80)
    }
    @Test func openingADetailShowsTheBottomEditor() {
        let e = editor()
        _ = e.setPane(.bottom, view: .mixer, shown: false, size: nil, in: nil)
        e.openDetail(.sequence(1), kind: .list)
        #expect(e.layout.views.arrange.bottom.shown && e.layout.views.arrange.bottom.view == .editor)
        #expect(e.layout.detail?.kind == .list)
    }
    @Test func setPaneRefusesAViewOffItsList() {
        let e = editor()
        let before = e.layout
        #expect(e.setPane(.left, view: .markers, shown: nil, size: nil, in: nil)
            == "markers is not on the left pane's list in the arrange view.")
        #expect(e.layout == before)
    }
    @Test func viewsRoundTripThroughTheEncodedLayout() {
        var l = EditorLayout()
        l.views.main = .mix
        l.views.mix.right = PaneState(shown: true, view: .mediaPool, size: 333)
        l.views.frame = [10, 20, 1200, 800]
        #expect(EditorLayout.decode(l.encoded()) == l)
    }
}
