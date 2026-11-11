//! A MIDI strip's mix through the session (plan 5g-1b Task 3): rows, set,
//! preview and note activity. Offline only; the engine host is
//! process-wide, so one test at a time.

use core_engine::fixtures::{Builder, Q};
use core_model::ids::DestId;
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread::sleep;
use std::time::{Duration, Instant};
use vizdaw_ffi::host::{drain_offline_log, force_offline, holder, reset_host};
use vizdaw_ffi::{MidiLevelRow, MidiMixRow, Session};

static SERIAL: Mutex<()> = Mutex::new(());

fn serial() -> MutexGuard<'static, ()> {
    let g = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    force_offline();
    reset_host();
    g
}

struct P {
    s: Arc<Session>,
    seq: u64,
    ext: u64,
    synth: u64,
}

/// An external destination on channel 3 (index 2) playing a long note,
/// velocity 100, and the built-in synth.
fn project() -> P {
    let mut b = Builder::new();
    let ext: DestId = b.dest("Ext", "virtual:out", 2).unwrap();
    let synth = b.dest("Synth", "builtin:synth", 0).unwrap();
    let (seq, lane) = b.sequence("S", "L", Some(ext)).unwrap();
    b.notes(seq, lane, &[(0, 60, 100 << 9, 4 * Q)]).unwrap();
    P {
        s: Session::from_document(b.finish()).unwrap(),
        seq: seq.0,
        ext: ext.0,
        synth: synth.0,
    }
}

fn row(s: &Session, id: u64) -> vizdaw_ffi::DestRow {
    s.destinations()
        .unwrap()
        .into_iter()
        .find(|d| d.id == id)
        .unwrap()
}

#[test]
fn set_midi_mix_round_trips_and_undoes() {
    let _g = serial();
    let p = project();
    assert_eq!(row(&p.s, p.ext).midi_mix, MidiMixRow::default());
    let mix = MidiMixRow {
        volume: Some(90),
        pan: Some(20),
        mute: true,
    };
    p.s.set_midi_mix(p.ext, mix.clone()).unwrap();
    let r = row(&p.s, p.ext);
    assert_eq!(r.midi_mix, mix);
    assert_eq!(r.channel, Some(2));
    let snap = p.s.undo().unwrap();
    assert_eq!(row(&p.s, p.ext).midi_mix, MidiMixRow::default());
    assert!(snap.redo_label.is_some());
}

#[test]
fn set_midi_mix_refuses_bad_destinations() {
    let _g = serial();
    let p = project();
    let v0 = p.s.snapshot().unwrap().version;
    let ok = MidiMixRow {
        volume: Some(50),
        pan: None,
        mute: false,
    };
    for (dest, mix) in [
        (p.synth, ok.clone()),
        (9_999, ok.clone()),
        (
            p.ext,
            MidiMixRow {
                volume: Some(128),
                ..ok.clone()
            },
        ),
        (
            p.ext,
            MidiMixRow {
                pan: Some(200),
                ..ok.clone()
            },
        ),
    ] {
        assert!(p.s.set_midi_mix(dest, mix).is_err(), "dest {dest}");
    }
    assert_eq!(p.s.snapshot().unwrap().version, v0, "nothing changed");
    assert_eq!(row(&p.s, p.ext).midi_mix, MidiMixRow::default());
    // Previews that cannot be valid send nothing either.
    assert!(p.s.preview_midi_mix(p.synth, Some(1), None).is_err());
    assert!(p.s.preview_midi_mix(p.ext, Some(128), None).is_err());
    assert!(p.s.preview_midi_mix(9_999, Some(1), None).is_err());
}

#[test]
fn preview_sends_without_an_edit() {
    let _g = serial();
    let p = project();
    // A fresh session claims the idle engine for its sequence first.
    assert!(
        p.s.preview_midi_mix(p.ext, Some(64), None).unwrap(),
        "the idle engine is claimed"
    );
    p.s.play(p.seq).unwrap();
    sleep(Duration::from_millis(50));
    let v0 = p.s.snapshot().unwrap().version;
    drain_offline_log();
    assert!(p.s.preview_midi_mix(p.ext, Some(64), Some(10)).unwrap());
    assert!(
        !p.s.preview_midi_mix(p.ext, None, None).unwrap(),
        "nothing to send"
    );
    sleep(Duration::from_millis(100));
    assert_eq!(p.s.snapshot().unwrap().version, v0, "no edit");
    // A committed set sends CC 7 then CC 10 after the commit.
    p.s.set_midi_mix(
        p.ext,
        MidiMixRow {
            volume: Some(90),
            pan: Some(20),
            mute: false,
        },
    )
    .unwrap();
    sleep(Duration::from_millis(100));
    p.s.stop().unwrap();
    let log = drain_offline_log();
    for cc in [[0xB2, 7, 64], [0xB2, 10, 10], [0xB2, 7, 90], [0xB2, 10, 20]] {
        assert!(
            log.iter().any(|(_, b)| b == &cc.to_vec()),
            "{cc:?} in {log:?}"
        );
    }
    reset_host();
}

