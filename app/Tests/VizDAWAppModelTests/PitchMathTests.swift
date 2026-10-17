import Testing
@testable import VizDAWAppModel

@Suite struct PitchMathTests {
    @Test func namesMatchTheCore() {
        #expect(PitchName.name(60) == "C4")
        #expect(PitchName.name(0) == "C-1")
        #expect(PitchName.name(61) == "C#4")
        #expect(PitchName.name(127) == "G9")
    }

    @Test func blackKeys() {
        #expect((60...71).filter(PitchName.isBlack) == [61, 63, 66, 68, 70])
    }

    @Test func scaleMasksHoldTheirPitchClasses() {
        func classes(_ s: Scale) -> [Int] { (0..<12).filter { s.mask & (1 << $0) != 0 } }
        #expect(classes(.major) == [0, 2, 4, 5, 7, 9, 11])
        #expect(classes(.naturalMinor) == [0, 2, 3, 5, 7, 8, 10])
        #expect(classes(.harmonicMinor) == [0, 2, 3, 5, 7, 8, 11])
        #expect(classes(.dorian) == [0, 2, 3, 5, 7, 9, 10])
        #expect(classes(.mixolydian) == [0, 2, 4, 5, 7, 9, 10])
        #expect(classes(.majorPentatonic) == [0, 2, 4, 7, 9])
        #expect(classes(.minorPentatonic) == [0, 3, 5, 7, 10])
    }

    @Test func keySignatureRoots() {
        #expect(ScaleMath.keyRoot(sharps: 0, minor: false) == 0)
        #expect(ScaleMath.keyRoot(sharps: 1, minor: false) == 7, "G")
        #expect(ScaleMath.keyRoot(sharps: 2, minor: false) == 2, "D")
        #expect(ScaleMath.keyRoot(sharps: -1, minor: false) == 5, "F")
        #expect(ScaleMath.keyRoot(sharps: -3, minor: false) == 3, "E flat")
        #expect(ScaleMath.keyRoot(sharps: 0, minor: true) == 9, "A minor")
        #expect(ScaleMath.keyRoot(sharps: 1, minor: true) == 4, "E minor")
    }
}
