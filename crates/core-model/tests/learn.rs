//! Learn maps in the project (plan 5f-1 Task 4; spec §4.8; Rulings 6, 18).

use core_model::automation::AutomationTarget as T;
use core_model::command::{Command, SettingsCmd, apply};
use core_model::document::Document;
use core_model::learn::{LearnKind, LearnMap, LearnMode, LearnSource, MAX_LEARN_MAPS};

fn doc() -> Document {
    Document::new()
}

fn src(port: &str, kind: LearnKind) -> LearnSource {
    LearnSource {
        port: port.into(),
        kind,
    }
}

fn map(source: LearnSource, target: T) -> LearnMap {
    LearnMap {
        source,
        target,
        mode: LearnMode::Absolute,
        pickup: true,
    }
}

fn set(map: LearnMap) -> Command {
    Command::Settings(SettingsCmd::SetLearn { map })
}

fn master_fader(d: &Document) -> T {
    T::StripFader(d.mixer().master())
}

#[test]
fn set_replaces_by_source_and_remove_takes_it_back() {
    let d = doc();
    let s = src("Knobs", LearnKind::Cc7 { channel: 0, cc: 7 });
    let m = map(s.clone(), master_fader(&d));
    let cmd = set(m.clone());
    assert_eq!(cmd.name(), "MIDI Learn");
    let d = apply(&d, &cmd).unwrap().doc;
    assert_eq!(d.settings().learn, vec![m.clone()]);
    let relative = LearnMap {
        mode: LearnMode::Relative2sComplement,
        pickup: false,
        ..m.clone()
    };
    let d = apply(&d, &set(relative.clone())).unwrap().doc;
    assert_eq!(d.settings().learn, vec![relative], "same source: replaced");
    let rm = Command::Settings(SettingsCmd::RemoveLearn {
        source: Some(s.clone()),
        target: None,
    });
    assert_eq!(rm.name(), "Remove MIDI Learn");
    let d = apply(&d, &rm).unwrap().doc;
    assert!(d.settings().learn.is_empty());
    assert!(apply(&d, &rm).is_err(), "nothing to remove");
}

/// Pre-flight M14: `surface.unlearn target` removes every map to the
/// target, as one command.
#[test]
fn removing_by_target_takes_every_map_to_it_as_one_step() {
    let mut d = doc();
    let t = master_fader(&d);
    for cc in [7, 8] {
        d = apply(
            &d,
            &set(map(src("Knobs", LearnKind::Cc7 { channel: 0, cc }), t)),
        )
        .unwrap()
        .doc;
    }
    let other = T::StripMute(d.mixer().master());
    let pad = src(
        "Pads",
        LearnKind::Note {
            channel: 9,
            note: 36,
        },
    );
    d = apply(&d, &set(map(pad.clone(), other))).unwrap().doc;
    let rm = Command::Settings(SettingsCmd::RemoveLearn {
        source: None,
        target: Some(t),
    });
    assert_eq!(rm.name(), "Remove MIDI Learn");
    let d = apply(&d, &rm).unwrap().doc;
    assert_eq!(d.settings().learn.len(), 1, "both maps to the fader went");
    assert_eq!(d.settings().learn[0].target, other);
    assert!(apply(&d, &rm).is_err(), "nothing maps to it any more");
    let both = Command::Settings(SettingsCmd::RemoveLearn {
        source: Some(pad),
        target: Some(other),
    });
    assert!(
        apply(&d, &both).is_err(),
        "exactly one of source and target"
    );
    let neither = Command::Settings(SettingsCmd::RemoveLearn {
        source: None,
        target: None,
    });
    assert!(apply(&d, &neither).is_err());
}

#[test]
fn a_learn_map_is_checked_by_the_model() {
    let d = doc();
    let t = master_fader(&d);
    for bad in [
        src("", LearnKind::Cc7 { channel: 0, cc: 7 }),
        src(&"x".repeat(256), LearnKind::Cc7 { channel: 0, cc: 7 }),
        src("a\u{7}b", LearnKind::Cc7 { channel: 0, cc: 7 }),
        src("K", LearnKind::Cc7 { channel: 16, cc: 7 }),
        src(
            "K",
            LearnKind::Cc7 {
                channel: 0,
                cc: 128,
            },
        ),
        src("K", LearnKind::Cc14 { channel: 0, cc: 32 }),
        src(
            "K",
            LearnKind::Note {
                channel: 0,
                note: 200,
            },
        ),
        src("K", LearnKind::PitchBend { channel: 99 }),
    ] {
        assert!(apply(&d, &set(map(bad.clone(), t))).is_err(), "{bad:?}");
    }
    let ok = src("K", LearnKind::Cc7 { channel: 0, cc: 1 });
    let gone = T::StripFader(core_model::ids::StripId(987_654));
    assert!(
        apply(&d, &set(map(ok.clone(), gone))).is_err(),
        "the target must exist"
    );
    // A destination the document has, so only the velocity-scale rule can refuse it.
    let d = apply(
        &d,
        &Command::Routing(core_model::command::RoutingCmd::AddDestination {
            name: "Plug".into(),
            kind: core_model::document::DestKind::plugin("p"),
            default_patch: None,
        }),
    )
    .unwrap()
    .doc;
    let dest = *d.instruments().keys().next().unwrap();
    assert!(
        apply(&d, &set(map(ok, T::VelocityScale(dest)))).is_err(),
        "velocity scale cannot be learned"
    );
}

