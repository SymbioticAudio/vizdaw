//! The plug-ins Co-Producer may use (plan 8d Task 3, Ruling 3): references
//! resolve by the Plugin Manager's rules, and every naming of Multi-Q
//! resolves to the bundled CLAP; the list offers only usable plug-ins,
//! built-ins first; a slot's parameters answer for this song only; and
//! `state.get mixer` names each slot's plug-in and each strip's output.
//! The catalog holds only the test plug-ins (and, where a test says so,
//! made-up AU entries that never load); Multi-Q is the fake set by
//! `bundled::debug_set_multiq`. Offline only.

use core_host::PluginKind;
use core_host::bundled::{MULTIQ_AU_ID, MULTIQ_CLAP_ID};
use core_host::catalog::{Catalog, CatalogEntry, ScanStatus, Source};
use core_model::mixer::{PluginFormat, PluginRef};
use serde_json::Value;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard};
use vizdaw_ffi::bundled::{self, debug_clear_multiq, debug_set_multiq};
use vizdaw_ffi::catalog::PluginKindTag;
use vizdaw_ffi::control::{ControlResult, result_json};
use vizdaw_ffi::host::{force_offline, reset_host};
use vizdaw_ffi::mixer_chores::{plugin_ref_text, resolve_plugin};
use vizdaw_ffi::{
    CoProducerParamsRow, CoProducerPluginRow, ParamRow, PluginFormatTag, PluginRefRow, Session,
    StripKindRow, TrackKindRow, VizError, co_producer_plugins, set_default_eq, set_new_channel_eq,
};

static SERIAL: Mutex<()> = Mutex::new(());

const GAIN_ID: &str = "vizdaw.test.gain";

/// One test at a time; offline; a catalog holding only the test plug-ins,
/// all enabled; the fake Multi-Q; new channels get it as their EQ.
fn serial() -> MutexGuard<'static, ()> {
    let g = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    force_offline();
    vizdaw_ffi::host::set_offline_rate(48_000.0);
    reset_host();
    vizdaw_ffi::catalog::with(|c| *c = Catalog::new());
    vizdaw_ffi::catalog::add_test_bundle(&test_plugins::bundle()).unwrap();
    set_new_channel_eq(true);
    set_default_eq(None);
    debug_clear_multiq();
    debug_set_multiq(Some(mq()));
    g
}

fn mq() -> PluginRef {
    PluginRef {
        format: PluginFormat::Clap,
        id: MULTIQ_CLAP_ID.into(),
        name: "Multi-Q".into(),
        vendor: String::new(),
        version: "0.10.9".into(),
    }
}

fn enable(id: &str, on: bool) {
    vizdaw_ffi::catalog::with(|c| c.set_enabled(PluginFormat::Clap, id, on));
}

fn catalog_ref(format: PluginFormat, id: &str) -> PluginRef {
    vizdaw_ffi::catalog::with(|c| c.preferred(format, id).unwrap().plugin.clone())
}

/// Records a made-up AU entry (never loaded) with `status`.
fn fake_au(id: &str, name: &str, kind: Option<PluginKind>, status: ScanStatus) {
    let src = Source {
        format: PluginFormat::Au,
        path: PathBuf::from(format!("au:{id}")),
        version: "1.0.0".into(),
        mtime: 0,
    };
    let entry = CatalogEntry {
        source: src.clone(),
        plugin: PluginRef {
            format: PluginFormat::Au,
            id: id.into(),
            name: name.into(),
            vendor: "Maker".into(),
            version: "1.0.0".into(),
        },
        kind,
        info: None,
        status,
    };
    vizdaw_ffi::catalog::with(|c| c.record(&src, vec![entry]));
}

fn refusal(r: Result<PluginRef, VizError>) -> String {
    match r {
        Err(VizError::Model { message }) => message,
        other => panic!("expected a refusal, got {other:?}"),
    }
}

fn kind_of(tag: PluginKindTag) -> PluginKind {
    match tag {
        PluginKindTag::Instrument => PluginKind::Instrument,
        PluginKindTag::Effect => PluginKind::Effect,
    }
}

fn refs(rows: &[CoProducerPluginRow]) -> Vec<&str> {
    rows.iter().map(|r| r.plugin.as_str()).collect()
}

#[test]
fn plugins_resolve_by_the_plugin_managers_rules() {
    let _g = serial();
    let gain = catalog_ref(PluginFormat::Clap, GAIN_ID);
    assert_eq!(
        resolve_plugin("clap:vizdaw.test.gain", PluginKind::Effect).unwrap(),
        gain
    );
    assert_eq!(
        refusal(resolve_plugin(
            "clap:vizdaw.test.gain",
            PluginKind::Instrument
        )),
        "Test Gain is an effect, not an instrument."
    );

    let synth = catalog_ref(PluginFormat::Clap, "vizdaw.test.synth4");
    assert_eq!(
        refusal(resolve_plugin(
            "clap:vizdaw.test.synth4",
            PluginKind::Effect
        )),
        format!("{} is an instrument, not an effect.", synth.name)
    );
    assert_eq!(
        resolve_plugin("clap:vizdaw.test.synth4", PluginKind::Instrument).unwrap(),
        synth
    );

    enable(GAIN_ID, false);
    assert_eq!(
        refusal(resolve_plugin("clap:vizdaw.test.gain", PluginKind::Effect)),
        "Test Gain is turned off in the Plugin Manager."
    );
    enable(GAIN_ID, true);
    assert!(resolve_plugin("clap:vizdaw.test.gain", PluginKind::Effect).is_ok());

    assert_eq!(
        refusal(resolve_plugin("clap:vizdaw.test.none", PluginKind::Effect)),
        "No installed plug-in is clap:vizdaw.test.none."
    );
    // The CLAP and AU forms are not interchangeable.
    assert_eq!(
        refusal(resolve_plugin("au:vizdaw.test.gain", PluginKind::Effect)),
        "No installed plug-in is au:vizdaw.test.gain."
    );
    for odd in ["", "clap:", "gain", "CLAP:vizdaw.test.gain", "\0"] {
        assert!(resolve_plugin(odd, PluginKind::Effect).is_err(), "{odd:?}");
    }
    let long = format!("clap:{}", "x".repeat(70_000));
    let m = refusal(resolve_plugin(&long, PluginKind::Effect));
    assert!(m.chars().count() < 200, "{}", m.len());

    // A name with a bidi mark shows through `shown_name`.
    fake_au(
        "aumu/Bidi/Test",
        "Evil\u{202E}Synth",
        Some(PluginKind::Instrument),
        ScanStatus::Ok,
    );
    assert_eq!(
        refusal(resolve_plugin("au:aumu/Bidi/Test", PluginKind::Effect)),
        "Evil Synth is an instrument, not an effect."
    );
    assert_eq!(
        resolve_plugin("au:aumu/Bidi/Test", PluginKind::Instrument)
            .unwrap()
            .id,
        "aumu/Bidi/Test"
    );

    // Blocklisted plug-ins cannot be used, with the reason.
    fake_au(
        "aufx/Bad1/Test",
        "Bad One",
        Some(PluginKind::Effect),
        ScanStatus::Blocklisted {
            reason: "crashed (signal 11)".into(),
        },
    );
    assert_eq!(
        refusal(resolve_plugin("au:aufx/Bad1/Test", PluginKind::Effect)),
        "Bad One cannot be used: it is blocklisted (crashed (signal 11))."
    );
    // Not validated yet (AU v3) is usable.
    fake_au(
        "aufx/New3/Test",
        "New Three",
        Some(PluginKind::Effect),
        ScanStatus::NotValidated,
    );
    assert_eq!(
        resolve_plugin("au:aufx/New3/Test", PluginKind::Effect)
            .unwrap()
            .name,
        "New Three"
    );
    // A missing one cannot be used.
    vizdaw_ffi::catalog::with(|c| {
        let found: Vec<Source> = c
            .entries()
            .iter()
            .map(|e| e.source.clone())
            .filter(|s| s.path.as_path() != std::path::Path::new("au:aufx/New3/Test"))
            .collect();
        c.mark_found(&found);
    });
    assert_eq!(
        refusal(resolve_plugin("au:aufx/New3/Test", PluginKind::Effect)),
        "New Three cannot be used: it is missing."
    );
}

