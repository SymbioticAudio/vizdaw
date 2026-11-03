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
