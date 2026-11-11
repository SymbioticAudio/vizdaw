//! MPE through a session (plan 5g-2 Task 6; Rulings 3, 4, 7, 12): the
//! track and destination settings are undoable edits shown in the rows; a
//! controller's MCM is written back as one step, after a recording;
//! steals and drops are reported. Offline engine only.

#![recursion_limit = "256"]

mod common;

use core_engine::offline::FakeInput;
use std::sync::{Mutex, MutexGuard};
use std::time::{Duration, Instant};
use vizdaw_ffi::host::{force_offline, reset_host, set_fake_input};
use vizdaw_ffi::mpe::{MpeRow, MpeZoneRow};
use vizdaw_ffi::{EditCommand, LaneEdit, LaneInputRow, Session, VizError};

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

fn chords(s: &Session, seq: u64) -> vizdaw_ffi::LaneRow {
    s.lanes(seq)
        .unwrap()
        .into_iter()
        .find(|l| l.name == "Chords")
        .unwrap()
}

fn lane_edit(s: &Session, e: LaneEdit) -> Result<(), VizError> {
    s.edit(EditCommand::Lane { edit: e }, None).map(|_| ())
}

const FULL: MpeRow = MpeRow {
    zone: MpeZoneRow::Lower,
    members: 15,
    bend_range: 48,
};

/// Chords listens to "MPE Keys" as MPE, armed; the session holds the engine.
fn mpe_lane(s: &Session) -> (u64, u64) {
    let seq = song(s);
    let lane = chords(s, seq).id;
    lane_edit(
        s,
        LaneEdit::SetInput {
            seq,
            lane,
            input: Some(LaneInputRow::Midi {
                port: Some("MPE Keys".into()),
                channel: None,
            }),
        },
    )
    .unwrap();
    lane_edit(
        s,
        LaneEdit::SetMpe {
            seq,
            lane,
            mpe: Some(FULL),
        },
    )
    .unwrap();
    lane_edit(
        s,
        LaneEdit::SetFlags {
            seq,
            lane,
            mute: None,
            solo: None,
            arm: Some(true),
        },
    )
    .unwrap();
    s.play(seq).unwrap();
    let until = Instant::now() + common::wait_limit();
    while !s.transport().unwrap().playing {
        assert!(Instant::now() < until, "never played");
        std::thread::sleep(Duration::from_millis(2));
    }
    s.stop().unwrap();
    (seq, lane)
}

#[test]
fn lane_and_destination_mpe_are_undoable_edits() {
    let _g = serial();
    let s = Session::new_demo().unwrap();
    let seq = song(&s);
    let lane = chords(&s, seq).id;
    lane_edit(
        &s,
        LaneEdit::SetMpe {
            seq,
            lane,
            mpe: Some(FULL),
        },
    )
    .unwrap();
    assert_eq!(chords(&s, seq).mpe, Some(FULL));
    assert_eq!(
        s.snapshot().unwrap().undo_label.as_deref(),
        Some("Set Track MPE")
    );
    let dest = s.destinations().unwrap()[0].id;
    s.set_destination_mpe(
        dest,
        Some(MpeRow {
            zone: MpeZoneRow::Upper,
            members: 4,
            bend_range: 24,
        }),
    )
    .unwrap();
    assert_eq!(
        s.destinations().unwrap()[0].mpe,
        Some(MpeRow {
            zone: MpeZoneRow::Upper,
            members: 4,
            bend_range: 24
        })
    );
    assert_eq!(
        s.snapshot().unwrap().undo_label.as_deref(),
        Some("Set Destination MPE")
    );
    s.undo().unwrap();
    s.undo().unwrap();
    assert_eq!(chords(&s, seq).mpe, None);
    assert_eq!(s.destinations().unwrap()[0].mpe, None);
    s.close().unwrap();
}

