import Foundation
import Testing
import VizDAWCore
@testable import VizDAWAppModel

private let fourFour = [MeterEntryRow(tick: 0, num: 4, den: 4, sharps: 0, minor: false)]

@MainActor
@Suite struct NudgeTests {
    @Test func timeSteps() {
        var snap = Snap()
        snap.division = .eighth
        snap.enabled = false
        var s = NudgeSettings()
        #expect(s.ticks(at: 0, meter: fourFour, snap: snap) == 7_680, "the grid's step even with snap off")
        s.time = .division(.quarter, triplet: true)
        #expect(s.ticks(at: 0, meter: fourFour, snap: snap) == 10_240)
        s.time = .division(.bar, triplet: false)
        #expect(s.ticks(at: 0, meter: [MeterEntryRow(tick: 0, num: 3, den: 4, sharps: 0, minor: false)], snap: snap) == 46_080)
        s.time = .ticks(0)
        #expect(s.ticks(at: 0, meter: fourFour, snap: snap) == 1)
        s.time = .ticks(240)
        #expect(s.ticks(at: 0, meter: fourFour, snap: snap) == 240)
    }

    @Test func keysFollowTheSignatureOrAreFixed() {
        var s = NudgeSettings()
        let gMajor = [MeterEntryRow(tick: 0, num: 4, den: 4, sharps: 1, minor: false)]
        #expect(s.key(at: 0, meter: gMajor) == (7, Scale.major.mask))
        let aMinor = [MeterEntryRow(tick: 0, num: 4, den: 4, sharps: 0, minor: true)]
        #expect(s.key(at: 0, meter: aMinor) == (9, Scale.naturalMinor.mask))
        s.followKeySignature = false
        s.root = 14
        s.scale = .dorian
        #expect(s.key(at: 0, meter: gMajor) == (2, Scale.dorian.mask))
    }

    func selected(_ events: [UInt64]) throws -> (FakeSession, EditorModel) {
        let f = rollFake()
        let m = try DocumentModel(session: f)
        m.editor.setSelection(SelectionRow(timeRange: nil, lanes: [LaneRef(seq: 3, lane: 31)], events: events))
        return (f, m.editor)
    }

    @Test func nudgeSendsTimeAndPitchCommands() throws {
        let (f, e) = try selected([1, 2])
        var s = NudgeSettings()
        #expect(e.nudge(.later, settings: s))
        #expect(f.edits.last == .event(edit: .nudge(events: [1, 2], ticks: 15_360, semitones: 0)))
        e.nudge(.earlier, settings: s)
        #expect(f.edits.last == .event(edit: .nudge(events: [1, 2], ticks: -15_360, semitones: 0)))
        s.pitch = .semitones(3)
        e.nudge(.up, settings: s)
        #expect(f.edits.last == .event(edit: .nudge(events: [1, 2], ticks: 0, semitones: 3)))
        s.pitch = .scaleSteps(2)
        e.nudge(.down, settings: s)
        #expect(f.edits.last == .event(edit: .transposeInScale(events: [1, 2], steps: -2, root: 0, mask: Scale.major.mask)))
    }

    @Test func scaleStepsReadTheKeyAtTheAnchor() throws {
        let f = rollFake()
        var info = fourFour(3)
        info.meter = [
            MeterEntryRow(tick: 0, num: 4, den: 4, sharps: 1, minor: false),
            MeterEntryRow(tick: 122_880, num: 4, den: 4, sharps: 0, minor: true),
        ]
        f.infoTable[3] = info
        f.laneEventTable["3/31"] = LaneEvents(
            seq: 3, lane: 31, rev: 2,
            events: [noteRow(1, tick: 0), noteRow(2, tick: 122_880 + 15_360)])
        let m = try DocumentModel(session: f)
        let e = m.editor
        var s = NudgeSettings()
        s.pitch = .scaleSteps(1)
        let lanes = [LaneRef(seq: 3, lane: 31)]

        e.setSelection(SelectionRow(timeRange: nil, lanes: lanes, events: [2]))
        #expect(e.nudge(.up, settings: s))
        #expect(f.edits.last == .event(edit: .transposeInScale(events: [2], steps: 1, root: 9, mask: Scale.naturalMinor.mask)))

        e.setSelection(SelectionRow(timeRange: TickRange(start: 122_880, end: 184_320), lanes: lanes, events: [1]))
        #expect(e.nudge(.up, settings: s))
        #expect(f.edits.last == .event(edit: .transposeInScale(events: [1], steps: 1, root: 9, mask: Scale.naturalMinor.mask)))

        e.setSelection(SelectionRow(timeRange: nil, lanes: lanes, events: [1]))
        #expect(e.nudge(.up, settings: s))
        #expect(f.edits.last == .event(edit: .transposeInScale(events: [1], steps: 1, root: 7, mask: Scale.major.mask)))
    }

