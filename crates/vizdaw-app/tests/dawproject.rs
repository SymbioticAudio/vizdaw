//! The Export DAWproject sheet's model (slice 7c Task 4; Swift
//! `DawprojectModelTests`): the core's check off the UI thread (only the
//! newest answer lands), the core's file name, an export that waits for
//! a running job (the interchange and media jobs) instead of failing as
//! Busy, `replace` only for what was there when the panel confirmed, and
//! a failure that brings the sheet back with its message.

mod support;

use std::sync::Arc;

use support::{FakeSession, demo_fake};
use vizdaw_app::DocumentModel;
use vizdaw_app::dawproject::{
    APPLY, EXPORT_FAILED, LEAVE_OUT, REGION_GAIN, SUBTITLE, TITLE, WAITING_TEXT,
};
use vizdaw_app::editor::BlockRuleSetting;
use vizdaw_app::for_daw::NO_SEQUENCE;
use vizdaw_app::jobs::failure_title;
use vizdaw_app::testing::{JobRequest, JobScript};
use vizdaw_ffi::{
    BitDepthRow, DawprojectExportRequestRow, JobKindRow, MidiImportRequestRow, ProjectFormatRow,
    RegionGainRow, ReportRow, VizError,
};

fn report(summary: &str) -> ReportRow {
    ReportRow {
        summary: summary.into(),
        items: vec![],
    }
}

fn fake() -> Arc<FakeSession> {
    let f = demo_fake();
    f.set_check_report("export_dawproject_check", report("check"));
    Arc::new(f)
}

fn shown(f: &Arc<FakeSession>) -> DocumentModel {
    let mut m = DocumentModel::new(f.clone()).unwrap();
    m.show_dawproject();
    m.run_dawproject_check();
    m
}

fn exports(f: &FakeSession) -> Vec<DawprojectExportRequestRow> {
    f.jobs_started()
        .into_iter()
        .filter_map(|j| match j.request {
            JobRequest::ExportDawproject(r) => Some(r),
            _ => None,
        })
        .collect()
}

fn model(message: &str) -> VizError {
    VizError::Model {
        message: message.into(),
    }
}

/// A job of `kind` that ends at the next poll.
fn running(f: &FakeSession, m: &mut DocumentModel, kind: JobKindRow) {
    f.script_job(JobScript::done().steps(&[0.4]));
    let started = match kind {
        JobKindRow::Bounce => {
            let req = m.bounce_request(Some("/tmp/b.wav"), false).unwrap();
            m.try_start_job(kind, |s| s.bounce(req))
        }
        JobKindRow::ImportMidi => m.try_start_job(kind, |s| {
            s.import_midi(MidiImportRequestRow {
                path: "/tmp/a.mid".into(),
                new_project: false,
                seq: 3,
                lane: None,
                tick: 0,
                channel_map: vec![],
                split_channels: None,
                mpe: false,
                use_tempo: false,
            })
        }),
        JobKindRow::Convert => m.try_start_job(kind, |s| s.convert_media()),
        JobKindRow::Relink => m.try_start_job(kind, |s| s.relink_media(1, "/tmp/r.wav".into())),
        JobKindRow::Format => m.try_start_job(kind, |s| {
            s.set_project_format(ProjectFormatRow {
                sample_rate: 96_000,
                depth: BitDepthRow::Int24,
            })
        }),
        other => panic!("no blocker for {other:?}"),
    };
    started.expect("the blocker starts");
}

/// Swift `checkRunsDetachedAndKeepsTheLatest`.
#[test]
fn check_runs_off_the_ui_thread_and_keeps_the_latest() {
    let f = fake();
    let mut m = DocumentModel::new(f.clone()).unwrap();
    m.show_dawproject();
    assert!(f.dawproject_checks().is_empty(), "not on this thread");
    assert!(m.dawproject().checking());
    m.run_dawproject_check();
    let checks = f.dawproject_checks();
    assert!(checks.len() == 1 && checks[0].path.is_empty());
    assert_eq!(m.dawproject().wont_transfer(), Some(&report("check")));
    assert!(m.dawproject().error().is_none());

    // Two quick changes: the first one's late answer is dropped.
    m.set_dawproject_region_gain(RegionGainRow::LeaveOut);
    let first = m.take_dawproject_check().unwrap();
    m.set_dawproject_region_gain(RegionGainRow::Apply);
    f.set_check_report("export_dawproject_check", report("second"));
    m.run_dawproject_check();
    m.land_dawproject_check(first.run(f.as_ref()));
    assert_eq!(m.dawproject().wont_transfer(), Some(&report("second")));
    assert_eq!(f.dawproject_checks().len(), 3);
    // The same choice again asks nothing.
    m.set_dawproject_region_gain(RegionGainRow::Apply);
    assert!(m.take_dawproject_check().is_none());

    // The core's refusal is the sheet's message.
    f.fail_next("export_dawproject_check", model("no such sequence"));
    m.refresh_dawproject();
    m.run_dawproject_check();
    assert!(m.dawproject().wont_transfer().is_none());
    assert_eq!(m.dawproject().error(), Some("no such sequence"));
}

