//! The automation settings model over the preferences (slice 9a Task 2;
//! Swift `AutomationPreferencesTests`): handed to the core at launch,
//! kept only once the core took them, tolerant decode.

use serde_json::json;
use vizdaw_app::Prefs;
use vizdaw_app::automation_prefs::{
    AFTER_WRITE_CHOICES, APPLIES_TEXT, AutomationPrefs, AutomationSettings, PARAMS_ONLY_HELP,
    PARAMS_ONLY_LABEL, RETURN_STEP, THINNING_HELP, return_text,
};
use vizdaw_app::testing::FakeCore;
use vizdaw_ffi::VizError;

#[test]
fn launch_hands_the_settings_to_the_core() {
    let prefs = Prefs::in_memory();
    let core = FakeCore::new();
    let mut a = AutomationPrefs::load(&prefs);
    assert_eq!(*a.settings(), AutomationSettings::default());
    assert_eq!(a.apply_all(&core), None);
    assert_eq!(core.calls(), ["set_automation_settings(Latch,250,Medium)"]);
    a.apply_at_launch(&core);
    assert_eq!(a.problem(), None);
    assert_eq!(core.calls().len(), 2);
    core.refuse_next(
        "set_automation_settings",
        VizError::Model {
            message: "no".into(),
        },
    );
    a.apply_at_launch(&core);
    assert_eq!(a.problem(), Some("no"));
}

#[test]
fn kept_only_once_the_core_took_them() {
    let mut prefs = Prefs::in_memory();
    let core = FakeCore::new();
    let mut a = AutomationPrefs::load(&prefs);
    a.set_after_write("touch", &mut prefs, &core).unwrap();
    a.set_return_ms(3000, &mut prefs, &core).unwrap();
    a.set_thinning("high", &mut prefs, &core).unwrap();
    let s = a.settings();
    assert_eq!(
        (s.after_write.as_str(), s.return_ms, s.thinning.as_str()),
        ("touch", 2000, "high")
    );
    assert_eq!(
        core.calls().last().unwrap(),
        "set_automation_settings(Touch,2000,High)"
    );
    assert_eq!(
        prefs.raw("automation"),
        Some(&json!({"afterWrite": "touch", "returnMs": 2000, "thinning": "high"}))
    );
    assert_eq!(AutomationPrefs::load(&prefs).settings(), a.settings());

    a.set_after_write("write", &mut prefs, &core).unwrap();
    assert_eq!(a.settings().after_write, "touch", "Write is no mode here");

    core.refuse_next(
        "set_automation_settings",
        VizError::Model {
            message: "the return time is 0 to 2000 ms".into(),
        },
    );
    assert!(a.set_return_ms(100, &mut prefs, &core).is_err());
    assert_eq!(a.settings().return_ms, 2000);
    assert_eq!(a.problem(), Some("the return time is 0 to 2000 ms"));
    assert_eq!(prefs.raw("automation").unwrap()["returnMs"], 2000);

    a.set_return_ms(100, &mut prefs, &core).unwrap();
    assert_eq!(a.settings().return_ms, 100);
    assert_eq!(a.problem(), None);
    a.set_thinning("extreme", &mut prefs, &core).unwrap();
    assert_eq!(
        a.settings().thinning,
        "high",
        "an unknown name changes nothing"
    );
}

#[test]
fn damaged_settings_keep_defaults() {
    let mut prefs = Prefs::in_memory();
    let d = AutomationSettings::default();
    let load = |p: &Prefs| AutomationPrefs::load(p).settings().clone();
    prefs
        .set_raw(
            "automation",
            json!({"afterWrite": "write", "returnMs": 99999, "thinning": 7}),
        )
        .unwrap();
    assert_eq!(load(&prefs), d);
    prefs
        .set_raw("automation", json!({"returnMs": 500}))
        .unwrap();
    assert_eq!(
        load(&prefs),
        AutomationSettings {
            return_ms: 500,
            ..d.clone()
        }
    );
    prefs.set_raw("automation", json!("not json")).unwrap();
    assert_eq!(load(&prefs), d);
}

#[test]
fn after_write_never_offers_write() {
    assert_eq!(
        AFTER_WRITE_CHOICES.map(|(_, t)| t),
        ["Off", "Read", "Touch", "Latch", "Trim"]
    );
    assert_eq!(
        AFTER_WRITE_CHOICES.map(|(id, _)| id),
        ["off", "read", "touch", "latch", "trim"]
    );
}

#[test]
fn texts() {
    assert_eq!(return_text(250), "Return time: 250 ms");
    assert_eq!(RETURN_STEP, 50);
    assert_eq!(APPLIES_TEXT, "Changes apply from the next Play.");
    assert_eq!(
        PARAMS_ONLY_LABEL,
        "Write plugin parameters only when touched"
    );
    assert_eq!(
        PARAMS_ONLY_HELP,
        "Always on: writing every parameter of every plugin would bury the tracks."
    );
    assert_eq!(
        THINNING_HELP,
        "How closely written automation follows the move: 0.1 dB on faders and sends, 0.25 % elsewhere at Medium."
    );
}
