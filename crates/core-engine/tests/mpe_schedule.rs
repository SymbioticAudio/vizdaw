//! MPE output in the schedule (plan 5g-2 Task 4; Rulings 7, 9, 10): notes
//! with per-note bend, timbre and pressure play on rotating member
//! channels with their start sent first; the configuration plays at frame
//! 0; steals are counted; a plain destination drops and reports. Played
//! through the offline backend and read from its log.

use core_engine::backend::AudioBackend;
use core_engine::engine::{Engine, EngineConfig};
use core_engine::fixtures::{Builder, Q};
use core_engine::offline::{OfflineBackend, OfflineSinks};
use core_model::command::{Command, EventCmd, RoutingCmd};
use core_model::event::{EventBody, EventSpec, PerNoteCtl, PerNoteKind};
use core_model::mpe::{MpeConfig, MpeZone};
use core_model::time::Tick;

const SR: f64 = 48_000.0;

/// `(frame, bytes)` of each logged message.
type Log = Vec<(u64, Vec<u8>)>;

fn note(tick: i64, pitch: u8, dur: i64, per_note: Vec<PerNoteCtl>) -> EventSpec {
    let mut body = EventBody::note(pitch, 0xC924, Tick(dur));
    if let EventBody::Note { per_note: p, .. } = &mut body {
        *p = per_note.into_iter().collect();
    }
    EventSpec::new(Tick(tick), body)
}

fn pn(kind: PerNoteKind, offset: i64, value: u32) -> PerNoteCtl {
    PerNoteCtl {
        kind,
        offset: Tick(offset),
        value,
    }
}

/// Plays `events` on "Keys" to "Synth" (MPE output `mpe`) and returns the
/// log's `(frame, bytes)` and the compile report's `(dropped_per_note, mpe_stolen)`.
fn play(events: Vec<EventSpec>, mpe: Option<MpeConfig>) -> (Log, (usize, usize)) {
    let mut b = Builder::new();
    let synth = b.dest("Synth", "builtin:synth", 0).unwrap();
    let (seq, keys) = b.sequence("Bar", "Keys", Some(synth)).unwrap();
    b.run(Command::Routing(RoutingCmd::SetMpe { dest: synth, mpe }))
        .unwrap();
    b.run(Command::Event(EventCmd::Insert {
        seq,
        lane: keys,
        events,
    }))
    .unwrap();
    let doc = b.finish();
    let sinks = OfflineSinks::new(100_000);
    let log = sinks.log();
    let (mut engine, renderer) = Engine::new(EngineConfig::new(SR), Box::new(sinks)).unwrap();
    let report = engine.load(&doc, seq).unwrap();
    let mut be = OfflineBackend::new(SR, 256);
    be.start(renderer).unwrap();
    engine.play().unwrap();
    be.run_frames(5 * 24_000);
    let got = log
        .drain()
        .into_iter()
        .map(|r| (r.frame, r.bytes().to_vec()))
        .collect();
    (got, (report.dropped_per_note, report.mpe_stolen))
}

fn without_rpn(v: &[(u64, Vec<u8>)]) -> Vec<(u64, Vec<u8>)> {
    v.iter()
        .filter(|(_, b)| !(b[0] & 0xF0 == 0xB0 && [101, 100, 6, 38].contains(&b[1])))
        .cloned()
        .collect()
}

#[test]
fn notes_play_on_member_channels_with_their_expression() {
    let events = vec![
        note(
            0,
            60,
            Q,
            vec![
                pn(PerNoteKind::PitchBend, 0, 0xA000_0000),
                pn(PerNoteKind::Registered(74), 0, 0x6000_0000),
                pn(PerNoteKind::PitchBend, Q / 2, 0x9000_0000),
            ],
        ),
        EventSpec::new(
            Tick(0),
            EventBody::PolyPressure {
                pitch: 60,
                value: 0x4000_0000,
            },
        ),
        EventSpec::new(
            Tick(Q / 4),
            EventBody::PolyPressure {
                pitch: 60,
                value: 0x6000_0000,
            },
        ),
        note(Q / 2, 64, Q, vec![]),
        EventSpec::new(
            Tick(Q),
            EventBody::Cc {
                index: 64,
                value: 0xFFFF_FFFF,
            },
        ),
    ];
    let (got, (dropped, stolen)) = play(events, Some(MpeConfig::default()));
    let cfg: Vec<&Vec<u8>> = got
        .iter()
        .filter(|(f, _)| *f == 0)
        .map(|(_, b)| b)
        .collect();
    assert!(
        cfg.contains(&&vec![0xB0, 100, 6]) && cfg.contains(&&vec![0xB0, 6, 15]),
        "the MCM at frame 0: {cfg:?}"
    );
    assert_eq!(
        without_rpn(&got),
        vec![
            (0, vec![0xE1, 0x00, 0x50]),
            (0, vec![0xB1, 74, 0x30]),
            (0, vec![0xD1, 0x20]),
            (0, vec![0x91, 60, 100]),
            (6000, vec![0xD1, 0x30]),
            (12_000, vec![0xE1, 0x00, 0x48]),
            (12_000, vec![0xE2, 0x00, 0x40]),
            (12_000, vec![0xB2, 74, 0x40]),
            (12_000, vec![0xD2, 0x00]),
            (12_000, vec![0x92, 64, 100]),
            (24_000, vec![0x81, 60, 0]),
            (24_000, vec![0xB0, 64, 127]),
            (36_000, vec![0x82, 64, 0]),
            // The sequence ends (one bar) with the pedal still down: the
            // renderer releases it on the manager channel.
            (96_000, vec![0xB0, 64, 0]),
        ]
    );
    assert_eq!((dropped, stolen), (0, 0));
}

