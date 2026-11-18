//! Starter project specs (plan 8e Ruling 5): `starter::parse` checks the
//! whole spec before anything happens, so a bad spec opens no window.

use core_control::starter::{self, MAX_BUSES, MAX_MARKERS, MAX_TRACKS, StarterKind};
use serde_json::{Value, json};

/// The synthwave example of plan 8e Task 1.
fn synthwave() -> Value {
    json!({
        "title": "Synthwave",
        "tempo": "118",
        "meter": "4/4",
        "markers": [
            {"bar": 1, "name": "Intro"},
            {"bar": 9, "name": "Verse"},
            {"bar": 17, "name": "Chorus"},
            {"bar": 33, "name": "Bridge"},
            {"bar": 41, "name": "Outro"}
        ],
        "buses": [
            {"name": "Reverb", "effects": ["clap:vizdaw.test.gain"]},
            {"name": "Delay", "output": "Reverb"}
        ],
        "tracks": [
            {"name": "Drums", "kind": "instrument"},
            {"name": "Bass", "kind": "instrument"},
            {"name": "Pad", "kind": "instrument", "sends": [{"bus": "Reverb", "level_db": "-12"}]},
            {"name": "Lead", "kind": "instrument"},
            {"name": "Arp", "kind": "instrument"}
        ]
    })
}

/// `base` with the value at `path` (keys and array indexes) set to
/// `value`, or removed for null.
fn with(base: &Value, path: &[&str], value: Value) -> Value {
    let mut v = base.clone();
    let (last, parents) = path.split_last().unwrap();
    let mut at = &mut v;
    for p in parents {
        at = match p.parse::<usize>() {
            Ok(i) => &mut at[i],
            Err(_) => &mut at[*p],
        };
    }
    match last.parse::<usize>() {
        Ok(i) => at.as_array_mut().unwrap()[i] = value,
        Err(_) => {
            let o = at.as_object_mut().unwrap();
            if value.is_null() {
                o.remove(*last);
            } else {
                o.insert(last.to_string(), value);
            }
        }
    }
    v
}

fn refusal(spec: &Value) -> String {
    match starter::parse(spec) {
        Ok(s) => panic!("{spec} parsed: {s:?}"),
        Err(m) => {
            assert!(!m.is_empty());
            m
        }
    }
}

#[test]
fn a_full_spec_parses() {
    let s = starter::parse(&synthwave()).unwrap();
    assert_eq!(s.title, "Synthwave");
    assert_eq!(s.tempo_milli, 118_000);
    assert_eq!(s.meter, (4, 4));
    let markers: Vec<(u32, &str)> = s.markers.iter().map(|(b, n)| (*b, n.as_str())).collect();
    assert_eq!(
        markers,
        [
            (1, "Intro"),
            (9, "Verse"),
            (17, "Chorus"),
            (33, "Bridge"),
            (41, "Outro")
        ]
    );
    assert_eq!(s.buses.len(), 2);
    assert_eq!(s.buses[0].name, "Reverb");
    assert_eq!(s.buses[0].effects, ["clap:vizdaw.test.gain"]);
    assert_eq!(s.buses[0].output, None);
    assert_eq!(s.buses[1].name, "Delay");
    assert!(s.buses[1].effects.is_empty());
    assert_eq!(s.buses[1].output.as_deref(), Some("Reverb"));
    let names: Vec<&str> = s.tracks.iter().map(|t| t.name.as_str()).collect();
    assert_eq!(names, ["Drums", "Bass", "Pad", "Lead", "Arp"]);
    for t in &s.tracks {
        assert_eq!(t.kind, StarterKind::Instrument, "{}", t.name);
        assert_eq!(t.plugin, None);
        assert!(t.effects.is_empty());
        assert_eq!(t.output, None);
    }
    assert_eq!(s.tracks[2].sends.len(), 1);
    assert_eq!(s.tracks[2].sends[0].bus, "Reverb");
    assert_eq!(s.tracks[2].sends[0].level_db, -12.0);
    assert!(s.tracks[0].sends.is_empty());

    // The optional parts: an audio track, a plug-in, effects, an output,
    // a send at the default level, a fractional tempo; names are trimmed
    // and a bus named in another case is the bus.
    let s = starter::parse(&json!({
        "title": "  Demo  ",
        "tempo": "92.5",
        "meter": "7/8",
        "tracks": [
            {"name": "Keys", "kind": "instrument", "plugin": "built-in-synth"},
            {"name": "Vox", "kind": "audio", "effects": ["multi-q", "au:aufx/abcd/Wxyz"],
             "output": "verb", "sends": [{"bus": "VERB"}]}
        ],
        "buses": [{"name": "Verb"}]
    }))
    .unwrap();
    assert_eq!(s.title, "Demo");
    assert_eq!(s.tempo_milli, 92_500);
    assert_eq!(s.meter, (7, 8));
    assert!(s.markers.is_empty());
    assert_eq!(s.tracks[0].plugin.as_deref(), Some("built-in-synth"));
    assert_eq!(s.tracks[1].kind, StarterKind::Audio);
    assert_eq!(s.tracks[1].effects, ["multi-q", "au:aufx/abcd/Wxyz"]);
    assert_eq!(s.tracks[1].output.as_deref(), Some("Verb"));
    assert_eq!(s.tracks[1].sends[0].bus, "Verb");
    assert_eq!(s.tracks[1].sends[0].level_db, 0.0);
    // Each limit holds exactly.
    let tracks: Vec<Value> = (0..MAX_TRACKS)
        .map(|i| json!({"name": format!("T{i}"), "kind": "audio"}))
        .collect();
    let buses: Vec<Value> = (0..MAX_BUSES)
        .map(|i| json!({"name": format!("B{i}")}))
        .collect();
    let markers: Vec<Value> = (0..MAX_MARKERS)
        .map(|i| json!({"bar": 999 - i, "name": "M"}))
        .collect();
    let full = json!({"title": "x".repeat(64), "tempo": "300", "meter": "32/32",
        "tracks": tracks, "buses": buses, "markers": markers});
    let s = starter::parse(&full).unwrap();
    assert_eq!(
        (s.tracks.len(), s.buses.len(), s.markers.len()),
        (32, 16, 32)
    );
    assert_eq!(s.tempo_milli, 300_000);
    assert!(starter::parse(&with(&full, &["tempo"], json!("20"))).is_ok());
}

