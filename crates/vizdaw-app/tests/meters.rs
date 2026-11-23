//! Meters and polling: Swift `metersHoldPeaksAndFall`,
//! `metersSanitiseNonFinitePeaks`, the 30 Hz tick, the decay by elapsed
//! time, silent meters for a session without the engine, and the heard
//! values while playing.

mod support;

use std::sync::Arc;
use std::time::{Duration, Instant};

use support::{FakeSession, mixer_fake};
use vizdaw_app::DocumentModel;
use vizdaw_app::mixer::meters::{METER_PERIOD, POLL_INTERVAL, decay};
use vizdaw_app::mixer::{MeterLevel, MixerMath, MixerModel};
use vizdaw_ffi::VizError;
use vizdaw_ffi::events::AutomationTargetRow as T;
use vizdaw_ffi::mixer::{MeterRow, MidiLevelRow, MixerLevels};

fn make() -> (Arc<FakeSession>, DocumentModel) {
    let f = Arc::new(mixer_fake());
    let mut d = DocumentModel::new(f.clone()).unwrap();
    assert!(d.ensure_mixer());
    (f, d)
}

fn mx<R>(d: &mut DocumentModel, f: impl FnOnce(&mut MixerModel, &mut DocumentModel) -> R) -> R {
    d.with_mixer(f).expect("a mixer")
}

fn levels(meters: Vec<MeterRow>) -> MixerLevels {
    MixerLevels {
        meters,
        ..MixerLevels::empty()
    }
}

fn row(strip: u64, l: f32, r: f32) -> MeterRow {
    MeterRow {
        strip,
        peak_l: l,
        peak_r: r,
    }
}

fn level(d: &DocumentModel, strip: u64) -> MeterLevel {
    d.mixer().unwrap().meters().level(strip)
}

#[test]
fn meters_hold_peaks_and_fall() {
    let (f, mut d) = make();
    let t0 = Instant::now();
    f.set_levels(MixerLevels {
        meters: vec![row(1, 1.0, 0.5)],
        dsp_load: 0.25,
        dsp_peak: 0.5,
        latency_frames: 480,
        faulted: vec![10],
        midi: vec![],
    });
    mx(&mut d, |m, d| m.poll_levels(d, t0));
    assert_eq!(
        level(&d, 1),
        MeterLevel {
            left: 1.0,
            right: 0.5
        }
    );
    assert!(d.mixer().unwrap().meters().faulted.contains(&10));
    f.set_levels(MixerLevels {
        meters: vec![row(1, 0.0, 0.6)],
        dsp_load: 0.25,
        dsp_peak: 0.5,
        latency_frames: 480,
        faulted: vec![10],
        midi: vec![],
    });
    mx(&mut d, |m, d| m.poll_levels(d, t0 + METER_PERIOD));
    assert_eq!(level(&d, 1).left, MixerMath::METER_DECAY);
    assert_eq!(level(&d, 1).right, 0.6);
    let m = d.mixer().unwrap();
    assert_eq!(
        m.load_text(0.0),
        "DSP 25% \u{00B7} peak 50%",
        "no rate before Play"
    );
    assert_eq!(
        m.load_text(48_000.0),
        "DSP 25% \u{00B7} peak 50% \u{00B7} PDC 10.0 ms"
    );
}

#[test]
fn meters_sanitise_non_finite_peaks() {
    let (f, mut d) = make();
    let t0 = Instant::now();
    f.set_levels(levels(vec![row(1, f32::NAN, f32::INFINITY)]));
    mx(&mut d, |m, d| m.poll_levels(d, t0));
    assert_eq!(level(&d, 1).left, 0.0);
    assert_eq!(level(&d, 1).right, MixerMath::MAX_METER_PEAK);
    f.set_levels(levels(vec![row(1, 0.0, 0.0)]));
    mx(&mut d, |m, d| m.poll_levels(d, t0 + METER_PERIOD));
    assert_eq!(
        level(&d, 1).right,
        MixerMath::MAX_METER_PEAK * MixerMath::METER_DECAY
    );
    f.set_levels(levels(vec![row(1, -1.0, 9.0)]));
    mx(&mut d, |m, d| m.poll_levels(d, t0 + METER_PERIOD * 2));
    assert_eq!(
        level(&d, 1).right,
        MixerMath::MAX_METER_PEAK,
        "huge peaks clamp"
    );
}

#[test]
fn the_decay_scales_with_the_time_that_passed() {
    assert_eq!(decay(METER_PERIOD), MixerMath::METER_DECAY);
    assert!((decay(METER_PERIOD * 2) - 0.85 * 0.85).abs() < 1e-6);
    assert!((decay(METER_PERIOD / 2) - 0.85f32.sqrt()).abs() < 1e-6);
    let (f, mut d) = make();
    let t0 = Instant::now();
    f.set_levels(levels(vec![row(1, 1.0, 1.0)]));
    mx(&mut d, |m, d| m.poll_levels(d, t0));
    let m = d.mixer().unwrap().meters();
    assert_eq!(m.level_at(1, t0).left, 1.0);
    assert!(
        (m.level_at(1, t0 + METER_PERIOD).left - 0.85).abs() < 1e-6,
        "a frame between reads falls by the time since the read"
    );
    f.set_levels(levels(vec![row(1, 0.0, 0.0)]));
    mx(&mut d, |m, d| m.poll_levels(d, t0 + METER_PERIOD * 3));
    assert!(
        (level(&d, 1).left - 0.85f32.powi(3)).abs() < 1e-6,
        "a late read falls further"
    );
}