#[test]
fn the_map_count_is_bounded() {
    let mut d = doc();
    let t = master_fader(&d);
    for i in 0..MAX_LEARN_MAPS {
        let s = src(
            &format!("P{}", i / 128),
            LearnKind::Cc7 {
                channel: 0,
                cc: (i % 128) as u8,
            },
        );
        d = apply(&d, &set(map(s, t))).unwrap().doc;
    }
    let one_more = src(
        "Extra",
        LearnKind::Note {
            channel: 0,
            note: 1,
        },
    );
    assert!(apply(&d, &set(map(one_more, t))).is_err());
}

#[test]
fn learn_maps_round_trip_and_are_absent_when_empty() {
    let d = doc();
    let json = serde_json::to_string(d.settings()).unwrap();
    assert!(
        !json.contains("learn"),
        "an empty list is not written: {json}"
    );
    let m = map(
        src(
            "Pads",
            LearnKind::Note {
                channel: 9,
                note: 36,
            },
        ),
        T::StripMute(d.mixer().master()),
    );
    let d = apply(&d, &set(m.clone())).unwrap().doc;
    let json = serde_json::to_string(d.settings()).unwrap();
    let back: core_model::document::Settings = serde_json::from_str(&json).unwrap();
    assert_eq!(back.learn, vec![m.clone()]);
    assert_eq!(core_model::file::VERSION, 3);
    let bare = serde_json::to_string(&m)
        .unwrap()
        .replace(",\"pickup\":true", "");
    assert!(!bare.contains("pickup"), "{bare}");
    let old: LearnMap = serde_json::from_str(&bare).unwrap();
    assert!(
        old.pickup,
        "pick-up is on when the field is absent (Ruling 18)"
    );
    assert!(!m.picks_up(), "a note map never picks up");
    assert!(
        map(
            src("K", LearnKind::Cc14 { channel: 0, cc: 1 }),
            T::StripMute(d.mixer().master())
        )
        .picks_up()
    );
}

#[test]
fn sources_describe_themselves() {
    assert_eq!(
        src("Knobs", LearnKind::Cc7 { channel: 0, cc: 7 }).describe(),
        "Knobs · CC 7 ch 1"
    );
    assert_eq!(
        src("Knobs", LearnKind::Cc14 { channel: 1, cc: 1 }).describe(),
        "Knobs · CC 1/33 ch 2"
    );
    assert_eq!(
        src("Keys", LearnKind::PitchBend { channel: 0 }).describe(),
        "Keys · Pitch Bend ch 1"
    );
    assert_eq!(
        src(
            "Pads",
            LearnKind::Note {
                channel: 9,
                note: 60
            }
        )
        .describe(),
        "Pads · Note C4 ch 10"
    );
}

