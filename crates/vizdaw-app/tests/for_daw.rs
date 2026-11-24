//! The Export for Another DAW sheet's model (slice 7c Task 4; Swift
//! `ForDawModelTests`): the choices reset to the project's format and the
//! core's stems, every change asks the check again off the UI thread
//! (only the newest answer lands), an export waits for a job the core
//! cannot run it beside instead of failing, the sheet stays up while the
//! export runs and closes when it ends, a failure brings it back with its
//! message, never an alert.
//!
//! Swift's `aDropAndAKeptExportWaitingOnTheSameJobBothProceedInOrder` is
//! about dropped MIDI files, which this port does not hold back yet: its
//! place is taken by a DAWproject export and this one kept on the same
//! job.

mod support;

use std::sync::Arc;

use support::{FakeSession, mixer_fake};
use vizdaw_app::DocumentModel;
use vizdaw_app::audio_format::{AudioFormat, AudioFormatChoices};
use vizdaw_app::bounce::BounceChoices;
use vizdaw_app::for_daw::{
    ADD_STEMS, BLOCKING, CANCEL_EXPORT_LABEL, CANCEL_WAITING_LABEL, EXPORT_FAILED, EXPORT_LABEL,
    ForDawChoices, MEDIA_JOBS, NO_SEQUENCE, PROGRESS_LABEL, STEMS, SUBTITLE, TITLE, WAITING_TEXT,
    stem_label,
};
use vizdaw_app::jobs::{failure_title, job_text};
use vizdaw_app::testing::{JobRequest, JobScript};
use vizdaw_ffi::{
    BitDepthRow, ContainerRow, DawprojectExportRequestRow, ForDawCheckRow, ForDawRequestRow,
    JobKindRow, MidiImportRequestRow, ProjectFormatRow, ProjectInfoRow, ReportRow,
    SmfExportRequestRow, VizError,
};

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

fn report(summary: &str) -> ReportRow {
    ReportRow {
        summary: summary.into(),
        items: vec![],
    }
}

fn check_row(summary: &str, strips: &[u64], frames: u64) -> ForDawCheckRow {
    ForDawCheckRow {
        report: report(summary),
        folder: "Song for another DAW".into(),
        files: vec![],
        strips: strips.to_vec(),
        start: "Every file starts at bar 1.".into(),
        frames,
        warning: None,
    }
}

/// Swift's fake: 44.1 kHz 16-bit, a check whose default stems are [2].
fn fake() -> Arc<FakeSession> {
    let f = mixer_fake();
    f.set_project_info(info(44_100, BitDepthRow::Int16));
    f.set_for_daw_check(check_row("Exported 2 tracks", &[2], 44_100));
    Arc::new(f)
}

/// The sheet shown with its first check landed.
fn shown(f: &Arc<FakeSession>) -> DocumentModel {
    let mut m = DocumentModel::new(f.clone()).unwrap();
    m.show_for_daw();
    m.run_for_daw_check();
    m
}

fn exports(f: &FakeSession) -> Vec<ForDawRequestRow> {
    f.jobs_started()
        .into_iter()
        .filter_map(|j| match j.request {
            JobRequest::ForDaw(r) => Some(r),
            _ => None,
        })
        .collect()
}

fn model(message: &str) -> VizError {
    VizError::Model {
        message: message.into(),
    }
}

