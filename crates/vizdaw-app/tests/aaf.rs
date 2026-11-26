//! The AAF sheets' model (slice 7c Task 5; Swift `AafModelTests`): the
//! export's check off the UI thread on every option (only the newest
//! answer lands), an export that waits for a running job, a failure back
//! in the sheet, the folder never replaced (the core numbers it); the
//! import's file read off the UI thread, its defaults (a new project) and
//! its request, into this project or a new one at the file's rate.

mod support;

use std::sync::Arc;

use support::{FakeSession, demo_fake};
use vizdaw_app::aaf::{
    self, AafExportChoices, EXPORT_FAILED, IMPORT_FAILED, NO_SEQUENCE, describe, writes_folder,
};
use vizdaw_app::jobs::failure_title;
use vizdaw_app::testing::{FakeCore, JobRequest, JobScript, seq};
use vizdaw_app::{DocId, DocumentModel, DocumentStore};
use vizdaw_ffi::{
    AafDepthRow, AafExportRequestRow, AafFpsRow, AafImportRequestRow, AafInfoRow, AafStereoRow,
    JobKindRow, ReportRow, VizError,
};

fn report(summary: &str) -> ReportRow {
    ReportRow {
        summary: summary.into(),
        items: vec![],
    }
}

fn model(message: &str) -> VizError {
    VizError::Model {
        message: message.into(),
    }
}

fn fake() -> Arc<FakeSession> {
    let f = demo_fake();
    f.set_check_report("export_aaf_check", report("check"));
    Arc::new(f)
}

fn shown(f: &Arc<FakeSession>) -> DocumentModel {
    let mut m = DocumentModel::new(f.clone()).unwrap();
    m.show_aaf_export();
    m.run_aaf_check();
    m
}

fn exports(f: &FakeSession) -> Vec<AafExportRequestRow> {
    f.jobs_started()
        .into_iter()
        .filter_map(|j| match j.request {
            JobRequest::ExportAaf(r) => Some(r),
            _ => None,
        })
        .collect()
}

fn imports(f: &FakeSession) -> Vec<AafImportRequestRow> {
    f.jobs_started()
        .into_iter()
        .filter_map(|j| match j.request {
            JobRequest::ImportAaf(r) => Some(r),
            _ => None,
        })
        .collect()
}

/// A bounce the UI started, running for `polls` more polls.
fn bouncing(f: &FakeSession, m: &mut DocumentModel, polls: usize) {
    f.script_job(JobScript::done().steps(&vec![0.4; polls + 1]));
    let req = m.bounce_request(Some("/tmp/b.wav"), false).unwrap();
    m.try_start_job(JobKindRow::Bounce, |s| s.bounce(req))
        .unwrap();
}

/// Swift `checkRefreshesOnEachOptionAndOnlyTheLatestLands`.
#[test]
fn the_check_refreshes_on_each_option_and_only_the_latest_lands() {
    let f = fake();
    let mut m = DocumentModel::new(f.clone()).unwrap();
    m.show_aaf_export();
    assert!(f.aaf_checks().is_empty(), "not on this thread");
    assert!(m.aaf_export().checking());
    m.run_aaf_check();
    let checks = f.aaf_checks();
    assert_eq!(checks.len(), 1);
    assert_eq!(
        checks[0],
        AafExportRequestRow {
            seq: 3,
            path: String::new(),
            embed: true,
            depth: AafDepthRow::Int24,
            stereo: AafStereoRow::MonoPairs,
            hour_start: false,
            fps: AafFpsRow::Fps25,
            clip_gain: true,
            automation: true,
        },
        "the defaults"
    );
    assert_eq!(m.aaf_export().wont_transfer(), Some(&report("check")));
    assert!(m.aaf_export().error().is_none());
    assert_eq!(m.aaf_export().default_name(), "Song AAF");
    assert_eq!(f.reads("aaf_folder_name"), 1);

    // Two quick changes: the first one's late answer is dropped.
    m.edit_aaf_export(|c| c.depth = AafDepthRow::Int16);
    let first = m.take_aaf_check().unwrap();
    m.edit_aaf_export(|c| c.fps = AafFpsRow::Fps24);
    f.set_check_report("export_aaf_check", report("second"));
    m.run_aaf_check();
    m.land_aaf_check(first.run(f.as_ref()));
    assert_eq!(m.aaf_export().wont_transfer(), Some(&report("second")));
    assert_eq!(f.aaf_checks().len(), 3);
    let newest = &f.aaf_checks()[1];
    assert!(newest.depth == AafDepthRow::Int16 && newest.fps == AafFpsRow::Fps24);

    // Every other option asks again, with the option sent.
    let edits: [fn(&mut AafExportChoices); 5] = [
        |c| c.embed = false,
        |c| c.stereo = AafStereoRow::Stereo,
        |c| c.hour_start = true,
        |c| c.clip_gain = false,
        |c| c.automation = false,
    ];
    for e in edits {
        m.edit_aaf_export(e);
        m.run_aaf_check();
    }
    let checks = f.aaf_checks();
    assert_eq!(checks.len(), 8);
    let last = checks.last().unwrap();
    assert!(!last.embed && last.stereo == AafStereoRow::Stereo && last.hour_start);
    assert!(!last.clip_gain && !last.automation);
    // The same value again asks nothing.
    m.edit_aaf_export(|c| c.automation = false);
    assert!(m.take_aaf_check().is_none());

    // The core's refusal is the sheet's message.
    f.fail_next("export_aaf_check", model("no such sequence"));
    m.refresh_aaf_export();
    m.run_aaf_check();
    assert!(m.aaf_export().wont_transfer().is_none());
    assert_eq!(m.aaf_export().error(), Some("no such sequence"));
}

