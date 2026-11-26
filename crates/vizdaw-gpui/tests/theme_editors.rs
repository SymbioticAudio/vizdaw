//! The editors, panes, headers and plug-in windows draw from the current
//! theme (slice 9b Task 4). GPUI's test platform paints nothing, so each
//! test reads the colour from the function the render calls.

use std::sync::Arc;

use gpui::TestAppContext;
use serde_json::json;
use vizdaw_app::DocumentStore;
use vizdaw_app::testing::{FakeCore, lane_row, strip_row};
use vizdaw_app::theme::{Theme, theme};
use vizdaw_ffi::mixer::{StripColourRow, StripKindRow, SwatchRow};
use vizdaw_gpui::AppState;
use vizdaw_gpui::appearance::{colour_bars, contrast, region_name_ink};
use vizdaw_gpui::arrangement::colors;
use vizdaw_gpui::plugin_windows::window_colours;
use vizdaw_gpui::strip_palette::tokens;
use vizdaw_gpui::track_headers::lane_bars;

fn use_prefs(cx: &mut TestAppContext, prefs_json: serde_json::Value) {
    cx.update(|cx| {
        let state = AppState::global(cx);
        state.update(cx, |s, _| {
            let mut prefs = vizdaw_app::Prefs::in_memory();
            prefs.set_raw("appearance", prefs_json).unwrap();
            s.set_prefs(prefs);
        });
    });
}

fn use_theme(cx: &mut TestAppContext, id: &str) -> &'static Theme {
    use_prefs(cx, json!({"theme": id}));
    theme(id).unwrap()
}

fn app(cx: &mut TestAppContext) {
    let store = DocumentStore::with_core(Arc::new(FakeCore::new()));
    cx.update(|cx| {
        vizdaw_gpui::install(store, cx);
    });
}

#[gpui::test]
fn the_arrangement_takes_the_theme(cx: &mut TestAppContext) {
    app(cx);
    let light = use_theme(cx, "classic-light");
    assert_eq!(colors::canvas(), light.surface.inset.rgb);
    assert_eq!(colors::ruler(), light.surface.panel.rgb);
    assert_eq!(colors::separator(), light.surface.separator.rgb);
    assert_eq!(colors::label(), light.text.primary.rgb);
    assert_eq!(colors::tick(), light.text.secondary.rgb);
    assert_eq!(colors::playhead(), light.state.record.rgb);
    assert_eq!(colors::selection(), light.surface.selection.rgb);
    assert_eq!(colors::region_line(), light.graph.line.rgb);
    assert_eq!(colors::region_selected(), light.graph.handle_selected.rgb);
    assert_eq!(colors::reference(), light.swatch.purple.rgb);
    assert_eq!(colors::offline(), light.state.offline.rgb);
    // The grid is graph.grid with its own alpha; beats a 0.35 of it.
    assert_eq!(colors::bar_line(), colors::paint(light.graph.grid));
    assert_eq!(
        colors::bar_line().a,
        f32::from(light.graph.grid.alpha) / 255.
    );
    assert!((colors::beat_line().a - colors::bar_line().a * 0.35).abs() < 1e-4);
    // A region's fill is graph.fill at its own alpha (opaque here).
    assert_eq!(colors::region_fill(), colors::paint(light.graph.fill));
    assert_eq!(colors::region_fill().a, 1.0);
    let dark = use_theme(cx, "studio-dark");
    assert_eq!(colors::canvas(), dark.surface.inset.rgb);
    assert_ne!(colors::canvas(), light.surface.inset.rgb);
    assert_eq!(colors::bar_line(), colors::paint(dark.graph.grid));
    assert_eq!(colors::region_fill(), colors::paint(dark.graph.fill));
    assert!(
        colors::region_fill().a < 0.5,
        "Studio Dark's fill is translucent"
    );
    assert_ne!(colors::region_fill(), colors::paint(light.graph.fill));
    // An unknown saved id draws Classic Light.
    use_prefs(cx, json!({"theme": "no-such-theme"}));
    assert_eq!(colors::canvas(), light.surface.inset.rgb);
}

#[gpui::test]
fn the_piano_roll_keys_take_the_theme(cx: &mut TestAppContext) {
    app(cx);
    for id in ["classic-light", "studio-dark", "console", "eg13"] {
        let t = use_theme(cx, id);
        assert_eq!(colors::white_key(), t.keys.white.rgb, "{id}");
        assert_eq!(colors::black_key(), t.keys.black.rgb, "{id}");
        assert_eq!(colors::key_label(), t.keys.black.rgb, "{id}");
        // The octave label is drawn on a white key: the contrast pair the
        // theme allows (keys.black on keys.white).
        assert!(
            contrast(colors::key_label(), colors::white_key()) >= 4.5,
            "{id}"
        );
        // The notes' track colours are the swatches, by index modulo 8.
        assert_eq!(colors::swatch(0), t.swatch.red.rgb, "{id}");
        assert_eq!(colors::swatch(5), t.swatch.blue.rgb, "{id}");
        assert_eq!(colors::swatch(8), t.swatch.red.rgb, "{id}");
    }
    let a = use_theme(cx, "classic-light");
    let b = use_theme(cx, "studio-dark");
    assert_ne!(a.keys.white.rgb, b.keys.white.rgb);
    assert_eq!(colors::white_key(), b.keys.white.rgb);
}