#[test]
fn playback_rotates_and_steals_like_thru() {
    let two = MpeConfig {
        zone: MpeZone::Upper,
        members: 2,
        bend_range: 48,
    };
    let events = vec![
        note(0, 60, 2 * Q, vec![]),
        note(0, 62, 2 * Q, vec![]),
        note(Q, 64, Q, vec![]),
    ];
    let (got, (_, stolen)) = play(events, Some(two));
    let ons: Vec<(u64, Vec<u8>)> = without_rpn(&got)
        .into_iter()
        .filter(|(_, b)| b[0] & 0xF0 == 0x90 || b[0] & 0xF0 == 0x80)
        .collect();
    assert_eq!(
        ons,
        vec![
            (0, vec![0x9E, 60, 100]),
            (0, vec![0x9D, 62, 100]),
            (24_000, vec![0x8E, 60, 0]),
            (24_000, vec![0x9E, 64, 100]),
            (48_000, vec![0x8D, 62, 0]),
            (48_000, vec![0x8E, 64, 0]),
        ]
    );
    assert_eq!(stolen, 1);
}

#[test]
fn a_plain_destination_drops_per_note_bend_and_reports_it() {
    let events = vec![
        note(
            0,
            60,
            Q,
            vec![
                pn(PerNoteKind::PitchBend, 0, 0xA000_0000),
                pn(PerNoteKind::Registered(74), Q / 2, 1),
            ],
        ),
        EventSpec::new(
            Tick(Q / 4),
            EventBody::PolyPressure {
                pitch: 60,
                value: 0x6000_0000,
            },
        ),
    ];
    let (got, (dropped, _)) = play(events, None);
    assert_eq!(
        got,
        vec![
            (0, vec![0x90, 60, 100]),
            (6000, vec![0xA0, 60, 0x30]),
            (24_000, vec![0x80, 60, 0])
        ]
    );
    assert_eq!(dropped, 2);
}

// External destinations, as `midi_strip.rs` builds them.

use core_engine::schedule::{
    CompileOptions, CompileReport, Schedule, TargetKind, TargetTable, WireMsg, compile,
};
use core_model::document::{DestKind, Document, MidiMix, Patch};
use core_model::ids::{DestId, SeqId};

fn compile_all(doc: &Document, seq: SeqId) -> (Schedule, CompileReport) {
    let mut table = TargetTable::new();
    compile(
        doc,
        seq,
        &CompileOptions::new(48_000.0),
        &mut |k: &TargetKind| table.intern(k.clone()).map(Ok),
    )
    .unwrap()
}

/// `(frame, bytes)` of every MIDI 1.0 entry.
fn entries(s: &Schedule) -> Vec<(u64, Vec<u8>)> {
    s.entries
        .iter()
        .filter(|e| matches!(e.msg, WireMsg::Midi1(_)))
        .map(|e| (e.frame, s.bytes(&e.msg).to_vec()))
        .collect()
}

fn ext(b: &mut Builder, name: &str, channel: u8, patch: Option<Patch>) -> DestId {
    b.dest_kind(
        name,
        DestKind::MidiEndpoint {
            endpoint_ref: "virtual:out".into(),
            group: 0,
            channel,
        },
        patch,
    )
    .unwrap()
}

const UPPER3: MpeConfig = MpeConfig {
    zone: MpeZone::Upper,
    members: 3,
    bend_range: 48,
};

