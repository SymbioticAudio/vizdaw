//! The Bounce sheet's model (slice 7b Task 6; Swift `BounceModelTests`):
//! the choices start from the project's format, the core's check runs off
//! the UI thread (only the newest answer lands), the request follows the
//! choices, a refused start is the sheet's error, a finished bounce says
//! what it wrote and closes the sheet, Cancel stops the job. Swift's
//! ninth test (`tailSettingIsKeptAndApplied`) is the audio settings'
//! bounce tail, not this sheet's.

mod support;

use std::sync::Arc;

use support::{FakeSession, demo_fake, mixer_fake};
use vizdaw_app::DocumentModel;
use vizdaw_app::bounce::{NO_SEQUENCE, RATES, Source, duration, extension, rate_title};
use vizdaw_app::testing::{JobRequest, JobScript};
use vizdaw_ffi::TickRange;
use vizdaw_ffi::{
    BitDepthRow, BounceCheckRow, BounceRangeRow, BounceRequestRow, BounceResultRow,
    BounceSourceRow, BouncedFileRow, ContainerRow, JobKindRow, ProjectFormatRow, ProjectInfoRow,
    VizError,
};

fn doc(f: &Arc<FakeSession>) -> DocumentModel {
    DocumentModel::new(f.clone()).unwrap()
}

fn info(rate: u32, depth: BitDepthRow) -> ProjectInfoRow {
    ProjectInfoRow {
        format: ProjectFormatRow {
            sample_rate: rate,
            depth,
        },
        mismatched: 0,
        rate_mismatch: false,
        device_rate: 0.0,
    }
}

fn model(msg: &str) -> VizError {
    VizError::Model {
        message: msg.into(),
    }
}

fn bounces(f: &FakeSession) -> Vec<BounceRequestRow> {
    f.jobs_started()
        .into_iter()
        .filter_map(|j| match j.request {
            JobRequest::Bounce(r) => Some(r),
            _ => None,
        })
        .collect()
}

/// Shows the sheet and runs its check (the UI runs it off its thread).
fn show(m: &mut DocumentModel) {
    m.show_bounce();
    m.run_bounce_check();
}

/// Swift `defaultsFollowTheProject`.
#[test]
fn defaults_follow_the_project() {
    let f = Arc::new(demo_fake());
    f.set_project_info(info(44_100, BitDepthRow::Int16));
    let mut m = doc(&f);
    show(&mut m);
    let c = m.bounce().choices().clone();
    assert!(
        c.source == Source::Mix
            && c.container == ContainerRow::Wav
            && c.rate == 44_100
            && c.depth == BitDepthRow::Int16
            && c.dither
    );
    assert!(!c.add_to_project && !c.use_selection);
    m.edit_bounce(|c| c.depth = BitDepthRow::Int24);
    assert!(m.bounce().choices().dither, "kept as set");
    assert!(m.bounce_request(Some("/tmp/x.wav"), false).unwrap().dither);
    m.edit_bounce(|c| c.depth = BitDepthRow::Float32);
    assert!(
        !m.bounce_request(Some("/tmp/x.wav"), false).unwrap().dither,
        "float ignores dither"
    );
    assert_eq!(RATES, [44_100, 48_000, 88_200, 96_000, 176_400, 192_000]);
    // Showing again starts from the project's defaults.
    f.set_project_info(info(96_000, BitDepthRow::Int24));
    m.hide_bounce();
    show(&mut m);
    let c = m.bounce().choices();
    assert!(c.rate == 96_000 && c.depth == BitDepthRow::Int24 && !c.dither);
}

/// Swift `theWarningComesFromTheCore`.
#[test]
fn the_warning_comes_from_the_core() {
    let f = Arc::new(demo_fake());
    let warning = "1 lane plays external MIDI and will be silent. Track it as audio first: Track Synth\u{2026}";
    f.set_bounce_check(BounceCheckRow {
        external_lanes: 1,
        frames: 96_000,
        files: vec![],
        warning: Some(warning.into()),
    });
    let mut m = doc(&f);
    show(&mut m);
    assert_eq!(m.bounce().warning(), Some(warning));
    assert_eq!(m.bounce().length(), "0:02.0 + tail");
    let asked = f.bounce_checks().len();
    m.edit_bounce(|c| {
        c.source = Source::Stems;
        c.stems = [1].into();
    });
    m.run_bounce_check();
    assert!(f.bounce_checks().len() > asked, "refetched after a change");
    assert_eq!(
        f.bounce_checks().last().unwrap().source,
        BounceSourceRow::Stems { strips: vec![1] }
    );
}

