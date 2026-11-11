//! MPE settings in the model (plan 5g-2 Task 1; Rulings 2, 3): a lane's
//! MPE input and a destination's MPE output are undoable, checked, and
//! saved only when set.

use core_model::ModelError;
use core_model::command::{Command, LaneCmd, RoutingCmd, SequenceCmd, apply};
use core_model::document::{DestKind, Document};
use core_model::file::{VERSION, from_json, to_json};
use core_model::history::History;
use core_model::ids::{DestId, LaneId, SeqId};
use core_model::mpe::{MpeConfig, MpeZone};
use core_model::selection::ViewFilter;
use serde_json::{Value, json};

fn doc() -> Document {
    let d = apply(
        &Document::new(),
        &Command::Sequence(SequenceCmd::New { name: "S".into() }),
    )
    .unwrap()
    .doc;
    apply(
        &d,
        &Command::Routing(RoutingCmd::AddDestination {
            name: "Synth".into(),
            kind: DestKind::MidiEndpoint {
                endpoint_ref: "Synth Port".into(),
                group: 0,
                channel: 0,
            },
            default_patch: None,
        }),
    )
    .unwrap()
    .doc
}

fn dest(d: &Document) -> DestId {
    *d.instruments()
        .keys()
        .find(|k| d.instruments().get(k).unwrap().name == "Synth")
        .unwrap()
}

fn lane_mpe(mpe: Option<MpeConfig>) -> Command {
    Command::Lane(LaneCmd::SetMpe {
        seq: SeqId(0),
        lane: LaneId(0),
        mpe,
    })
}

#[test]
fn zones_name_their_channels() {
    let lower = MpeConfig {
        zone: MpeZone::Lower,
        members: 3,
        bend_range: 48,
    };
    assert_eq!(lower.manager(), 0);
    assert_eq!(
        (0..4).map(|k| lower.member(k)).collect::<Vec<_>>(),
        vec![Some(1), Some(2), Some(3), None]
    );
    assert!(lower.is_member(3) && !lower.is_member(0) && !lower.is_member(4));
    let upper = MpeConfig {
        zone: MpeZone::Upper,
        members: 3,
        bend_range: 48,
    };
    assert_eq!(upper.manager(), 15);
    assert_eq!(
        (0..4).map(|k| upper.member(k)).collect::<Vec<_>>(),
        vec![Some(14), Some(13), Some(12), None]
    );
    assert!(upper.is_member(12) && !upper.is_member(11) && !upper.is_member(15));
    assert_eq!(
        MpeConfig::default(),
        MpeConfig {
            zone: MpeZone::Lower,
            members: 15,
            bend_range: 48
        }
    );
    let full = MpeConfig::default();
    assert_eq!(full.member(14), Some(15));
}

#[test]
fn lane_and_destination_settings_are_undoable() {
    let mut h = History::new(doc());
    let f = ViewFilter::default();
    let d = dest(h.doc());
    let cfg = MpeConfig {
        zone: MpeZone::Upper,
        members: 7,
        bend_range: 24,
    };
    h.execute(&lane_mpe(Some(cfg)), &f).unwrap();
    assert_eq!(h.undo_label(), Some("Set Track MPE"));
    h.execute(
        &Command::Routing(RoutingCmd::SetMpe {
            dest: d,
            mpe: Some(MpeConfig::default()),
        }),
        &f,
    )
    .unwrap();
    assert_eq!(h.undo_label(), Some("Set Destination MPE"));
    assert_eq!(h.doc().lane(SeqId(0), LaneId(0)).unwrap().mpe, Some(cfg));
    assert_eq!(
        h.doc().instruments().get(&d).unwrap().mpe,
        Some(MpeConfig::default())
    );
    h.undo().unwrap();
    h.undo().unwrap();
    assert_eq!(h.doc().lane(SeqId(0), LaneId(0)).unwrap().mpe, None);
    assert_eq!(h.doc().instruments().get(&d).unwrap().mpe, None);
}

