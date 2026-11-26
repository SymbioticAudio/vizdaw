//! The Merge DAWproject sheet's model (slice 7c Task 6; Swift
//! `MergeModelTests`, all 18): the Hold job watched through the
//! document's job polling, the snapshot list read off the UI thread
//! (only the newest lands), Merge and Cancel, and a held merge that
//! queues what the core would refuse beside it (the three exports) and
//! releases it when the hold ends. Also the held-merge blocks of
//! Convert and the rate change, and the store's return-prompt Merge.

mod support;

use std::path::Path;
use std::sync::Arc;

use support::{FakeSession, mixer_fake};
use vizdaw_app::merge::{ANOTHER_WAITING, CANCELLED, HELD_HELP};
use vizdaw_app::store::{MergeFile, Opened, ReturnChoice, ReturnNext};
use vizdaw_app::testing::{FakeCore, JobRequest, JobScript};
use vizdaw_app::{BannerAction, DocSession, DocumentModel, DocumentStore, ProjectBanner};
use vizdaw_ffi::{
    BitDepthRow, DawprojectInfoRow, DawprojectMergeRequestRow, ForDawCheckRow, JobKindRow,
    MergePreviewRow, MergeStepRow, ProjectFormatRow, ProjectInfoRow, RecoveryInfo, ReportItemRow,
    ReportKindRow, ReportRow, SnapshotRow, VizError,
};

const PID: &str = "0f8b6f8e-3c1a-4d2e-9b7a-5e6f7a8b9c0d";
const EID: &str = "1a2b3c4d-5e6f-4a7b-8c9d-0e1f2a3b4c5d";
const OTHER: &str = "2b3c4d5e-6f7a-4b8c-9d0e-1f2a3b4c5d6e";
const PATH: &str = "/tmp/back.dawproject";
const SECOND: &str = "/tmp/second.dawproject";

fn info(export: Option<&str>) -> DawprojectInfoRow {
    DawprojectInfoRow {
        sample_rate: 48_000,
        tracks: 2,
        audio_files: 0,
        from_vizdaw: true,
        export_id: export.map(str::to_owned),
        project_id: Some(PID.into()),
    }
}

fn with_export() -> DawprojectInfoRow {
    info(Some(EID))
}

fn report(summary: &str) -> ReportRow {
    ReportRow {
        summary: summary.into(),
        items: vec![],
    }
}

fn preview_row() -> MergePreviewRow {
    MergePreviewRow {
        applied: 3,
        kept: 1,
        added: 1,
        removed: 0,
        conflicts: 2,
        conflict_lines: vec![
            "Drums: level changed in both".into(),
            "Keys: notes changed in both".into(),
        ],
        summary: "Merged: 3 applied, 1 kept, 1 added, 0 removed, 2 in conflict".into(),
        export_id: EID.into(),
        exported_at: 1_800_000_000,
    }
}

fn fake() -> Arc<FakeSession> {
    let f = mixer_fake();
    f.set_path(Some("/tmp/Song.vizdaw"));
    f.set_project_id(Some(PID));
    f.set_project_info(ProjectInfoRow {
        format: ProjectFormatRow {
            sample_rate: 44_100,
            depth: BitDepthRow::Int16,
        },
        mismatched: 0,
        rate_mismatch: false,
        device_rate: 0.0,
    });
    f.set_for_daw_check(ForDawCheckRow {
        report: report("check"),
        folder: "Song for another DAW".into(),
        files: vec![],
        strips: vec![],
        start: "Every file starts at bar 1.".into(),
        frames: 44_100,
        warning: None,
    });
    f.set_check_report("export_dawproject_check", report("check"));
    f.set_check_report("export_aaf_check", report("check"));
    Arc::new(f)
}

/// The next merge job runs one poll, then ends done with the preview.
fn script_hold(f: &FakeSession) {
    f.script_job(JobScript::done().merge(preview_row()).steps(&[0.3]));
}

fn merges(f: &FakeSession) -> Vec<DawprojectMergeRequestRow> {
    f.jobs_started()
        .into_iter()
        .filter_map(|j| match j.request {
            JobRequest::MergeDawproject(r) => Some(r),
            _ => None,
        })
        .collect()
}

fn calls(f: &FakeSession, name: &str) -> Vec<String> {
    f.calls()
        .into_iter()
        .filter(|c| c.starts_with(name))
        .collect()
}