/// Swift `exportQueuesBehindARunningJob`.
#[test]
fn the_export_waits_behind_a_running_job() {
    let f = fake();
    let mut m = shown(&f);
    bouncing(&f, &mut m, 1);
    m.edit_aaf_export(|c| c.fps = AafFpsRow::Fps30);
    m.run_aaf_check();
    m.start_aaf_export("/tmp/out");
    assert!(m.aaf_export().waiting() && exports(&f).is_empty());
    assert!(m.alert.is_none() && m.aaf_export().job().is_none());
    m.poll_jobs();
    assert!(exports(&f).is_empty(), "still running");
    f.script_job(JobScript::done().steps(&[0.4]));
    m.poll_jobs();
    let r = exports(&f).pop().expect("the bounce ended: it starts");
    assert!(!m.aaf_export().waiting() && m.aaf_export_running());
    assert!(r.path == "/tmp/out" && r.fps == AafFpsRow::Fps30 && r.seq == 3);

    // Cancel while waiting sends nothing.
    let g = fake();
    let mut n = shown(&g);
    bouncing(&g, &mut n, 0);
    n.start_aaf_export("/tmp/out");
    assert!(n.aaf_export().waiting());
    n.cancel_aaf_export_waiting();
    n.poll_jobs();
    assert!(exports(&g).is_empty() && n.aaf_export().job().is_none());
    assert!(!n.aaf_export().waiting());
}

/// A held merge blocks the export too (Swift `blocked`).
#[test]
fn a_held_merge_blocks_the_export() {
    let f = fake();
    let mut m = shown(&f);
    m.set_merge_held(true);
    m.start_aaf_export("/tmp/out");
    assert!(m.aaf_export().waiting() && exports(&f).is_empty());
}

