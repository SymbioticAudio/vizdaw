import Foundation
import Testing
import VizDAWCore
@testable import VizDAWAppModel

/// A clock the relay's jog window runs on.
@MainActor final class RelayClock {
    var now = ContinuousClock.now
}

/// Settings › Surfaces and the event relay (plan 5f-1 Task 10).
@MainActor
@Suite struct SurfaceSettingsTests {
    @Test func addListAndRemove() {
        let c = FakeSurfaceCore()
        let s = SurfaceSettings(core: c)
        s.add(.mcu, input: "In A", output: "Out A", position: nil)
        #expect(s.rows.map(\.kind) == [.mcu])
        #expect(SurfaceSettings.title(.mcuXt) == "MCU XT-compatible")
        #expect(SurfaceSettings.status(s.rows[0]) == "Connected · not answering")
        s.remove(s.rows[0].id)
        #expect(s.rows.isEmpty)
    }

    @Test func aRefusedAddIsAnAlert() {
        let c = FakeSurfaceCore()
        c.addError = .Model(message: "VizDAW drives one MCU-compatible main unit and up to three extenders")
        let s = SurfaceSettings(core: c)
        s.add(.mcu, input: "In A", output: "Out A", position: nil)
        #expect(s.alert?.title == "Add Surface Failed")
    }

    @Test func eventsSelectJogAndRunCommands() throws {
        let f = mixerFake()
        let m = try DocumentModel(session: f)
        let clock = RelayClock()
        let relay = SurfaceRelay(now: { clock.now })
        var performed: [String] = []
        var quiet: [String] = []
        let start = m.transport.tick
        relay.apply(
            [.command(id: "transport.stop", programmatic: false), .select(strip: 2), .jog(detents: 4)], m,
            perform: { performed.append($0) }, quietly: { quiet.append($0) })
        #expect(performed == ["transport.stop"] && quiet.isEmpty)
        #expect(m.selectedStrips == [2])
        clock.now += SurfaceRelay.jogWindow
        relay.flush(m)
        #expect(f.locates == [start + Int64(4) * Int64(MeterMath.ppq) / 4])
    }

    /// Pre-flight M3: a spun wheel is one locate (each locate lands a
    /// writing pass).
    @Test func jogsWithin150MsAreOneLocate() throws {
        let f = mixerFake()
        let m = try DocumentModel(session: f)
        let clock = RelayClock()
        let relay = SurfaceRelay(now: { clock.now })
        let start = m.transport.tick
        relay.apply([.jog(detents: 2)], m, perform: { _ in }, quietly: { _ in })
        clock.now += .milliseconds(100)
        relay.apply([.jog(detents: 3)], m, perform: { _ in }, quietly: { _ in })
        relay.flush(m)
        #expect(f.locates.isEmpty, "still inside the window")
        clock.now += .milliseconds(60)
        relay.flush(m)
        #expect(f.locates == [start + Int64(5) * Int64(MeterMath.ppq) / 4], "one locate for the spin")
    }

    /// Pre-flight M21: arm needs no mixer window.
    @Test func armTogglesTheStripsInputLane() throws {
        let f = demoFake()
        var vox = lane(40, "Vox")
        vox.audioOut = 7
        f.laneTable[3] = [vox]
        let s = DocumentStore(makeEmpty: { FakeSession([]) }, makeDemo: { f }, open: { _ in demoFake() })
        let m = try #require(s.newDemo().flatMap(s.model))
        m.select(3)
        #expect(m.inputLane(path: 7)?.id == 40)
        let n = f.edits.count
        SurfaceRelay().apply([.arm(strip: 9, path: 7), .arm(strip: 10, path: nil)], m, perform: { _ in }, quietly: { _ in })
        #expect(f.edits.count == n + 1, "a strip that is no AudioPath does nothing")
        #expect(f.edits.last == .lane(edit: .setFlags(seq: 3, lane: 40, mute: nil, solo: nil, arm: true)))
    }

