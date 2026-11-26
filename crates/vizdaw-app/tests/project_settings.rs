//! Project Settings (slice 7b Task 4; Swift `ProjectSettingsModelTests`'s
//! Project Settings tests): the sheet loads the project's format and song
//! info, says what a format change converts, and OK sends only what
//! changed — one `set_song_info`, the format job — waiting while a held
//! merge blocks a format change. The Swift suite's region-name tests
//! belong to the region inspector (not in this slice).

mod support;

use std::sync::Arc;

use support::{FakeSession, demo_fake};
use vizdaw_app::DocumentModel;
use vizdaw_app::project_settings::{
    COMMENT_LABEL, FIELDS, FORMAT_SECTION, HELD_HELP, NO_AUDIO_NOTE, ProjectSettings, SONG_SECTION,
    SongText, TITLE, note,
};
use vizdaw_app::testing::{JobRequest, JobScript};
use vizdaw_ffi::{
    BitDepthRow, JobKindRow, MediaOriginRow, MediaRow, ProjectFormatRow, ProjectInfoRow,
    SongInfoRow, VizError,
};

fn fake() -> Arc<FakeSession> {
    Arc::new(demo_fake())
}

fn doc(f: &Arc<FakeSession>) -> DocumentModel {
    DocumentModel::new(f.clone()).unwrap()
}

fn media(id: u64, frames: u64) -> MediaRow {
    MediaRow {
        id,
        name: format!("m{id}.wav"),
        path: format!("Audio/m{id}.caf"),
        channels: 2,
        frames,
        sample_rate: 48_000,
        origin: MediaOriginRow::Imported,
        original_name: None,
        offline: None,
        uses: 1,
        used_percent: 100.0,
        peak: None,
    }
}

fn format(rate: u32, depth: BitDepthRow) -> ProjectFormatRow {
    ProjectFormatRow {
        sample_rate: rate,
        depth,
    }
}

fn formats_started(f: &FakeSession) -> Vec<ProjectFormatRow> {
    f.jobs_started()
        .into_iter()
        .filter_map(|j| match j.request {
            JobRequest::Format(fm) => Some(fm),
            _ => None,
        })
        .collect()
}

fn refusal() -> VizError {
    VizError::Model {
        message: "no".into(),
    }
}

/// Swift `applySendsOnlyWhatChanged`.
#[test]
fn apply_sends_only_what_changed() {
    let f = fake();
    f.set_song(SongInfoRow {
        title: Some("Old".into()),
        ..SongInfoRow::default()
    });
    let mut m = doc(&f);
    let mut p = ProjectSettings::load(&mut m);
    assert_eq!(p.song.title, "Old");
    assert_eq!(p.song.comment, "");
    assert_eq!(p.format(), format(48_000, BitDepthRow::Int24));
    p.apply(&mut m);
    assert_eq!(f.count("set_song_info"), 0);
    assert!(formats_started(&f).is_empty());
    p.song.title = "New".into();
    p.apply(&mut m);
    assert_eq!(f.count("set_song_info"), 1);
    assert_eq!(f.song().title.as_deref(), Some("New"));
    assert_eq!(f.song().artist, None);
    assert!(formats_started(&f).is_empty());
    p.apply(&mut m);
    assert_eq!(f.count("set_song_info"), 1, "applied once");
    p.rate = 96_000;
    p.apply(&mut m);
    assert_eq!(formats_started(&f), [format(96_000, BitDepthRow::Int24)]);
    assert_eq!(f.count("set_song_info"), 1);
    m.poll_jobs();
    let mut p = ProjectSettings::load(&mut m);
    p.song.composer = "C".into();
    p.depth = BitDepthRow::Int16;
    p.apply(&mut m);
    assert_eq!(f.count("set_song_info"), 2);
    assert_eq!(formats_started(&f).len(), 2);
    // Emptying a field clears it.
    let mut p = ProjectSettings::load(&mut m);
    p.song.composer = String::new();
    p.apply(&mut m);
    assert_eq!(f.song().composer, None);
}