#[test]
fn hostile_mpe_settings_are_refused() {
    let d = doc();
    let dst = dest(&d);
    for bad in [
        MpeConfig {
            zone: MpeZone::Lower,
            members: 0,
            bend_range: 48,
        },
        MpeConfig {
            zone: MpeZone::Lower,
            members: 16,
            bend_range: 48,
        },
        MpeConfig {
            zone: MpeZone::Upper,
            members: 255,
            bend_range: 48,
        },
        MpeConfig {
            zone: MpeZone::Lower,
            members: 15,
            bend_range: 0,
        },
        MpeConfig {
            zone: MpeZone::Lower,
            members: 15,
            bend_range: 97,
        },
    ] {
        assert!(
            matches!(
                apply(&d, &lane_mpe(Some(bad))),
                Err(ModelError::OutOfRange { .. })
            ),
            "{bad:?}"
        );
        assert!(
            apply(
                &d,
                &Command::Routing(RoutingCmd::SetMpe {
                    dest: dst,
                    mpe: Some(bad)
                })
            )
            .is_err()
        );
    }
    assert!(
        apply(
            &d,
            &Command::Routing(RoutingCmd::SetMpe {
                dest: DestId(999),
                mpe: None
            })
        )
        .is_err()
    );
    assert!(
        apply(
            &d,
            &Command::Lane(LaneCmd::SetMpe {
                seq: SeqId(0),
                lane: LaneId(9),
                mpe: None
            })
        )
        .is_err()
    );
    // The edges are fine.
    assert!(
        apply(
            &d,
            &lane_mpe(Some(MpeConfig {
                zone: MpeZone::Upper,
                members: 1,
                bend_range: 1
            }))
        )
        .is_ok()
    );
    assert!(
        apply(
            &d,
            &lane_mpe(Some(MpeConfig {
                zone: MpeZone::Lower,
                members: 15,
                bend_range: 96
            }))
        )
        .is_ok()
    );
}

#[test]
fn files_write_mpe_only_when_set() {
    let d = doc();
    let dst = dest(&d);
    let text = to_json(&d).unwrap();
    let v: Value = serde_json::from_str(&text).unwrap();
    assert!(
        v["document"]["sequences"][0]["lanes"][0]
            .get("mpe")
            .is_none()
    );
    assert!(!text.contains("\"mpe\""));
    let d2 = apply(
        &d,
        &lane_mpe(Some(MpeConfig {
            zone: MpeZone::Upper,
            members: 7,
            bend_range: 24,
        })),
    )
    .unwrap()
    .doc;
    let d2 = apply(
        &d2,
        &Command::Routing(RoutingCmd::SetMpe {
            dest: dst,
            mpe: Some(MpeConfig::default()),
        }),
    )
    .unwrap()
    .doc;
    let text = to_json(&d2).unwrap();
    let v: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(
        v["document"]["sequences"][0]["lanes"][0]["mpe"],
        json!({"zone": "upper", "members": 7, "bend_range": 24})
    );
    assert_eq!(from_json(&text).unwrap(), d2);
    // A damaged setting makes the file Corrupt, naming the field.
    let bad = text.replace("\"members\": 7", "\"members\": 40");
    assert_ne!(bad, text, "the test edits the saved text");
    match from_json(&bad) {
        Err(ModelError::Corrupt { problems }) => assert!(
            problems.iter().any(|p| p.contains("mpe.members")),
            "{problems:?}"
        ),
        other => panic!("{other:?}"),
    }
    assert_eq!(VERSION, 7);
}

#[test]
fn editing_a_destination_keeps_its_mpe() {
    let d = doc();
    let dst = dest(&d);
    let d = apply(
        &d,
        &Command::Routing(RoutingCmd::SetMpe {
            dest: dst,
            mpe: Some(MpeConfig::default()),
        }),
    )
    .unwrap()
    .doc;
    let d = apply(
        &d,
        &Command::Routing(RoutingCmd::EditDestination {
            dest: dst,
            kind: DestKind::MidiEndpoint {
                endpoint_ref: "Other Port".into(),
                group: 0,
                channel: 3,
            },
            default_patch: None,
        }),
    )
    .unwrap()
    .doc;
    assert_eq!(
        d.instruments().get(&dst).unwrap().mpe,
        Some(MpeConfig::default())
    );
    // P9: turning it into a plugin keeps MPE output too.
    let d = apply(
        &d,
        &Command::Routing(RoutingCmd::EditDestination {
            dest: dst,
            kind: DestKind::plugin("Synth"),
            default_patch: None,
        }),
    )
    .unwrap()
    .doc;
    assert_eq!(
        d.instruments().get(&dst).unwrap().mpe,
        Some(MpeConfig::default())
    );
}
