use core_engine::EngineError;
use core_engine::backend::MidiPortInfo;
use core_engine::fixtures::{BAR, Builder, Q};
use core_engine::midi1::Midi1Msg;
use core_engine::schedule::{
    BuiltinRoute, CompileOptions, CompileReport, Entry, MAX_TARGETS, Schedule, TargetIdx,
    TargetKind, TargetTable, WireMsg, compile,
};
use core_model::command::{Command, SequenceCmd};
use core_model::document::{DestKind, Document, Patch};
use core_model::event::EventBody;
use core_model::ids::SeqId;
use core_model::time::Tick;
use std::sync::Arc;

fn compile_all(
    doc: &Document,
    seq: SeqId,
    opts: &CompileOptions,
) -> (Schedule, CompileReport, TargetTable) {
    let mut table = TargetTable::new();
    let (s, r) = compile(doc, seq, opts, &mut |k: &TargetKind| {
        table.intern(k.clone()).map(Ok)
    })
    .unwrap();
    (s, r, table)
}

fn bytes(s: &Schedule) -> Vec<(u64, u8, Vec<u8>)> {
    s.entries
        .iter()
        .map(|e| (e.frame, e.target.0, s.bytes(&e.msg).to_vec()))
        .collect()
}

fn one_bar() -> (Document, SeqId) {
    let mut b = Builder::new();
    let synth = b.dest("Synth", "builtin:synth", 0).unwrap();
    let (seq, lane) = b.sequence("Bar", "Notes", Some(synth)).unwrap();
    b.notes(
        seq,
        lane,
        &[
            (0, 60, 0x8000, Q / 2),
            (Q, 62, 0x8000, Q / 2),
            (2 * Q, 64, 0x8000, Q / 2),
            (3 * Q, 65, 0x8000, Q / 2),
        ],
    )
    .unwrap();
    (b.finish(), seq)
}

#[test]
fn entry_is_24_bytes() {
    assert_eq!(std::mem::size_of::<Entry>(), 24);
}

#[test]
fn quarter_notes_at_120_bpm_land_on_exact_frames() {
    let (doc, seq) = one_bar();
    let (s, r, table) = compile_all(&doc, seq, &CompileOptions::new(48_000.0));
    let ons: Vec<u64> = s
        .entries
        .iter()
        .filter(|e| s.bytes(&e.msg)[0] & 0xF0 == 0x90)
        .map(|e| e.frame)
        .collect();
    assert_eq!(ons, vec![0, 24_000, 48_000, 72_000]);
    assert_eq!(s.length_frames, 96_000);
    assert_eq!(s.loop_range, Some((0, 96_000)));
    assert_eq!(r.events, 4);
    assert_eq!(r.entries, 8);
    assert!(r.unroutable.is_empty() && r.warnings.is_empty());
    assert_eq!(table.get(TargetIdx(0)), Some(&TargetKind::BuiltinSynth));
    assert_eq!(s.used_channels[0], 1);
}

#[test]
fn same_frame_order_is_off_patch_control_on() {
    let mut b = Builder::new();
    let synth = b.dest("Synth", "builtin:synth", 0).unwrap();
    let (seq, lane) = b.sequence("S", "L", Some(synth)).unwrap();
    // A note ending exactly where the same pitch starts again, plus a
    // controller and a program change on that frame, inserted in the
    // "wrong" order.
    b.notes(seq, lane, &[(Q, 60, 0x8000, Q)]).unwrap();
    b.event(
        seq,
        lane,
        2 * Q,
        EventBody::Cc {
            index: 7,
            value: 1 << 25,
        },
    )
    .unwrap();
    b.notes(seq, lane, &[(2 * Q, 60, 0x8000, Q)]).unwrap();
    b.event(
        seq,
        lane,
        2 * Q,
        EventBody::Program {
            program: 5,
            bank: Some((0, 1)),
        },
    )
    .unwrap();
    let doc = b.finish();
    let (s, _, _) = compile_all(&doc, seq, &CompileOptions::new(48_000.0));
    let at: Vec<Vec<u8>> = bytes(&s)
        .into_iter()
        .filter(|(f, _, _)| *f == 48_000)
        .map(|(_, _, b)| b)
        .collect();
    assert_eq!(
        at,
        vec![
            vec![0x80, 60, 0],
            vec![0xB0, 0, 0],
            vec![0xB0, 32, 1],
            vec![0xC0, 5],
            vec![0xB0, 7, 1],
            vec![0x90, 60, 64],
        ]
    );
}