#[test]
fn levels_report_midi_activity() {
    let _g = serial();
    let p = project();
    assert!(p.s.levels().unwrap().midi.is_empty());
    p.s.play(p.seq).unwrap();
    let want = MidiLevelRow {
        dest: p.ext,
        velocity: 100,
    };
    let t = Instant::now();
    let mut seen = false;
    while t.elapsed() < Duration::from_secs(3) && !seen {
        sleep(Duration::from_millis(20));
        seen = p.s.levels().unwrap().midi.contains(&want);
    }
    p.s.stop().unwrap();
    assert!(seen, "note-on velocity 100 reaches levels().midi");
    reset_host();
}

/// The volume and pan messages to the external destination (channel 3)
/// in the log, after giving the offline device time to render them (a
/// reload's reset, CC 121, is left out).
fn ccs() -> Vec<Vec<u8>> {
    sleep(Duration::from_millis(150));
    drain_offline_log()
        .into_iter()
        .map(|(_, b)| b)
        .filter(|b| b.first() == Some(&0xB2) && matches!(b.get(1), Some(7 | 10)))
        .collect()
}

fn vol(v: Option<u8>) -> MidiMixRow {
    MidiMixRow {
        volume: v,
        pan: None,
        mute: false,
    }
}

#[test]
fn a_fresh_session_reaches_the_synth_while_stopped() {
    let _g = serial();
    let p = project();
    p.s.set_midi_mix(p.ext, vol(Some(70))).unwrap();
    assert_eq!(ccs(), vec![vec![0xB2, 7, 70]], "sent once, while stopped");
    reset_host();
}

#[test]
fn undo_and_redo_send_the_restored_volume() {
    let _g = serial();
    let p = project();
    p.s.set_midi_mix(p.ext, vol(Some(90))).unwrap();
    p.s.set_midi_mix(p.ext, vol(Some(40))).unwrap();
    assert_eq!(ccs(), vec![vec![0xB2, 7, 90], vec![0xB2, 7, 40]]);
    p.s.undo().unwrap();
    assert_eq!(ccs(), vec![vec![0xB2, 7, 90]], "undo puts the synth back");
    p.s.redo().unwrap();
    assert_eq!(ccs(), vec![vec![0xB2, 7, 40]], "redo sends it again");
    reset_host();
}

#[test]
fn an_undo_back_to_unset_sends_nothing() {
    let _g = serial();
    let p = project();
    p.s.set_midi_mix(p.ext, vol(Some(90))).unwrap();
    assert_eq!(ccs(), vec![vec![0xB2, 7, 90]]);
    p.s.undo().unwrap();
    assert_eq!(row(&p.s, p.ext).midi_mix, MidiMixRow::default());
    assert!(ccs().is_empty(), "unset never sends");
    reset_host();
}

#[test]
fn an_edit_sends_only_what_changed() {
    let _g = serial();
    let p = project();
    let mut mix = MidiMixRow {
        volume: Some(90),
        pan: Some(20),
        mute: false,
    };
    p.s.set_midi_mix(p.ext, mix.clone()).unwrap();
    assert_eq!(ccs(), vec![vec![0xB2, 7, 90], vec![0xB2, 10, 20]]);
    // Mute alone sends no controller (final review m5).
    mix.mute = true;
    p.s.set_midi_mix(p.ext, mix.clone()).unwrap();
    assert!(ccs().is_empty());
    mix.pan = Some(100);
    p.s.set_midi_mix(p.ext, mix.clone()).unwrap();
    assert_eq!(ccs(), vec![vec![0xB2, 10, 100]], "pan alone");
    reset_host();
}

#[test]
fn a_busy_engine_elsewhere_sends_nothing_and_fails_nothing() {
    let _g = serial();
    let p = project();
    let q = project();
    q.s.play(q.seq).unwrap();
    sleep(Duration::from_millis(50));
    drain_offline_log();
    p.s.set_midi_mix(p.ext, vol(Some(77))).unwrap();
    assert!(!p.s.preview_midi_mix(p.ext, Some(66), None).unwrap());
    let log = ccs();
    q.s.stop().unwrap();
    assert!(
        !log.iter()
            .any(|b| b == &vec![0xB2, 7, 77] || b == &vec![0xB2, 7, 66]),
        "{log:?}"
    );
    assert_eq!(row(&p.s, p.ext).midi_mix, vol(Some(77)), "the edit stands");
    reset_host();
}

