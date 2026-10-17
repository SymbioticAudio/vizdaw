import Testing
import VizDAWCore
@testable import VizDAWAppModel

private let move1: EditCommand = .event(edit: .move(events: [1], delta: 100, toLane: nil))
private let move2: EditCommand = .event(edit: .move(events: [1], delta: 200, toLane: nil))

@MainActor
@Suite struct GestureTests {
    func open() throws -> (FakeSession, DocumentModel, EditorModel) {
        let f = editorFake()
        let m = try DocumentModel(session: f)
        f.calls = []
        #expect(m.editor.beginGesture("Move Block", lanes: [chords]))
        return (f, m, m.editor)
    }

    @Test func aDragIsOneEditEndingWithTheLastTarget() throws {
        let (f, _, e) = try open()
        e.updateGesture(move1)
        e.updateGesture(move2)
        e.endGesture(move2)
        #expect(f.calls == ["begin Move Block", "gesture", "gesture", "end"])
        #expect(f.gestureEdits == [move1, move2])
        #expect(e.gesture == nil)
    }

    @Test func busyIntermediateEditIsDroppedSilently() throws {
        let (f, _, e) = try open()
        f.gestureErrors = [.Busy(message: "busy")]
        e.updateGesture(move1)
        #expect(e.status == nil)
        #expect(e.gesture != nil)
        e.updateGesture(move2)
        e.endGesture(move2)
        #expect(f.calls == ["begin Move Block", "gesture refused", "gesture", "end"])
    }

    @Test func busyFinalEditIsRetriedOnTick() throws {
        let (f, _, e) = try open()
        e.updateGesture(move1)
        f.gestureErrors = [.Busy(message: "busy")]
        e.endGesture(move2)
        #expect(e.gesture?.finishing == true)
        #expect(!f.calls.contains("end"))
        e.tick()
        #expect(f.gestureEdits == [move1, move2])
        #expect(f.calls.last == "end")
        #expect(e.gesture == nil)
    }

    @Test func thePluginTimerRetriesThePendingFinal() throws {
        let (f, m, e) = try open()
        f.gestureErrors = [.Busy(message: "busy")]
        e.endGesture(move1)
        m.idle()
        #expect(f.calls.last == "end")
        #expect(e.gesture == nil)
    }

    @Test func aBusyEndEditIsRetriedToo() throws {
        let (f, _, e) = try open()
        e.updateGesture(move1)
        f.mixerError = .Busy(message: "busy")
        e.endGesture(move1)
        #expect(e.gesture != nil)
        f.mixerError = nil
        e.tick()
        #expect(e.gesture == nil)
        #expect(f.gestureEdits == [move1], "the applied target is not sent again")
    }

    @Test func escapeWhileTheFinalEditWaitsAborts() throws {
        let (f, _, e) = try open()
        f.gestureErrors = [.Busy(message: "busy")]
        e.endGesture(move1)
        e.cancelGesture()
        #expect(f.calls.last == "abort")
        e.tick()
        #expect(f.calls.last == "abort")
        #expect(e.gesture == nil)
    }

    @Test func refusedFinalEditAbortsAndShowsStatus() throws {
        let (f, _, e) = try open()
        f.gestureErrors = [.Model(message: "would make a cycle")]
        e.endGesture(move1)
        #expect(f.calls == ["begin Move Block", "gesture refused", "abort"])
        #expect(e.status == "would make a cycle")
        #expect(e.gesture == nil)
    }

    @Test func aRefusedIntermediateTargetIsSilent() throws {
        let (f, _, e) = try open()
        f.gestureErrors = [.Model(message: "would make a cycle")]
        e.updateGesture(move1)
        #expect(e.status == nil)
        #expect(e.gesture?.applied == nil)
    }

    @Test func aDragWhoseLaneVanishesIsCancelled() throws {
        let (f, m, e) = try open()
        e.updateGesture(move1)
        var s = f.snap
        s.version += 1
        s.laneRevs.removeAll { $0.lane == 31 }
        f.idleUpdates = [IdleUpdate(snapshot: s, slotsChanged: false, paramsChanged: [], problems: [])]
        m.idle()
        #expect(f.calls.last == "abort")
        #expect(e.gesture == nil)
        #expect(e.status?.contains("cancelled") == true)
    }

    @Test func endingWithoutAMoveClosesAnEmptyEdit() throws {
        let (f, _, e) = try open()
        e.endGesture(nil)
        #expect(f.calls == ["begin Move Block", "end"])
    }

    @Test func returningToTheStartRewinds() throws {
        let (f, _, e) = try open()
        e.updateGesture(move1)
        e.updateGesture(nil)
        #expect(f.calls == ["begin Move Block", "gesture", "abort", "begin Move Block"])
        e.endGesture(nil)
        #expect(f.calls.last == "end")
    }

    @Test func onlyOneGestureAtATime() throws {
        let (_, _, e) = try open()
        #expect(!e.beginGesture("Copy Block", lanes: []))
    }

    @Test func aBusyIntermediateEditForgetsTheAppliedTarget() throws {
        let (f, _, e) = try open()
        e.updateGesture(move1)
        f.gestureErrors = [.Busy(message: "busy")]
        e.updateGesture(move2)
        e.endGesture(move1)
        #expect(f.calls == ["begin Move Block", "gesture", "gesture refused", "gesture", "end"])
        #expect(f.gestureEdits == [move1, move1])
        #expect(e.gesture == nil)
    }

    @Test func aBusyIntermediateEditThenBackToTheStartRewinds() throws {
        let (f, _, e) = try open()
        e.updateGesture(move1)
        f.gestureErrors = [.Busy(message: "busy")]
        e.updateGesture(move2)
        e.updateGesture(nil)
        #expect(f.calls.suffix(2) == ["abort", "begin Move Block"])
    }

    @Test func aRefusedTargetThenMouseUpWithoutOneAborts() throws {
        let (f, _, e) = try open()
        e.updateGesture(move1)
        f.gestureErrors = [.Model(message: "refused")]
        e.updateGesture(move2)
        e.endGesture(nil)
        #expect(f.calls.suffix(1) == ["abort"])
        #expect(!f.calls.contains("end"))
    }

    @Test func aGestureCancelledByASnapshotStaysCancelled() throws {
        let (f, m, e) = try open()
        var s = f.snap
        s.version += 1
        s.laneRevs.removeAll { $0.lane == 31 }
        f.bump(s)
        e.updateGesture(move1)
        withExtendedLifetime(m) {}
        #expect(e.gesture == nil)
    }
}
