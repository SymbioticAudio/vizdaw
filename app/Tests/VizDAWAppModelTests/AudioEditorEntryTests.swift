import Foundation
import Testing
@testable import VizDAWAppModel

@Suite struct AudioEditorEntryTests {
    @Test func gainsAcceptDecibelsAndClamp() {
        #expect(AudioEditorEntry.gain("-4.2") == -4.2)
        #expect(AudioEditorEntry.gain("+6 dB") == 6)
        #expect(AudioEditorEntry.gain("30") == 24)
        for s in ["-200", "-inf", "\u{2212}\u{221E}"] { #expect(AudioEditorEntry.gain(s) == -144, "\(s)") }
        let z = AudioEditorEntry.gain("-0.04")
        #expect(z == 0)
        #expect(z?.sign == .plus)
        #expect(AudioEditorEntry.gainText(-144) == "\u{2212}\u{221E} dB")
        #expect(AudioEditorEntry.gainText(24) == "+24.0 dB")
        #expect(AudioEditorEntry.gainText(-4.2) == "\u{2212}4.2 dB")
        #expect(AudioEditorEntry.gainText(-0.0) == "0.0 dB")
    }

    @Test func hostileGainsAreRefused() {
        let bad = ["", "   ", "abc", "4.2.1", "--3", "nan", "inf", "1e3", "1,5",
                   "-4.2 dBFS", "4\u{0}", String(repeating: "1", count: 33), String(repeating: "9", count: 300)]
        for s in bad { #expect(AudioEditorEntry.gain(s) == nil, "\(s)") }
    }

    @Test func timesAcceptTheirForms() {
        let d = AudioEditorEntry.duration
        #expect(d("250", .ms) == 0.25)
        #expect(d("250 ms", .s) == 0.25)
        #expect(d("1.5 s", .ms) == 1.5)
        #expect(d("1.5", .s) == 1.5)
        #expect(d("1.5S", .ms) == 1.5)
        #expect(d("1:02.5", .s) == 62.5)
        #expect(d("0:00.250", .ms) == 0.25)
        #expect(d(" 2 ms ", .ms) == 0.002)
        #expect(d("0", .ms) == 0)
        #expect(d(".5", .s) == 0.5)
        #expect(AudioEditorEntry.frames(0.25, rate: 48_000) == 12_000)
        #expect(AudioEditorEntry.frames(0.0000104, rate: 48_000) == 0)
        #expect(AudioEditorEntry.frames(1, rate: 0) == 0)
    }

    @Test func hostileTimesAreRefused() {
        let bad = ["", " ", "abc", "-5", "+5", "\u{2212}5", "1.2.3", "1e3", "nan", "inf",
                   "5 sec", "5 mss", "5ms5", "ms", "5  ms", "1:75", "1:5", ":30", "1:", "1::00",
                   "99999:00", "1:60", "1:00.1234", "0:00.", "1:02:03", "0x10", "\u{FF12}\u{FF15}\u{FF10}", "\u{663}", "5\nms", "\u{7}5",
                   String(repeating: "1", count: 33), String(repeating: "9", count: 300)]
        for s in bad {
            #expect(AudioEditorEntry.duration(s, bare: .ms) == nil, "\(s)")
            #expect(AudioEditorEntry.duration(s, bare: .s) == nil, "\(s)")
        }
    }

    @Test func peaksClampAndRefuseMinusInfinity() {
        #expect(AudioEditorEntry.peak("-0.3") == -0.3)
        #expect(AudioEditorEntry.peak("\u{2212}1 dB") == -1)
        #expect(AudioEditorEntry.peak("+3") == 0)
        #expect(AudioEditorEntry.peak("-200") == -96)
        for s in ["-inf", "\u{2212}\u{221E}", "abc", ""] { #expect(AudioEditorEntry.peak(s) == nil, "\(s)") }
        #expect(AudioEditorEntry.peakText(-0.3) == "\u{2212}0.3 dB")
        #expect(AudioEditorEntry.peakText(0) == "0.0 dB")
    }

    @Test func theTextsReadBack() {
        #expect(AudioEditorEntry.fadeText(frames: 12_000, rate: 48_000) == "250 ms")
        #expect(AudioEditorEntry.fadeText(frames: 72_000, rate: 48_000) == "1500 ms")
        #expect(AudioEditorEntry.fadeText(frames: 0, rate: 48_000) == "0 ms")
        #expect(AudioEditorEntry.fadeText(frames: 5, rate: 0) == "\u{2013}")
        let back = AudioEditorEntry.duration(AudioEditorEntry.fadeText(frames: 12_000, rate: 48_000), bare: .ms)!
        #expect(AudioEditorEntry.frames(back, rate: 48_000) == 12_000)
        #expect(AudioEditorEntry.duration(Formatting.time(1.25), bare: .s) == 1.25)
        #expect(AudioEditorEntry.gain(AudioEditorEntry.gainText(-4.2)) == -4.2)
    }

    /// Task 2 review M3: the edges of the grammar, the clamps and the texts.
    @Test func theEdgesHold() {
        let d = AudioEditorEntry.duration
        #expect(abs((d("1:59.999", .s) ?? 0) - 119.999) < 1e-9)
        #expect(d("0:00.5", .s) == 0.5)
        #expect(d("9999:00", .s) == 599_940)
        // Frames: clamped to 0…Int64.max / 2; NaN, ±∞ and negative give 0.
        #expect(AudioEditorEntry.frames(1e300, rate: 48_000) == Int64.max / 2)
        #expect(AudioEditorEntry.frames(Double(Int64.max), rate: 192_000) == Int64.max / 2)
        for s in [Double.nan, .infinity, -.infinity, -1] {
            #expect(AudioEditorEntry.frames(s, rate: 48_000) == 0, "\(s)")
        }
        // Whole ms, half away from zero.
        #expect(AudioEditorEntry.fadeText(frames: 24, rate: 48_000) == "1 ms")
        #expect(AudioEditorEntry.fadeText(frames: 23, rate: 48_000) == "0 ms")
        #expect(AudioEditorEntry.fadeText(frames: 100, rate: 44_100) == "2 ms")
        #expect(AudioEditorEntry.fadeText(frames: -5, rate: 48_000) == "0 ms")
        #expect(AudioEditorEntry.fadeText(frames: .max, rate: 1) == "9000000000000000000 ms")
        // Gain in tenths.
        #expect(AudioEditorEntry.gain("-4.26") == -4.3)
        #expect(AudioEditorEntry.gainText(-0.63) == "\u{2212}0.6 dB")
        #expect(AudioEditorEntry.gainText(11.04) == "+11.0 dB")
        // The hints, as the status line shows them.
        #expect(AudioEditorEntry.gainHint == "Type a gain in dB, like \u{2212}4.2 or +6.")
        #expect(AudioEditorEntry.fadeHint == "Type a fade like 250 ms, 1.5 s or 480 frames.")
        #expect(AudioEditorEntry.timeHint == "Type a time like 0:01.250, 1.25 s or 1250 ms.")
        #expect(AudioEditorEntry.peakHint == "Type a peak from \u{2212}96 to 0 dB, like \u{2212}0.3.")
        #expect(AudioEditorEntry.noMediaHint == "This region's audio file is unknown.")
        for (box, hint) in [
            (AudioEditorBox.gain, AudioEditorEntry.gainHint), (.fadeIn, AudioEditorEntry.fadeHint),
            (.fadeOut, AudioEditorEntry.fadeHint), (.crossfade, AudioEditorEntry.fadeHint), (.start, AudioEditorEntry.timeHint),
            (.length, AudioEditorEntry.timeHint), (.peak, AudioEditorEntry.peakHint),
        ] {
            #expect(box.hint == hint, "\(box)")
        }
    }

    /// Plan 6c Task 6: the fade boxes read ms, seconds and whole frames.
    @Test func fadeFramesReadsMsSecondsAndFrames() {
        func f(_ t: String) -> Int64? { AudioEditorEntry.fadeFrames(t, rate: 48_000) }
        #expect(f("250 ms") == 12_000)
        #expect(f("1.5 s") == 72_000)
        #expect(f("480 frames") == 480)
        #expect(f("1 frame") == 1)
        #expect(f("10") == 480, "a bare number is in ms")
        #expect(f("0 frames") == 0)
        #expect(f("4611686018427387903 frames") == Int64.max / 2)
        for t in [
            "1.5 frames", "-3 frames", "NaN", "inf ms", "1e30 frames", "", "frames", "4611686018427387904 frames",
            "99999999999999999999 frames", "+3 frames", "3 framess", "0x10 frames",
        ] {
            #expect(f(t) == nil, "\(t)")
        }
        #expect(AudioEditorEntry.crossfadeInHint == "This region's fade in is a crossfade: type its length in the Crossfade box.")
        #expect(AudioEditorEntry.noCrossfadeHint == "This region has no crossfade. Use Region \u{203A} Crossfade.")
    }
}