#[test]
fn a_destination_no_track_plays_is_not_reached_and_fails_nothing() {
    let _g = serial();
    let mut b = Builder::new();
    let ext: DestId = b.dest("Ext", "virtual:out", 2).unwrap();
    let idle: DestId = b.dest("Idle", "virtual:out", 5).unwrap();
    let (seq, lane) = b.sequence("S", "L", Some(ext)).unwrap();
    b.notes(seq, lane, &[(0, 60, 100 << 9, Q)]).unwrap();
    let s = Session::from_document(b.finish()).unwrap();
    s.set_midi_mix(idle.0, vol(Some(55))).unwrap();
    assert!(!s.preview_midi_mix(idle.0, Some(56), None).unwrap());
    sleep(Duration::from_millis(150));
    let log = drain_offline_log();
    assert!(
        !log.iter().any(|(_, b)| b.first() == Some(&0xB5)),
        "{log:?}"
    );
    assert_eq!(row(&s, idle.0).midi_mix, vol(Some(55)), "the edit stands");
    reset_host();
}

/// Note-ons to the external destination seen within `wait`.
fn note_ons_within(wait: Duration) -> usize {
    let t = Instant::now();
    let mut n = 0;
    while t.elapsed() < wait && n == 0 {
        sleep(Duration::from_millis(20));
        n = drain_offline_log()
            .iter()
            .filter(|(_, b)| b.first() == Some(&0x92) && b.get(2).is_some_and(|v| *v > 0))
            .count();
    }
    n
}

#[test]
fn an_undo_of_a_mute_lets_the_next_note_play() {
    let _g = serial();
    let mut b = Builder::new();
    let ext: DestId = b.dest("Ext", "virtual:out", 2).unwrap();
    let (seq, lane) = b.sequence("S", "L", Some(ext)).unwrap();
    let notes: Vec<(i64, u8, u16, i64)> =
        (0..64).map(|i| (i * Q / 2, 60, 100 << 9, Q / 4)).collect();
    b.notes(seq, lane, &notes).unwrap();
    let s = Session::from_document(b.finish()).unwrap();
    s.play(seq.0).unwrap();
    assert!(note_ons_within(Duration::from_secs(3)) > 0, "playing");
    let mute = MidiMixRow {
        volume: None,
        pan: None,
        mute: true,
    };
    s.set_midi_mix(ext.0, mute).unwrap();
    sleep(Duration::from_millis(100));
    drain_offline_log();
    assert_eq!(note_ons_within(Duration::from_millis(800)), 0, "muted");
    s.undo().unwrap();
    assert!(
        note_ons_within(Duration::from_secs(3)) > 0,
        "the next note plays after Undo"
    );
    s.stop().unwrap();
    reset_host();
}

/// Re-review N1: with "Song" (on A) shown and "Pads" (on B) not, a change
/// to B claims nothing (no engine is loaded with a sequence not shown);
/// a change to A claims for Song and reaches the synth.
#[test]
fn only_the_shown_sequence_is_claimed_and_only_when_it_plays_the_destination() {
    let _g = serial();
    let mut b = Builder::new();
    let a: DestId = b.dest("A", "virtual:out", 2).unwrap();
    let pads: DestId = b.dest("B", "virtual:out", 5).unwrap();
    let (song, la) = b.sequence("Song", "L", Some(a)).unwrap();
    b.notes(song, la, &[(0, 60, 100 << 9, Q)]).unwrap();
    let (other, lb) = b.sequence("Pads", "L", Some(pads)).unwrap();
    b.notes(other, lb, &[(0, 60, 100 << 9, Q)]).unwrap();
    let s = Session::from_document(b.finish()).unwrap();
    s.set_midi_mix(pads.0, vol(Some(55))).unwrap();
    assert!(!s.preview_midi_mix(pads.0, Some(56), None).unwrap());
    assert_eq!(holder(), None, "no engine claimed for a sequence not shown");
    s.set_midi_mix(a.0, vol(Some(70))).unwrap();
    assert!(holder().is_some(), "claimed for the shown sequence");
    sleep(Duration::from_millis(150));
    let log = drain_offline_log();
    assert!(log.iter().any(|(_, b)| b == &vec![0xB2, 7, 70]), "{log:?}");
    reset_host();
}