/// Swift `showLoadsAndSaysWhatAFormatChangeConverts`: the sheet opens
/// loaded, with the project's format and the note.
#[test]
fn load_reads_the_format_the_song_info_and_the_note() {
    let f = fake();
    f.set_song(SongInfoRow {
        title: Some("Song".into()),
        comment: Some("two\nlines".into()),
        ..SongInfoRow::default()
    });
    f.set_project_info(ProjectInfoRow {
        format: format(44_100, BitDepthRow::Float32),
        mismatched: 0,
        rate_mismatch: false,
        device_rate: 0.0,
    });
    let mut m = doc(&f);
    let p = ProjectSettings::load(&mut m);
    assert_eq!(p.song.title, "Song");
    assert_eq!(p.song.comment, "two\nlines");
    assert_eq!(p.rate, 44_100);
    assert_eq!(p.depth, BitDepthRow::Float32);
    assert_eq!(p.media_note, NO_AUDIO_NOTE);
    f.set_media(vec![media(1, 48_000 * 90), media(2, 48_000 * 30)]);
    m.refresh_after_job();
    let p = ProjectSettings::load(&mut m);
    assert_eq!(
        p.media_note,
        "Changing the format converts 2 audio files (2 min of audio) into new files (one undo step)."
    );
    assert!(m.alert.is_none());
}

#[test]
fn the_note_counts_files_and_their_length() {
    assert_eq!(note(&[]), "The project has no audio files yet.");
    assert_eq!(
        note(&[media(1, 48_000 * 5)]),
        "Changing the format converts 1 audio file (5 s of audio) into new files (one undo step)."
    );
    assert_eq!(
        note(&[media(1, 48_000 * 59), media(2, 48_000)]),
        "Changing the format converts 2 audio files (1 min of audio) into new files (one undo step)."
    );
    // A rate of 0 counts as 1 (Swift `max(sampleRate, 1)`).
    let mut odd = media(1, 3);
    odd.sample_rate = 0;
    assert_eq!(
        note(&[odd]),
        "Changing the format converts 1 audio file (3 s of audio) into new files (one undo step)."
    );
}

/// The format change is the core's format job ("Converting media…"); a
/// refused start is the alert "Project Settings Failed" and OK tries again.
#[test]
fn a_format_change_starts_the_format_job_and_a_refusal_is_an_alert() {
    let f = fake();
    f.script_job(JobScript::done().steps(&[0.5]));
    let mut m = doc(&f);
    let mut p = ProjectSettings::load(&mut m);
    p.depth = BitDepthRow::Float32;
    p.apply(&mut m);
    let started = f.jobs_started();
    assert_eq!(started.len(), 1);
    assert_eq!(started[0].kind, JobKindRow::Format);
    assert_eq!(m.banners()[0].text(), "Converting media…");
    m.poll_jobs();
    // Refused: an alert, and the change is still to send.
    f.fail_next("set_project_format", refusal());
    let mut p = ProjectSettings::load(&mut m);
    p.rate = 96_000;
    p.apply(&mut m);
    assert_eq!(m.alert.as_ref().unwrap().title, "Project Settings Failed");
    m.alert = None;
    p.apply(&mut m);
    assert_eq!(
        formats_started(&f).last(),
        Some(&format(96_000, BitDepthRow::Float32))
    );
    assert!(m.alert.is_none());
}

/// A held DAWproject merge holds a format change back (OK waits, with
/// Swift's help); a song-info change alone is not blocked.
#[test]
fn a_held_merge_blocks_a_format_change_only() {
    let f = fake();
    let mut m = doc(&f);
    m.set_merge_held(true);
    let mut p = ProjectSettings::load(&mut m);
    assert!(!p.format_blocked(&m));
    p.song.genre = "Jazz".into();
    assert!(!p.format_blocked(&m), "song info only");
    p.rate = 96_000;
    assert!(p.format_blocked(&m));
    p.rate = 48_000;
    assert!(!p.format_blocked(&m), "back to the project's format");
    p.rate = 96_000;
    m.set_merge_held(false);
    assert!(!p.format_blocked(&m));
    assert_eq!(
        HELD_HELP,
        "Finish or cancel the DAWproject merge waiting to be confirmed first."
    );
}

/// A failed read or song-info write is the alert "Project Settings
/// Failed"; a failed write keeps the change to send.
#[test]
fn failures_are_project_settings_failed_alerts() {
    let f = fake();
    let mut m = doc(&f);
    f.fail_next("song_info", refusal());
    let p = ProjectSettings::load(&mut m);
    assert_eq!(m.alert.take().unwrap().title, "Project Settings Failed");
    assert_eq!(p.format(), format(48_000, BitDepthRow::Int24));
    let mut p = ProjectSettings::load(&mut m);
    p.song.year = "1999".into();
    f.fail_next("set_song_info", refusal());
    p.apply(&mut m);
    assert_eq!(m.alert.take().unwrap().title, "Project Settings Failed");
    p.apply(&mut m);
    assert_eq!(f.song().year.as_deref(), Some("1999"));
    assert!(m.alert.is_none());
}

