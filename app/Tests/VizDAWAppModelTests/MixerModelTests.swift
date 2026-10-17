import Foundation
import Testing
import VizDAWCore
@testable import VizDAWAppModel

@MainActor
@Suite struct MixerMathTests {
    @Test func faderPositionsRoundTripAndFloorIsMinusInfinity() {
        #expect(MixerMath.faderPosition(-144) == 0)
        #expect(MixerMath.faderPosition(12) == 1)
        #expect(MixerMath.faderDb(0) == -144)
        for db in [-59.5, -24.5, -3.0, 0.0, 6.0, 12.0] {
            #expect(abs(MixerMath.faderDb(MixerMath.faderPosition(db)) - db) < 0.06, "\(db)")
        }
    }

    @Test func labels() {
        #expect(MixerMath.dbLabel(-144) == "−∞")
        #expect(MixerMath.dbLabel(3.5) == "+3.5")
        #expect(MixerMath.dbLabel(-12) == "-12.0")
        #expect(MixerMath.dbLabel(0) == "0.0")
        #expect(MixerMath.panLabel(0) == "C")
        #expect(MixerMath.panLabel(-0.5) == "L50")
        #expect(MixerMath.panLabel(0.25) == "R25")
        #expect(MixerMath.meterFill(0) == 0)
        #expect(MixerMath.meterFill(1) > 0.9 && MixerMath.meterFill(1) < 0.91)
        #expect(MixerMath.meterFill(.nan) == 0)
    }
}

@MainActor
@Suite struct MixerModelTests {
    func model(_ f: FakeSession) throws -> MixerModel {
        try MixerModel(document: DocumentModel(session: f))
    }

    @Test func masterIsSeparateAndOutputsAreNamed() throws {
        let f = mixerFake()
        f.dests = [DestRow(id: 7, name: "Piano", plugin: nil)]
        f.mixerSnap.strips[0].output = 2
        let m = try model(f)
        #expect(m.strips.map(\.name) == ["Synth", "Bus 1"])
        #expect(m.master?.name == "Master")
        #expect(m.outputName(m.strips[0]) == "Bus 1")
        #expect(m.outputName(m.strips[1]) == "Master")
        #expect(m.targets(for: m.strips[1]).isEmpty, "a bus does not send to itself")
        #expect(m.sidechainSources(for: m.strips[0]).map(\.name) == ["Bus 1"])
        #expect(m.destinations.map(\.name) == ["Piano"])
    }

    @Test func aFaderDragIsOneEditAndHeardOnEveryMove() throws {
        let f = mixerFake()
        let m = try model(f)
        m.dragStarted("Set Fader")
        m.setGain(1, db: -3)
        m.setGain(1, db: -6)
        m.dragEnded()
        #expect(f.calls == ["begin Set Fader", "setGain 1 -3.0", "setGain 1 -6.0", "end"])
    }

    @Test func theMixerIsRefetchedOnlyWhenTheDocumentOrSlotsChange() throws {
        let f = mixerFake()
        let doc = try DocumentModel(session: f)
        let m = try MixerModel(document: doc)
        let before = f.mixerCalls
        m.refreshIfNeeded()
        #expect(f.mixerCalls == before)
        doc.rename(1, to: "Kit")
        m.refreshIfNeeded()
        #expect(f.mixerCalls == before + 1)
        f.idleUpdates = [IdleUpdate(snapshot: nil, slotsChanged: true, paramsChanged: [], problems: [])]
        doc.idle()
        m.refreshIfNeeded()
        #expect(f.mixerCalls == before + 2)
    }

