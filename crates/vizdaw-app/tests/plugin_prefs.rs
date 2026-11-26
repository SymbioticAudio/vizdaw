//! The plug-in settings model over the preferences (slice 9c Task 5; Swift
//! `PluginSettingsTests`) and the `plugins.*` commands.

use core_control::ErrorCode;
use core_control::registry::ArgValue;
use serde_json::json;
use vizdaw_app::Prefs;
use vizdaw_app::plugin_prefs::{
    EQ_KEY, InstallOutcome, MULTIQ_CLAP_ID, NEW_CHANNEL_EQ_KEY, NO_BUNDLED_EQ, NOT_IN_THIS_BUILD,
    PluginPrefs, eq_choices_from, run_plugins,
};
use vizdaw_app::testing::FakeCore;
use vizdaw_ffi::VizError;
use vizdaw_ffi::bundled::DefaultEqRow;
use vizdaw_ffi::catalog::{CatalogRowFfi, PluginKindTag, PluginStatusRow};
use vizdaw_ffi::snapshot::{PluginFormatTag, PluginRefRow};

const DEST: &str = "/tmp/vizdaw-plugin-settings-test";

fn gain() -> DefaultEqRow {
    DefaultEqRow {
        format: PluginFormatTag::Clap,
        id: "vizdaw.test.gain".into(),
        name: "Test Gain".into(),
    }
}

fn model(prefs: &mut Prefs, core: &FakeCore) -> PluginPrefs {
    let mut p = PluginPrefs::load(prefs, core);
    p.set_install_destination(Some(DEST.into()));
    p
}

fn pushed(core: &FakeCore, what: &str) -> Vec<String> {
    core.calls()
        .into_iter()
        .filter(|c| c.starts_with(what))
        .collect()
}

fn flag(n: &str, v: bool) -> Vec<(String, ArgValue)> {
    vec![(n.into(), ArgValue::Flag(v))]
}

fn text(n: &str, v: &str) -> Vec<(String, ArgValue)> {
    vec![(n.into(), ArgValue::Text(v.into()))]
}

fn run(
    id: &str,
    args: &[(String, ArgValue)],
    p: &mut PluginPrefs,
    prefs: &mut Prefs,
    core: &FakeCore,
) -> Result<Option<String>, core_control::ControlError> {
    run_plugins(id, args, p, prefs, core).map(|r| r.status)
}

#[test]
fn defaults_on_and_pushes() {
    let mut prefs = Prefs::in_memory();
    let core = FakeCore::new();
    let p = model(&mut prefs, &core);
    assert!(p.new_channel_eq());
    assert_eq!(
        pushed(&core, "set_new_channel_eq"),
        ["set_new_channel_eq(true)"]
    );
    assert_eq!(pushed(&core, "set_default_eq"), ["set_default_eq(multi-q)"]);
}

#[test]
fn persists() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("prefs.json");
    let mut prefs = Prefs::at(&file);
    let core = FakeCore::new();
    let mut p = model(&mut prefs, &core);
    p.set_new_channel_eq(false, &mut prefs, &core).unwrap();
    assert!(!p.new_channel_eq());
    assert_eq!(
        pushed(&core, "set_new_channel_eq"),
        ["set_new_channel_eq(true)", "set_new_channel_eq(false)"]
    );
    let mut prefs = Prefs::at(&file);
    assert_eq!(prefs.raw(NEW_CHANNEL_EQ_KEY), Some(&json!(false)));
    let core2 = FakeCore::new();
    let again = model(&mut prefs, &core2);
    assert!(!again.new_channel_eq());
    assert_eq!(
        pushed(&core2, "set_new_channel_eq"),
        ["set_new_channel_eq(false)"]
    );
}

#[test]
fn exists_asks_to_replace() {
    let mut prefs = Prefs::in_memory();
    let core = FakeCore::new();
    let mut p = model(&mut prefs, &core);
    core.set_call_error(
        "install_multiq",
        Some(VizError::Exists {
            path: format!("{DEST}/Multi-Q.component"),
        }),
    );
    assert_eq!(p.install(false, &core), InstallOutcome::AskReplace);
    assert_eq!(p.status(), None);
    core.set_call_error("install_multiq", None);
    let out = p.install(true, &core);
    assert_eq!(
        out,
        InstallOutcome::Installed(format!("{DEST}/Multi-Q.component"))
    );
    assert_eq!(
        pushed(&core, "install_multiq"),
        [
            format!("install_multiq(Some(\"{DEST}\"),false)"),
            format!("install_multiq(Some(\"{DEST}\"),true)"),
        ]
    );
    assert_eq!(p.status(), Some(format!("Installed in {DEST}.").as_str()));
}