/// An external MPE destination (endpoint channel 3, Upper zone of 3) with
/// program 5 (bank 1/2), volume 90, pan 30 and `mute`, playing `events`.
fn ext_mpe(mute: bool, events: Vec<EventSpec>) -> (Schedule, CompileReport) {
    let mut b = Builder::new();
    let d = ext(
        &mut b,
        "Ext",
        2,
        Some(Patch {
            program: 5,
            bank: Some((1, 2)),
        }),
    );
    b.run(Command::Routing(RoutingCmd::SetMidiMix {
        dest: d,
        mix: MidiMix {
            volume: Some(90),
            pan: Some(30),
            mute,
        },
    }))
    .unwrap();
    b.run(Command::Routing(RoutingCmd::SetMpe {
        dest: d,
        mpe: Some(UPPER3),
    }))
    .unwrap();
    let (seq, lane) = b.sequence("S", "L", Some(d)).unwrap();
    b.run(Command::Event(EventCmd::Insert { seq, lane, events }))
        .unwrap();
    compile_all(&b.finish(), seq)
}

/// The 11 configuration messages of an Upper zone of 3 members.
fn upper3_configuration() -> Vec<Vec<u8>> {
    vec![
        vec![0xBF, 101, 0],
        vec![0xBF, 100, 6],
        vec![0xBF, 6, 3],
        vec![0xBF, 101, 127],
        vec![0xBF, 100, 127],
        vec![0xBE, 101, 0],
        vec![0xBE, 100, 0],
        vec![0xBE, 6, 48],
        vec![0xBE, 38, 0],
        vec![0xBE, 101, 127],
        vec![0xBE, 100, 127],
    ]
}

#[test]
fn an_mpe_destinations_patch_and_mix_go_on_its_manager_channel() {
    let (s, _) = ext_mpe(
        false,
        vec![note(
            0,
            60,
            Q,
            vec![pn(PerNoteKind::PitchBend, 0, 0xA000_0000)],
        )],
    );
    let got = entries(&s);
    let mut want: Vec<(u64, Vec<u8>)> =
        upper3_configuration().into_iter().map(|b| (0, b)).collect();
    want.extend([
        (0, vec![0xBF, 0, 1]),
        (0, vec![0xBF, 32, 2]),
        (0, vec![0xCF, 5]),
        (0, vec![0xBF, 7, 90]),
        (0, vec![0xBF, 10, 30]),
        (0, vec![0xEE, 0x00, 0x50]),
        (0, vec![0xBE, 74, 0x40]),
        (0, vec![0xDE, 0x00]),
        (0, vec![0x9E, 60, 100]),
        (24_000, vec![0x8E, 60, 0]),
    ]);
    assert_eq!(got, want);
    assert!(
        got.iter().all(|(_, b)| b[0] & 0x0F != 2),
        "nothing on the endpoint channel: {got:?}"
    );
}

#[test]
fn a_muted_mpe_destination_compiles_configuration_but_no_notes() {
    let bend = |t| {
        vec![
            pn(PerNoteKind::PitchBend, 0, 0xA000_0000),
            pn(PerNoteKind::PitchBend, t, 0x9000_0000),
        ]
    };
    let events = vec![
        note(0, 60, Q, bend(Q / 2)),
        note(Q, 62, Q, bend(Q / 4)),
        EventSpec::new(
            Tick(Q / 2),
            EventBody::Cc {
                index: 1,
                value: 0x6000_0000,
            },
        ),
        EventSpec::new(Tick(Q / 2), EventBody::PitchBend { value: 0xA000_0000 }),
    ];
    let (s, report) = ext_mpe(true, events);
    let got = entries(&s);
    let members = [0xE_u8, 0xD, 0xC];
    for (_, b) in &got {
        let (kind, ch) = (b[0] & 0xF0, b[0] & 0x0F);
        assert!(kind != 0x80 && kind != 0x90, "no notes: {got:?}");
        if members.contains(&ch) {
            let rpn = kind == 0xB0 && [101, 100, 6, 38].contains(&b[1]);
            assert!(rpn, "no member-channel expression: {got:?}");
        }
    }
    let frame0: Vec<Vec<u8>> = got
        .iter()
        .filter(|(f, _)| *f == 0)
        .map(|(_, b)| b.clone())
        .collect();
    let mut want = upper3_configuration();
    want.extend([
        vec![0xBF, 0, 1],
        vec![0xBF, 32, 2],
        vec![0xCF, 5],
        vec![0xBF, 7, 90],
        vec![0xBF, 10, 30],
    ]);
    assert_eq!(frame0, want);
    assert!(
        got.contains(&(12_000, vec![0xBF, 1, 0x30])),
        "the CC 1 plays: {got:?}"
    );
    assert!(
        got.contains(&(12_000, vec![0xEF, 0x00, 0x50])),
        "the channel bend plays: {got:?}"
    );
    assert_eq!(report.mpe_stolen, 0);
}

