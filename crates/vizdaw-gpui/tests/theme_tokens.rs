//! The strip tokens are the current theme's (slice 9b Task 2).

use vizdaw_app::theme::theme;
use vizdaw_gpui::strip_palette::{CLASSIC_LIGHT, StripTokens};

/// The strip palette Task 1 of slice 8c wrote by hand is Classic Light's.
#[test]
fn classic_light_matches_the_strip_palette() {
    let t = StripTokens::from_theme(theme("classic-light").unwrap());
    assert_eq!(t, CLASSIC_LIGHT);
}

#[test]
fn every_theme_gives_its_own_tokens() {
    let light = StripTokens::from_theme(theme("classic-light").unwrap());
    let dark = StripTokens::from_theme(theme("studio-dark").unwrap());
    assert_ne!(light.surface_panel, dark.surface_panel);
    assert_eq!(dark.accent, theme("studio-dark").unwrap().accent.accent.rgb);
}
