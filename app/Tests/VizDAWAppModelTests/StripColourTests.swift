import AppKit
import Foundation
import SymbioticTokens
import Testing
import VizDAWCore
@testable import VizDAWAppModel
@testable import VizDAWEditors

private func pathLane(_ id: UInt64, path: UInt64? = nil, dest: UInt64? = nil) -> LaneRow {
    var l = lane(id, "Lane \(id)")
    l.audioOut = path
    l.midiDest = dest
    return l
}

private func dest(_ id: UInt64, plugin: Bool = false, external: Bool = false) -> DestRow {
    DestRow(
        id: id, name: "Dest \(id)",
        plugin: plugin ? PluginRefRow(format: .clap, id: "x.P\(id)", name: "P\(id)", vendor: "V", version: "1") : nil,
        external: external, returnLatencyMs: 0)
}

/// The strip colour in Arrange (owner G1 Q2, plan 7b Task B7): a lane's
/// header bar and its regions take its strip's colour, as the Mix view's
/// colour bar does; the Colour menu is on the strip name and the lane header.
@MainActor
@Suite struct StripColourTests {
    @Test func theColourMenuListsNoneAndTheEightSwatchesInOrder() {
        #expect(StripPalette.swatches == [
            SwatchRow.red, SwatchRow.orange, SwatchRow.yellow, SwatchRow.green,
            SwatchRow.teal, SwatchRow.blue, SwatchRow.purple, SwatchRow.pink,
        ])
        #expect(StripPalette.swatches.map(StripPalette.title) == [
            "Red", "Orange", "Yellow", "Green", "Teal", "Blue", "Purple", "Pink",
        ])
        #expect(StripPalette.noColourTitle == "None")
    }

    @Test func aLaneTakesItsStripsColourElseItsKindsColour() {
        let t = Theme.studioDark
        var vocal = stripRow(4, "Vocal", kind: .audioPath(path: 7))
        let synth = stripRow(1, "Synth", kind: .builtinSynth)
        let inst = stripRow(5, "Keys", kind: .instrument(dest: 9))
        let strips = [synth, vocal, inst]
        let dests = [dest(8), dest(9, plugin: true), dest(10, external: true)]
        // No own colour: the kind's.
        #expect(StripPalette.lane(pathLane(1, path: 7), destinations: dests, strips: strips, t) == t.strip.channel)
        #expect(StripPalette.lane(pathLane(2, dest: 8), destinations: dests, strips: strips, t) == t.strip.instrument)
        #expect(StripPalette.lane(pathLane(3, dest: 9), destinations: dests, strips: strips, t) == t.strip.instrument)
        // Its own colour, shared by every lane on the path.
        vocal.colour = SwatchRow.pink
        let coloured = [synth, vocal, inst]
        #expect(StripPalette.lane(pathLane(1, path: 7), destinations: dests, strips: coloured, t) == t.swatch.pink)
        #expect(StripPalette.lane(pathLane(6, path: 7), destinations: dests, strips: coloured, t) == t.swatch.pink)
        // External MIDI has no strip: its destination kind's colour, whatever
        // colour the strips have.
        #expect(StripPalette.lane(pathLane(4, dest: 10), destinations: dests, strips: coloured, t) == t.strip.instrument)
        #expect(StripPalette.laneStrip(pathLane(4, dest: 10), destinations: dests, strips: coloured) == nil)
        // No destination and no path: nothing to colour by.
        #expect(StripPalette.lane(pathLane(5), destinations: dests, strips: coloured, t) == nil)
    }

    /// Song's Chords lane (31) plays to path 7, whose strip is Vocal (4).
    func header() throws -> (FakeSession, DocumentModel, MixerModel, LaneHeaderView) {
        let f = editorFake()
        f.laneTable[3]?[1].audioOut = 7
        f.mixerSnap = MixerSnapshot(
            version: f.snap.version,
            strips: [stripRow(4, "Vocal", kind: .audioPath(path: 7)), stripRow(0, "Master", kind: .master)],
            buses: [])
        let m = try DocumentModel(session: f)
        let mixer = try MixerModel(document: m)
        m.storeMixer = mixer
        return (f, m, mixer, LaneHeaderView(document: m))
    }

    @Test func theDocumentColoursItsLanesThroughItsMixer() throws {
        // `storeMixer` is weak: the test holds each mixer.
        let (f, m, first, _) = try header()
        let t = Theme.classicLight
        #expect(m.laneColour(m.lanes[1], t) == t.strip.channel)
        f.mixerSnap.strips[0].colour = SwatchRow.teal
        let mixer = try MixerModel(document: m)
        m.storeMixer = mixer
        #expect(m.laneColour(m.lanes[1], t) == t.swatch.teal)
        withExtendedLifetime((first, mixer)) {}
        m.storeMixer = nil
        #expect(m.laneColour(m.lanes[1], t) == nil, "no mixer yet: the theme's region colour")
    }

    @Test func theLaneHeadersColourMenuSetsTheStripColour() throws {
        let (f, m, mixer, h) = try header()
        defer { withExtendedLifetime(mixer) {} }
        let menu = h.laneMenu(seq: 3, lane: m.lanes[1])
        let colour = try #require(menu.items.first { $0.title == "Colour" }?.submenu)
        let items = colour.items.filter { !$0.isSeparatorItem }
        #expect(items.map(\.title) == ["None", "Red", "Orange", "Yellow", "Green", "Teal", "Blue", "Purple", "Pink"])
        #expect(items[0].state == .on, "no colour yet")
        #expect(items.dropFirst().allSatisfy { $0.image != nil }, "each swatch shows its colour")
        let red = items[1]
        _ = red.target?.perform(red.action)
        #expect(f.calls.last == "setStripColour 4 red")
        // A lane without a strip offers no colour.
        let other = h.laneMenu(seq: 3, lane: m.lanes[0])
        #expect(other.items.first { $0.title == "Colour" }?.isEnabled != true)
    }
}

