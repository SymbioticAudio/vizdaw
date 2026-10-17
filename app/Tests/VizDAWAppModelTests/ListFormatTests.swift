import Testing
import VizDAWCore
@testable import VizDAWAppModel

private let q: Int64 = 15_360
private let fourFour = [MeterEntryRow(tick: 0, num: 4, den: 4, sharps: 0, minor: false)]

@Suite struct ListFormatTests {
    @Test func positionsReadLikeTheTransport() {
        #expect(Bbu.of(0, meter: fourFour).text == "1.1.0")
        #expect(Bbu.of(q, meter: fourFour).text == "1.2.0")
        #expect(Bbu.of(4 * q + q / 2, meter: fourFour).text == "2.1.480")
        #expect(Bbu.of(-q, meter: fourFour).text == "0.4.0", "a pickup")
        let sixEight = fourFour + [MeterEntryRow(tick: 4 * q, num: 6, den: 8, sharps: 0, minor: false)]
        #expect(Bbu.of(4 * q + 3 * (q / 2), meter: sixEight).text == "2.4.0")
        #expect(Bbu(bar: 2, beat: 1, unit: 480).tick(meter: fourFour) == 4 * q + q / 2)
        for t: Int64 in [0, 1, q - 1, 7 * q + 123, -5 * q] {
            let b = Bbu.of(t, meter: fourFour)
            let back = b.tick(meter: fourFour)
            #expect(back <= t && Bbu.of(back, meter: fourFour) == b, "\(t)")
        }
    }

    @Test func typedPositionsKeepTheSubFieldsLeftEmpty() {
        let cur = Bbu(bar: 3, beat: 2, unit: 240)
        #expect(PositionInput.parse("5", current: cur) == Bbu(bar: 5, beat: 1, unit: 0), "bar number: downbeat")
        #expect(PositionInput.parse("5.3", current: cur) == Bbu(bar: 5, beat: 3, unit: 0))
        #expect(PositionInput.parse("5.3.480", current: cur) == Bbu(bar: 5, beat: 3, unit: 480))
        #expect(PositionInput.parse(".4", current: cur) == Bbu(bar: 3, beat: 4, unit: 240), "beat only")
        #expect(PositionInput.parse("..480", current: cur) == Bbu(bar: 3, beat: 2, unit: 480), "unit only")
        #expect(PositionInput.parse("7..", current: cur) == Bbu(bar: 7, beat: 2, unit: 240), "bar only")
        #expect(PositionInput.parse(" 6 ", current: cur) == Bbu(bar: 6, beat: 1, unit: 0))
        #expect(PositionInput.parse("-1", current: cur) == Bbu(bar: -1, beat: 1, unit: 0))
    }

    @Test func hugeAndBadPositionsAreRefused() {
        let cur = Bbu(bar: 1, beat: 1, unit: 0)
        for bad in ["", "x", "5.0", "5.1.960", "1.2.3.4", "99999999999999999999", "1.1.99999999999", "1000001", "5.1000"] {
            #expect(PositionInput.parse(bad, current: cur) == nil, "\(bad)")
        }
        let far = Bbu(bar: Bbu.bars.upperBound, beat: Bbu.beats.upperBound, unit: Bbu.units.upperBound)
        #expect(far.tick(meter: fourFour) > 0, "the largest typed position does not trap")
        #expect(DurationText.parse("99999999999", current: 0, beat: q) == nil)
    }

    @Test func lengthsAreBeatsAndUnits() {
        #expect(DurationText.text(q, beat: q) == "1.0")
        #expect(DurationText.text(q + q / 4, beat: q) == "1.240")
        #expect(DurationText.text(0, beat: q) == "0.0")
        #expect(DurationText.parse("2", current: q, beat: q) == 2 * q)
        #expect(DurationText.parse(".480", current: q, beat: q) == q + q / 2, "units only")
        #expect(DurationText.parse("1.5", current: 0, beat: q) == q + 80)
        #expect(DurationText.parse("a", current: 0, beat: q) == nil)
        #expect(DurationText.parse("-1", current: 0, beat: q) == nil)
    }

    @Test func smpteRoundTrips() {
        #expect(Smpte.text(seconds: 3661.5, fps: 25) == "01:01:01:12")
        #expect(Smpte.text(seconds: -0.5, fps: 25) == "-00:00:00:12")
        #expect(Smpte.text(seconds: .nan, fps: 30) == "--:--:--:--")
        #expect(abs(Smpte.parse("01:01:01:12", fps: 25)! - 3661.48) < 1e-9)
        #expect(abs(Smpte.parse("1:12", fps: 25)! - 1.48) < 1e-9, "fields count from the right")
        #expect(Smpte.parse("-0:12", fps: 24) == -0.5)
        #expect(Smpte.parse("00:00:00:25", fps: 25) == nil, "frame out of range")
        #expect(Smpte.parse("1:60:00:00", fps: 25) == nil)
        #expect(Smpte.parse("x", fps: 25) == nil)
    }

    @Test func sevenBitValuesUpscaleByTheMidi2Rule() {
        #expect(Velocity7.up(100) == 0xC924)
        #expect(Velocity7.up(127) == 0xFFFF)
        #expect(Velocity7.up(64) == 0x8000)
        #expect(Velocity7.up(200) == 0xFFFF && Velocity7.up(-5) == 0)
        #expect(Velocity7.of(0xC924) == 100)
        #expect(Value7.up(127) == 0xFFFF_FFFF && Value7.up(64) == 0x8000_0000)
        #expect(Value7.of(Int64(Value7.up(100))) == 100)
        #expect(Bend14.up(0) == 0x8000_0000 && Bend14.up(8191) == 0xFFFF_FFFF && Bend14.up(-8192) == 0)
        #expect(Bend14.of(Int64(Bend14.up(-100))) == -100)
    }

    @Test func pitchAndKeyNamesParse() {
        #expect(PitchName.parse("C4") == 60)
        #expect(PitchName.parse("c#4") == 61)
        #expect(PitchName.parse("Db4") == 61)
        #expect(PitchName.parse("bb3") == 58)
        #expect(PitchName.parse("C-1") == 0)
        #expect(PitchName.parse("G9") == 127)
        #expect(PitchName.parse("G#9") == nil)
        #expect(PitchName.parse("64") == 64)
        #expect(PitchName.parse("H2") == nil && PitchName.parse("C") == nil && PitchName.parse("200") == nil)
        #expect(KeyName.name(sharps: 2, minor: false) == "D major")
        #expect(KeyName.name(sharps: -3, minor: true) == "C minor")
        #expect(KeyName.parse("D major")! == (2, false))
        #expect(KeyName.parse("c minor")! == (-3, true))
        #expect(KeyName.parse("F# minor")! == (3, true))
        #expect(KeyName.parse("Gb major")! == (-6, false))
        #expect(KeyName.parse("H major") == nil && KeyName.parse("C") == nil)
    }

    @Test func hugeOctavesAreRefused() {
        for bad in ["C99999999999999999999", "C9223372036854775807", "C-9223372036854775808"] {
            #expect(PitchName.parse(bad) == nil, "\(bad)")
        }
    }

    @Test func smpteEveryFrameRoundTrips() {
        for fps in Smpte.rates {
            for n in 0..<(fps * 5) {
                let t = Smpte.text(seconds: Double(n) / Double(fps), fps: fps)
                let s = Smpte.parse(t, fps: fps)
                #expect(s != nil && Smpte.text(seconds: s!, fps: fps) == t, "\(fps) \(n) \(t)")
            }
        }
    }
}
