use core_model::ModelError;
use core_model::command::{Command, EventCmd, SequenceCmd, SettingsCmd, apply};
use core_model::document::{DestKind, Document};
use core_model::event::{EventBody, EventSpec};
use core_model::file::{
    FORMAT, Migration, VERSION, autosave, from_json, list_autosaves, load, run_migrations, save,
    to_json,
};
use core_model::history::Timestamp;
use core_model::ids::{DestId, LaneId, OutputPathId, PluginInstanceId, SeqId, StripId};
use core_model::mixer::{Output, PluginFormat, PluginRef, StripKind};
use core_model::query::{flatten, seq_length};
use core_model::selection::ViewFilter;
use core_model::time::{PPQ, Tick};
use serde_json::{Value, json};
use std::path::PathBuf;

/// A fresh, empty directory under the system temp dir.
fn temp_package(tag: &str) -> PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let p = std::env::temp_dir().join(format!(
        "vizdaw-test-{tag}-{}-{nanos}.vizdaw",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&p);
    p
}

fn fixture(name: &str) -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/v1")
        .join(format!("{name}.json"));
    std::fs::read_to_string(path).unwrap()
}

fn small_doc() -> Document {
    let d = apply(
        &Document::new(),
        &Command::Sequence(SequenceCmd::New { name: "S".into() }),
    )
    .unwrap()
    .doc;
    apply(
        &d,
        &Command::Event(EventCmd::Insert {
            seq: SeqId(0),
            lane: LaneId(0),
            events: vec![EventSpec::new(Tick(0), EventBody::note(60, 1, Tick(PPQ)))],
        }),
    )
    .unwrap()
    .doc
}

#[test]
fn save_then_load_is_identical_and_leaves_no_temp_file() {
    let pkg = temp_package("save");
    let d = small_doc();
    save(&d, &pkg).unwrap();
    assert!(pkg.join("Media").is_dir());
    assert!(pkg.join("Autosave").is_dir());
    assert!(!pkg.join("project.json.tmp").exists());
    assert_eq!(load(&pkg).unwrap(), d);
    // Saving again replaces the file atomically.
    let d2 = apply(
        &d,
        &Command::Sequence(SequenceCmd::Rename {
            seq: SeqId(0),
            name: "T".into(),
        }),
    )
    .unwrap()
    .doc;
    save(&d2, &pkg).unwrap();
    assert_eq!(load(&pkg).unwrap(), d2);
    std::fs::remove_dir_all(&pkg).unwrap();
}

#[test]
fn top_level_fields_and_newer_version() {
    let text = to_json(&small_doc()).unwrap();
    let mut v: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(v["format"], FORMAT);
    assert_eq!(v["version"], VERSION);
    v["version"] = json!(VERSION + 1);
    assert_eq!(
        from_json(&v.to_string()),
        Err(ModelError::NewerFormat { found: VERSION + 1 })
    );
    v["version"] = json!(0);
    assert!(matches!(
        from_json(&v.to_string()),
        Err(ModelError::Corrupt { .. })
    ));
    v["version"] = json!(VERSION);
    v["format"] = json!("something-else");
    assert!(matches!(
        from_json(&v.to_string()),
        Err(ModelError::Corrupt { .. })
    ));
    assert!(matches!(
        from_json("not json"),
        Err(ModelError::Corrupt { .. })
    ));
}

#[test]
fn unknown_fields_are_rejected_at_every_level() {
    let text = to_json(&small_doc()).unwrap();
    for pointer in [
        "",
        "/document",
        "/document/mixer",
        "/document/mixer/strips/0",
        "/document/sequences/0",
        "/document/sequences/0/lanes/0/takes/0/events/0/body",
    ] {
        let mut v: Value = serde_json::from_str(&text).unwrap();
        v.pointer_mut(pointer)
            .unwrap()
            .as_object_mut()
            .unwrap()
            .insert("extra".into(), json!(1));
        assert!(
            matches!(from_json(&v.to_string()), Err(ModelError::Corrupt { .. })),
            "{pointer}"
        );
    }
}

#[test]
fn loading_reports_every_integrity_problem() {
    let text = to_json(&small_doc()).unwrap();
    let mut v: Value = serde_json::from_str(&text).unwrap();
    let ev = v
        .pointer_mut("/document/sequences/0/lanes/0/takes/0/events/0")
        .unwrap();
    ev["body"]["pitch"] = json!(130);
    ev["dest"] = json!(42);
    v["document"]["sequences"][0]["meter"]["entries"]
        .as_array_mut()
        .unwrap()
        .push(json!({"tick": 100, "meter": {"num": 3, "den": 4}, "key": {"sharps": 0, "minor": false}}));
    match from_json(&v.to_string()) {
        Err(ModelError::Corrupt { problems }) => {
            assert_eq!(problems.len(), 2, "{problems:?}");
            assert!(problems.iter().any(|p| p.contains("barline")));
            assert!(problems.iter().any(|p| p.contains("dest42")));
        }
        other => panic!("expected Corrupt, got {other:?}"),
    }
}