fn hold(path: &str, snapshot: Option<&str>) -> DawprojectMergeRequestRow {
    DawprojectMergeRequestRow {
        path: path.into(),
        snapshot: snapshot.map(str::to_owned),
        step: MergeStepRow::Hold,
        date: vizdaw_app::interchange_commands::merge_date_today(),
    }
}

/// Begun and held with the preview.
fn held(f: &Arc<FakeSession>) -> (DocumentModel, u64) {
    let mut m = DocumentModel::new(f.clone()).unwrap();
    script_hold(f);
    m.begin_merge(PATH, with_export());
    let job = m.merge().job().unwrap();
    m.poll_jobs();
    assert!(m.merge().held());
    (m, job)
}

fn started_kinds(f: &FakeSession) -> Vec<JobKindRow> {
    f.jobs_started().into_iter().map(|j| j.kind).collect()
}

/// Swift `previewArrivesFromTheHeldJob`.
#[test]
fn the_preview_arrives_from_the_held_job() {
    let f = fake();
    let mut m = DocumentModel::new(f.clone()).unwrap();
    script_hold(&f);
    m.begin_merge(PATH, with_export());
    let g = m.merge();
    assert!(g.shown() && g.path() == PATH && g.preview().is_none() && !g.can_merge());
    assert!(m.merge_running());
    assert_eq!(merges(&f), [hold(PATH, None)]);
    assert_eq!(
        f.reads("dawproject_snapshots"),
        0,
        "no list with an export ID"
    );
    m.poll_jobs();
    let g = m.merge();
    assert_eq!(g.preview(), Some(&preview_row()));
    assert!(!m.merge_running() && g.held() && g.can_merge() && g.error().is_none());
    assert!(g.shown() && m.alert.is_none() && m.merge_held());
}

/// Swift `aLostExportIDListsSnapshotsOffTheMainActor`.
#[test]
fn a_lost_export_id_lists_snapshots_off_the_ui_thread() {
    let f = fake();
    let rows = vec![
        SnapshotRow {
            export_id: OTHER.into(),
            exported_at: 1_800_000_500,
            tracks: 3,
        },
        SnapshotRow {
            export_id: EID.into(),
            exported_at: 1_800_000_000,
            tracks: 1,
        },
    ];
    f.set_snapshots(rows.clone());
    let mut m = DocumentModel::new(f.clone()).unwrap();
    m.begin_merge(PATH, info(None));
    assert_eq!(f.reads("dawproject_snapshots"), 0, "begin reads nothing");
    let read = m.take_merge_snapshots().unwrap();
    assert!(m.take_merge_snapshots().is_none());
    let listed = read.run(f.as_ref());
    m.land_merge_snapshots(listed);
    assert_eq!(m.merge().snapshots(), rows.as_slice());
    assert!(m.merge().shown() && m.merge().choosing());
    assert!(merges(&f).is_empty() && m.merge().job().is_none());
    // Another file while the choice waits: it waits too.
    m.begin_merge(SECOND, with_export());
    m.run_merge_snapshots();
    assert_eq!(m.merge().path(), PATH);
    assert_eq!(f.reads("dawproject_snapshots"), 1);
    m.choose_merge_snapshot(EID);
    assert_eq!(merges(&f), [hold(PATH, Some(EID))]);
    assert!(!m.merge().choosing());
}

/// Swift `noSnapshotsShowsTheSentence`.
#[test]
fn no_snapshots_shows_the_sentence() {
    let f = fake();
    let mut m = DocumentModel::new(f.clone()).unwrap();
    let sentence = vizdaw_ffi::dawproject::NOT_FROM_HERE;
    f.script_job(JobScript::failed(sentence).steps(&[0.2]));
    m.begin_merge(PATH, info(None));
    m.run_merge_snapshots();
    assert!(m.merge().snapshots().is_empty());
    assert_eq!(merges(&f), [hold(PATH, None)]);
    m.poll_jobs();
    let g = m.merge();
    assert_eq!(g.error(), Some(sentence));
    assert!(!g.can_merge() && !g.held() && g.shown());
    assert!(m.alert.is_none(), "the sheet shows it, not an alert");
}

