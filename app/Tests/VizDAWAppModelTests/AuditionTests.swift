import Testing
import VizDAWCore
@testable import VizDAWAppModel

private let c4 = AuditionNote(dest: 5, pitch: 60)
private let d4 = AuditionNote(dest: 5, pitch: 62)

@MainActor
@Suite struct AuditionTests {
    func make() throws -> (FakeSession, EditorModel) {
        let f = editorFake()
        let m = try DocumentModel(session: f)
        return (f, m.editor)
    }

    func log(_ f: FakeSession) -> [String] {
        f.auditions.map { "\($0.on ? "on" : "off") \($0.pitch)" }
    }

    @Test func aNoteSoundsAndEnds() throws {
        let (f, e) = try make()
        e.auditionOn(c4, velocity: 0x8000)
        e.auditionOff(c4)
        e.auditionOff(c4)
        #expect(log(f) == ["on 60", "off 60"], "a second off for the same note sends nothing")
        #expect(e.status == nil)
    }

    @Test func aBusyNoteOffIsRetriedOnTick() throws {
        let (f, e) = try make()
        e.auditionOn(c4, velocity: 0x8000)
        f.auditionErrors = [.Busy(message: "busy"), .Busy(message: "busy")]
        e.auditionOff(c4)
        #expect(e.hasPendingNoteOffs)
        e.tick()
        #expect(e.hasPendingNoteOffs, "still busy")
        e.tick()
        #expect(!e.hasPendingNoteOffs)
        #expect(log(f) == ["on 60", "off 60"])
        #expect(e.status == nil, "Busy is silent")
    }

    @Test func thePluginTimerRetriesNoteOffs() throws {
        let f = editorFake()
        let m = try DocumentModel(session: f)
        m.editor.auditionOn(c4, velocity: 0x8000)
        f.auditionErrors = [.Busy(message: "busy")]
        m.editor.auditionOff(c4)
        m.idle()
        #expect(log(f) == ["on 60", "off 60"])
    }

    @Test func aNoteOffThatKeepsFailingIsDroppedAfterTheRetries() throws {
        let (f, e) = try make()
        e.auditionOn(c4, velocity: 0x8000)
        f.auditionErrors = Array(repeating: .Busy(message: "busy"), count: EditorModel.noteOffRetries + 5)
        e.auditionOff(c4)
        for _ in 0..<EditorModel.noteOffRetries { e.tick() }
        #expect(!e.hasPendingNoteOffs)
        #expect(log(f) == ["on 60"])
        #expect(e.status == EditorModel.stuckNoteHint)
    }

    @Test func aNonBusyNoteOffErrorIsReportedAtOnce() throws {
        let (f, e) = try make()
        e.auditionOn(c4, velocity: 0x8000)
        f.auditionErrors = [.Engine(message: "queue full")]
        e.auditionOff(c4)
        #expect(!e.hasPendingNoteOffs, "not retried")
        #expect(e.status != nil)
        #expect(log(f) == ["on 60"])
    }

    @Test func offAllMeetingBusyQueuesRetries() throws {
        let (f, e) = try make()
        e.auditionOn(c4, velocity: 0x8000)
        e.auditionOn(d4, velocity: 0x8000)
        f.auditionErrors = [.Busy(message: "busy"), .Busy(message: "busy")]
        e.auditionOffAll()
        #expect(e.hasPendingNoteOffs)
        e.tick()
        #expect(!e.hasPendingNoteOffs)
        #expect(Set(log(f).suffix(2)) == ["off 60", "off 62"])
    }

    @Test func aNoteOnWaitsForItsPendingNoteOff() throws {
        let (f, e) = try make()
        e.auditionOn(c4, velocity: 0x8000)
        f.auditionErrors = [.Busy(message: "busy"), .Busy(message: "busy")]
        e.auditionOff(c4)
        // The retry inside auditionOn meets Busy again: the new note-on for
        // the same key is not sent (it would be ended by the late note-off).
        e.auditionOn(c4, velocity: 0x8000)
        #expect(log(f) == ["on 60"])
        e.auditionOn(c4, velocity: 0x8000)
        #expect(log(f) == ["on 60", "off 60", "on 60"])
    }

    @Test func busyNoteOnIsDroppedSilently() throws {
        let (f, e) = try make()
        f.auditionErrors = [.Busy(message: "busy")]
        e.auditionOn(c4, velocity: 0x8000)
        e.auditionOff(c4)
        #expect(f.auditions.isEmpty, "never started, so never ended")
        #expect(e.status == nil)
    }

    @Test func noEngineShowsOneHint() throws {
        let (f, e) = try make()
        f.auditionSent = false
        e.auditionOn(c4, velocity: 0x8000)
        #expect(e.status == EditorModel.auditionHint)
        e.clearStatus()
        e.auditionOn(d4, velocity: 0x8000)
        #expect(e.status == nil, "once per document")
        e.auditionOff(d4)
        #expect(log(f) == ["on 60", "on 62"], "nothing held, nothing to end")
    }

    @Test func offAllEndsEverySoundingNote() throws {
        let (f, e) = try make()
        e.auditionOn(c4, velocity: 0x8000)
        e.auditionOn(d4, velocity: 0x8000)
        e.auditionOffAll()
        #expect(Set(log(f).suffix(2)) == ["off 60", "off 62"])
    }
}
