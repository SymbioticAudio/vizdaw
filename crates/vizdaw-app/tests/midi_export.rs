//! The Export MIDI File / MIDI Clip File sheet's model (slice 7c Task 2;
//! Swift `InterchangeTests`' export rows): the choices' defaults and dry
//! runs (only the newest answer lands), where Export… writes, a failure
//! that brings the sheet back with its message (never an alert), the
//! report an export keeps, and the replace question before a clip export
//! into a folder writes over anything.

mod support;

use std::sync::Arc;

use support::{FakeSession, demo_fake, lane_row};
use vizdaw_app::interchange::{ExportDestination, ExportKind, ReplaceQuestion, export_track_label};
use vizdaw_app::report_text;
use vizdaw_app::testing::{FakeCore, JobRequest, JobScript};
use vizdaw_app::{DocId, DocumentStore};
use vizdaw_ffi::edit::LaneRef;
use vizdaw_ffi::selection::{SelectionRow, TickRange};
use vizdaw_ffi::{
    ClipExportRequestRow, ClipNamesRow, ReportItemRow, ReportKindRow, ReportRow,
    SmfExportRequestRow, VizError,
};

/// The demo with events in its shown sequence's tracks (Swift `busy`).
fn busy() -> FakeSession {
    let f = demo_fake();
    let mut a = lane_row(30, "Groove", false);
    let mut b = lane_row(31, "Chords", false);
    a.midi_event_count = 12;
    b.midi_event_count = 3;
    f.set_lanes(3, vec![a, b]);
    f
}

fn store_with(fake: &Arc<FakeSession>) -> (DocumentStore, DocId) {
    let core = Arc::new(FakeCore::new());
    let mut s = DocumentStore::with_core(core.clone());
    core.hand_out("new_demo", fake.clone());
    let id = s.new_demo().unwrap();
    (s, id)
}

fn report(summary: &str) -> ReportRow {
    ReportRow {
        summary: summary.into(),
        items: vec![],
    }
}

fn smf_exports(f: &FakeSession) -> Vec<SmfExportRequestRow> {
    f.jobs_started()
        .into_iter()
        .filter_map(|j| match j.request {
            JobRequest::ExportMidi(r) => Some(r),
            _ => None,
        })
        .collect()
}

fn clip_exports(f: &FakeSession) -> Vec<ClipExportRequestRow> {
    f.jobs_started()
        .into_iter()
        .filter_map(|j| match j.request {
            JobRequest::ExportMidiClip(r) => Some(r),
            _ => None,
        })
        .collect()
}

fn model_error(message: &str) -> VizError {
    VizError::Model {
        message: message.into(),
    }
}

#[test]
fn export_options_refresh_the_dry_run() {
    let f = Arc::new(busy());
    f.set_check_report("export_midi_check", report("smf"));
    f.set_check_report("export_midi_clip_check", report("clip"));
    let (mut s, id) = store_with(&f);
    let d = s.document_mut(id).unwrap();
    d.show_midi_export(ExportKind::Smf);
    assert!(d.midi_export().checking(), "no answer yet");
    d.run_midi_export_check();
    let e = d.midi_export();
    assert!(e.shown() && e.kind() == ExportKind::Smf);
    let c = e.choices();
    assert_eq!(c.lanes.iter().copied().collect::<Vec<_>>(), [30, 31]);
    assert!(c.ppq == 960 && !c.type0 && !c.mpe && !c.use_selection);
    assert_eq!(e.wont_transfer(), Some(&report("smf")));
    assert_eq!(f.smf_checks().len(), 1);
    d.edit_midi_export(|c| c.type0 = true);
    d.run_midi_export_check();
    d.edit_midi_export(|c| c.ppq = 480);
    d.run_midi_export_check();
    d.edit_midi_export(|c| {
        c.lanes.remove(&30);
    });
    d.run_midi_export_check();
    d.edit_midi_export(|c| c.mpe = true);
    d.run_midi_export_check();
    // No change asks nothing.
    d.edit_midi_export(|c| c.mpe = true);
    d.run_midi_export_check();
    let checks = f.smf_checks();
    assert_eq!(checks.len(), 5);
    assert_eq!(
        checks.last(),
        Some(&SmfExportRequestRow {
            seq: 3,
            lanes: vec![31],
            path: String::new(),
            replace: false,
            type0: true,
            ppq: 480,
            mpe: true,
        })
    );
    // The clip export has its own dry run, the choices reset.
    d.show_midi_export(ExportKind::Clip);
    d.run_midi_export_check();
    assert_eq!(f.clip_checks().len(), 1);
    assert_eq!(f.clip_checks()[0].lanes, [30, 31]);
    assert_eq!(d.midi_export().wont_transfer(), Some(&report("clip")));
    d.edit_midi_export(|c| {
        c.lanes.remove(&31);
    });
    d.run_midi_export_check();
    assert_eq!(f.clip_checks().len(), 2);
    assert_eq!(f.clip_checks()[1].lanes, [30]);
    assert_eq!(f.smf_checks().len(), 5);
}