/// Swift `confirmShowsTheReport`.
#[test]
fn confirm_shows_the_report() {
    let f = fake();
    let (mut m, job) = held(&f);
    let r = ReportRow {
        summary: "Merged: 3 applied, 1 kept, 1 added, 0 removed, 2 in conflict".into(),
        items: vec![ReportItemRow {
            kind: ReportKindRow::Conflict,
            what: "Drums: level changed in both".into(),
            count: 1,
            details: vec![],
        }],
    };
    f.set_merge_report(r.clone());
    m.confirm_merge();
    assert_eq!(
        calls(&f, "confirm_merge"),
        [format!("confirm_merge({job})")]
    );
    assert_eq!(m.last_report(), Some(&r));
    assert!(m.report_shown() && m.report_written().is_empty());
    assert!(!m.merge().shown() && !m.merge().held() && !m.merge_held() && m.alert.is_none());
    // Runs once: a second press sends nothing.
    m.confirm_merge();
    m.cancel_merge();
    assert_eq!(calls(&f, "confirm_merge").len(), 1);
    assert!(calls(&f, "cancel_merge").is_empty());
}

/// Swift `aRefusedConfirmShowsTheSentence` (Review Focus 3).
#[test]
fn a_refused_confirm_shows_the_sentence() {
    let f = fake();
    let (mut m, job) = held(&f);
    let sentence = "The project changed after the preview; preview the merge again.";
    f.fail_next(
        "confirm_merge",
        VizError::Model {
            message: sentence.into(),
        },
    );
    m.confirm_merge();
    assert!(
        calls(&f, "confirm_merge").is_empty(),
        "refused: not applied"
    );
    let _ = job;
    let g = m.merge();
    assert!(g.error() == Some(sentence) && g.shown() && !m.report_shown());
    assert!(
        !g.can_merge() && !m.merge_held(),
        "the core dropped the hold"
    );
    // Busy keeps the hold (recording, an open edit): Merge stays.
    let f2 = fake();
    let (mut m2, _) = held(&f2);
    f2.fail_next(
        "confirm_merge",
        VizError::Busy {
            message: "Stop recording, then merge.".into(),
        },
    );
    m2.confirm_merge();
    let g2 = m2.merge();
    assert!(g2.error() == Some("Stop recording, then merge.") && g2.shown() && g2.can_merge());
    assert!(m2.merge_held() && m2.alert.is_none());
}

/// Swift `cancelSendsCancelMerge`.
#[test]
fn cancel_sends_cancel_merge() {
    let f = fake();
    let (mut m, job) = held(&f);
    m.cancel_merge();
    assert_eq!(calls(&f, "cancel_merge"), [format!("cancel_merge({job})")]);
    assert!(f.cancelled_jobs().is_empty());
    assert!(!m.merge().shown() && !m.merge().held() && m.merge().job().is_none());
    assert!(!m.merge_held());
    m.cancel_merge();
    assert_eq!(calls(&f, "cancel_merge").len(), 1, "once");

    // Running: the job is cancelled, and no hold is asked to go.
    let f2 = fake();
    let mut m2 = DocumentModel::new(f2.clone()).unwrap();
    f2.script_job(JobScript::done().steps(&[0.1, 0.2]));
    m2.begin_merge(PATH, with_export());
    let running = m2.merge().job().unwrap();
    m2.cancel_merge();
    assert_eq!(f2.cancelled_jobs(), [running]);
    assert!(calls(&f2, "cancel_merge").is_empty() && !m2.merge().shown());
    m2.poll_jobs();
    assert!(m2.alert.is_none() && !m2.merge().shown(), "ends quietly");

    // A job that finished as Cancel was pressed: its hold is let go.
    let f3 = fake();
    let mut m3 = DocumentModel::new(f3.clone()).unwrap();
    script_hold(&f3);
    m3.begin_merge(PATH, with_export());
    let late = m3.merge().job().unwrap();
    // The core finished it just as Cancel was pressed.
    f3.job(late).unwrap();
    m3.cancel_merge();
    assert_eq!(f3.cancelled_jobs(), [late]);
    m3.poll_jobs();
    assert_eq!(
        calls(&f3, "cancel_merge"),
        [format!("cancel_merge({late})")]
    );
    assert!(!m3.merge().shown() && m3.merge().preview().is_none());

    // While the snapshot list is read: nothing was sent, nothing lands.
    let f4 = fake();
    f4.set_snapshots(vec![SnapshotRow {
        export_id: EID.into(),
        exported_at: 1,
        tracks: 1,
    }]);
    let mut m4 = DocumentModel::new(f4.clone()).unwrap();
    m4.begin_merge(PATH, info(None));
    let read = m4.take_merge_snapshots().unwrap();
    m4.cancel_merge();
    m4.land_merge_snapshots(read.run(f4.as_ref()));
    assert!(m4.merge().snapshots().is_empty() && !m4.merge().shown());
    assert!(merges(&f4).is_empty() && f4.cancelled_jobs().is_empty());
    // A listing for a merge that went never lands on the next one.
    let stale = {
        m4.begin_merge(PATH, info(None));
        let r = m4.take_merge_snapshots().unwrap();
        m4.cancel_merge();
        r.run(f4.as_ref())
    };
    m4.begin_merge(SECOND, info(None));
    m4.land_merge_snapshots(stale);
    assert!(m4.merge().snapshots().is_empty() && m4.merge().shown());
    m4.run_merge_snapshots();
    assert_eq!(m4.merge().snapshots().len(), 1);
}