    @Test func nothingSelectedNothingSent() throws {
        let (f, e) = try selected([])
        #expect(!e.nudge(.up, settings: NudgeSettings()))
        #expect(f.edits.isEmpty)
    }

    @Test func refusedWhileADragIsOpen() throws {
        let (f, e) = try selected([1])
        #expect(e.beginGesture("Move Notes", lanes: [LaneKey(3, 31)], origin: .other))
        #expect(!e.nudge(.up, settings: NudgeSettings()))
        #expect(f.edits.isEmpty)
        #expect(e.status == EditorModel.gestureOpenMessage)
    }

    @Test func theAnchorIsTheRangeStartElseTheEarliestSelectedEvent() throws {
        let (_, e) = try selected([2])
        #expect(e.nudgeAnchor()?.tick == 15_360)
        #expect(e.nudgeAnchor()?.seq == 3)
        e.setSelection(
            SelectionRow(timeRange: TickRange(start: 61_440, end: 122_880), lanes: [LaneRef(seq: 3, lane: 31)], events: [2]))
        #expect(e.nudgeAnchor()?.tick == 61_440)
    }

    @Test func preferencesPersistAndDecodeTolerantly() throws {
        let name = "VizDAWTests.nudge.\(UUID().uuidString)"
        let d = try #require(UserDefaults(suiteName: name))
        defer { d.removePersistentDomain(forName: name) }
        let p = EditingPreferences(defaults: d)
        #expect(p.nudge == NudgeSettings())
        p.nudge.pitch = .scaleSteps(2)
        p.nudge.time = .ticks(120)
        let q = EditingPreferences(defaults: d)
        #expect(q.nudge.pitch == .scaleSteps(2))
        #expect(q.nudge.time == .ticks(120))
        d.set(Data(#"{"scale":"nope","followKeySignature":false}"#.utf8), forKey: EditingPreferences.key)
        let r = EditingPreferences(defaults: d)
        #expect(r.nudge.scale == .major)
        #expect(!r.nudge.followKeySignature)
        d.set(Data("garbage".utf8), forKey: EditingPreferences.key)
        #expect(EditingPreferences(defaults: d).nudge == NudgeSettings())
    }
}

@Suite @MainActor struct MemoryDefaultsTests {
    @Test func nudgeSettingsRoundTripWithoutTouchingStandard() {
        let before = UserDefaults.standard.data(forKey: EditingPreferences.key)
        let mem = MemoryDefaults()
        let a = EditingPreferences(defaults: mem)
        var n = NudgeSettings()
        n.pitch = .semitones(7)
        a.nudge = n
        #expect(EditingPreferences(defaults: mem).nudge.pitch == .semitones(7))
        #expect(UserDefaults.standard.data(forKey: EditingPreferences.key) == before)
        let l = ListPreferences(defaults: mem)
        l.columns = ListColumns()
        #expect(mem.data(forKey: ListPreferences.key) != nil)
        mem.removeObject(forKey: ListPreferences.key)
        #expect(mem.data(forKey: ListPreferences.key) == nil)
    }

    @Test func appStoreIsMemoryOnlyWithASupportDir() {
        #expect(MemoryDefaults.appStore(environment: ["VIZDAW_SUPPORT_DIR": "/x"]) is MemoryDefaults)
        #expect(!(MemoryDefaults.appStore(environment: [:]) is MemoryDefaults))
    }
}
