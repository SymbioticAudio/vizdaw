import Foundation
import Testing
import VizDAWCore
@testable import VizDAWAppModel

/// The learn UI's model (plan 5f-1 Task 11; Rulings 7, 8, 11, 18).
@MainActor
@Suite struct LearnModelTests {
    func model() throws -> (LearnModel, FakeSession, FakeSurfaceCore) {
        let f = mixerFake()
        let m = try DocumentModel(session: f)
        let c = FakeSurfaceCore()
        return (LearnModel(document: m, core: c), f, c)
    }

    func row(_ mode: LearnModeRow = .absolute, pickup: Bool = true, picksUp: Bool = true) -> LearnMapRow {
        LearnMapRow(
            source: LearnSourceRow(port: "Knobs", kind: .cc7, channel: 0, number: 7), text: "Knobs · CC 7 ch 1",
            target: .stripFader(strip: 1), missing: false, mode: mode, pickup: pickup, picksUp: picksUp)
    }

    @Test func learningShowsABannerUntilTheMapLands() throws {
        let (l, f, c) = try model()
        c.learnRow = LearnStateRow(waiting: "Fader 1", last: nil)
        l.learn(.stripFader(strip: 1))
        #expect(f.learnTargets == [.stripFader(strip: 1)])
        #expect(l.banner == "Waiting for MIDI… move a control (Esc cancels)")
        f.learnMapRows = [row()]
        c.learnRow = LearnStateRow(waiting: nil, last: "Knobs · CC 7 ch 1")
        l.poll()
        #expect(l.banner == nil)
        #expect(l.maps.map(\.text) == ["Knobs · CC 7 ch 1"])
    }

    @Test func cancelEndsTheLearn() throws {
        let (l, f, c) = try model()
        c.learnRow = LearnStateRow(waiting: "Undo", last: nil)
        l.learnCommand("edit.undo")
        #expect(c.learnCommands == ["edit.undo"] && l.banner != nil)
        // Final review T11 m8: the learn's document took the surfaces' focus.
        #expect(f.focusCalls == 1)
        l.cancel()
        #expect(c.cancels == 1 && l.banner == nil)
    }

    /// Final review R2 and T11 m9: the banner names what the learn waits
    /// for and, with two or more projects open, the project.
    @Test func theBannerNamesWhatItWaitsForAndTheProject() throws {
        #expect(LearnModel.waiting(project: nil) == LearnModel.waitingText)
        #expect(
            LearnModel.waiting(project: "Song")
                == "Waiting for MIDI in “Song”… move a control (Esc cancels)")
        #expect(
            LearnModel.waiting(project: "Song", for: "Undo")
                == "Waiting for MIDI for Undo in “Song”… move a control (Esc cancels)")
        let (l, _, c) = try model()
        c.learnRow = LearnStateRow(waiting: "Fader 1", last: nil)
        l.learn(.stripFader(strip: 1))
        #expect(l.bannerText(project: nil) == "Waiting for MIDI for Fader 1… move a control (Esc cancels)")
        #expect(
            l.bannerText(project: "Song")
                == "Waiting for MIDI for Fader 1 in “Song”… move a control (Esc cancels)")
    }

    @Test func modesAndRemovalReachTheSession() throws {
        let (l, f, _) = try model()
        let r = row()
        l.setMode(r, .relativeBinOffset)
        #expect(f.learnModes.map(\.1) == [.relativeBinOffset] && f.learnModes.first?.0 == r.source)
        l.remove(r)
        #expect(f.removedLearns == [r.source])
        f.learnError = .Model(message: "unknown learn source")
        l.setMode(r, .absolute)
        #expect(l.alert?.title == "MIDI Learn Failed")
    }

    /// Owner answer 2026-11-02 (Ruling 18): pick-up per map, set in the
    /// sheet, only where it applies.
    @Test func pickUpIsShownAndSet() throws {
        let (l, f, _) = try model()
        l.setPickup(row(), false)
        #expect(f.learnPickups.map(\.1) == [false])
        l.setPickup(row(.relativeBinOffset, pickup: false, picksUp: false), true)
        #expect(f.learnPickups.count == 1, "a relative map has no pick-up")
        #expect(LearnModel.pickupHelp == "The control takes over only once it reaches the current value")
    }

    @Test func modeTitles() {
        #expect(LearnModel.modeTitle(.absolute) == "Absolute")
        #expect(LearnModel.modeTitle(.relative2sComplement) == "Relative (2's complement)")
        #expect(LearnModel.modeTitle(.relativeBinOffset) == "Relative (binary offset)")
    }

    /// Step 3: a refused learn raises "MIDI Learn Failed" and shows no banner.
    @Test func aRefusedLearnIsAnAlert() throws {
        let (l, _, c) = try model()
        c.learnError = .Model(message: "“file.open” cannot be learned")
        l.learnCommand("file.open")
        #expect(l.alert?.title == "MIDI Learn Failed" && l.banner == nil)
    }

    /// The plugin window's Learn button acts on the last parameter a
    /// gesture touched in it, and is disabled before any.
    @Test func thePluginWindowRemembersTheLastTouchedParameter() throws {
        let f = mixerFake()
        f.paramTable[10] = [
            ParamRow(
                id: 0, name: "Gain", module: "", min: 0, max: 1, default: 0.5, value: 0.5, stepped: false,
                automatable: true, hidden: false),
            ParamRow(
                id: 1, name: "Tone", module: "", min: 0, max: 1, default: 0.5, value: 0.5, stepped: false,
                automatable: true, hidden: false),
        ]
        let w = PluginWindowModel(document: try DocumentModel(session: f), slot: 10)
        #expect(w.lastTouched == nil)
        w.beginParam(1)
        w.setParam(1, 0.3)
        w.commitParam(1)
        #expect(w.lastTouched?.slot == 10 && w.lastTouched?.param == 1)
        w.setParam(0, 0.9)
        #expect(w.lastTouched?.param == 0, "a click is a gesture too")
    }

    /// Settings › Surfaces: a learned command's Remove reaches the core.
    @Test func aCommandMapIsRemoved() {
        let c = FakeSurfaceCore()
        let s = SurfaceSettings(core: c)
        s.removeCommandMap("edit.undo")
        #expect(c.removedCommands == ["edit.undo"])
        // Final review T11 m8: a refused removal is an alert.
        c.removeError = .Model(message: "no MIDI control is learned for “edit.redo”")
        s.removeCommandMap("edit.redo")
        #expect(s.alert?.title == "Remove MIDI Learn Failed")
    }

    /// Final review T11 m8: the plugin window follows only its own slot's
    /// touched parameters.
    @Test func followTouchedTakesOnlyItsOwnSlot() throws {
        let f = mixerFake()
        f.paramTable[10] = [
            ParamRow(
                id: 3, name: "Gain", module: "", min: 0, max: 1, default: 0.5, value: 0.5, stepped: false,
                automatable: true, hidden: false)
        ]
        let w = PluginWindowModel(document: try DocumentModel(session: f), slot: 10)
        w.followTouched([.pluginParam(slot: 11, param: 3), .stripFader(strip: 1)])
        #expect(w.lastTouched == nil, "another slot's parameter and a fader")
        w.followTouched([.pluginParam(slot: 11, param: 2), .pluginParam(slot: 10, param: 3)])
        #expect(w.lastTouched?.slot == 10 && w.lastTouched?.param == 3)
    }
}