/// Swift `aDoneHoldDoesNotOpenTheReport`.
#[test]
fn a_done_hold_does_not_open_the_report() {
    let f = fake();
    let (m, _) = held(&f);
    assert!(!m.report_shown() && m.last_report().is_none());
    assert_eq!(m.merge().preview(), Some(&preview_row()));
}

/// Swift `aHeldMergeQueuesWhatItBlocks` (the exports; the port has no
/// dropped-MIDI queue): each export waits, and Merge lets them go; the
/// second merge then waits for them.
#[test]
fn a_held_merge_queues_what_it_blocks() {
    let f = fake();
    let (mut m, _) = held(&f);
    assert!(m.merge_held());
    // Each export waits.
    m.show_dawproject();
    m.run_dawproject_check();
    m.start_dawproject("/tmp/out.dawproject", false);
    m.show_for_daw();
    m.start_for_daw("/tmp/out");
    m.show_aaf_export();
    m.start_aaf_export("/tmp/out");
    assert!(m.dawproject().waiting() && m.for_daw().waiting() && m.aaf_export().waiting());
    assert_eq!(started_kinds(&f), [JobKindRow::MergeDawproject]);
    // Another merge waits for this one.
    m.begin_merge(SECOND, with_export());
    assert_eq!(merges(&f).len(), 1);
    assert!(m.merge().path() == PATH && m.merge().preview() == Some(&preview_row()));
    // Merge: the exports go (one at a time, as the core runs them); the
    // second merge then waits for them.
    f.script_job(JobScript::done().steps(&[0.5]));
    m.confirm_merge();
    assert!(!m.merge_held() && m.alert.is_none());
    assert_eq!(
        started_kinds(&f),
        [JobKindRow::MergeDawproject, JobKindRow::ExportProject]
    );
    assert!(!m.for_daw().waiting() && m.dawproject().waiting() && m.aaf_export().waiting());
    assert!(m.merge().waiting() && m.merge().shown() && m.merge().path() == SECOND);
    assert!(m.merge().preview().is_none());
    f.script_job(JobScript::done().steps(&[0.5]));
    m.poll_jobs();
    m.poll_jobs();
    f.script_job(JobScript::done().steps(&[0.5]));
    m.poll_jobs();
    m.poll_jobs();
    assert_eq!(
        started_kinds(&f)[1..4],
        [
            JobKindRow::ExportProject,
            JobKindRow::ExportDawproject,
            JobKindRow::ExportAaf
        ]
    );
    // The AAF export ended at its first poll: the second merge went.
    assert_eq!(
        merges(&f)
            .iter()
            .map(|r| r.path.as_str())
            .collect::<Vec<_>>(),
        [PATH, SECOND]
    );
    assert!(!m.merge().waiting() && m.alert.is_none());
}

/// Carried from Tasks 4–5: letting a hold go (`set_merge_held(false)`,
/// Swift `holdEnded`) starts each kept export; a held merge blocks each.
#[test]
fn letting_the_hold_go_releases_each_kept_export() {
    let f = fake();
    let mut m = DocumentModel::new(f.clone()).unwrap();
    m.set_merge_held(true);
    m.show_dawproject();
    m.run_dawproject_check();
    m.start_dawproject("/tmp/out.dawproject", false);
    assert!(m.dawproject().waiting() && f.jobs_started().is_empty());
    m.show_for_daw();
    m.start_for_daw("/tmp/out");
    assert!(m.for_daw().waiting() && f.jobs_started().is_empty());
    m.show_aaf_export();
    m.start_aaf_export("/tmp/out");
    assert!(m.aaf_export().waiting() && f.jobs_started().is_empty());
    // Nothing ends: only the hold going lets them go.
    m.poll_jobs();
    assert!(f.jobs_started().is_empty());
    f.script_job(JobScript::done().steps(&[0.5]));
    m.set_merge_held(false);
    assert_eq!(started_kinds(&f), [JobKindRow::ExportProject]);
    // The others wait for it (the core runs one at a time), then go in
    // turn as each ends.
    assert!(m.dawproject().waiting() && m.aaf_export().waiting());
    f.script_job(JobScript::done().steps(&[0.5]));
    m.poll_jobs();
    m.poll_jobs();
    m.poll_jobs();
    m.poll_jobs();
    assert_eq!(
        started_kinds(&f),
        [
            JobKindRow::ExportProject,
            JobKindRow::ExportDawproject,
            JobKindRow::ExportAaf
        ]
    );
}