/// The 13 texts: an empty one is no value, both ways; the sheet's
/// strings are Swift's.
#[test]
fn song_text_round_trips_and_the_sheet_uses_swifts_strings() {
    let row = SongInfoRow {
        title: Some("T".into()),
        artist: Some("A".into()),
        album: Some("Al".into()),
        original_artist: Some("O".into()),
        composer: Some("C".into()),
        songwriter: Some("S".into()),
        producer: Some("P".into()),
        arranger: Some("Ar".into()),
        year: Some("Y".into()),
        genre: Some("G".into()),
        copyright: Some("Co".into()),
        website: Some("W".into()),
        comment: Some("Cm".into()),
    };
    assert_eq!(SongText::from_row(&row).row(), row);
    assert_eq!(
        SongText::from_row(&SongInfoRow::default()).row(),
        SongInfoRow::default()
    );
    assert_eq!(
        SongText::from_row(&SongInfoRow {
            title: Some(String::new()),
            ..SongInfoRow::default()
        })
        .row(),
        SongInfoRow::default()
    );
    let labels: Vec<(&str, &str)> = FIELDS.iter().map(|f| (f.label, f.accessibility)).collect();
    assert_eq!(
        labels,
        [
            ("Title", "Song title"),
            ("Artist", "Artist"),
            ("Album", "Album"),
            ("Original artist", "Original artist"),
            ("Composer", "Composer"),
            ("Songwriter", "Songwriter"),
            ("Producer", "Producer"),
            ("Arranger", "Arranger"),
            ("Year", "Year"),
            ("Genre", "Genre"),
            ("Copyright", "Copyright"),
            ("Website", "Website"),
        ]
    );
    // Each field edits its own text.
    let mut s = SongText::default();
    for (i, f) in FIELDS.iter().enumerate() {
        *(f.get_mut)(&mut s) = i.to_string();
    }
    assert_eq!(s.website, "11");
    assert_eq!(s.original_artist, "3");
    assert_eq!(
        (TITLE, FORMAT_SECTION, SONG_SECTION, COMMENT_LABEL),
        ("Project Settings", "Format", "Song info", "Comment")
    );
}

/// Review fix: a failed song-info read keeps the format that was read,
/// and OK never sends song texts it did not read (they would overwrite
/// the stored ones with empties).
#[test]
fn a_failed_song_read_sends_no_song_info_and_keeps_the_format() {
    let f = fake();
    f.set_song(SongInfoRow {
        title: Some("Kept".into()),
        artist: Some("Kept too".into()),
        ..SongInfoRow::default()
    });
    f.set_project_info(ProjectInfoRow {
        format: format(44_100, BitDepthRow::Int24),
        mismatched: 0,
        rate_mismatch: false,
        device_rate: 0.0,
    });
    let mut m = doc(&f);
    f.fail_next("song_info", refusal());
    let mut p = ProjectSettings::load(&mut m);
    assert_eq!(m.alert.take().unwrap().title, "Project Settings Failed");
    assert!(!p.song_read());
    assert_eq!(
        p.format(),
        format(44_100, BitDepthRow::Int24),
        "the read format"
    );
    p.song.year = "1999".into();
    p.depth = BitDepthRow::Int16;
    p.apply(&mut m);
    assert_eq!(f.count("set_song_info"), 0);
    assert_eq!(f.song().title.as_deref(), Some("Kept"));
    // A depth-only change keeps the read rate.
    assert_eq!(formats_started(&f), [format(44_100, BitDepthRow::Int16)]);
}

/// Review fix: a failed format read sends no format change; the song
/// info that was read still applies.
#[test]
fn a_failed_format_read_sends_no_format_change() {
    let f = fake();
    let mut m = doc(&f);
    f.fail_next("project_info", refusal());
    let mut p = ProjectSettings::load(&mut m);
    assert_eq!(m.alert.take().unwrap().title, "Project Settings Failed");
    assert!(!p.format_read());
    p.rate = 96_000;
    p.song.genre = "Jazz".into();
    p.apply(&mut m);
    assert!(formats_started(&f).is_empty());
    assert!(!p.format_blocked(&m) || !m.merge_held());
    assert_eq!(f.song().genre.as_deref(), Some("Jazz"));
}