#[test]
fn multiq_is_the_bundled_clap() {
    let _g = serial();
    let bundled = bundled::multiq().unwrap();
    assert_eq!(bundled.format, PluginFormat::Clap);
    // The catalog's own copies are never what Co-Producer loads.
    fake_au(
        MULTIQ_AU_ID,
        "Multi-Q",
        Some(PluginKind::Effect),
        ScanStatus::Ok,
    );
    let forms = [
        "multi-q".to_string(),
        format!("clap:{MULTIQ_CLAP_ID}"),
        format!("au:{MULTIQ_AU_ID}"),
    ];
    for f in &forms {
        let got = resolve_plugin(f, PluginKind::Effect).unwrap();
        assert_eq!(got, bundled, "{f}");
        assert_eq!(got.format, PluginFormat::Clap, "{f}");
        assert_eq!(plugin_ref_text(&got), "multi-q", "{f}");
        assert_eq!(
            refusal(resolve_plugin(f, PluginKind::Instrument)),
            "Multi-Q is an effect, not an instrument.",
            "{f}"
        );
    }
    debug_set_multiq(None);
    for f in &forms {
        assert_eq!(
            refusal(resolve_plugin(f, PluginKind::Effect)),
            "Multi-Q is not available in this build.",
            "{f}"
        );
    }
    assert_eq!(
        refusal(resolve_plugin("multi-q", PluginKind::Instrument)),
        "Multi-Q is an effect, not an instrument."
    );
}

#[test]
fn built_in_synth_is_not_a_plugin_to_resolve() {
    let _g = serial();
    assert_eq!(
        refusal(resolve_plugin("built-in-synth", PluginKind::Effect)),
        "Built-in Synth is an instrument, not an effect."
    );
    assert!(resolve_plugin("built-in-synth", PluginKind::Instrument).is_err());
}

#[test]
fn the_list_offers_only_usable_plugins() {
    let _g = serial();
    let all = co_producer_plugins(None);
    assert!(!all.truncated);
    let rows = &all.plugins;
    assert_eq!(
        refs(rows),
        [
            "multi-q",
            "built-in-synth",
            "clap:vizdaw.test.synth4",
            "clap:vizdaw.test.bank",
            "clap:vizdaw.test.crasher",
            "clap:vizdaw.test.ducker",
            "clap:vizdaw.test.gain",
            "clap:vizdaw.test.hanger",
            "clap:vizdaw.test.nan",
        ]
    );
    let first = &rows[0];
    assert_eq!(
        (
            first.name.as_str(),
            first.maker.as_str(),
            first.format.as_str(),
            first.kind
        ),
        ("Multi-Q", "", "built_in", PluginKindTag::Effect)
    );
    let second = &rows[1];
    assert_eq!(
        (
            second.name.as_str(),
            second.maker.as_str(),
            second.format.as_str(),
            second.kind
        ),
        ("Built-in Synth", "", "built_in", PluginKindTag::Instrument)
    );
    let gain = rows.iter().find(|r| r.plugin == "clap:vizdaw.test.gain");
    let gain = gain.unwrap();
    assert_eq!(
        (gain.name.as_str(), gain.format.as_str(), gain.kind),
        ("Test Gain", "clap", PluginKindTag::Effect)
    );
    assert_eq!(gain.maker, "VizDAW");
    assert!(!gain.version.is_empty());

    // The catalog's Multi-Q entries are never listed: Multi-Q is once.
    fake_au(
        MULTIQ_AU_ID,
        "Multi-Q",
        Some(PluginKind::Effect),
        ScanStatus::Ok,
    );
    let rows = co_producer_plugins(None).plugins;
    for r in &rows {
        assert!(
            !r.plugin.contains(MULTIQ_CLAP_ID) && !r.plugin.contains(MULTIQ_AU_ID),
            "{r:?}"
        );
    }

    // Disabled, blocklisted and unknown-kind plug-ins are left out; a
    // not-yet-validated AU is in, with the AU form.
    enable(GAIN_ID, false);
    fake_au(
        "aufx/Bad1/Test",
        "Bad One",
        Some(PluginKind::Effect),
        ScanStatus::Blocklisted {
            reason: "timed out".into(),
        },
    );
    fake_au("aufx/Huh1/Test", "Huh", None, ScanStatus::Ok);
    fake_au(
        "aufx/New3/Test",
        "A New Three",
        Some(PluginKind::Effect),
        ScanStatus::NotValidated,
    );
    let rows = co_producer_plugins(None).plugins;
    let got = refs(&rows);
    assert!(!got.contains(&"clap:vizdaw.test.gain"), "{got:?}");
    assert!(!got.contains(&"au:aufx/Bad1/Test"), "{got:?}");
    assert!(!got.contains(&"au:aufx/Huh1/Test"), "{got:?}");
    // Effects by name, ignoring case: "A New Three" comes first.
    assert_eq!(got[3], "au:aufx/New3/Test", "{got:?}");
    assert_eq!(rows[3].format, "au");

    // Effects only, Multi-Q first.
    let fx = co_producer_plugins(Some(PluginKindTag::Effect)).plugins;
    assert_eq!(fx[0].plugin, "multi-q");
    assert!(fx.iter().all(|r| r.kind == PluginKindTag::Effect), "{fx:?}");
    assert!(!refs(&fx).contains(&"built-in-synth"));
    // Instruments only, the Built-in Synth first.
    let inst = co_producer_plugins(Some(PluginKindTag::Instrument)).plugins;
    assert_eq!(refs(&inst), ["built-in-synth", "clap:vizdaw.test.synth4"]);

    // Without a usable Multi-Q, it is not offered.
    debug_set_multiq(None);
    let rows = co_producer_plugins(None).plugins;
    assert_eq!(rows[0].plugin, "built-in-synth");
}

#[test]
fn the_list_is_capped() {
    let _g = serial();
    vizdaw_ffi::catalog::with(|c| {
        let src = Source {
            format: PluginFormat::Au,
            path: PathBuf::from("au:many"),
            version: "1".into(),
            mtime: 0,
        };
        let entries = (0..vizdaw_ffi::mixer_chores::MAX_PLUGINS + 10)
            .map(|n| CatalogEntry {
                source: src.clone(),
                plugin: PluginRef {
                    format: PluginFormat::Au,
                    id: format!("aufx/{n:04}/Many"),
                    name: format!("Many {n:05}"),
                    vendor: String::new(),
                    version: "1".into(),
                },
                kind: Some(PluginKind::Effect),
                info: None,
                status: ScanStatus::Ok,
            })
            .collect();
        c.record(&src, entries);
    });
    let all = co_producer_plugins(None);
    assert!(all.truncated);
    assert_eq!(all.plugins.len(), vizdaw_ffi::mixer_chores::MAX_PLUGINS);
    assert_eq!(all.plugins[0].plugin, "multi-q");
}

#[test]
fn reference_text_round_trips() {
    let _g = serial();
    fake_au(
        "aufx/New3/Test",
        "New Three",
        Some(PluginKind::Effect),
        ScanStatus::NotValidated,
    );
    let rows = co_producer_plugins(None).plugins;
    assert!(rows.len() > 3);
    for row in &rows {
        if row.plugin == "built-in-synth" {
            // Not a plug-in: `track.new_plugin_instrument` takes it as it
            // is (Ruling 1), and it is not resolved.
            continue;
        }
        let got = resolve_plugin(&row.plugin, kind_of(row.kind))
            .unwrap_or_else(|e| panic!("{}: {e}", row.plugin));
        assert_eq!(plugin_ref_text(&got), row.plugin);
    }
    assert_eq!(
        plugin_ref_text(&PluginRef {
            format: PluginFormat::Au,
            id: "aufx/Abcd/Efgh".into(),
            name: String::new(),
            vendor: String::new(),
            version: String::new(),
        }),
        "au:aufx/Abcd/Efgh"
    );
}

fn first_seq(s: &Session) -> u64 {
    s.snapshot().unwrap().sequences[0].id
}

