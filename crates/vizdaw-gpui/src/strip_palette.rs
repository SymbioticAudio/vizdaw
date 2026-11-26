//! The channel strip's colours as named tokens (Swift `StripPalette` over
//! the theme's `strip`, `meter`, `state`, `control`, `fader`, `knob`,
//! `readout`, `surface`, `text` and `swatch` groups). The values are the
//! Classic Light theme's (symbiotic-design `tokens/tokens.json`); the
//! themes slice swaps the values, never the names. Fader caps take their
//! strip kind's colour and knobs their function's, never the strip's own
//! colour, which only the colour bar takes.

use gpui::{Rgba, rgb};
use vizdaw_app::theme::{Theme, themes};
use vizdaw_ffi::automation::AutomationModeRow;
use vizdaw_ffi::edit::MonitorModeRow;
use vizdaw_ffi::mixer::{StripColourRow, StripRow, SwatchRow};

/// One theme's strip colours (0xRRGGBB).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct StripTokens {
    // Fader caps by strip kind (`strip.*`).
    pub strip_channel: u32,
    pub strip_instrument: u32,
    pub strip_aux_return: u32,
    pub strip_bus: u32,
    pub strip_master: u32,
    // Meter bands (`meter.*`).
    pub meter_background: u32,
    pub meter_low: u32,
    pub meter_mid: u32,
    pub meter_high: u32,
    pub meter_clip: u32,
    // States (`state.*`).
    pub state_record: u32,
    pub state_play: u32,
    pub state_solo: u32,
    pub state_mute: u32,
    pub state_warning: u32,
    pub state_error: u32,
    pub state_offline: u32,
    // Knob arcs by function (`control.*`).
    pub control_gain: u32,
    pub control_pan: u32,
    pub control_send: u32,
    // The fader (`fader.*`).
    pub fader_well: u32,
    pub fader_scale: u32,
    pub fader_cap_line: u32,
    // The knob (`knob.*`).
    pub knob_skirt: u32,
    pub knob_cap: u32,
    pub knob_track: u32,
    pub knob_indicator: u32,
    // Number boxes (`readout.*`, `surface.raised` while editing).
    pub readout_background: u32,
    pub readout_text: u32,
    // Surfaces and text.
    pub surface_panel: u32,
    pub surface_raised: u32,
    pub surface_inset: u32,
    pub surface_separator: u32,
    pub focus_ring: u32,
    pub accent: u32,
    pub text_primary: u32,
    pub text_secondary: u32,
    pub text_inverse: u32,
    // The eight strip colours (`swatch.*`).
    pub swatch_red: u32,
    pub swatch_orange: u32,
    pub swatch_yellow: u32,
    pub swatch_green: u32,
    pub swatch_teal: u32,
    pub swatch_blue: u32,
    pub swatch_purple: u32,
    pub swatch_pink: u32,
    /// Corner radius of caps, wells, boxes and toggles (`radius.s`).
    pub radius: f32,
}

/// Classic Light, the default theme, as written by hand before the theme
/// table (a test holds `from_theme` to it).
pub const CLASSIC_LIGHT: StripTokens = StripTokens {
    strip_channel: 0x8E8E8E,
    strip_instrument: 0x3FA34A,
    strip_aux_return: 0x4A7AD0,
    strip_bus: 0xC99400,
    strip_master: 0xD04040,
    meter_background: 0x1A1A1A,
    meter_low: 0x10C030,
    meter_mid: 0xF0D800,
    meter_high: 0xFF8A00,
    meter_clip: 0xFF2A1A,
    state_record: 0xD81010,
    state_play: 0x00860A,
    state_solo: 0xF2CC00,
    state_mute: 0xEE7A00,
    state_warning: 0x9A5200,
    state_error: 0xB80000,
    state_offline: 0x7A7A7A,
    control_gain: 0x5E5E5E,
    control_pan: 0x0070B8,
    control_send: 0x7A2FBF,
    fader_well: 0x2E2E2E,
    fader_scale: 0x2A2A2A,
    fader_cap_line: 0x000000,
    knob_skirt: 0xB8B8B8,
    knob_cap: 0xF4F4F4,
    knob_track: 0x8E8E8E,
    knob_indicator: 0x000000,
    readout_background: 0xFFFFFF,
    readout_text: 0x000000,
    surface_panel: 0xDDDDDD,
    surface_raised: 0xEEEEEE,
    surface_inset: 0xFFFFFF,
    surface_separator: 0x7A7A7A,
    focus_ring: 0x1F4FBF,
    accent: 0x1F4FBF,
    text_primary: 0x000000,
    text_secondary: 0x3A3A3A,
    text_inverse: 0xFFFFFF,
    swatch_red: 0xC8201C,
    swatch_orange: 0xC25A00,
    swatch_yellow: 0x8F7400,
    swatch_green: 0x25822E,
    swatch_teal: 0x00807E,
    swatch_blue: 0x1F5FD0,
    swatch_purple: 0x7A35C2,
    swatch_pink: 0xC0287A,
    radius: 3.,
};

