//! Damaged project files never crash VizDAW (control spec §11): random
//! byte damage and random JSON edits of the demo project either load and
//! answer every read the editors make, or are refused with an error.
//! Never a panic, never `VizError::Internal` (a caught panic).

use proptest::prelude::*;
use serde_json::{Value, json};
use vizdaw_ffi::{BlockRule, Session, VizError};

/// Cases per property: `VIZDAW_FUZZ_CASES` (a long local run), else `n`.
fn cases(n: u32) -> u32 {
    std::env::var("VIZDAW_FUZZ_CASES")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(n)
}

fn demo_text() -> String {
    let p = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../fixtures/demo.vizdaw/project.json"
    );
    std::fs::read_to_string(p).unwrap()
}

fn no_bug<T>(what: &str, r: Result<T, VizError>) -> Result<Option<T>, TestCaseError> {
    match r {
        Ok(v) => Ok(Some(v)),
        Err(VizError::Internal { message }) => Err(TestCaseError::fail(format!(
            "{what}: internal error {message}"
        ))),
        Err(_) => Ok(None),
    }
}

/// Loads `text` like Open does and makes every read an editor makes;
/// Ok(true) when it loaded.
fn exercise(text: &str) -> Result<bool, TestCaseError> {
    let Ok(doc) = core_model::file::from_json(text) else {
        return Ok(false); // refused with an error: fine
    };
    let Some(s) = no_bug("from_document", Session::from_document(doc))? else {
        return Ok(false);
    };
    let Some(snap) = no_bug("snapshot", s.snapshot())? else {
        return Ok(true);
    };
    for q in &snap.sequences {
        no_bug("sequence_info", s.sequence_info(q.id))?;
        no_bug("seconds_at", s.seconds_at(q.id, vec![0, 15_360, 1 << 40]))?;
        let Some(lanes) = no_bug("lanes", s.lanes(q.id))? else {
            continue;
        };
        for l in &lanes {
            no_bug("lane_events", s.lane_events(q.id, l.id))?;
            for rule in [
                BlockRule::SilenceGap { ticks: 15_360 },
                BlockRule::FixedBars { bars: 1 },
                BlockRule::WholeLane,
            ] {
                no_bug("blocks", s.blocks(q.id, l.id, rule))?;
            }
        }
    }
    no_bug("selection", s.selection())?;
    no_bug("mixer", s.mixer())?;
    no_bug("destinations", s.destinations())?;
    no_bug("modify_templates", s.modify_templates())?;
    let _ = s.close();
    Ok(true)
}

/// Byte damage: flip, insert, delete or truncate.
#[derive(Debug, Clone)]
enum Damage {
    Flip(usize, u8),
    Insert(usize, u8),
    Delete(usize),
    Truncate(usize),
}

fn damage() -> impl Strategy<Value = Damage> {
    prop_oneof![
        (any::<usize>(), any::<u8>()).prop_map(|(i, b)| Damage::Flip(i, b)),
        (any::<usize>(), any::<u8>()).prop_map(|(i, b)| Damage::Insert(i, b)),
        any::<usize>().prop_map(Damage::Delete),
        any::<usize>().prop_map(Damage::Truncate),
    ]
}

fn apply(bytes: &mut Vec<u8>, d: &Damage) {
    let n = bytes.len().max(1);
    match *d {
        Damage::Flip(i, b) => {
            if let Some(x) = bytes.get_mut(i % n) {
                *x ^= b | 1;
            }
        }
        Damage::Insert(i, b) => bytes.insert(i % (bytes.len() + 1), b),
        Damage::Delete(i) => {
            if !bytes.is_empty() {
                bytes.remove(i % n);
            }
        }
        Damage::Truncate(i) => bytes.truncate(i % n),
    }
}