/// Each export alone is released by the hold going.
#[test]
fn each_export_alone_is_released() {
    for which in 0..3 {
        let f = fake();
        let mut m = DocumentModel::new(f.clone()).unwrap();
        m.set_merge_held(true);
        match which {
            0 => {
                m.show_dawproject();
                m.run_dawproject_check();
                m.start_dawproject("/tmp/out.dawproject", false);
            }
            1 => {
                m.show_for_daw();
                m.start_for_daw("/tmp/out");
            }
            _ => {
                m.show_aaf_export();
                m.start_aaf_export("/tmp/out");
            }
        }
        assert!(f.jobs_started().is_empty(), "{which}: blocked");
        m.set_merge_held(false);
        assert_eq!(f.jobs_started().len(), 1, "{which}: released");
    }
}

/// Swift `closingDuringAHoldStartsNothing` (Review I1): closing the
/// document during a hold ends it quietly; nothing queued starts, and
/// no merge call is sent.
#[test]
fn closing_during_a_hold_starts_nothing() {
    let core = Arc::new(FakeCore::new());
    let mut s = DocumentStore::with_core(core.clone());
    let f = fake();
    core.hand_out("open", f.clone());
    let Ok(Opened::Doc(id)) = s.open(Path::new("/tmp/Song.vizdaw")) else {
        panic!("not opened");
    };
    script_hold(&f);
    s.with_document(id, |m, _| {
        m.begin_merge(PATH, with_export());
        m.poll_jobs();
        assert!(m.merge_held());
        m.show_dawproject();
        m.run_dawproject_check();
        m.start_dawproject("/tmp/out.dawproject", false);
        m.begin_merge(SECOND, with_export());
        assert!(m.dawproject().waiting());
    });
    s.close(id);
    assert!(s.document(id).is_none());
    // What the sheet's window would do as it goes: nothing reaches it.
    assert!(s.with_document(id, |m, _| m.cancel_merge()).is_none());
    assert_eq!(merges(&f).len(), 1);
    assert!(!started_kinds(&f).contains(&JobKindRow::ExportDawproject));
    assert!(calls(&f, "cancel_merge").is_empty() && calls(&f, "confirm_merge").is_empty());
    assert!(s.alert().is_none());
}

/// Swift `aQueuedMergeWaitsForTheReportSheet`: the merge queued behind a
/// confirmed one begins while the report is up (the window shows the
/// report first; the GPUI tests check the order).
#[test]
fn a_queued_merge_begins_after_the_confirmed_one() {
    let f = fake();
    let (mut m, _) = held(&f);
    m.begin_merge(SECOND, with_export());
    f.set_merge_report(report(
        "Merged: 1 applied, 0 kept, 0 added, 0 removed, 0 in conflict",
    ));
    f.script_job(JobScript::done().steps(&[0.5]));
    m.confirm_merge();
    assert!(m.report_shown() && m.merge().shown() && m.merge().path() == SECOND);
    let second = m.merge().job().unwrap();
    assert!(f.cancelled_jobs().is_empty() && calls(&f, "cancel_merge").is_empty());
    // Its own Cancel cancels it.
    m.cancel_merge();
    assert!(!m.merge().shown() && f.cancelled_jobs() == [second]);
}

/// Swift `previewTakesTheSelectedSnapshot`.
#[test]
fn preview_takes_the_selected_snapshot() {
    let f = fake();
    f.set_snapshots(vec![SnapshotRow {
        export_id: OTHER.into(),
        exported_at: 1_800_000_500,
        tracks: 3,
    }]);
    let mut m = DocumentModel::new(f.clone()).unwrap();
    m.begin_merge(PATH, info(None));
    m.run_merge_snapshots();
    assert_eq!(m.merge().selected_snapshot(), Some(OTHER), "the newest");
    m.select_merge_snapshot(None);
    m.preview_selected_merge();
    assert!(merges(&f).is_empty());
    m.select_merge_snapshot(Some(OTHER));
    f.script_job(JobScript::done().steps(&[0.5]));
    m.preview_selected_merge();
    assert_eq!(merges(&f), [hold(PATH, Some(OTHER))]);
    assert!(m.merge_running());
}