/// Plays a two-bar MPE part from `from` and returns the log.
fn play_from(from: Option<u64>) -> Vec<(u64, Vec<u8>)> {
    let mut b = Builder::new();
    let synth = b.dest("Synth", "builtin:synth", 0).unwrap();
    let (seq, keys) = b.sequence("Bar", "Keys", Some(synth)).unwrap();
    b.run(Command::Routing(RoutingCmd::SetMpe {
        dest: synth,
        mpe: Some(MpeConfig::default()),
    }))
    .unwrap();
    let events = vec![
        note(0, 60, Q, vec![]),
        note(4 * Q, 62, Q, vec![]),
        note(8 * Q, 64, Q, vec![]),
    ];
    b.run(Command::Event(EventCmd::Insert {
        seq,
        lane: keys,
        events,
    }))
    .unwrap();
    let doc = b.finish();
    let sinks = OfflineSinks::new(100_000);
    let log = sinks.log();
    let (mut engine, renderer) = Engine::new(EngineConfig::new(SR), Box::new(sinks)).unwrap();
    engine.load(&doc, seq).unwrap();
    let mut be = OfflineBackend::new(SR, 256);
    be.start(renderer).unwrap();
    if let Some(f) = from {
        engine.locate_frame(f).unwrap();
    }
    engine.play().unwrap();
    be.run_frames(3 * 24_000);
    log.drain()
        .into_iter()
        .map(|r| (r.frame, r.bytes().to_vec()))
        .collect()
}

fn count(got: &[(u64, Vec<u8>)], bytes: &[u8]) -> usize {
    got.iter().filter(|(_, b)| b == bytes).count()
}

#[test]
fn a_mid_song_play_sends_the_mpe_configuration() {
    let got = play_from(Some(2 * 24_000));
    let first_on = got
        .iter()
        .position(|(_, b)| b[0] & 0xF0 == 0x90)
        .expect("a note plays");
    let first_block: Vec<&Vec<u8>> = got
        .iter()
        .filter(|(f, _)| *f < 256)
        .map(|(_, b)| b)
        .collect();
    assert_eq!(
        first_block
            .iter()
            .filter(|b| ***b == [0xB0, 100, 6])
            .count(),
        1,
        "{got:?}"
    );
    assert_eq!(
        first_block.iter().filter(|b| ***b == [0xB0, 6, 15]).count(),
        1,
        "{got:?}"
    );
    let mcm = got.iter().position(|(_, b)| *b == [0xB0, 100, 6]).unwrap();
    assert!(
        mcm < first_on,
        "the configuration goes before the first note: {got:?}"
    );
    assert_eq!(count(&got, &[0xB0, 100, 6]), 1, "{got:?}");

    let from_start = play_from(None);
    assert_eq!(count(&from_start, &[0xB0, 100, 6]), 1, "{from_start:?}");
    assert_eq!(count(&from_start, &[0xB0, 6, 15]), 1, "{from_start:?}");
}

#[test]
fn a_plugin_mpe_destination_gets_its_configuration_and_patch_on_the_manager_channel() {
    let mut b = Builder::new();
    let patch = Patch {
        program: 5,
        bank: Some((1, 2)),
    };
    let d = b
        .dest_kind("Synth", DestKind::plugin("synth"), Some(patch))
        .unwrap();
    b.run(Command::Routing(RoutingCmd::SetMpe {
        dest: d,
        mpe: Some(UPPER3),
    }))
    .unwrap();
    let (seq, lane) = b.sequence("S", "L", Some(d)).unwrap();
    b.run(Command::Event(EventCmd::Insert {
        seq,
        lane,
        events: vec![note(0, 60, Q, vec![])],
    }))
    .unwrap();
    let (s, _) = compile_all(&b.finish(), seq);
    let got = entries(&s);
    let mut want: Vec<(u64, Vec<u8>)> =
        upper3_configuration().into_iter().map(|b| (0, b)).collect();
    want.extend([
        (0, vec![0xBF, 0, 1]),
        (0, vec![0xBF, 32, 2]),
        (0, vec![0xCF, 5]),
        (0, vec![0xEE, 0x00, 0x40]),
        (0, vec![0xBE, 74, 0x40]),
        (0, vec![0xDE, 0x00]),
        (0, vec![0x9E, 60, 100]),
        (24_000, vec![0x8E, 60, 0]),
    ]);
    assert_eq!(
        got, want,
        "configuration, then patch, no mix: a plugin is not external"
    );
}

