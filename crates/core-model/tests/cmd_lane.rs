use core_model::ModelError;
use core_model::command::{Command, LaneCmd, SequenceCmd, apply};
use core_model::document::{DestKind, Destination, Document, DocumentRepr, Settings};
use core_model::ids::{AnyId, Counters, DestId, LaneId, OutputPathId, SeqId, TakeId};
use core_model::lane::QuantizeSpec;
use core_model::rules::validate;
use core_model::time::Tick;

fn run(d: &Document, c: LaneCmd) -> core_model::Result<Document> {
    apply(d, &Command::Lane(c)).map(|a| a.doc)
}

/// One destination (dest0) and one new sequence seq0 with lane0/take0.
fn base() -> Document {
    let d = Document::from_parts(DocumentRepr {
        settings: Settings::default(),
        instruments: vec![Destination {
            id: DestId(0),
            name: "Piano".into(),
            kind: DestKind::plugin("p"),
            default_patch: None,
            return_latency_ms: 0.0,
            midi_mix: Default::default(),
            mpe: None,
        }],
        outputs: vec![],
        media: vec![],
        sequences: vec![],
        mixer: None,
        counters: Counters {
            dest: 1,
            plugin: 1,
            ..Counters::default()
        },
    })
    .unwrap();
    apply(
        &d,
        &Command::Sequence(SequenceCmd::New { name: "S".into() }),
    )
    .unwrap()
    .doc
}

fn lane_ids(d: &Document) -> Vec<LaneId> {
    d.sequence(SeqId(0))
        .unwrap()
        .lanes
        .iter()
        .map(|l| l.id)
        .collect()
}

#[test]
fn add_inserts_at_index_and_reorder_moves() {
    let d = run(
        &base(),
        LaneCmd::Add {
            seq: SeqId(0),
            name: "B".into(),
            index: None,
        },
    )
    .unwrap();
    let d = run(
        &d,
        LaneCmd::Add {
            seq: SeqId(0),
            name: "C".into(),
            index: Some(0),
        },
    )
    .unwrap();
    assert_eq!(lane_ids(&d), vec![LaneId(2), LaneId(0), LaneId(1)]);
    let d = run(
        &d,
        LaneCmd::Reorder {
            seq: SeqId(0),
            lane: LaneId(2),
            to: 99,
        },
    )
    .unwrap();
    assert_eq!(lane_ids(&d), vec![LaneId(0), LaneId(1), LaneId(2)]);
    assert!(validate(&d).is_empty());
}

#[test]
fn remove_reports_lane() {
    let a = apply(
        &base(),
        &Command::Lane(LaneCmd::Remove {
            seq: SeqId(0),
            lane: LaneId(0),
        }),
    )
    .unwrap();
    assert_eq!(a.report.removed, vec![AnyId::Lane(LaneId(0))]);
    assert!(lane_ids(&a.doc).is_empty());
    assert_eq!(
        Command::Lane(LaneCmd::Remove {
            seq: SeqId(0),
            lane: LaneId(0),
        })
        .name(),
        "Delete Track"
    );
}

#[test]
fn destination_and_output_must_exist() {
    let d = run(
        &base(),
        LaneCmd::SetDest {
            seq: SeqId(0),
            lane: LaneId(0),
            dest: Some(DestId(0)),
        },
    )
    .unwrap();
    assert_eq!(
        d.lane(SeqId(0), LaneId(0)).unwrap().midi_dest,
        Some(DestId(0))
    );
    assert_eq!(
        run(
            &d,
            LaneCmd::SetDest {
                seq: SeqId(0),
                lane: LaneId(0),
                dest: Some(DestId(7))
            }
        ),
        Err(ModelError::UnknownId(AnyId::Dest(DestId(7))))
    );
    assert_eq!(
        run(
            &d,
            LaneCmd::SetOutput {
                seq: SeqId(0),
                lane: LaneId(0),
                output: Some(OutputPathId(0))
            }
        ),
        Err(ModelError::UnknownId(AnyId::Output(OutputPathId(0))))
    );
}

#[test]
fn flags_change_only_given_values() {
    let d = run(
        &base(),
        LaneCmd::SetFlags {
            seq: SeqId(0),
            lane: LaneId(0),
            mute: Some(true),
            solo: None,
            arm: Some(true),
        },
    )
    .unwrap();
    let d = run(
        &d,
        LaneCmd::SetFlags {
            seq: SeqId(0),
            lane: LaneId(0),
            mute: None,
            solo: Some(true),
            arm: None,
        },
    )
    .unwrap();
    let l = d.lane(SeqId(0), LaneId(0)).unwrap();
    assert!(l.mute && l.solo && l.arm);
}

