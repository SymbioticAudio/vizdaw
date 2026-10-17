import Testing
import VizDAWCore
@testable import VizDAWAppModel

@Suite struct FormattingTests {
    @Test(arguments: [
        (0.0, "0:00.000"),
        (1.5, "0:01.500"),
        (61.0004, "1:01.000"),
        (59.9996, "1:00.000"),
        (3725.25, "62:05.250"),
        (-1.5, "-0:01.500"),
        (-0.0001, "0:00.000"),
    ])
    func time(seconds: Double, text: String) {
        #expect(Formatting.time(seconds) == text)
    }

    @Test func nonFiniteTimeIsDashes() {
        #expect(Formatting.time(.nan) == "--:--.---")
        #expect(Formatting.time(.infinity) == "--:--.---")
    }

    @Test func hugeFiniteTimeDoesNotTrap() {
        let big = Formatting.time(.greatestFiniteMagnitude)
        #expect(big == Formatting.time(1e300))
        #expect(big.hasSuffix(".000") || big.contains(":"))
        #expect(Formatting.time(-.greatestFiniteMagnitude) == "-" + big)
        #expect(Formatting.time(9.3e15) == big)
    }

    @Test func bbuPassesRustTextThrough() {
        #expect(Formatting.bbu("5.2.480") == "5.2.480")
        #expect(Formatting.bbu("0.4.0") == "0.4.0")
        #expect(Formatting.bbu("-1.1.0") == "-1.1.0")
        #expect(Formatting.bbu("") == "-.-.-")
    }

    @Test func positionCombinesBbuAndTime() {
        let t = TransportState(
            playing: true, seq: 1, tick: 0, positionBbu: "0.4.0", seconds: -0.5,
            sampleRate: 48_000, device: "Offline", faults: [], report: [], hostTimeNs: 0,
        ticksPerSecond: 30_720)
        #expect(Formatting.position(t) == "0.4.0  -0:00.500")
        #expect(Formatting.device(t) == "Offline · 48 kHz")
    }

    @Test func sampleRates() {
        #expect(Formatting.sampleRate(44_100) == "44.1 kHz")
        #expect(Formatting.sampleRate(96_000) == "96 kHz")
        #expect(Formatting.sampleRate(0) == "")
    }

    @Test func deviceBeforeFirstPlay() {
        let t = TransportState(
            playing: false, seq: nil, tick: 0, positionBbu: "1.1.0", seconds: 0,
            sampleRate: 0, device: "", faults: [], report: [], hostTimeNs: 0,
        ticksPerSecond: 30_720)
        #expect(Formatting.device(t) == "No output")
    }

    @Test func windowTitleMarksEdits() {
        var s = DocSnapshot(
            version: 0, title: "demo", dirty: false, undoLabel: nil, redoLabel: nil,
            sequences: [], warnings: [], laneRevs: [])
        #expect(Formatting.windowTitle(s) == "demo")
        s.dirty = true
        #expect(Formatting.windowTitle(s) == "demo — Edited")
    }
}