/// The strip tokens of the six themes, in `themes()` order.
fn all_tokens() -> &'static [StripTokens] {
    static TABLE: std::sync::OnceLock<Vec<StripTokens>> = std::sync::OnceLock::new();
    TABLE.get_or_init(|| themes().iter().map(StripTokens::from_theme).collect())
}

/// The tokens of the theme the windows draw in now (see
/// [`crate::appearance`]).
pub fn tokens() -> &'static StripTokens {
    &all_tokens()[crate::appearance::current_index()]
}

/// What a knob controls: it takes its colour from the function.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KnobFunction {
    Gain,
    Pan,
    Send,
}

impl StripTokens {
    /// The tokens `t` gives the strips.
    pub fn from_theme(t: &Theme) -> StripTokens {
        StripTokens {
            strip_channel: t.strip.channel.rgb,
            strip_instrument: t.strip.instrument.rgb,
            strip_aux_return: t.strip.aux_return.rgb,
            strip_bus: t.strip.bus.rgb,
            strip_master: t.strip.master.rgb,
            meter_background: t.meter.background.rgb,
            meter_low: t.meter.low.rgb,
            meter_mid: t.meter.mid.rgb,
            meter_high: t.meter.high.rgb,
            meter_clip: t.meter.clip.rgb,
            state_record: t.state.record.rgb,
            state_play: t.state.play.rgb,
            state_solo: t.state.solo.rgb,
            state_mute: t.state.mute.rgb,
            state_warning: t.state.warning.rgb,
            state_error: t.state.error.rgb,
            state_offline: t.state.offline.rgb,
            control_gain: t.control.gain.rgb,
            control_pan: t.control.pan.rgb,
            control_send: t.control.send.rgb,
            fader_well: t.fader.well.rgb,
            fader_scale: t.fader.scale.rgb,
            fader_cap_line: t.fader.cap_line.rgb,
            knob_skirt: t.knob.skirt.rgb,
            knob_cap: t.knob.cap.rgb,
            knob_track: t.knob.track.rgb,
            knob_indicator: t.knob.indicator.rgb,
            readout_background: t.readout.background.rgb,
            readout_text: t.readout.text.rgb,
            surface_panel: t.surface.panel.rgb,
            surface_raised: t.surface.raised.rgb,
            surface_inset: t.surface.inset.rgb,
            surface_separator: t.surface.separator.rgb,
            focus_ring: t.surface.focus_ring.rgb,
            accent: t.accent.accent.rgb,
            text_primary: t.text.primary.rgb,
            text_secondary: t.text.secondary.rgb,
            text_inverse: t.text.inverse.rgb,
            swatch_red: t.swatch.red.rgb,
            swatch_orange: t.swatch.orange.rgb,
            swatch_yellow: t.swatch.yellow.rgb,
            swatch_green: t.swatch.green.rgb,
            swatch_teal: t.swatch.teal.rgb,
            swatch_blue: t.swatch.blue.rgb,
            swatch_purple: t.swatch.purple.rgb,
            swatch_pink: t.swatch.pink.rgb,
            radius: t.radius.s,
        }
    }

    /// A fader cap's colour: its strip kind's.
    pub fn cap(&self, k: StripColourRow) -> Rgba {
        rgb(match k {
            StripColourRow::Channel => self.strip_channel,
            StripColourRow::Instrument => self.strip_instrument,
            StripColourRow::AuxReturn => self.strip_aux_return,
            StripColourRow::Bus => self.strip_bus,
            StripColourRow::Master => self.strip_master,
        })
    }