    @Test func metersHoldPeaksAndFall() throws {
        let f = mixerFake()
        let m = try model(f)
        f.levelsValue = MixerLevels(
            meters: [MeterRow(strip: 1, peakL: 1, peakR: 0.5)], dspLoad: 0.25, dspPeak: 0.5,
            latencyFrames: 480, faulted: [10])
        m.pollLevels()
        #expect(m.meters[1] == MeterLevel(left: 1, right: 0.5))
        #expect(m.faulted == [10])
        f.levelsValue.meters = [MeterRow(strip: 1, peakL: 0, peakR: 0.6)]
        m.pollLevels()
        #expect(m.meters[1]?.left == MixerMath.meterDecay)
        #expect(m.meters[1]?.right == 0.6)
        #expect(m.loadText == "DSP 25% · peak 50%", "no rate before Play")
    }

    @Test func busesGetFreeNamesAndEditsReachTheSession() throws {
        let f = mixerFake()
        let m = try model(f)
        m.addBus()
        m.toggleMute(m.strips[0])
        m.setOutput(1, bus: 2)
        // The session answers with the strip's new send.
        f.mixerSnap.strips[0].sends = [SendRow(target: 2, levelDb: 0, preFader: false, enabled: true)]
        m.addSend(1, to: 2)
        m.editSend(1, 0) { $0.preFader = true }
        m.toggleBypass(m.strips[0].inserts[0])
        m.setSidechain(10, source: 2)
        m.moveInsert(10, to: 2, at: 0)
        #expect(f.calls == [
            "addBus Bus 2", "mute 1 true", "output 1 2", "addSend 1 2",
            "editSend 1 0 2 0.0 true true", "bypass 10 true", "sidechain 10 2", "moveInsert 10 2 0",
        ])
    }

    @Test func theBrowserPickGoesWhereItWasOpened() throws {
        let f = mixerFake()
        let m = try model(f)
        let p = PluginRefRow(format: .clap, id: "u.diva", name: "Diva", vendor: "u-he", version: "1")
        m.browser = .insert(strip: 1)
        m.pick(p)
        m.browser = .instrument
        #expect(m.browser?.wantsInstrument == true)
        m.pick(p)
        m.browser = .destination(7)
        m.pick(p)
        #expect(m.browser == nil)
        #expect(f.calls == ["addInsert 1 end u.diva", "addInstrument Diva u.diva", "destPlugin 7 u.diva"])
        #expect(MixerModel.BrowserTarget.insert(strip: 1).wantsInstrument == false)
    }

    @Test func renamingAndErrors() throws {
        let f = mixerFake()
        let m = try model(f)
        m.beginRename(m.strips[1])
        #expect(m.renameDraft == "Bus 1")
        m.renameDraft = "  Verb "
        m.commitRename()
        m.beginRename(m.strips[1])
        m.commitRename()  // unchanged: no command
        #expect(f.calls == ["renameStrip 2 Verb"])
        f.mixerError = .Model(message: "routing cycle")
        m.setOutput(2, bus: 1)
        #expect(m.document.alert?.title == "Output Failed")
        #expect(m.document.alert?.message == "routing cycle")
    }

    @Test func aBusySessionSkipsTheRefreshSilentlyAndRetries() throws {
        let f = mixerFake()
        let doc = try DocumentModel(session: f)
        let m = try MixerModel(document: doc)
        let rows = m.strips.map(\.name)
        f.mixerSnap.strips.append(stripRow(3, "New"))
        f.mixerFetchError = .Busy(message: "editor opening")
        doc.rename(1, to: "Kit")
        m.refreshIfNeeded()
        m.pollLevels()
        #expect(doc.alert == nil)
        #expect(m.strips.map(\.name) == rows, "the previous rows stay")
        f.mixerFetchError = nil
        m.refreshIfNeeded()
        #expect(m.strips.map(\.name) == rows + ["New"], "retried on the next tick")
        #expect(doc.alert == nil)
    }

    @Test func aDragIsEndedOnceWhenTheWindowClosesOrTheStripGoes() throws {
        let f = mixerFake()
        let m = try model(f)
        m.dragStarted("Set Fader", strip: 1)
        m.endOpenDrag()  // window closes
        m.endOpenDrag()
        m.dragEnded()
        #expect(f.calls == ["begin Set Fader", "end"])
        f.calls = []
        m.dragStarted("Set Fader", strip: 2)
        f.mixerSnap.strips.removeAll { $0.id == 2 }
        m.setGain(1, db: 0)  // refetch sees strip 2 gone
        #expect(f.calls == ["begin Set Fader", "setGain 1 0.0", "end"])
        m.dragEnded()
        #expect(f.calls.filter { $0 == "end" }.count == 1)
    }

    @Test func aFailedBeginMeansNoEnd() throws {
        let f = mixerFake()
        let m = try model(f)
        f.beginError = .Model(message: "open")
        m.dragStarted("Set Fader", strip: 1)
        m.dragEnded()
        #expect(!f.calls.contains("end"))
    }

    @Test func aNewDragEndsTheOldOne() throws {
        let f = mixerFake()
        let m = try model(f)
        m.dragStarted("A", strip: 1)
        m.dragStarted("B", strip: 1)
        #expect(f.calls == ["begin A", "end", "begin B"])
    }

    @Test func browserModelsAreReusedWhileOpenAndResetOnClose() throws {
        let m = try model(mixerFake())
        m.browser = .instrument
        let a = m.browserModel(.instrument) { PluginBrowserModel(kind: .instrument) { [] } }
        let b = m.browserModel(.instrument) { PluginBrowserModel(kind: .instrument) { [] } }
        #expect(a === b)
        m.browser = nil
        let c = m.browserModel(.instrument) { PluginBrowserModel(kind: .instrument) { [] } }
        #expect(a !== c)
    }

    @Test func metersSanitiseNonFinitePeaks() throws {
        let f = mixerFake()
        let m = try model(f)
        f.levelsValue.meters = [MeterRow(strip: 1, peakL: .nan, peakR: .infinity)]
        m.pollLevels()
        #expect(m.meters[1]?.left == 0)
        #expect(m.meters[1]?.right == MixerMath.maxMeterPeak)
        f.levelsValue.meters = [MeterRow(strip: 1, peakL: 0, peakR: 0)]
        m.pollLevels()
        #expect(m.meters[1]?.right == MixerMath.maxMeterPeak * MixerMath.meterDecay)
        #expect(MixerMath.dbLabel(.nan) == "−∞")
    }

    @Test func aBusyLoadRefetchIsOwedEvenWithoutAVersionChange() throws {
        let f = mixerFake()
        f.held = [10]
        let m = try model(f)
        f.mixerFetchError = .Busy(message: "b")
        m.load([10])
        f.held = []
        f.mixerFetchError = nil
        m.refreshIfNeeded()
        #expect(m.held.isEmpty)
    }

    @Test func identicalCommandAlertsAreNotReSet() throws {
        let f = mixerFake()
        let m = try model(f)
        f.mixerError = .Model(message: "no")
        m.setGain(1, db: 0)
        let first = m.document.alert
        m.setGain(1, db: 1)
        #expect(m.document.alert?.id == first?.id)
    }

    @Test func safeModeLoadsAStripsHeldSlots() throws {
        let f = mixerFake()
        f.held = [10]
        let m = try model(f)
        #expect(m.heldSlots(of: m.strips[0]) == [10])
        m.load(m.heldSlots(of: m.strips[0]))
        #expect(f.loadedPlugins == [[10]])
        #expect(m.held.isEmpty)
    }
}

@MainActor
@Suite struct StoreMixerTests {
    @Test func oneMixerPerDocumentGoneWithIt() throws {
        let f = mixerFake()
        let store = DocumentStore(makeEmpty: { f }, makeDemo: { f }, open: { _ in f })
        let id = try #require(store.newEmpty())
        let m = try #require(store.mixer(id))
        #expect(store.mixer(id) === m)
        store.close(id)
        #expect(store.mixer(id) == nil)
        #expect(store.mixer(UUID()) == nil)
    }
}