#[test]
fn export_tracks_start_with_the_selected_ones() {
    let f = Arc::new(busy());
    let (mut s, id) = store_with(&f);
    let d = s.document_mut(id).unwrap();
    d.set_selection(SelectionRow {
        time_range: Some(TickRange {
            start: 960,
            end: 1920,
        }),
        lanes: vec![LaneRef { seq: 3, lane: 31 }],
        events: vec![],
    });
    d.show_midi_export(ExportKind::Clip);
    d.run_midi_export_check();
    assert!(d.midi_export_has_selection());
    assert_eq!(
        d.midi_export()
            .choices()
            .lanes
            .iter()
            .copied()
            .collect::<Vec<_>>(),
        [31]
    );
    let first = f.clip_checks().pop().unwrap();
    assert_eq!((first.start, first.end), (None, None), "the whole sequence");
    d.edit_midi_export(|c| c.use_selection = true);
    d.run_midi_export_check();
    let last = f.clip_checks().pop().unwrap();
    assert_eq!((last.start, last.end), (Some(960), Some(1920)));
    d.start_midi_export("/tmp/x.midi2", true);
    assert_eq!(
        clip_exports(&f),
        [ClipExportRequestRow {
            seq: 3,
            lanes: vec![31],
            path: "/tmp/x.midi2".into(),
            replace: true,
            start: Some(960),
            end: Some(1920),
        }]
    );
    assert!(!d.midi_export().shown());
}

/// A selected track of another sequence (or one gone) is not a choice:
/// every track is.
#[test]
fn a_selection_elsewhere_exports_every_track() {
    let f = Arc::new(busy());
    let (mut s, id) = store_with(&f);
    let d = s.document_mut(id).unwrap();
    d.set_selection(SelectionRow {
        time_range: None,
        lanes: vec![LaneRef { seq: 9, lane: 31 }],
        events: vec![],
    });
    d.show_midi_export(ExportKind::Smf);
    assert_eq!(d.midi_export().choices().lanes.len(), 2);
    assert!(!d.midi_export_has_selection());
}

/// Two changes in a row: the first's answer arrives last and is dropped.
#[test]
fn a_stale_dry_run_answer_is_dropped() {
    let f = Arc::new(busy());
    let (mut s, id) = store_with(&f);
    let d = s.document_mut(id).unwrap();
    d.show_midi_export(ExportKind::Smf);
    d.run_midi_export_check();
    d.edit_midi_export(|c| c.ppq = 480);
    let first = d.take_midi_export_check().unwrap();
    d.edit_midi_export(|c| c.ppq = 15_360);
    let second = d.take_midi_export_check().unwrap();
    assert!(d.take_midi_export_check().is_none(), "taken once");
    f.set_check_report("export_midi_check", report("ppq 15360"));
    let newer = second.run(d.session().as_ref());
    f.set_check_report("export_midi_check", report("ppq 480"));
    let older = first.run(d.session().as_ref());
    d.land_midi_export_check(newer);
    d.land_midi_export_check(older);
    assert_eq!(d.midi_export().wont_transfer(), Some(&report("ppq 15360")));
}