    /// Final review R3 (Task 10 M6): a surface event that arrives while
    /// an earlier one runs (a Save panel's modal run loop still ticks) waits
    /// until the earlier one returns, and a command that can open a dialog
    /// runs without the surface's keyboard bypass.
    @Test func eventsDuringAnEventWaitAndDialogCommandsDoNotBypass() throws {
        let m = try DocumentModel(session: mixerFake())
        let relay = SurfaceRelay()
        var log: [String] = []
        var actingInSave: Bool?
        func perform(_ id: String) {
            log.append("begin \(id)")
            if id == "file.save" {
                actingInSave = m.surfaceActing
                relay.apply([.command(id: "transport.stop", programmatic: false)], m, perform: perform, quietly: { _ in })
            }
            log.append("end \(id)")
        }
        relay.apply([.command(id: "file.save", programmatic: false)], m, perform: perform, quietly: { _ in })
        #expect(log == ["begin file.save", "end file.save", "begin transport.stop", "end transport.stop"])
        #expect(actingInSave == false, "a dialog command has no bypass")
        #expect(!m.surfaceActing)
    }

    /// Ruling T9-1: a surface button is not a keystroke, so it acts while
    /// a text field has the keyboard; the keys still wait.
    @Test func aSurfaceButtonActsWhileATextFieldHasTheKeyboard() throws {
        let m = try DocumentModel(session: mixerFake())
        m.editor.panelKey = true
        #expect(!m.transportKeysEnabled)
        #expect(TransportCommands.run("transport.play", m) == .unavailable(TransportCommands.textFieldHasKeyboard))
        SurfaceRelay().apply(
            [.command(id: "transport.play", programmatic: false)], m,
            perform: { TransportCommands.run($0, m) }, quietly: { _ in })
        #expect(m.isPlaying)
        #expect(!m.transportKeysEnabled, "the keys still wait for the field")
    }

    /// Fix round 1 I1: a jog then Play in one batch locates first, so
    /// Play starts from the jogged place and no locate lands mid-pass.
    @Test func aJogIsLocatedBeforeALaterCommandInTheBatch() throws {
        let f = mixerFake()
        let m = try DocumentModel(session: f)
        let start = m.transport.tick
        var seen: [[Int64]] = []
        SurfaceRelay().apply(
            [.jog(detents: 4), .command(id: "transport.play", programmatic: false)], m,
            perform: { _ in seen.append(f.locates) }, quietly: { _ in })
        #expect(seen == [[start + Int64(4) * MeterMath.ppq / 4]])
    }

    /// Fix round 1 I1: the same across two batches inside the window.
    @Test func aJogIsLocatedBeforeACommandInTheNextBatch() throws {
        let f = mixerFake()
        let m = try DocumentModel(session: f)
        let clock = RelayClock()
        let relay = SurfaceRelay(now: { clock.now })
        let start = m.transport.tick
        relay.apply([.jog(detents: 2)], m, perform: { _ in }, quietly: { _ in })
        clock.now += .milliseconds(40)
        relay.flush(m)
        #expect(f.locates.isEmpty)
        var seen: [[Int64]] = []
        relay.apply(
            [.command(id: "transport.play", programmatic: true)], m,
            perform: { _ in }, quietly: { _ in seen.append(f.locates) })
        #expect(seen == [[start + Int64(2) * MeterMath.ppq / 4]])
        clock.now += SurfaceRelay.jogWindow
        relay.flush(m)
        #expect(f.locates.count == 1, "nothing left to locate")
    }

    /// Review M2: the jog counts from where the transport stands now (a
    /// core-side marker jump the last poll has not seen), not below 0.
    @Test func aJogCountsFromTheCurrentTickAndStopsAtZero() throws {
        let f = mixerFake()
        let m = try DocumentModel(session: f)
        let clock = RelayClock()
        let relay = SurfaceRelay(now: { clock.now })
        f.state.tick = 7680
        relay.apply([.jog(detents: 1)], m, perform: { _ in }, quietly: { _ in })
        clock.now += SurfaceRelay.jogWindow
        relay.flush(m)
        #expect(f.locates == [7680 + MeterMath.ppq / 4])
        relay.apply([.jog(detents: -100)], m, perform: { _ in }, quietly: { _ in })
        clock.now += SurfaceRelay.jogWindow
        relay.flush(m)
        #expect(f.locates.last == 0)
    }