/// Hostile and damaged files (control spec §11; Ruling 6): a map the
/// commands would refuse is reported Corrupt on load; a map whose target
/// was deleted later stays (it does nothing); a map without `mode` and
/// `pickup` loads with the defaults. Both commands are undo steps.
#[test]
fn learn_maps_load_checked_and_undo_as_one_step() {
    use core_model::file::{from_json, to_json};
    use core_model::history::History;
    use core_model::selection::ViewFilter;
    use serde_json::{Value, json};

    let d = doc();
    let mut h = History::new(d.clone());
    let s = src("Knobs", LearnKind::Cc7 { channel: 0, cc: 7 });
    h.execute(
        &set(map(s.clone(), master_fader(&d))),
        &ViewFilter::default(),
    )
    .unwrap();
    assert_eq!(h.undo_label(), Some("MIDI Learn"));
    let rm = Command::Settings(SettingsCmd::RemoveLearn {
        source: Some(s),
        target: None,
    });
    h.execute(&rm, &ViewFilter::default()).unwrap();
    assert_eq!(h.undo_label(), Some("Remove MIDI Learn"));
    h.undo().unwrap();
    assert_eq!(h.doc().settings().learn.len(), 1, "undo puts the map back");
    h.undo().unwrap();
    assert!(
        h.doc().settings().learn.is_empty(),
        "undo takes the learn back"
    );
    h.redo().unwrap();
    assert_eq!(h.doc().settings().learn.len(), 1);

    let text = to_json(h.doc()).unwrap();
    assert_eq!(
        from_json(&text).unwrap().settings().learn,
        h.doc().settings().learn
    );
    let good: Value = serde_json::from_str(&text).unwrap();
    let with_map = |m: Value| {
        let mut v = good.clone();
        v["document"]["settings"]["learn"] = json!([m]);
        v.to_string()
    };
    let m0 = good["document"]["settings"]["learn"][0].clone();

    // Defaults for the fields a map may omit.
    let mut bare = m0.clone();
    bare.as_object_mut().unwrap().remove("mode");
    bare.as_object_mut().unwrap().remove("pickup");
    let loaded = from_json(&with_map(bare)).unwrap();
    assert_eq!(loaded.settings().learn[0].mode, LearnMode::Absolute);
    assert!(loaded.settings().learn[0].pickup);

    // A dangling target is kept.
    let mut dangling = m0.clone();
    dangling["target"] = json!({"strip_fader": 987_654});
    assert_eq!(
        from_json(&with_map(dangling))
            .unwrap()
            .settings()
            .learn
            .len(),
        1
    );

    // What the commands refuse, a file may not smuggle in.
    let mut bad_channel = m0.clone();
    bad_channel["source"]["kind"] = json!({"cc7": {"channel": 16, "cc": 7}});
    let mut bad_port = m0.clone();
    bad_port["source"]["port"] = json!("");
    let mut velocity = m0.clone();
    velocity["target"] = json!({"velocity_scale": 0});
    let mut unknown = m0.clone();
    unknown["extra"] = json!(1);
    let mut wrong_type = m0.clone();
    wrong_type["pickup"] = json!("yes");
    for bad in [bad_channel, bad_port, velocity, unknown, wrong_type] {
        assert!(
            matches!(
                from_json(&with_map(bad.clone())),
                Err(core_model::ModelError::Corrupt { .. })
            ),
            "{bad}"
        );
    }
    let mut v = good.clone();
    v["document"]["settings"]["learn"] = json!([m0.clone(), m0.clone()]);
    assert!(
        from_json(&v.to_string()).is_err(),
        "two maps with one source"
    );
    let many: Vec<Value> = (0..=MAX_LEARN_MAPS)
        .map(|i| {
            let mut m = m0.clone();
            m["source"]["port"] = json!(format!("P{i}"));
            m
        })
        .collect();
    let mut v = good.clone();
    v["document"]["settings"]["learn"] = json!(many);
    assert!(
        from_json(&v.to_string()).is_err(),
        "more than MAX_LEARN_MAPS"
    );
}

/// Plan 5f-1 Task 4 review: a kind refuses fields it does not know, as the
/// other learn types do, and every kind as the app writes it still loads.
#[test]
fn learn_kinds_refuse_unknown_fields_and_still_load() {
    for (text, kind) in [
        (
            r#"{"cc7":{"channel":0,"cc":7}}"#,
            LearnKind::Cc7 { channel: 0, cc: 7 },
        ),
        (
            r#"{"cc14":{"channel":1,"cc":2}}"#,
            LearnKind::Cc14 { channel: 1, cc: 2 },
        ),
        (
            r#"{"pitch_bend":{"channel":2}}"#,
            LearnKind::PitchBend { channel: 2 },
        ),
        (
            r#"{"note":{"channel":9,"note":36}}"#,
            LearnKind::Note {
                channel: 9,
                note: 36,
            },
        ),
    ] {
        assert_eq!(serde_json::from_str::<LearnKind>(text).unwrap(), kind);
        assert_eq!(serde_json::to_string(&kind).unwrap(), text);
    }
    for text in [
        r#"{"cc7":{"channel":0,"cc":7,"extra":1}}"#,
        r#"{"pitch_bend":{"channel":2,"cc":1}}"#,
    ] {
        assert!(serde_json::from_str::<LearnKind>(text).is_err(), "{text}");
    }
    let map: LearnMap = serde_json::from_str(
        r#"{"source":{"port":"Knobs","kind":{"cc7":{"channel":0,"cc":7}}},"target":{"strip_fader":1},"mode":"absolute","pickup":false}"#,
    )
    .unwrap();
    assert!(!map.pickup && map.picks_up(), "can pick up, does not");
}