#[test]
fn length_quantize_and_shift_are_validated() {
    let id = (SeqId(0), LaneId(0));
    assert!(matches!(
        run(
            &base(),
            LaneCmd::SetLength {
                seq: id.0,
                lane: id.1,
                length: Some(Tick(0)),
                looped: true,
                locked: false
            }
        ),
        Err(ModelError::OutOfRange { .. })
    ));
    let d = run(
        &base(),
        LaneCmd::SetLength {
            seq: id.0,
            lane: id.1,
            length: Some(Tick(960)),
            looped: true,
            locked: true,
        },
    )
    .unwrap();
    let l = d.lane(id.0, id.1).unwrap();
    assert_eq!(
        (l.length, l.looped, l.length_locked),
        (Some(Tick(960)), true, true)
    );
    assert!(
        run(
            &d,
            LaneCmd::SetPlayQuantize {
                seq: id.0,
                lane: id.1,
                quantize: Some(QuantizeSpec::grid(Tick(0), 100))
            }
        )
        .is_err()
    );
    assert!(
        run(
            &d,
            LaneCmd::SetPlayQuantize {
                seq: id.0,
                lane: id.1,
                quantize: Some(QuantizeSpec::grid(Tick(960), 101))
            }
        )
        .is_err()
    );
    let d = run(
        &d,
        LaneCmd::SetPlayShift {
            seq: id.0,
            lane: id.1,
            shift: Tick(-30),
        },
    )
    .unwrap();
    assert_eq!(d.lane(id.0, id.1).unwrap().play_shift, Tick(-30));
}

#[test]
fn add_take_becomes_active_and_can_switch_back() {
    let d = run(
        &base(),
        LaneCmd::AddTake {
            seq: SeqId(0),
            lane: LaneId(0),
            name: "Take 2".into(),
        },
    )
    .unwrap();
    let l = d.lane(SeqId(0), LaneId(0)).unwrap();
    assert_eq!(l.takes.len(), 2);
    assert_eq!(l.active_take, TakeId(1));
    let d = run(
        &d,
        LaneCmd::SetActiveTake {
            seq: SeqId(0),
            lane: LaneId(0),
            take: TakeId(0),
        },
    )
    .unwrap();
    assert_eq!(d.lane(SeqId(0), LaneId(0)).unwrap().active_take, TakeId(0));
    assert_eq!(
        run(
            &d,
            LaneCmd::SetActiveTake {
                seq: SeqId(0),
                lane: LaneId(0),
                take: TakeId(5)
            }
        ),
        Err(ModelError::UnknownId(AnyId::Take(TakeId(5))))
    );
}

fn note(tick: i64) -> core_model::event::EventSpec {
    use core_model::event::{EventBody, EventSpec};
    EventSpec::new(
        Tick(tick),
        EventBody::Note {
            pitch: 60,
            velocity: 100,
            release_velocity: 0,
            duration: Tick(10),
            attribute: None,
            per_note: Default::default(),
        },
    )
}

fn auto_point(tick: i64) -> core_model::event::EventSpec {
    use core_model::automation::AutomationTarget;
    use core_model::event::{EventBody, EventSpec};
    EventSpec::new(
        Tick(tick),
        EventBody::Automation {
            target: AutomationTarget::VelocityScale(DestId(0)),
            value: 0.5,
        },
    )
}

fn insert(d: &Document, lane: u64, events: Vec<core_model::event::EventSpec>) -> Document {
    use core_model::command::EventCmd;
    apply(
        d,
        &Command::Event(EventCmd::Insert {
            seq: SeqId(0),
            lane: LaneId(lane),
            events,
        }),
    )
    .unwrap()
    .doc
}