    /// Review M1: Go to Position and the other field keys move the
    /// keyboard focus, so they still wait while a field is edited.
    @Test func aFieldKeyStillWaitsForTheTextField() throws {
        let f = demoFake()
        let s = DocumentStore(makeEmpty: { FakeSession([]) }, makeDemo: { f }, open: { _ in demoFake() })
        let id = try #require(s.newDemo())
        let m = try #require(s.model(id))
        let host = FakeControlHost()
        m.editor.panelKey = true
        var codes: [ControlErrorCode] = []
        SurfaceRelay().apply(
            [.command(id: "transport.go_position", programmatic: false)], m,
            perform: {
                if case .failed(let e) = ControlCommands.run($0, args: [], doc: (id, m), store: s, host: host) {
                    codes.append(e.code)
                }
            }, quietly: { _ in })
        #expect(codes == [.unavailable])
        #expect(host.calls.isEmpty, "no field took the focus")
    }

    /// Review M5: the selection reaches the surfaces only when it changes.
    @Test func theSelectionIsReportedWhenItChanges() throws {
        let f = mixerFake()
        let m = try DocumentModel(session: f)
        m.selectedStrips = [2]
        m.selectedStrips = [2]
        m.selectedStrips = [3, 1]
        #expect(f.surfaceSelections == [[2], [1, 3]])
    }

    /// Review M5: `idle` drains the session's surface events into the relay.
    @Test func idleDrainsSurfaceEvents() throws {
        let f = mixerFake()
        let m = try DocumentModel(session: f)
        var performed: [String] = []
        m.surfaceCommandSink = { performed.append($0) }
        f.surfaceEvents = [.select(strip: 2), .command(id: "transport.stop", programmatic: false)]
        m.idle()
        #expect(m.selectedStrips == [2] && performed == ["transport.stop"])
        #expect(f.surfaceEvents.isEmpty)
    }

    /// Plan 5f-4 Ruling 16 (owner, 2026-11-03): the keyboard's Count-in
    /// toggles the document's record count-in between off and its last bar
    /// count.
    @Test func aCountInPressTogglesTheRecordCountIn() throws {
        let f = mixerFake()
        let m = try DocumentModel(session: f)
        let relay = SurfaceRelay()
        m.record.countInBars = 3
        m.record.countInBars = 0
        relay.apply([.countIn], m, perform: { _ in }, quietly: { _ in })
        #expect(m.record.countInBars == 3, "its last bar count")
        relay.apply([.countIn], m, perform: { _ in }, quietly: { _ in })
        #expect(m.record.countInBars == 0)
        relay.apply([.countIn], m, perform: { _ in }, quietly: { _ in })
        #expect(m.record.countInBars == 3)
    }

    /// Ruling 16: the document tells the core its count-in for the
    /// keyboard's light, once per change.
    @Test func idleTellsTheSurfacesTheCountIn() throws {
        let f = mixerFake()
        let m = try DocumentModel(session: f)
        m.record.countInBars = 0
        m.idle()
        #expect(f.surfaceCountIns == [false], "the first idle says it")
        m.idle()
        #expect(f.surfaceCountIns == [false], "unchanged: not again")
        m.record.countInBars = 2
        m.idle()
        m.record.countInBars = 1
        m.idle()
        m.record.countInBars = 0
        m.idle()
        #expect(f.surfaceCountIns == [false, true, false])
    }

    /// Final review T7 m3: a count-in the core refused is said again at the
    /// next idle.
    @Test func aRefusedCountInIsSaidAgain() throws {
        let f = mixerFake()
        let m = try DocumentModel(session: f)
        m.record.countInBars = 2
        f.surfaceCountInError = .Model(message: "busy")
        m.idle()
        #expect(f.surfaceCountIns == [true])
        f.surfaceCountInError = nil
        m.idle()
        #expect(f.surfaceCountIns == [true, true], "said again")
        m.idle()
        #expect(f.surfaceCountIns == [true, true], "then once only")
    }

