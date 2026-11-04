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
                   "99999:00", "0x10", "\u{FF12}\u{FF15}\u{FF10}", "\u{663}", "5\nms", "\u{7}5",
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
}