#[test]
fn raw_bank_select_ccs_stay_with_the_program_change() {
    let mut b = Builder::new();
    let synth = b.dest("Synth", "builtin:synth", 0).unwrap();
    let (seq, lane) = b.sequence("S", "L", Some(synth)).unwrap();
    let cc = |index: u8, v7: u32| EventBody::Cc {
        index,
        value: v7 << 25,
    };
    // Volume first in source order, then CC0, CC32, PC.
    b.event(seq, lane, Q, cc(7, 100)).unwrap();
    b.event(seq, lane, Q, cc(0, 1)).unwrap();
    b.event(seq, lane, Q, cc(32, 2)).unwrap();
    b.event(
        seq,
        lane,
        Q,
        EventBody::Program {
            program: 5,
            bank: None,
        },
    )
    .unwrap();
    let doc = b.finish();
    let (s, _, _) = compile_all(&doc, seq, &CompileOptions::new(48_000.0));
    let at: Vec<Vec<u8>> = bytes(&s)
        .into_iter()
        .filter(|(f, _, _)| *f == 24_000)
        .map(|(_, _, b)| b)
        .collect();
    assert_eq!(
        at,
        vec![
            vec![0xB0, 0, 1],
            vec![0xB0, 32, 2],
            vec![0xC0, 5],
            vec![0xB0, 7, 100],
        ]
    );
}

/// `(frame, status)` of every note-on/off, in schedule order.
fn note_frames(s: &Schedule) -> Vec<(u64, u8)> {
    bytes(s)
        .into_iter()
        .filter(|(_, _, b)| matches!(b[0] & 0xF0, 0x80 | 0x90))
        .map(|(f, _, b)| (f, b[0]))
        .collect()
}

#[test]
fn zero_duration_note_off_follows_its_on() {
    let mut b = Builder::new();
    let synth = b.dest("Synth", "builtin:synth", 0).unwrap();
    let (seq, lane) = b.sequence("S", "L", Some(synth)).unwrap();
    b.notes(seq, lane, &[(Q, 60, 0x8000, 0)]).unwrap();
    let doc = b.finish();
    let (s, _, _) = compile_all(&doc, seq, &CompileOptions::new(48_000.0));
    assert_eq!(note_frames(&s), vec![(24_000, 0x90), (24_001, 0x80)]);
}

#[test]
fn one_tick_note_at_fast_tempo_and_low_rate_off_follows_its_on() {
    let mut b = Builder::new();
    let synth = b.dest("Synth", "builtin:synth", 0).unwrap();
    let (seq, lane) = b.sequence("S", "L", Some(synth)).unwrap();
    b.tempo(seq, &[(0, 999, false)]).unwrap();
    b.notes(seq, lane, &[(Q, 60, 0x8000, 1)]).unwrap();
    let doc = b.finish();
    let (s, _, _) = compile_all(&doc, seq, &CompileOptions::new(22_050.0));
    // One quarter at 999 BPM is 1324.3 frames; one tick is 0.086 frames.
    assert_eq!(note_frames(&s), vec![(1_324, 0x90), (1_325, 0x80)]);
}

#[test]
fn default_patch_plays_first_on_frame_zero() {
    let mut b = Builder::new();
    let d = b
        .dest_kind(
            "Strings",
            DestKind::MidiEndpoint {
                endpoint_ref: "builtin:synth".into(),
                group: 0,
                channel: 2,
            },
            Some(Patch {
                program: 48,
                bank: Some((121, 0)),
            }),
        )
        .unwrap();
    let unused = b
        .dest_kind(
            "Unused",
            DestKind::MidiEndpoint {
                endpoint_ref: "builtin:synth".into(),
                group: 0,
                channel: 3,
            },
            Some(Patch {
                program: 1,
                bank: None,
            }),
        )
        .unwrap();
    let (seq, lane) = b.sequence("S", "L", Some(d)).unwrap();
    assert_ne!(unused, d);
    b.notes(seq, lane, &[(0, 60, 0x8000, Q)]).unwrap();
    b.event(
        seq,
        lane,
        0,
        EventBody::Program {
            program: 49,
            bank: None,
        },
    )
    .unwrap();
    let doc = b.finish();
    let (s, _, _) = compile_all(&doc, seq, &CompileOptions::new(48_000.0));
    let first: Vec<Vec<u8>> = bytes(&s).into_iter().take(5).map(|(_, _, b)| b).collect();
    assert_eq!(
        first,
        vec![
            vec![0xB2, 0, 121],
            vec![0xB2, 32, 0],
            vec![0xC2, 48],
            vec![0xC2, 49],
            vec![0x92, 60, 64]
        ]
    );
}

