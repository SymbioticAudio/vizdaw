//! A device that refuses the project rate (plan 5a Ruling 20), simulated
//! with the offline backend's rate hook. Its own test binary: the hook and
//! the engine host are process-wide, so these tests run one at a time.

use std::sync::{Mutex, MutexGuard};
use std::time::{Duration, Instant};
use vizdaw_ffi::{BitDepthRow, JobStateRow, ProjectFormatRow, Session};

static SERIAL: Mutex<()> = Mutex::new(());

fn serial() -> MutexGuard<'static, ()> {
    let g = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    vizdaw_ffi::host::force_offline();
    vizdaw_ffi::host::set_offline_rate(0.0);
    vizdaw_ffi::host::reset_host();
    g
}

#[test]
fn refused_device_rate_mutes_regions() {
    let _g = serial();
    vizdaw_ffi::host::set_offline_rate(44_100.0);
    let s = Session::new_demo_audio().unwrap();
    // Ruling 20 / F19: the device rate is known only once a host exists.
    let i = s.project_info().unwrap();
    assert_eq!((i.rate_mismatch, i.device_rate), (false, 0.0));
    let seq = s.snapshot().unwrap().sequences[0].id;
    s.play(seq).unwrap();
    let t = s.transport().unwrap();
    assert!(t.rate_mismatch);
    assert_eq!(t.sample_rate, 44_100.0);
    let i = s.project_info().unwrap();
    assert!(i.rate_mismatch);
    assert_eq!(i.device_rate, 44_100.0);
    s.stop().unwrap();
    // Following the project again: the 48 kHz demo matches.
    vizdaw_ffi::host::set_offline_rate(0.0);
    vizdaw_ffi::host::reset_host();
    let s2 = Session::new_demo().unwrap();
    s2.play(s2.snapshot().unwrap().sequences[0].id).unwrap();
    let t = s2.transport().unwrap();
    assert!(!t.rate_mismatch);
    assert_eq!(t.sample_rate, 48_000.0);
    s2.stop().unwrap();
    s.close().unwrap();
    s2.close().unwrap();
}

/// A format change drops the host this session plays through; the next
/// Play opens at the new rate (offline: the project's).
#[test]
fn a_format_change_reopens_the_host_at_the_new_rate() {
    let _g = serial();
    let s = Session::new_demo().unwrap();
    let seq = s.snapshot().unwrap().sequences[0].id;
    s.play(seq).unwrap();
    assert_eq!(s.transport().unwrap().sample_rate, 48_000.0);
    let created = vizdaw_ffi::host::hosts_created();
    // The demo has no media: the change needs no conversion.
    let id = s
        .set_project_format(ProjectFormatRow {
            sample_rate: 44_100,
            depth: BitDepthRow::Int24,
        })
        .unwrap();
    let until = Instant::now() + Duration::from_secs(30);
    let j = loop {
        let j = s.job(id).unwrap();
        if j.state != JobStateRow::Running {
            break j;
        }
        assert!(Instant::now() < until);
        std::thread::sleep(Duration::from_millis(5));
    };
    assert_eq!(j.state, JobStateRow::Done, "{j:?}");
    let t = s.transport().unwrap();
    assert!(!t.playing, "the host went");
    assert_eq!(t.sample_rate, 0.0);
    s.play(seq).unwrap();
    let t = s.transport().unwrap();
    assert_eq!((t.sample_rate, t.rate_mismatch), (44_100.0, false));
    assert_eq!(vizdaw_ffi::host::hosts_created(), created + 1);
    // Undo goes back to 48 kHz: that host goes too.
    s.undo().unwrap();
    s.play(seq).unwrap();
    let t = s.transport().unwrap();
    assert_eq!((t.sample_rate, t.rate_mismatch), (48_000.0, false));
    s.stop().unwrap();
    s.close().unwrap();
}

/// A new project at 96 kHz plays on an offline host at 96 kHz.
#[test]
fn the_offline_host_follows_a_new_projects_rate() {
    let _g = serial();
    let s = Session::new_project(ProjectFormatRow {
        sample_rate: 96_000,
        depth: BitDepthRow::Float32,
    })
    .unwrap();
    // A sequence to play.
    let snap = s
        .edit(
            vizdaw_ffi::EditCommand::Sequence {
                edit: vizdaw_ffi::SequenceEdit::New {
                    name: "Song".into(),
                },
            },
            None,
        )
        .unwrap();
    s.play(snap.sequences[0].id).unwrap();
    let t = s.transport().unwrap();
    assert_eq!((t.sample_rate, t.rate_mismatch), (96_000.0, false));
    assert_eq!(s.project_info().unwrap().device_rate, 96_000.0);
    s.stop().unwrap();
    s.close().unwrap();
}
