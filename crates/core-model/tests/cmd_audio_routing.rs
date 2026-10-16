use core_model::ModelError;
use core_model::command::{AudioCmd, Command, EventCmd, LaneCmd, RoutingCmd, SequenceCmd, apply};
use core_model::document::{DestKind, Document, Patch};
use core_model::event::{EventBody, EventSpec, Fade, FadeShape};
use core_model::ids::{AnyId, DestId, EventId, LaneId, MediaId, OutputPathId, SeqId};
use core_model::query::resolve_event;
use core_model::rules::validate;
use core_model::time::{PPQ, Tick};

fn run(d: &Document, c: Command) -> core_model::Result<Document> {
    apply(d, &c).map(|a| a.doc)
}

fn routing(c: RoutingCmd) -> Command {
    Command::Routing(c)
}

fn audio(c: AudioCmd) -> Command {
    Command::Audio(c)
}

fn endpoint(group: u8, channel: u8) -> DestKind {
    DestKind::MidiEndpoint {
        endpoint_ref: "ep".into(),
        group,
        channel,
    }
}

/// seq0/lane0; dest0 (g1 c2), dest1 (plugin); out0, out1; media0 and media1
/// (96 000 frames at 48 kHz = 2 s = 4 beats at 120 BPM).
fn base() -> Document {
    let mut d = run(
        &Document::new(),
        Command::Sequence(SequenceCmd::New { name: "S".into() }),
    )
    .unwrap();
    for c in [
        RoutingCmd::AddDestination {
            name: "Ext".into(),
            kind: endpoint(1, 2),
            default_patch: None,
        },
        RoutingCmd::AddDestination {
            name: "Plug".into(),
            kind: DestKind::PluginInstrument {
                plugin_ref: "p".into(),
            },
            default_patch: Some(Patch {
                program: 3,
                bank: Some((0, 1)),
            }),
        },
        RoutingCmd::AddOutput {
            name: "Main".into(),
            reference: "bus:0".into(),
        },
        RoutingCmd::AddOutput {
            name: "Aux".into(),
            reference: "bus:1".into(),
        },
        RoutingCmd::AddMedia {
            path: "Media/a.wav".into(),
            sample_rate: 48_000,
            channels: 2,
            length_frames: 96_000,
            content_hash: None,
        },
        RoutingCmd::AddMedia {
            path: "Media/b.wav".into(),
            sample_rate: 48_000,
            channels: 2,
            length_frames: 96_000,
            content_hash: None,
        },
    ] {
        d = run(&d, routing(c)).unwrap();
    }
    d
}

fn with_region(d: &Document) -> Document {
    run(
        d,
        audio(AudioCmd::Insert {
            seq: SeqId(0),
            lane: LaneId(0),
            tick: Tick(PPQ),
            media: MediaId(0),
            source_offset: 0,
            length: 96_000,
            gain_db: 0.0,
            fade_in: Fade {
                length: Tick(10),
                shape: FadeShape::Linear,
            },
            fade_out: Fade {
                length: Tick(20),
                shape: FadeShape::SCurve,
            },
        }),
    )
    .unwrap()
}

fn region(d: &Document, id: u64) -> (Tick, u64, u64, Fade, Fade) {
    let (e, _) = d.event(EventId(id)).unwrap();
    match e.body {
        EventBody::AudioRegion {
            source_offset,
            length,
            fade_in,
            fade_out,
            ..
        } => (e.tick, source_offset, length, fade_in, fade_out),
        _ => panic!("not a region"),
    }
}

#[test]
fn add_items_allocate_ids_and_validate() {
    let d = base();
    assert_eq!(d.instruments().len(), 2);
    assert_eq!(d.outputs().len(), 2);
    assert_eq!(d.media().len(), 2);
    assert!(
        run(
            &d,
            routing(RoutingCmd::AddDestination {
                name: "x".into(),
                kind: endpoint(16, 0),
                default_patch: None
            })
        )
        .is_err()
    );
    assert!(
        run(
            &d,
            routing(RoutingCmd::AddMedia {
                path: "x".into(),
                sample_rate: 0,
                channels: 1,
                length_frames: 1,
                content_hash: None
            })
        )
        .is_err()
    );
    assert!(validate(&d).is_empty());
}

#[test]
fn editing_a_destination_reroutes_its_users() {
    let d = run(
        &base(),
        Command::Lane(LaneCmd::SetDest {
            seq: SeqId(0),
            lane: LaneId(0),
            dest: Some(DestId(0)),
        }),
    )
    .unwrap();
    let d = run(
        &d,
        Command::Event(EventCmd::Insert {
            seq: SeqId(0),
            lane: LaneId(0),
            events: vec![EventSpec::new(Tick(0), EventBody::note(60, 1, Tick(1)))],
        }),
    )
    .unwrap();
    assert_eq!(
        resolve_event(&d, EventId(0)).unwrap().group_channel,
        Some((1, 2))
    );
    let d = run(
        &d,
        routing(RoutingCmd::EditDestination {
            dest: DestId(0),
            kind: endpoint(3, 4),
            default_patch: None,
        }),
    )
    .unwrap();
    assert_eq!(
        resolve_event(&d, EventId(0)).unwrap().group_channel,
        Some((3, 4))
    );
    let d = run(
        &d,
        routing(RoutingCmd::RenameDestination {
            dest: DestId(0),
            name: "New".into(),
        }),
    )
    .unwrap();
    assert_eq!(d.instruments().get(&DestId(0)).unwrap().name, "New");
}