/// A job of `kind` started by the UI that runs for `polls` polls after
/// its start.
fn running(f: &FakeSession, m: &mut DocumentModel, kind: JobKindRow, polls: usize) {
    f.script_job(JobScript::done().steps(&vec![0.4; polls + 1]));
    let seq = m.selected_seq().unwrap_or(3);
    let started = match kind {
        JobKindRow::Bounce => {
            let req = m.bounce_request(Some("/tmp/b.wav"), false).unwrap();
            m.try_start_job(kind, |s| s.bounce(req))
        }
        JobKindRow::ImportMidi => m.try_start_job(kind, |s| {
            s.import_midi(MidiImportRequestRow {
                path: "/tmp/a.mid".into(),
                new_project: false,
                seq,
                lane: None,
                tick: 0,
                channel_map: vec![],
                split_channels: None,
                mpe: false,
                use_tempo: false,
            })
        }),
        JobKindRow::ExportMidi => m.try_start_job(kind, |s| {
            s.export_midi(SmfExportRequestRow {
                seq,
                lanes: vec![],
                path: "/tmp/a.mid".into(),
                replace: false,
                type0: false,
                ppq: 960,
                mpe: false,
            })
        }),
        JobKindRow::ExportProject => {
            let req = m.for_daw_request("/tmp/other").unwrap();
            m.try_start_job(kind, |s| s.export_for_daw(req))
        }
        JobKindRow::ExportDawproject => m.try_start_job(kind, |s| {
            s.export_dawproject(DawprojectExportRequestRow {
                seq,
                path: "/tmp/a.dawproject".into(),
                replace: false,
                region_gain: vizdaw_ffi::RegionGainRow::Apply,
                blocks: vizdaw_app::editor::BlockRuleSetting::default().rule(),
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

/// Swift `showResetsToTheCoresDefaults`.
#[test]
fn show_resets_to_the_cores_defaults() {
    let f = fake();
    let mut m = shown(&f);
    let d = m.for_daw();
    let c = d.choices();
    assert!(d.shown());
    assert_eq!(c.format.container, ContainerRow::Wav);
    assert!(c.format.rate == 44_100 && !c.format.other_rate);
    assert_eq!(c.format.depth, BitDepthRow::Int16);
    assert!(c.format.dither && !c.add_to_project, "dither on at 16 bits");
    assert_eq!(c.stems.iter().copied().collect::<Vec<_>>(), [2]);
    assert_eq!(d.check().unwrap().strips, [2]);
    assert_eq!(
        d.strips().iter().map(|s| s.id).collect::<Vec<_>>(),
        [1, 2],
        "every strip but the master"
    );
    let checks = f.for_daw_checks();
    assert_eq!(checks.len(), 1, "no second check for the stems");
    assert_eq!(
        checks[0].strips, None,
        "the first check asks for the defaults"
    );

    m.hide_for_daw();
    f.set_project_info(info(50_000, BitDepthRow::Float32));
    m.edit_for_daw(|c| c.format.depth = BitDepthRow::Int24);
    m.show_for_daw();
    m.run_for_daw_check();
    let c = m.for_daw().choices();
    assert!(c.format.depth == BitDepthRow::Float32 && c.format.rate == 50_000);
    assert!(c.format.other_rate && !c.format.dither);
}

/// Swift `everyChangeRefreshesTheCheckOffTheMainActor`.
#[test]
fn every_change_refreshes_the_check_off_the_ui_thread() {
    let f = fake();
    let mut m = shown(&f);
    let mut n = f.for_daw_checks().len();
    let mut next = |m: &mut DocumentModel, what: &str, change: &dyn Fn(&mut ForDawChoices)| {
        m.edit_for_daw(change);
        assert_eq!(
            f.for_daw_checks().len(),
            n,
            "{what}: not asked on this thread"
        );
        m.run_for_daw_check();
        let checks = f.for_daw_checks();
        assert_eq!(checks.len(), n + 1, "{what} checks once");
        let sent: Vec<u64> = m.for_daw().choices().stems.iter().copied().collect();
        assert_eq!(checks.last().unwrap().strips.as_deref(), Some(&sent[..]));
        n = checks.len();
    };
    next(&mut m, "stems", &|c| {
        c.stems = [1, 2].into();
    });
    next(&mut m, "container", &|c| {
        c.format.container = ContainerRow::Aiff
    });
    next(&mut m, "depth", &|c| c.format.depth = BitDepthRow::Int24);
    next(&mut m, "rate", &|c| c.format.rate = 96_000);
    next(&mut m, "add", &|c| c.add_to_project = true);
    let last = f.for_daw_checks().pop().unwrap();
    assert!(last.add_to_project && last.rate == 96_000);
    assert!(last.container == ContainerRow::Aiff && last.depth == BitDepthRow::Int24);
    // No change asks nothing.
    m.edit_for_daw(|c| c.add_to_project = true);
    assert!(m.take_for_daw_check().is_none());

    // Two changes in a row: the first one's late answer is dropped.
    m.edit_for_daw(|c| c.format.rate = 88_200);
    let first = m.take_for_daw_check().unwrap();
    m.edit_for_daw(|c| c.format.rate = 192_000);
    f.set_for_daw_check(check_row("rate 192000", &[2], 0));
    m.run_for_daw_check();
    assert_eq!(m.for_daw().check().unwrap().report.summary, "rate 192000");
    f.set_for_daw_check(check_row("rate 88200", &[2], 0));
    let late = first.run(f.as_ref());
    m.land_for_daw_check(late);
    assert_eq!(
        m.for_daw().check().unwrap().report.summary,
        "rate 192000",
        "a stale result is dropped"
    );
}

/// Swift `startSendsTheChoices`.
#[test]
fn start_sends_the_choices() {
    let f = fake();
    f.script_job(JobScript::done().steps(&[0.3]));
    let mut m = shown(&f);
    m.edit_for_daw(|c| {
        c.format.depth = BitDepthRow::Float32;
        c.stems = [2].into();
    });
    m.run_for_daw_check();
    m.start_for_daw("/tmp/x");
    let r = exports(&f).pop().expect("an export");
    assert_eq!(r.path, "/tmp/x");
    assert_eq!(r.strips.as_deref(), Some(&[2][..]));
    assert!(!r.dither, "never at 32-bit float");
    assert_eq!(r.depth, BitDepthRow::Float32);
    assert!(r.seq == 3 && r.rate == 44_100 && !r.add_to_project);
    let d = m.for_daw();
    assert!(d.job().is_some() && m.for_daw_running() && !d.waiting());
}

/// Swift `exportWaitsForTheRunningJob`.
#[test]
fn export_waits_for_the_running_job() {
    let f = fake();
    let mut m = shown(&f);
    running(&f, &mut m, JobKindRow::ImportMidi, 1);
    f.script_job(JobScript::done().steps(&[0.3]));
    m.start_for_daw("/tmp/x");
    assert!(exports(&f).is_empty() && m.for_daw().waiting() && m.alert.is_none());
    m.poll_jobs();
    assert!(exports(&f).is_empty(), "still running");
    m.poll_jobs();
    assert_eq!(exports(&f).len(), 1, "the import ended: it starts");
    assert!(!m.for_daw().waiting() && m.for_daw_running());
    m.poll_jobs();
    assert!(!m.for_daw_running());

    // Cancelled before the job ends: nothing starts.
    m.show_for_daw();
    m.run_for_daw_check();
    running(&f, &mut m, JobKindRow::ImportMidi, 0);
    m.start_for_daw("/tmp/y");
    assert!(m.for_daw().waiting());
    m.cancel_for_daw_waiting();
    assert!(!m.for_daw().waiting());
    m.poll_jobs();
    assert_eq!(exports(&f).len(), 1);
}

/// Swift `addWaitsForMediaJobs`.
#[test]
fn add_waits_for_media_jobs() {
    let f = fake();
    let mut m = shown(&f);
    running(&f, &mut m, JobKindRow::Convert, 3);
    m.start_for_daw("/tmp/x");
    assert_eq!(exports(&f).len(), 1, "without Add it starts at once");
    assert!(!m.for_daw().waiting());

    m.show_for_daw();
    m.run_for_daw_check();
    m.edit_for_daw(|c| c.add_to_project = true);
    m.run_for_daw_check();
    m.start_for_daw("/tmp/x");
    assert!(exports(&f).len() == 1 && m.for_daw().waiting());
    m.poll_jobs();
    m.poll_jobs();
    assert!(
        exports(&f).len() == 1 && m.for_daw().waiting(),
        "still converting"
    );
    m.poll_jobs();
    assert_eq!(exports(&f).len(), 2, "the convert ended");
    assert!(!m.for_daw().waiting() && exports(&f)[1].add_to_project);
    assert_eq!(MEDIA_JOBS.len(), 4);
}

/// Swift `aFinishedExportShowsTheReportWithTheFolder`.
#[test]
fn a_finished_export_shows_the_report_with_the_folder() {
    let f = fake();
    f.script_job(
        JobScript::done()
            .steps(&[0.5])
            .report(report("Exported 2 tracks"))
            .written(&["/tmp/x/Song for another DAW"]),
    );
    let mut m = shown(&f);
    m.start_for_daw("/tmp/x");
    assert!(m.for_daw().shown() && m.for_daw_running());
    m.poll_jobs();
    assert_eq!(m.last_report(), Some(&report("Exported 2 tracks")));
    assert!(m.report_shown());
    assert_eq!(m.report_written(), ["/tmp/x/Song for another DAW"]);
    assert!(m.jobs().is_empty() && !m.for_daw_running());
    assert!(!m.for_daw().shown(), "the sheet closes");
}

/// Swift `aFailedExportReopensTheSheet`.
#[test]
fn a_failed_export_reopens_the_sheet() {
    let f = fake();
    f.script_job(JobScript::failed("The disk is full.").steps(&[0.5]));
    let mut m = shown(&f);
    m.edit_for_daw(|c| c.format.rate = 96_000);
    m.run_for_daw_check();
    m.start_for_daw("/tmp/x");
    m.hide_for_daw();
    m.poll_jobs();
    let d = m.for_daw();
    assert!(d.shown() && d.error() == Some("The disk is full."));
    assert!(m.alert.is_none(), "not an alert");
    assert_eq!(
        d.choices().format.rate,
        96_000,
        "reopened with the choices kept"
    );

    // Cancelled: quiet, and the sheet goes.
    f.script_job(JobScript::done().steps(&[0.5, 0.5]));
    m.start_for_daw("/tmp/x");
    m.cancel_for_daw();
    m.poll_jobs();
    assert!(!m.for_daw().shown() && m.alert.is_none());

    // Refused at the start: the message in the sheet, no alert.
    m.show_for_daw();
    m.run_for_daw_check();
    m.hide_for_daw();
    f.fail_next("export_for_daw", model("Choose where to export."));
    m.start_for_daw("/tmp/x");
    let d = m.for_daw();
    assert!(d.shown() && d.error() == Some("Choose where to export."));
    assert!(m.alert.is_none() && !m.for_daw_running());

    // Failed at its first poll: still the sheet's message.
    f.script_job(JobScript::failed("No room."));
    m.start_for_daw("/tmp/x");
    assert_eq!(m.for_daw().error(), Some("No room."));
    assert!(m.alert.is_none());
}

/// A check in flight when the export fails lands late: the message stays.
#[test]
fn a_late_check_does_not_erase_the_failure() {
    let f = fake();
    let mut m = shown(&f);
    m.edit_for_daw(|c| c.format.rate = 96_000);
    let late = m.take_for_daw_check().unwrap();
    f.fail_next("export_for_daw", model("Busy elsewhere."));
    m.start_for_daw("/tmp/x");
    m.land_for_daw_check(late.run(f.as_ref()));
    assert_eq!(m.for_daw().error(), Some("Busy elsewhere."));
}

/// Swift `noStringSaysLane`, with every string of the sheet.
#[test]
fn no_string_says_lane() {
    let f = fake();
    f.script_job(JobScript::failed(
        "Choose at least one strip to add its stem to the project.",
    ));
    let mut m = shown(&f);
    let mut strings: Vec<String> = [
        TITLE,
        SUBTITLE,
        STEMS,
        ADD_STEMS,
        WAITING_TEXT,
        PROGRESS_LABEL,
        CANCEL_WAITING_LABEL,
        CANCEL_EXPORT_LABEL,
        EXPORT_LABEL,
        EXPORT_FAILED,
        job_text(JobKindRow::ExportProject),
        failure_title(JobKindRow::ExportProject),
    ]
    .map(str::to_owned)
    .to_vec();
    strings.push(stem_label("Bass"));
    let mut none = DocumentModel::new(Arc::new(FakeSession::new(vec![]))).unwrap();
    none.show_for_daw();
    assert_eq!(none.for_daw().error(), Some(NO_SEQUENCE));
    strings.push(none.for_daw().error().unwrap().to_owned());
    m.start_for_daw("/tmp/x");
    strings.push(m.for_daw().error().unwrap().to_owned());
    f.fail_next("export_for_daw_check", model("No track is shown."));
    m.edit_for_daw(|c| {
        c.stems = [1].into();
    });
    m.run_for_daw_check();
    strings.push(m.for_daw().error().unwrap().to_owned());
    assert_eq!(failure_title(JobKindRow::ExportProject), EXPORT_FAILED);
    assert!(strings.iter().all(|s| !s.to_lowercase().contains("lane")));
}

/// Swift `eachBlockingKindBlocksOnItsOwn`.
#[test]
fn each_blocking_kind_blocks_on_its_own() {
    for kind in [
        JobKindRow::Bounce,
        JobKindRow::ExportMidi,
        JobKindRow::ExportProject,
        JobKindRow::ExportDawproject,
        JobKindRow::ImportMidi,
    ] {
        assert!(BLOCKING.contains(&kind));
        let f = fake();
        let mut m = shown(&f);
        running(&f, &mut m, kind, 0);
        let before = exports(&f).len();
        m.start_for_daw("/tmp/x");
        assert!(
            exports(&f).len() == before && m.for_daw().waiting(),
            "{kind:?} blocks"
        );
        m.poll_jobs();
        assert!(
            exports(&f).len() == before + 1 && !m.for_daw().waiting(),
            "{kind:?} ends: it starts"
        );
    }
}

/// Swift `twoBlockersEndingInOnePollStartTheExportOnce`.
#[test]
fn two_blockers_ending_in_one_poll_start_the_export_once() {
    let f = fake();
    let mut m = shown(&f);
    running(&f, &mut m, JobKindRow::Bounce, 0);
    running(&f, &mut m, JobKindRow::ImportMidi, 0);
    m.start_for_daw("/tmp/x");
    m.poll_jobs();
    assert_eq!(exports(&f).len(), 1);
    assert!(m.alert.is_none());
}

/// Swift `anUnknownJobCountsAsEnded`.
#[test]
fn an_unknown_job_counts_as_ended() {
    let f = fake();
    let mut m = shown(&f);
    // Listed as a bounce the session is busy answering about at first,
    // then no longer knows.
    f.fail_next(
        "job",
        VizError::Busy {
            message: "busy".into(),
        },
    );
    m.try_start_job(JobKindRow::Bounce, |_| Ok(77)).unwrap();
    assert_eq!(m.jobs().len(), 1);
    m.start_for_daw("/tmp/x");
    assert!(m.for_daw().waiting());
    m.poll_jobs();
    assert_eq!(exports(&f).len(), 1);
    assert!(!m.for_daw().waiting());
}

/// In place of Swift's dropped-files test: a DAWproject export and this
/// one kept on the same job go in order, the second waiting for the
/// first.
#[test]
fn two_kept_exports_waiting_on_the_same_job_proceed_in_order() {
    let f = fake();
    let mut m = shown(&f);
    running(&f, &mut m, JobKindRow::Bounce, 0);
    m.start_for_daw("/tmp/x");
    m.show_dawproject();
    m.run_dawproject_check();
    m.start_dawproject("/tmp/a.dawproject", false);
    assert!(m.for_daw().waiting() && m.dawproject().waiting());
    f.script_job(JobScript::done().steps(&[0.5]));
    m.poll_jobs();
    assert_eq!(exports(&f).len(), 1, "the first kept export starts");
    assert!(m.dawproject().waiting(), "the other waits for it");
    m.poll_jobs();
    assert!(!m.dawproject().waiting() && m.dawproject().job().is_some());
}

/// Swift `showingAgainKeepsAWaitingExport`.
#[test]
fn showing_again_keeps_a_waiting_export() {
    let f = fake();
    let mut m = shown(&f);
    running(&f, &mut m, JobKindRow::Bounce, 0);
    m.edit_for_daw(|c| c.format.rate = 96_000);
    m.run_for_daw_check();
    m.start_for_daw("/tmp/x");
    m.hide_for_daw();
    m.show_for_daw();
    m.run_for_daw_check();
    assert!(m.for_daw().waiting());
    assert_eq!(m.for_daw().choices().format.rate, 96_000);
    m.poll_jobs();
    assert_eq!(exports(&f).last().unwrap().rate, 96_000);
}

/// Swift `theSheetStaysUpWhileTheExportRuns`.
#[test]
fn the_sheet_stays_up_while_the_export_runs() {
    let f = fake();
    f.script_job(JobScript::done().steps(&[0.3, 0.6]));
    let mut m = shown(&f);
    m.start_for_daw("/tmp/x");
    assert!(
        m.for_daw().shown() && m.for_daw_running(),
        "progress and Cancel"
    );
    assert_eq!(m.for_daw_progress(), Some(0.3));
    let job = m.for_daw().job();
    m.hide_for_daw();
    m.show_for_daw();
    assert!(
        m.for_daw_running() && m.for_daw().job() == job,
        "showing again keeps the job"
    );
    m.start_for_daw("/tmp/y");
    assert_eq!(exports(&f).len(), 1, "no second export from the sheet");
    assert!(m.for_daw().job() == job && !m.for_daw().waiting());
    m.poll_jobs();
    m.poll_jobs();
    assert!(!m.for_daw().shown(), "closes on success");
}

/// Swift `cancellingTheRunningExportClosesTheSheet`.
#[test]
fn cancelling_the_running_export_closes_the_sheet() {
    let f = fake();
    f.script_job(JobScript::done().steps(&[0.3, 0.6]));
    let mut m = shown(&f);
    m.start_for_daw("/tmp/x");
    m.cancel_for_daw();
    assert_eq!(f.cancelled_jobs(), [m.for_daw().job().unwrap()]);
    m.poll_jobs();
    assert!(!m.for_daw().shown() && m.alert.is_none());
}

/// Swift `theLengthUsesTheRateTheCheckWasMadeAt`.
#[test]
fn the_length_uses_the_rate_the_check_was_made_at() {
    let f = fake();
    let mut m = shown(&f);
    assert_eq!(m.for_daw().length(), "0:01.0 + tail");
    m.edit_for_daw(|c| c.format.rate = 88_200);
    assert_eq!(
        m.for_daw().length(),
        "0:01.0 + tail",
        "the old check is still at the old rate"
    );
    m.run_for_daw_check();
    assert_eq!(m.for_daw().length(), "0:00.5 + tail");
}

/// Swift `noSequenceDropsAnOlderCheck`.
#[test]
fn no_sequence_drops_an_older_check() {
    let f = fake();
    let mut m = shown(&f);
    m.edit_for_daw(|c| c.format.rate = 96_000);
    let older = m.take_for_daw_check().unwrap();
    m.select(None);
    m.refresh_for_daw();
    m.land_for_daw_check(older.run(f.as_ref()));
    assert!(m.for_daw().check().is_none());
    assert_eq!(m.for_daw().error(), Some(NO_SEQUENCE));
    m.start_for_daw("/tmp/x");
    assert!(exports(&f).is_empty() && m.for_daw().error() == Some(NO_SEQUENCE));
}

/// Swift `lengthIsTheStemsTimeAndTheTail`.
#[test]
fn length_is_the_stems_time_and_the_tail() {
    let f = fake();
    let mut m = shown(&f);
    assert_eq!(m.for_daw().length(), "0:01.0 + tail");
    m.edit_for_daw(|c| c.format.rate = 88_200);
    m.run_for_daw_check();
    assert_eq!(m.for_daw().length(), "0:00.5 + tail");
    m.select(None);
    m.refresh_for_daw();
    m.run_for_daw_check();
    assert_eq!(m.for_daw().length(), "", "no check, no length");
}

/// Swift `bothModelsOfferTheSameFormatChoices`: Bounce and For Another
/// DAW share the format rows.
#[test]
fn both_models_offer_the_same_format_choices() {
    fn choose(c: &mut dyn AudioFormatChoices) {
        c.edit_audio_format(&|f: &mut AudioFormat| {
            f.container = ContainerRow::Caf;
            f.depth = BitDepthRow::Float32;
            f.dither = true;
            f.set_rate_choice(0);
            f.set_other_rate(1_000_000);
        });
    }
    let mut bounce = BounceChoices::default();
    let mut for_daw = ForDawChoices::default();
    let all: [&mut dyn AudioFormatChoices; 2] = [&mut bounce, &mut for_daw];
    for c in all {
        choose(c);
        let f = c.audio_format();
        assert!(f.container == ContainerRow::Caf && f.depth == BitDepthRow::Float32 && f.dither);
        assert!(f.other_rate && f.rate_choice() == 0 && f.rate == 384_000);
        assert!(!f.sent_dither(), "never at 32-bit float");
    }
    assert_eq!(
        bounce.container,
        ContainerRow::Caf,
        "the bounce's own fields"
    );
    assert_eq!(bounce.rate, 384_000);
}

/// The folder for another DAW is never written over: the request names
/// only the folder it goes in (the core numbers "<Song> for another DAW"
/// when taken), so no replace question is asked.
#[test]
fn the_export_goes_into_the_chosen_folder_without_replacing() {
    let f = fake();
    let mut m = shown(&f);
    m.start_for_daw("/tmp/x");
    let r = exports(&f).pop().unwrap();
    assert_eq!(r.path, "/tmp/x");
    assert_eq!(m.for_daw().check().unwrap().folder, "Song for another DAW");
}