#[test]
fn hostile_mpe_calls_are_refused() {
    let _g = serial();
    let s = Session::new_demo().unwrap();
    let seq = song(&s);
    let lane = chords(&s, seq).id;
    let dest = s.destinations().unwrap()[0].id;
    for bad in [
        MpeRow {
            zone: MpeZoneRow::Lower,
            members: 0,
            bend_range: 48,
        },
        MpeRow {
            zone: MpeZoneRow::Lower,
            members: 16,
            bend_range: 48,
        },
        MpeRow {
            zone: MpeZoneRow::Upper,
            members: 3,
            bend_range: 0,
        },
        MpeRow {
            zone: MpeZoneRow::Upper,
            members: 3,
            bend_range: 200,
        },
    ] {
        assert!(matches!(
            lane_edit(
                &s,
                LaneEdit::SetMpe {
                    seq,
                    lane,
                    mpe: Some(bad)
                }
            ),
            Err(VizError::Model { .. })
        ));
        assert!(matches!(
            s.set_destination_mpe(dest, Some(bad)),
            Err(VizError::Model { .. })
        ));
    }
    assert!(matches!(
        s.set_destination_mpe(987_654, None),
        Err(VizError::Model { .. })
    ));
    assert_eq!(chords(&s, seq).mpe, None);
    s.close().unwrap();
}

#[test]
fn an_mcm_writes_the_lanes_setting_back() {
    let _g = serial();
    let s = Session::new_demo().unwrap();
    let (seq, _) = mpe_lane(&s);
    assert!(
        s.inject_midi(
            "MPE Keys".into(),
            vec![0xB0, 101, 0, 0xB0, 100, 6, 0xB0, 6, 7]
        )
        .unwrap()
    );
    let until = Instant::now() + common::wait_limit();
    while chords(&s, seq).mpe.map(|m| m.members) != Some(7) {
        assert!(Instant::now() < until, "the MCM was never written back");
        s.idle().unwrap();
        std::thread::sleep(Duration::from_millis(5));
    }
    assert_eq!(
        s.snapshot().unwrap().undo_label.as_deref(),
        Some("Set Track MPE")
    );
    s.undo().unwrap();
    assert_eq!(chords(&s, seq).mpe, Some(FULL));
    s.close().unwrap();
}

#[test]
fn an_mcm_during_a_recording_waits_for_the_pass() {
    let _g = serial();
    let s = Session::new_demo().unwrap();
    let (seq, _) = mpe_lane(&s);
    s.record(
        seq,
        vizdaw_ffi::record::RecordSpecRow {
            replace: true,
            punch: None,
            looped: false,
            count_in_bars: 0,
            pre_roll_bars: 0,
            passes_as_takes: true,
        },
    )
    .unwrap();
    let until = Instant::now() + common::wait_limit();
    while !s.record_state().unwrap().recording {
        assert!(Instant::now() < until, "never recorded");
        std::thread::sleep(Duration::from_millis(2));
    }
    assert!(
        s.inject_midi(
            "MPE Keys".into(),
            vec![0xB0, 101, 0, 0xB0, 100, 6, 0xB0, 6, 5]
        )
        .unwrap()
    );
    for _ in 0..20 {
        s.idle().unwrap();
        std::thread::sleep(Duration::from_millis(5));
    }
    assert_eq!(chords(&s, seq).mpe, Some(FULL), "not while recording");
    s.stop().unwrap();
    while chords(&s, seq).mpe.map(|m| m.members) != Some(5) {
        assert!(
            Instant::now() < until,
            "the MCM never landed after the pass"
        );
        s.idle().unwrap();
        std::thread::sleep(Duration::from_millis(5));
    }
    s.close().unwrap();
}

