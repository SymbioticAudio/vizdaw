use core_model::ModelError;
use core_model::command::{Command, EventCmd, SequenceCmd, SettingsCmd, apply};
use core_model::document::Document;
use core_model::event::{EventBody, EventSpec};
use core_model::file::{
    FORMAT, Migration, VERSION, autosave, from_json, list_autosaves, load, run_migrations, save,
    to_json,
};
use core_model::history::Timestamp;
use core_model::ids::{LaneId, SeqId};
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
        .join("tests/fixtures")
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
    let d = from_json(&text).unwrap();
    // Canonical: re-serializing reproduces the fixture byte for byte.
    assert_eq!(to_json(&d).unwrap() + "\n", text);
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