#[test]
fn duplicate_lane_copies_and_indexes_events() {
    use core_model::command::EventCmd;
    // lane0 is "Lane 1"; add B after it, then give lane0 two takes.
    let d = run(
        &base(),
        LaneCmd::Add {
            seq: SeqId(0),
            name: "B".into(),
            index: None,
        },
    )
    .unwrap();
    let d = insert(&d, 0, vec![note(0), note(100)]);
    let d = run(
        &d,
        LaneCmd::AddTake {
            seq: SeqId(0),
            lane: LaneId(0),
            name: "Take 2".into(),
        },
    )
    .unwrap();
    let d = insert(&d, 0, vec![note(5)]);
    let d = run(
        &d,
        LaneCmd::SetFlags {
            seq: SeqId(0),
            lane: LaneId(0),
            mute: Some(true),
            solo: None,
            arm: Some(true),
        },
    )
    .unwrap();
    let before = d.clone();
    let a = apply(
        &d,
        &Command::Lane(LaneCmd::Duplicate {
            seq: SeqId(0),
            lane: LaneId(0),
        }),
    )
    .unwrap();
    let d = a.doc;
    let s = d.sequence(SeqId(0)).unwrap();
    let names: Vec<_> = s.lanes.iter().map(|l| l.name.as_str()).collect();
    assert_eq!(names, vec!["Track 1", "Track 1 copy", "B"]);
    let (src, copy) = (&s.lanes[0], &s.lanes[1]);
    assert!(!copy.arm);
    assert!(copy.mute, "settings are copied");
    assert_eq!(src.takes.len(), copy.takes.len());
    let mut ids = std::collections::BTreeSet::new();
    for (a, b) in src.takes.iter().zip(copy.takes.iter()) {
        assert_eq!(a.events.len(), b.events.len());
        assert_ne!(a.id, b.id);
        assert!(ids.insert(format!("take{}", a.id.0)) && ids.insert(format!("take{}", b.id.0)));
        for e in a.events.values().chain(b.events.values()) {
            assert!(ids.insert(format!("ev{}", e.id.0)), "shared event id");
        }
    }
    assert_ne!(src.id, copy.id);
    assert_eq!(
        copy.active().unwrap().name,
        src.active().unwrap().name,
        "the active take carries over"
    );
    // The original is untouched.
    assert_eq!(before.sequence(SeqId(0)).unwrap().lanes[0], *src);
    for id in [
        AnyId::Lane(copy.id),
        AnyId::Take(copy.takes[0].id),
        AnyId::Take(copy.takes[1].id),
        AnyId::Event(copy.takes[0].events.values().next().unwrap().id),
    ] {
        assert!(a.report.created.contains(&id), "{id:?}");
    }
    // The copy is indexed: its event moves.
    let ev = copy.takes[0].events.values().next().unwrap().id;
    apply(
        &d,
        &Command::Event(EventCmd::Move {
            events: vec![ev],
            delta: Tick(7),
            to_lane: None,
        }),
    )
    .unwrap();
    assert!(validate(&d).is_empty());
}

#[test]
fn duplicate_lane_skips_automation_points() {
    let d = insert(
        &base(),
        0,
        vec![
            note(0),
            note(50),
            auto_point(0),
            auto_point(60),
            auto_point(120),
        ],
    );
    let d = run(
        &d,
        LaneCmd::Duplicate {
            seq: SeqId(0),
            lane: LaneId(0),
        },
    )
    .unwrap();
    let s = d.sequence(SeqId(0)).unwrap();
    let count = |l: &core_model::lane::Lane, auto: bool| {
        l.takes
            .iter()
            .flat_map(|t| t.events.values())
            .filter(|e| matches!(e.body, core_model::event::EventBody::Automation { .. }) == auto)
            .count()
    };
    assert_eq!(
        (count(&s.lanes[0], false), count(&s.lanes[0], true)),
        (2, 3)
    );
    assert_eq!(
        (count(&s.lanes[1], false), count(&s.lanes[1], true)),
        (2, 0)
    );
}

#[test]
fn duplicate_lane_refuses_a_return_lane() {
    use core_model::lane::{InputChannels, LaneInput};
    let d = base();
    // dest1: an external destination for the return to name.
    let d = apply(
        &d,
        &Command::Routing(core_model::command::RoutingCmd::AddDestination {
            name: "Hw".into(),
            kind: DestKind::MidiEndpoint {
                endpoint_ref: "hw".into(),
                group: 0,
                channel: 0,
            },
            default_patch: None,
        }),
    )
    .unwrap()
    .doc;
    let ext = *d
        .instruments()
        .iter()
        .find(|(_, x)| x.kind.is_external())
        .unwrap()
        .0;
    let d = run(
        &d,
        LaneCmd::SetInput {
            seq: SeqId(0),
            lane: LaneId(0),
            input: Some(LaneInput::Audio {
                channels: InputChannels::Mono(0),
                return_of: Some(ext),
            }),
        },
    )
    .unwrap();
    let err = run(
        &d,
        LaneCmd::Duplicate {
            seq: SeqId(0),
            lane: LaneId(0),
        },
    )
    .unwrap_err();
    assert_eq!(
        err.to_string(),
        ModelError::invalid(
            "Duplicate Track does not copy a Track Synth return track: duplicate its MIDI track and run Track Synth again."
        )
        .to_string()
    );
    // Unknown ids are refused too.
    assert!(
        run(
            &d,
            LaneCmd::Duplicate {
                seq: SeqId(0),
                lane: LaneId(99)
            }
        )
        .is_err()
    );
    assert!(
        run(
            &d,
            LaneCmd::Duplicate {
                seq: SeqId(9),
                lane: LaneId(0)
            }
        )
        .is_err()
    );
}

#[test]
fn rename_refuses_a_blank_name() {
    let rename = |name: &str| {
        run(
            &base(),
            LaneCmd::Rename {
                seq: SeqId(0),
                lane: LaneId(0),
                name: name.into(),
            },
        )
    };
    for bad in [
        "",
        "   ",
        "a\u{7}b",
        "\t",
        "a\u{200B}b",
        "\u{FEFF}",
        "x\u{2060}",
        "\u{200D}",
    ] {
        assert!(rename(bad).is_err(), "{bad:?}");
    }
    let d = rename("  Bass ").unwrap();
    assert_eq!(d.sequence(SeqId(0)).unwrap().lanes[0].name, "Bass");
}
