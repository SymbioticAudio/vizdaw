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
        #expect(f.gestures.map(\.1) == [.touch(value: 0.7), .value(value: 0.2), .release])
        #expect(f.gestures.allSatisfy { $0.0 == .pluginParam(slot: 10, param: 0) })
        #expect(f.calls.isEmpty, "the core commits the step at the release")
        #expect(w.params.first?.value == 0.2)
    }

    /// Ruling 8: values are normalised by the parameter's range.
    @Test func sliderValuesAreNormalisedByTheRange() throws {
        let f = mixerFake()
        f.paramTable[10] = [
            ParamRow(
                id: 3, name: "Cutoff", module: "", min: 20, max: 20_020, default: 1000, value: 1000,
                stepped: false, automatable: true, hidden: false)
        ]
        let w = PluginWindowModel(document: try DocumentModel(session: f), slot: 10)
        w.setParam(3, 10_020)
        w.setParam(3, 99_999)
        w.commitParam(3)
        #expect(f.gestures.map(\.1) == [.touch(value: 0.5), .value(value: 1), .release])
    }

    /// A range with `max <= min` is 0; a NaN never reaches the core.
    @Test func anEmptyRangeIsZeroAndNaNIsClamped() throws {
        let f = mixerFake()
        f.paramTable[10] = [
            ParamRow(
                id: 1, name: "Fixed", module: "", min: 5, max: 5, default: 5, value: 5,
                stepped: false, automatable: true, hidden: false),
            param(2, "Gain"),
        ]
        let w = PluginWindowModel(document: try DocumentModel(session: f), slot: 10)
        w.setParam(1, 7)
        w.commitParam(1)
        w.setParam(2, .nan)
        w.setParam(2, .infinity)
        w.setParam(2, -.infinity)
        w.commitParam(2)
        #expect(
            f.gestures.map(\.1) == [
                .touch(value: 0), .release, .touch(value: 0), .value(value: 1), .value(value: 0), .release,
            ])
        #expect(w.params.allSatisfy { !$0.value.isNaN }, "the shown value is never NaN")
    }

    /// The release lands the step: the document shows it at once.
    @Test func theReleaseAppliesTheSnapshot() throws {
        let f = mixerFake()
        f.paramTable[10] = [param(0, "Gain")]
        let doc = try DocumentModel(session: f)
        let w = PluginWindowModel(document: doc, slot: 10)
        w.setParam(0, 0.7)
        f.snap.version += 1
        w.commitParam(0)
        #expect(doc.snapshot.version == f.snap.version)
        #expect(w.params.first?.value == 0.7)
    }

    /// A refused touch is not kept: the drag's later moves send nothing and
    /// its end sends no release; the next drag touches again.
    @Test func aRefusedTouchIsNotKept() throws {
        let f = mixerFake()
        f.paramTable[10] = [param(0, "Gain")]
        let doc = try DocumentModel(session: f)
        let w = PluginWindowModel(document: doc, slot: 10)
        f.gestureError = .Busy(message: "Finish the open edit first.")
        w.setParam(0, 0.7)
        #expect(doc.alert?.title == "Parameter Failed")
        f.gestureError = nil
        w.setParam(0, 0.8)
        w.commitParam(0)
        #expect(f.gestures.isEmpty)
        #expect(w.params.first?.value == 0.5, "not heard, not shown")
        w.endOpenDrags()
        #expect(f.gestures.isEmpty)
        w.setParam(0, 0.3)
        w.commitParam(0)
        #expect(f.gestures.map(\.1) == [.touch(value: 0.3), .release])
    }

    /// A move the core refuses keeps the taken touch: the release follows.
    @Test func aFailingMoveStillReleasesTheTakenTouch() throws {
        let f = mixerFake()
        f.paramTable[10] = [param(0, "Gain")]
        let doc = try DocumentModel(session: f)
        let w = PluginWindowModel(document: doc, slot: 10)
        w.setParam(0, 0.7)
        f.valueError = .Model(message: "refused")
        w.setParam(0, 0.2)
        #expect(doc.alert?.title == "Parameter Failed")
        f.valueError = nil
        w.commitParam(0)
        #expect(f.gestures.map(\.1) == [.touch(value: 0.7), .release])
    }

    /// The parameter or the slot going mid-drag releases its touch once.
    @Test func aParameterOrSlotGoingMidDragReleasesOnce() throws {
        let f = mixerFake()
        f.paramTable[10] = [param(0, "Gain"), param(1, "Pan")]
        let w = PluginWindowModel(document: try DocumentModel(session: f), slot: 10)
        w.setParam(0, 0.7)
        w.setParam(1, 0.1)
        f.paramTable[10] = [param(1, "Pan")]
        w.refresh()
        #expect(f.gestures.map(\.1) == [.touch(value: 0.7), .touch(value: 0.1), .release])
        #expect(f.gestures.last?.0 == .pluginParam(slot: 10, param: 0))
        w.commitParam(0)
        #expect(f.gestures.count == 3, "released once")
        f.mixerSnap.strips[0].inserts = []
        f.paramTable[10] = nil
        w.refresh()
        #expect(w.isGone)
        #expect(f.gestures.count == 4)
        #expect(f.gestures.last?.0 == .pluginParam(slot: 10, param: 1))
        #expect(f.gestures.last?.1 == .release)
        w.endOpenDrags()
        #expect(f.gestures.count == 4, "released once")
    }

    /// A Busy refresh mid-drag releases nothing and keeps the dragged value.
    @Test func aBusyRefreshMidDragKeepsTheTouch() throws {
        let f = mixerFake()
        f.paramTable[10] = [param(0, "Gain")]
        let w = PluginWindowModel(document: try DocumentModel(session: f), slot: 10)
        w.setParam(0, 0.7)
        f.readBusy = true
        f.mixerFetchError = VizError.Busy(message: "busy")
        w.refresh()
        f.readBusy = false
        f.mixerFetchError = nil
        w.refresh()
        #expect(f.gestures.map(\.1) == [.touch(value: 0.7)])
        #expect(w.params.first?.value == 0.7, "the dragged value stays shown")
        w.commitParam(0)
        #expect(f.gestures.map(\.1) == [.touch(value: 0.7), .release])
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
        #expect(f.gestures.map(\.1) == [.touch(value: 0.9), .release])
        w.endOpenDrags()
        #expect(f.gestures.filter { $0.1 == .release }.count == 1, "released once")
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