#[test]
fn other_failures_say_why() {
    let mut prefs = Prefs::in_memory();
    let core = FakeCore::new();
    let mut p = model(&mut prefs, &core);
    let why = "Multi-Q could not be installed: Permission denied";
    core.set_call_error(
        "install_multiq",
        Some(VizError::Io {
            message: why.into(),
        }),
    );
    assert_eq!(p.install(false, &core), InstallOutcome::Failed(why.into()));
    assert_eq!(p.status(), Some(why));
}

#[test]
fn commands_never_ask() {
    let mut prefs = Prefs::in_memory();
    let core = FakeCore::new();
    let mut p = model(&mut prefs, &core);
    core.set_call_error(
        "install_multiq",
        Some(VizError::Exists {
            path: format!("{DEST}/Multi-Q.component"),
        }),
    );
    let e = run("plugins.install_multiq", &[], &mut p, &mut prefs, &core).unwrap_err();
    assert_eq!(e.code, ErrorCode::DialogRequired);
    assert_eq!(e.dialog.as_deref(), Some("Replace"));
    assert_eq!(
        pushed(&core, "install_multiq"),
        [format!("install_multiq(Some(\"{DEST}\"),false)")]
    );

    core.set_call_error("install_multiq", None);
    let s = run(
        "plugins.install_multiq",
        &flag("replace", true),
        &mut p,
        &mut prefs,
        &core,
    )
    .unwrap();
    assert_eq!(
        s.as_deref(),
        Some(format!("{DEST}/Multi-Q.component").as_str())
    );
    assert_eq!(
        pushed(&core, "install_multiq").last().unwrap(),
        &format!("install_multiq(Some(\"{DEST}\"),true)")
    );

    core.set_call_error(
        "install_multiq",
        Some(VizError::Model {
            message: NOT_IN_THIS_BUILD.into(),
        }),
    );
    let e = run("plugins.install_multiq", &[], &mut p, &mut prefs, &core).unwrap_err();
    assert_eq!(e.code, ErrorCode::Unavailable);
    assert_eq!(e.message, "Multi-Q is not in this build of VizDAW.");

    core.set_call_error(
        "install_multiq",
        Some(VizError::Io {
            message: "disk full".into(),
        }),
    );
    let e = run("plugins.install_multiq", &[], &mut p, &mut prefs, &core).unwrap_err();
    assert_eq!(e.code, ErrorCode::Failed);
    assert_eq!(e.message, "Install Failed: disk full");
}

#[test]
fn switch_command_sets_and_keeps() {
    let mut prefs = Prefs::in_memory();
    let core = FakeCore::new();
    let mut p = model(&mut prefs, &core);
    let s = run(
        "plugins.new_channel_eq",
        &flag("on", false),
        &mut p,
        &mut prefs,
        &core,
    )
    .unwrap();
    assert_eq!(s.as_deref(), Some("new-channel EQ off"));
    assert!(!p.new_channel_eq());
    assert_eq!(
        pushed(&core, "set_new_channel_eq"),
        ["set_new_channel_eq(true)", "set_new_channel_eq(false)"]
    );
    assert!(!PluginPrefs::load(&mut prefs, &core).new_channel_eq());
    let e = run("plugins.new_channel_eq", &[], &mut p, &mut prefs, &core).unwrap_err();
    assert_eq!(e.code, ErrorCode::InvalidParams);
    assert_eq!(e.message, "plugins.new_channel_eq: `on` is required");
}

#[test]
fn no_bundled_eq_disables() {
    let mut prefs = Prefs::in_memory();
    let core = FakeCore::new();
    core.set_bundled(None, false);
    let mut p = model(&mut prefs, &core);
    assert_eq!(p.bundled_eq_name(), None);
    assert_eq!(
        p.install(false, &core),
        InstallOutcome::Failed(NO_BUNDLED_EQ.into())
    );
    assert_eq!(p.status(), Some("This build of VizDAW has no bundled EQ."));
    assert!(pushed(&core, "install_multiq").is_empty());
    let e = run("plugins.install_multiq", &[], &mut p, &mut prefs, &core).unwrap_err();
    assert_eq!(e.code, ErrorCode::Unavailable);
    assert_eq!(e.message, "Multi-Q is not in this build of VizDAW.");
    // A build with the EQ but no AU: the command is unavailable too.
    core.set_bundled(Some("Multi-Q"), false);
    let mut p = model(&mut prefs, &core);
    let e = run("plugins.install_multiq", &[], &mut p, &mut prefs, &core).unwrap_err();
    assert_eq!(e.code, ErrorCode::Unavailable);
    assert!(pushed(&core, "install_multiq").is_empty());
}