#[test]
fn migrations_run_step_by_step() {
    fn add_a(mut v: Value) -> core_model::Result<Value> {
        v["a"] = json!(true);
        Ok(v)
    }
    fn add_b(mut v: Value) -> core_model::Result<Value> {
        assert_eq!(v["version"], 2);
        v["b"] = json!(true);
        Ok(v)
    }
    let steps: &[Migration] = &[add_a, add_b];
    let out = run_migrations(json!({"version": 1}), 1, 3, 1, steps).unwrap();
    assert_eq!(out, json!({"version": 3, "a": true, "b": true}));
    let out = run_migrations(json!({"version": 2}), 2, 3, 1, steps).unwrap();
    assert_eq!(out, json!({"version": 3, "b": true}));
    assert!(run_migrations(json!({}), 1, 4, 1, steps).is_err());
    assert!(run_migrations(json!({}), 0, 3, 1, steps).is_err());
}

#[test]
fn autosave_keeps_the_newest_n() {
    let pkg = temp_package("autosave");
    let d = apply(
        &small_doc(),
        &Command::Settings(SettingsCmd::SetAutosaveKeep { keep: 3 }),
    )
    .unwrap()
    .doc;
    for ms in [5_u64, 1, 4, 2, 3] {
        autosave(&d, &pkg, Timestamp(ms * 1000)).unwrap();
    }
    let names: Vec<String> = list_autosaves(&pkg)
        .unwrap()
        .iter()
        .map(|p| p.file_name().unwrap().to_string_lossy().into_owned())
        .collect();
    assert_eq!(
        names,
        vec![
            "project-00000000000000005000.json",
            "project-00000000000000004000.json",
            "project-00000000000000003000.json"
        ]
    );
    // Same timestamp twice does not overwrite.
    let p = autosave(&d, &pkg, Timestamp(5000)).unwrap();
    assert!(p.ends_with("project-00000000000000005001.json"));
    assert_eq!(from_json(&std::fs::read_to_string(&p).unwrap()).unwrap(), d);
    std::fs::remove_dir_all(&pkg).unwrap();
}

/// A readable summary of a fixture: lengths and the flattened first sequence.
fn summary(d: &Document) -> Value {
    let lengths: Vec<Value> = d
        .sequences()
        .values()
        .map(|s| json!([s.name, seq_length(d, s.id).unwrap().0]))
        .collect();
    let first = *d.sequences().keys().next().unwrap();
    let events: Vec<String> = flatten(d, first, None, &ViewFilter::default())
        .unwrap()
        .map(|r| {
            let what = match &r.body {
                EventBody::Note { pitch, .. } => format!("note {pitch}"),
                other => format!("{:?}", other.kind()),
            };
            format!(
                "{} {} dest={:?} gc={:?} out={:?}",
                r.tick.0,
                what,
                r.dest.map(|x| x.0),
                r.group_channel,
                r.audio_out.map(|x| x.0)
            )
        })
        .collect();
    json!({ "lengths": lengths, "flattened": events })
}

fn golden(name: &str) {
    let text = fixture(name);
    // Version-1 input: loading migrates it.
    let d = from_json(&text).unwrap();
    let v2 = to_json(&d).unwrap();
    assert!(v2.contains("\"version\": 2"), "{name}");
    // Canonical: a version-2 file re-serializes byte for byte.
    assert_eq!(to_json(&from_json(&v2).unwrap()).unwrap(), v2, "{name}");
    insta::assert_json_snapshot!(name, summary(&d));
}

#[test]
fn golden_minimal() {
    golden("minimal");
}

#[test]
fn golden_nested_references() {
    golden("nested_references");
}

#[test]
fn golden_mixed_lanes() {
    golden("mixed_lanes");
}

#[test]
fn golden_drum_lane() {
    golden("drum_lane");
}

/// Insert a bogus field into every object matching `pick`, return whether any changed.
fn poison(v: &mut Value, pick: &dyn Fn(&serde_json::Map<String, Value>) -> bool) -> bool {
    let mut hit = false;
    match v {
        Value::Object(m) => {
            if pick(m) {
                m.insert("bogus".into(), json!(1));
                hit = true;
            }
            for c in m.values_mut() {
                hit |= poison(c, pick);
            }
        }
        Value::Array(a) => {
            for c in a {
                hit |= poison(c, pick);
            }
        }
        _ => {}
    }
    hit
}

