import Foundation
import Testing
import SymbioticTokens
import VizDAWCore
@testable import VizDAWAppModel

@MainActor
@Suite struct MixerStripTests {
    func model(_ f: FakeSession) throws -> MixerModel {
        try MixerModel(document: DocumentModel(session: f))
    }

    @Test func faderCapsFollowTheStripKindInEveryTheme() {
        let kinds: [StripColourRow] = [.channel, .instrument, .auxReturn, .bus, .master]
        for t in Theme.all {
            let tokens = [t.strip.channel, t.strip.instrument, t.strip.auxReturn, t.strip.bus, t.strip.master]
            for (k, token) in zip(kinds, tokens) { #expect(StripPalette.cap(k, t) == token, "\(t.id) \(k)") }
            #expect(Set(kinds.map { StripPalette.cap($0, t) }).count == 5, "\(t.id): five distinct caps")
            var blue = stripRow(1, "S", kind: .builtinSynth)
            blue.colour = .blue
            #expect(StripPalette.bar(blue, t) == t.swatch.blue)
            #expect(StripPalette.cap(blue.kindColour, t) == t.strip.instrument)
            let plain = stripRow(2, "B")
            #expect(StripPalette.bar(plain, t) == StripPalette.cap(plain.kindColour, t))
        }
    }

    /// Final review m4: the I toggle (strip and lane header) lights in the
    /// state tokens, not the user's strip colours: input in state.warning,
    /// auto in state.play.
    @Test func theInputMonitorLightsInStateTokens() {
        for t in Theme.all {
            #expect(StripPalette.monitor(.input, t) == t.state.warning, "\(t.id)")
            #expect(StripPalette.monitor(.auto, t) == t.state.play, "\(t.id)")
            #expect(StripPalette.monitor(.off, t) == t.state.play, "\(t.id): unlit, the auto colour")
        }
    }

    @Test func knobsAndAutomationUseTheirTokens() {
        for t in Theme.all {
            #expect(StripPalette.knob(.pan, t) == t.control.pan)
            #expect(StripPalette.knob(.gain, t) == t.control.gain)
            #expect(StripPalette.knob(.send, t) == t.control.send)
            #expect(StripPalette.automation(.write, t) == t.state.record)
            #expect(StripPalette.automation(.read, t) == t.state.play)
            #expect(StripPalette.automation(.touch, t) == t.state.solo)
            #expect(StripPalette.automation(.latch, t) == t.state.warning)
            #expect(StripPalette.automation(.trim, t) == t.control.send)
            #expect(StripPalette.automation(.off, t) == t.state.offline)
            #expect(StripPalette.meter(1.0, t) == t.meter.clip)
            #expect(StripPalette.meter(2.0, t) == t.meter.clip)
            #expect(StripPalette.meter(0.01, t) == t.meter.low)
            #expect(StripPalette.meter(0, t) == t.meter.low)
            #expect(StripPalette.meter(0.2, t) == t.meter.mid)
            #expect(StripPalette.meter(0.8, t) == t.meter.high)
            #expect(StripPalette.swatch(.pink, t) == t.swatch.pink)
            #expect(StripPalette.swatch(.red, t) == t.swatch.red)
        }
    }

    @Test func typingALevelIsOneClickAndARefusalChangesNothing() throws {
        let f = mixerFake()
        let m = try model(f)
        let synth = m.strips[0]
        let t = AutomationTargetRow.stripFader(strip: synth.id)
        #expect(m.typeLevel(synth, "-4.2"))
        let v = AutomationLaw.faderNorm(-4.2)
        #expect(f.gestures.map(\.0) == [t, t, t])
        #expect(f.gestures.map(\.1) == [.touch(value: v), .value(value: v), .release])
        let n = f.gestures.count
        #expect(!m.typeLevel(synth, "abc"))
        #expect(f.gestures.count == n)
        #expect(m.document.editor.status == StripEntry.levelHint)
        m.touch(t)
        let n2 = f.gestures.count
        #expect(!m.typeLevel(synth, "-6"))
        #expect(f.gestures.count == n2)
        #expect(m.document.editor.status == StripEntry.touchedHint)
    }

    @Test func typingPanAndSendLevelsUsesTheirTargets() throws {
        let f = mixerFake()
        f.mixerSnap.strips[0].sends = [SendRow(target: 2, levelDb: 0, preFader: false, enabled: true)]
        let m = try model(f)
        let s = m.strips[0]
        #expect(m.typePan(s, "L 24"))
        #expect(f.gestures.last?.0 == .stripPan(strip: s.id))
        #expect(f.gestures.last?.1 == .release)
        #expect(f.gestures.dropLast().last?.1 == .value(value: (-0.24 + 1) / 2))
        #expect(!m.typePan(s, "LR"))
        #expect(m.document.editor.status == StripEntry.panHint)
        #expect(m.typeSend(s, 0, "-6"))
        #expect(f.gestures.last?.0 == .sendLevel(strip: s.id, send: 0))
        #expect(f.gestures.dropLast().last?.1 == .value(value: AutomationLaw.faderNorm(-6)))
        let n = f.gestures.count
        #expect(!m.typeSend(s, 5, "-6"))
        #expect(f.gestures.count == n)
    }

    @Test func theEqSlotLoadsBypassesAndEmpties() throws {
        let f = mixerFake()
        let m = try model(f)
        let s = m.strips[0]
        let ref = PluginRefRow(format: .clap, id: "x.EQ", name: "EQ", vendor: "V", version: "1")
        m.openBrowser(.eq(strip: s.id), in: .window)
        #expect(!(MixerModel.BrowserTarget.eq(strip: s.id)).wantsInstrument)
        m.pick(ref)
        #expect(f.calls == ["setEq \(s.id) x.EQ"])
        let eq = try #require(m.strips[0].eq)
        // The same plug-in again is no step.
        m.openBrowser(.eq(strip: s.id), in: .window)
        m.pick(ref)
        #expect(f.calls == ["setEq \(s.id) x.EQ"], "the plug-in already in the slot")
        m.toggleBypass(eq)
        #expect(f.calls.last == "bypass \(eq.id) true")
        m.clearEq(m.strips[0])
        #expect(f.calls.last == "setEq \(s.id) none")
        f.mixerError = .Model(message: "no")
        m.openBrowser(.eq(strip: s.id), in: .window)
        m.pick(ref)
        #expect(m.document.alert?.title == "Load EQ Failed")
    }

    /// B6 review: the core refuses to clear an empty slot, so an empty EQ
    /// is no call and no "Clear EQ Failed" alert.
    @Test func clearingAnEmptyEqSlotSendsNothing() throws {
        let f = mixerFake()
        let m = try model(f)
        #expect(m.strips[0].eq == nil)
        f.mixerError = .Model(message: "empty slot")
        m.clearEq(m.strips[0])
        #expect(f.calls.isEmpty)
        #expect(m.document.alert == nil)
    }

    @Test func aStripColourIsOneStep() throws {
        let f = mixerFake()
        let m = try model(f)
        let synth = m.strips[0]
        m.setColour(synth, .red)
        #expect(f.calls == ["setStripColour \(synth.id) red"])
        m.setColour(m.strips[0], nil)
        #expect(f.calls.last == "setStripColour \(synth.id) none")
        #expect(f.calls.count == 2)
        f.mixerError = .Model(message: "no")
        m.setColour(m.strips[0], .blue)
        #expect(m.document.alert?.title == "Strip Colour Failed")
    }

    /// B1 review M5: a change to what is already there is no undo step.
    @Test func aNoOpColourSendsNothing() throws {
        let f = mixerFake()
        f.mixerSnap.strips[0].colour = .green
        let m = try model(f)
        m.setColour(m.strips[0], .green)
        m.setColour(m.strips[1], nil)
        #expect(f.calls.isEmpty)
    }

    @Test func pickingTheEqAlreadyInTheSlotSendsNothing() throws {
        let f = mixerFake()
        let ref = PluginRefRow(format: .clap, id: "x.EQ", name: "EQ", vendor: "V", version: "1")
        var eq = slotRow(77, "EQ")
        eq.plugin = ref
        f.mixerSnap.strips[0].eq = eq
        let m = try model(f)
        m.openBrowser(.eq(strip: m.strips[0].id), in: .window)
        m.pick(ref)
        #expect(f.calls.isEmpty)
        #expect(m.browser == nil)
    }

    @Test func theEqSlotIsSeenByEveryStripSlotReader() throws {
        let f = mixerFake()
        var strip = stripRow(1, "Synth", kind: .builtinSynth, inserts: [slotRow(10, "Gain")])
        strip.instrument = slotRow(11, "Inst")
        strip.eq = slotRow(12, "EQ")
        f.mixerSnap.strips[0] = strip
        let m = try model(f)
        #expect(m.strips[0].slots.map(\.id) == [11, 12, 10])
        let w = PluginWindowModel(document: m.document, slot: 12)
        w.refresh()
        #expect(w.row?.id == 12)
        #expect(w.stripName == "Synth")
        f.held = [12]
        m.load([])
        #expect(m.heldSlots(of: m.strips[0]) == [12])
        f.paramTable[12] = [
            ParamRow(
                id: 3, name: "Band", module: "", min: 0, max: 1, default: 0, value: 0, stepped: false,
                automatable: true, hidden: false)
        ]
        let t = try AutomationTargets.load(f)
        #expect(t.menus.contains { $0.title == "EQ \u{00B7} Synth" && $0.lanes == [.automation(.param(slot: 12, param: 3))] })
    }

    @Test func recordArmAndMonitorNeedAnInputLane() throws {
        let f = mixerFake()
        f.mixerSnap.strips.insert(stripRow(4, "Vocal", kind: .audioPath(path: 7)), at: 1)
        var vox = lane(40, "Vox")
        vox.audioOut = 7
        f.laneTable[3] = [vox]
        let m = try model(f)
        m.document.select(3)
        let vocal = m.strips[1]
        #expect(m.canArm(vocal))
        m.toggleArm(vocal)
        #expect(f.edits.last == .lane(edit: .setFlags(seq: 3, lane: 40, mute: nil, solo: nil, arm: true)))
        m.cycleMonitor(vocal)
        #expect(f.edits.last == .lane(edit: .setMonitor(seq: 3, lane: 40, monitor: .input)))
        let n = f.edits.count
        let synth = m.strips[0]
        #expect(!m.canArm(synth))
        m.toggleArm(synth)
        m.cycleMonitor(synth)
        let master = try #require(m.master)
        #expect(!m.canArm(master))
        m.toggleArm(master)
        #expect(f.edits.count == n)
    }
}
