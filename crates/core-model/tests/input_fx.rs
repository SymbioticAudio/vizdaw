//! MIDI input effects in the model (plan 5g-1 Task 1; sub-project 5 spec
//! §4.5, §6.8; Rulings 12, 14, 15): a lane's chain of up to 8 effects is
//! one undoable lane edit, checked by range, saved only when not empty.

use core_model::ModelError;
use core_model::command::{Command, LaneCmd, SequenceCmd, apply};
use core_model::document::Document;
use core_model::file::{VERSION, from_json, migrate_v4_to_v5, to_json};
use core_model::history::History;
use core_model::ids::{LaneId, SeqId};
use core_model::input_fx::{
    Arp, ArpMode, Feel, InputFx, InputFxKind, MAX_INPUT_FX, NoteValue, VelocityFx, check_chain,
    input_quantize_grids,
};
use core_model::selection::ViewFilter;
use core_model::time::{PPQ, Tick};
use serde_json::{Value, json};

fn doc() -> Document {
    apply(
        &Document::new(),
        &Command::Sequence(SequenceCmd::New { name: "S".into() }),
    )
    .unwrap()
    .doc
}

fn set(chain: Vec<InputFx>) -> Command {
    Command::Lane(LaneCmd::SetInputFx {
        seq: SeqId(0),
        lane: LaneId(0),
        chain,
    })
}

fn fx(kind: InputFxKind) -> InputFx {
    InputFx::new(kind)
}

fn transpose(s: i8) -> InputFx {
    fx(InputFxKind::Transpose { semitones: s })
}

#[test]
fn a_chain_is_one_undoable_lane_edit() {
    let mut h = History::new(doc());
    let f = ViewFilter::default();
    assert!(
        h.doc()
            .lane(SeqId(0), LaneId(0))
            .unwrap()
            .input_fx
            .is_empty()
    );
    let chain = vec![transpose(12), fx(InputFxKind::Arpeggiator(Arp::default()))];
    h.execute(&set(chain.clone()), &f).unwrap();
    assert_eq!(h.undo_label(), Some("Set Input Effects"));
    let got: Vec<InputFx> = h
        .doc()
        .lane(SeqId(0), LaneId(0))
        .unwrap()
        .input_fx
        .iter()
        .cloned()
        .collect();
    assert_eq!(got, chain);
    h.undo().unwrap();
    assert!(
        h.doc()
            .lane(SeqId(0), LaneId(0))
            .unwrap()
            .input_fx
            .is_empty()
    );
}

#[test]
fn defaults_follow_the_ruling() {
    let a = Arp::default();
    assert_eq!(
        (a.mode, a.rate, a.octaves, a.gate, a.velocity, a.latch),
        (
            ArpMode::Up,
            NoteValue {
                division: 16,
                feel: Feel::Straight
            },
            1,
            50,
            None,
            false
        )
    );
    let v = VelocityFx::default();
    assert_eq!(
        (v.scale, v.offset, v.min, v.max, v.fixed),
        (100, 0, 1, 127, None)
    );
    assert_eq!(NoteValue::SIXTEENTH.ticks(), Tick(PPQ / 4));
}

#[test]
fn note_values_are_whole_ticks_from_1_1_to_1_64_triplet() {
    let nv = |division, feel| NoteValue { division, feel }.ticks().0;
    assert_eq!(nv(1, Feel::Straight), 4 * PPQ);
    assert_eq!(nv(4, Feel::Straight), PPQ);
    assert_eq!(nv(8, Feel::Dotted), PPQ * 3 / 4);
    assert_eq!(nv(8, Feel::Triplet), PPQ / 3);
    assert_eq!(nv(64, Feel::Triplet), 640);
    assert_eq!(nv(64, Feel::Dotted), 1440);
    let g = input_quantize_grids();
    assert_eq!(g[0], Tick(2 * PPQ));
    assert_eq!(g[1], Tick(3 * PPQ));
    assert_eq!(g[2], Tick(4 * PPQ / 3));
    assert_eq!(g[9], Tick(PPQ / 4));
    assert_eq!(g[14], Tick(PPQ / 12));
}