    /// Review M4: a refused add keeps the sheet open and says why in it.
    @Test func aRefusedAddKeepsTheSheet() {
        let c = FakeSurfaceCore()
        let s = SurfaceSettings(core: c)
        s.beginAdd()
        c.addError = .Model(message: "VizDAW drives one MCU-compatible main unit and up to three extenders")
        s.commitAdd()
        #expect(s.draft != nil && s.alert == nil)
        #expect(s.draftProblem == "VizDAW drives one MCU-compatible main unit and up to three extenders")
        c.addError = nil
        s.commitAdd()
        #expect(s.draft == nil && s.draftProblem == nil && s.rows.count == 1)
    }

    /// Plan 5f-4 Ruling 10: choosing the Kontrol fills both ports with its
    /// DAW port, and the unit is added without a position.
    @Test func aKontrolDraftPicksTheKeyboardsDawPorts() {
        let c = FakeSurfaceCore()
        c.kontrol = SurfacePortsRow(sources: ["KONTROL S88 MK3 DAW"], destinations: ["KONTROL S88 MK3 DAW"])
        let s = SurfaceSettings(core: c)
        s.beginAdd()
        #expect(s.draft?.input == "In A")
        s.setDraftKind(.kontrolMk3)
        #expect(s.draft?.kind == .kontrolMk3)
        #expect(s.draft?.input == "KONTROL S88 MK3 DAW" && s.draft?.output == "KONTROL S88 MK3 DAW")
        #expect(s.draftNote == nil)
        s.commitAdd()
        #expect(s.draft == nil)
        #expect(c.rows.first?.kind == .kontrolMk3 && c.rows.first?.position == 0)
        #expect(s.showsKontrolNotice, "the list shows the notice once a Kontrol is there (M11)")
    }

    /// Ruling 10: with no DAW port, the ports stay and the sheet says why.
    @Test func aKontrolDraftWithoutAKeyboardSaysSo() {
        let c = FakeSurfaceCore()
        let s = SurfaceSettings(core: c)
        s.beginAdd()
        s.setDraftKind(.kontrolMk3)
        #expect(s.draft?.input == "In A" && s.draft?.output == "Out A")
        #expect(s.draftNote == SurfaceSettings.noKontrol)
        s.setDraftKind(.hui)
        #expect(s.draftNote == nil && s.draft?.kind == .hui)
        s.cancelAdd()
        s.setDraftKind(.kontrolMk3)
        #expect(s.draft == nil, "no sheet, nothing to set")
    }

    /// Pre-flight I4 (Ruling 10): a row says why its unit is not answering;
    /// a line that names no row (an unknown kind kept in surfaces.json) is
    /// shown under the list.
    @Test func aKontrolWithOldFirmwareSaysWhy() {
        let c = FakeSurfaceCore()
        let s = SurfaceSettings(core: c)
        #expect(!s.showsKontrolNotice)
        s.add(.kontrolMk3, input: "KONTROL S88 MK3 DAW", output: "KONTROL S88 MK3 DAW", position: nil)
        let old = "the keyboard answered protocol version 3; VizDAW needs version 4 (keyboard firmware 2.0 or later)"
        let kept = "surface 3: “future_box” needs a newer VizDAW; it is kept in surfaces.json and not driven"
        c.stateRow = SurfaceStateRow(
            surfaces: [], feedback: [], learning: nil,
            problems: ["surface 0: an older line", "surface 0: \(old)", kept])
        s.refresh()
        #expect(s.problem(for: 0) == old, "the last line about it, without the prefix")
        #expect(s.problem(for: 1) == nil)
        #expect(s.otherProblems == [kept])
        #expect(s.showsKontrolNotice)
    }

    /// Final review I2: a row that answers shows no problem line (the core
    /// drops them too; a line left from before is not shown).
    @Test func anAnsweringRowShowsNoProblemLine() {
        let c = FakeSurfaceCore()
        let s = SurfaceSettings(core: c)
        s.add(.kontrolMk3, input: "KONTROL S88 MK3 DAW", output: "KONTROL S88 MK3 DAW", position: nil)
        c.stateRow = SurfaceStateRow(
            surfaces: [], feedback: [], learning: nil,
            problems: ["surface 0: the keyboard answered protocol version 3"])
        s.refresh()
        #expect(s.problem(for: 0) != nil, "not answering: the line says why")
        c.rows[0].online = true
        s.refresh()
        #expect(s.problem(for: 0) == nil, "answering: no line under Connected")
    }