#[test]
fn removing_a_destination_in_use_needs_a_replacement() {
    let d = run(
        &base(),
        Command::Lane(LaneCmd::SetDest {
            seq: SeqId(0),
            lane: LaneId(0),
            dest: Some(DestId(0)),
        }),
    )
    .unwrap();
    let mut spec = EventSpec::new(Tick(0), EventBody::note(60, 1, Tick(1)));
    spec.dest = Some(DestId(0));
    let d = run(
        &d,
        Command::Event(EventCmd::Insert {
            seq: SeqId(0),
            lane: LaneId(0),
            events: vec![spec],
        }),
    )
    .unwrap();
    assert_eq!(
        run(
            &d,
            routing(RoutingCmd::RemoveDestination {
                dest: DestId(0),
                replacement: None
            })
        ),
        Err(ModelError::InUse {
            users: vec![AnyId::Lane(LaneId(0)), AnyId::Event(EventId(0))]
        })
    );
    assert!(
        run(
            &d,
            routing(RoutingCmd::RemoveDestination {
                dest: DestId(0),
                replacement: Some(DestId(0))
            })
        )
        .is_err()
    );
    assert!(
        run(
            &d,
            routing(RoutingCmd::RemoveDestination {
                dest: DestId(0),
                replacement: Some(DestId(9))
            })
        )
        .is_err()
    );
    let d = run(
        &d,
        routing(RoutingCmd::RemoveDestination {
            dest: DestId(0),
            replacement: Some(DestId(1)),
        }),
    )
    .unwrap();
    assert_eq!(
        d.lane(SeqId(0), LaneId(0)).unwrap().midi_dest,
        Some(DestId(1))
    );
    assert_eq!(d.event(EventId(0)).unwrap().0.dest, Some(DestId(1)));
    assert!(validate(&d).is_empty());
    // Unused destinations go without a replacement.
    assert!(
        run(
            &base(),
            routing(RoutingCmd::RemoveDestination {
                dest: DestId(1),
                replacement: None
            })
        )
        .is_ok()
    );
}

#[test]
fn removing_outputs_and_media_in_use() {
    let d = run(
        &base(),
        Command::Lane(LaneCmd::SetOutput {
            seq: SeqId(0),
            lane: LaneId(0),
            output: Some(OutputPathId(0)),
        }),
    )
    .unwrap();
    assert!(matches!(
        run(
            &d,
            routing(RoutingCmd::RemoveOutput {
                output: OutputPathId(0),
                replacement: None
            })
        ),
        Err(ModelError::InUse { .. })
    ));
    let d2 = run(
        &d,
        routing(RoutingCmd::RemoveOutput {
            output: OutputPathId(0),
            replacement: Some(OutputPathId(1)),
        }),
    )
    .unwrap();
    assert_eq!(
        d2.lane(SeqId(0), LaneId(0)).unwrap().audio_out,
        Some(OutputPathId(1))
    );
    let d = with_region(&d);
    assert_eq!(
        run(
            &d,
            routing(RoutingCmd::RemoveMedia {
                media: MediaId(0),
                replacement: None
            })
        ),
        Err(ModelError::InUse {
            users: vec![AnyId::Event(EventId(0))]
        })
    );
    let d = run(
        &d,
        routing(RoutingCmd::RemoveMedia {
            media: MediaId(0),
            replacement: Some(MediaId(1)),
        }),
    )
    .unwrap();
    assert!(matches!(
        d.event(EventId(0)).unwrap().0.body,
        EventBody::AudioRegion {
            media: MediaId(1),
            ..
        }
    ));
    assert!(validate(&d).is_empty());
}

#[test]
fn insert_region_checks_media_bounds() {
    let bad = audio(AudioCmd::Insert {
        seq: SeqId(0),
        lane: LaneId(0),
        tick: Tick(0),
        media: MediaId(0),
        source_offset: 1,
        length: 96_000,
        gain_db: 0.0,
        fade_in: Fade::default(),
        fade_out: Fade::default(),
    });
    assert!(matches!(
        run(&base(), bad),
        Err(ModelError::OutOfRange {
            field: "region.source_end",
            ..
        })
    ));
}