#[test]
fn hostile_chains_are_refused_and_change_nothing() {
    let d = doc();
    let arp = |f: &dyn Fn(&mut Arp)| {
        let mut a = Arp::default();
        f(&mut a);
        fx(InputFxKind::Arpeggiator(a))
    };
    let vel = |f: &dyn Fn(&mut VelocityFx)| {
        let mut v = VelocityFx::default();
        f(&mut v);
        fx(InputFxKind::Velocity(v))
    };
    let q = |grid: i64, strength: u8| {
        fx(InputFxKind::InputQuantize {
            grid: Tick(grid),
            strength,
        })
    };
    let bad: Vec<Vec<InputFx>> = vec![
        vec![transpose(1); MAX_INPUT_FX + 1],
        vec![arp(&|a| a.octaves = 0)],
        vec![arp(&|a| a.octaves = 5)],
        vec![arp(&|a| a.gate = 4)],
        vec![arp(&|a| a.gate = 201)],
        vec![arp(&|a| a.velocity = Some(0))],
        vec![arp(&|a| a.velocity = Some(128))],
        vec![arp(&|a| a.rate.division = 3)],
        vec![arp(&|a| a.rate.division = 128)],
        vec![transpose(49)],
        vec![transpose(-49)],
        vec![vel(&|v| v.scale = 401)],
        vec![vel(&|v| v.offset = 128)],
        vec![vel(&|v| v.offset = -128)],
        vec![vel(&|v| v.min = 0)],
        vec![vel(&|v| v.max = 128)],
        vec![vel(&|v| {
            v.min = 90;
            v.max = 80
        })],
        vec![vel(&|v| v.fixed = Some(0))],
        vec![fx(InputFxKind::Chord { intervals: vec![] })],
        vec![fx(InputFxKind::Chord {
            intervals: vec![1, 2, 3, 4, 5, 6, 7],
        })],
        vec![fx(InputFxKind::Chord { intervals: vec![0] })],
        vec![fx(InputFxKind::Chord {
            intervals: vec![4, 4],
        })],
        vec![fx(InputFxKind::Chord {
            intervals: vec![49],
        })],
        vec![q(1000, 100)],
        vec![q(0, 100)],
        vec![q(-3840, 100)],
        vec![q(3840, 101)],
        vec![q(3840, 100), q(1920, 50)],
    ];
    for chain in bad {
        let r = apply(&d, &set(chain.clone()));
        assert!(
            matches!(
                r,
                Err(ModelError::OutOfRange { .. } | ModelError::Invalid { .. })
            ),
            "{chain:?}: {r:?}"
        );
        assert!(check_chain(&chain).is_err());
    }
    // The edges are fine.
    let ok = vec![
        arp(&|a| {
            a.octaves = 4;
            a.gate = 200;
            a.velocity = Some(127);
            a.rate.division = 64
        }),
        arp(&|a| {
            a.octaves = 1;
            a.gate = 5;
            a.velocity = Some(1);
            a.rate.division = 1
        }),
        transpose(48),
        transpose(-48),
        vel(&|v| {
            v.scale = 400;
            v.offset = -127;
            v.min = 5;
            v.max = 5;
            v.fixed = Some(127)
        }),
        fx(InputFxKind::Chord {
            intervals: vec![-48, -12, 3, 7, 12, 48],
        }),
        q(PPQ / 12, 0),
    ];
    assert!(apply(&d, &set(ok)).is_ok());
    // Unknown lanes are refused too.
    assert!(
        apply(
            &d,
            &Command::Lane(LaneCmd::SetInputFx {
                seq: SeqId(0),
                lane: LaneId(9),
                chain: vec![]
            })
        )
        .is_err()
    );
}

fn lane_json(text: &str) -> Value {
    let v: Value = serde_json::from_str(text).unwrap();
    v["document"]["sequences"][0]["lanes"][0].clone()
}

