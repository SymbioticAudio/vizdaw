//! The import and export report's model (slice 7c Task 3; Swift
//! `InterchangeTests`' ReportText rows and `anExportsReportKeepsWhatItWrote`):
//! the wording shared by the sheet, Copy and Save as Text (groups in
//! Swift's order, empty ones omitted, details capped with "and N more"),
//! the sheet's title, what an export wrote (Show in Finder), and File ›
//! Last Import or Export Report….

mod support;

use std::sync::Arc;

use support::{FakeSession, demo_fake, lane_row};
use vizdaw_app::interchange::ExportKind;
use vizdaw_app::report_text::{self, NO_REPORT_YET};
use vizdaw_app::testing::{FakeCore, JobScript};
use vizdaw_app::{DocId, DocumentStore};
use vizdaw_ffi::{ReportItemRow, ReportKindRow, ReportRow};

fn item(kind: ReportKindRow, what: &str, count: u64, details: &[&str]) -> ReportItemRow {
    ReportItemRow {
        kind,
        what: what.into(),
        count,
        details: details.iter().map(|d| d.to_string()).collect(),
    }
}

fn report(summary: &str) -> ReportRow {
    ReportRow {
        summary: summary.into(),
        items: vec![],
    }
}

fn busy() -> FakeSession {
    let f = demo_fake();
    let mut a = lane_row(30, "Groove", false);
    a.midi_event_count = 12;
    f.set_lanes(3, vec![a, lane_row(31, "Chords", false)]);
    f
}

fn store_with(fake: &Arc<FakeSession>) -> (DocumentStore, DocId) {
    let core = Arc::new(FakeCore::new());
    let mut s = DocumentStore::with_core(core.clone());
    core.hand_out("new_demo", fake.clone());
    let id = s.new_demo().unwrap();
    (s, id)
}

/// Swift `reportTextGroupsAndCaps`.
#[test]
fn report_text_groups_and_caps() {
    let bars: Vec<String> = (1..=50).map(|i| format!("bar {i}")).collect();
    let bars: Vec<&str> = bars.iter().map(String::as_str).collect();
    let r = ReportRow {
        summary: "Imported 2 tracks".into(),
        items: vec![
            item(ReportKindRow::Note, "The file's title was not kept", 1, &[]),
            item(ReportKindRow::LeftOut, "Muted notes", 120, &bars),
            item(ReportKindRow::Changed, "Tempo", 1, &["bar 3"]),
        ],
    };
    let g = report_text::groups(&r);
    let titles: Vec<&str> = g.iter().map(|(t, _)| *t).collect();
    assert_eq!(titles, ["Changed", "Left out", "Notes"]);
    let left = g[1].1[0];
    assert_eq!(report_text::line(left), "Muted notes (120)");
    assert_eq!(report_text::line(g[0].1[0]), "Tempo");
    let d = report_text::details(left);
    assert_eq!(d.len(), 51);
    assert_eq!(d.first().map(String::as_str), Some("bar 1"));
    assert_eq!(d.last().map(String::as_str), Some("and 70 more"));
    assert_eq!(report_text::details(g[0].1[0]), ["bar 3"]);
    let only = ReportRow {
        summary: "Exported 1 track".into(),
        items: vec![item(ReportKindRow::Note, "A note", 1, &[])],
    };
    let titles: Vec<&str> = report_text::groups(&only).iter().map(|(t, _)| *t).collect();
    assert_eq!(titles, ["Notes"], "empty groups are omitted");
    let plain = report_text::plain(&r);
    assert!(plain.starts_with("Imported 2 tracks\n"));
    assert!(plain.contains("Left out\n") && plain.contains("  Muted notes (120)\n    bar 1\n"));
    assert!(plain.contains("    and 70 more"));
    assert_eq!(
        report_text::plain(&report("Exported 3 tracks")),
        "Exported 3 tracks\nEverything transferred."
    );
    assert!(!plain.to_lowercase().contains("lane"));
}

/// Swift `conflictsComeFirst`: a merge's Conflicts lead, on the sheet and
/// in the text.
#[test]
fn conflicts_come_first() {
    let r = ReportRow {
        summary: "Merged: 1 applied, 0 kept, 0 added, 0 removed, 1 in conflict".into(),
        items: vec![
            item(ReportKindRow::Note, "A note", 1, &[]),
            item(ReportKindRow::LeftOut, "Left", 1, &[]),
            item(ReportKindRow::Changed, "Changed", 1, &[]),
            item(
                ReportKindRow::Conflict,
                "Drums: level changed in both",
                1,
                &[],
            ),
        ],
    };
    let titles: Vec<&str> = report_text::groups(&r).iter().map(|(t, _)| *t).collect();
    assert_eq!(titles, ["Conflicts", "Changed", "Left out", "Notes"]);
    assert_eq!(
        report_text::plain(&r),
        "Merged: 1 applied, 0 kept, 0 added, 0 removed, 1 in conflict\n\
         Conflicts\n  Drums: level changed in both\n\
         Changed\n  Changed\n\
         Left out\n  Left\n\
         Notes\n  A note"
    );
}

