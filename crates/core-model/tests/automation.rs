//! Automation events (editors spec §4d, Plan 4d Rulings 2–7).

use core_model::automation::{
    AutomationTarget, SendIndex, check_value, fader_db, fader_norm, from_u32, pan_norm, pan_of,
    param_plain, switch_on, to_u32, velocity_scale_percent,
};
use core_model::command::{Command, EventCmd, SequenceCmd, apply};
use core_model::document::Document;
use core_model::event::{EventBody, EventKind, EventSpec};
use core_model::file::{from_json, to_json};
use core_model::ids::{DestId, EventId, LaneId, PluginInstanceId, SeqId, StripId};
use core_model::mixer::{GAIN_FLOOR_DB, ParamId};
use core_model::rule::{Rule, events_matching};
use core_model::selection::ViewFilter;
use core_model::time::Tick;

fn auto(t: i64, target: AutomationTarget, value: f64) -> EventSpec {
    EventSpec::new(Tick(t), EventBody::Automation { target, value })
}

fn doc_with(events: Vec<EventSpec>) -> core_model::Result<Document> {
    let d = apply(
        &Document::new(),
        &Command::Sequence(SequenceCmd::New { name: "S".into() }),
    )?
    .doc;
    Ok(apply(
        &d,
        &Command::Event(EventCmd::Insert {
            seq: SeqId(0),
            lane: LaneId(0),
            events,
        }),
    )?
    .doc)
}

#[test]
fn the_laws_match_the_rulings() {
    assert_eq!(fader_db(1.0), 12.0);
    assert!(fader_db(0.0) == GAIN_FLOOR_DB && fader_db(0.002) == GAIN_FLOOR_DB);
    assert!(
        (fader_db(fader_norm(0.0))).abs() < 1e-9,
        "unity round-trips"
    );
    assert!((fader_norm(0.0) - 0.630_957).abs() < 1e-6);
    assert!((fader_norm(-6.0) - fader_norm(-6.0 + 1e-12)).abs() < 1e-9);
    assert_eq!(fader_norm(GAIN_FLOOR_DB), 0.0);
    assert_eq!(fader_db(f64::NAN), GAIN_FLOOR_DB);
    assert_eq!((pan_of(0.0), pan_of(0.5), pan_of(1.0)), (-1.0, 0.0, 1.0));
    assert_eq!(pan_norm(-1.0), 0.0);
    assert!(!switch_on(0.49) && switch_on(0.5));
    assert_eq!(velocity_scale_percent(0.0), 1.0);
    assert_eq!(velocity_scale_percent(0.5), 100.0);
    assert_eq!(velocity_scale_percent(1.0), 250.0);
    assert_eq!(param_plain(0.25, 0.0, 2.0, false), 0.5);
    assert_eq!(param_plain(0.3, 0.0, 10.0, true), 3.0);
    assert_eq!((to_u32(0.0), to_u32(1.0)), (0, u32::MAX));
    assert_eq!(
        from_u32(to_u32(0.25)),
        to_u32(0.25) as f64 / u32::MAX as f64
    );
    assert!(check_value(0.0) && check_value(1.0));
    assert!(!check_value(-0.1) && !check_value(1.5) && !check_value(f64::NAN));
    assert!(AutomationTarget::StripMute(StripId(1)).is_stepped());
    assert!(!AutomationTarget::StripFader(StripId(1)).is_stepped());
}

#[test]
fn automation_events_check_their_value_and_are_not_midi() {
    let target = AutomationTarget::SendLevel(StripId(3), SendIndex(1));
    let d = doc_with(vec![auto(0, target, 0.25)]).unwrap();
    let (ev, _) = d.event(EventId(0)).unwrap();
    assert_eq!(ev.body.kind(), EventKind::Automation);
    assert!(!ev.body.is_midi());
    for bad in [1.5, -0.5, f64::NAN, f64::INFINITY] {
        assert!(
            doc_with(vec![auto(0, target, bad)]).is_err(),
            "{bad} was accepted"
        );
    }
    // A target that does not exist is accepted (it may come back by undo).
    assert!(
        doc_with(vec![auto(
            0,
            AutomationTarget::StripFader(StripId(999)),
            0.5
        )])
        .is_ok()
    );
}

#[test]
fn the_automation_rule_matches_its_target_only() {
    let fader = AutomationTarget::StripFader(StripId(1));
    let param = AutomationTarget::PluginParam(PluginInstanceId(2), ParamId(7));
    let d = doc_with(vec![
        auto(0, fader, 0.1),
        auto(10, param, 0.2),
        auto(20, AutomationTarget::VelocityScale(DestId(0)), 0.5),
    ])
    .unwrap();
    let lanes = [(SeqId(0), LaneId(0))].into_iter().collect();
    let found = |target| {
        events_matching(
            &d,
            &lanes,
            &Rule::Automation { target },
            &ViewFilter::default(),
        )
        .into_iter()
        .map(|e| e.0)
        .collect::<Vec<_>>()
    };
    assert_eq!(found(fader), vec![0]);
    assert_eq!(found(param), vec![1]);
    let kind = Rule::Kind {
        kinds: vec![EventKind::Automation],
    };
    assert_eq!(
        events_matching(&d, &lanes, &kind, &ViewFilter::default()).len(),
        3
    );
}

#[test]
fn automation_serialises_as_an_appended_variant_and_round_trips() {
    let body = EventBody::Automation {
        target: AutomationTarget::StripFader(StripId(3)),
        value: 0.5,
    };
    assert_eq!(
        serde_json::to_value(&body).unwrap(),
        serde_json::json!({"type": "automation", "target": {"strip_fader": 3}, "value": 0.5})
    );
    let send = serde_json::to_value(EventBody::Automation {
        target: AutomationTarget::SendLevel(StripId(3), SendIndex(2)),
        value: 1.0,
    })
    .unwrap();
    assert_eq!(send["target"], serde_json::json!({"send_level": [3, 2]}));
    let d = doc_with(vec![auto(5, AutomationTarget::StripPan(StripId(1)), 0.75)]).unwrap();
    let text = to_json(&d).unwrap();
    assert_eq!(from_json(&text).unwrap(), d);
    // A project without automation (every saved project so far) still
    // loads: its JSON has no automation variant at all.
    let plain = doc_with(vec![]).unwrap();
    assert!(!to_json(&plain).unwrap().contains("automation"));
    assert_eq!(from_json(&to_json(&plain).unwrap()).unwrap(), plain);
}