/// A new song with one audio track (whose EQ is the fake Multi-Q) and a
/// test gain insert on it, not yet running: (session, strip, insert slot,
/// EQ slot).
fn song_with_gain() -> (Arc<Session>, u64, u64, u64) {
    let s = Session::new_empty().unwrap();
    s.new_track(first_seq(&s), TrackKindRow::Audio, None)
        .unwrap();
    let strip = s
        .mixer()
        .unwrap()
        .strips
        .into_iter()
        .find(|x| matches!(x.kind, StripKindRow::AudioPath { .. }))
        .unwrap();
    s.add_insert(
        strip.id,
        None,
        PluginRefRow {
            format: PluginFormatTag::Clap,
            id: GAIN_ID.into(),
            name: "Test Gain".into(),
            vendor: "VizDAW".into(),
            version: "1.0.0".into(),
        },
    )
    .unwrap();
    let strip = s
        .mixer()
        .unwrap()
        .strips
        .into_iter()
        .find(|x| x.id == strip.id)
        .unwrap();
    let insert = strip.inserts[0].id;
    let eq = strip.eq.unwrap().id;
    (s, strip.id, insert, eq)
}

fn params_refusal(r: Result<CoProducerParamsRow, VizError>) -> String {
    match r {
        Err(VizError::Model { message }) => message,
        other => panic!("expected a refusal, got {other:?}"),
    }
}

#[test]
fn params_answer_for_this_song_only() {
    let _g = serial();
    let (s, _strip, insert, eq) = song_with_gain();
    let row = s.co_producer_params(insert).unwrap();
    assert_eq!(
        row,
        CoProducerParamsRow {
            slot: insert,
            plugin: "clap:vizdaw.test.gain".into(),
            running: false,
            params: Vec::new(),
        }
    );
    let row = s.co_producer_params(eq).unwrap();
    assert_eq!((row.plugin.as_str(), row.running), ("multi-q", false));

    assert_eq!(
        params_refusal(s.co_producer_params(999)),
        "This song has no slot 999."
    );
    assert!(s.co_producer_params(u64::MAX).is_err());

    // Another song's slot is not this song's.
    let other = Session::new_empty().unwrap();
    assert!(other.plugin_slots().unwrap().iter().all(|r| r.id != insert));
    assert_eq!(
        params_refusal(other.co_producer_params(insert)),
        format!("This song has no slot {insert}.")
    );
    other.close().unwrap();
    s.close().unwrap();
}

/// `state.get mixer`'s JSON for `s`.
fn mixer_json(s: &Session) -> Value {
    result_json(&ControlResult::Mixer {
        doc: "d1".into(),
        mixer: s.mixer().unwrap(),
    })
}

#[test]
fn mixer_state_names_plugins() {
    let _g = serial();
    let (s, strip, insert, eq) = song_with_gain();
    let j = mixer_json(&s);
    let strips = j["strips"].as_array().unwrap();
    let ours = strips
        .iter()
        .find(|x| x["id"].as_u64() == Some(strip))
        .unwrap();
    assert_eq!(ours["inserts"][0]["slot"].as_u64(), Some(insert));
    assert_eq!(ours["inserts"][0]["plugin"], "clap:vizdaw.test.gain");
    assert_eq!(ours["eq"]["slot"].as_u64(), Some(eq));
    assert_eq!(ours["eq"]["plugin"], "multi-q");
    assert_eq!(ours["output"], Value::Null);
    assert_eq!(ours["output_name"], "Master");
    for x in strips {
        assert!(x.get("output_name").is_some(), "{x}");
    }
    let master = strips.iter().find(|x| x["kind_label"] == "Master").unwrap();
    assert_eq!(master["output_name"], Value::Null);
    s.close().unwrap();
}

#[test]
fn plugin_results_json() {
    let _g = serial();
    let plugins = co_producer_plugins(Some(PluginKindTag::Instrument));
    let j = result_json(&ControlResult::CoProducerPlugins {
        plugins: plugins.plugins.clone(),
        truncated: plugins.truncated,
    });
    let mut keys: Vec<&String> = j.as_object().unwrap().keys().collect();
    keys.sort();
    assert_eq!(keys, ["plugins", "truncated"]);
    assert_eq!(j["truncated"], false);
    assert_eq!(
        j["plugins"][0],
        serde_json::json!({
            "plugin": "built-in-synth", "name": "Built-in Synth", "maker": "",
            "format": "built_in", "kind": "instrument", "version": "",
        })
    );
    assert_eq!(j["plugins"][1]["kind"], "instrument");
    assert_eq!(j["plugins"][1]["format"], "clap");

    let j = result_json(&ControlResult::CoProducerParams {
        doc: "d1".into(),
        row: CoProducerParamsRow {
            slot: 7,
            plugin: "clap:vizdaw.test.gain".into(),
            running: true,
            params: vec![ParamRow {
                id: 0,
                name: "Gain".into(),
                module: String::new(),
                min: 0.0,
                max: 2.0,
                default: 1.0,
                value: 0.5,
                stepped: false,
                automatable: true,
                hidden: false,
            }],
        },
    });
    assert_eq!(
        j,
        serde_json::json!({
            "doc": "d1", "slot": 7, "plugin": "clap:vizdaw.test.gain", "running": true,
            "params": [{
                "id": 0, "name": "Gain", "module": "", "min": 0.0, "max": 2.0,
                "default": 1.0, "value": 0.5, "stepped": false, "automatable": true,
            }],
        })
    );
}

/// Runs `test` again alone in a child of this test binary whose `HOME`
/// and `CFFIXED_USER_HOME` are a temp folder; answers whether the caller
/// is that child (and so runs the body).
fn in_temp_home(test: &str) -> bool {
    const MARK: &str = "VIZDAW_TEST_TEMP_HOME";
    if let Some(home) = std::env::var_os(MARK) {
        assert_eq!(std::env::var_os("HOME"), Some(home.clone()), "HOME");
        assert_eq!(
            std::env::var_os("CFFIXED_USER_HOME"),
            Some(home),
            "CFFIXED_USER_HOME"
        );
        return true;
    }
    let home = tempfile::tempdir().unwrap();
    let out = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            test,
            "--exact",
            "--include-ignored",
            "--nocapture",
            "--test-threads=1",
        ])
        .env("HOME", home.path())
        .env("CFFIXED_USER_HOME", home.path())
        .env(MARK, home.path())
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    print!("{stdout}");
    eprint!("{}", String::from_utf8_lossy(&out.stderr));
    assert!(out.status.success(), "{test} failed in its temp home");
    assert!(
        stdout.contains("test result: ok. 1 passed"),
        "the child did not run {test} exactly once"
    );
    false
}

#[cfg(target_os = "macos")]
#[test]
fn params_list_a_running_plugin() {
    use std::time::{Duration, Instant};
    use vizdaw_ffi::SlotStatus;
    if !in_temp_home("params_list_a_running_plugin") {
        return;
    }
    let _g = serial();
    let (s, _strip, insert, _eq) = song_with_gain();
    let start = Instant::now();
    while !s
        .plugin_slots()
        .unwrap()
        .iter()
        .any(|r| r.id == insert && r.status == SlotStatus::Live)
    {
        assert!(start.elapsed() < Duration::from_secs(10), "timed out");
        s.idle().unwrap();
        std::thread::sleep(Duration::from_millis(2));
    }
    let row = s.co_producer_params(insert).unwrap();
    assert!(row.running);
    assert_eq!(row.plugin, "clap:vizdaw.test.gain");
    let shown: Vec<ParamRow> = s
        .params(insert)
        .unwrap()
        .into_iter()
        .filter(|p| !p.hidden)
        .collect();
    assert!(!shown.is_empty());
    assert_eq!(row.params, shown);
    // Reading changes nothing.
    let before = s.snapshot().unwrap();
    s.co_producer_params(insert).unwrap();
    let after = s.snapshot().unwrap();
    assert_eq!(
        (before.version, before.dirty, before.undo_label),
        (after.version, after.dirty, after.undo_label)
    );
    s.close().unwrap();
}

// ---- Plan 8d Task 4: mixer commands, their previews and the yes (Rulings
// 1 and 2). Every test that makes a song runs in a child with a temp home:
// its instruments and inserts load real test plug-ins. ----

mod task4 {
    use super::{GAIN_ID, catalog_ref, in_temp_home, serial};
    use core_host::PluginKind;
    use core_model::command::{Command, MixerCmd};
    use core_model::document::{DestKind, Document};
    use core_model::ids::{PluginInstanceId, StripId};
    use core_model::mixer::{AuxSend, Output, ParamId, PluginFormat, PluginRef, StripKind};
    use std::collections::BTreeMap;
    use std::sync::Arc;
    use std::time::{Duration, Instant};
    use vizdaw_ffi::co_producer::{
        CoProducerApproval, NEEDS_APPROVAL, PlannedRow, PreviewRow, USED,
    };
    use vizdaw_ffi::control::{ControlArg, ControlValue};
    use vizdaw_ffi::mixer_chores::{PluginLookup, commands_for, resolve_plugin};
    use vizdaw_ffi::{
        ParamRow, PluginRefRow, Session, SlotStatus, StripKindRow, StripRow, TrackKindRow, VizError,
    };

