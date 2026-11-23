//! The piano roll's saved settings (`ui.json` `pianoRoll`) and the
//! detail's shown tracks (`detail.lanes`): Swift
//! `PianoRollModelTests.settingsAndShownLanesDecodeTolerantly`.

use serde_json::{Value, json};
use vizdaw_app::editor::{DetailEditorKind, DetailState, EditorLayout, EditorTarget};
use vizdaw_app::piano_roll::PianoRollSettings;

fn decode(json: &str) -> EditorLayout {
    EditorLayout::decode(Some(json))
}

#[test]
fn piano_roll_settings_have_swifts_defaults() {
    let s = PianoRollSettings::default();
    assert!(s.show_keyboard);
    assert_eq!(s.row_height, 10.);
    assert_eq!(s.ticks_per_point, 96.);
    assert_eq!(s.expression, json!({"pitchBend": {}}));
    assert!(s.show_regions);
    assert!(s.show_expression);
    assert_eq!(EditorLayout::default().piano_roll, s);
}

#[test]
fn piano_roll_settings_clamp_and_fall_back() {
    let l = decode(r#"{"pianoRoll":{"rowHeight":999,"showKeyboard":false}}"#);
    assert_eq!(l.piano_roll.row_height, 40.);
    assert!(!l.piano_roll.show_keyboard);
    assert_eq!(l.piano_roll.ticks_per_point, 96.);
    assert_eq!(
        decode(r#"{"pianoRoll":7}"#).piano_roll,
        PianoRollSettings::default()
    );
    let l = decode(r#"{"pianoRoll":{"rowHeight":1,"ticksPerPoint":1000000}}"#);
    assert_eq!(l.piano_roll.row_height, 4.);
    assert_eq!(l.piano_roll.ticks_per_point, 61_440.);
    let l = decode(r#"{"pianoRoll":{"rowHeight":-3,"ticksPerPoint":0}}"#);
    assert_eq!(l.piano_roll.row_height, 10.);
    assert_eq!(l.piano_roll.ticks_per_point, 96.);
    let l = decode(
        r#"{"pianoRoll":{"rowHeight":"tall","ticksPerPoint":null,"showRegions":3,"expression":5}}"#,
    );
    assert_eq!(l.piano_roll, PianoRollSettings::default(), "each bad field");
    // A NaN zoom never decodes from JSON; the clamp treats one as missing.
    assert_eq!(
        EditorLayout::sane(
            f64::NAN,
            &EditorLayout::TICKS_PER_POINT_RANGE,
            PianoRollSettings::default().ticks_per_point
        ),
        96.
    );
    assert_eq!(
        EditorLayout::sane(f64::INFINITY, &PianoRollSettings::ROW_HEIGHT_RANGE, 10.),
        10.
    );
}

#[test]
fn the_expression_kinds_swift_writes_are_kept() {
    for e in [
        json!({"pitchBend": {}}),
        json!({"registered": {"_0": 74}}),
        json!({"assignable": {"_0": 2}}),
    ] {
        let text = json!({"pianoRoll": {"expression": e}}).to_string();
        assert_eq!(decode(&text).piano_roll.expression, e);
    }
    for bad in [
        json!({"registered": {"_0": 300}}),
        json!({"bend": {}}),
        json!({"pitchBend": {}, "registered": {"_0": 1}}),
        json!("pitchBend"),
    ] {
        let text = json!({"pianoRoll": {"expression": bad}}).to_string();
        assert_eq!(
            decode(&text).piano_roll.expression,
            json!({"pitchBend": {}}),
            "{bad}"
        );
    }
}

#[test]
fn piano_roll_keys_survive_a_save() {
    let existing = json!({
        "pianoRoll": {
            "showKeyboard": false,
            "rowHeight": 12,
            "ticksPerPoint": 48,
            "expression": {"registered": {"_0": 74}},
            "showRegions": false,
            "showExpression": false,
            "future": {"x": 1}
        },
        "other": true
    })
    .to_string();
    let mut l = decode(&existing);
    l.piano_roll.row_height = 20.;
    let saved: Value = serde_json::from_str(&l.merge_into(Some(&existing))).unwrap();
    let p = &saved["pianoRoll"];
    assert_eq!(p["rowHeight"], json!(20.0));
    assert_eq!(p["showKeyboard"], json!(false));
    assert_eq!(p["ticksPerPoint"], json!(48.0));
    assert_eq!(p["expression"], json!({"registered": {"_0": 74}}));
    assert_eq!(p["showRegions"], json!(false));
    assert_eq!(p["showExpression"], json!(false));
    assert_eq!(p["future"], json!({"x": 1}), "an unknown key stays");
    assert_eq!(saved["other"], json!(true));
}

#[test]
fn a_fresh_save_writes_all_six_keys() {
    let saved: Value = serde_json::from_str(&EditorLayout::default().encoded()).unwrap();
    assert_eq!(
        saved["pianoRoll"],
        json!({
            "showKeyboard": true,
            "rowHeight": 10.0,
            "ticksPerPoint": 96.0,
            "expression": {"pitchBend": {}},
            "showRegions": true,
            "showExpression": true
        })
    );
}

#[test]
fn the_shown_tracks_ride_in_the_detail() {
    let mut d = DetailState::new(
        DetailEditorKind::PianoRoll,
        EditorTarget::Sequence { seq: 3 },
    );
    assert_eq!(d.lanes(), None);
    d.set_lanes(Some(vec![31]));
    assert_eq!(d.lanes(), Some(vec![31]));
    let text = serde_json::to_string(&d).unwrap();
    let v: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(v["lanes"], json!([31]), "Swift's shape");
    let back: DetailState = serde_json::from_str(&text).unwrap();
    assert_eq!(back, d);
    d.set_lanes(None);
    let v: Value = serde_json::to_value(&d).unwrap();
    assert!(v.get("lanes").is_none(), "none is left out");
    let old = r#"{"kind":"pianoRoll","target":{"sequence":{"_0":3}}}"#;
    let old: DetailState = serde_json::from_str(old).unwrap();
    assert_eq!(old.lanes(), None, "4a layouts");
    let bad = r#"{"kind":"pianoRoll","target":{"sequence":{"_0":3}},"lanes":"x"}"#;
    let bad: DetailState = serde_json::from_str(bad).unwrap();
    assert_eq!(bad.lanes(), None);
}
