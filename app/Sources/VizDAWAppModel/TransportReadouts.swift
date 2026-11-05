import CoreGraphics
import Foundation
import SymbioticTokens
import VizDAWCore

/// The four readouts' texts (plan 7d Ruling 6).
public struct ReadoutTexts: Equatable, Sendable {
    public let bar, time, tempo, signature: String

    public init(bar: String, time: String, tempo: String, signature: String) {
        self.bar = bar
        self.time = time
        self.tempo = tempo
        self.signature = signature
    }

    /// The text a box shows.
    public func text(_ box: TransportReadouts.Box) -> String {
        switch box {
        case .bar: bar
        case .time: time
        case .tempo: tempo
        case .signature: signature
        }
    }
}

/// The transport bar's LCD readouts: Bar, Time, Tempo and Sig (plan 7d
/// Ruling 6), their texts, their look and the one label-case rule.
public enum TransportReadouts {
    public enum Box: CaseIterable, Sendable {
        case bar, time, tempo, signature

        /// The shown label.
        public var title: String {
            switch self {
            case .bar: "Bar"
            case .time: "Time"
            case .tempo: "Tempo"
            case .signature: "Sig"
            }
        }

        public var accessibilityLabel: String {
            switch self {
            case .bar: "Bar Readout"
            case .time: "Time Readout"
            case .tempo: "Tempo Readout"
            case .signature: "Signature Readout"
            }
        }
    }

    /// The dash for a value that is not there.
    static let dash = "–"
    /// The ticks of one beat's quarter: ticks per second × 60 / this = BPM.
    private static let ticksPerQuarter = 15_360.0
    /// The last tempo shown, in hundredths of a BPM.
    private static let tempoCeiling = 99_999.0
    /// Positions are clamped to ±2⁵⁰ (`TimeAxis.limit`, which this module
    /// cannot see; a test pins them equal).
    static let tickLimit: Int64 = 1 << 50

    /// `ticksPerSecond` as BPM to two places; a dash for 0, a negative or
    /// non-finite value, and for one above 999.99 once rounded.
    public static func tempoText(ticksPerSecond: Double) -> String {
        guard ticksPerSecond.isFinite, ticksPerSecond > 0 else { return dash }
        let hundredths = (ticksPerSecond * 60 / ticksPerQuarter * 100).rounded()
        guard hundredths.isFinite, hundredths <= tempoCeiling else { return dash }
        return String(format: "%.2f", hundredths / 100)
    }

    /// The meter in effect at `tick`; a dash with no sequence info. The
    /// tick is clamped first and never goes through `MeterMath.bar`, whose
    /// bar arithmetic traps at `Int64.min`.
    static func signatureText(tick: Int64, meter: [MeterEntryRow]?) -> String {
        guard let meter else { return dash }
        let t = min(max(tick, -tickLimit), tickLimit)
        guard let m = meter.last(where: { $0.tick <= t }) ?? meter.first else { return "4/4" }
        return "\(m.num)/\(m.den)"
    }

    public static func texts(_ t: TransportState, meter: [MeterEntryRow]?) -> ReadoutTexts {
        ReadoutTexts(
            bar: Formatting.bbu(t.positionBbu), time: Formatting.time(t.seconds),
            tempo: tempoText(ticksPerSecond: t.ticksPerSecond),
            signature: signatureText(tick: t.tick, meter: meter))
    }

    /// The widest text each box shows: its fixed width is this in its font.
    public static let widest = ReadoutTexts(
        bar: "9999.4.960", time: "999:59.999", tempo: "999.99", signature: "15/16")

    /// How the boxes draw in a theme.
    public struct ReadoutLook: Equatable, Sendable {
        public let fill, value, label: ThemeColor
        /// The group's edge; nil for separate tiles.
        public let edge: ThemeColor?
        /// Between the boxes.
        public let gap: CGFloat
        public let radius: CGFloat
        /// Inside a box, at each side.
        public let padding: CGFloat
    }

    /// A hairline's width.
    static let hairline: CGFloat = 1
    /// The label's share of the text colour over the fill.
    private static let labelShare = 0.72

    /// In a tile theme each box is a tile in the accent; elsewhere the
    /// boxes are one group in the readout colours, edged in the separator.
    public static func look(_ t: Theme) -> ReadoutLook {
        if t.style.tile == .tile {
            return ReadoutLook(
                fill: t.accent.accent, value: t.accent.onAccent, label: t.accent.onAccent, edge: nil,
                gap: t.spacing.xs, radius: t.radius.m, padding: t.spacing.m)
        }
        return ReadoutLook(
            fill: t.readout.background, value: t.readout.text,
            label: blend(t.readout.text, over: t.readout.background, share: labelShare),
            edge: t.surface.separator, gap: hairline, radius: t.radius.m, padding: t.spacing.m)
    }

    /// `top` at `share` over `bottom`, in sRGB.
    static func blend(_ top: ThemeColor, over bottom: ThemeColor, share: Double) -> ThemeColor {
        let a = top.fallbackComponents
        let b = bottom.fallbackComponents
        func byte(_ x: Double, _ y: Double) -> UInt32 {
            UInt32(((x * share + y * (1 - share)) * 255).rounded())
        }
        let rgb = byte(a.red, b.red) << 24 | byte(a.green, b.green) << 16 | byte(a.blue, b.blue) << 8
        return ThemeColor(hex: rgb | 0xFF)
    }

    /// A label: capitals, except lower case in a lower-case theme. The
    /// strip headings use it too, so the two cannot drift.
    public static func label(_ text: String, _ t: Theme) -> String {
        t.style.labels == .lowercase ? text.lowercased() : text.uppercased()
    }
}

extension DocumentModel {
    /// The bar, time, tempo and signature the transport bar shows. The
    /// meter comes from the sequence info, whose cache is not observable:
    /// a view also watches `snapshot`.
    public var readoutTexts: ReadoutTexts {
        let seq = transport.seq ?? selectedSeq
        return TransportReadouts.texts(transport, meter: seq.flatMap { editor.info($0)?.meter })
    }
}
