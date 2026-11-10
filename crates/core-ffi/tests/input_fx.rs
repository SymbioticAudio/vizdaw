//! Input effects through a session (plan 5g-1 Task 6; spec §6.8, §12.1):
//! the chain is one undoable lane edit shown in the lane rows; the
//! model refuses hostile chains; the channel's JSON round-trips and
//! hostile JSON is refused; dropped notes are reported.

#![recursion_limit = "256"]

mod common;

use core_engine::offline::FakeInput;
use std::sync::{Mutex, MutexGuard};
use std::time::{Duration, Instant};
use vizdaw_ffi::host::{force_offline, reset_host, set_fake_input};
use vizdaw_ffi::input_fx::{
    ArpModeRow, FeelRow, InputFxKindRow, InputFxRow, input_fx_from_json, input_fx_json,
};
use vizdaw_ffi::{EditCommand, LaneEdit, LaneInputRow, LaneRow, Session, VizError};

static SERIAL: Mutex<()> = Mutex::new(());

fn serial() -> MutexGuard<'static, ()> {
    let g = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    force_offline();
    set_fake_input(Some(FakeInput::Signals));
    reset_host();
    g
}

fn song(s: &Session) -> u64 {
    s.snapshot()
        .unwrap()
        .sequences
        .iter()
        .find(|r| r.name == "Song")
        .unwrap()
        .id
}

fn lane(s: &Session, seq: u64, name: &str) -> LaneRow {
    s.lanes(seq)
        .unwrap()
        .into_iter()
        .find(|l| l.name == name)
        .unwrap()
}

fn set(s: &Session, seq: u64, lane: u64, chain: Vec<InputFxRow>) -> Result<(), VizError> {
    s.edit(
        EditCommand::Lane {
            edit: LaneEdit::SetInputFx { seq, lane, chain },
        },
        None,
    )
    .map(|_| ())
}

fn row(kind: InputFxKindRow) -> InputFxRow {
    InputFxRow {
        kind,
        bypass: false,
    }
}

fn arp() -> InputFxKindRow {
    InputFxKindRow::Arpeggiator {
        mode: ArpModeRow::Up,
        division: 16,
        feel: FeelRow::Straight,
        octaves: 1,
        gate: 50,
        velocity: None,
        latch: false,
    }
}

#[test]
fn input_effects_are_one_undoable_lane_edit() {
    let _g = serial();
    let s = Session::new_demo().unwrap();
    let seq = song(&s);
    let chords = lane(&s, seq, "Chords").id;
    assert!(lane(&s, seq, "Chords").input_fx.is_empty());
    let chain = vec![
        row(InputFxKindRow::Transpose { semitones: 12 }),
        InputFxRow {
            kind: arp(),
            bypass: true,
        },
    ];
    set(&s, seq, chords, chain.clone()).unwrap();
    assert_eq!(lane(&s, seq, "Chords").input_fx, chain);
    assert_eq!(
        s.snapshot().unwrap().undo_label.as_deref(),
        Some("Set Input Effects")
    );
    s.undo().unwrap();
    assert!(lane(&s, seq, "Chords").input_fx.is_empty());
    s.close().unwrap();
}

#[test]
fn hostile_chains_are_refused_by_the_model() {
    let _g = serial();
    let s = Session::new_demo().unwrap();
    let seq = song(&s);
    let chords = lane(&s, seq, "Chords").id;
    let with_arp = |f: &dyn Fn(&mut InputFxKindRow)| {
        let mut k = arp();
        f(&mut k);
        vec![row(k)]
    };
    for bad in [
        vec![row(InputFxKindRow::Transpose { semitones: 1 }); 9],
        with_arp(&|k| {
            if let InputFxKindRow::Arpeggiator { octaves, .. } = k {
                *octaves = 5
            }
        }),
        with_arp(&|k| {
            if let InputFxKindRow::Arpeggiator { gate, .. } = k {
                *gate = 4
            }
        }),
        with_arp(&|k| {
            if let InputFxKindRow::Arpeggiator { division, .. } = k {
                *division = 3
            }
        }),
        vec![row(InputFxKindRow::Transpose { semitones: 49 })],
        vec![row(InputFxKindRow::Chord { intervals: vec![0] })],
        vec![row(InputFxKindRow::Chord {
            intervals: vec![1, 2, 3, 4, 5, 6, 7],
        })],
        vec![row(InputFxKindRow::Velocity {
            scale: 100,
            offset: 0,
            min: 90,
            max: 80,
            fixed: None,
        })],
        vec![row(InputFxKindRow::InputQuantize {
            grid: 1000,
            strength: 100,
        })],
        vec![
            row(InputFxKindRow::InputQuantize {
                grid: 3840,
                strength: 100,
            }),
            row(InputFxKindRow::InputQuantize {
                grid: 1920,
                strength: 100,
            }),
        ],
    ] {
        let r = set(&s, seq, chords, bad.clone());
        assert!(matches!(r, Err(VizError::Model { .. })), "{bad:?}: {r:?}");
    }
    assert!(lane(&s, seq, "Chords").input_fx.is_empty());
    s.close().unwrap();
}