#[test]
fn choice_defaults_to_multiq_and_persists() {
    let mut prefs = Prefs::in_memory();
    let core = FakeCore::new();
    let mut p = model(&mut prefs, &core);
    assert_eq!(p.default_eq(), None);
    assert_eq!(p.default_eq_name(), "Multi-Q");
    p.set_default_eq(Some(gain()), &mut prefs, &core).unwrap();
    assert_eq!(p.default_eq(), Some(&gain()));
    assert_eq!(
        pushed(&core, "set_default_eq"),
        [
            "set_default_eq(multi-q)",
            "set_default_eq(clap:vizdaw.test.gain)"
        ]
    );
    assert_eq!(
        prefs.raw(EQ_KEY),
        Some(&json!({"format": "clap", "id": "vizdaw.test.gain", "name": "Test Gain"}))
    );
    let core2 = FakeCore::new();
    let mut again = model(&mut prefs, &core2);
    assert_eq!(again.default_eq(), Some(&gain()));
    assert_eq!(again.default_eq_name(), "Test Gain");
    assert_eq!(
        pushed(&core2, "set_default_eq"),
        ["set_default_eq(clap:vizdaw.test.gain)"]
    );
    again.set_default_eq(None, &mut prefs, &core2).unwrap();
    assert_eq!(prefs.raw(EQ_KEY), None);
}

#[test]
fn damaged_choice_is_multiq_and_removed() {
    let damaged = [
        json!("clap:vizdaw.test.gain"),
        json!({"format": "vst", "id": "vizdaw.test.gain", "name": "Test Gain"}),
        json!({"format": "clap", "name": "Test Gain"}),
        json!({"format": "clap", "id": "", "name": "Test Gain"}),
        json!({"format": "clap", "id": "vizdaw.test\u{0}gain", "name": "Test Gain"}),
    ];
    for v in damaged {
        let mut prefs = Prefs::in_memory();
        prefs.set_raw(EQ_KEY, v.clone()).unwrap();
        let core = FakeCore::new();
        let p = model(&mut prefs, &core);
        assert_eq!(p.default_eq(), None, "{v}");
        assert_eq!(pushed(&core, "set_default_eq"), ["set_default_eq(multi-q)"]);
        assert_eq!(prefs.raw(EQ_KEY), None, "{v}");
    }
}

#[test]
fn a_stored_vst3_choice_is_kept() {
    let mut prefs = Prefs::in_memory();
    let v = json!({"format": "vst3", "id": "ABCD", "name": "Some EQ"});
    prefs.set_raw(EQ_KEY, v.clone()).unwrap();
    let core = FakeCore::new();
    let p = model(&mut prefs, &core);
    let row = p.default_eq().expect("kept");
    assert_eq!(row.format, PluginFormatTag::Vst3);
    assert_eq!(
        pushed(&core, "set_default_eq"),
        ["set_default_eq(vst3:ABCD)"]
    );
    assert_eq!(prefs.raw(EQ_KEY), Some(&v));
}

#[test]
fn missing_choice_shown_and_said() {
    let mut prefs = Prefs::in_memory();
    let core = FakeCore::new();
    model(&mut prefs, &core)
        .set_default_eq(Some(gain()), &mut prefs, &core)
        .unwrap();
    core.set_eq_note(Some(
        "Test Gain is not available, so new channels get Multi-Q.",
    ));
    let mut p = model(&mut prefs, &core);
    p.refresh(&core);
    assert_eq!(p.default_eq(), Some(&gain()));
    assert!(!p.default_eq_available());
    assert_eq!(
        p.note(),
        Some("Test Gain is not available, so new channels get Multi-Q.")
    );
    core.set_eq_choices(vec![gain()]);
    core.set_eq_note(None);
    p.refresh(&core);
    assert!(p.default_eq_available());
    assert_eq!(p.note(), None);
}