    const STAMP: &str = "2026-11-17 10-00";
    const CHANGED: &str = "The song changed since the preview; preview again.";
    const SYNTH: &str = "vizdaw.test.synth4";

    fn int(name: &str, value: i64) -> ControlArg {
        ControlArg {
            name: name.into(),
            value: ControlValue::Int { value },
        }
    }

    fn id(name: &str, value: u64) -> ControlArg {
        int(name, value as i64)
    }

    fn text(name: &str, value: &str) -> ControlArg {
        ControlArg {
            name: name.into(),
            value: ControlValue::Text {
                value: value.into(),
            },
        }
    }

    fn flag(name: &str, value: bool) -> ControlArg {
        ControlArg {
            name: name.into(),
            value: ControlValue::Flag { value },
        }
    }

    /// A null `IntOrNull` argument, as the registry hands it on.
    fn null(name: &str) -> ControlArg {
        text(name, core_control::registry::NULL_TEXT)
    }

    fn planned(id: &str, args: Vec<ControlArg>) -> PlannedRow {
        PlannedRow {
            id: id.into(),
            args,
        }
    }

    fn doc(s: &Session) -> Document {
        core_model::file::from_json(&s.debug_document_json()).unwrap()
    }

    /// The document without plug-in state blobs, which a plug-in that has
    /// just started running stores on its own.
    fn shape(s: &Session) -> serde_json::Value {
        fn strip(v: &mut serde_json::Value) {
            match v {
                serde_json::Value::Object(m) => {
                    m.remove("state");
                    m.values_mut().for_each(strip);
                }
                serde_json::Value::Array(a) => a.iter_mut().for_each(strip),
                _ => {}
            }
        }
        let mut v: serde_json::Value = serde_json::from_str(&s.debug_document_json()).unwrap();
        strip(&mut v);
        v
    }

    fn strips(s: &Session) -> Vec<StripRow> {
        s.mixer().unwrap().strips
    }

    fn strip_row(s: &Session, id: u64) -> StripRow {
        strips(s).into_iter().find(|x| x.id == id).unwrap()
    }

    fn strip_id(s: &Session, name: &str) -> u64 {
        strips(s)
            .into_iter()
            .find(|x| x.name == name)
            .unwrap_or_else(|| panic!("no strip {name}"))
            .id
    }

    fn message<T: std::fmt::Debug>(r: Result<T, VizError>) -> String {
        match r {
            Err(e) => e.to_string(),
            Ok(v) => panic!("expected a refusal, got {v:?}"),
        }
    }

    /// The composer's strips Kick, Snare and Bass (plug-in instruments),
    /// a test gain insert on Bass, and Co-Producer's bus Drums, made in a
    /// request of its own; saved.
    struct Song {
        s: Arc<Session>,
        _dir: tempfile::TempDir,
        kick: u64,
        snare: u64,
        bass: u64,
        drums: u64,
        master: u64,
        bass_insert: u64,
        bass_eq: u64,
    }

    fn song() -> Song {
        let dir = tempfile::tempdir().unwrap();
        let s = Session::new_empty().unwrap();
        let synth = PluginRefRow::from(&catalog_ref(PluginFormat::Clap, SYNTH));
        for name in ["Kick", "Snare", "Bass"] {
            s.add_plugin_instrument(name.into(), synth.clone()).unwrap();
        }
        let (kick, snare, bass) = (
            strip_id(&s, "Kick"),
            strip_id(&s, "Snare"),
            strip_id(&s, "Bass"),
        );
        s.add_insert(
            bass,
            None,
            PluginRefRow::from(&catalog_ref(PluginFormat::Clap, GAIN_ID)),
        )
        .unwrap();
        if strip_row(&s, bass).eq.is_none() {
            s.set_eq(bass, Some(PluginRefRow::from(&super::mq())))
                .unwrap();
        }
        let row = strip_row(&s, bass);
        let (bass_insert, bass_eq) = (row.inserts[0].id, row.eq.unwrap().id);
        s.save_as(dir.path().join("P.vizdaw").display().to_string())
            .unwrap();
        s.co_producer_begin("add a drum bus".into()).unwrap();
        s.co_producer_enter(STAMP.into()).unwrap();
        s.add_bus("Drums".into()).unwrap();
        s.co_producer_leave();
        s.co_producer_end().unwrap();
        let drums = strip_id(&s, "Drums (Co-Producer)");
        let master = strips(&s)
            .into_iter()
            .find(|x| x.kind == StripKindRow::Master)
            .unwrap()
            .id;
        Song {
            s,
            _dir: dir,
            kick,
            snare,
            bass,
            drums,
            master,
            bass_insert,
            bass_eq,
        }
    }

    fn wait_live(s: &Session, slot: u64) {
        let start = Instant::now();
        while !s
            .plugin_slots()
            .unwrap()
            .iter()
            .any(|r| r.id == slot && r.status == SlotStatus::Live)
        {
            assert!(
                start.elapsed() < Duration::from_secs(20),
                "slot {slot} never ran"
            );
            s.idle().unwrap();
            std::thread::sleep(Duration::from_millis(2));
        }
    }

    /// One Co-Producer call, as the app's apply makes it: the take with
    /// `approval`, then each command (a bus with a send through
    /// `new_bus_from`, the rest through `run_mixer`).
    fn take_with(
        s: &Session,
        token: String,
        plan: &[PlannedRow],
        approval: CoProducerApproval,
    ) -> Result<(), VizError> {
        s.co_producer_enter(STAMP.into()).unwrap();
        let r = s
            .co_producer_take(token, plan.to_vec(), approval)
            .and_then(|()| {
                for p in plan {
                    if p.id == "mixer.new_bus" {
                        let arg = |n: &str| p.args.iter().find(|a| a.name == n).map(|a| &a.value);
                        let Some(ControlValue::Int { value: strip }) = arg("strip") else {
                            panic!("a bus with a send names its strip");
                        };
                        let name = match arg("name") {
                            Some(ControlValue::Text { value }) => value.clone(),
                            _ => "Bus 1".into(),
                        };
                        s.new_bus_from(*strip as u64, name)?;
                    } else {
                        s.run_mixer(p.id.clone(), p.args.clone())?;
                    }
                }
                Ok(())
            });
        s.co_producer_leave();
        r
    }

    fn take_and_run(s: &Session, token: String, plan: &[PlannedRow]) -> Result<(), VizError> {
        take_with(s, token, plan, CoProducerApproval::Sheet)
    }

    /// Previews `plan`, takes it with the sheet's Allow and runs it.
    fn apply(s: &Session, plan: Vec<PlannedRow>) {
        let p = s.co_producer_preview(plan.clone()).unwrap();
        take_and_run(s, p.token, &plan).unwrap();
    }

    fn items(p: &PreviewRow) -> Vec<(String, String, bool)> {
        p.items
            .iter()
            .map(|i| (i.thing.clone(), i.change.clone(), i.composers_work))
            .collect()
    }

    fn row(id: u64, name: &str, max: f64, value: f64, stepped: bool, hidden: bool) -> ParamRow {
        ParamRow {
            id,
            name: name.into(),
            module: String::new(),
            min: 0.0,
            max,
            default: value,
            value,
            stepped,
            automatable: true,
            hidden,
        }
    }

    /// Resolves through Task 3; answers Gain (0–1, 0.5), a stepped Mode
    /// (0–3) and a hidden Secret for `slot` while `running`.
    struct Fake {
        slot: u64,
        running: bool,
    }

    impl PluginLookup for Fake {
        fn resolve(&self, text: &str, want: PluginKind) -> Result<PluginRef, VizError> {
            resolve_plugin(text, want)
        }
        fn params(&self, slot: PluginInstanceId) -> Option<Vec<ParamRow>> {
            (self.running && slot.0 == self.slot).then(|| {
                vec![
                    row(0, "Gain", 1.0, 0.5, false, false),
                    row(1, "Mode", 3.0, 1.0, true, false),
                    row(2, "Secret", 1.0, 0.0, false, true),
                ]
            })
        }
    }