#[test]
fn mpe_stolen_is_reported() {
    let _g = serial();
    let s = Session::new_demo().unwrap();
    let (_, _) = mpe_lane(&s);
    // The demo's destination is plain: bend drops; then MPE output with two members: a steal.
    assert!(
        s.inject_midi(
            "MPE Keys".into(),
            vec![0x91, 60, 100, 0xE1, 0, 0x50, 0x81, 60, 0]
        )
        .unwrap()
    );
    let until = Instant::now() + common::wait_limit();
    while s.input_info().unwrap().per_note_dropped < 1 {
        assert!(Instant::now() < until, "the drop was never counted");
        std::thread::sleep(Duration::from_millis(5));
    }
    let dest = s
        .destinations()
        .unwrap()
        .iter()
        .find(|d| Some(&d.name) == chords(&s, song(&s)).destination.as_ref())
        .unwrap()
        .id;
    s.set_destination_mpe(
        dest,
        Some(MpeRow {
            zone: MpeZoneRow::Lower,
            members: 2,
            bend_range: 48,
        }),
    )
    .unwrap();
    assert!(
        s.inject_midi(
            "MPE Keys".into(),
            vec![0x91, 60, 100, 0x92, 62, 100, 0x93, 64, 100]
        )
        .unwrap()
    );
    while s.input_info().unwrap().mpe_stolen < 1 {
        assert!(Instant::now() < until, "the steal was never counted");
        std::thread::sleep(Duration::from_millis(5));
    }
    s.close().unwrap();
}

#[test]
fn a_zone_off_mcm_turns_the_lanes_mpe_off_in_one_step() {
    let _g = serial();
    let s = Session::new_demo().unwrap();
    let (seq, _) = mpe_lane(&s);
    let v0 = s.snapshot().unwrap().version;
    // Members 0: the controller turned its zone off (ruling T2-b).
    assert!(
        s.inject_midi(
            "MPE Keys".into(),
            vec![0xB0, 101, 0, 0xB0, 100, 6, 0xB0, 6, 0]
        )
        .unwrap()
    );
    let until = Instant::now() + common::wait_limit();
    while chords(&s, seq).mpe.is_some() {
        assert!(
            Instant::now() < until,
            "the zone-off was never written back"
        );
        s.idle().unwrap();
        std::thread::sleep(Duration::from_millis(5));
    }
    let snap = s.snapshot().unwrap();
    assert_eq!(snap.undo_label.as_deref(), Some("Set Track MPE"));
    assert_eq!(snap.version, v0 + 1, "one step");
    s.undo().unwrap();
    assert_eq!(chords(&s, seq).mpe, Some(FULL));
    s.close().unwrap();
}

#[test]
fn a_hand_set_mpe_during_a_recording_beats_the_waiting_mcm() {
    let _g = serial();
    let s = Session::new_demo().unwrap();
    let (seq, lane) = mpe_lane(&s);
    s.record(
        seq,
        vizdaw_ffi::record::RecordSpecRow {
            replace: true,
            punch: None,
            looped: false,
            count_in_bars: 0,
            pre_roll_bars: 0,
            passes_as_takes: true,
        },
    )
    .unwrap();
    let until = Instant::now() + common::wait_limit();
    while !s.record_state().unwrap().recording {
        assert!(Instant::now() < until, "never recorded");
        std::thread::sleep(Duration::from_millis(2));
    }
    assert!(
        s.inject_midi(
            "MPE Keys".into(),
            vec![0xB0, 101, 0, 0xB0, 100, 6, 0xB0, 6, 5]
        )
        .unwrap()
    );
    for _ in 0..20 {
        s.idle().unwrap();
        std::thread::sleep(Duration::from_millis(5));
    }
    let mine = MpeRow {
        zone: MpeZoneRow::Upper,
        members: 3,
        bend_range: 24,
    };
    lane_edit(
        &s,
        LaneEdit::SetMpe {
            seq,
            lane,
            mpe: Some(mine),
        },
    )
    .unwrap();
    s.stop().unwrap();
    while s.record_state().unwrap().recording
        || s.snapshot().unwrap().undo_label.as_deref() == Some("Set Track MPE")
    {
        assert!(Instant::now() < until, "the pass never landed");
        s.idle().unwrap();
        std::thread::sleep(Duration::from_millis(5));
    }
    for _ in 0..20 {
        s.idle().unwrap();
        std::thread::sleep(Duration::from_millis(5));
    }
    assert_eq!(chords(&s, seq).mpe, Some(mine), "the user's choice stays");
    assert_ne!(
        s.snapshot().unwrap().undo_label.as_deref(),
        Some("Set Track MPE"),
        "no write-back after the pass"
    );
    s.close().unwrap();
}