/// Fix round 1 (controller): the number boxes are readouts in every theme.
@Suite struct StripReadoutTests {
    @Test func theNumberBoxesTakeTheReadoutTokens() {
        for t in Theme.all {
            let r = StripPalette.numberBox(t)
            #expect(r.fill == t.readout.background, "\(t.id)")
            #expect(r.digits == t.readout.text, "\(t.id)")
            #expect(r.edge == t.readout.text, "\(t.id)")
        }
        #expect(StripPalette.numberBox(.eg13).digits != StripPalette.numberBox(.console).digits)
    }
}

/// Fix round 2 (review I1, M3, M6).
@MainActor
@Suite struct StripColourRound2Tests {
    /// Song's Chords lane (31) plays through path 7 (Vocal, teal); the
    /// document's mixer is made only when asked for, as when no pane shows a
    /// strip or the mixer.
    func onDemand() throws -> (FakeSession, DocumentModel, () -> MixerModel?, Int) {
        let f = editorFake()
        f.laneTable[3]?[1].audioOut = 7
        f.mixerSnap = MixerSnapshot(
            version: f.snap.version,
            strips: [stripRow(4, "Vocal", kind: .audioPath(path: 7)), stripRow(0, "Master", kind: .master)],
            buses: [])
        f.mixerSnap.strips[0].colour = SwatchRow.teal
        let m = try DocumentModel(session: f)
        var made = 0
        var mixer: MixerModel?
        let provider: () -> MixerModel? = {
            if mixer == nil {
                mixer = try? MixerModel(document: m)
                made += 1
                m.storeMixer = mixer
                m.mixerArrived()
            }
            return mixer
        }
        m.mixerProvider = provider
        return (f, m, provider, made)
    }

    @Test func aDocumentWithNoMixerPaneStillColoursItsLanesAndOffersColour() throws {
        let (_, m, provider, _) = try onDemand()
        #expect(m.storeMixer == nil, "no pane made the mixer")
        let version = m.mixerVersion
        let t = Theme.studioDark
        #expect(m.laneColour(m.lanes[1], t) == t.swatch.teal)
        #expect(m.mixerVersion > version, "the mixer's arrival is observable")
        let keep = provider()
        let h = LaneHeaderView(document: m)
        let colour = try #require(h.laneMenu(seq: 3, lane: m.lanes[1]).items.first { $0.title == "Colour" })
        #expect(colour.isEnabled)
        let v = ArrangementView(document: m)
        #expect(v.tint(1) == v.theme.swatch.teal.nsColor)
        withExtendedLifetime(keep) {}
    }

    @Test func aFailedMixerIsAskedForOnce() throws {
        let f = editorFake()
        let m = try DocumentModel(session: f)
        var asks = 0
        m.mixerProvider = {
            asks += 1
            return nil
        }
        _ = m.laneColour(m.lanes[0], .classicLight)
        _ = m.laneColour(m.lanes[1], .classicLight)
        #expect(asks == 1, "drawing does not retry a mixer that could not be made")
    }

    @Test func colourBarsFollowThePreferenceAndRegionTintsDoNot() throws {
        let (_, m, provider, _) = try onDemand()
        let keep = provider()
        let prefs = AppearancePreferences(defaults: MemoryDefaults())
        let h = LaneHeaderView(document: m)
        h.appearancePrefs = prefs
        #expect(h.colourBar(m.lanes[1]) != nil)
        prefs.setColourBars(false)
        #expect(h.colourBar(m.lanes[1]) == nil, "the bar hides with the preference")
        let v = ArrangementView(document: m)
        #expect(v.tint(1) != nil, "regions keep the strip's colour")
        withExtendedLifetime(keep) {}
    }

    @Test func pickingTheCurrentColourInTheLaneMenuSendsNothing() throws {
        let (f, m, provider, _) = try onDemand()
        let keep = provider()
        let h = LaneHeaderView(document: m)
        let colour = try #require(h.laneMenu(seq: 3, lane: m.lanes[1]).items.first { $0.title == "Colour" }?.submenu)
        let teal = try #require(colour.items.first { $0.title == "Teal" })
        #expect(teal.state == .on)
        _ = teal.target?.perform(teal.action)
        #expect(f.calls.isEmpty, "the strip is teal already")
        withExtendedLifetime(keep) {}
    }
}