/// Swift `defaultNameComesFromTheCore`.
#[test]
fn the_default_name_comes_from_the_core() {
    let f = fake();
    let mut m = shown(&f);
    assert_eq!(m.dawproject().default_name(), "Song.dawproject");
    assert_eq!(f.reads("dawproject_name"), 1);
    // Opening again asks again, and refreshes the check.
    m.hide_dawproject();
    m.show_dawproject();
    m.run_dawproject_check();
    assert_eq!(f.reads("dawproject_name"), 2);
    assert_eq!(
        f.dawproject_checks().len(),
        2,
        "opening refreshes the check"
    );
    // A refused name leaves it empty (Export… is off then).
    m.hide_dawproject();
    f.fail_next("dawproject_name", model("no"));
    m.show_dawproject();
    assert_eq!(m.dawproject().default_name(), "");
}

/// Swift `exportWaitsForTheRunningJob`.
#[test]
fn export_waits_for_the_running_job() {
    let f = fake();
    let mut m = shown(&f);
    f.script_job(JobScript::done().steps(&[0.4, 0.4]));
    let req = m.bounce_request(Some("/tmp/b.wav"), false).unwrap();
    m.try_start_job(JobKindRow::Bounce, |s| s.bounce(req))
        .unwrap();
    m.start_dawproject("/tmp/a.dawproject", true);
    assert!(m.dawproject().waiting() && exports(&f).is_empty());
    assert!(m.alert.is_none() && m.dawproject().job().is_none());
    m.poll_jobs();
    assert!(exports(&f).is_empty(), "still running");
    f.script_job(JobScript::done().steps(&[0.4]));
    m.poll_jobs();
    let r = exports(&f).pop().expect("the bounce ended: it starts");
    assert!(!m.dawproject().waiting() && m.dawproject_running());
    assert!(r.path == "/tmp/a.dawproject" && r.replace && r.seq == 3);
}

/// Swift `exportWaitsForAMediaJob` (the external edit is not a job the
/// port starts yet).
#[test]
fn export_waits_for_a_media_job() {
    for kind in [JobKindRow::Format, JobKindRow::Convert, JobKindRow::Relink] {
        let f = fake();
        let mut m = shown(&f);
        running(&f, &mut m, kind);
        m.start_dawproject("/tmp/a.dawproject", false);
        assert!(
            m.dawproject().waiting() && exports(&f).is_empty(),
            "{kind:?}"
        );
        assert!(m.alert.is_none());
        m.poll_jobs();
        assert_eq!(exports(&f).len(), 1, "{kind:?}");
        assert!(!m.dawproject().waiting());
        assert!(!exports(&f)[0].replace);
    }
}

/// Swift `replaceOnlyWhatWasThereAtConfirm`: the kept request keeps what
/// the panel confirmed, whatever is there when it starts.
#[test]
fn replace_only_what_was_there_at_confirm() {
    for taken in [false, true] {
        let f = fake();
        let mut m = shown(&f);
        running(&f, &mut m, JobKindRow::Bounce);
        m.start_dawproject("/tmp/a.dawproject", taken);
        assert!(m.dawproject().waiting() && exports(&f).is_empty());
        m.poll_jobs();
        let r = exports(&f).pop().unwrap();
        assert!(
            r.path == "/tmp/a.dawproject" && r.replace == taken,
            "taken: {taken}"
        );
    }
}

/// Swift `cancelWaitingSendsNothing`.
#[test]
fn cancel_waiting_sends_nothing() {
    let f = fake();
    let mut m = shown(&f);
    running(&f, &mut m, JobKindRow::ImportMidi);
    m.start_dawproject("/tmp/a.dawproject", false);
    assert!(m.dawproject().waiting());
    m.cancel_dawproject_waiting();
    assert!(!m.dawproject().waiting());
    m.poll_jobs();
    assert!(exports(&f).is_empty() && m.dawproject().job().is_none());
    assert!(!m.dawproject_running());
}

