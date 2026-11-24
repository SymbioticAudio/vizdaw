//! The File menu's enablement (slice 7b Task 3; Swift `AppCommands`'s
//! File groups) and the New Project format panel's choice (Swift
//! `FormatPanel`).

mod support;

use std::sync::Arc;

use core_control::registry::ArgValue;
use support::{FakeSession, demo_fake};
use vizdaw_app::DocumentModel;
use vizdaw_app::commands::{FILE_COMMANDS, file_enabled};
use vizdaw_app::formats::{
    BIT_DEPTH_LABEL, DEPTHS, FALLBACK_FORMAT, FormatChoice, NEW_PROJECT_CREATE,
    NEW_PROJECT_MESSAGE, NEW_PROJECT_TITLE, SAMPLE_RATE_LABEL, depth_title, rate_title,
};
use vizdaw_app::interchange_commands::{self as ic, InterchangeRequest};
use vizdaw_app::testing::JobScript;
use vizdaw_ffi::session::CopiesSizeRow;
use vizdaw_ffi::{
    BitDepthRow, MediaOriginRow, MediaRow, OfflineReasonRow, ProjectFormatRow, ReportRow,
};

fn doc(f: &Arc<FakeSession>) -> DocumentModel {
    DocumentModel::new(f.clone()).unwrap()
}

fn media(id: u64, offline: Option<OfflineReasonRow>) -> MediaRow {
    MediaRow {
        id,
        name: format!("m{id}.wav"),
        path: format!("Audio/m{id}.caf"),
        channels: 2,
        frames: 48_000,
        sample_rate: 48_000,
        origin: MediaOriginRow::Imported,
        original_name: None,
        offline,
        uses: 1,
        used_percent: 100.0,
        peak: None,
    }
}

fn report() -> ReportRow {
    ReportRow {
        summary: "Exported 2 tracks".into(),
        items: vec![],
    }
}

/// The items enabled without a document and with one, before any state.
#[test]
fn without_a_document_only_the_new_projects_the_opens_import_project_and_close_run() {
    let always = [
        "file.new",
        "file.open",
        "file.import_dawproject",
        "file.import_aaf",
        "file.create_demo",
        "file.create_demo_audio",
        "file.close",
    ];
    for id in FILE_COMMANDS {
        assert_eq!(file_enabled(id, None), always.contains(id), "{id}");
    }
    let f = Arc::new(demo_fake());
    let d = doc(&f);
    let state_bound = ["file.open_copy", "file.interchange_report", "media.relink"];
    for id in FILE_COMMANDS {
        assert_eq!(
            file_enabled(id, Some(&d)),
            !state_bound.contains(id),
            "{id}"
        );
    }
    assert!(!file_enabled("media.convert", Some(&d)), "not a menu item");
}

#[test]
fn open_copy_needs_a_saved_project_with_copies() {
    let copies = |n| CopiesSizeRow { count: n, bytes: 0 };
    // Copies, but untitled.
    let f = Arc::new(demo_fake());
    f.set_copies_size(copies(2));
    assert!(!file_enabled("file.open_copy", Some(&doc(&f))));
    // Saved, no copies.
    let f = Arc::new(demo_fake());
    f.set_path(Some("/m/Song.vizdaw"));
    f.set_copies_size(copies(0));
    assert!(!file_enabled("file.open_copy", Some(&doc(&f))));
    // Saved with copies.
    let f = Arc::new(demo_fake());
    f.set_path(Some("/m/Song.vizdaw"));
    f.set_copies_size(copies(1));
    assert!(file_enabled("file.open_copy", Some(&doc(&f))));
}

#[test]
fn the_copies_are_counted_at_most_once_a_second() {
    let f = Arc::new(demo_fake());
    f.set_path(Some("/m/Song.vizdaw"));
    f.set_copies_size(CopiesSizeRow { count: 1, bytes: 0 });
    let d = doc(&f);
    for _ in 0..5 {
        assert!(d.has_copies());
    }
    assert_eq!(f.reads("co_producer_copies_size"), 1);
}

#[test]
fn the_report_item_waits_for_a_report_the_ui_kept() {
    let f = Arc::new(demo_fake());
    // The session's own report does not enable it (Swift reads the UI's).
    f.set_interchange_report(Some(report()));
    let mut d = doc(&f);
    assert!(!file_enabled("file.interchange_report", Some(&d)));
    // An export the UI started ends well with a report.
    f.script_job(JobScript::done().report(report()));
    let args = vec![("path".to_owned(), ArgValue::Text("/m/Song.mid".into()))];
    let Ok(InterchangeRequest::Job(job)) = ic::request(ic::EXPORT_MIDI, &args, &d) else {
        panic!("an export job");
    };
    d.start_job(job.kind(), job.refusal_title(), |s| job.start(s))
        .expect("started");
    d.poll_jobs();
    assert_eq!(d.last_report(), Some(&report()));
    assert!(file_enabled("file.interchange_report", Some(&d)));
}