#[test]
fn a_refused_dry_run_is_the_sheets_error() {
    let f = Arc::new(busy());
    f.set_call_error("export_midi_check", Some(model_error("no tracks")));
    let (mut s, id) = store_with(&f);
    let d = s.document_mut(id).unwrap();
    d.show_midi_export(ExportKind::Smf);
    d.run_midi_export_check();
    let e = d.midi_export();
    assert_eq!(e.error(), Some("no tracks"));
    assert!(e.wont_transfer().is_none() && !e.checking());
    f.set_call_error("export_midi_check", None);
    d.edit_midi_export(|c| c.type0 = true);
    d.run_midi_export_check();
    assert!(d.midi_export().error().is_none());
    assert!(d.midi_export().wont_transfer().is_some());
}

#[test]
fn no_sequence_shown_says_so() {
    let f = Arc::new(busy());
    let (mut s, id) = store_with(&f);
    let d = s.document_mut(id).unwrap();
    d.select(None);
    d.show_midi_export(ExportKind::Smf);
    assert!(d.take_midi_export_check().is_none());
    assert_eq!(d.midi_export().error(), Some("No sequence is shown."));
    d.start_midi_export("/tmp/x.mid", true);
    assert!(smf_exports(&f).is_empty());
    assert_eq!(d.midi_export().error(), Some("No sequence is shown."));
    d.export_midi_to_folder("/tmp/out");
    assert!(d.take_clip_names().is_none());
    assert!(d.alert.is_none());
}

/// Export… asks for `<song>.mid`, one clip track's `<track>.midi2`, or a
/// folder for several clip tracks.
#[test]
fn export_asks_where_by_kind_and_tracks() {
    let f = Arc::new(busy());
    let (mut s, id) = store_with(&f);
    let d = s.document_mut(id).unwrap();
    let title = d.snapshot().title.clone();
    d.show_midi_export(ExportKind::Smf);
    assert_eq!(
        d.midi_export_destination(),
        ExportDestination::File {
            name: title,
            ext: "mid"
        }
    );
    d.show_midi_export(ExportKind::Clip);
    assert_eq!(d.midi_export_destination(), ExportDestination::Folder);
    d.edit_midi_export(|c| {
        c.lanes.remove(&30);
    });
    assert_eq!(
        d.midi_export_destination(),
        ExportDestination::File {
            name: "Chords".into(),
            ext: "midi2"
        }
    );
}

#[test]
fn a_later_export_failure_returns_to_the_sheet() {
    let f = Arc::new(busy());
    f.script_job(JobScript::failed("disk full").steps(&[0.5]));
    let (mut s, id) = store_with(&f);
    let d = s.document_mut(id).unwrap();
    d.show_midi_export(ExportKind::Smf);
    d.run_midi_export_check();
    d.start_midi_export("/tmp/x.mid", true);
    assert!(!d.midi_export().shown());
    assert_eq!(d.jobs().len(), 1, "still running");
    d.poll_jobs();
    let e = d.midi_export();
    assert!(e.shown());
    assert_eq!(e.error(), Some("disk full"));
    assert!(d.alert.is_none());
    assert_eq!(smf_exports(&f)[0].path, "/tmp/x.mid");
    assert!(smf_exports(&f)[0].replace);
}

#[test]
fn an_immediate_export_failure_keeps_the_sheet() {
    let f = Arc::new(busy());
    f.script_job(JobScript::failed("could not write"));
    let (mut s, id) = store_with(&f);
    let d = s.document_mut(id).unwrap();
    d.show_midi_export(ExportKind::Clip);
    d.run_midi_export_check();
    d.start_midi_export("/tmp/x.midi2", true);
    assert!(d.jobs().is_empty() && clip_exports(&f).len() == 1);
    assert!(d.midi_export().shown());
    assert_eq!(d.midi_export().error(), Some("could not write"));
    assert!(d.alert.is_none());
    // Refused up front: the sheet stays with the reason.
    f.set_call_error("export_midi_clip", Some(model_error("inside the project")));
    d.start_midi_export("/tmp/p.vizdaw/x.midi2", true);
    assert!(d.midi_export().shown());
    assert_eq!(d.midi_export().error(), Some("inside the project"));
    assert!(d.alert.is_none());
}

/// A cancelled export ends quietly: no sheet, no alert.
#[test]
fn a_cancelled_export_is_quiet() {
    let f = Arc::new(busy());
    f.script_job(JobScript::cancelled());
    let (mut s, id) = store_with(&f);
    let d = s.document_mut(id).unwrap();
    d.show_midi_export(ExportKind::Smf);
    d.start_midi_export("/tmp/x.mid", true);
    assert!(!d.midi_export().shown() && d.midi_export().error().is_none());
    assert!(d.alert.is_none());
}

