//! The Editing preferences over the preferences file (slice 9c Task 4;
//! Swift `NudgeTests` :141-217).

use serde_json::json;
use vizdaw_app::Prefs;
use vizdaw_app::editing_prefs::{
    EditingPrefs, NudgeSettings, NudgeTime, ScaleKind, TICKS_MAX, ticks_text,
};
use vizdaw_app::editor::Division;

#[test]
fn nudge_prefs_persist_and_decode_tolerantly() {
    let mut prefs = Prefs::in_memory();
    let mut p = EditingPrefs::load(&prefs);
    assert_eq!(*p.nudge(), NudgeSettings::default());
    assert_eq!(p.nudge().time, NudgeTime::Grid);
    assert!(p.nudge().follow_key_signature);
    let mut n = p.nudge().clone();
    n.pitch_scale_steps = true;
    n.amount = 2;
    n.time = NudgeTime::Ticks(120);
    p.set_nudge(n.clone(), &mut prefs).unwrap();
    let q = EditingPrefs::load(&prefs);
    assert_eq!(*q.nudge(), n);
    n.time = NudgeTime::Division(Division::ThirtySecond, true);
    n.scale = ScaleKind::MinorPentatonic;
    n.root = 7;
    p.set_nudge(n.clone(), &mut prefs).unwrap();
    assert_eq!(*EditingPrefs::load(&prefs).nudge(), n);
    // Swift's spelling.
    assert_eq!(
        prefs.raw("nudge").unwrap()["time"],
        json!({"division": {"_0": "thirtySecond", "triplet": true}})
    );
    assert_eq!(
        prefs.raw("nudge").unwrap()["pitch"],
        json!({"scaleSteps": {"_0": 2}})
    );

    // A damaged field keeps its default, the others are read.
    let mut damaged = Prefs::in_memory();
    damaged
        .set_raw(
            "nudge",
            json!({"scale": "nope", "followKeySignature": false}),
        )
        .unwrap();
    let r = EditingPrefs::load(&damaged);
    assert_eq!(r.nudge().scale, ScaleKind::Major);
    assert!(!r.nudge().follow_key_signature);
    damaged.set_raw("nudge", json!("garbage")).unwrap();
    assert_eq!(
        *EditingPrefs::load(&damaged).nudge(),
        NudgeSettings::default()
    );
    // A root past 11 wraps; ticks and amount are clamped.
    damaged
        .set_raw(
            "nudge",
            json!({"root": 14, "time": {"ticks": {"_0": 999999}}, "pitch": {"semitones": {"_0": 40}}}),
        )
        .unwrap();
    let c = EditingPrefs::load(&damaged);
    assert_eq!(c.nudge().root, 2);
    assert_eq!(c.nudge().time, NudgeTime::Ticks(TICKS_MAX));
    assert_eq!(c.nudge().amount, 12);
    assert!(!c.nudge().pitch_scale_steps);
}

#[test]
fn crossfade_is_kept_and_clamped() {
    let mut prefs = Prefs::in_memory();
    let mut p = EditingPrefs::load(&prefs);
    assert_eq!(p.crossfade_ms(), 10.);
    p.set_crossfade_ms(0., &mut prefs).unwrap();
    assert_eq!(p.crossfade_ms(), 1.);
    p.set_crossfade_ms(5000., &mut prefs).unwrap();
    assert_eq!(p.crossfade_ms(), 1000.);
    p.set_crossfade_ms(f64::NAN, &mut prefs).unwrap();
    assert_eq!(p.crossfade_ms(), 10., "not a number is the default");
    p.set_crossfade_ms(25., &mut prefs).unwrap();
    assert_eq!(EditingPrefs::load(&prefs).crossfade_ms(), 25.);
    prefs.set_raw("crossfadeMs", json!(-3.0)).unwrap();
    assert_eq!(EditingPrefs::load(&prefs).crossfade_ms(), 1.);
    prefs.set_raw("crossfadeMs", json!("junk")).unwrap();
    assert_eq!(EditingPrefs::load(&prefs).crossfade_ms(), 10.);
}

#[test]
fn zero_crossings_default_on_and_kept() {
    let mut prefs = Prefs::in_memory();
    let mut p = EditingPrefs::load(&prefs);
    assert!(p.zero_crossings());
    p.set_zero_crossings(false, &mut prefs).unwrap();
    assert!(!EditingPrefs::load(&prefs).zero_crossings());
    p.set_zero_crossings(true, &mut prefs).unwrap();
    assert!(EditingPrefs::load(&prefs).zero_crossings());
    prefs.set_raw("zeroCrossings", json!("junk")).unwrap();
    assert!(EditingPrefs::load(&prefs).zero_crossings());
    prefs.set_raw("zeroCrossings", json!(7)).unwrap();
    assert!(EditingPrefs::load(&prefs).zero_crossings());
}

#[test]
fn a_damaged_field_keeps_its_default() {
    let mut prefs = Prefs::in_memory();
    prefs.set_raw("externalEditor", json!(5)).unwrap();
    assert_eq!(EditingPrefs::load(&prefs).external_editor(), None);
    let mut p = EditingPrefs::load(&prefs);
    p.set_external_editor(Some("/Applications/Wave.app".into()), &mut prefs)
        .unwrap();
    assert_eq!(
        EditingPrefs::load(&prefs).external_editor(),
        Some(std::path::Path::new("/Applications/Wave.app"))
    );
    p.set_external_editor(None, &mut prefs).unwrap();
    assert_eq!(EditingPrefs::load(&prefs).external_editor(), None);
    // One damaged field leaves the others.
    prefs.set_raw("crossfadeMs", json!(true)).unwrap();
    prefs.set_raw("zeroCrossings", json!(false)).unwrap();
    let q = EditingPrefs::load(&prefs);
    assert_eq!((q.crossfade_ms(), q.zero_crossings()), (10., false));
    assert_eq!(ticks_text(240), "240 ticks (15360 per quarter note)");
}

#[test]
fn a_failed_write_is_the_problem_and_the_value_stays_for_this_run() {
    let dir = tempfile::tempdir().unwrap();
    // The prefs file's parent is a file: the write fails.
    let blocker = dir.path().join("f");
    std::fs::write(&blocker, "x").unwrap();
    let mut prefs = Prefs::at(blocker.join("prefs.json"));
    let mut p = EditingPrefs::load(&prefs);
    assert!(p.set_zero_crossings(false, &mut prefs).is_err());
    assert!(!p.zero_crossings());
    assert!(p.problem().is_some());
}
