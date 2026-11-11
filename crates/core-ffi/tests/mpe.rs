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
