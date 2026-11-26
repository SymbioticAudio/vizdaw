//! The built-in themes, parsed once from the checked-in copy of
//! symbiotic-design 0.2.3 `tokens/tokens.json` (`assets/symbiotic-tokens.json`).

use serde::{Deserialize, Deserializer};
use std::sync::OnceLock;

/// A colour from the tokens: `rgb` is 0xRRGGBB, `alpha` is 0-255 (255 where
/// the JSON gives `#RRGGBB`, the trailing byte where it gives `#RRGGBBAA`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Color {
    pub rgb: u32,
    pub alpha: u8,
}

impl Color {
    /// 0xRRGGBBAA.
    pub fn rgba(self) -> u32 {
        (self.rgb << 8) | u32::from(self.alpha)
    }
}

impl<'de> Deserialize<'de> for Color {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        use serde::de::Error;
        let s = String::deserialize(d)?;
        let hex = s
            .strip_prefix('#')
            .ok_or_else(|| D::Error::custom(format!("colour {s}")))?;
        let n = u32::from_str_radix(hex, 16).map_err(D::Error::custom)?;
        match hex.len() {
            6 => Ok(Color { rgb: n, alpha: 255 }),
            8 => Ok(Color {
                rgb: n >> 8,
                alpha: (n & 0xFF) as u8,
            }),
            _ => Err(D::Error::custom(format!("colour {s}"))),
        }
    }
}

macro_rules! group {
    ($name:ident { $($field:ident),* $(,)? }) => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
        #[serde(rename_all = "camelCase")]
        pub struct $name { $(pub $field: Color),* }
    };
}

group!(Surface {
    window,
    panel,
    raised,
    inset,
    separator,
    selection,
    focus_ring
});
group!(Text {
    primary,
    secondary,
    disabled,
    inverse
});
group!(Accent { accent, on_accent });
group!(State {
    record,
    play,
    solo,
    mute,
    warning,
    error,
    offline
});
group!(Meter {
    background,
    low,
    mid,
    high,
    clip
});
group!(Graph {
    line,
    fill,
    grid,
    handle,
    handle_selected
});
group!(Control {
    gain,
    eq_low,
    eq_low_mid,
    eq_high_mid,
    eq_high,
    send,
    pan,
    dynamics,
    filter
});
group!(Strip {
    channel,
    instrument,
    aux_return,
    bus,
    master
});
group!(Brand {
    gold,
    cyan,
    cyan_ink,
    ink,
    paper,
    rule
});
group!(Knob {
    skirt,
    cap,
    track,
    indicator
});
group!(Fader {
    well,
    scale,
    cap_line
});
group!(Relief {
    highlight,
    shadow,
    cap_top,
    cap_bottom,
    panel_top,
    panel_bottom
});
group!(Swatch {
    red,
    orange,
    yellow,
    green,
    teal,
    blue,
    purple,
    pink
});
group!(Keys { white, black });
group!(Readout { background, text });
group!(Band { light, dark });

#[derive(Debug, Clone, Copy, PartialEq, Deserialize)]
pub struct Radius {
    pub none: f32,
    pub s: f32,
    pub m: f32,
    pub l: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Appearance {
    Light,
    Dark,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Labels {
    Plain,
    Lowercase,
    Caps,
    CondensedCaps,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum LegendFamily {
    System,
    Serif,
    Technical,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum NumberFamily {
    System,
    Serif,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Corners {
    Rounded,
    Square,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PanelBands {
    Plain,
    Banded,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum KnobSkirt {
    Plain,
    Knurled,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum CapStyle {
    Outlined,
    Flush,
    Ridged,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Tile {
    Bar,
    Tile,
}

/// The theme's drawing choices (slice 9b uses only `corners`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ThemeStyles {
    pub labels: Labels,
    pub legend_family: LegendFamily,
    pub number_family: NumberFamily,
    pub corners: Corners,
    pub panel_bands: PanelBands,
    pub knob_skirt: KnobSkirt,
    pub cap_style: CapStyle,
    pub tile: Tile,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum KnobFinish {
    Flat,
    Shaded,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
struct Values {
    surface: Surface,
    text: Text,
    accent: Accent,
    state: State,
    meter: Meter,
    graph: Graph,
    control: Control,
    strip: Strip,
    brand: Brand,
    radius: Radius,
    knob: Knob,
    fader: Fader,
    relief: Relief,
    swatch: Swatch,
    keys: Keys,
    readout: Readout,
    band: Band,
}

#[derive(Deserialize)]
struct RawTheme {
    id: String,
    name: String,
    appearance: Appearance,
    #[serde(rename = "knobFinish")]
    knob_finish: KnobFinish,
    style: ThemeStyles,
    values: Values,
}

#[derive(Deserialize)]
struct RawFile {
    themes: Vec<RawTheme>,
}

/// One built-in theme: every colour group of the tokens, plus radius.
#[derive(Debug, Clone, PartialEq)]
pub struct Theme {
    pub id: String,
    pub name: String,
    pub appearance: Appearance,
    pub knob_finish: KnobFinish,
    pub styles: ThemeStyles,
    pub surface: Surface,
    pub text: Text,
    pub accent: Accent,
    pub state: State,
    pub meter: Meter,
    pub graph: Graph,
    pub control: Control,
    pub strip: Strip,
    pub brand: Brand,
    pub radius: Radius,
    pub knob: Knob,
    pub fader: Fader,
    pub relief: Relief,
    pub swatch: Swatch,
    pub keys: Keys,
    pub readout: Readout,
    pub band: Band,
}

impl From<RawTheme> for Theme {
    fn from(r: RawTheme) -> Self {
        let v = r.values;
        Theme {
            id: r.id,
            name: r.name,
            appearance: r.appearance,
            knob_finish: r.knob_finish,
            styles: r.style,
            surface: v.surface,
            text: v.text,
            accent: v.accent,
            state: v.state,
            meter: v.meter,
            graph: v.graph,
            control: v.control,
            strip: v.strip,
            brand: v.brand,
            radius: v.radius,
            knob: v.knob,
            fader: v.fader,
            relief: v.relief,
            swatch: v.swatch,
            keys: v.keys,
            readout: v.readout,
            band: v.band,
        }
    }
}

/// The id of the default theme.
pub const DEFAULT_THEME_ID: &str = "classic-light";

/// The six built-in themes, in `THEME_IDS` order, parsed once.
pub fn themes() -> &'static [Theme] {
    static TABLE: OnceLock<Vec<Theme>> = OnceLock::new();
    TABLE.get_or_init(|| {
        let file: RawFile = serde_json::from_str(include_str!("../assets/symbiotic-tokens.json"))
            .expect("assets/symbiotic-tokens.json is checked in and must parse");
        file.themes.into_iter().map(Theme::from).collect()
    })
}

/// The built-in theme with this id.
pub fn theme(id: &str) -> Option<&'static Theme> {
    themes().iter().find(|t| t.id == id)
}

/// The theme with this id, or Classic Light for an unknown one.
pub fn theme_or_default(id: &str) -> &'static Theme {
    theme(id)
        .or_else(|| theme(DEFAULT_THEME_ID))
        .expect("classic-light is in the table")
}