#[test]
fn trim_moves_start_and_offset_together() {
    let d = with_region(&base());
    // 24 000 frames = 0.5 s = 1 beat at 120 BPM.
    let d = run(
        &d,
        audio(AudioCmd::Trim {
            event: EventId(0),
            front_frames: 24_000,
            back_frames: 24_000,
        }),
    )
    .unwrap();
    let (tick, off, len, _, _) = region(&d, 0);
    assert_eq!((tick, off, len), (Tick(2 * PPQ), 24_000, 48_000));
    assert!(
        run(
            &d,
            audio(AudioCmd::Trim {
                event: EventId(0),
                front_frames: -30_000,
                back_frames: 0
            })
        )
        .is_err()
    );
    assert!(
        run(
            &d,
            audio(AudioCmd::Trim {
                event: EventId(0),
                front_frames: 0,
                back_frames: 48_000
            })
        )
        .is_err()
    );
}

#[test]
fn split_divides_frames_and_fades() {
    let d = with_region(&base());
    let a = apply(
        &d,
        &audio(AudioCmd::Split {
            event: EventId(0),
            at: Tick(2 * PPQ),
        }),
    )
    .unwrap();
    assert_eq!(a.report.created, vec![AnyId::Event(EventId(1))]);
    let (t0, o0, l0, fi0, fo0) = region(&a.doc, 0);
    let (t1, o1, l1, fi1, fo1) = region(&a.doc, 1);
    assert_eq!((t0, o0, l0), (Tick(PPQ), 0, 24_000));
    assert_eq!((t1, o1, l1), (Tick(2 * PPQ), 24_000, 72_000));
    assert_eq!(
        (fi0.length, fo0, fi1, fo1.length),
        (Tick(10), Fade::default(), Fade::default(), Tick(20))
    );
    assert!(
        run(
            &d,
            audio(AudioCmd::Split {
                event: EventId(0),
                at: Tick(PPQ)
            })
        )
        .is_err()
    );
    assert!(
        run(
            &d,
            audio(AudioCmd::Split {
                event: EventId(0),
                at: Tick(9 * PPQ)
            })
        )
        .is_err()
    );
}

#[test]
fn gain_and_fades() {
    let d = with_region(&base());
    let d = run(
        &d,
        audio(AudioCmd::SetGain {
            event: EventId(0),
            gain_db: -12.5,
        }),
    )
    .unwrap();
    assert!(
        matches!(d.event(EventId(0)).unwrap().0.body, EventBody::AudioRegion { gain_db, .. } if gain_db == -12.5)
    );
    assert!(
        run(
            &d,
            audio(AudioCmd::SetGain {
                event: EventId(0),
                gain_db: f32::INFINITY
            })
        )
        .is_err()
    );
    let f = Fade {
        length: Tick(99),
        shape: FadeShape::Exponential,
    };
    let d = run(
        &d,
        audio(AudioCmd::SetFades {
            event: EventId(0),
            fade_in: f,
            fade_out: f,
        }),
    )
    .unwrap();
    assert_eq!(region(&d, 0).3, f);
    assert!(
        run(
            &d,
            audio(AudioCmd::SetFades {
                event: EventId(0),
                fade_in: Fade {
                    length: Tick(-1),
                    shape: FadeShape::Linear
                },
                fade_out: f
            })
        )
        .is_err()
    );
}

/// A 4-beat region at beat 1 with a 1.5-beat fade in and a 2-beat fade out.
fn with_long_fades(d: &Document) -> Document {
    run(
        d,
        audio(AudioCmd::Insert {
            seq: SeqId(0),
            lane: LaneId(0),
            tick: Tick(PPQ),
            media: MediaId(0),
            source_offset: 0,
            length: 96_000,
            gain_db: 0.0,
            fade_in: Fade {
                length: Tick(3 * PPQ / 2),
                shape: FadeShape::Linear,
            },
            fade_out: Fade {
                length: Tick(2 * PPQ),
                shape: FadeShape::Linear,
            },
        }),
    )
    .unwrap()
}

#[test]
fn split_and_trim_clamp_fades_to_the_part_length() {
    let d = with_long_fades(&base());
    // Split one beat in: part one is 1 beat, part two 3 beats.
    let s = run(
        &d,
        audio(AudioCmd::Split {
            event: EventId(0),
            at: Tick(2 * PPQ),
        }),
    )
    .unwrap();
    let (_, _, _, fi0, _) = region(&s, 0);
    let (_, _, _, _, fo1) = region(&s, 1);
    assert_eq!(fi0.length, Tick(PPQ));
    assert_eq!(fo1.length, Tick(2 * PPQ));
    // Trim three beats off the end: one beat left, both fades clamp to it.
    let t = run(
        &d,
        audio(AudioCmd::Trim {
            event: EventId(0),
            front_frames: 0,
            back_frames: 72_000,
        }),
    )
    .unwrap();
    let (_, _, len, fi, fo) = region(&t, 0);
    assert_eq!(len, 24_000);
    assert_eq!((fi.length, fo.length), (Tick(PPQ), Tick(PPQ)));
}