#[test]
fn unroutable_events_are_reported_not_played() {
    let mut b = Builder::new();
    let plugin = b
        .dest_kind("Pad", DestKind::plugin("au:pad"), None)
        .unwrap();
    let missing = b.dest("Rack", "Missing Synth", 0).unwrap();
    let synth = b.dest("Synth", "builtin:synth", 0).unwrap();
    let (seq, pad_lane) = b.sequence("S", "Pad", Some(plugin)).unwrap();
    b.notes(seq, pad_lane, &[(0, 60, 0x8000, Q)]).unwrap();
    let rack_lane = b.lane(seq, "Rack", Some(missing)).unwrap();
    b.notes(seq, rack_lane, &[(0, 62, 0x8000, Q)]).unwrap();
    let none_lane = b.lane(seq, "Nowhere", None).unwrap();
    b.notes(seq, none_lane, &[(0, 64, 0x8000, Q)]).unwrap();
    let ok_lane = b.lane(seq, "Ok", Some(synth)).unwrap();
    b.notes(seq, ok_lane, &[(0, 67, 0x8000, Q)]).unwrap();
    let doc = b.finish();
    let (s, r, table) = compile_all(&doc, seq, &CompileOptions::new(48_000.0));
    // The plugin instrument is a target of its own (its sink comes from
    // the plugin factory, which this resolver stands in for).
    assert_eq!(r.events, 2);
    assert_eq!(s.entries.len(), 4);
    assert!(
        table
            .iter()
            .any(|(_, k)| matches!(k, TargetKind::PluginInstrument(_)))
    );
    assert_eq!(r.unroutable.len(), 2);
    let reasons: Vec<&str> = r.unroutable.iter().map(|u| u.reason.as_str()).collect();
    assert!(
        reasons.iter().any(|w| w.contains("Missing Synth")),
        "{reasons:?}"
    );
    assert!(reasons.contains(&"no destination"), "{reasons:?}");
    assert!(r.unroutable.iter().all(|u| u.path.len() == 1));
    assert_eq!(r.warnings.len(), 2, "{:?}", r.warnings);
}

#[test]
fn named_destination_routes_by_name_or_unique_id() {
    let mut b = Builder::new();
    let by_name = b.dest("A", "IAC Bus 1", 0).unwrap();
    let by_id = b.dest("B", "-12345", 1).unwrap();
    let (seq, l1) = b.sequence("S", "A", Some(by_name)).unwrap();
    b.notes(seq, l1, &[(0, 60, 0x8000, Q)]).unwrap();
    let l2 = b.lane(seq, "B", Some(by_id)).unwrap();
    b.notes(seq, l2, &[(0, 61, 0x8000, Q)]).unwrap();
    let doc = b.finish();
    let ports = vec![
        MidiPortInfo {
            name: "IAC Bus 1".into(),
            unique_id: Some(7),
        },
        MidiPortInfo {
            name: "Other".into(),
            unique_id: Some(-12345),
        },
    ];
    let opts = CompileOptions {
        sample_rate: 48_000.0,
        route: BuiltinRoute::Builtin,
        ports: ports.clone(),
        offline_media: Default::default(),
    };
    let (s, r, table) = compile_all(&doc, seq, &opts);
    assert!(r.unroutable.is_empty(), "{:?}", r.unroutable);
    assert_eq!(table.len(), 2);
    assert_eq!(
        table.get(TargetIdx(0)),
        Some(&TargetKind::MidiDestination(ports[0].clone()))
    );
    assert_eq!(
        table.get(TargetIdx(1)),
        Some(&TargetKind::MidiDestination(ports[1].clone()))
    );
    assert_eq!(s.entries.len(), 4);
}

#[test]
fn route_both_duplicates_builtin_events() {
    let (doc, seq) = one_bar();
    let opts = CompileOptions {
        sample_rate: 48_000.0,
        route: BuiltinRoute::Both,
        ports: vec![],
        offline_media: Default::default(),
    };
    let (s, _, table) = compile_all(&doc, seq, &opts);
    assert_eq!(
        table.get(TargetIdx(1)),
        Some(&TargetKind::MidiVirtualSource)
    );
    assert_eq!(s.entries.len(), 16);
    let virt = CompileOptions {
        route: BuiltinRoute::Virtual,
        ..opts
    };
    let (_, _, table) = compile_all(&doc, seq, &virt);
    assert_eq!(
        table.get(TargetIdx(0)),
        Some(&TargetKind::MidiVirtualSource)
    );
    assert_eq!(table.len(), 1);
}