/// An external destination on channel 3 with a set mix, played by a lane
/// of the shown sequence (as `midi_strip.rs`).
fn mix_project() -> (std::sync::Arc<Session>, u64) {
    use core_engine::fixtures::{Builder, Q};
    let mut b = Builder::new();
    let ext = b.dest("Ext", "virtual:out", 2).unwrap();
    let (seq, lane) = b.sequence("S", "L", Some(ext)).unwrap();
    b.notes(seq, lane, &[(0, 60, 100 << 9, 4 * Q)]).unwrap();
    let s = Session::from_document(b.finish()).unwrap();
    s.set_midi_mix(
        ext.0,
        vizdaw_ffi::MidiMixRow {
            volume: Some(90),
            pan: Some(20),
            mute: false,
        },
    )
    .unwrap();
    (s, ext.0)
}

/// The RPN and mix controllers sent since the last call (the reload's
/// resets left out).
fn config_ccs() -> Vec<Vec<u8>> {
    std::thread::sleep(Duration::from_millis(150));
    vizdaw_ffi::host::drain_offline_log()
        .into_iter()
        .map(|(_, b)| b)
        .filter(|b| {
            b.first().is_some_and(|s| s & 0xF0 == 0xB0)
                && matches!(b.get(1), Some(101 | 100 | 6 | 38 | 7 | 10))
        })
        .collect()
}

fn zone_on(mgr: u8, mem: u8, members: u8, bend: u8) -> Vec<Vec<u8>> {
    let (m, n) = (0xB0 | mgr, 0xB0 | mem);
    vec![
        vec![m, 101, 0],
        vec![m, 100, 6],
        vec![m, 6, members],
        vec![m, 101, 127],
        vec![m, 100, 127],
        vec![n, 101, 0],
        vec![n, 100, 0],
        vec![n, 6, bend],
        vec![n, 38, 0],
        vec![n, 101, 127],
        vec![n, 100, 127],
    ]
}

fn zone_off(mgr: u8) -> Vec<Vec<u8>> {
    let m = 0xB0 | mgr;
    vec![
        vec![m, 101, 0],
        vec![m, 100, 6],
        vec![m, 6, 0],
        vec![m, 101, 127],
        vec![m, 100, 127],
    ]
}

#[test]
fn a_destination_mpe_change_configures_the_device_now() {
    let _g = serial();
    let (s, ext) = mix_project();
    assert_eq!(
        config_ccs(),
        vec![vec![0xB2, 7, 90], vec![0xB2, 10, 20]],
        "the mix, before MPE"
    );
    // On: the configuration, then the mix on the manager channel.
    s.set_destination_mpe(
        ext,
        Some(MpeRow {
            zone: MpeZoneRow::Lower,
            members: 4,
            bend_range: 24,
        }),
    )
    .unwrap();
    let mut on = zone_on(0, 1, 4, 24);
    on.extend([vec![0xB0, 7, 90], vec![0xB0, 10, 20]]);
    assert_eq!(config_ccs(), on);
    // The other zone: the old one off, the new one on, the mix there.
    s.set_destination_mpe(
        ext,
        Some(MpeRow {
            zone: MpeZoneRow::Upper,
            members: 2,
            bend_range: 48,
        }),
    )
    .unwrap();
    let mut moved = zone_off(0);
    moved.extend(zone_on(15, 14, 2, 48));
    moved.extend([vec![0xBF, 7, 90], vec![0xBF, 10, 20]]);
    assert_eq!(config_ccs(), moved);
    // Off: the zone-off MCM on the old manager channel, nothing else.
    s.set_destination_mpe(ext, None).unwrap();
    assert_eq!(config_ccs(), zone_off(15));
    // Undo sends what it restores; redo turns it off again.
    s.undo().unwrap();
    let mut back = zone_on(15, 14, 2, 48);
    back.extend([vec![0xBF, 7, 90], vec![0xBF, 10, 20]]);
    assert_eq!(config_ccs(), back);
    s.redo().unwrap();
    assert_eq!(config_ccs(), zone_off(15));
    reset_host();
}
