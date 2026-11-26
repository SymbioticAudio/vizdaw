//! The chrome draws from the current theme (slice 9b Task 3): the bar,
//! the sheet card and the Settings window read their colours from it each
//! time they draw, and no chrome file keeps a colour literal.
//!
//! GPUI's test platform paints nothing, so the colour an element draws is
//! read from the function the render calls (`transport_bar::colors`,
//! `sheet_host::card_colours`, `settings_window::background`).

use std::sync::Arc;

use gpui::TestAppContext;
use serde_json::json;
use vizdaw_app::DocumentStore;
use vizdaw_app::testing::FakeCore;
use vizdaw_app::theme::{Theme, theme};
use vizdaw_gpui::AppState;
use vizdaw_gpui::settings_window::background;
use vizdaw_gpui::sheet_host::card_colours;
use vizdaw_gpui::transport_bar::colors;

fn use_theme(cx: &mut TestAppContext, id: &str) -> &'static Theme {
    cx.update(|cx| {
        let state = AppState::global(cx);
        state.update(cx, |s, _| {
            let mut prefs = vizdaw_app::Prefs::in_memory();
            prefs.set_raw("appearance", json!({"theme": id})).unwrap();
            s.set_prefs(prefs);
        });
    });
    theme(id).unwrap()
}

fn app(cx: &mut TestAppContext) {
    let store = DocumentStore::with_core(Arc::new(FakeCore::new()));
    cx.update(|cx| {
        vizdaw_gpui::install(store, cx);
    });
}

#[gpui::test]
fn the_transport_bar_takes_the_theme(cx: &mut TestAppContext) {
    app(cx);
    let light = use_theme(cx, "classic-light");
    let (bar, text, button) = (colors::bar(), colors::text(), colors::button());
    assert_eq!(bar, light.surface.panel.rgb);
    assert_eq!(text, light.text.primary.rgb);
    assert_eq!(button, light.surface.raised.rgb);
    let dark = use_theme(cx, "studio-dark");
    assert_eq!(colors::bar(), dark.surface.panel.rgb);
    assert_eq!(colors::text(), dark.text.primary.rgb);
    assert_ne!(colors::bar(), bar);
    assert_ne!(colors::text(), text);
    // The playing and on faces follow too.
    assert_eq!(colors::playing(), dark.state.play.rgb);
    // The readouts: value in readout.text, label a 0.72 mix.
    assert_eq!(colors::readout_text(), dark.readout.text.rgb);
    assert_eq!(
        colors::readout_label(),
        colors::mix(dark.readout.background.rgb, dark.readout.text.rgb, 0.72)
    );
    let eg13 = use_theme(cx, "eg13");
    assert_eq!(colors::readout_text(), eg13.readout.text.rgb);
    assert_ne!(colors::readout_text(), colors::text());
    use_theme(cx, "studio-dark");
    assert_eq!(colors::on(), colors::on_of(dark));
    assert_ne!(colors::on_of(dark), colors::on_of(light));
}

#[gpui::test]
fn a_sheet_card_takes_the_theme(cx: &mut TestAppContext) {
    app(cx);
    let light = use_theme(cx, "classic-light");
    let a = card_colours();
    assert_eq!(a.background, light.surface.window.rgb);
    assert_eq!(a.text, light.text.primary.rgb);
    assert_eq!(a.default_button, light.accent.accent.rgb);
    assert_eq!(a.default_button_text, light.accent.on_accent.rgb);
    assert_eq!(a.destructive_text, light.state.error.rgb);
    let dark = use_theme(cx, "studio-dark");
    let b = card_colours();
    assert_eq!(b.background, dark.surface.window.rgb);
    assert_ne!(a.background, b.background);
    assert_ne!(a.text, b.text);
}

#[gpui::test]
fn the_settings_window_takes_the_theme(cx: &mut TestAppContext) {
    app(cx);
    let light = use_theme(cx, "classic-light");
    assert_eq!(background(), light.surface.window.rgb);
    let dark = use_theme(cx, "studio-dark");
    assert_eq!(background(), dark.surface.window.rgb);
    assert_ne!(light.surface.window.rgb, dark.surface.window.rgb);
}

/// A hex colour, `rgb(0x…)`, `rgba(0x…)`, `hsla(…)`, `black()` or
/// `white()` in a chrome file would stay the same in every theme.
#[test]
fn no_chrome_file_keeps_a_colour_literal() {
    let src = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut files: Vec<std::path::PathBuf> = [
        "transport_bar",
        "transport_fields",
        "record_popover",
        "menu_bar",
        "banners",
        "sheet_host",
        "text_input",
        "format_panel",
        "shell",
        "settings_window",
    ]
    .iter()
    .map(|n| src.join(format!("{n}.rs")))
    .collect();
    for dir in ["sheets", "controls"] {
        for e in std::fs::read_dir(src.join(dir)).unwrap() {
            files.push(e.unwrap().path());
        }
    }
    let mut hits = Vec::new();
    for f in files {
        let text = std::fs::read_to_string(&f).unwrap();
        for (n, line) in text.lines().enumerate() {
            let code = line.split("//").next().unwrap();
            // The one allowed struct literal: the "on" fill built from the
            // accent token's channels (alpha applied to a token).
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
            let named = ["blue", "red", "green", "yellow", "white"].iter().any(|n| {
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
                || code.contains("white()")
            {
                hits.push(format!("{}:{}: {}", f.display(), n + 1, line.trim()));
            }
        }
    }
    assert!(hits.is_empty(), "colour literals: {hits:#?}");
}