#[test]
fn default_eq_sets_and_answers() {
    let mut prefs = Prefs::in_memory();
    let core = FakeCore::new();
    core.set_eq_choices(vec![gain()]);
    let mut p = model(&mut prefs, &core);
    let mut set = |plugin: &str| {
        run(
            "plugins.default_eq",
            &text("plugin", plugin),
            &mut p,
            &mut prefs,
            &core,
        )
    };
    assert_eq!(
        set("clap:vizdaw.test.gain").unwrap().as_deref(),
        Some("default EQ Test Gain")
    );
    assert_eq!(
        set("multi-q").unwrap().as_deref(),
        Some("default EQ Multi-Q")
    );
    assert_eq!(
        pushed(&core, "set_default_eq"),
        [
            "set_default_eq(multi-q)",
            "set_default_eq(clap:vizdaw.test.gain)",
            "set_default_eq(multi-q)"
        ]
    );
}

#[test]
fn default_eq_refuses_bad_input() {
    let mut prefs = Prefs::in_memory();
    let core = FakeCore::new();
    core.set_eq_choices(vec![gain()]);
    let mut p = model(&mut prefs, &core);
    p.set_default_eq(Some(gain()), &mut prefs, &core).unwrap();
    for bad in ["clap:", "vst:x", "gain", "clap:a\u{0}b", "au:", ""] {
        let e = run(
            "plugins.default_eq",
            &text("plugin", bad),
            &mut p,
            &mut prefs,
            &core,
        )
        .unwrap_err();
        assert_eq!(e.code, ErrorCode::InvalidParams, "{bad:?}");
        assert_eq!(
            e.message,
            "plugins.default_eq: `plugin` is multi-q, clap:<id> or au:<type/subtype/manufacturer>"
        );
        assert_eq!(p.default_eq(), Some(&gain()));
    }
    let e = run(
        "plugins.default_eq",
        &text("plugin", "clap:vizdaw.test.synth4"),
        &mut p,
        &mut prefs,
        &core,
    )
    .unwrap_err();
    assert_eq!(e.code, ErrorCode::UnknownId);
    assert_eq!(e.message, "No installed effect is clap:vizdaw.test.synth4.");
    let e = run("plugins.default_eq", &[], &mut p, &mut prefs, &core).unwrap_err();
    assert_eq!(e.code, ErrorCode::InvalidParams);
    assert_eq!(e.message, "plugins.default_eq: `plugin` is required");
    assert_eq!(p.default_eq(), Some(&gain()));
    assert_eq!(pushed(&core, "set_default_eq").len(), 2);
}

fn row(
    id: &str,
    name: &str,
    format: PluginFormatTag,
    kind: Option<PluginKindTag>,
    status: PluginStatusRow,
    enabled: bool,
) -> CatalogRowFfi {
    CatalogRowFfi {
        plugin: PluginRefRow {
            format,
            id: id.into(),
            name: name.into(),
            vendor: "VizDAW".into(),
            version: "1".into(),
        },
        kind,
        status,
        enabled,
        path: String::new(),
    }
}

#[test]
fn eq_choices_are_usable_effects_minus_the_multiq_clap() {
    use PluginFormatTag::{Au, Clap};
    use PluginKindTag::{Effect, Instrument};
    use PluginStatusRow::{Disabled, Missing, NotValidated, Ok};
    let rows = [
        row(
            "vizdaw.test.gain",
            "Test Gain",
            Clap,
            Some(Effect),
            Ok,
            true,
        ),
        row(MULTIQ_CLAP_ID, "Multi-Q", Clap, Some(Effect), Ok, true),
        row(
            "aufx/Gain/Test",
            "AU Gain",
            Au,
            Some(Effect),
            NotValidated,
            true,
        ),
        row("vizdaw.test.off", "Off", Clap, Some(Effect), Disabled, true),
        row(
            "vizdaw.test.gone",
            "Gone",
            Clap,
            Some(Effect),
            Missing,
            true,
        ),
        row("vizdaw.test.pref", "Pref", Clap, Some(Effect), Ok, false),
        row("", "Empty", Clap, Some(Effect), Ok, true),
        row(
            "vizdaw.test.synth4",
            "Synth",
            Clap,
            Some(Instrument),
            Ok,
            true,
        ),
        row("vizdaw.test.unknown", "Unknown", Clap, None, Ok, true),
    ];
    assert_eq!(
        eq_choices_from(&rows),
        vec![
            gain(),
            DefaultEqRow {
                format: Au,
                id: "aufx/Gain/Test".into(),
                name: "AU Gain".into()
            }
        ]
    );
}