#[gpui::test]
fn region_names_read_on_the_regions_fill(cx: &mut TestAppContext) {
    app(cx);
    for id in [
        "classic-light",
        "studio-dark",
        "flat",
        "console",
        "symbiotic",
        "eg13",
    ] {
        let t = use_theme(cx, id);
        let under = t.surface.inset.rgb;
        let ink = region_name_ink(
            t,
            t.graph.fill.rgb,
            f32::from(t.graph.fill.alpha) / 255.,
            under,
        );
        let drawn = vizdaw_gpui::appearance::composite(
            t.graph.fill.rgb,
            f32::from(t.graph.fill.alpha) / 255.,
            under,
        );
        assert!(
            contrast(ink, drawn) >= 4.5,
            "{id}: {ink:06x} on {drawn:06x}"
        );
    }
}

#[gpui::test]
fn colour_bars_off_hides_the_bars(cx: &mut TestAppContext) {
    app(cx);
    let lanes = vec![lane_row(1, "Kick", false), {
        let mut l = lane_row(2, "Pad", false);
        l.audio_out = Some(7);
        l
    }];
    let mut red = strip_row(10, "Pad", StripKindRow::AudioPath { path: 7 });
    red.colour = Some(SwatchRow::Red);
    let strips = vec![red];
    // On (the default): a track with a strip draws the strip's bar.
    use_prefs(cx, json!({"theme": "classic-light"}));
    assert!(colour_bars());
    let bars = lane_bars(&lanes, &[], &strips, colour_bars());
    assert_eq!(bars[0], None, "a track that plays nowhere has no bar");
    let bar = bars[1].expect("the Pad has a bar");
    let t = theme("classic-light").unwrap();
    assert_eq!(tokens().resolve(bar), gpui::rgb(t.swatch.red.rgb));
    let dark = use_theme(cx, "studio-dark");
    assert_eq!(tokens().resolve(bar), gpui::rgb(dark.swatch.red.rgb));
    // A track on an external-less MIDI destination takes the instrument cap.
    let mut midi = lane_row(3, "Keys", false);
    midi.midi_dest = Some(4);
    let b = lane_bars(&[midi], &[], &strips, true)[0].expect("midi dest");
    assert_eq!(
        tokens().resolve(b),
        tokens().cap(StripColourRow::Instrument)
    );
    // Off: no track has a bar, and the strips' flag is off.
    use_prefs(cx, json!({"theme": "classic-light", "colourBars": false}));
    assert!(!colour_bars());
    assert!(
        lane_bars(&lanes, &[], &strips, colour_bars())
            .iter()
            .all(Option::is_none)
    );
    // The theme choice kept it.
    use_theme(cx, "studio-dark");
    assert!(colour_bars());
}

#[gpui::test]
fn a_plugin_window_header_takes_the_theme(cx: &mut TestAppContext) {
    app(cx);
    let light = use_theme(cx, "classic-light");
    let c = window_colours();
    assert_eq!(c.background, light.surface.window.rgb);
    assert_eq!(c.header, light.surface.panel.rgb);
    assert_eq!(c.divider, light.surface.separator.rgb);
    assert_eq!(c.text, light.text.primary.rgb);
    assert_eq!(c.dim, light.text.secondary.rgb);
    assert_eq!(c.button, light.surface.raised.rgb);
    assert_eq!(c.fill, light.accent.accent.rgb);
    let dark = use_theme(cx, "studio-dark");
    let d = window_colours();
    assert_eq!(d.background, dark.surface.window.rgb);
    assert_ne!(c.background, d.background);
    assert_ne!(c.text, d.text);
}

/// Every file of `src` is scanned; no colour literal stays. The allow-list,
/// each with its reason: `strip_palette.rs` holds the hand-written Classic
/// Light table a test holds to the theme; `appearance.rs` is the theme
/// accessor, whose pure black and white are Swift's `swatchLegend` and
/// `regionNameInk` inks; and the transport bar's "on" fill is built from
/// the accent token's channels (alpha applied to a token).
#[test]
fn no_source_file_keeps_a_colour_literal() {
    let src = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut files = Vec::new();
    let mut dirs = vec![src];
    while let Some(d) = dirs.pop() {
        for e in std::fs::read_dir(d).unwrap() {
            let p = e.unwrap().path();
            if p.is_dir() {
                dirs.push(p);
            } else if p.extension().is_some_and(|x| x == "rs") {
                files.push(p);
            }
        }
    }
    files.retain(|f| !f.ends_with("strip_palette.rs") && !f.ends_with("appearance.rs"));
    let mut hits = Vec::new();
    for f in files {
        let text = std::fs::read_to_string(&f).unwrap();
        for (n, line) in text.lines().enumerate() {
            let code = line.split("//").next().unwrap();
            if f.ends_with("transport_bar.rs") && code.contains("Rgba { r, g, b, a: 0.18 }") {
                continue;
            }
            let hex = code.match_indices("0x").any(|(i, _)| {
                code[i + 2..]
                    .chars()
                    .take_while(|c| c.is_ascii_hexdigit() || *c == '_')
                    .count()
                    >= 6
            });
            let named = ["blue", "red", "green", "yellow", "white", "black"]
                .iter()
                .any(|n| {
                    code.match_indices(&format!("{n}()")).any(|(i, _)| {
                        !code[..i]
                            .chars()
                            .next_back()
                            .is_some_and(|c| c.is_alphanumeric() || c == '_')
                    })
                });
            if hex
                || named
                || (code.contains("Rgba {") && !code.contains("-> Rgba {"))
                || (code.contains("Hsla {") && !code.contains("-> Hsla {"))
                || code.contains("hsla(")
                || code.replace("transparent_black()", "").contains("black()")
            {
                hits.push(format!("{}:{}: {}", f.display(), n + 1, line.trim()));
            }
        }
    }
    assert!(hits.is_empty(), "colour literals: {hits:#?}");
}
