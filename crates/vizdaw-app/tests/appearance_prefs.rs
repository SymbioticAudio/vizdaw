//! Settings > Appearance's preference and `ui.theme` (Swift
//! `AppearanceTests`).

use core_control::ErrorCode;
use core_control::registry::{ArgValue, THEME_IDS};
use serde_json::json;
use vizdaw_app::Prefs;
use vizdaw_app::appearance_prefs::{APPEARANCE_KEY, AppearancePrefs, run_ui_theme};
use vizdaw_app::commands::APP_COMMANDS;

fn text(v: &str) -> Vec<(String, ArgValue)> {
    vec![("theme".into(), ArgValue::Text(v.into()))]
}

#[test]
fn default_is_classic_light() {
    let a = AppearancePrefs::load(&Prefs::in_memory());
    assert_eq!(a.theme, "classic-light");
    assert_eq!(a.resolved().name, "Classic Light");
    assert!(a.colour_bars);
}

#[test]
fn choose_saves() {
    let mut p = Prefs::in_memory();
    let mut a = AppearancePrefs::load(&p);
    a.set_theme("console", &mut p).unwrap();
    assert_eq!(a.resolved().id, "console");
    let again = AppearancePrefs::load(&p);
    assert_eq!(again.theme, "console");
    assert!(again.colour_bars);
    assert!(a.problem.is_none());
}

#[test]
fn unknown_saved_theme_falls_back_and_is_kept() {
    let mut p = Prefs::in_memory();
    p.set_raw(
        APPEARANCE_KEY,
        json!({"theme": "neon", "colourBars": false}),
    )
    .unwrap();
    let a = AppearancePrefs::load(&p);
    assert_eq!(a.theme, "neon");
    assert_eq!(a.resolved().id, "classic-light");
    assert!(!a.colour_bars, "a readable field is kept");
    assert_eq!(
        p.raw(APPEARANCE_KEY),
        Some(&json!({"theme": "neon", "colourBars": false})),
        "not rewritten"
    );
    // A damaged value: all defaults; a damaged field: only it.
    p.set_raw(APPEARANCE_KEY, json!("junk")).unwrap();
    let a = AppearancePrefs::load(&p);
    assert!(a.colour_bars && a.theme == "classic-light");
    p.set_raw(APPEARANCE_KEY, json!({"theme": 7, "colourBars": false}))
        .unwrap();
    let a = AppearancePrefs::load(&p);
    assert!(!a.colour_bars && a.theme == "classic-light");
}

#[test]
fn choose_refuses_unknown() {
    let mut p = Prefs::in_memory();
    let mut a = AppearancePrefs::load(&p);
    for id in ["svp-light", "", "user.nope"] {
        assert!(a.set_theme(id, &mut p).is_err(), "{id}");
    }
    assert_eq!(a.theme, "classic-light");
    assert!(p.raw(APPEARANCE_KEY).is_none());
}

#[test]
fn ui_theme_arg_checks() {
    let mut p = Prefs::in_memory();
    let mut a = AppearancePrefs::load(&p);
    let cases: Vec<(&str, Vec<(String, ArgValue)>)> = vec![
        ("none", vec![]),
        ("unknown id", text("svp-light")),
        (
            "extra",
            vec![
                ("theme".into(), ArgValue::Text("flat".into())),
                ("other".into(), ArgValue::Flag(true)),
            ],
        ),
        ("int", vec![("theme".into(), ArgValue::Int(7))]),
        ("flag", vec![("theme".into(), ArgValue::Flag(true))]),
    ];
    for (what, args) in cases {
        let e = run_ui_theme(&args, &mut a, &mut p).unwrap_err();
        assert_eq!(e.code, ErrorCode::InvalidParams, "{what}");
    }
    let e = run_ui_theme(&text("user.nope"), &mut a, &mut p).unwrap_err();
    assert_eq!(e.message, "user.nope is not a theme.");
    let e = run_ui_theme(&[], &mut a, &mut p).unwrap_err();
    assert_eq!(e.message, "ui.theme: `theme` names the theme");
    assert_eq!(a.theme, "classic-light");
    assert!(p.raw(APPEARANCE_KEY).is_none());
}

#[test]
fn ui_theme_status_needs_no_document() {
    let mut p = Prefs::in_memory();
    let mut a = AppearancePrefs::load(&p);
    let ran = run_ui_theme(&text("flat"), &mut a, &mut p).unwrap();
    assert_eq!(ran.status.as_deref(), Some("theme flat"));
    assert_eq!(a.theme, "flat");
    assert!(APP_COMMANDS.contains(&"ui.theme"));
}

#[test]
fn colour_bars_saved() {
    let mut p = Prefs::in_memory();
    let mut a = AppearancePrefs::load(&p);
    a.set_colour_bars(false, &mut p);
    assert!(!a.colour_bars);
    let again = AppearancePrefs::load(&p);
    assert!(!again.colour_bars && again.theme == "classic-light");
}

#[test]
fn every_built_in_choosable() {
    let mut p = Prefs::in_memory();
    let mut a = AppearancePrefs::load(&p);
    for id in THEME_IDS {
        a.set_theme(id, &mut p).unwrap();
        assert_eq!(a.resolved().id, *id);
    }
}

#[test]
fn a_failed_write_still_switches_for_this_run() {
    let dir = std::env::temp_dir().join(format!("vizdaw-appearance-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    // The preferences file's parent is a file: every write fails.
    let blocker = dir.join("blocker");
    std::fs::write(&blocker, b"x").unwrap();
    let mut p = Prefs::at(blocker.join("prefs.json"));
    let mut a = AppearancePrefs::load(&p);
    a.set_theme("eg13", &mut p).unwrap();
    assert_eq!(a.resolved().id, "eg13");
    assert!(a.problem.is_some());
    let _ = std::fs::remove_dir_all(&dir);
}
