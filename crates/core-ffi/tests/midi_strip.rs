//! A MIDI strip's mix through the session (plan 5g-1b Task 3): rows, set,
//! preview and note activity. Offline only; the engine host is
//! process-wide, so one test at a time.

use core_engine::fixtures::{Builder, Q};
use core_model::ids::DestId;
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread::sleep;
use std::time::{Duration, Instant};
use vizdaw_ffi::host::{drain_offline_log, force_offline, reset_host};
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
    assert!(
        !p.s.preview_midi_mix(p.ext, Some(64), None).unwrap(),
        "no engine yet"
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