/// Swift `failedExportReopensWithTheMessage`.
#[test]
fn a_failed_export_reopens_with_the_message() {
    let f = fake();
    let mut m = shown(&f);
    f.script_job(JobScript::failed("Song AAF is already in /tmp/out.").steps(&[0.5]));
    m.start_aaf_export("/tmp/out");
    let job = m.aaf_export().job().unwrap();
    assert!(m.aaf_export_running());
    assert!(
        m.jobs()
            .iter()
            .any(|j| j.id == job && j.kind == JobKindRow::ExportAaf)
    );
    m.hide_aaf_export();
    m.poll_jobs();
    assert_eq!(
        m.aaf_export().error(),
        Some("Song AAF is already in /tmp/out.")
    );
    assert!(m.aaf_export().shown(), "the sheet again");
    assert!(!m.report_shown() && m.last_report().is_none() && m.alert.is_none());
    assert!(!m.aaf_export_running());

    // A cancelled one is quiet; a finished one closes and shows its report.
    f.script_job(JobScript::done().steps(&[0.5]));
    m.start_aaf_export("/tmp/out");
    m.cancel_aaf_export();
    m.poll_jobs();
    assert!(m.alert.is_none() && !m.report_shown() && !m.aaf_export().shown());
    m.show_aaf_export();
    m.run_aaf_check();
    f.script_job(
        JobScript::done()
            .steps(&[0.5])
            .report(report("Exported 2 tracks"))
            .written(&["/tmp/out/Song AAF/Song.aaf"]),
    );
    m.start_aaf_export("/tmp/out");
    m.poll_jobs();
    assert!(m.report_shown());
    assert_eq!(m.report_written(), ["/tmp/out/Song AAF/Song.aaf"]);
    assert!(!m.aaf_export().shown() && m.alert.is_none());

    // Refused as it starts: the message in the sheet, no alert.
    m.show_aaf_export();
    m.run_aaf_check();
    f.fail_next("export_aaf", model("the folder is not writable"));
    m.start_aaf_export("/tmp/out");
    assert_eq!(m.aaf_export().error(), Some("the folder is not writable"));
    assert!(m.aaf_export().shown() && m.alert.is_none() && !m.aaf_export_running());

    // A check in flight when the start is refused lands late: the
    // refusal stays.
    m.edit_aaf_export(|c| c.embed = false);
    let late = m.take_aaf_check().unwrap();
    f.fail_next("export_aaf", model("the folder is not writable"));
    m.start_aaf_export("/tmp/out");
    m.land_aaf_check(late.run(f.as_ref()));
    assert_eq!(m.aaf_export().error(), Some("the folder is not writable"));
    assert_eq!(failure_title(JobKindRow::ExportAaf), EXPORT_FAILED);
}

/// Showing the sheet again while an export waits or runs keeps it (no
/// new name, no reset).
#[test]
fn showing_again_keeps_a_waiting_or_running_export() {
    let f = fake();
    let mut m = shown(&f);
    bouncing(&f, &mut m, 0);
    m.edit_aaf_export(|c| c.hour_start = true);
    m.run_aaf_check();
    m.start_aaf_export("/tmp/out");
    m.hide_aaf_export();
    m.show_aaf_export();
    assert!(m.aaf_export().waiting(), "still waiting");
    assert_eq!(f.reads("aaf_folder_name"), 1, "not reset");
    f.script_job(JobScript::done().steps(&[0.5, 0.5]));
    m.poll_jobs();
    let job = m.aaf_export().job();
    assert!(m.aaf_export_running());
    m.hide_aaf_export();
    m.show_aaf_export();
    assert!(m.aaf_export_running() && m.aaf_export().job() == job);
    assert!(exports(&f).pop().unwrap().hour_start);
    // Reopened after it ended: fresh.
    m.hide_aaf_export();
    while m.aaf_export_running() {
        m.poll_jobs();
    }
    m.show_aaf_export();
    assert_eq!(f.reads("aaf_folder_name"), 2);
    assert!(m.aaf_export().job().is_none() && m.aaf_export().checking());
    assert!(m.aaf_export().choices().hour_start, "the choices are kept");
}

/// No sequence shown: the sheet says so, nothing starts.
#[test]
fn no_sequence_is_the_sheets_message() {
    let f = Arc::new(FakeSession::new(vec![]));
    let mut m = DocumentModel::new(f.clone()).unwrap();
    m.show_aaf_export();
    m.run_aaf_check();
    assert_eq!(m.aaf_export().error(), Some(NO_SEQUENCE));
    assert_eq!(m.aaf_export().default_name(), "");
    m.start_aaf_export("/tmp/out");
    assert!(m.aaf_export().job().is_none() && exports(&f).is_empty());
    assert_eq!(
        writes_folder("Song AAF"),
        "Writes the folder \u{201c}Song AAF\u{201d}."
    );
    for s in [
        aaf::EXPORT_TITLE,
        aaf::EXPORT_SUBTITLE,
        aaf::EMBED,
        aaf::STEREO_TRACKS,
        aaf::IMPORT_TITLE,
        aaf::JOIN_PAIRS,
        aaf::MIDI_BESIDE,
        aaf::LOOK_FOR_MISSING,
    ] {
        assert!(!s.to_lowercase().contains("lane"), "{s}");
    }
}

// ---------------------------------------------------------------------
// Import

fn info(rate: u32, offline: u32, midi: Option<&str>) -> AafInfoRow {
    AafInfoRow {
        sample_rate: rate,
        tracks: 3,
        audio_files: 2,
        offline,
        from_vizdaw: false,
        midi_beside: midi.map(str::to_owned),
    }
}

