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
#[cfg(target_os = "macos")]
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
