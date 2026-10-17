/// Note names as the core prints them: C-1 is 0, middle C (60) is "C4".
public enum PitchName {
    static let names = ["C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B"]

    public static func name(_ p: UInt8) -> String {
        "\(names[Int(p % 12)])\(Int(p / 12) - 1)"
    }

    public static func isBlack(_ p: UInt8) -> Bool {
        [1, 3, 6, 8, 10].contains(Int(p % 12))
    }
}

/// Scales nudge can follow (Plan 4b Ruling 16). Stepping itself lives in the
/// core (`scale_step`, `EventCmd::TransposeInScale`); the UI only names them.
public enum Scale: String, Codable, CaseIterable, Sendable {
    case major, naturalMinor, harmonicMinor, dorian, mixolydian, majorPentatonic, minorPentatonic

    /// Bit `i`: pitch class `root + i` (the core's mask convention).
    public var mask: UInt16 {
        switch self {
        case .major: 0b1010_1011_0101
        case .naturalMinor: 0b0101_1010_1101
        case .harmonicMinor: 0b1001_1010_1101
        case .dorian: 0b0110_1010_1101
        case .mixolydian: 0b0110_1011_0101
        case .majorPentatonic: 0b0010_1001_0101
        case .minorPentatonic: 0b0100_1010_1001
        }
    }

    public var title: String {
        switch self {
        case .major: "Major"
        case .naturalMinor: "Natural Minor"
        case .harmonicMinor: "Harmonic Minor"
        case .dorian: "Dorian"
        case .mixolydian: "Mixolydian"
        case .majorPentatonic: "Major Pentatonic"
        case .minorPentatonic: "Minor Pentatonic"
        }
    }
}

/// Keys (Plan 4b Ruling 17).
public enum ScaleMath {
    public static let rootNames = PitchName.names

    /// The root pitch class of a key signature: (7 × sharps) mod 12, plus 9
    /// for a minor key.
    public static func keyRoot(sharps: Int8, minor: Bool) -> UInt8 {
        let major = ((7 * Int(sharps)) % 12 + 12) % 12
        return UInt8((major + (minor ? 9 : 0)) % 12)
    }
}