    fn built(
        d: &Document,
        id: &str,
        args: Vec<ControlArg>,
        host: &dyn PluginLookup,
    ) -> Result<Vec<Command>, VizError> {
        let args = vizdaw_ffi::co_producer::validated(id, &args)?;
        commands_for(d, id, &args, host)
    }

    #[test]
    fn commands_are_the_mixers_own() {
        if !in_temp_home("task4::commands_are_the_mixers_own") {
            return;
        }
        let _g = serial();
        let t = song();
        let fake = Fake {
            slot: t.bass_insert,
            running: true,
        };
        let d = doc(&t.s);
        let (kick, bass, snare, drums) = (
            StripId(t.kick),
            StripId(t.bass),
            StripId(t.snare),
            StripId(t.drums),
        );
        assert_eq!(
            built(
                &d,
                "mixer.set_strip",
                vec![
                    id("strip", t.kick),
                    text("gain_db", "-6"),
                    int("pan", -30),
                    flag("mute", true),
                ],
                &fake
            )
            .unwrap(),
            vec![
                Command::Mixer(MixerCmd::SetGain {
                    strip: kick,
                    gain_db: -6.0
                }),
                Command::Mixer(MixerCmd::SetPan {
                    strip: kick,
                    pan: -0.3
                }),
                Command::Mixer(MixerCmd::SetMute {
                    strip: kick,
                    mute: true
                }),
            ]
        );
        assert_eq!(
            built(
                &d,
                "mixer.set_output",
                vec![id("strip", t.kick), null("bus")],
                &fake
            )
            .unwrap(),
            vec![Command::Mixer(MixerCmd::SetOutput {
                strip: kick,
                output: Output::Master
            })]
        );
        assert_eq!(
            built(
                &d,
                "mixer.add_insert",
                vec![
                    id("strip", t.bass),
                    text("plugin", "clap:vizdaw.test.gain"),
                    int("index", 1)
                ],
                &fake
            )
            .unwrap(),
            vec![Command::Mixer(MixerCmd::AddInsert {
                strip: bass,
                index: Some(0),
                plugin: catalog_ref(PluginFormat::Clap, GAIN_ID),
            })]
        );
        // A send to Drums, pre-fader and off, then only its level set.
        let old = AuxSend {
            target: drums,
            level_db: -10.0,
            pre_fader: true,
            enabled: false,
            co_producer: false,
        };
        let d2 = core_model::command::apply(
            &d,
            &Command::Mixer(MixerCmd::AddSend {
                strip: snare,
                send: old,
            }),
        )
        .unwrap()
        .doc;
        assert_eq!(
            built(
                &d2,
                "mixer.set_send",
                vec![
                    id("strip", t.snare),
                    int("index", 1),
                    text("level_db", "-3")
                ],
                &fake
            )
            .unwrap(),
            vec![Command::Mixer(MixerCmd::EditSend {
                strip: snare,
                index: 0,
                send: AuxSend {
                    level_db: -3.0,
                    ..old
                },
            })]
        );
        assert_eq!(
            built(
                &d,
                "mixer.set_param",
                vec![
                    id("slot", t.bass_insert),
                    int("param", 0),
                    text("value", "0.75")
                ],
                &fake
            )
            .unwrap(),
            vec![Command::Mixer(MixerCmd::SetPluginParams {
                slot: PluginInstanceId(t.bass_insert),
                params: vec![(ParamId(0), 0.75)],
            })]
        );
        t.s.close().unwrap();
    }

    #[test]
    fn the_strip_decides_who_says_yes() {
        if !in_temp_home("task4::the_strip_decides_who_says_yes") {
            return;
        }
        let _g = serial();
        let t = song();
        let s = &t.s;
        s.co_producer_begin("mix the drums".into()).unwrap();
        // Free: Co-Producer's own bus.
        let ours = planned(
            "mixer.add_insert",
            vec![id("strip", t.drums), text("plugin", "multi-q")],
        );
        let p = s.co_producer_preview(vec![ours.clone()]).unwrap();
        assert!(!p.needs_yes, "{p:?}");
        assert!(!p.items.is_empty());
        assert!(p.items.iter().all(|i| !i.composers_work), "{p:?}");
        let p = s
            .co_producer_preview(vec![planned(
                "mixer.set_strip",
                vec![id("strip", t.drums), text("gain_db", "-3")],
            )])
            .unwrap();
        assert!(!p.needs_yes, "{p:?}");
        // Asks: Co-Producer's Drums fed into a bus of the composer's, by
        // its output or a send, changes what that bus plays.
        let theirs = {
            s.add_bus("Hall".into()).unwrap();
            strip_id(s, "Hall")
        };
        for feed in [
            planned(
                "mixer.set_output",
                vec![id("strip", t.drums), id("bus", theirs)],
            ),
            planned(
                "mixer.add_send",
                vec![id("strip", t.drums), id("bus", theirs)],
            ),
        ] {
            let p = s.co_producer_preview(vec![feed.clone()]).unwrap();
            assert!(p.needs_yes, "{}: {p:?}", feed.id);
        }
        // Asks: the composer's Kick routed to it.
        let p = s
            .co_producer_preview(vec![
                ours,
                planned(
                    "mixer.set_output",
                    vec![id("strip", t.kick), id("bus", t.drums)],
                ),
            ])
            .unwrap();
        assert!(p.needs_yes);
        let asks: Vec<_> = items(&p).into_iter().filter(|i| i.2).collect();
        assert_eq!(
            asks,
            vec![(
                "Strip Kick".into(),
                "output Master → Drums (Co-Producer)".into(),
                true
            )]
        );
        // Asks, for Co-Producer's own insert on the composer's Bass.
        let before: Vec<u64> = strip_row(s, t.bass).inserts.iter().map(|x| x.id).collect();
        apply(
            s,
            vec![planned(
                "mixer.add_insert",
                vec![id("strip", t.bass), text("plugin", "clap:vizdaw.test.gain")],
            )],
        );
        let added = strip_row(s, t.bass)
            .inserts
            .iter()
            .map(|x| x.id)
            .find(|x| !before.contains(x))
            .unwrap();
        assert!(
            doc(s)
                .plugin_slot(PluginInstanceId(added))
                .unwrap()
                .co_producer
        );
        let p = s
            .co_producer_preview(vec![planned(
                "mixer.set_bypass",
                vec![id("slot", added), flag("bypass", true)],
            )])
            .unwrap();
        assert!(p.needs_yes, "{p:?}");
        wait_live(s, added);
        let param = s.co_producer_params(added).unwrap().params[0].clone();
        let p = s
            .co_producer_preview(vec![planned(
                "mixer.set_param",
                vec![
                    id("slot", added),
                    id("param", param.id),
                    text("value", &param.min.to_string()),
                ],
            )])
            .unwrap();
        assert!(p.needs_yes, "{p:?}");
        // Asks, for a new bus with a send from the composer's Snare.
        let p = s
            .co_producer_preview(vec![planned(
                "mixer.new_bus",
                vec![id("strip", t.snare), text("name", "Verb")],
            )])
            .unwrap();
        assert!(p.needs_yes);
        let shown = items(&p);
        assert!(
            shown.contains(&("Strip Verb (Co-Producer)".into(), "added".into(), false)),
            "{shown:?}"
        );
        assert!(
            shown.contains(&(
                "Strip Snare".into(),
                "send to Verb (Co-Producer) added (0.0 dB, post-fader)".into(),
                true
            )),
            "{shown:?}"
        );
        s.co_producer_end().unwrap();
        t.s.close().unwrap();
    }

