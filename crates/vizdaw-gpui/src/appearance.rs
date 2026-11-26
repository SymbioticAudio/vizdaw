//! The current theme (slice 9b). One slot per UI thread holds the theme
//! every window draws in: `strip_palette::tokens()` and [`current_theme`]
//! read it, so the ~60 call sites that have no `cx` stay as they were.
//! The slot is set at launch (`AppState::set_prefs`) and by each theme
//! change ([`apply`]), which also redraws every window. GPUI draws on one
//! thread, so a thread-local is the whole story; it also keeps tests,
//! each on its own thread, apart.

use std::cell::Cell;

use gpui::App;
use vizdaw_app::theme::{self, Theme, themes};

thread_local! {
    /// The index into `themes()` of the theme drawn.
    static CURRENT: Cell<usize> = const { Cell::new(0) };
    /// Whether strips and track headers draw their colour bar
    /// (`AppearancePrefs::colour_bars`).
    static COLOUR_BARS: Cell<bool> = const { Cell::new(true) };
}

/// Whether the colour bars draw (Show channel colour bars).
pub fn colour_bars() -> bool {
    COLOUR_BARS.with(Cell::get)
}

/// Sets [`colour_bars`], without redrawing anything.
pub(crate) fn set_colour_bars(on: bool) {
    COLOUR_BARS.with(|c| c.set(on));
}

/// The theme every window draws in.
pub fn current_theme(_cx: &App) -> &'static Theme {
    current()
}

/// [`current_theme`] where there is no `cx` (token accessors).
pub fn current() -> &'static Theme {
    &themes()[CURRENT.with(Cell::get)]
}

/// The index of the current theme in `themes()`.
pub(crate) fn current_index() -> usize {
    CURRENT.with(Cell::get)
}

/// Makes `id` (Classic Light when it names no theme) the one drawn,
/// without redrawing anything.
pub(crate) fn set_current(id: &str) {
    let t = theme::theme_or_default(id);
    let i = themes().iter().position(|x| x.id == t.id).unwrap_or(0);
    CURRENT.with(|c| c.set(i));
}

/// [`set_current`], then every window redraws.
pub(crate) fn apply(id: &str, cx: &mut App) {
    set_current(id);
    cx.refresh_windows();
}

/// An sRGB colour's WCAG relative luminance (0 black to 1 white).
fn luminance(rgb: u32) -> f32 {
    let lin = |shift: u32| {
        let v = ((rgb >> shift) & 0xff) as f32 / 255.;
        if v <= 0.04045 {
            v / 12.92
        } else {
            ((v + 0.055) / 1.055).powf(2.4)
        }
    };
    0.2126 * lin(16) + 0.7152 * lin(8) + 0.0722 * lin(0)
}

/// The WCAG contrast ratio of two colours (1 to 21; Swift
/// `Theme.contrast`).
pub fn contrast(a: u32, b: u32) -> f32 {
    let (la, lb) = (luminance(a), luminance(b));
    (la.max(lb) + 0.05) / (la.min(lb) + 0.05)
}

/// `top` at `alpha` over the opaque `bottom` (Swift `Theme.composite`).
pub fn composite(top: u32, alpha: f32, bottom: u32) -> u32 {
    let ch = |shift: u32| {
        let t = ((top >> shift) & 0xff) as f32;
        let b = ((bottom >> shift) & 0xff) as f32;
        (t * alpha + b * (1. - alpha)).round() as u32
    };
    (ch(16) << 16) | (ch(8) << 8) | ch(0)
}

/// Text on a lit toggle's `fill`: the black or the white key ink,
/// whichever has the higher contrast (Swift `Theme.legend(on:)`).
pub fn legend_on(t: &Theme, fill: u32) -> u32 {
    let (dark, light) = (t.keys.black.rgb, t.keys.white.rgb);
    if contrast(dark, fill) >= contrast(light, fill) {
        dark
    } else {
        light
    }
}

/// Text on a track swatch: pure black or the white key ink, whichever
/// has the higher contrast (Swift `Theme.swatchLegend(on:)`).
fn swatch_legend_on(t: &Theme, fill: u32) -> u32 {
    let (dark, light) = (0x000000, t.keys.white.rgb);
    if contrast(dark, fill) >= contrast(light, fill) {
        dark
    } else {
        light
    }
}

/// A region name's ink: the swatch legend of `fill` (at `alpha`) as
/// drawn over the opaque `under`; where that is short of 4.5:1, pure
/// black or white, whichever reads better (Swift `Theme.regionNameInk`).
pub fn region_name_ink(t: &Theme, fill: u32, alpha: f32, under: u32) -> u32 {
    let drawn = composite(fill, alpha, under);
    let ink = swatch_legend_on(t, drawn);
    if contrast(ink, drawn) >= 4.5 {
        return ink;
    }
    if contrast(0x000000, drawn) >= contrast(0xffffff, drawn) {
        0x000000
    } else {
        0xffffff
    }
}