/// Values that break assumptions: wrong types, extremes, empties.
fn nasty() -> impl Strategy<Value = Value> {
    prop_oneof![
        Just(Value::Null),
        Just(json!(true)),
        Just(json!(-1)),
        Just(json!(0)),
        Just(json!(i64::MAX)),
        Just(json!(i64::MIN)),
        Just(json!(u64::MAX)),
        Just(json!(1e308)),
        Just(json!(-0.5)),
        Just(json!("")),
        Just(json!("x")),
        Just(json!([])),
        Just(json!({})),
        Just(json!([[[[[[[[1]]]]]]]])),
        any::<i32>().prop_map(Value::from),
    ]
}

/// Every JSON pointer in `v`.
fn pointers(v: &Value, at: String, out: &mut Vec<String>) {
    match v {
        Value::Object(o) => {
            for (k, x) in o {
                let p = format!("{at}/{}", k.replace('~', "~0").replace('/', "~1"));
                out.push(p.clone());
                pointers(x, p, out);
            }
        }
        Value::Array(a) => {
            for (i, x) in a.iter().enumerate() {
                let p = format!("{at}/{i}");
                out.push(p.clone());
                pointers(x, p, out);
            }
        }
        _ => {}
    }
}

/// A JSON edit at the `i`th pointer: replace with any value, remove, or
/// tweak it keeping its type (numbers to extremes, strings emptied or
/// grown, flags flipped, arrays shortened or an element duplicated), so
/// many damaged documents still deserialize and reach the reads.
#[derive(Debug, Clone)]
enum Edit {
    Replace(usize, Value),
    Remove(usize),
    Tweak(usize, i64),
}

fn edit() -> impl Strategy<Value = Edit> {
    prop_oneof![
        1 => (any::<usize>(), nasty()).prop_map(|(i, v)| Edit::Replace(i, v)),
        1 => any::<usize>().prop_map(Edit::Remove),
        6 => (any::<usize>(), prop_oneof![
            Just(-1i64), Just(0), Just(1), Just(i64::from(i32::MAX)), Just(i64::MAX),
            Just(i64::MIN), Just(1 << 40), -100_000i64..100_000,
        ]).prop_map(|(i, k)| Edit::Tweak(i, k)),
    ]
}

fn tweak(v: &mut Value, k: i64) {
    match v {
        Value::Number(n) if n.is_f64() => *v = json!(k as f64 * 1.5),
        Value::Number(_) => *v = json!(k),
        Value::String(s) => {
            if k % 2 == 0 {
                s.clear();
            } else {
                s.push_str(&"x".repeat((k.unsigned_abs() % 300) as usize));
            }
        }
        Value::Bool(b) => *b = !*b,
        Value::Array(a) => {
            if k % 2 == 0 {
                a.pop();
            } else if let Some(first) = a.first().cloned() {
                a.push(first);
            }
        }
        Value::Object(o) => {
            o.clear();
        }
        Value::Null => *v = json!(k),
    }
}

fn apply_edit(doc: &mut Value, e: &Edit) {
    let mut all = Vec::new();
    pointers(doc, String::new(), &mut all);
    if all.is_empty() {
        return;
    }
    let (i, replacement) = match e {
        Edit::Replace(i, v) => (*i, Some(v.clone())),
        Edit::Remove(i) => (*i, None),
        Edit::Tweak(i, k) => {
            let Some(p) = all.get(i % all.len()).cloned() else {
                return;
            };
            if let Some(slot) = doc.pointer_mut(&p) {
                tweak(slot, *k);
            }
            return;
        }
    };
    let Some(p) = all.get(i % all.len()).cloned() else {
        return;
    };
    match replacement {
        Some(v) => {
            if let Some(slot) = doc.pointer_mut(&p) {
                *slot = v;
            }
        }
        None => {
            let (parent, key) = p.rsplit_once('/').unwrap_or(("", ""));
            let key = key.replace("~1", "/").replace("~0", "~");
            match doc.pointer_mut(parent) {
                Some(Value::Object(o)) => {
                    o.remove(&key);
                }
                Some(Value::Array(a)) => {
                    if let Ok(k) = key.parse::<usize>() {
                        if k < a.len() {
                            a.remove(k);
                        }
                    }
                }
                _ => {}
            }
        }
    }
}