#[test]
fn files_write_a_chain_only_when_set_and_read_it_back() {
    let d = doc();
    let text = to_json(&d).unwrap();
    assert!(lane_json(&text).get("input_fx").is_none());
    let a = Arp {
        latch: true,
        ..Arp::default()
    };
    let chain = vec![
        fx(InputFxKind::Arpeggiator(a)),
        InputFx {
            kind: InputFxKind::Transpose { semitones: -5 },
            bypass: true,
        },
    ];
    let d2 = apply(&d, &set(chain)).unwrap().doc;
    let text = to_json(&d2).unwrap();
    assert_eq!(
        lane_json(&text)["input_fx"],
        json!([
            {"kind": {"arpeggiator": {"mode": "up", "rate": {"division": 16}, "octaves": 1, "gate": 50, "latch": true}}},
            {"kind": {"transpose": {"semitones": -5}}, "bypass": true}
        ])
    );
    assert_eq!(from_json(&text).unwrap(), d2);
    assert_eq!(VERSION, 6);
}

/// Ruling 18: a version-4 project opens with empty chains; the step is the
/// identity.
#[test]
fn a_version_4_project_opens_with_empty_chains() {
    let d = doc();
    let mut v: Value = serde_json::from_str(&to_json(&d).unwrap()).unwrap();
    assert_eq!(v["version"], json!(6));
    v["version"] = json!(4);
    let back = from_json(&serde_json::to_string(&v).unwrap()).unwrap();
    assert_eq!(back, d);
    assert!(back.lane(SeqId(0), LaneId(0)).unwrap().input_fx.is_empty());
    assert!(to_json(&back).unwrap().contains("\"version\": 6"));
    assert_eq!(migrate_v4_to_v5(v.clone()).unwrap(), v);
}

/// Ruling 18: a version-5 file with a chain reads back in this build, and
/// a newer version is refused by its number, not as Corrupt.
#[test]
fn a_version_5_file_with_a_chain_reads_back_and_a_newer_one_names_its_version() {
    let d = apply(&doc(), &set(vec![transpose(7)])).unwrap().doc;
    let text = to_json(&d).unwrap();
    assert!(text.contains("\"version\": 6"));
    assert_eq!(from_json(&text).unwrap(), d);
    let mut v: Value = serde_json::from_str(&text).unwrap();
    v["version"] = json!(7);
    assert!(matches!(
        from_json(&serde_json::to_string(&v).unwrap()),
        Err(ModelError::NewerFormat { found: 7 })
    ));
}

/// Ruling 14: Duplicate Track clones the lane, chain included.
#[test]
fn duplicate_track_copies_the_chain() {
    let d = apply(&doc(), &set(vec![transpose(7)])).unwrap().doc;
    let d = apply(
        &d,
        &Command::Lane(LaneCmd::Duplicate {
            seq: SeqId(0),
            lane: LaneId(0),
        }),
    )
    .unwrap()
    .doc;
    let lanes = &d.sequence(SeqId(0)).unwrap().lanes;
    assert_eq!(lanes.len(), 2);
    assert_eq!(lanes[1].input_fx, lanes[0].input_fx);
}

#[test]
fn a_damaged_chain_in_a_file_is_refused() {
    let text = to_json(&doc()).unwrap();
    let mut v: Value = serde_json::from_str(&text).unwrap();
    v["document"]["sequences"][0]["lanes"][0]["input_fx"] = json!([{"kind": {"arpeggiator": {"mode": "up", "rate": {"division": 16}, "octaves": 9, "gate": 50}}}]);
    match from_json(&serde_json::to_string(&v).unwrap()) {
        Err(ModelError::Corrupt { problems }) => {
            assert!(
                problems.iter().any(|p| p.contains("arpeggiator.octaves")),
                "{problems:?}"
            )
        }
        other => panic!("{other:?}"),
    }
    v["document"]["sequences"][0]["lanes"][0]["input_fx"] = json!([{"kind": {"flanger": {}}}]);
    assert!(from_json(&serde_json::to_string(&v).unwrap()).is_err());
}