/// Swift `moreIsOnlyAfterGivenDetails`: "and N more" follows given
/// details only, as the core's `Report::plain` does.
#[test]
fn more_is_only_after_given_details() {
    let bare = item(ReportKindRow::LeftOut, "Muted notes", 120, &[]);
    assert!(report_text::details(&bare).is_empty());
    let some = item(
        ReportKindRow::LeftOut,
        "Muted notes",
        5,
        &["bar 1", "bar 2"],
    );
    assert_eq!(
        report_text::details(&some),
        ["bar 1", "bar 2", "and 3 more"]
    );
    let r = ReportRow {
        summary: "Exported".into(),
        items: vec![bare],
    };
    assert_eq!(
        report_text::plain(&r),
        "Exported\nLeft out\n  Muted notes (120)"
    );
}

/// Swift `ReportSheet.title`: what was written, or the summary's start.
#[test]
fn the_title_follows_the_summary_and_what_was_written() {
    let t = |s: &str, w: &[&str]| {
        let w: Vec<String> = w.iter().map(|p| p.to_string()).collect();
        report_text::title(&report(s), &w)
    };
    assert_eq!(t("Exported 2 tracks", &[]), "Export Report");
    assert_eq!(t("Imported 2 tracks", &["/tmp/x.mid"]), "Export Report");
    assert_eq!(t("Imported 2 tracks", &[]), "Import Report");
    assert_eq!(t("Merged: 1 applied", &[]), "Merge Report");
    assert_eq!(t("Nothing to merge: the files match", &[]), "Merge Report");
    assert_eq!(t("Something else", &[]), "Report");
    assert_eq!(t("", &[]), "Report");
    assert_eq!(report_text::TITLES.len(), 4);
}

#[test]
fn the_sheets_strings() {
    assert_eq!(
        report_text::EVERYTHING_TRANSFERRED,
        "Everything transferred."
    );
    assert_eq!(report_text::COPY, "Copy");
    assert_eq!(report_text::COPY_LABEL, "Copy the report");
    assert_eq!(report_text::SAVE_AS_TEXT, "Save as Text\u{2026}");
    assert_eq!(report_text::SAVE_LABEL, "Save the report as text");
    assert_eq!(report_text::DONE, "Done");
    assert_eq!(NO_REPORT_YET, "There is no import or export report yet.");
    // The owner's ruling: Linux says "Show in Folder".
    if cfg!(target_os = "macos") {
        assert_eq!(report_text::SHOW_FILES, "Show in Finder");
        assert_eq!(
            report_text::SHOW_FILES_LABEL,
            "Show the exported files in Finder"
        );
    } else {
        assert_eq!(report_text::SHOW_FILES, "Show in Folder");
        assert_eq!(
            report_text::SHOW_FILES_LABEL,
            "Show the exported files in their folder"
        );
    }
}

/// A group's heading is `<title> (<count>)`, its accessibility label
/// `<title>, <count>`; the count adds up its items' counts.
#[test]
fn a_groups_heading_counts_its_items() {
    let items = [
        item(ReportKindRow::LeftOut, "Muted notes", 120, &[]),
        item(ReportKindRow::LeftOut, "Audio regions", 3, &[]),
    ];
    let refs: Vec<&ReportItemRow> = items.iter().collect();
    assert_eq!(report_text::group_count(&refs), 123);
    assert_eq!(
        report_text::group_heading("Left out", &refs),
        "Left out (123)"
    );
    assert_eq!(report_text::group_label("Left out", &refs), "Left out, 123");
}

/// Swift `anExportsReportKeepsWhatItWrote`: the report sheet offers Show
/// in Finder for what an export wrote; the same report again keeps it,
/// another one has none.
#[test]
fn an_exports_report_keeps_what_it_wrote() {
    let f = Arc::new(busy());
    let r = report("Exported 2 tracks");
    f.script_job(JobScript::done().report(r.clone()).written(&["/tmp/x.mid"]));
    let (mut s, id) = store_with(&f);
    let d = s.document_mut(id).unwrap();
    d.show_midi_export(ExportKind::Smf);
    d.start_midi_export("/tmp/x.mid", true);
    d.poll_jobs();
    assert!(d.report_shown());
    assert_eq!(d.report_written(), ["/tmp/x.mid"]);
    // The same report again keeps them.
    f.set_interchange_report(Some(r.clone()));
    d.hide_report();
    d.show_last_report();
    assert!(d.report_shown());
    assert_eq!(d.report_written(), ["/tmp/x.mid"]);
    // Another one has none.
    let other = report("Imported 1 track");
    f.set_interchange_report(Some(other.clone()));
    d.show_last_report();
    assert_eq!(d.last_report(), Some(&other));
    assert!(d.report_written().is_empty());
}