#[test]
fn a_strip_missing_from_a_take_falls_to_silence() {
    let (f, mut d) = make();
    let t0 = Instant::now();
    f.set_levels(levels(vec![row(1, 1.0, 1.0), row(2, 0.5, 0.5)]));
    mx(&mut d, |m, d| m.poll_levels(d, t0));
    f.set_levels(levels(vec![row(2, 0.5, 0.5)]));
    mx(&mut d, |m, d| m.poll_levels(d, t0 + METER_PERIOD));
    assert_eq!(
        level(&d, 1).left,
        MixerMath::METER_DECAY,
        "it falls, not jumps"
    );
    mx(&mut d, |m, d| m.poll_levels(d, t0 + Duration::from_secs(2)));
    assert_eq!(level(&d, 1), MeterLevel::SILENT);
    let strips: Vec<u64> = d.mixer().unwrap().meters().strips().collect();
    assert_eq!(strips, [2], "a silent strip is dropped");
}

#[test]
fn a_second_document_has_silent_meters() {
    let (f, mut d) = make();
    // A session that does not hold the engine reads empty levels.
    f.set_levels(MixerLevels::empty());
    let t0 = Instant::now();
    assert!(mx(&mut d, |m, d| m.tick(d, t0)));
    for strip in [0, 1, 2] {
        assert_eq!(level(&d, strip), MeterLevel::SILENT);
    }
    assert!(
        d.alert.is_none() && d.status().is_none(),
        "silence, not an error"
    );
    assert_eq!(f.levels_reads(), 1);
}

#[test]
fn a_refused_read_keeps_the_last_meters() {
    let (f, mut d) = make();
    let t0 = Instant::now();
    f.set_levels(levels(vec![row(1, 0.5, 0.5)]));
    mx(&mut d, |m, d| m.poll_levels(d, t0));
    f.set_call_error(
        "levels",
        Some(VizError::Busy {
            message: "busy".into(),
        }),
    );
    mx(&mut d, |m, d| m.poll_levels(d, t0 + METER_PERIOD));
    assert_eq!(level(&d, 1).left, 0.5);
    assert!(d.alert.is_none());
}

#[test]
fn the_tick_reads_at_most_every_poll_interval() {
    let (f, mut d) = make();
    let t0 = Instant::now();
    assert!(mx(&mut d, |m, d| m.tick(d, t0)));
    assert!(!mx(&mut d, |m, d| m.tick(d, t0 + Duration::from_millis(10))));
    assert!(!mx(&mut d, |m, d| m.tick(d, t0 + Duration::from_millis(29))));
    assert!(mx(&mut d, |m, d| m.tick(d, t0 + POLL_INTERVAL)));
    assert_eq!(
        f.levels_reads(),
        2,
        "one read per frame however many placements ask"
    );
}

#[test]
fn heard_values_are_read_with_the_meters_while_playing() {
    let (f, mut d) = make();
    let t0 = Instant::now();
    mx(&mut d, |m, d| m.tick(d, t0));
    assert!(f.heard_calls().is_empty(), "stopped: nothing is read");
    f.set_playing(true);
    let _ = d.poll_transport();
    f.set_heard([(T::StripPan { strip: 2 }, -0.5)].into());
    mx(&mut d, |m, d| m.tick(d, t0 + POLL_INTERVAL));
    assert_eq!(
        f.heard_calls()[0],
        [
            T::StripFader { strip: 1 },
            T::StripPan { strip: 1 },
            T::StripFader { strip: 2 },
            T::StripPan { strip: 2 },
            T::StripFader { strip: 0 },
            T::StripPan { strip: 0 },
        ]
    );
    let m = d.mixer().unwrap();
    assert_eq!(
        m.pan(m.strip(2).unwrap()),
        -0.5,
        "the pan follows its automation"
    );
}

#[test]
fn midi_activity_rises_with_velocity_and_falls() {
    let (f, mut d) = make();
    let t0 = Instant::now();
    f.set_levels(MixerLevels {
        midi: vec![MidiLevelRow {
            dest: 7,
            velocity: 127,
        }],
        ..MixerLevels::empty()
    });
    mx(&mut d, |m, d| m.poll_levels(d, t0));
    assert_eq!(d.mixer().unwrap().meters().midi_activity(7), 1.0);
    f.set_levels(MixerLevels::empty());
    mx(&mut d, |m, d| m.poll_levels(d, t0 + METER_PERIOD));
    assert!((d.mixer().unwrap().meters().midi_activity(7) - 0.85).abs() < 1e-6);
    mx(&mut d, |m, d| m.poll_levels(d, t0 + Duration::from_secs(5)));
    assert_eq!(d.mixer().unwrap().meters().midi_activity(7), 0.0);
}