/// Swift `previewAgainAfterTheProjectChanged`.
#[test]
fn preview_again_after_the_project_changed() {
    let f = fake();
    let (mut m, _) = held(&f);
    assert!(!m.merge().can_preview_again());
    f.fail_next(
        "confirm_merge",
        VizError::Model {
            message: "The project changed after the preview; preview the merge again.".into(),
        },
    );
    m.confirm_merge();
    assert!(m.merge().can_preview_again() && !m.merge().can_merge());
    script_hold(&f);
    m.preview_merge_again();
    let all = merges(&f);
    assert!(all.len() == 2 && all[1].path == PATH);
    let g = m.merge();
    assert!(g.error().is_none() && !g.can_preview_again() && g.preview().is_none());
    assert!(m.merge_running());
    m.poll_jobs();
    assert!(m.merge().can_merge());
}

/// Swift `aHeldMergeBlocksAFormatChangeAndRelink`, and the 7a carry:
/// Convert and the rate change are off with the help, and do nothing.
#[test]
fn a_held_merge_blocks_convert_the_rate_change_and_relink() {
    let f = fake();
    f.set_project_info(ProjectInfoRow {
        format: ProjectFormatRow {
            sample_rate: 44_100,
            depth: BitDepthRow::Int16,
        },
        mismatched: 2,
        rate_mismatch: true,
        device_rate: 48_000.0,
    });
    let (mut m, _) = held(&f);
    m.refresh_project_info();
    let banners = m.banners();
    let convert = banners
        .iter()
        .find(|b| matches!(b, ProjectBanner::Mismatched { .. }))
        .unwrap();
    let rate = banners
        .iter()
        .find(|b| matches!(b, ProjectBanner::RateMismatch { .. }))
        .unwrap();
    for b in [convert, rate] {
        assert!(!m.banner_enabled(b));
        assert_eq!(m.banner_help(b), Some(HELD_HELP));
    }
    assert_eq!(
        HELD_HELP,
        "Finish or cancel the DAWproject merge waiting to be confirmed first."
    );
    let before = f.jobs_started().len();
    m.run_banner(BannerAction::Convert);
    m.run_banner(BannerAction::ChangeRateToDevice);
    assert_eq!(f.jobs_started().len(), before, "nothing starts");
    // Relink and a format change wait too.
    assert!(m.relink_blocked());
    // Once the hold goes, both are back.
    m.cancel_merge();
    let banners = m.banners();
    assert!(
        banners
            .iter()
            .all(|b| m.banner_enabled(b) && m.banner_help(b).is_none())
    );
    m.run_banner(BannerAction::Convert);
    assert_eq!(f.jobs_started().len(), before + 1);
}

/// Swift `aHoldDoneInTheFirstPollIsHeld`.
#[test]
fn a_hold_done_in_the_first_poll_is_held() {
    let f = fake();
    let mut m = DocumentModel::new(f.clone()).unwrap();
    f.script_job(JobScript::done().merge(preview_row()));
    m.begin_merge(PATH, with_export());
    let job = m.merge().job().unwrap();
    assert!(m.merge().held(), "held, so Cancel can let it go");
    m.cancel_merge();
    assert_eq!(calls(&f, "cancel_merge"), [format!("cancel_merge({job})")]);
}

/// Swift `aChannelActiveFalseDoesNotCancel`: in the port the sheet is
/// taken down under the channel without a press, which cancels nothing
/// (the GPUI test checks the sheet); the model only cancels on Cancel.
#[test]
fn only_cancel_cancels() {
    let f = fake();
    let (mut m, job) = held(&f);
    m.poll_jobs();
    m.refresh_after_job();
    assert!(m.merge().shown() && m.merge().held() && calls(&f, "cancel_merge").is_empty());
    m.cancel_merge();
    assert_eq!(calls(&f, "cancel_merge"), [format!("cancel_merge({job})")]);
}