    #[test]
    fn every_solo_change_asks() {
        if !in_temp_home("task4::every_solo_change_asks") {
            return;
        }
        let _g = serial();
        let t = song();
        let s = &t.s;
        s.co_producer_begin("solo the drums".into()).unwrap();
        let solo = |on: bool| {
            vec![planned(
                "mixer.set_strip",
                vec![id("strip", t.drums), flag("solo", on)],
            )]
        };
        // Soloing Co-Producer's own Drums silences the composer's tracks.
        let p = s.co_producer_preview(solo(true)).unwrap();
        assert!(p.needs_yes, "{p:?}");
        assert!(
            items(&p).contains(&(
                "Strip Drums (Co-Producer)".into(),
                "soloed: other tracks are silent while it's on".into(),
                true
            )),
            "{p:?}"
        );
        assert!(p.items.iter().all(|i| i.composers_work), "{p:?}");
        take_and_run(s, p.token, &solo(true)).unwrap();
        assert!(strip_row(s, t.drums).solo);
        // Unsoloing it asks too.
        let p = s.co_producer_preview(solo(false)).unwrap();
        assert!(p.needs_yes, "{p:?}");
        assert!(
            items(&p).contains(&(
                "Strip Drums (Co-Producer)".into(),
                "unsoloed: other tracks play again".into(),
                true
            )),
            "{p:?}"
        );
        take_and_run(s, p.token, &solo(false)).unwrap();
        // A mute of Co-Producer's own strip silences only it: free.
        let p = s
            .co_producer_preview(vec![planned(
                "mixer.set_strip",
                vec![id("strip", t.drums), flag("mute", true)],
            )])
            .unwrap();
        assert!(!p.needs_yes, "{p:?}");
        s.co_producer_end().unwrap();
        t.s.close().unwrap();
    }

    #[test]
    fn a_composers_strip_needs_a_granted_take() {
        if !in_temp_home("task4::a_composers_strip_needs_a_granted_take") {
            return;
        }
        let _g = serial();
        let t = song();
        let s = &t.s;
        s.co_producer_begin("route the kick".into()).unwrap();
        let plan = vec![planned(
            "mixer.set_output",
            vec![id("strip", t.kick), id("bus", t.drums)],
        )];
        // No approval: refused, nothing runs, and the token is used up.
        let p = s.co_producer_preview(plan.clone()).unwrap();
        assert!(p.needs_yes);
        let before = s.debug_document_json();
        let e = take_with(s, p.token.clone(), &plan, CoProducerApproval::None).unwrap_err();
        assert_eq!(e.to_string(), NEEDS_APPROVAL);
        assert_eq!(s.debug_document_json(), before);
        let e = take_and_run(s, p.token, &plan).unwrap_err();
        assert_eq!(e.to_string(), USED);
        assert_eq!(s.debug_document_json(), before);
        assert_eq!(strip_row(s, t.kick).output, None);
        // A granted take runs it.
        let p = s.co_producer_preview(plan.clone()).unwrap();
        take_and_run(s, p.token, &plan).unwrap();
        assert_eq!(strip_row(s, t.kick).output, Some(t.drums));
        // A plan on Co-Producer's own strips needs no approval. (Muting
        // Drums would ask now that Kick feeds it: plan 8e Task 6c.)
        let free = vec![planned(
            "mixer.set_strip",
            vec![id("strip", t.drums), text("gain_db", "-3")],
        )];
        let p = s.co_producer_preview(free.clone()).unwrap();
        assert!(!p.needs_yes);
        take_with(s, p.token, &free, CoProducerApproval::None).unwrap();
        assert_eq!(strip_row(s, t.drums).gain_db, -3.0);
        s.co_producer_end().unwrap();
        t.s.close().unwrap();
    }

    #[test]
    fn removing_a_bus_shows_what_it_fed() {
        if !in_temp_home("task4::removing_a_bus_shows_what_it_fed") {
            return;
        }
        let _g = serial();
        let t = song();
        let s = &t.s;
        s.co_producer_begin("tidy the buses".into()).unwrap();
        apply(
            s,
            vec![planned(
                "mixer.set_output",
                vec![id("strip", t.kick), id("bus", t.drums)],
            )],
        );
        let p = s
            .co_producer_preview(vec![planned(
                "mixer.remove_bus",
                vec![id("strip", t.drums)],
            )])
            .unwrap();
        assert!(p.needs_yes);
        assert_eq!(
            items(&p),
            vec![
                // Plan 8e Task 6c, fix round 1: removing a Co-Producer
                // bus the composer's Kick reaches asks, as a mute does.
                (
                    "Strip Drums (Co-Producer)".into(),
                    "deleted; Kick goes silent through it".into(),
                    true
                ),
                (
                    "Strip Kick".into(),
                    "output Drums (Co-Producer) → Master".into(),
                    true
                ),
            ]
        );
        s.co_producer_end().unwrap();
        t.s.close().unwrap();
    }

    #[test]
    fn mixer_previews_match_what_the_apply_does() {
        if !in_temp_home("task4::mixer_previews_match_what_the_apply_does") {
            return;
        }
        let _g = serial();
        let t = song();
        let s = &t.s;
        wait_live(s, t.bass_insert);
        let gain = s.co_producer_params(t.bass_insert).unwrap().params[0].clone();
        let value = (gain.min + gain.max) / 4.0;
        s.co_producer_begin("mix it".into()).unwrap();
        // Each step's new insert is the one the next steps name.
        let mut slot_of_new = None;
        let ids: Vec<&str> = vec![
            "mixer.set_strip",
            "mixer.set_output",
            "mixer.add_send",
            "mixer.set_send",
            "mixer.add_insert",
            "mixer.move_insert",
            "mixer.set_bypass",
            "mixer.set_eq",
            "mixer.set_param",
            "mixer.remove_insert",
            "mixer.remove_send",
            "mixer.remove_bus",
        ];
        for which in ids {
            let args = match which {
                "mixer.set_strip" => vec![
                    id("strip", t.kick),
                    text("gain_db", "-6"),
                    int("pan", -30),
                    flag("mute", true),
                ],
                "mixer.set_output" => vec![id("strip", t.kick), id("bus", t.drums)],
                "mixer.add_send" => vec![
                    id("strip", t.snare),
                    id("bus", t.drums),
                    text("level_db", "-3"),
                    flag("pre_fader", true),
                ],
                "mixer.set_send" => vec![
                    id("strip", t.snare),
                    int("index", 1),
                    text("level_db", "-6.5"),
                    flag("enabled", false),
                ],
                "mixer.add_insert" => vec![
                    id("strip", t.bass),
                    text("plugin", "clap:vizdaw.test.gain"),
                    int("index", 1),
                ],
                "mixer.move_insert" => vec![id("slot", slot_of_new.unwrap()), int("index", 2)],
                "mixer.set_bypass" => vec![id("slot", t.bass_insert), flag("bypass", true)],
                "mixer.set_eq" => vec![
                    id("strip", t.snare),
                    text("plugin", "clap:vizdaw.test.gain"),
                ],
                "mixer.set_param" => vec![
                    id("slot", t.bass_insert),
                    id("param", gain.id),
                    text("value", &value.to_string()),
                ],
                "mixer.remove_insert" => vec![id("slot", slot_of_new.unwrap())],
                "mixer.remove_send" => vec![id("strip", t.snare), int("index", 1)],
                _ => vec![id("strip", t.drums)],
            };
            let plan = vec![planned(which, args)];
            // A hand edit between the preview and the take.
            let p = s.co_producer_preview(plan.clone()).unwrap();
            s.set_gain(t.master, -1.0).unwrap();
            let before = shape(s);
            let e = take_and_run(s, p.token, &plan).unwrap_err();
            assert_eq!(e.to_string(), CHANGED, "{which}");
            assert_eq!(shape(s), before, "{which}");
            // Previewed again, it does exactly what it said.
            let p = s.co_producer_preview(plan.clone()).unwrap();
            assert!(!p.items.is_empty(), "{which}");
            let params: BTreeMap<u64, Vec<ParamRow>> = [t.bass_insert]
                .iter()
                .map(|x| (*x, s.co_producer_params(*x).unwrap().params))
                .collect();
            let start = doc(s);
            let inserts: Vec<u64> = strip_row(s, t.bass).inserts.iter().map(|x| x.id).collect();
            take_and_run(s, p.token, &plan).unwrap();
            let lookup = |slot: PluginInstanceId, param: ParamId| {
                params
                    .get(&slot.0)?
                    .iter()
                    .find(|r| r.id == param.0)
                    .map(|r| (r.name.clone(), r.value))
            };
            // Plan 8e Task 6c: removing Drums, which Kick still reaches,
            // adds its line to Drums's item.
            let said: Vec<(String, String)> =
                core_model::diff::describe_with(&start, &doc(s), &lookup)
                    .into_iter()
                    .map(|i| {
                        let line = if which == "mixer.remove_bus"
                            && i.thing == "Strip Drums (Co-Producer)"
                        {
                            "; Kick goes silent through it"
                        } else {
                            ""
                        };
                        (i.thing, format!("{}{line}", i.change))
                    })
                    .collect();
            let shown: Vec<(String, String)> =
                p.items.into_iter().map(|i| (i.thing, i.change)).collect();
            assert_eq!(said, shown, "{which}");
            if which == "mixer.add_insert" {
                slot_of_new = strip_row(s, t.bass)
                    .inserts
                    .iter()
                    .map(|x| x.id)
                    .find(|x| !inserts.contains(x));
            }
        }
        // What they did.
        let d = doc(s);
        let m = d.mixer();
        assert_eq!(m.strip(StripId(t.kick)).unwrap().gain_db, -6.0);
        assert_eq!(m.strip(StripId(t.kick)).unwrap().output, Output::Master);
        assert!(m.strip(StripId(t.drums)).is_err());
        assert!(m.strip(StripId(t.snare)).unwrap().sends.is_empty());
        let bass = m.strip(StripId(t.bass)).unwrap();
        assert_eq!(bass.inserts.len(), 1);
        assert!(bass.inserts[0].bypass);
        assert_eq!(bass.inserts[0].params.get(&ParamId(gain.id)), Some(&value));
        s.co_producer_end().unwrap();
        t.s.close().unwrap();
    }

