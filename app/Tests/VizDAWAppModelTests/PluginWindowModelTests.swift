import Foundation
import Testing
import VizDAWCore
@testable import VizDAWAppModel

@MainActor
@Suite struct PluginWindowModelTests {
    func param(_ id: UInt64, _ name: String, value: Double = 0.5, stepped: Bool = false, hidden: Bool = false) -> ParamRow {
        ParamRow(
            id: id, name: name, module: "", min: 0, max: 1, default: 0.5, value: value,
            stepped: stepped, automatable: true, hidden: hidden)
    }

    @Test func titleNamesStripAndPluginAndHiddenParamsAreLeftOut() throws {
        let f = mixerFake()
        f.paramTable[10] = [param(0, "Gain"), param(1, "Secret", hidden: true), param(2, "Mode", value: 2.6, stepped: true)]
        let w = PluginWindowModel(document: try DocumentModel(session: f), slot: 10)
        #expect(w.title == "Synth — Gain")
        #expect(w.visibleParams.map(\.name) == ["Gain", "Mode"])
        #expect(PluginWindowModel.valueText(w.visibleParams[1]) == "3")
        #expect(PluginWindowModel.valueText(w.visibleParams[0]) == "0.50")
        #expect(!w.isGone && w.isLive)
    }

    @Test func aParameterDragIsHeardAndCommittedOnce() throws {
        let f = mixerFake()
        f.paramTable[10] = [param(0, "Gain")]
        let w = PluginWindowModel(document: try DocumentModel(session: f), slot: 10)
        w.setParam(0, 0.7)
        w.setParam(0, 0.2)
        w.commitParam(0)
        #expect(f.liveSets.map(\.2) == [0.7, 0.2])
        #expect(f.calls == ["commitParams 10 0=0.2"])
    }

    @Test func theEditorOpensWhenThePluginHasOneElseParametersShow() throws {
        let f = mixerFake()
        let w = PluginWindowModel(document: try DocumentModel(session: f), slot: 10)
        #expect(!w.openEditor(parent: 0x1000))
        #expect(w.editorSize == nil)
        #expect(w.editorProblem?.contains("editor unavailable") == true)
        f.editors[10] = EditorSize(width: 400, height: 300, resizable: false)
        #expect(w.openEditor(parent: 0x1000))
        #expect(w.editorSize?.width == 400)
        w.resize(width: 500, height: 500)
        #expect(w.editorSize?.width == 400, "a fixed-size editor keeps its size")
        w.opened()
        #expect(f.windowsOpen == [10])
        w.closed()
        #expect(f.closedEditors == [10])
        #expect(f.windowsOpen.isEmpty)
    }

    @Test func aPlaceholderShowsItsReasonAndARemovedSlotIsGone() throws {
        let f = mixerFake()
        f.mixerSnap.strips[0].inserts[0].status = .placeholder(reason: "Disabled in Plugin Manager")
        let w = PluginWindowModel(document: try DocumentModel(session: f), slot: 10)
        #expect(!w.openEditor(parent: 0x1000))
        #expect(w.editorProblem == "Disabled in Plugin Manager")
        f.mixerSnap.strips[0].inserts = []
        w.refresh()
        #expect(w.isGone)
    }

    @Test func anUnloadedInstanceDropsItsEditorAndTheNextOneOpensAgain() throws {
        let f = mixerFake()
        f.editors[10] = EditorSize(width: 400, height: 300, resizable: false)
        let w = PluginWindowModel(document: try DocumentModel(session: f), slot: 10)
        #expect(w.openEditor(parent: 0x1000))
        // Turned off in the Plugin Manager: the slot stays, a placeholder.
        f.mixerSnap.strips[0].inserts[0].status = .placeholder(reason: "Disabled in Plugin Manager")
        w.refresh()
        #expect(!w.isGone)
        #expect(w.editorSize == nil)
        #expect(w.placeholderReason == "Disabled in Plugin Manager")
        // Turned on again: the new instance's editor opens.
        f.mixerSnap.strips[0].inserts[0].status = .live
        w.refresh()
        #expect(w.openEditor(parent: 0x1000))
        #expect(w.editorSize?.width == 400)
        #expect(w.closed())
    }

    @Test func bypassFromTheHeader() throws {
        let f = mixerFake()
        let w = PluginWindowModel(document: try DocumentModel(session: f), slot: 10)
        w.toggleBypass()
        #expect(f.calls == ["bypass 10 true"])
    }

    @Test func aBusyCloseIsPendingAndRetriedUntilItSucceeds() throws {
        let f = mixerFake()
        f.editors[10] = EditorSize(width: 400, height: 300, resizable: false)
        let w = PluginWindowModel(document: try DocumentModel(session: f), slot: 10)
        #expect(w.openEditor(parent: 0x1000))
        f.closeBusy = 1
        #expect(!w.closed(), "the view must be kept")
        #expect(w.closePending)
        #expect(f.closedEditors.isEmpty)
        #expect(f.windowsOpen.isEmpty)
        #expect(w.retryClose(), "the next tick succeeds")
        #expect(!w.closePending)
        #expect(f.closedEditors == [10])
        #expect(f.endedGestures == [10])
        #expect(w.retryClose(), "nothing left to retry")
        #expect(f.closedEditors == [10])
    }

    @Test func aDragLeftOpenByTheClosingWindowIsCommitted() throws {
        let f = mixerFake()
        f.paramTable[10] = [param(0, "Gain")]
        let w = PluginWindowModel(document: try DocumentModel(session: f), slot: 10)
        w.setParam(0, 0.9)
        #expect(w.closed())
        #expect(f.calls == ["commitParams 10 0=0.9"])
        w.endOpenDrags()
        #expect(f.calls == ["commitParams 10 0=0.9"], "committed once")
    }

    @Test func aBusyRefreshKeepsTheWindowAndItsParameters() throws {
        let f = mixerFake()
        f.paramTable[10] = [param(0, "Gain")]
        let w = PluginWindowModel(document: try DocumentModel(session: f), slot: 10)
        w.opened()
        f.mixerFetchError = VizError.Busy(message: "busy")
        f.readBusy = true
        #expect(!w.refresh())
        #expect(!w.isGone)
        #expect(w.visibleParams.count == 1)
        #expect(w.title == "Synth — Gain")
        #expect(f.windowsOpen == [10], "still saved as open")
        f.mixerFetchError = nil
        f.readBusy = false
        #expect(w.refresh())
    }

    @Test func aBusyEditorOpenIsOwedAndRetried() throws {
        let f = mixerFake()
        f.editors[10] = EditorSize(width: 400, height: 300, resizable: false)
        let w = PluginWindowModel(document: try DocumentModel(session: f), slot: 10)
        f.openBusy = 1
        #expect(!w.openEditor(parent: 0x1000))
        #expect(w.openOwed)
        #expect(w.editorSize == nil)
        #expect(w.openEditor(parent: 0x1000))
        #expect(!w.openOwed)
        #expect(w.editorSize?.width == 400)
    }
}
