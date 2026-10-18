import Foundation
import Testing
import VizDAWCore
@testable import VizDAWAppModel

/// Step entry in the app (plan 4f Task 8): turning it on, its keys, MIDI
/// notes, keyboard slots and their clashes.
@MainActor
@Suite struct StepEntryTests {
    let host = FakeControlHost()

    func demo() throws -> (DocumentStore, UUID, DocumentModel, FakeSession) {
        let f = demoFake()
        let s = DocumentStore(makeEmpty: { FakeSession([]) }, makeDemo: { f }, open: { _ in demoFake() })
        let id = try #require(s.newDemo())
        return (s, id, try #require(s.model(id)), f)
    }

    func run(_ id: String, _ args: [ControlArg], _ m: DocumentModel, _ s: DocumentStore, _ doc: UUID)
        -> ControlFailure?
    {
        if case .failed(let f) = ControlCommands.run(id, args: args, doc: (doc, m), store: s, host: host) { return f }
        return nil
    }

    func run(_ id: String, _ m: DocumentModel, _ s: DocumentStore, _ doc: UUID) -> ControlFailure? {
        run(id, [], m, s, doc)
    }

    @Test func stepEntryStartsOnTheSelectedLaneAtThePosition() throws {
        let (s, id, m, f) = try demo()
        f.state.tick = 61_440
        #expect(run("step.entry", m, s, id) == nil)
        #expect(m.stepEntry.isOn && m.stepEntry.target == LaneRef(seq: 3, lane: 30), "the first lane")
        #expect(m.stepEntry.state?.tick == 61_440 && m.stepEntry.state?.value == .quarter)
        #expect(m.stepEntry.summary == "Step Entry · Groove · 1/4")
        _ = run("step.entry", m, s, id)
        #expect(!m.stepEntry.isOn, "toggled off")
        m.editor.setSelection(SelectionRow(timeRange: nil, lanes: [LaneRef(seq: 3, lane: 31)], events: []))
        _ = run("step.entry", [ControlArg(name: "on", value: .flag(value: true))], m, s, id)
        #expect(m.stepEntry.target == LaneRef(seq: 3, lane: 31), "the selected lane")
        m.stepEntry.stop()
        m.playStop()
        #expect(run("step.entry", m, s, id)?.message == StepEntryModel.playingMessage)
    }

    @Test func stepKeysSendInputsAndStepsMoveTheTransport() throws {
        let (s, id, m, f) = try demo()
        #expect(run("step.rest", m, s, id) == .of(.unavailable, StepEntryModel.offMessage))
        _ = run("step.entry", m, s, id)
        _ = run("step.eighth", m, s, id)
        #expect(f.stepCalls.last?.input == .value(value: .eighth))
        #expect(f.state.tick == 0, "a value is no step")
        _ = run("step.rest", m, s, id)
        #expect(f.stepCalls.last?.input == .rest && f.state.tick == 15_360, "the transport follows")
        let version = m.snapshot.version
        _ = run("step.notes", [ControlArg(name: "pitches", value: .text(value: "C4 E4"))], m, s, id)
        let typed = StepEntryModel.typedVelocity
        #expect(
            f.stepCalls.last?.input
                == .notes(notes: [StepNoteRow(pitch: 60, velocity: typed), StepNoteRow(pitch: 64, velocity: typed)]))
        #expect(m.snapshot.version > version, "the new document is applied")
        #expect(f.stepCalls.last?.state.tick == 15_360, "each input starts where the transport is")
        f.state.tick = 0
        _ = run("step.tie", m, s, id)
        #expect(f.stepCalls.last?.state.tick == 0, "moving the transport moves the next step")
        let e = run("step.notes", [ControlArg(name: "pitches", value: .text(value: "H9"))], m, s, id)
        #expect(e?.code == .failed && e?.message.contains("not a pitch") == true)
        for (cmd, input) in [
            ("step.dot", StepInputRow.dot), ("step.triplet", .triplet), ("step.delete_last", .deleteLast),
            ("step.whole", .value(value: .whole)), ("step.thirty_second", .value(value: .thirtySecond)),
        ] {
            _ = run(cmd, m, s, id)
            #expect(f.stepCalls.last?.input == input, "\(cmd)")
        }
    }

    @Test func aRefusedStepShowsInTheStatusLine() throws {
        let (s, id, m, f) = try demo()
        _ = run("step.entry", m, s, id)
        f.stepEntryError = .Model(message: "a reference cannot contain itself")
        #expect(run("step.rest", m, s, id) == .of(.failed, "a reference cannot contain itself"))
        #expect(m.editor.status == "a reference cannot contain itself")
        #expect(m.stepEntry.state?.tick == 0, "the state stays")
    }

    @Test func playedNotesGoToStepEntryFirst() throws {
        let (s, id, m, f) = try demo()
        #expect(!m.listensForNotes)
        _ = run("step.entry", m, s, id)
        #expect(m.listensForNotes)
        let input = FakeNoteInput()
        let hub = NoteInputHub(input: input)
        hub.poll([m], focused: m)
        #expect(input.starts == 1)
        input.queued = [NoteInRow(pitch: 60, velocity: 0x8000, channel: 0), NoteInRow(pitch: 60, velocity: 0, channel: 0, on: false)]
        hub.poll([m], focused: m)
        #expect(f.stepCalls.suffix(2).map(\.input) == [.noteOn(note: StepNoteRow(pitch: 60, velocity: 0x8000)), .noteOff(pitch: 60)])
        _ = run("step.entry", m, s, id)
        hub.poll([m], focused: m)
        #expect(input.stops == 1, "nobody listens any more")
    }