/// Swift `aThirdMergeIsRefusedNotReplacing`.
#[test]
fn a_third_merge_is_refused_not_replacing() {
    let f = fake();
    let (mut m, _) = held(&f);
    m.begin_merge(SECOND, with_export());
    m.begin_merge("/tmp/third.dawproject", with_export());
    assert_eq!(m.status(), Some(ANOTHER_WAITING));
    f.script_job(JobScript::done().steps(&[0.5]));
    m.cancel_merge();
    assert_eq!(m.merge().path(), SECOND);
}

/// Swift `aCancelledHoldShowsASentence`.
#[test]
fn a_cancelled_hold_shows_a_sentence() {
    let f = fake();
    let mut m = DocumentModel::new(f.clone()).unwrap();
    f.script_job(JobScript::cancelled().steps(&[0.5]));
    m.begin_merge(PATH, with_export());
    m.poll_jobs();
    let g = m.merge();
    assert!(g.job().is_none() && !g.held() && g.error() == Some(CANCELLED));
    assert!(m.alert.is_none());
}

/// Swift `aMergeWaitsForARunningExport`.
#[test]
fn a_merge_waits_for_a_running_export() {
    let f = fake();
    let mut m = DocumentModel::new(f.clone()).unwrap();
    m.show_dawproject();
    m.run_dawproject_check();
    f.script_job(JobScript::done().steps(&[0.4]));
    m.start_dawproject("/tmp/out.dawproject", false);
    assert!(m.dawproject_running());
    m.begin_merge(PATH, with_export());
    assert!(m.merge().waiting() && merges(&f).is_empty() && m.merge().shown());
    f.script_job(JobScript::done().steps(&[0.5]));
    m.poll_jobs();
    assert!(!m.merge().waiting() && merges(&f).len() == 1 && m.merge_running());
}

#[test]
fn the_sheet_strings() {
    use vizdaw_app::merge::*;
    assert_eq!(TITLE, "Merge DAWproject");
    assert_eq!(
        WAITING_TEXT,
        "Waiting for the running job to finish\u{2026}"
    );
    assert_eq!(READING, "Reading the file\u{2026}");
    assert_eq!(WHICH_EXPORT, "Which export did this file come from?");
    assert_eq!(NO_CONFLICTS, "No conflicts.");
    assert_eq!(CONFLICTS, "Conflicts (VizDAW's version is kept)");
    assert_eq!(CANCEL_HELD_LABEL, "Cancel the merge; nothing changes");
    assert_eq!(MERGE_LABEL, "Merge into this project");
    assert_eq!(
        ANOTHER_WAITING,
        "A merge is waiting; finish it before opening another."
    );
    assert_eq!(conflicts_label(2), "Conflicts, 2");
    assert_eq!(
        (tracks_text(1), tracks_text(3)),
        ("1 track".into(), "3 tracks".into())
    );
    // 2027-01-15 08:05 UTC, and an hour east; noon and midnight.
    assert_eq!(when_at(1_800_000_300, 0), "Jan 15, 2027 at 8:05 AM");
    assert_eq!(when_at(1_800_000_300, 3_600), "Jan 15, 2027 at 9:05 AM");
    assert_eq!(when_at(1_800_014_400, 0), "Jan 15, 2027 at 12:00 PM");
    assert_eq!(when_at(1_799_971_200, 0), "Jan 15, 2027 at 12:00 AM");
    let row = SnapshotRow {
        export_id: EID.into(),
        exported_at: 1_800_000_300,
        tracks: 1,
    };
    assert!(snapshot_label(&row).starts_with("Export of "));
    assert!(snapshot_label(&row).ends_with(", 1 track"));
}

// ---- the store: the return prompt's Merge ----

fn store() -> (DocumentStore, Arc<FakeCore>) {
    let core = Arc::new(FakeCore::new());
    (DocumentStore::with_core(core.clone()), core)
}

fn open_song(
    s: &mut DocumentStore,
    core: &FakeCore,
    path: &str,
) -> (vizdaw_app::DocId, Arc<FakeSession>) {
    let f = fake();
    f.set_path(Some(path));
    core.hand_out("open", f.clone());
    let Ok(Opened::Doc(id)) = s.open(Path::new(path)) else {
        panic!("{path} did not open");
    };
    (id, f)
}

fn prompt(s: &mut DocumentStore, core: &FakeCore, export: Option<&str>) {
    core.set_dawproject_info(PATH, info(export));
    s.open_dawproject(PATH);
    s.run_reads();
    assert!(s.return_prompt().is_some());
}

