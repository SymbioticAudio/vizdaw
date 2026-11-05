import Foundation
import SymbioticTokens
import VizDAWCore

/// What a knob controls (it takes its colour from the function).
public enum KnobFunction { case gain, pan, send }

/// The channel strip's colours, all theme tokens (plan 7b Ruling 10): fader
/// caps by strip kind, knobs by function, never the strip's own colour,
/// which only the colour bar takes.
public enum StripPalette {
    public static func cap(_ k: StripColourRow, _ t: Theme) -> ThemeColor {
        switch k {
        case .channel: t.strip.channel
        case .instrument: t.strip.instrument
        case .auxReturn: t.strip.auxReturn
        case .bus: t.strip.bus
        case .master: t.strip.master
        }
    }

    /// The input monitor's I toggle (final review m4): input in the warning
    /// state, auto (and off, unlit) in the play state; never a strip colour.
    public static func monitor(_ m: MonitorModeRow, _ t: Theme) -> ThemeColor {
        m == .input ? t.state.warning : t.state.play
    }

    // The cases are written with the type name: the theme scan reads a bare
    // member such as the red case as a system colour.
    public static func swatch(_ c: SwatchRow, _ t: Theme) -> ThemeColor {
        switch c {
        case SwatchRow.red: t.swatch.red
        case SwatchRow.orange: t.swatch.orange
        case SwatchRow.yellow: t.swatch.yellow
        case SwatchRow.green: t.swatch.green
        case SwatchRow.teal: t.swatch.teal
        case SwatchRow.blue: t.swatch.blue
        case SwatchRow.purple: t.swatch.purple
        case SwatchRow.pink: t.swatch.pink
        }
    }

    /// The eight strip colours, in the Colour menu's order.
    public static let swatches: [SwatchRow] = [
        SwatchRow.red, SwatchRow.orange, SwatchRow.yellow, SwatchRow.green,
        SwatchRow.teal, SwatchRow.blue, SwatchRow.purple, SwatchRow.pink,
    ]

    /// The Colour menu's first item: no colour of its own (the kind's).
    public static let noColourTitle = "None"

    /// A swatch's name in the Colour menu.
    public static func title(_ c: SwatchRow) -> String {
        switch c {
        case SwatchRow.red: "Red"
        case SwatchRow.orange: "Orange"
        case SwatchRow.yellow: "Yellow"
        case SwatchRow.green: "Green"
        case SwatchRow.teal: "Teal"
        case SwatchRow.blue: "Blue"
        case SwatchRow.purple: "Purple"
        case SwatchRow.pink: "Pink"
        }
    }

    /// The strip `lane` plays through (`StripFocus`'s rule); nil for an
    /// external destination or none.
    public static func laneStrip(_ lane: LaneRow, destinations: [DestRow], strips: [StripRow]) -> StripRow? {
        guard let id = StripFocus.strip(of: lane, destinations: destinations, strips: strips) else { return nil }
        return strips.first { $0.id == id }
    }

    /// A lane's colour in Arrange (owner G1 Q2): its strip's colour bar, so
    /// the lanes on one path share it; an external MIDI lane, which has no
    /// strip, its destination kind's (an instrument's); nil for a lane that
    /// plays nowhere (the theme's region colour).
    public static func lane(_ lane: LaneRow, destinations: [DestRow], strips: [StripRow], _ t: Theme) -> ThemeColor? {
        if let s = laneStrip(lane, destinations: destinations, strips: strips) { return bar(s, t) }
        if lane.midiDest != nil { return cap(.instrument, t) }
        return nil
    }

    /// The number boxes' readout look in every theme (fix round 1): the
    /// fill, the digits and the edge from the `readout` group.
    public static func numberBox(_ t: Theme) -> (fill: ThemeColor, digits: ThemeColor, edge: ThemeColor) {
        (t.readout.background, t.readout.text, t.readout.text)
    }

    /// The colour bar: the strip's own colour, else its kind's.
    public static func bar(_ s: StripRow, _ t: Theme) -> ThemeColor {
        s.colour.map { swatch($0, t) } ?? cap(s.kindColour, t)
    }

    public static func knob(_ f: KnobFunction, _ t: Theme) -> ThemeColor {
        switch f {
        case .gain: t.control.gain
        case .pan: t.control.pan
        case .send: t.control.send
        }
    }

    public static func automation(_ m: AutomationModeRow, _ t: Theme) -> ThemeColor {
        switch AutomationModes.colorName(m) {
        case "green": t.state.play
        case "yellow": t.state.solo
        case "orange": t.state.warning
        case "red": t.state.record
        case "purple": t.control.send
        default: t.state.offline
        }
    }

    /// The meter's band for a linear peak: low below −18 dBFS, mid to −6,
    /// high below 0, clip at 0 and above.
    public static func meter(_ peak: Float, _ t: Theme) -> ThemeColor {
        guard !peak.isNaN, peak > 0 else { return t.meter.low }
        let db = peak.isFinite ? 20 * log10(Double(peak)) : 1
        if db < -18 { return t.meter.low }
        if db < -6 { return t.meter.mid }
        if db < 0 { return t.meter.high }
        return t.meter.clip
    }
}

extension StripRow {
    /// The instrument (when there is one), then the EQ (when set), then the
    /// inserts: the order the strip shows and the engine processes. Every
    /// reader of a strip's slots uses it.
    public var slots: [SlotRow] {
        (instrument.map { [$0] } ?? []) + (eq.map { [$0] } ?? []) + inserts
    }
}
