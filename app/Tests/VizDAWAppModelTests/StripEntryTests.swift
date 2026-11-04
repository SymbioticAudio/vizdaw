import Foundation
import Testing
@testable import VizDAWAppModel

/// Plan 7b Ruling 15: the typed-value grammars.
@Suite struct StripEntryTests {
    @Test func levelsAcceptDecibelsWithOrWithoutTheUnit() {
        for text in ["-4.2", "\u{2212}4.2 dB", "-4.2db", " -4.2 DB "] {
            #expect(StripEntry.level(text) == -4.2, "\(text)")
        }
        #expect(StripEntry.level("+3") == 3)
        #expect(StripEntry.level("0") == 0)
        #expect(StripEntry.level(".5") == 0.5)
        #expect(StripEntry.level("-4.25") == -4.3)
    }

    @Test func levelsClampAndReadMinusInfinity() {
        #expect(StripEntry.level("40") == 12)
        for text in ["-200", "-144", "-inf", "\u{2212}\u{221E}", "-\u{221E} dB", "\u{2212}inf", "-INF dB"] {
            #expect(StripEntry.level(text) == -144, "\(text)")
        }
        #expect(StripEntry.levelText(-144) == "\u{2212}\u{221E} dB")
        for text in ["-0.04", "-0"] {
            let v = StripEntry.level(text)
            #expect(v == 0 && v?.sign == .plus, "\(text)")
        }
        #expect(StripEntry.levelText(-0.0) == "0.0 dB")
        #expect(StripEntry.levelText(-4.2) == "\u{2212}4.2 dB")
        #expect(StripEntry.levelText(3) == "+3.0 dB")
    }

    @Test func hostileLevelsAreRefused() {
        let bad = [
            "", "   ", "abc", "4.2.1", "--3", "+-3",
            "nan", "NaN", "inf", "+inf", "1e3", "1e309", "1,5",
            "-4.2 dBFS", "-4.2 dB dB", "4\u{0}",
            String(repeating: "1", count: 33), String(repeating: "9", count: 300),
            "\u{FF14}", "\u{0663}", "4.", "- 4",
        ]
        for text in bad { #expect(StripEntry.level(text) == nil, "\(text.prefix(20))") }
    }

    @Test func pansAcceptLeftCentreRight() {
        for text in ["C", "c", "0", "L0"] { #expect(StripEntry.pan(text) == 0, "\(text)") }
        for text in ["L24", "L 24", "l 24", "-24", "\u{2212}24"] {
            #expect(StripEntry.pan(text) == -0.24, "\(text)")
        }
        for text in ["R10", "r 10", "10", "+10"] { #expect(StripEntry.pan(text) == 0.10, "\(text)") }
        #expect(StripEntry.pan("L150") == -1)
        #expect(StripEntry.pan("L 24.6") == -0.25)
        let r = StripEntry.pan("R 0.4")
        #expect(r == 0 && r?.sign == .plus)
    }

    @Test func hostilePansAreRefused() {
        let bad = [
            "", "LR", "L", "L-5", "L R", "LL24", "24L", "C 5", "nan", "L nan", "\u{221E}", "L1e2",
            "R\u{7}", "L \u{FF12}\u{FF14}", "R \u{0663}", String(repeating: "L", count: 33), "L  24",
        ]
        for text in bad { #expect(StripEntry.pan(text) == nil, "\(text.prefix(20))") }
    }

    @Test func typedValuesReadBackAsTheyShow() {
        for db in [-60.0, -4.2, 0, 12] { #expect(StripEntry.level(StripEntry.levelText(db)) == db, "\(db)") }
        for p in [-1.0, -0.24, 0, 0.1] { #expect(StripEntry.pan(StripEntry.panText(p)) == p, "\(p)") }
        #expect(StripEntry.panText(-0.24) == "L 24")
        #expect(StripEntry.panText(0) == "C")
        #expect(StripEntry.panText(0.1) == "R 10")
    }
}
