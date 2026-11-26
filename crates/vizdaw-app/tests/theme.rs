use core_control::registry::THEME_IDS;
use vizdaw_app::theme::{Appearance, Corners, theme, theme_or_default, themes};

#[test]
fn the_table_is_the_channels_six_in_order() {
    let ids: Vec<&str> = themes().iter().map(|t| t.id.as_str()).collect();
    assert_eq!(ids, THEME_IDS);
    let names: Vec<&str> = themes().iter().map(|t| t.name.as_str()).collect();
    assert_eq!(
        names,
        [
            "Classic Light",
            "Studio Dark",
            "Flat",
            "Console",
            "Symbiotic",
            "EG13"
        ]
    );
}

#[test]
fn every_theme_defines_every_token() {
    let json: serde_json::Value =
        serde_json::from_str(include_str!("../assets/symbiotic-tokens.json")).unwrap();
    let arr = json["themes"].as_array().unwrap();
    for (t, j) in themes().iter().zip(arr) {
        let v = &j["values"];
        let hex = |g: &str, k: &str| -> (u32, u8) {
            let s = v[g][k].as_str().unwrap_or_else(|| panic!("{g}.{k}"));
            let s = &s[1..];
            let n = u32::from_str_radix(s, 16).unwrap();
            if s.len() == 8 {
                (n >> 8, (n & 0xFF) as u8)
            } else {
                (n, 255)
            }
        };
        let eq = |c: vizdaw_app::theme::Color, g: &str, k: &str| {
            let (rgb, a) = hex(g, k);
            assert_eq!((c.rgb, c.alpha), (rgb, a), "{} {g}.{k}", t.id);
        };
        eq(t.surface.window, "surface", "window");
        eq(t.surface.panel, "surface", "panel");
        eq(t.surface.raised, "surface", "raised");
        eq(t.surface.inset, "surface", "inset");
        eq(t.surface.separator, "surface", "separator");
        eq(t.surface.selection, "surface", "selection");
        eq(t.surface.focus_ring, "surface", "focusRing");
        eq(t.text.primary, "text", "primary");
        eq(t.text.inverse, "text", "inverse");
        eq(t.accent.accent, "accent", "accent");
        eq(t.accent.on_accent, "accent", "onAccent");
        eq(t.state.record, "state", "record");
        eq(t.state.offline, "state", "offline");
        eq(t.meter.clip, "meter", "clip");
        eq(t.graph.grid, "graph", "grid");
        eq(t.graph.handle_selected, "graph", "handleSelected");
        eq(t.control.eq_high_mid, "control", "eqHighMid");
        eq(t.control.filter, "control", "filter");
        eq(t.strip.aux_return, "strip", "auxReturn");
        eq(t.strip.master, "strip", "master");
        eq(t.brand.cyan_ink, "brand", "cyanInk");
        eq(t.knob.indicator, "knob", "indicator");
        eq(t.fader.cap_line, "fader", "capLine");
        eq(t.relief.panel_bottom, "relief", "panelBottom");
        eq(t.relief.cap_top, "relief", "capTop");
        eq(t.swatch.pink, "swatch", "pink");
        eq(t.swatch.teal, "swatch", "teal");
        eq(t.keys.white, "keys", "white");
        eq(t.keys.black, "keys", "black");
        eq(t.readout.background, "readout", "background");
        eq(t.band.dark, "band", "dark");
        assert_eq!(t.radius.m, v["radius"]["m"].as_f64().unwrap() as f32);
        assert_eq!(t.radius.l, v["radius"]["l"].as_f64().unwrap() as f32);
    }
    // Spot checks against known values.
    let c = theme("classic-light").unwrap();
    assert_eq!(c.surface.window.rgb, 0xCFCFCF);
    assert_eq!(c.graph.grid.rgb, 0x000000);
    assert_eq!(c.graph.grid.alpha, 0x30);
    assert_eq!(c.appearance, Appearance::Light);
    assert_eq!(theme("studio-dark").unwrap().appearance, Appearance::Dark);
    assert_eq!(c.styles.corners, Corners::Rounded);
    assert_eq!(theme("flat").unwrap().styles.corners, Corners::Square);
    assert_eq!(theme("flat").unwrap().radius.m, 0.0);
    assert_eq!(c.radius.l, 10.0);
}

#[test]
fn theme_names_keep_the_clean_room() {
    let banned = [
        "svp",
        "studio vision",
        "opcode",
        "logic",
        "apple",
        "midas",
        "soundcraft",
        "steinberg",
        "cubase",
        "metro",
        "zune",
        "microsoft",
    ];
    for t in themes() {
        let n = t.name.to_lowercase();
        assert!(!banned.iter().any(|b| n.contains(b)), "{}", t.name);
    }
}

#[test]
fn an_unknown_id_falls_back_to_classic_light() {
    assert!(theme("svp-light").is_none());
    assert!(theme("").is_none());
    assert!(theme("user.mine").is_none());
    assert_eq!(theme_or_default("neon").id, "classic-light");
    assert_eq!(theme_or_default("flat").id, "flat");
}