#[test]
fn a_job_without_a_report_or_of_another_kind_keeps_none() {
    let f = Arc::new(demo_fake());
    let mut d = doc(&f);
    // An audio import's report is not an interchange report.
    f.script_job(JobScript::done().report(report()));
    d.import_audio_at(vec!["/m/a.wav".into()], None);
    d.poll_jobs();
    assert_eq!(d.last_report(), None);
}

#[test]
fn relink_needs_a_relinkable_item_and_no_held_merge() {
    let f = Arc::new(demo_fake());
    // Another format or an unreadable file is Convert's, not Relink's.
    f.set_media(vec![
        media(1, None),
        media(2, Some(OfflineReasonRow::Format)),
        media(3, Some(OfflineReasonRow::Unreadable)),
    ]);
    let mut d = doc(&f);
    assert!(!file_enabled("media.relink", Some(&d)));
    for reason in [
        OfflineReasonRow::Missing,
        OfflineReasonRow::Length,
        OfflineReasonRow::Changed,
    ] {
        let f = Arc::new(demo_fake());
        f.set_media(vec![media(1, None), media(4, Some(reason))]);
        let d = doc(&f);
        assert_eq!(d.relinkable().iter().map(|r| r.id).collect::<Vec<_>>(), [4]);
        assert!(file_enabled("media.relink", Some(&d)), "{reason:?}");
    }
    // The rows are read again once the media version moves.
    f.set_media(vec![media(5, Some(OfflineReasonRow::Missing))]);
    assert!(!file_enabled("media.relink", Some(&d)), "cached");
    d.refresh_after_job();
    assert!(file_enabled("media.relink", Some(&d)));
    d.set_merge_held(true);
    assert!(!file_enabled("media.relink", Some(&d)), "a held merge");
    d.set_merge_held(false);
    assert!(file_enabled("media.relink", Some(&d)));
}

#[test]
fn the_format_panel_has_swifts_strings_rates_and_depths() {
    assert_eq!(NEW_PROJECT_TITLE, "New Project");
    assert_eq!(
        NEW_PROJECT_MESSAGE,
        "Every audio file of the project is stored in this format."
    );
    assert_eq!(NEW_PROJECT_CREATE, "Create");
    assert_eq!(SAMPLE_RATE_LABEL, "Sample rate:");
    assert_eq!(BIT_DEPTH_LABEL, "Bit depth:");
    assert_eq!(rate_title(44_100), "44.1 kHz");
    assert_eq!(rate_title(48_000), "48 kHz");
    assert_eq!(rate_title(88_200), "88.2 kHz");
    assert_eq!(rate_title(192_000), "192 kHz");
    assert_eq!(
        DEPTHS.map(|(d, _)| depth_title(d)),
        ["16-bit", "24-bit", "32-bit float"]
    );
    let c = FormatChoice::new(vizdaw_ffi::project_rates(), FALLBACK_FORMAT);
    assert_eq!(
        c.rate_titles(),
        vizdaw_ffi::project_rates()
            .into_iter()
            .map(rate_title)
            .collect::<Vec<_>>()
    );
    assert_eq!(c.depth_titles(), ["16-bit", "24-bit", "32-bit float"]);
}

#[test]
fn the_format_panel_starts_at_the_default_and_takes_what_is_chosen() {
    let initial = ProjectFormatRow {
        sample_rate: 96_000,
        depth: BitDepthRow::Float32,
    };
    let mut c = FormatChoice::new(vec![44_100, 48_000, 96_000], initial);
    assert_eq!(c.format, initial);
    c.choose_rate("44.1 kHz");
    c.choose_depth("16-bit");
    assert_eq!(
        c.format,
        ProjectFormatRow {
            sample_rate: 44_100,
            depth: BitDepthRow::Int16,
        }
    );
    // Not one of its items: nothing changes.
    c.choose_rate("12 kHz");
    c.choose_depth("8-bit");
    assert_eq!(c.format.sample_rate, 44_100);
    assert_eq!(c.format.depth, BitDepthRow::Int16);
    // A rate it does not offer starts at the first one.
    let c = FormatChoice::new(
        vec![44_100, 48_000],
        ProjectFormatRow {
            sample_rate: 22_050,
            depth: BitDepthRow::Int24,
        },
    );
    assert_eq!(c.format.sample_rate, 44_100);
}
