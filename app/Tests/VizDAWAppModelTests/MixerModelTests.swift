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

    /// Plan 5d-2 Ruling 4: an AudioPath strip's input controls edit the
    /// shown sequence's lane that plays through it, the armed one first.
    @Test func anAudioPathStripsInputIsItsLanes() throws {
        let f = mixerFake()
        f.mixerSnap.strips.insert(stripRow(4, "Vocal", kind: .audioPath(path: 7)), at: 0)
        var a = lane(30, "Vocal A")
        a.audioOut = 7
        var b = lane(31, "Vocal B")
        b.audioOut = 7
        f.laneTable[3] = [a, b]
        let m = try model(f)
        m.document.select(3)
        let strip = try #require(m.strips.first)
        #expect(m.inputLane(for: strip)?.id == 30, "the first, with none armed")
        b.arm = true
        f.laneTable[3] = [a, b]
        f.snap.version += 1
        m.document.refreshAfterJob()
        #expect(m.inputLane(for: strip)?.id == 31, "the armed one")
        #expect(m.inputLane(for: m.strips[1]) == nil, "an instrument strip has none")
        f.laneTable[3] = [lane(30, "Keys")]
        f.snap.version += 1
        m.document.refreshAfterJob()
        #expect(m.inputLane(for: strip) == nil, "no lane plays through it in this sequence")
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
        #expect(f.gestures.map(\.0) == [.stripMute(strip: 1), .stripMute(strip: 1), .stripMute(strip: 1)], "mute is a gesture (5e-2)")
        #expect(f.calls == [
            "addBus Bus 2", "output 1 2", "addSend 1 2",
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

    @Test func theInstrumentMenuGroupsByMakerAndMarksTheCurrentPlugin() throws {
        let rows = [
            catalogRow("b.2", "Zeta", vendor: "beta", kind: .instrument),
            catalogRow("a.1", "Mono", vendor: "Alpha", kind: .instrument),
            catalogRow("b.1", "alpha", vendor: "beta", kind: .instrument),
            catalogRow("n.1", "Loose", vendor: "", kind: .instrument),
        ]
        let current = rows[2].plugin
        let menu = MixerModel.instrumentMenu(rows, current: current)
        #expect(menu.map(\.vendor) == ["Alpha", "beta", "Other"])
        #expect(menu[1].choices.map(\.plugin.name) == ["alpha", "Zeta"])
        #expect(menu.flatMap(\.choices).filter(\.isCurrent).map(\.plugin.id) == ["b.1"])
        #expect(MixerModel.instrumentMenu(rows, current: nil).flatMap(\.choices).allSatisfy { !$0.isCurrent })
    }

    @Test func theInstrumentMenuLeavesOutEffectsAndDisabledPlugins() throws {
        let rows = [
            catalogRow("e", "Verb", vendor: "A", kind: .effect),
            catalogRow("d", "Off", vendor: "A", kind: .instrument, status: .disabled),
            catalogRow("m", "Gone", vendor: "A", kind: .instrument, status: .missing),
            catalogRow("k", "Keys", vendor: "A", kind: .instrument),
        ]
        let menu = MixerModel.instrumentMenu(rows, current: nil)
        #expect(menu.map(\.vendor) == ["A"])
        #expect(menu[0].choices.map(\.plugin.id) == ["k"])
        #expect(MixerModel.instrumentMenu([], current: nil).isEmpty)
        #expect(MixerModel.instrumentMenu([rows[0]], current: nil).isEmpty)
    }

    @Test func pickingAnInstrumentReplacesItInThatDestination() throws {
        let f = mixerFake()
        let m = try model(f)
        var strip = stripRow(5, "Keys", kind: .instrument(dest: 7))
        strip.instrument = slotRow(50, "Old")
        let new = PluginRefRow(format: .clap, id: "u.diva", name: "Diva", vendor: "u-he", version: "1")
        let before = f.calls.count
        m.pickInstrument(new, on: strip)
        #expect(Array(f.calls.dropFirst(before)) == ["destPlugin 7 u.diva"])
        #expect(m.document.alert == nil)
    }

    @Test func pickingTheLoadedInstrumentOrOnANonInstrumentStripDoesNothing() throws {
        let f = mixerFake()
        let m = try model(f)
        var strip = stripRow(5, "Keys", kind: .instrument(dest: 7))
        strip.instrument = slotRow(50, "Old")
        let before = f.calls.count
        m.pickInstrument(strip.instrument!.plugin, on: strip)
        m.pickInstrument(strip.instrument!.plugin, on: stripRow(6, "Bus", kind: .bus))
        #expect(f.calls.count == before)
    }

    @Test func aFailingInstrumentPickAlerts() throws {
        let f = mixerFake()
        let m = try model(f)
        f.mixerError = .Model(message: "no such plugin")
        let strip = stripRow(5, "Keys", kind: .instrument(dest: 7))
        m.pickInstrument(PluginRefRow(format: .clap, id: "x", name: "X", vendor: "V", version: "1"), on: strip)
        #expect(m.document.alert?.title == "Instrument Failed")
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
        m.setOutput(1, bus: 2)
        let first = m.document.alert
        m.setOutput(1, bus: nil)
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

    /// 5d-2 T6 Minor 3: a strip's input lane plays through its own path,
    /// in the shown sequence only.
    @Test func anAudioPathStripIgnoresOtherPathsAndOtherSequences() throws {
        let f = mixerFake()
        f.mixerSnap.strips.insert(stripRow(4, "Vocal", kind: .audioPath(path: 7)), at: 0)
        var other = lane(30, "Elsewhere")
        other.audioOut = 8
        f.laneTable[3] = [other]
        var hidden = lane(40, "Hidden")
        hidden.audioOut = 7
        f.laneTable[4] = [hidden]
        let m = try model(f)
        m.document.select(3)
        let strip = try #require(m.strips.first)
        #expect(m.inputLane(for: strip) == nil, "another path, and another sequence's lane")

        // Positive control: a lane on the strip's own path in the shown
        // sequence is the input lane (the nil above is not a blind spot).
        var own = lane(31, "Own")
        own.audioOut = 7
        f.laneTable[3] = [other, own]
        let m2 = try model(f)
        m2.document.select(3)
        let strip2 = try #require(m2.strips.first)
        #expect(m2.inputLane(for: strip2)?.id == 31)
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