/// A store whose front document is `fake`.
fn store_with(fake: &Arc<FakeSession>) -> (DocumentStore, Arc<FakeCore>, DocId) {
    let core = Arc::new(FakeCore::new());
    let mut s = DocumentStore::with_core(core.clone());
    core.hand_out("new_demo", fake.clone());
    let id = s.new_demo().unwrap();
    (s, core, id)
}

fn read(s: &mut DocumentStore, id: DocId, path: &str) {
    s.begin_aaf_import(id, path);
    s.run_reads();
}

/// Swift `chooseFillsTheMidiBeside`.
#[test]
fn choosing_a_file_reads_it_then_fills_the_midi_beside() {
    let f = fake();
    let (mut s, core, id) = store_with(&f);
    core.set_aaf_info("/x/Song.aaf", info(44_100, 1, Some("/x/Song MIDI.mid")));
    s.begin_aaf_import(id, "/x/Song.aaf");
    assert!(
        s.document(id).unwrap().aaf_import().is_none(),
        "the sheet waits for the file to be read"
    );
    assert!(
        !core.calls().iter().any(|c| c.starts_with("aaf_info")),
        "not on this thread"
    );
    s.run_reads();
    assert!(core.calls().contains(&"aaf_info(/x/Song.aaf)".to_owned()));
    let a = s.document(id).unwrap().aaf_import().unwrap().clone();
    assert!(a.shown() && a.path() == "/x/Song.aaf" && a.info().sample_rate == 44_100);
    assert_eq!(a.midi_path.as_deref(), Some("/x/Song MIDI.mid"));
    assert!(a.new_project, "a new project by default");
    assert!(a.join_pairs && a.media_folder.is_none());
    assert_eq!(
        describe(a.info(), a.path()),
        "Song.aaf: 44100 Hz, 3 tracks, 2 audio files, 1 missing"
    );
    assert_eq!(
        describe(
            &AafInfoRow {
                tracks: 1,
                audio_files: 1,
                ..info(48_000, 0, None)
            },
            "/y/One.aaf"
        ),
        "One.aaf: 48000 Hz, 1 track, 1 audio file"
    );

    // A refusal is the alert "Import Failed", and no sheet.
    s.with_document(id, |d, _| d.hide_aaf_import());
    read(&mut s, id, "/x/b.aaf");
    assert!(!s.document(id).unwrap().aaf_import().unwrap().shown());
    let alert = s.take_alert().unwrap();
    assert_eq!(alert.title, "Import Failed");
    assert!(alert.message.contains("b.aaf"), "{}", alert.message);
}

/// Only the newest file chosen lands; a closed document drops it.
#[test]
fn only_the_newest_read_lands() {
    let f = fake();
    let (mut s, core, id) = store_with(&f);
    core.set_aaf_info("/x/a.aaf", info(44_100, 0, None));
    core.set_aaf_info("/x/b.aaf", info(96_000, 0, None));
    s.begin_aaf_import(id, "/x/a.aaf");
    s.begin_aaf_import(id, "/x/b.aaf");
    // The older read lands last: it is dropped.
    let reads = s.take_reads();
    let landed: Vec<_> = reads.into_iter().map(|r| r.run()).collect();
    for l in landed.into_iter().rev() {
        s.land(l);
    }
    let a = s.document(id).unwrap().aaf_import().unwrap().clone();
    assert_eq!(a.path(), "/x/b.aaf");
    assert_eq!(a.info().sample_rate, 96_000);
    // Read again after edits: the defaults again.
    s.with_document(id, |d, _| {
        d.edit_aaf_import(|a| {
            a.new_project = false;
            a.join_pairs = false;
            a.media_folder = Some("/media".into());
        })
    });
    read(&mut s, id, "/x/a.aaf");
    let a = s.document(id).unwrap().aaf_import().unwrap().clone();
    assert!(a.new_project && a.join_pairs && a.media_folder.is_none());
    // A document closed before its read landed drops it.
    s.begin_aaf_import(id, "/x/a.aaf");
    s.close(id);
    s.run_reads();
    assert!(s.take_alert().is_none());
}