#[test]
fn chain_json_round_trips() {
    let chain = vec![
        row(arp()),
        InputFxRow {
            kind: InputFxKindRow::Velocity {
                scale: 120,
                offset: -3,
                min: 10,
                max: 120,
                fixed: None,
            },
            bypass: true,
        },
        row(InputFxKindRow::Chord {
            intervals: vec![4, 7],
        }),
        row(InputFxKindRow::InputQuantize {
            grid: 3840,
            strength: 75,
        }),
    ];
    let text = input_fx_json(&chain).to_string();
    assert_eq!(input_fx_from_json(text).unwrap(), chain);
    assert_eq!(
        input_fx_json(&[row(InputFxKindRow::Transpose { semitones: -5 })]),
        serde_json::json!([{"kind": {"transpose": {"semitones": -5}}}])
    );
}

#[test]
fn hostile_chain_json_is_refused() {
    for bad in [
        String::new(),
        "{".into(),
        "{}".into(),
        "[1]".into(),
        r#"[{"kind": {"flanger": {}}}]"#.into(),
        r#"[{"kind": {"transpose": {"semitones": 300}}}]"#.into(),
        r#"[{"kind": {"transpose": {"semitones": 1}}, "extra": 1}]"#.into(),
        format!(
            "[{}]",
            vec![r#"{"kind": {"transpose": {"semitones": 1}}}"#; 5000].join(",")
        ),
    ] {
        let r = input_fx_from_json(bad.clone());
        assert!(
            matches!(r, Err(VizError::Model { .. })),
            "{}: {r:?}",
            &bad[..bad.len().min(60)]
        );
    }
}

#[test]
fn midi_fx_dropped_is_reported() {
    let _g = serial();
    let s = Session::new_demo().unwrap();
    let seq = song(&s);
    let chords = lane(&s, seq, "Chords").id;
    s.edit(
        EditCommand::Lane {
            edit: LaneEdit::SetInput {
                seq,
                lane: chords,
                input: Some(LaneInputRow::Midi {
                    port: Some("Keys".into()),
                    channel: None,
                }),
            },
        },
        None,
    )
    .unwrap();
    set(&s, seq, chords, vec![row(arp())]).unwrap();
    s.edit(
        EditCommand::Lane {
            edit: LaneEdit::SetFlags {
                seq,
                lane: chords,
                mute: None,
                solo: None,
                arm: Some(true),
            },
        },
        None,
    )
    .unwrap();
    s.play(seq).unwrap();
    let until = Instant::now() + common::wait_limit();
    while !s.transport().unwrap().playing {
        assert!(Instant::now() < until, "never played");
        std::thread::sleep(Duration::from_millis(2));
    }
    s.stop().unwrap();
    let keys: Vec<u8> = (30..63u8).flat_map(|p| [0x90, p, 100]).collect();
    assert!(s.inject_midi("Keys".into(), keys).unwrap());
    while s.input_info().unwrap().midi_fx_dropped < 1 {
        assert!(Instant::now() < until, "the drop was never counted");
        std::thread::sleep(Duration::from_millis(5));
    }
    assert_eq!(s.input_info().unwrap().midi_fx_dropped, 1);
    s.close().unwrap();
}

/// Spec §6.8 "no chain for lanes that are not listening" (plan 5g-1
/// Ruling 13, Task 4 review): a lane with a chain gets a route, and so its
/// chain, only while it is armed or monitoring its input.
#[test]
fn only_a_listening_lane_gets_its_chain() {
    use core_engine::fixtures::Builder;
    use core_model::command::{Command, LaneCmd};
    use core_model::input_fx::{InputFx, InputFxKind};
    use core_model::lane::{LaneInput, MonitorMode};
    use vizdaw_ffi::host::input_spec;
    let mut b = Builder::new();
    let (seq, keys) = b.sequence("S", "Keys", None).unwrap();
    let chain = vec![InputFx::new(InputFxKind::Transpose { semitones: 12 })];
    for cmd in [
        LaneCmd::SetInput {
            seq,
            lane: keys,
            input: Some(LaneInput::Midi {
                port: Some("Keys".into()),
                channel: None,
            }),
        },
        LaneCmd::SetInputFx {
            seq,
            lane: keys,
            chain: chain.clone(),
        },
    ] {
        b.run(Command::Lane(cmd)).unwrap();
    }
    let doc = b.finish();
    assert!(
        input_spec(&doc, seq).midi.iter().all(|r| r.lane != keys),
        "neither armed nor monitoring"
    );
    let mut b = Builder::from_doc(doc.clone());
    b.run(Command::Lane(LaneCmd::SetFlags {
        seq,
        lane: keys,
        mute: None,
        solo: None,
        arm: Some(true),
    }))
    .unwrap();
    let armed = input_spec(&b.finish(), seq);
    assert_eq!(
        armed
            .midi
            .iter()
            .find(|r| r.lane == keys)
            .map(|r| r.fx.clone()),
        Some(chain.clone())
    );
    let mut b = Builder::from_doc(doc);
    b.run(Command::Lane(LaneCmd::SetMonitor {
        seq,
        lane: keys,
        monitor: MonitorMode::Input,
    }))
    .unwrap();
    let monitored = input_spec(&b.finish(), seq);
    assert_eq!(
        monitored
            .midi
            .iter()
            .find(|r| r.lane == keys)
            .map(|r| r.fx.clone()),
        Some(chain)
    );
}