/// Only an export's files are kept: an import's report has none to show.
#[test]
fn an_imports_report_keeps_no_files() {
    let f = Arc::new(busy());
    f.script_job(
        JobScript::done()
            .report(report("Exported 2 tracks"))
            .written(&["/tmp/x.mid"]),
    );
    f.script_job(
        JobScript::done()
            .report(report("Imported 1 track"))
            .written(&["/tmp/ignored"]),
    );
    let (mut s, id) = store_with(&f);
    let d = s.document_mut(id).unwrap();
    d.show_midi_export(ExportKind::Smf);
    d.start_midi_export("/tmp/x.mid", true);
    d.poll_jobs();
    assert_eq!(d.report_written(), ["/tmp/x.mid"]);
    let job = vizdaw_app::jobs::FileJob::ImportMidi(vizdaw_ffi::MidiImportRequestRow {
        path: "/tmp/a.mid".into(),
        new_project: false,
        seq: 3,
        lane: None,
        tick: 0,
        channel_map: vec![],
        split_channels: None,
        mpe: false,
        use_tempo: false,
    });
    d.start_job(job.kind(), "MIDI Import Failed", |s| job.start(s));
    d.poll_jobs();
    assert_eq!(d.last_report(), Some(&report("Imported 1 track")));
    assert!(d.report_written().is_empty());
}

/// Swift `showLastReport`: none at all is the status line, and nothing
/// shows.
#[test]
fn no_report_yet_is_the_status_line() {
    let f = Arc::new(busy());
    let (mut s, id) = store_with(&f);
    let d = s.document_mut(id).unwrap();
    d.show_last_report();
    assert!(!d.report_shown());
    assert_eq!(d.status(), Some(NO_REPORT_YET));
    assert!(d.last_report().is_none());
}

/// The session's last report is shown (it may be newer than the UI's,
/// made on the channel); without one, the UI's own.
#[test]
fn the_last_report_is_the_sessions_else_the_uis() {
    let f = Arc::new(busy());
    let (mut s, id) = store_with(&f);
    let channel = report("Imported 3 tracks");
    f.set_interchange_report(Some(channel.clone()));
    let d = s.document_mut(id).unwrap();
    d.show_last_report();
    assert!(d.report_shown());
    assert_eq!(d.last_report(), Some(&channel));
    // The session forgot it (or cannot say): the UI's own shows.
    f.set_interchange_report(None);
    d.hide_report();
    d.show_last_report();
    assert!(d.report_shown());
    assert_eq!(d.last_report(), Some(&channel));
}

/// A merge's report (Swift `InterchangeModel.show`): shown as an
/// import's is, with nothing written.
#[test]
fn a_merge_report_shows_with_nothing_written() {
    let f = Arc::new(busy());
    f.script_job(
        JobScript::done()
            .report(report("Exported 2 tracks"))
            .written(&["/tmp/x.mid"]),
    );
    let (mut s, id) = store_with(&f);
    let d = s.document_mut(id).unwrap();
    d.show_midi_export(ExportKind::Smf);
    d.start_midi_export("/tmp/x.mid", true);
    d.poll_jobs();
    d.hide_report();
    let merged = report("Merged: 1 applied");
    d.show_report(merged.clone());
    assert!(d.report_shown());
    assert_eq!(d.last_report(), Some(&merged));
    assert!(d.report_written().is_empty());
}

/// Save as Text…'s failure names the file.
#[test]
fn a_failed_save_names_the_file() {
    assert_eq!(
        report_text::save_failed_title(std::path::Path::new("/x/MIDI Report.txt")),
        "The file \u{201c}MIDI Report.txt\u{201d} couldn\u{2019}t be saved."
    );
}

/// Show in Finder shows each folder written into once, with its first
/// file.
#[test]
fn show_in_finder_shows_each_folder_once() {
    let w = |p: &[&str]| p.iter().map(|s| s.to_string()).collect::<Vec<_>>();
    assert_eq!(report_text::reveal_paths(&w(&["/o/x.mid"])), ["/o/x.mid"]);
    assert_eq!(
        report_text::reveal_paths(&w(&["/o/01 A.midi2", "/o/02 B.midi2", "/p/c.wav"])),
        ["/o/01 A.midi2", "/p/c.wav"]
    );
    assert!(report_text::reveal_paths(&[]).is_empty());
}