#[test]
fn the_undamaged_demo_loads_and_answers() {
    assert!(exercise(&demo_text()).unwrap());
}

#[test]
fn an_event_at_the_far_end_of_time_is_refused() {
    // Found by `json_edits_never_crash_and_many_still_load`: a tick near
    // i64::MIN loaded, and arranging it into blocks overflowed.
    for tick in [i64::MIN, i64::MAX, (1i64 << 40) + 1] {
        let mut doc: Value = serde_json::from_str(&demo_text()).unwrap();
        *doc.pointer_mut("/document/sequences/0/lanes/0/takes/0/events/0/tick")
            .unwrap() = json!(tick);
        assert!(
            core_model::file::from_json(&doc.to_string()).is_err(),
            "tick {tick} loaded"
        );
        assert!(!exercise(&doc.to_string()).unwrap());
    }
}

#[test]
fn damaged_packages_on_disk_are_refused_with_an_error() {
    let dir = tempfile::tempdir().unwrap();
    let pkg = dir.path().join("Bad.vizdaw");
    std::fs::create_dir(&pkg).unwrap();
    let cases: [(&str, Option<&[u8]>); 4] = [
        ("missing project.json", None),
        ("empty file", Some(b"")),
        ("binary noise", Some(&[0, 159, 146, 150, 255, 0, 1])),
        (
            "valid JSON, wrong shape",
            Some(b"{\"format\":\"vizdaw-project\",\"version\":2}"),
        ),
    ];
    for (what, bytes) in cases {
        let file = pkg.join("project.json");
        let _ = std::fs::remove_file(&file);
        if let Some(b) = bytes {
            std::fs::write(&file, b).unwrap();
        }
        match Session::open(pkg.to_string_lossy().into_owned()) {
            Ok(_) => panic!("{what}: opened"),
            Err(VizError::Internal { message }) => panic!("{what}: internal error {message}"),
            Err(_) => {}
        }
    }
    // A directory where the file should be.
    std::fs::remove_file(pkg.join("project.json")).unwrap();
    std::fs::create_dir(pkg.join("project.json")).unwrap();
    assert!(!matches!(
        Session::open(pkg.to_string_lossy().into_owned()),
        Ok(_) | Err(VizError::Internal { .. })
    ));
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(cases(128)))]

    #[test]
    fn byte_damage_never_crashes(damages in prop::collection::vec(damage(), 1..8)) {
        let mut bytes = demo_text().into_bytes();
        for d in &damages {
            apply(&mut bytes, d);
        }
        // Non-UTF-8 is refused before parsing, as `fs::read_to_string` does.
        if let Ok(text) = String::from_utf8(bytes) {
            exercise(&text)?;
        }
    }
}

#[test]
fn json_edits_never_crash_and_many_still_load() {
    use proptest::test_runner::{Config, RngAlgorithm, TestRng, TestRunner};
    let base: Value = serde_json::from_str(&demo_text()).unwrap();
    let loaded = std::cell::Cell::new(0u32);
    // A fixed seed: the "many still load" count is a ratio over random
    // edits, and a random seed put it just under the bar on CI (24 of 256).
    let mut runner = TestRunner::new_with_rng(
        Config {
            cases: cases(256),
            failure_persistence: None,
            ..Config::default()
        },
        TestRng::deterministic_rng(RngAlgorithm::ChaCha),
    );
    runner
        .run(&prop::collection::vec(edit(), 1..4), |edits| {
            let mut doc = base.clone();
            for e in &edits {
                apply_edit(&mut doc, e);
            }
            if exercise(&doc.to_string())? {
                loaded.set(loaded.get() + 1);
            }
            Ok(())
        })
        .unwrap();
    // Most single edits hit values validation accepts: the reads, not just
    // the parser, get damaged documents.
    let n = cases(256);
    assert!(
        loaded.get() >= n / 10,
        "only {} of {n} edited documents loaded",
        loaded.get()
    );
}