/// An MPE part on the built-in synth (Lower zone of 15) with a note at
/// `Q` held to `5 * Q`, started; returns the engine, backend and log.
fn held_part() -> (
    Engine,
    OfflineBackend,
    core_engine::offline::EventLog,
    core_model::document::Document,
    SeqId,
) {
    let mut b = Builder::new();
    let synth = b.dest("Synth", "builtin:synth", 0).unwrap();
    let (seq, keys) = b.sequence("Bar", "Keys", Some(synth)).unwrap();
    b.run(Command::Routing(RoutingCmd::SetMpe {
        dest: synth,
        mpe: Some(MpeConfig::default()),
    }))
    .unwrap();
    let events = vec![note(Q, 60, 4 * Q, vec![]), note(8 * Q, 64, Q, vec![])];
    b.run(Command::Event(EventCmd::Insert {
        seq,
        lane: keys,
        events,
    }))
    .unwrap();
    let doc = b.finish();
    let sinks = OfflineSinks::new(100_000);
    let log = sinks.log();
    let (mut engine, renderer) = Engine::new(EngineConfig::new(SR), Box::new(sinks)).unwrap();
    engine.load(&doc, seq).unwrap();
    let mut be = OfflineBackend::new(SR, 256);
    be.start(renderer).unwrap();
    (engine, be, log, doc, seq)
}

/// Per member channel 1-15, note-ons minus note-offs: 0 when nothing hangs.
fn sounding(got: &[(u64, Vec<u8>)]) -> i64 {
    got.iter()
        .filter(|(_, b)| (1..16).contains(&(b[0] & 0x0F)))
        .map(|(_, b)| match b[0] & 0xF0 {
            0x90 => 1,
            0x80 => -1,
            _ => 0,
        })
        .sum()
}

#[test]
fn a_wrap_to_a_loop_start_past_0_resends_the_configuration_and_releases_member_notes() {
    let (mut engine, mut be, log, _, _) = held_part();
    engine.set_loop_range(Some((24_000, 72_000))).unwrap();
    engine.play().unwrap();
    be.run_frames(72_000 + 24_000);
    let got: Log = log
        .drain()
        .into_iter()
        .map(|r| (r.frame, r.bytes().to_vec()))
        .collect();
    assert_eq!(
        count(&got, &[0xB0, 100, 6]),
        2,
        "frame 0, then the wrap: {got:?}"
    );
    let wrap: Vec<&Vec<u8>> = got
        .iter()
        .filter(|(f, _)| *f == 72_000)
        .map(|(_, b)| b)
        .collect();
    let off = wrap
        .iter()
        .position(|b| **b == [0x81, 60, 0])
        .expect("the held note ends at the wrap");
    let mcm = wrap
        .iter()
        .position(|b| **b == [0xB0, 100, 6])
        .expect("the configuration at the wrap");
    let on = wrap
        .iter()
        .position(|b| b[0] == 0x91 || b[0] == 0x92)
        .expect("the note restarts");
    assert!(off < mcm && mcm < on, "{wrap:?}");
    engine.stop().unwrap();
    be.run_frames(1024);
    let got: Log = got
        .into_iter()
        .chain(
            log.drain()
                .into_iter()
                .map(|r| (r.frame, r.bytes().to_vec())),
        )
        .collect();
    assert_eq!(sounding(&got), 0, "nothing hangs: {got:?}");
}

#[test]
fn a_mid_play_swap_resends_the_configuration_and_releases_member_notes() {
    let (mut engine, mut be, log, doc, seq) = held_part();
    engine.play().unwrap();
    be.run_frames(48_000);
    engine.load(&doc, seq).unwrap();
    be.run_frames(24_000);
    let got: Log = log
        .drain()
        .into_iter()
        .map(|r| (r.frame, r.bytes().to_vec()))
        .collect();
    assert_eq!(
        count(&got, &[0xB0, 100, 6]),
        2,
        "frame 0, then the swap: {got:?}"
    );
    let mcm2 = got.iter().rposition(|(_, b)| *b == [0xB0, 100, 6]).unwrap();
    let off = got
        .iter()
        .position(|(_, b)| *b == [0x81, 60, 0])
        .expect("the held note ends at the swap");
    assert!(off < mcm2, "{got:?}");
    assert_eq!(sounding(&got), 0, "nothing hangs: {got:?}");
}