/// Swift `failedJobShowsItsMessage`.
#[test]
fn a_failed_job_shows_its_message() {
    let f = fake();
    let mut m = shown(&f);
    f.script_job(JobScript::failed("disk full").steps(&[0.5]));
    m.start_dawproject("/tmp/a.dawproject", false);
    let job = m.dawproject().job().unwrap();
    assert!(m.dawproject_running());
    assert!(
        m.jobs()
            .iter()
            .any(|j| j.id == job && j.kind == JobKindRow::ExportDawproject)
    );
    m.hide_dawproject();
    m.poll_jobs();
    assert_eq!(m.dawproject().error(), Some("disk full"));
    assert!(m.dawproject().shown(), "the sheet again, with the message");
    assert!(!m.report_shown() && m.last_report().is_none() && m.alert.is_none());
    assert!(!m.dawproject_running());

    // A cancelled one is quiet; a finished one shows its report.
    f.script_job(JobScript::done().steps(&[0.5]));
    m.start_dawproject("/tmp/b.dawproject", false);
    m.cancel_dawproject();
    m.poll_jobs();
    assert!(m.alert.is_none() && !m.report_shown() && !m.dawproject().shown());
    m.show_dawproject();
    m.run_dawproject_check();
    f.script_job(
        JobScript::done()
            .steps(&[0.5])
            .report(report("Exported 2 tracks"))
            .written(&["/tmp/c.dawproject"]),
    );
    m.start_dawproject("/tmp/c.dawproject", false);
    m.poll_jobs();
    assert!(m.report_shown());
    assert_eq!(m.last_report(), Some(&report("Exported 2 tracks")));
    assert_eq!(m.report_written(), ["/tmp/c.dawproject"]);
    assert!(!m.dawproject().shown() && m.alert.is_none());

    // Refused when it starts: the message in the sheet, no alert.
    m.show_dawproject();
    m.run_dawproject_check();
    f.fail_next("export_dawproject", model("a.dawproject exists"));
    m.start_dawproject("/tmp/a.dawproject", false);
    assert_eq!(m.dawproject().error(), Some("a.dawproject exists"));
    assert!(m.dawproject().shown() && m.alert.is_none() && !m.dawproject_running());

    // A check still in flight when the start is refused lands late: the
    // refusal stays.
    m.set_dawproject_region_gain(RegionGainRow::LeaveOut);
    let late = m.take_dawproject_check().unwrap();
    f.fail_next("export_dawproject", model("a.dawproject exists"));
    m.start_dawproject("/tmp/a.dawproject", false);
    m.land_dawproject_check(late.run(f.as_ref()));
    assert_eq!(m.dawproject().error(), Some("a.dawproject exists"));
    assert_eq!(failure_title(JobKindRow::ExportDawproject), EXPORT_FAILED);
}

/// Swift `requestCarriesTheBlockRule`.
#[test]
fn the_request_carries_the_block_rule() {
    let f = fake();
    let mut m = DocumentModel::new(f.clone()).unwrap();
    m.update_layout(|l| l.block_rule = BlockRuleSetting::FixedBars { bars: 2 });
    m.show_dawproject();
    m.run_dawproject_check();
    let rule = BlockRuleSetting::FixedBars { bars: 2 }.rule();
    assert_eq!(f.dawproject_checks().last().unwrap().blocks, rule);
    m.set_dawproject_region_gain(RegionGainRow::LeaveOut);
    m.run_dawproject_check();
    assert_eq!(
        f.dawproject_checks().last().unwrap().region_gain,
        RegionGainRow::LeaveOut
    );
    m.start_dawproject("/tmp/a.dawproject", true);
    assert_eq!(
        exports(&f).last(),
        Some(&DawprojectExportRequestRow {
            seq: 3,
            path: "/tmp/a.dawproject".into(),
            replace: true,
            region_gain: RegionGainRow::LeaveOut,
            blocks: rule,
        })
    );
}

/// Swift `noSequenceIsTheSheetsMessage`.
#[test]
fn no_sequence_is_the_sheets_message() {
    let f = Arc::new(FakeSession::new(vec![]));
    let mut m = DocumentModel::new(f.clone()).unwrap();
    m.show_dawproject();
    m.run_dawproject_check();
    assert_eq!(m.dawproject().error(), Some(NO_SEQUENCE));
    assert!(m.dawproject().wont_transfer().is_none());
    m.start_dawproject("/tmp/a.dawproject", false);
    assert!(m.dawproject().job().is_none());
    assert_eq!(m.dawproject().error(), Some(NO_SEQUENCE));
    // No string says lane.
    for s in [
        TITLE,
        SUBTITLE,
        REGION_GAIN,
        APPLY,
        LEAVE_OUT,
        WAITING_TEXT,
        EXPORT_FAILED,
    ] {
        assert!(!s.to_lowercase().contains("lane"), "{s}");
    }
}

/// Showing the sheet again while an export waits or runs keeps it (and
/// asks the core for nothing new).
#[test]
fn showing_again_keeps_a_waiting_or_running_export() {
    let f = fake();
    let mut m = shown(&f);
    running(&f, &mut m, JobKindRow::Bounce);
    m.set_dawproject_region_gain(RegionGainRow::LeaveOut);
    m.run_dawproject_check();
    m.start_dawproject("/tmp/a.dawproject", false);
    m.hide_dawproject();
    m.show_dawproject();
    assert!(m.dawproject().waiting(), "still waiting");
    assert_eq!(f.reads("dawproject_name"), 1, "not reset");
    f.script_job(JobScript::done().steps(&[0.5, 0.5]));
    m.poll_jobs();
    let job = m.dawproject().job();
    assert!(m.dawproject_running());
    m.hide_dawproject();
    m.show_dawproject();
    assert!(
        m.dawproject_running() && m.dawproject().job() == job,
        "keeps the job"
    );
    assert_eq!(
        exports(&f).pop().unwrap().region_gain,
        RegionGainRow::LeaveOut
    );
}