/// Another kind's failure is its own alert, and opens no MIDI sheet.
#[test]
fn a_failed_dawproject_export_is_an_alert_and_opens_no_midi_sheet() {
    let f = Arc::new(busy());
    f.script_job(JobScript::failed("disk full"));
    let (mut s, id) = store_with(&f);
    let d = s.document_mut(id).unwrap();
    let job = vizdaw_app::jobs::FileJob::ExportDawproject(vizdaw_ffi::DawprojectExportRequestRow {
        seq: 3,
        path: "/tmp/x.dawproject".into(),
        replace: false,
        region_gain: vizdaw_ffi::RegionGainRow::Apply,
        blocks: vizdaw_ffi::blocks::BlockRule::WholeLane,
    });
    d.start_job(job.kind(), "DAWproject Export Failed", |s| job.start(s));
    let a = d.alert.clone().expect("an alert");
    assert_eq!(
        (a.title.as_str(), a.message.as_str()),
        ("DAWproject Export Failed", "disk full")
    );
    assert!(!d.midi_export().shown() && d.midi_export().error().is_none());
}

#[test]
fn an_exports_report_is_kept_to_show() {
    let f = Arc::new(busy());
    f.script_job(JobScript::done().report(report("Exported 2 tracks")));
    let (mut s, id) = store_with(&f);
    let d = s.document_mut(id).unwrap();
    d.show_midi_export(ExportKind::Smf);
    d.start_midi_export("/tmp/x.mid", true);
    assert!(d.report_shown());
    assert_eq!(d.last_report(), Some(&report("Exported 2 tracks")));
    assert!(!d.midi_export().shown());
}

/// Several clip tracks go into a folder, which never replaces files
/// silently: the core names the files and says which are taken; the
/// export replaces only when the user chose Replace.
#[test]
fn a_clip_folder_export_asks_before_replacing() {
    let f = Arc::new(busy());
    let (mut s, id) = store_with(&f);
    let d = s.document_mut(id).unwrap();
    let names = |taken: &[&str]| ClipNamesRow {
        names: vec!["01 Groove.midi2".into(), "02 Chords.midi2".into()],
        taken: taken.iter().map(|t| (*t).to_owned()).collect(),
    };
    d.show_midi_export(ExportKind::Clip);
    f.set_clip_names(names(&[]));
    d.export_midi_to_folder("/tmp/out");
    assert!(
        clip_exports(&f).is_empty(),
        "asked first, off the UI thread"
    );
    d.run_clip_names();
    let asked = f.clip_name_requests().pop().unwrap();
    assert_eq!(
        (asked.path.as_str(), asked.lanes.as_slice(), asked.replace),
        ("/tmp/out", [30, 31].as_slice(), false)
    );
    let e = clip_exports(&f).pop().unwrap();
    assert!(e.path == "/tmp/out" && !e.replace);
    assert!(d.midi_export().replace_question().is_none());
    // One of the names is taken: asked, nothing written yet.
    d.show_midi_export(ExportKind::Clip);
    f.set_clip_names(names(&["02 Chords.midi2"]));
    d.export_midi_to_folder("/tmp/out");
    d.run_clip_names();
    assert_eq!(clip_exports(&f).len(), 1);
    assert!(d.midi_export().shown());
    let q = d.midi_export().replace_question().unwrap().clone();
    assert_eq!(q.taken, ["02 Chords.midi2"]);
    assert_eq!(q.message(), "1 file already exists in out. Replace it?");
    d.answer_midi_export_replace(false);
    assert_eq!(clip_exports(&f).len(), 1);
    assert!(d.midi_export().replace_question().is_none() && d.midi_export().shown());
    f.set_clip_names(names(&["01 Groove.midi2", "02 Chords.midi2"]));
    d.export_midi_to_folder("/tmp/out");
    d.run_clip_names();
    assert_eq!(
        d.midi_export().replace_question().unwrap().message(),
        "2 files already exist in out. Replace them?"
    );
    d.answer_midi_export_replace(true);
    let e = clip_exports(&f).pop().unwrap();
    assert!(clip_exports(&f).len() == 2 && e.replace && e.path == "/tmp/out");
    assert!(d.midi_export().replace_question().is_none());
    // A second answer does nothing.
    d.answer_midi_export_replace(true);
    assert_eq!(clip_exports(&f).len(), 2);
    // The core's refusal (a file that appeared meanwhile) shows in the sheet.
    d.show_midi_export(ExportKind::Clip);
    f.set_clip_names(names(&[]));
    f.set_call_error(
        "export_midi_clip",
        Some(model_error(
            "01 Groove.midi2 exists; choose another name or replace it",
        )),
    );
    d.export_midi_to_folder("/tmp/out");
    d.run_clip_names();
    assert!(d.midi_export().shown());
    assert_eq!(
        d.midi_export().error(),
        Some("01 Groove.midi2 exists; choose another name or replace it")
    );
    assert!(d.alert.is_none());
    f.set_call_error("export_midi_clip", None);
    // A folder the core refuses: the reason, nothing started.
    f.fail_next("export_midi_clip_names", model_error("out is not a folder"));
    d.export_midi_to_folder("/tmp/out");
    d.run_clip_names();
    assert!(d.midi_export().shown());
    assert_eq!(d.midi_export().error(), Some("out is not a folder"));
    assert_eq!(clip_exports(&f).len(), 2);
}