    @Test func keyboardSlotsPlayTheirSequenceOrEnterAReference() throws {
        let (s, id, m, f) = try demo()
        #expect(m.pressSlot(1) == .done)
        #expect(m.selectedSeq == 1 && f.playsFrom.last?.seq == 1 && f.playsFrom.last?.from == 0)
        m.playStop()
        m.choose(3)
        _ = run("step.entry", m, s, id)
        #expect(m.pressSlot(1) == .done)
        #expect(f.stepCalls.last?.input == .reference(seq: 1) && m.selectedSeq == 3, "entered, not played")
    }

    @Test func aSlotThatIsACommandsKeyIsRefused() throws {
        let (_, _, m, f) = try demo()
        m.keys = KeyBindings(defaults: MemoryDefaults())
        m.beginSlot(3)
        m.slotDraft?.key = "s"
        m.slotDraft?.command = true
        m.commitSlot()
        #expect(m.slotDraft?.problem == "⌘S is the key of “Save”.")
        #expect(f.edits.isEmpty, "nothing set")
        m.slotDraft?.command = false
        m.slotDraft?.control = true
        m.commitSlot()
        #expect(m.slotDraft == nil)
        m.beginSlot(3)
        m.slotDraft?.key = "="
        m.commitSlot()
        #expect(m.slotDraft?.problem == "= is the key of “Tie”.", "step entry's keys are refused too")
        m.slotDraft = nil
        #expect(f.edits.last == .sequence(edit: .setKeyboardSlot(seq: 3, slot: KeySlotRow(key: "s", modifiers: 2))))
    }

    @Test func checkMarksFollowTheState() throws {
        let (s, id, m, _) = try demo()
        #expect(!StepCommands.isOn("step.entry", m) && !StepCommands.enabled("step.rest", m))
        #expect(StepCommands.enabled("step.entry", m))
        _ = run("step.entry", m, s, id)
        #expect(StepCommands.isOn("step.quarter", m) && !StepCommands.isOn("step.eighth", m))
        #expect(StepCommands.enabled("step.rest", m) && !StepCommands.isOn("step.dot", m))
    }

    @Test func aRefusedNoteOffDoesNotStickTheChord() throws {
        let (s, id, m, f) = try demo()
        _ = run("step.entry", m, s, id)
        f.stepAnswer = { state, input in
            var r = state
            if case .noteOn(let n) = input { r.held.append(n.pitch) }
            if case .noteOff(let p) = input { r.held.removeAll { $0 == p } }
            return StepResultRow(state: r, snapshot: nil, moved: false)
        }
        m.stepEntry.receive([NoteInRow(pitch: 60, velocity: 0x8000, channel: 0)])
        m.playStop()
        #expect(m.isPlaying)
        m.stepEntry.receive([NoteInRow(pitch: 60, velocity: 0, channel: 0, on: false)])
        m.playStop()
        let before = f.stepCalls.count
        m.stepEntry.receive([
            NoteInRow(pitch: 64, velocity: 0x8000, channel: 0), NoteInRow(pitch: 64, velocity: 0, channel: 0, on: false),
        ])
        #expect(f.stepCalls.count == before + 2)
        let held = f.stepCalls[before].state
        #expect(held.held.isEmpty && held.chord.isEmpty, "the stale key was let go")
    }

    @Test func stepEntryEndsWhenAnotherSequenceIsShown() throws {
        let (s, id, m, f) = try demo()
        _ = run("step.entry", m, s, id)
        m.choose(1)
        let before = f.stepCalls.count
        #expect(run("step.rest", m, s, id) != nil)
        #expect(f.stepCalls.count == before && !m.stepEntry.isOn)
        #expect(m.editor.status == StepEntryModel.sequenceChanged)
    }

    @Test func stepEntryEndsWhenItsLaneIsGone() throws {
        let (s, id, m, f) = try demo()
        _ = run("step.entry", m, s, id)
        f.laneTable[3] = f.laneTable[3]?.filter { $0.id != 30 }
        var snap = try f.snapshot()
        snap.version += 1
        m.apply(snap)
        let before = f.stepCalls.count
        #expect(run("step.rest", m, s, id) != nil)
        #expect(f.stepCalls.count == before && !m.stepEntry.isOn)
        #expect(m.editor.status == StepEntryModel.laneGone)
    }

    @Test func stepKeysWinOnlyDuringStepEntry() {
        let keys = KeyBindings(defaults: MemoryDefaults())
        func route(_ code: UInt16, step: Bool) -> KeyRoute {
            KeyRouter.route(
                keyCode: code, modifiers: noMods, typing: false, stepEntry: step, menus: true, keys: keys, sequences: [])
        }
        #expect(route(18, step: false) == .command("locator.go_1"))
        #expect(route(18, step: true) == .command("step.whole"))
        #expect(route(83, step: true) == .command("step.whole"), "the keypad")
        keys.reserveNumberRow = true
        #expect(route(18, step: true) == .command("locator.go_1"))
        #expect(route(83, step: true) == .command("step.whole"))
    }
}