/// The check runs where the UI puts it, and only the newest answer lands
/// (an older one finishing late never overwrites it).
#[test]
fn only_the_newest_check_lands() {
    let f = Arc::new(demo_fake());
    let mut m = doc(&f);
    m.show_bounce();
    assert_eq!(f.bounce_checks().len(), 0, "nothing ran on this thread");
    let old = m.take_bounce_check().expect("a check to run");
    m.edit_bounce(|c| c.rate = 96_000);
    let new = m.take_bounce_check().expect("a newer check");
    assert!(m.take_bounce_check().is_none(), "taken once");
    f.set_bounce_check(BounceCheckRow {
        external_lanes: 0,
        frames: 96_000,
        files: vec![],
        warning: Some("new".into()),
    });
    let new = new.run(f.as_ref());
    f.set_bounce_check(BounceCheckRow {
        external_lanes: 0,
        frames: 48_000,
        files: vec![],
        warning: Some("old".into()),
    });
    let old = old.run(f.as_ref());
    m.land_bounce_check(new);
    m.land_bounce_check(old);
    assert_eq!(m.bounce().warning(), Some("new"));
    assert_eq!(m.bounce().length(), "0:01.0 + tail");
    // An edit that changes nothing asks for no check.
    m.edit_bounce(|c| c.rate = 96_000);
    assert!(m.take_bounce_check().is_none());
}

/// Swift `startBuildsTheRequest`.
#[test]
fn start_builds_the_request() {
    let f = Arc::new(mixer_fake());
    f.script_job(JobScript::done().steps(&[0.5; 10]));
    let mut m = doc(&f);
    show(&mut m);
    let ids: Vec<u64> = m.bounce().strips().iter().map(|s| s.id).collect();
    assert_eq!(ids, [1, 2], "every strip but master");
    m.edit_bounce(|c| {
        c.source = Source::Stems;
        c.stems = [2, 1].into();
    });
    m.start_bounce(Some("/tmp/stems"), false);
    let r = bounces(&f).last().cloned().unwrap();
    assert_eq!(r.source, BounceSourceRow::Stems { strips: vec![1, 2] });
    assert_eq!(r.path.as_deref(), Some("/tmp/stems"));
    assert!(!r.add_to_project);
    let job = m.bounce().job().expect("running");
    assert!(m.jobs().iter().any(|j| j.kind == JobKindRow::Bounce));
    assert!(m.bounce_running());
    assert_eq!(m.bounce_progress(), Some(0.5));
    m.cancel_bounce();
    assert_eq!(f.cancelled_jobs().last(), Some(&job));
    // The cancelled end is quiet and closes the sheet.
    m.poll_jobs();
    assert!(m.alert.is_none());
    assert!(!m.bounce().shown() && m.bounce().job().is_none());
    show(&mut m);
    m.edit_bounce(|c| {
        c.source = Source::Mix;
        c.add_to_project = true;
    });
    m.start_bounce(None, false);
    let add = bounces(&f).last().cloned().unwrap();
    assert!(add.path.is_none() && add.add_to_project && add.source == BounceSourceRow::Mix);
}

/// Swift `aRefusedStartIsAnAlert`: shown in the sheet, not behind it.
#[test]
fn a_refused_start_is_the_sheets_error() {
    let f = Arc::new(demo_fake());
    f.fail_next(
        "bounce",
        model("x.wav exists; choose another name or replace it"),
    );
    let mut m = doc(&f);
    show(&mut m);
    m.start_bounce(Some("/tmp/x.wav"), false);
    assert!(m.alert.is_none(), "shown in the sheet, not behind it");
    assert_eq!(
        m.bounce().error(),
        Some("x.wav exists; choose another name or replace it")
    );
    assert!(m.bounce().job().is_none());
    assert!(m.bounce().shown());
}

/// Swift `stemsNeverReplace` (owner answer 2: the core numbers stems).
#[test]
fn stems_never_replace() {
    let f = Arc::new(demo_fake());
    let mut m = doc(&f);
    show(&mut m);
    m.edit_bounce(|c| {
        c.source = Source::Stems;
        c.stems = [1].into();
    });
    m.start_bounce(Some("/tmp/s"), true);
    assert!(!bounces(&f).last().unwrap().replace, "stems never replace");
    m.edit_bounce(|c| c.source = Source::Mix);
    show(&mut m);
    m.start_bounce(Some("/tmp/s.wav"), true);
    assert!(
        bounces(&f).last().unwrap().replace,
        "a mix replaces as asked"
    );
}

/// Swift `aFailedCheckIsShownAndCleared`.
#[test]
fn a_failed_check_is_shown_and_cleared() {
    let f = Arc::new(demo_fake());
    f.fail_next("bounce_check", model("bad rate"));
    let mut m = doc(&f);
    show(&mut m);
    assert_eq!(m.bounce().error(), Some("bad rate"));
    assert!(m.bounce().warning().is_none() && m.bounce().length().is_empty());
    m.edit_bounce(|c| c.rate = 96_000);
    assert!(
        m.bounce().error().is_none(),
        "cleared as the check is asked"
    );
    m.run_bounce_check();
    assert!(m.bounce().error().is_none());
}