#[test]
fn merge_into_the_matched_project() {
    let (mut s, core) = store();
    let (id, f) = open_song(&mut s, &core, "/tmp/Song.vizdaw");
    prompt(&mut s, &core, Some(EID));
    assert_eq!(s.return_prompt().unwrap().matched, Some(id));
    f.script_job(JobScript::done().steps(&[0.5]));
    assert_eq!(
        s.answer_return(ReturnChoice::Merge),
        Some(ReturnNext::Merging(id))
    );
    assert!(s.return_prompt().is_none());
    assert_eq!(merges(&f), [hold(PATH, None)]);
    assert!(s.document(id).unwrap().merge().shown());
}

/// 7a carry: a prompt naming a document closed since is not used by
/// Merge (its match goes on close); Merge asks for the project.
#[test]
fn a_closed_match_is_not_used() {
    let (mut s, core) = store();
    let (id, f) = open_song(&mut s, &core, "/tmp/Song.vizdaw");
    prompt(&mut s, &core, Some(EID));
    // A second prompt queued behind it names the same project.
    core.set_dawproject_info(SECOND, info(Some(EID)));
    s.open_dawproject(SECOND);
    s.run_reads();
    s.close(id);
    let p = s.return_prompt().unwrap();
    assert!(p.matched.is_none() && p.match_name.is_none());
    assert_eq!(
        p.informative(),
        "Merge it into the project it came from, or open it as a new project."
    );
    assert!(matches!(
        s.answer_return(ReturnChoice::Merge),
        Some(ReturnNext::ChooseProject(MergeFile { ref path, .. })) if path == PATH
    ));
    let q = s.return_prompt().unwrap();
    assert!(q.path == SECOND && q.matched.is_none() && q.match_name.is_none());
    assert!(merges(&f).is_empty());
}

#[test]
fn merge_into_a_chosen_project() {
    let (mut s, core) = store();
    prompt(&mut s, &core, Some(EID));
    let Some(ReturnNext::ChooseProject(file)) = s.answer_return(ReturnChoice::Merge) else {
        panic!("asks for the project");
    };
    // Linux: the package's project.json is that package.
    let f = fake();
    core.hand_out("open", f.clone());
    let id = s
        .merge_into(Path::new("/tmp/Song.vizdaw/project.json"), file.clone())
        .unwrap();
    assert!(
        core.calls().iter().any(|c| c == "open(/tmp/Song.vizdaw)"),
        "{:?}",
        core.calls()
    );
    assert_eq!(merges(&f), [hold(PATH, None)]);
    assert!(s.document(id).unwrap().merge().shown());
    // Already open: that document, nothing opened again.
    f.set_path(Some("/tmp/Song.vizdaw"));
    s.with_document(id, |m, _| m.cancel_merge());
    let opens = core.calls().len();
    assert_eq!(
        s.merge_into(Path::new("/tmp/Song.vizdaw"), file.clone()),
        Some(id)
    );
    assert_eq!(core.calls().len(), opens);
    s.with_document(id, |m, _| m.cancel_merge());
    // Its project.json (Linux) is that open package too.
    assert_eq!(
        s.merge_into(Path::new("/tmp/Song.vizdaw/project.json"), file.clone()),
        Some(id)
    );
    assert_eq!(core.calls().len(), opens);
    // Not a project: Open Failed.
    let (mut t, _) = store();
    assert!(t.merge_into(Path::new("/tmp/x.mid"), file).is_none());
    let a = t.alert().unwrap();
    assert_eq!(
        (a.title.as_str(), a.message.as_str()),
        ("Open Failed", "x.mid is not a .vizdaw project.")
    );
}

/// A chosen project that asks a recovery question first begins the merge
/// once the answer opens it (Cancel: nothing).
#[test]
fn merge_after_a_recovery_question() {
    let (mut s, core) = store();
    prompt(&mut s, &core, Some(EID));
    let Some(ReturnNext::ChooseProject(file)) = s.answer_return(ReturnChoice::Merge) else {
        panic!("asks for the project");
    };
    core.set_recovery(
        "/tmp/Song.vizdaw",
        RecoveryInfo {
            crashed: false,
            loading: vec![],
            autosave: Some("/tmp/Song.vizdaw/autosave".into()),
        },
    );
    assert!(
        s.merge_into(Path::new("/tmp/Song.vizdaw"), file.clone())
            .is_none()
    );
    assert!(s.open_question().is_some());
    let f = fake();
    core.hand_out("open", f.clone());
    let id = s.answer_autosave(false).unwrap();
    assert!(s.document(id).unwrap().merge().shown());
    assert_eq!(merges(&f).len(), 1);
}