    /// Final review T6 m1: a half-found keyboard fills the side that was
    /// found, and the sheet says nothing.
    @Test func aHalfFoundKontrolFillsTheSideFound() {
        let c = FakeSurfaceCore()
        c.kontrol = SurfacePortsRow(sources: ["KONTROL S88 MK3 DAW"], destinations: [])
        let s = SurfaceSettings(core: c)
        s.beginAdd()
        s.setDraftKind(.kontrolMk3)
        #expect(s.draft?.input == "KONTROL S88 MK3 DAW" && s.draft?.output == "Out A")
        #expect(s.draftNote == nil)
        c.kontrol = SurfacePortsRow(sources: [], destinations: ["KONTROL S49 MK3 DAW"])
        s.beginAdd()
        s.setDraftKind(.kontrolMk3)
        #expect(s.draft?.input == "In A" && s.draft?.output == "KONTROL S49 MK3 DAW")
        #expect(s.draftNote == nil)
    }

    /// Final review T6 m2: choosing the Kontrol reads the ports again first,
    /// so a DAW port that appeared since the sheet opened is in the pickers.
    @Test func choosingTheKontrolRefreshesThePorts() {
        let c = FakeSurfaceCore()
        let s = SurfaceSettings(core: c)
        s.beginAdd()
        let reads = c.portReads
        c.portList = SurfacePortsRow(sources: ["In A", "KONTROL S88 MK3 DAW"], destinations: ["Out A", "KONTROL S88 MK3 DAW"])
        c.kontrol = SurfacePortsRow(sources: ["KONTROL S88 MK3 DAW"], destinations: ["KONTROL S88 MK3 DAW"])
        s.setDraftKind(.kontrolMk3)
        #expect(c.portReads > reads)
        #expect(s.ports.sources.contains("KONTROL S88 MK3 DAW"))
        #expect(s.draft?.input == "KONTROL S88 MK3 DAW")
    }

    /// Final review T6 m3: no ports at all and the Kontrol chosen: the note,
    /// and the pickers stay empty.
    @Test func noPortsAtAllAndTheKontrolGiveTheNote() {
        let c = FakeSurfaceCore()
        c.portList = SurfacePortsRow(sources: [], destinations: [])
        let s = SurfaceSettings(core: c)
        s.beginAdd()
        s.setDraftKind(.kontrolMk3)
        #expect(s.draftNote == SurfaceSettings.noKontrol)
        #expect(s.draft?.input == "" && s.draft?.output == "")
    }

    /// Ruling 11: descriptive wording only.
    @Test func theKontrolSheetSpeaksDescriptively() {
        #expect(SurfaceSettings.kontrolNotice
            == "Kontrol and Native Instruments are trademarks of Native Instruments GmbH. VizDAW is not affiliated with or endorsed by Native Instruments.")
        for text in [SurfaceSettings.kontrolHelp, SurfaceSettings.noKontrol, SurfaceSettings.title(.kontrolMk3)] {
            #expect(!text.localizedCaseInsensitiveContains("official"))
            #expect(!text.contains("NKS"))
        }
    }

    /// Pre-flight I9: a command `surface.inject` or `midi.inject` caused
    /// never takes the key path, which would open the Save panel.
    @Test func anInjectedSaveOpensNoDialog() throws {
        let m = try DocumentModel(session: mixerFake())
        var performed: [String] = []
        var quiet: [String] = []
        SurfaceRelay().apply(
            [.command(id: "file.save", programmatic: true), .command(id: "edit.undo", programmatic: false)], m,
            perform: { performed.append($0) }, quietly: { quiet.append($0) })
        #expect(quiet == ["file.save"])
        #expect(performed == ["edit.undo"], "a real button still runs as a key")
    }
}
