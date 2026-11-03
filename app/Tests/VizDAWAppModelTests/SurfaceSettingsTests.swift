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