#[test]
fn starter_specs_refuse_hostile_input() {
    let base = synthwave();
    let unknown = |key: &str| format!("Unknown key {key} in the starter project.");
    // The sentences Ruling 5 gives.
    for (spec, want) in [
        (
            with(&base, &["tracks", "0", "name"], json!("bass")),
            "Two strips are named Bass.".to_string(),
        ),
        (
            with(&base, &["buses", "1", "name"], json!("pad")),
            "Two strips are named Pad.".to_string(),
        ),
        (
            with(&base, &["tracks", "0", "output"], json!("Nowhere")),
            "No bus Nowhere in the starter project.".to_string(),
        ),
        (
            with(
                &base,
                &["tracks", "0", "sends"],
                json!([{"bus": "Nowhere"}]),
            ),
            "No bus Nowhere in the starter project.".to_string(),
        ),
        (
            with(&base, &["tracks", "0", "output"], json!("Bass")),
            "No bus Bass in the starter project.".to_string(),
        ),
        (
            with(&base, &["buses", "0", "output"], json!("Delay")),
            "Buses Reverb and Delay feed each other.".to_string(),
        ),
        (
            with(
                &base,
                &["tracks", "4"],
                json!({"name": "Arp", "kind": "audio", "plugin": "built-in-synth"}),
            ),
            "plugin is only for instrument tracks.".to_string(),
        ),
        (with(&base, &["extra"], json!(1)), unknown("extra")),
        (
            with(&base, &["buses", "1", "sends"], json!([])),
            unknown("sends"),
        ),
        (
            with(&base, &["tracks", "2", "colour"], json!("red")),
            unknown("colour"),
        ),
        (
            with(
                &base,
                &["tracks", "2", "sends", "0", "pre_fader"],
                json!(true),
            ),
            unknown("pre_fader"),
        ),
        (
            with(&base, &["markers", "0", "tick"], json!(0)),
            unknown("tick"),
        ),
    ] {
        assert_eq!(refusal(&spec), want, "{spec}");
    }
    // A three-bus cycle names its first two buses.
    let three = with(
        &with(
            &base,
            &["buses"],
            json!([
                {"name": "A", "output": "B"},
                {"name": "B", "output": "C"},
                {"name": "C", "output": "A"}
            ]),
        ),
        &["tracks", "2", "sends"],
        json!([]),
    );
    assert_eq!(refusal(&three), "Buses A and B feed each other.");
    // Every other hostile spec is refused with a sentence.
    let many = |n: usize, f: &dyn Fn(usize) -> Value| Value::Array((0..n).map(f).collect());
    let mut cases = vec![
        json!(null),
        json!([]),
        json!("spec"),
        with(&base, &["title"], Value::Null),
        with(&base, &["tracks"], Value::Null),
        with(&base, &["tracks"], json!([])),
        with(&base, &["tempo"], Value::Null),
        with(&base, &["meter"], Value::Null),
        with(
            &base,
            &["tracks"],
            many(33, &|i| json!({"name": format!("T{i}"), "kind": "audio"})),
        ),
        with(
            &base,
            &["buses"],
            many(17, &|i| json!({"name": format!("B{i}")})),
        ),
        with(
            &base,
            &["markers"],
            many(33, &|i| json!({"bar": i + 1, "name": "M"})),
        ),
        with(&base, &["markers", "0", "bar"], json!(0)),
        with(&base, &["markers", "0", "bar"], json!(1000)),
        with(&base, &["markers", "0", "bar"], json!(-1)),
        with(&base, &["markers", "0", "bar"], json!("1")),
        with(&base, &["markers", "0", "bar"], json!(1.5)),
        with(&base, &["markers", "0", "name"], json!("")),
        with(&base, &["markers", "0", "name"], json!("a\u{202E}b")),
        with(&base, &["markers", "0", "name"], Value::Null),
        with(&base, &["markers"], json!({})),
        with(&base, &["tracks", "1", "kind"], json!("midi")),
        with(&base, &["tracks", "1", "kind"], Value::Null),
        with(
            &base,
            &["buses", "0", "effects"],
            many(9, &|_| json!("multi-q")),
        ),
        with(
            &base,
            &["tracks", "0", "effects"],
            many(9, &|_| json!("multi-q")),
        ),
        with(
            &base,
            &["tracks", "0", "sends"],
            many(9, &|_| json!({"bus": "Reverb"})),
        ),
        with(
            &base,
            &["tracks", "2", "sends", "0", "level_db"],
            json!("13"),
        ),
        with(
            &base,
            &["tracks", "2", "sends", "0", "level_db"],
            json!("-145"),
        ),
        with(
            &base,
            &["tracks", "2", "sends", "0", "level_db"],
            json!(-12),
        ),
        with(
            &base,
            &["tracks", "2", "sends", "0", "level_db"],
            json!("1e1"),
        ),
        with(&base, &["tracks", "2", "sends", "0", "bus"], Value::Null),
        with(&base, &["tracks", "2", "sends", "0"], json!("Reverb")),
        with(&base, &["tracks", "0", "plugin"], json!("vst:x")),
        with(&base, &["tracks", "0", "plugin"], json!("clap:")),
        with(&base, &["tracks", "0", "plugin"], json!(1)),
        with(
            &base,
            &["tracks", "0", "plugin"],
            json!(format!("clap:{}", "x".repeat(508))),
        ),
        with(&base, &["buses", "0", "effects", "0"], json!("vst:x")),
        with(
            &base,
            &["buses", "0", "effects", "0"],
            json!("built-in-synth"),
        ),
        with(
            &base,
            &["buses", "0", "effects", "0"],
            json!(format!("clap:{}", "x".repeat(508))),
        ),
        with(&base, &["buses", "0", "effects"], json!("multi-q")),
        with(&base, &["tracks", "0", "name"], json!("Dr\u{0}ums")),
        with(&base, &["buses", "0", "name"], json!("Re\u{0}verb")),
        with(&base, &["tracks", "0", "name"], json!("   ")),
        with(&base, &["tracks", "0", "name"], json!("x".repeat(65))),
        with(&base, &["tracks", "0", "name"], json!(7)),
        with(&base, &["title"], json!("x".repeat(70_000))),
        with(&base, &["title"], json!("Synth\u{0}wave")),
        with(&base, &["title"], json!("")),
        with(&base, &["title"], json!(1)),
        with(&base, &["buses", "0", "output"], json!("Reverb")),
        with(&base, &["tracks", "0", "output"], json!(1)),
    ];
    for tempo in [
        json!("19"),
        json!("301"),
        json!("1e3"),
        json!("NaN"),
        json!(120),
        json!(""),
        json!("+120"),
        json!("120 "),
    ] {
        cases.push(with(&base, &["tempo"], tempo));
    }
    for meter in [
        json!("4/3"),
        json!("0/4"),
        json!("33/4"),
        json!("4/64"),
        json!("4"),
        json!("4/4/4"),
        json!(" 4/4"),
        json!("-4/4"),
        json!("4/0"),
        json!(4),
    ] {
        cases.push(with(&base, &["meter"], meter));
    }
    for spec in &cases {
        let m = refusal(spec);
        assert!(m.ends_with('.'), "{spec}: {m}");
        assert!(m.chars().count() < 300, "{m}");
    }
    // A hostile key is named safely: cut short, with no control character.
    let m = refusal(&with(&base, &[&"k\u{0}".repeat(40_000)], json!(1)));
    assert!(m.starts_with("Unknown key "), "{m}");
    assert!(m.chars().count() < 200, "{m}");
    assert!(!m.contains('\u{0}'), "{m}");
}