/// Swift `importIntoThisProjectSendsNewProjectFalse`.
#[test]
fn import_into_this_project_sends_new_project_false() {
    let f = fake();
    f.script_job(JobScript::done().steps(&[0.5]));
    let (mut s, core, id) = store_with(&f);
    core.set_aaf_info("/x/Song.aaf", info(44_100, 0, Some("/x/Song MIDI.mid")));
    read(&mut s, id, "/x/Song.aaf");
    s.with_document(id, |d, _| {
        d.edit_aaf_import(|a| {
            a.new_project = false;
            a.join_pairs = false;
            a.media_folder = Some("/media".into());
        })
    });
    assert_eq!(s.start_aaf_import(id), None, "no project is made");
    assert_eq!(
        imports(&f),
        [AafImportRequestRow {
            path: "/x/Song.aaf".into(),
            new_project: false,
            midi_path: Some("/x/Song MIDI.mid".into()),
            media_folder: Some("/media".into()),
            join_pairs: false,
        }]
    );
    let d = s.document(id).unwrap();
    assert!(!d.aaf_import().unwrap().shown() && s.count() == 1);
    assert!(d.jobs().iter().any(|j| j.kind == JobKindRow::ImportAaf));
    assert_eq!(failure_title(JobKindRow::ImportAaf), IMPORT_FAILED);
    // An emptied MIDI field sends none (the core then looks beside it).
    read(&mut s, id, "/x/Song.aaf");
    s.with_document(id, |d, _| {
        d.edit_aaf_import(|a| {
            a.new_project = false;
            a.midi_path = Some(String::new());
        })
    });
    s.start_aaf_import(id);
    assert_eq!(imports(&f).pop().unwrap().midi_path, None);
    // Refused at once: the alert "AAF Import Failed".
    read(&mut s, id, "/x/Song.aaf");
    s.with_document(id, |d, _| d.edit_aaf_import(|a| a.new_project = false));
    f.fail_next("import_aaf", model("busy"));
    s.start_aaf_import(id);
    let d = s.document(id).unwrap();
    assert_eq!(d.alert.as_ref().unwrap().title, IMPORT_FAILED);
}

/// Swift `theSheetsNewProjectCarriesItsChoices`: "Open as a new
/// project" makes a project at the file's rate importing it with the
/// sheet's choices; its window opens.
#[test]
fn the_sheets_new_project_carries_its_choices() {
    let f = fake();
    let (mut s, core, id) = store_with(&f);
    core.set_aaf_info("/x/c.aaf", info(88_200, 0, None));
    read(&mut s, id, "/x/c.aaf");
    s.with_document(id, |d, _| {
        d.edit_aaf_import(|a| {
            a.join_pairs = false;
            a.midi_path = Some("/x/other.mid".into());
        })
    });
    let made = Arc::new(FakeSession::new(vec![seq(9, "New", true, 0)]));
    core.hand_out("new_project", made.clone());
    let new = s.start_aaf_import(id).expect("a new project");
    assert!(new != id && s.document(new).is_some() && s.count() == 2);
    assert!(!s.document(id).unwrap().aaf_import().unwrap().shown());
    assert!(imports(&f).is_empty());
    assert!(
        core.calls()
            .iter()
            .any(|c| c.starts_with("new_project(88200,")),
        "{:?}",
        core.calls()
    );
    assert_eq!(
        imports(&made),
        [AafImportRequestRow {
            path: "/x/c.aaf".into(),
            new_project: true,
            midi_path: Some("/x/other.mid".into()),
            media_folder: None,
            join_pairs: false,
        }]
    );
}

/// Swift `aRefusedOpenLeavesNoProject` (the sheet's new project): a job
/// that fails on the untouched project takes it with it, with the
/// store's alert "Import Failed".
#[test]
fn a_failed_new_project_import_leaves_nothing() {
    let f = fake();
    let (mut s, core, id) = store_with(&f);
    core.set_aaf_info("/x/c.aaf", info(48_000, 0, None));
    read(&mut s, id, "/x/c.aaf");
    let made = Arc::new(FakeSession::new(vec![seq(9, "New", true, 0)]));
    made.script_job(JobScript::failed("the file is damaged").steps(&[0.5]));
    core.hand_out("new_project", made);
    let new = s.start_aaf_import(id).unwrap();
    s.with_document(new, |d, _| d.poll_jobs());
    assert!(s.document(new).is_none() && s.count() == 1);
    let a = s.take_alert().unwrap();
    assert_eq!(
        (a.title.as_str(), a.message.as_str()),
        ("Import Failed", "the file is damaged")
    );
}