#[test]
fn unknown_fields_inside_persisted_enums_are_rejected() {
    use core_model::modify::{Modify, ModifyTemplate, Quantize, Transpose};
    use core_model::rule::Rule;
    use core_model::rule::SelectMode;
    use core_model::sequence::{GeneratorParams, SeqKind};
    let d = small_doc();
    let d = apply(
        &d,
        &Command::Sequence(SequenceCmd::SetKind {
            seq: SeqId(0),
            kind: SeqKind::Generated {
                generator: GeneratorParams {
                    generator: "g".into(),
                    params: Default::default(),
                },
            },
        }),
    )
    .unwrap()
    .doc;
    let t = ModifyTemplate {
        rule: Some(Rule::Not(Box::new(Rule::TimeRange {
            start: Tick(0),
            end: Tick(10),
        }))),
        mode: SelectMode::Select,
        modify: Some(Modify::Quantize(Quantize::Grid {
            grid: Tick(PPQ),
            strength: 100,
        })),
    };
    let d = apply(
        &d,
        &Command::Settings(SettingsCmd::SetTemplate {
            name: "T".into(),
            template: Some(t),
        }),
    )
    .unwrap()
    .doc;
    let t2 = ModifyTemplate {
        rule: None,
        mode: SelectMode::Add,
        modify: Some(Modify::Transpose(Transpose::Interval { semitones: 1 })),
    };
    let d = apply(
        &d,
        &Command::Settings(SettingsCmd::SetTemplate {
            name: "U".into(),
            template: Some(t2),
        }),
    )
    .unwrap()
    .doc;
    let base: Value = serde_json::from_str(&to_json(&d).unwrap()).unwrap();
    assert!(from_json(&serde_json::to_string(&base).unwrap()).is_ok());
    type Pick = Box<dyn Fn(&serde_json::Map<String, Value>) -> bool>;
    let picks: Vec<(&str, Pick)> = vec![
        (
            "generated",
            Box::new(|m| m.get("generator").is_some_and(Value::is_object)),
        ),
        (
            "rule",
            Box::new(|m| m.contains_key("start") && m.contains_key("end")),
        ),
        (
            "quantize",
            Box::new(|m| m.contains_key("grid") && m.contains_key("strength")),
        ),
        ("transpose", Box::new(|m| m.contains_key("semitones"))),
    ];
    for (name, pick) in picks {
        let mut v = base.clone();
        assert!(poison(&mut v, &*pick), "{name}: nothing matched");
        assert!(
            from_json(&serde_json::to_string(&v).unwrap()).is_err(),
            "{name}: bogus field accepted"
        );
    }
}

#[test]
fn v1_plugin_instruments_migrate_to_slots_and_strips() {
    let d = from_json(&fixture("mixed_lanes")).unwrap();
    let DestKind::PluginInstrument { slot } = &d.instruments().get(&DestId(0)).unwrap().kind else {
        panic!("dest0 is not a plugin instrument");
    };
    assert_eq!(slot.id, PluginInstanceId(0));
    assert_eq!(slot.plugin, PluginRef::from_legacy("au:piano"));
    assert_eq!(slot.plugin.format, PluginFormat::Au);
    assert!(slot.params.is_empty() && slot.state.is_none() && !slot.bypass);
    let strips: Vec<(StripId, StripKind, String)> = d
        .mixer()
        .strips()
        .values()
        .map(|s| (s.id, s.kind, s.name.clone()))
        .collect();
    assert_eq!(
        strips,
        vec![
            (StripId(0), StripKind::Master, "Master".to_string()),
            (
                StripId(1),
                StripKind::Instrument(DestId(0)),
                "Piano".to_string()
            ),
            (
                StripId(2),
                StripKind::AudioPath(OutputPathId(0)),
                "Main".to_string()
            ),
        ]
    );
    assert!(
        d.mixer()
            .strips()
            .values()
            .all(|s| s.output == Output::Master)
    );
    assert_eq!((d.counters().strip, d.counters().plugin), (3, 1));
    let v: Value = serde_json::from_str(&to_json(&d).unwrap()).unwrap();
    assert_eq!(v["version"], 2);
    assert_eq!(
        v["document"]["instruments"][0]["kind"]["plugin_instrument"]["slot"]["plugin"]["format"],
        "au"
    );

    // drum_lane: dest0 is an external endpoint, dest1 the plugin "clap:perc".
    let d = from_json(&fixture("drum_lane")).unwrap();
    assert_eq!(
        d.mixer().find_kind(StripKind::Instrument(DestId(1))),
        Some(StripId(1))
    );
    assert_eq!(d.mixer().find_kind(StripKind::BuiltinSynth), None);
    assert_eq!(
        d.plugin_slot(PluginInstanceId(0)).unwrap().plugin.id,
        "clap:perc"
    );
}