/// Swift `aFinishedBounceSaysWhatItWrote`.
#[test]
fn a_finished_bounce_says_what_it_wrote() {
    let f = Arc::new(demo_fake());
    f.script_job(JobScript::done().bounce(BounceResultRow {
        files: vec![BouncedFileRow {
            path: "/tmp/s/Bass 2.wav".into(),
            frames: 96_000,
            rate: 48_000,
            bits: "24-bit".into(),
            channels: 2,
            clipped: 0,
        }],
        media: vec![],
    }));
    let mut m = doc(&f);
    show(&mut m);
    m.start_bounce(Some("/tmp/s/Bass.wav"), true);
    assert!(m.status().is_some_and(|s| s.contains("/tmp/s/Bass 2.wav")));
    assert!(
        !m.bounce().shown() && m.bounce().job().is_none(),
        "instant Done closes"
    );
}

/// A bounce that ends later (polled) closes the sheet too, and a failure
/// is the alert "Bounce Failed".
#[test]
fn a_failed_bounce_is_the_alert_and_closes_the_sheet() {
    let f = Arc::new(demo_fake());
    f.script_job(JobScript::failed("no space").steps(&[0.5]));
    let mut m = doc(&f);
    show(&mut m);
    m.start_bounce(Some("/tmp/x.wav"), true);
    assert!(m.bounce_running() && m.bounce().shown());
    m.poll_jobs();
    assert!(!m.bounce().shown() && m.bounce().job().is_none());
    let a = m.alert.clone().expect("an alert");
    assert_eq!(a.title, "Bounce Failed");
    assert!(a.message.starts_with("no space"));
}

/// Swift `rateChoiceAndSelectionAndDestination`.
#[test]
fn rate_choice_and_selection_and_destination() {
    let f = Arc::new(demo_fake());
    let mut m = doc(&f);
    show(&mut m);
    m.edit_bounce(|c| c.set_rate_choice(0));
    assert!(m.bounce().choices().other_rate);
    assert_eq!(m.bounce().choices().rate_choice(), 0);
    m.edit_bounce(|c| c.set_other_rate(5));
    assert_eq!(m.bounce().choices().rate, 8_000);
    m.edit_bounce(|c| c.set_other_rate(999_999));
    assert_eq!(m.bounce().choices().rate, 384_000);
    m.edit_bounce(|c| c.set_rate_choice(96_000));
    assert!(!m.bounce().choices().other_rate && m.bounce().choices().rate == 96_000);
    assert!(m.bounce().choices().needs_destination());
    m.edit_bounce(|c| {
        c.add_to_project = true;
        c.write_files = false;
    });
    assert!(!m.bounce().choices().needs_destination());
    m.edit_bounce(|c| c.add_to_project = false);
    assert!(
        m.bounce().choices().needs_destination(),
        "a stale write_files cannot skip the panel"
    );
    assert!(!m.bounce_has_selection());
    m.set_time_range(TickRange { start: 0, end: 960 });
    m.edit_bounce(|c| c.use_selection = true);
    assert!(m.bounce_has_selection());
    assert_eq!(
        m.bounce_request(Some("/tmp/x.wav"), false).unwrap().range,
        BounceRangeRow::Ticks { start: 0, end: 960 }
    );
}

/// A project rate the picker does not list starts at "Other…".
#[test]
fn an_unlisted_project_rate_is_other() {
    let f = Arc::new(demo_fake());
    f.set_project_info(info(22_050, BitDepthRow::Int24));
    let mut m = doc(&f);
    show(&mut m);
    assert!(m.bounce().choices().other_rate);
    assert_eq!(m.bounce().choices().rate, 22_050);
}

/// With no sequence shown the sheet says so and nothing is checked.
#[test]
fn no_sequence_is_the_sheets_error() {
    let f = Arc::new(FakeSession::new(vec![]));
    let mut m = doc(&f);
    m.show_bounce();
    assert_eq!(m.bounce().error(), Some(NO_SEQUENCE));
    assert!(m.take_bounce_check().is_none());
    m.start_bounce(Some("/tmp/x.wav"), true);
    assert_eq!(m.bounce().error(), Some(NO_SEQUENCE));
    assert!(bounces(&f).is_empty());
}

#[test]
fn texts_and_extensions() {
    assert_eq!(duration(48_000 * 92 + 19_200, 48_000), "1:32.4");
    assert_eq!(duration(48_000 * 3_725, 48_000), "1:02:05.0");
    assert_eq!(rate_title(44_100), "44100 Hz");
    assert_eq!(extension(ContainerRow::Aiff), "aif");
    assert_eq!(extension(ContainerRow::Wav), "wav");
    assert_eq!(extension(ContainerRow::Caf), "caf");
}
