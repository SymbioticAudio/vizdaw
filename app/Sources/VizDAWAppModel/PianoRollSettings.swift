import VizDAWCore

/// A MIDI 2.0 per-note controller the expression pane shows (Plan 4b
/// Ruling 14).
public enum ExpressionKind: Codable, Hashable, Sendable {
    case pitchBend
    case registered(UInt8)
    case assignable(UInt8)

    public static let choices: [ExpressionKind] = [
        .pitchBend, .registered(1), .registered(2), .registered(3), .registered(7), .registered(10),
        .registered(74), .assignable(1), .assignable(2), .assignable(3), .assignable(4),
    ]

    static let registeredNames: [UInt8: String] = [
        1: "Modulation", 2: "Breath", 3: "Pitch 7.25", 7: "Volume", 10: "Pan", 74: "Timbre",
    ]

    public init(_ row: PerNoteKindRow) {
        switch row {
        case .pitchBend: self = .pitchBend
        case .registered(let i): self = .registered(i)
        case .assignable(let i): self = .assignable(i)
        }
    }

    public var row: PerNoteKindRow {
        switch self {
        case .pitchBend: .pitchBend
        case .registered(let i): .registered(index: i)
        case .assignable(let i): .assignable(index: i)
        }
    }

    public var title: String {
        switch self {
        case .pitchBend: "Per-Note Pitch Bend"
        case .registered(let i): "Registered \(i)" + (Self.registeredNames[i].map { " (\($0))" } ?? "")
        case .assignable(let i): "Assignable \(i)"
        }
    }

    /// A note's value before its first point: centre for pitch bend.
    public var neutral: UInt32 { self == .pitchBend ? 0x8000_0000 : 0 }
}

/// View settings every piano roll shares, saved with the project (Plan 4b
/// Ruling 11). Decoding is tolerant like `EditorLayout`'s.
public struct PianoRollSettings: Codable, Equatable, Sendable {
    public var showKeyboard = true
    public var rowHeight: Double = 10
    /// The piano roll's own zoom (a 4/4 bar is 640 pt at 96).
    public var ticksPerPoint: Double = 96
    public var expression: ExpressionKind = .pitchBend
    public var showRegions = true
    public var showExpression = true

    public static let rowHeightRange: ClosedRange<Double> = 4...40

    public init() {}

    enum CodingKeys: String, CodingKey {
        case showKeyboard, rowHeight, ticksPerPoint, expression, showRegions, showExpression
    }

    public init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        let d = PianoRollSettings()
        showKeyboard = c.field(.showKeyboard, d.showKeyboard)
        rowHeight = EditorLayout.sane(c.field(.rowHeight, d.rowHeight), Self.rowHeightRange, d.rowHeight)
        ticksPerPoint = EditorLayout.sane(
            c.field(.ticksPerPoint, d.ticksPerPoint), EditorLayout.ticksPerPointRange, d.ticksPerPoint)
        expression = c.field(.expression, d.expression)
        showRegions = c.field(.showRegions, d.showRegions)
        showExpression = c.field(.showExpression, d.showExpression)
    }
}