    pub fn swatch(&self, c: SwatchRow) -> Rgba {
        rgb(match c {
            SwatchRow::Red => self.swatch_red,
            SwatchRow::Orange => self.swatch_orange,
            SwatchRow::Yellow => self.swatch_yellow,
            SwatchRow::Green => self.swatch_green,
            SwatchRow::Teal => self.swatch_teal,
            SwatchRow::Blue => self.swatch_blue,
            SwatchRow::Purple => self.swatch_purple,
            SwatchRow::Pink => self.swatch_pink,
        })
    }

    /// The colour bar: the strip's own colour, else the accent for a
    /// Co-Producer strip, else its kind's.
    pub fn bar(&self, s: &StripRow) -> Rgba {
        if let Some(c) = s.colour {
            return self.swatch(c);
        }
        if s.co_producer {
            return rgb(self.accent);
        }
        self.cap(s.kind_colour)
    }

    pub fn knob(&self, f: KnobFunction) -> Rgba {
        rgb(match f {
            KnobFunction::Gain => self.control_gain,
            KnobFunction::Pan => self.control_pan,
            KnobFunction::Send => self.control_send,
        })
    }

    /// The input monitor's I toggle: input in the warning colour, auto
    /// (and off, unlit) in the play colour.
    pub fn monitor(&self, m: MonitorModeRow) -> Rgba {
        rgb(if m == MonitorModeRow::Input {
            self.state_warning
        } else {
            self.state_play
        })
    }

    /// The automation mode's dot: Read green, Touch yellow, Latch
    /// orange, Write red, Trim purple, Off grey.
    pub fn automation(&self, m: AutomationModeRow) -> Rgba {
        rgb(match m {
            AutomationModeRow::Off => self.state_offline,
            AutomationModeRow::Read => self.state_play,
            AutomationModeRow::Touch => self.state_solo,
            AutomationModeRow::Latch => self.state_warning,
            AutomationModeRow::Write => self.state_record,
            AutomationModeRow::Trim => self.control_send,
        })
    }

    /// The meter's band for a linear peak: low below −18 dBFS, mid to −6,
    /// high below 0, clip at 0 and above.
    pub fn meter(&self, peak: f32) -> Rgba {
        rgb(match meter_band(peak) {
            MeterBand::Low => self.meter_low,
            MeterBand::Mid => self.meter_mid,
            MeterBand::High => self.meter_high,
            MeterBand::Clip => self.meter_clip,
        })
    }

    /// The legend on a lit toggle: dark text on a light fill, light on a
    /// dark one.
    pub fn legend_on(&self, fill: u32) -> Rgba {
        let c = rgb(fill);
        let luma = 0.2126 * c.r + 0.7152 * c.g + 0.0722 * c.b;
        rgb(if luma > 0.5 {
            self.text_primary
        } else {
            self.text_inverse
        })
    }
}

/// A meter band.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MeterBand {
    Low,
    Mid,
    High,
    Clip,
}

/// The band of a linear peak (Swift `StripPalette.meter`).
pub fn meter_band(peak: f32) -> MeterBand {
    if peak.is_nan() || peak <= 0.0 {
        return MeterBand::Low;
    }
    let db = if peak.is_finite() {
        20.0 * f64::from(peak).log10()
    } else {
        1.0
    };
    if db < -18.0 {
        MeterBand::Low
    } else if db < -6.0 {
        MeterBand::Mid
    } else if db < 0.0 {
        MeterBand::High
    } else {
        MeterBand::Clip
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn meter_bands_follow_the_swift_thresholds() {
        let at = |db: f64| 10f64.powf(db / 20.0) as f32;
        assert_eq!(meter_band(0.0), MeterBand::Low);
        assert_eq!(meter_band(f32::NAN), MeterBand::Low);
        assert_eq!(meter_band(at(-18.1)), MeterBand::Low);
        assert_eq!(meter_band(at(-17.9)), MeterBand::Mid);
        assert_eq!(meter_band(at(-5.9)), MeterBand::High);
        assert_eq!(meter_band(1.0), MeterBand::Clip);
        assert_eq!(meter_band(f32::INFINITY), MeterBand::Clip);
    }

    #[test]
    fn lit_legends_contrast_with_their_fill() {
        let t = tokens();
        assert_eq!(t.legend_on(t.state_solo), rgb(t.text_primary));
        assert_eq!(t.legend_on(t.state_record), rgb(t.text_inverse));
    }
}