    #[test]
    fn odd_mixer_targets_are_refused_at_preview() {
        if !in_temp_home("task4::odd_mixer_targets_are_refused_at_preview") {
            return;
        }
        let _g = serial();
        let t = song();
        let s = &t.s;
        s.co_producer_begin("route the buses".into()).unwrap();
        // Room feeds Drums: Drums cannot go to Room.
        s.co_producer_enter(STAMP.into()).unwrap();
        s.add_bus("Room".into()).unwrap();
        s.co_producer_leave();
        let room = strip_id(s, "Room (Co-Producer)");
        apply(
            s,
            vec![planned(
                "mixer.set_output",
                vec![id("strip", room), id("bus", t.drums)],
            )],
        );
        let other = Session::new_empty().unwrap();
        while other
            .plugin_slots()
            .unwrap()
            .iter()
            .all(|r| doc(s).plugin_slot(PluginInstanceId(r.id)).is_some())
        {
            other.add_bus("B".into()).unwrap();
        }
        let foreign = other
            .plugin_slots()
            .unwrap()
            .iter()
            .map(|r| r.id)
            .find(|x| doc(s).plugin_slot(PluginInstanceId(*x)).is_none())
            .unwrap();
        let before = s.debug_document_json();
        let cases: Vec<(PlannedRow, String)> = vec![
            (
                planned(
                    "mixer.set_output",
                    vec![id("strip", t.drums), id("bus", room)],
                ),
                "routing cycle".into(),
            ),
            (
                planned(
                    "mixer.add_send",
                    vec![id("strip", t.snare), id("bus", t.kick)],
                ),
                format!("Strip {} is not a bus.", t.kick),
            ),
            (
                planned(
                    "mixer.set_output",
                    vec![id("strip", t.snare), id("bus", t.kick)],
                ),
                format!("Strip {} is not a bus.", t.kick),
            ),
            (
                planned(
                    "mixer.remove_send",
                    vec![id("strip", t.kick), int("index", 9)],
                ),
                "Strip Kick has 0 sends.".into(),
            ),
            (
                planned(
                    "mixer.add_insert",
                    vec![
                        id("strip", t.bass),
                        text("plugin", "clap:vizdaw.test.gain"),
                        int("index", 99),
                    ],
                ),
                "Strip Bass has 1 inserts.".into(),
            ),
            (
                planned(
                    "mixer.move_insert",
                    vec![id("slot", t.bass_insert), int("index", 2)],
                ),
                "Strip Bass has 1 inserts.".into(),
            ),
            (
                planned("mixer.remove_insert", vec![id("slot", t.bass_eq)]),
                format!("Slot {} is not an insert.", t.bass_eq),
            ),
            (
                planned("mixer.remove_bus", vec![id("strip", t.kick)]),
                format!("Strip {} is not a bus.", t.kick),
            ),
            (
                planned(
                    "mixer.set_bypass",
                    vec![id("slot", foreign), flag("bypass", true)],
                ),
                format!("This song has no slot {foreign}."),
            ),
            // The plan runs nothing: a good first command does not stay.
            (
                planned(
                    "mixer.set_strip",
                    vec![id("strip", t.kick), flag("mute", true)],
                ),
                String::new(),
            ),
        ];
        for (p, want) in &cases {
            let mut plan = vec![p.clone()];
            if want.is_empty() {
                plan.push(cases[0].0.clone());
            }
            let e = message(s.co_producer_preview(plan));
            let want = if want.is_empty() {
                "routing cycle"
            } else {
                want
            };
            assert!(e.contains(want), "{}: {e}", p.id);
            assert_eq!(s.debug_document_json(), before, "{}", p.id);
        }
        // Master has no output, no sends, and is not a bus; strip 0 is
        // master in a new song, which the registry already refuses.
        let d = doc(s);
        let fake = Fake {
            slot: t.bass_insert,
            running: true,
        };
        let master = vec![(
            "strip".to_string(),
            core_control::registry::ArgValue::Int(t.master as i64),
        )];
        let null_bus = (
            "bus".to_string(),
            core_control::registry::ArgValue::Text(core_control::registry::NULL_TEXT.into()),
        );
        let bus = (
            "bus".to_string(),
            core_control::registry::ArgValue::Int(t.drums as i64),
        );
        for (which, extra, want) in [
            (
                "mixer.set_output",
                vec![null_bus],
                "Master has no output to set.",
            ),
            ("mixer.add_send", vec![bus], "Master has no sends."),
            ("mixer.remove_bus", vec![], "Master is not a bus."),
        ] {
            let mut args = master.clone();
            args.extend(extra);
            assert_eq!(message(commands_for(&d, which, &args, &fake)), want);
        }
        assert!(
            s.co_producer_preview(vec![planned(
                "mixer.set_output",
                vec![id("strip", t.master), null("bus")]
            )])
            .is_err()
        );
        assert_eq!(s.debug_document_json(), before);
        s.co_producer_end().unwrap();
        other.close().unwrap();
        t.s.close().unwrap();
    }

    #[test]
    fn parameters_need_a_running_plugin() {
        if !in_temp_home("task4::parameters_need_a_running_plugin") {
            return;
        }
        let _g = serial();
        let t = song();
        let d = doc(&t.s);
        let n = t.bass_insert;
        let set = |param: i64, value: &str, running: bool| {
            message(built(
                &d,
                "mixer.set_param",
                vec![id("slot", n), int("param", param), text("value", value)],
                &Fake { slot: n, running },
            ))
        };
        assert_eq!(
            set(0, "0.5", false),
            format!(
                "The plug-in in slot {n} is not running yet; read plugin_params again in a moment."
            )
        );
        assert_eq!(set(0, "2", true), "Gain is 0 to 1.");
        assert_eq!(set(0, "-0.5", true), "Gain is 0 to 1.");
        assert_eq!(set(1, "0.5", true), "Mode takes whole values.");
        assert_eq!(set(2, "0.5", true), format!("Slot {n} has no parameter 2."));
        assert_eq!(set(9, "0.5", true), format!("Slot {n} has no parameter 9."));
        for bad in ["NaN", "1e3", "inf", "+1", "0x10", "1,5", ""] {
            assert!(
                built(
                    &d,
                    "mixer.set_param",
                    vec![id("slot", n), int("param", 0), text("value", bad)],
                    &Fake {
                        slot: n,
                        running: true
                    },
                )
                .is_err(),
                "{bad}"
            );
        }
        // A whole value on the stepped one is fine.
        assert!(
            built(
                &d,
                "mixer.set_param",
                vec![id("slot", n), int("param", 1), text("value", "2")],
                &Fake {
                    slot: n,
                    running: true
                },
            )
            .is_ok()
        );
        t.s.close().unwrap();
    }