/// A names answer from before the sheet was shown again (or a newer
/// question) is dropped: it starts nothing and asks nothing.
#[test]
fn a_stale_names_answer_is_dropped() {
    let f = Arc::new(busy());
    let (mut s, id) = store_with(&f);
    let d = s.document_mut(id).unwrap();
    d.show_midi_export(ExportKind::Clip);
    f.set_clip_names(ClipNamesRow {
        names: vec!["01 Groove.midi2".into()],
        taken: vec!["01 Groove.midi2".into()],
    });
    d.export_midi_to_folder("/tmp/out");
    let ask = d.take_clip_names().unwrap();
    let answer = ask.run(d.session().as_ref());
    d.show_midi_export(ExportKind::Clip);
    d.land_clip_names(answer);
    assert!(d.midi_export().replace_question().is_none());
    assert!(clip_exports(&f).is_empty());
}

#[test]
fn the_sheets_texts() {
    assert_eq!(ExportKind::Smf.title(), "Export MIDI File");
    assert_eq!(ExportKind::Clip.title(), "Export MIDI Clip File");
    assert_eq!(
        ExportKind::Smf.subtitle(),
        "Notes, controllers, tempo and markers as a Standard MIDI File"
    );
    assert_eq!(
        ExportKind::Clip.subtitle(),
        "One track, lossless, as a MIDI 2.0 Clip File"
    );
    assert_eq!(export_track_label("Groove"), "Export track Groove");
    let q = ReplaceQuestion {
        folder: "/a/b/Clips".into(),
        taken: vec!["x".into(), "y".into(), "z".into()],
    };
    assert_eq!(q.message(), "3 files already exist in Clips. Replace them?");
}

/// The dry run's groups: Conflicts, Changed, Left out, Notes (empty ones
/// left out); a line counts what happened more than once.
#[test]
fn wont_transfer_groups_in_swifts_order() {
    let item = |kind, what: &str, count| ReportItemRow {
        kind,
        what: what.into(),
        count,
        details: vec![],
    };
    let r = ReportRow {
        summary: String::new(),
        items: vec![
            item(ReportKindRow::Note, "n", 1),
            item(ReportKindRow::LeftOut, "Plug-ins", 3),
            item(ReportKindRow::Conflict, "c", 1),
            item(ReportKindRow::LeftOut, "Sends", 1),
        ],
    };
    let g = report_text::groups(&r);
    let titles: Vec<&str> = g.iter().map(|(t, _)| *t).collect();
    assert_eq!(titles, ["Conflicts", "Left out", "Notes"]);
    let lines: Vec<String> = g[1].1.iter().map(|i| report_text::line(i)).collect();
    assert_eq!(lines, ["Plug-ins (3)", "Sends"]);
    assert!(report_text::groups(&report("x")).is_empty());
}
