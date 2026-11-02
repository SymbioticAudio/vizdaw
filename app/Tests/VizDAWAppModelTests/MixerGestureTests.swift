import Foundation
import Testing
import VizDAWCore
@testable import VizDAWAppModel

/// The mixer's moves are gestures (plan 5e-2 Task 2; spec §7.1, §7.5):
/// drags are touch, values and release; clicks are all three at once; a
/// touched control shows the hand's value and has a ring; modes per strip
/// and for all; the strip selection.
@MainActor
@Suite struct MixerGestureTests {
    func model(_ f: FakeSession = mixerFake()) throws -> (MixerModel, FakeSession) {
        (try MixerModel(document: DocumentModel(session: f)), f)
    }

    let fader = AutomationTargetRow.stripFader(strip: 1)

    @Test func aFaderDragIsAGestureOnTheCore() throws {
        let (m, f) = try model()
        m.touch(fader)
        m.move(fader, to: 0.5)
        m.move(fader, to: 0.4)
        m.release(fader)
        #expect(
            f.gestures.map(\.1) == [.touch(value: nil), .value(value: 0.5), .value(value: 0.4), .release],
            "mouse-down sends no value: the core starts from the value heard now (5e-1 Ruling 26)")
        #expect(f.gestures.allSatisfy { $0.0 == fader })
        #expect(f.calls.isEmpty, "no direct setGain any more")
    }

    @Test func clicksAreTouchValueAndRelease() throws {
        let (m, f) = try model()
        let strip = try #require(m.strips.first)
        m.toggleMute(strip)
        #expect(f.gestures.map(\.1) == [.touch(value: 1), .value(value: 1), .release])
        #expect(f.gestures.allSatisfy { $0.0 == .stripMute(strip: 1) })
        f.gestures = []
        m.toggleSolo(strip)
        #expect(f.gestures.first?.0 == .stripSolo(strip: 1))
    }

    @Test func aTouchedControlShowsItsLiveValue() throws {
        let (m, _) = try model()
        let strip = try #require(m.strips.first)
        #expect(m.gainDb(strip) == 0)
        m.touch(fader)
        #expect(m.isTouched(fader) && m.gainDb(strip) == 0, "touched, still showing what it showed")
        m.move(fader, to: AutomationLaw.faderNorm(-6))
        #expect(abs(m.gainDb(strip) + 6) < 1e-9, "the hand's value")
        #expect(m.isTouched(fader))
        m.release(fader)
        #expect(m.gainDb(strip) == 0, "the snapshot's again")
        #expect(!m.isTouched(fader))
        let pan = AutomationTargetRow.stripPan(strip: 1)
        m.touch(pan)
        m.move(pan, to: 0.25)
        #expect(m.pan(strip) == -0.5)
    }

    @Test func ringsFollowTheCoresTouchedTargets() throws {
        let (m, f) = try model()
        f.automationRow.touched = [.stripPan(strip: 2)]
        m.pollAutomation()
        #expect(m.isTouched(.stripPan(strip: 2)), "a channel or plugin touch has a ring")
        f.automationRow.touched = []
        m.pollAutomation()
        #expect(!m.isTouched(.stripPan(strip: 2)))
    }

    @Test func pollingKeepsTheLastTouchedSetWhenTheCoreIsBusy() throws {
        let (m, f) = try model()
        f.automationRow.touched = [.stripPan(strip: 2)]
        m.pollAutomation()
        f.automationError = .Busy(message: "Finish the open edit first.")
        f.automationRow.touched = []
        m.pollAutomation()
        #expect(m.isTouched(.stripPan(strip: 2)), "Busy keeps the last set")
        f.automationError = nil
        m.pollAutomation()
        #expect(!m.isTouched(.stripPan(strip: 2)))
    }

    /// Two controls touched at once (plan 5e-1 Ruling 14: one shared
    /// static edit in the core, so the real core takes both).
    @Test func twoTargetsTouchedAtOnceAreIndependent() throws {
        let (m, f) = try model()
        let other = AutomationTargetRow.stripFader(strip: 2)
        m.touch(fader)
        m.touch(other)
        m.release(fader)
        #expect(m.isTouched(other) && !m.isTouched(fader))
        #expect(f.gestures.filter { $0.1 == .release }.map(\.0) == [fader])
    }

    @Test func touchesAreReleasedOnceWhenTheWindowClosesOrTheStripGoes() throws {
        let (m, f) = try model()
        m.touch(fader)
        m.endOpenDrag()  // the window closes
        m.endOpenDrag()
        m.release(fader)
        #expect(f.gestures.filter { $0.1 == .release }.count == 1)
        f.gestures = []
        let other = AutomationTargetRow.stripFader(strip: 2)
        m.touch(other)
        f.mixerSnap.strips.removeAll { $0.id == 2 }
        m.load([])  // a refetch sees strip 2 gone
        #expect(f.gestures.map(\.1) == [.touch(value: nil), .release])
        m.release(other)
        #expect(f.gestures.filter { $0.1 == .release }.count == 1)
    }

    @Test func aRefusedTouchSendsNoRelease() throws {
        let (m, f) = try model()
        f.gestureError = .Busy(message: "Finish the open edit first.")
        m.touch(fader)
        #expect(m.document.alert?.message == "Finish the open edit first.")
        f.gestureError = nil
        m.move(fader, to: 0.3)
        m.release(fader)
        #expect(f.gestures.isEmpty, "nothing after a refused touch")
    }

    @Test func modesAreSetPerStripAndForAll() throws {
        let (m, f) = try model()
        m.setMode(2, .touch)
        #expect(f.modeCalls.last?.0 == [2] && f.modeCalls.last?.1 == .touch)
        m.setAllModes(.latch)
        #expect(f.modeCalls.last?.0 == [1, 2, 0] && f.modeCalls.last?.1 == .latch)
        #expect(Set(AutomationModes.all.map(AutomationModes.colorName)).count == 6, "one colour each")
        #expect(AutomationModes.all.map(AutomationModes.title) == ["Off", "Read", "Touch", "Latch", "Write", "Trim"])
    }

    @Test func theSelectionForgetsStripsThatGo() throws {
        let (m, f) = try model()
        m.select(1, adding: false)
        m.select(2, adding: true)
        #expect(m.document.selectedStrips == [1, 2])
        m.select(2, adding: true)
        #expect(m.document.selectedStrips == [1], "⌘-click removes")
        m.select(0, adding: false)
        #expect(m.document.selectedStrips == [0], "master can be selected")
        m.select(2, adding: true)
        f.mixerSnap.strips.removeAll { $0.id == 2 }
        m.load([])
        #expect(m.document.selectedStrips == [0])
    }
}