    #[test]
    fn run_mixer_is_one_step() {
        if !in_temp_home("task4::run_mixer_is_one_step") {
            return;
        }
        let _g = serial();
        let t = song();
        let s = &t.s;
        let set = || {
            s.run_mixer(
                "mixer.set_strip".into(),
                vec![
                    id("strip", t.kick),
                    text("gain_db", "-6"),
                    int("pan", 20),
                    flag("mute", true),
                ],
            )
        };
        set().unwrap();
        let k = strip_row(s, t.kick);
        assert_eq!((k.gain_db, k.pan, k.mute), (-6.0, 0.2, true));
        assert_eq!(
            s.snapshot().unwrap().undo_label.as_deref(),
            Some("Set Strip")
        );
        s.undo().unwrap();
        let k = strip_row(s, t.kick);
        assert_eq!((k.gain_db, k.pan, k.mute), (0.0, 0.0, false));
        // Inside a request it joins the request's step.
        let r = s.co_producer_begin("balance the kick".into()).unwrap();
        s.co_producer_enter(STAMP.into()).unwrap();
        set().unwrap();
        s.co_producer_leave();
        assert_eq!(s.co_producer_end().unwrap(), Some(r.label.clone()));
        assert!(r.label.starts_with("Co-Producer: "), "{}", r.label);
        assert_eq!(s.snapshot().unwrap().undo_label, Some(r.label));
        s.undo().unwrap();
        assert!(!strip_row(s, t.kick).mute);
        // Busy while an edit is open.
        s.begin_edit("Drag".into()).unwrap();
        assert!(matches!(set(), Err(VizError::Busy { .. })));
        s.end_edit().unwrap();
        t.s.close().unwrap();
    }

    #[test]
    fn new_plugin_instruments_are_marked() {
        if !in_temp_home("task4::new_plugin_instruments_are_marked") {
            return;
        }
        let _g = serial();
        let t = song();
        let s = &t.s;
        let seq = s.snapshot().unwrap().sequences[0].id;
        s.co_producer_begin("add a synth".into()).unwrap();
        s.co_producer_enter(STAMP.into()).unwrap();
        let before = doc(s);
        s.run_mixer(
            "track.new_plugin_instrument".into(),
            vec![id("seq", seq), text("plugin", "clap:vizdaw.test.synth4")],
        )
        .unwrap();
        let after = doc(s);
        let synth = catalog_ref(PluginFormat::Clap, SYNTH);
        let dests: Vec<_> = after
            .instruments()
            .values()
            .filter(|d| !before.instruments().contains_key(&d.id))
            .collect();
        assert_eq!(dests.len(), 1);
        assert_eq!(dests[0].name, synth.name);
        assert!(
            matches!(&dests[0].kind, DestKind::PluginInstrument { slot } if slot.plugin == synth)
        );
        let seq_id = core_model::ids::SeqId(seq);
        let lanes: Vec<_> = after
            .sequence(seq_id)
            .unwrap()
            .lanes
            .iter()
            .filter(|l| before.sequence(seq_id).unwrap().lane(l.id).is_none())
            .collect();
        assert_eq!(lanes.len(), 1);
        assert_eq!(lanes[0].midi_dest, Some(dests[0].id));
        assert!(lanes[0].co_producer);
        assert!(
            lanes[0].name.ends_with(" (Co-Producer)"),
            "{}",
            lanes[0].name
        );
        let strip = after
            .mixer()
            .strip(
                after
                    .mixer()
                    .find_kind(StripKind::Instrument(dests[0].id))
                    .unwrap(),
            )
            .unwrap();
        assert!(strip.co_producer);
        assert_eq!(strip.name, format!("{} (Co-Producer)", synth.name));
        let kind = s
            .lanes(seq)
            .unwrap()
            .into_iter()
            .find(|l| l.id == lanes[0].id.0)
            .unwrap()
            .kind;
        assert_eq!(kind, TrackKindRow::Instrument);

        // The Built-in Synth: the track track.new_instrument makes.
        s.run_mixer(
            "track.new_plugin_instrument".into(),
            vec![id("seq", seq), text("plugin", "built-in-synth")],
        )
        .unwrap();
        let ours = doc(s);
        let made = ours
            .sequence(seq_id)
            .unwrap()
            .lanes
            .iter()
            .find(|l| after.sequence(seq_id).unwrap().lane(l.id).is_none())
            .unwrap()
            .clone();
        // Multi-Q is not an instrument; nothing is added.
        let json = s.debug_document_json();
        assert_eq!(
            message(s.run_mixer(
                "track.new_plugin_instrument".into(),
                vec![id("seq", seq), text("plugin", "multi-q")],
            )),
            "Multi-Q is an effect, not an instrument."
        );
        assert_eq!(s.debug_document_json(), json);
        s.co_producer_leave();
        s.co_producer_end().unwrap();
        s.new_track(seq, TrackKindRow::Instrument, None).unwrap();
        let hand = doc(s)
            .sequence(seq_id)
            .unwrap()
            .lanes
            .iter()
            .find(|l| ours.sequence(seq_id).unwrap().lane(l.id).is_none())
            .unwrap()
            .clone();
        assert_eq!(made.midi_dest, hand.midi_dest);
        assert!(
            doc(s).instruments()[&made.midi_dest.unwrap()]
                .kind
                .is_builtin_synth()
        );
        assert_eq!(
            (made.input, made.monitor, made.audio_out),
            (hand.input, hand.monitor, hand.audio_out)
        );
        assert!(
            made.name.starts_with("Inst ") && made.co_producer,
            "{}",
            made.name
        );
        t.s.close().unwrap();
    }

    #[test]
    fn run_mixer_refuses_hostile_input() {
        if !in_temp_home("task4::run_mixer_refuses_hostile_input") {
            return;
        }
        let _g = serial();
        let t = song();
        let s = &t.s;
        let long = format!("clap:{}", "x".repeat(70_000));
        let before = s.debug_document_json();
        let cases: Vec<(&str, Vec<ControlArg>)> = vec![
            ("mixer.nope", vec![id("strip", t.kick)]),
            ("mixer.new_bus", vec![id("strip", t.kick)]),
            (
                "mixer.set_colour",
                vec![id("strip", t.kick), text("colour", "red")],
            ),
            ("mixer.set_strip", vec![text("gain_db", "-6")]),
            ("mixer.set_strip", vec![id("strip", t.kick)]),
            (
                "mixer.set_strip",
                vec![int("strip", u64::MAX as i64), text("gain_db", "-6")],
            ),
            (
                "mixer.set_strip",
                vec![int("strip", (1 << 53) - 1), text("gain_db", "-6")],
            ),
            (
                "mixer.set_strip",
                vec![text("strip", "1"), text("gain_db", "-6")],
            ),
            (
                "mixer.set_strip",
                vec![id("strip", t.kick), text("gain_db", "1e3")],
            ),
            (
                "mixer.set_strip",
                vec![id("strip", t.kick), text("gain_db", "13")],
            ),
            (
                "mixer.set_strip",
                vec![id("strip", t.kick), flag("mute", true), int("extra", 1)],
            ),
            (
                "mixer.add_insert",
                vec![id("strip", t.bass), text("plugin", &long)],
            ),
            (
                "mixer.add_insert",
                vec![id("strip", t.bass), text("plugin", "clap:a\0b")],
            ),
            (
                "mixer.add_insert",
                vec![
                    id("strip", t.bass),
                    text("plugin", "clap:vizdaw.test.synth4"),
                ],
            ),
            (
                "mixer.add_insert",
                vec![id("strip", t.bass), text("plugin", "built-in-synth")],
            ),
            (
                "mixer.set_eq",
                vec![id("strip", t.bass), text("plugin", "clap:vizdaw.test.none")],
            ),
            ("mixer.remove_insert", vec![id("slot", t.bass_eq)]),
            (
                "mixer.set_param",
                vec![id("slot", t.bass_eq), int("param", 0), text("value", "NaN")],
            ),
            ("mixer.set_send", vec![id("strip", t.kick), int("index", 1)]),
            (
                "mixer.add_send",
                vec![
                    id("strip", t.kick),
                    id("bus", t.drums),
                    text("level_db", "-1e3"),
                ],
            ),
            ("track.new_plugin_instrument", vec![text("plugin", &long)]),
            (
                "track.new_plugin_instrument",
                vec![int("seq", 999), text("plugin", "built-in-synth")],
            ),
            (
                "track.new_plugin_instrument",
                vec![text("plugin", "clap:vizdaw.test.gain")],
            ),
        ];
        for (which, args) in cases {
            let r = s.run_mixer(which.into(), args.clone());
            assert!(r.is_err(), "{which} {args:?}");
            assert_eq!(s.debug_document_json(), before, "{which}");
        }
        let shown = message(s.run_mixer(format!("mixer.{}", "x".repeat(70_000)), vec![]));
        assert!(shown.chars().count() < 200, "{shown}");
        t.s.close().unwrap();
    }
}