#[test]
fn groups_fold_with_a_warning_and_sysex_goes_to_the_arena() {
    let mut b = Builder::new();
    let g2 = b
        .dest_kind(
            "G2",
            DestKind::MidiEndpoint {
                endpoint_ref: "builtin:synth".into(),
                group: 2,
                channel: 4,
            },
            None,
        )
        .unwrap();
    let (seq, lane) = b.sequence("S", "L", Some(g2)).unwrap();
    b.notes(seq, lane, &[(0, 60, 0x8000, Q)]).unwrap();
    b.event(
        seq,
        lane,
        Q,
        EventBody::SysEx {
            data: Arc::from(&[0x7E, 0x7F, 0x09, 0x01][..]),
            eight_bit: false,
        },
    )
    .unwrap();
    let doc = b.finish();
    let (s, r, _) = compile_all(&doc, seq, &CompileOptions::new(48_000.0));
    assert_eq!(r.group_folded, 2);
    assert!(r.warnings.iter().any(|w| w.contains("groups other than 0")));
    let sysex: Vec<Vec<u8>> = s
        .entries
        .iter()
        .filter(|e| matches!(e.msg, WireMsg::SysEx { .. }))
        .map(|e| s.bytes(&e.msg).to_vec())
        .collect();
    assert_eq!(sysex, vec![vec![0xF0, 0x7E, 0x7F, 0x09, 0x01, 0xF7]]);
    assert_eq!(
        s.entries[0].msg,
        WireMsg::Midi1(Midi1Msg::three(0x94, 60, 64))
    );
    assert_eq!(s.used_channels[0], 1 << 4);
}

#[test]
fn pickup_puts_frame_zero_at_bar_zero() {
    let (doc, seq) = one_bar();
    let doc = core_model::command::apply(
        &doc,
        &Command::Sequence(SequenceCmd::SetStartOffset {
            seq,
            offset: Tick(Q),
        }),
    )
    .unwrap()
    .doc;
    let (s, _, _) = compile_all(&doc, seq, &CompileOptions::new(48_000.0));
    // One quarter (24000 frames) of pickup comes before tick 0.
    assert_eq!(s.entries[0].frame, 24_000);
    assert_eq!(s.loop_range, Some((24_000, 120_000)));
    assert_eq!(s.tempo.tick_to_frame(Tick(0)), 24_000);
    assert_eq!(s.tempo.frame_to_tick(0), Tick(-Q));
}

#[test]
fn too_many_targets_is_an_error() {
    let mut b = Builder::new();
    let (seq, _) = b.sequence("S", "L", None).unwrap();
    let mut ports = Vec::new();
    for i in 0..=MAX_TARGETS {
        let name = format!("Port {i}");
        let d = b.dest(&name, &name, 0).unwrap();
        let l = b.lane(seq, &name, Some(d)).unwrap();
        b.notes(seq, l, &[(0, 60, 0x8000, Q)]).unwrap();
        ports.push(MidiPortInfo {
            name,
            unique_id: None,
        });
    }
    let doc = b.finish();
    let opts = CompileOptions {
        sample_rate: 48_000.0,
        route: BuiltinRoute::Builtin,
        ports,
        offline_media: Default::default(),
    };
    let mut table = TargetTable::new();
    let err = compile(&doc, seq, &opts, &mut |k: &TargetKind| {
        table.intern(k.clone()).map(Ok)
    })
    .unwrap_err();
    assert_eq!(err, EngineError::TooManyTargets);
}

#[test]
fn unknown_sequence_is_a_model_error() {
    let (doc, _) = one_bar();
    let mut table = TargetTable::new();
    let err = compile(
        &doc,
        SeqId(999),
        &CompileOptions::new(48_000.0),
        &mut |k: &TargetKind| table.intern(k.clone()).map(Ok),
    )
    .unwrap_err();
    assert!(matches!(err, EngineError::Model(_)));
}

#[test]
fn length_covers_whole_bars() {
    let mut b = Builder::new();
    let synth = b.dest("Synth", "builtin:synth", 0).unwrap();
    let (seq, lane) = b.sequence("S", "L", Some(synth)).unwrap();
    b.notes(seq, lane, &[(BAR + Q, 60, 0x8000, Q)]).unwrap();
    let doc = b.finish();
    let (s, _, _) = compile_all(&doc, seq, &CompileOptions::new(44_100.0));
    assert_eq!(s.length_frames, 2 * 88_200);
}
