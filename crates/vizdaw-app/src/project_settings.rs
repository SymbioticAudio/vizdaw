//! File › Project Settings… (Swift `ProjectSettingsModel`,
//! `ProjectSettingsSheet`): the project's format and its 13 song-info
//! texts. [`ProjectSettings::load`] reads them (and what a format change
//! would convert); [`ProjectSettings::apply`] (the sheet's OK) sends only
//! what changed: one `set_song_info` (one undo step, "Set Song Info") and,
//! when the format changed, the core's format job. While a DAWproject
//! merge is held a format change waits ([`ProjectSettings::format_blocked`],
//! help [`HELD_HELP`]).

use vizdaw_ffi::{BitDepthRow, MediaRow, ProjectFormatRow, SongInfoRow};

use crate::alerts::AlertItem;
use crate::document::DocumentModel;
use crate::formats::FALLBACK_FORMAT;

/// The sheet's title.
pub const TITLE: &str = "Project Settings";
/// The first section.
pub const FORMAT_SECTION: &str = "Format";
/// The second section.
pub const SONG_SECTION: &str = "Song info";
/// The rate picker's label.
pub const SAMPLE_RATE: &str = "Sample rate";
/// The depth picker's label.
pub const BIT_DEPTH: &str = "Bit depth";
/// The multi-line field's label.
pub const COMMENT_LABEL: &str = "Comment";
/// The buttons.
pub const CANCEL: &str = "Cancel";
pub const OK: &str = "OK";
/// The alert title of a failed read, write or format change.
pub const FAILED: &str = "Project Settings Failed";
/// The note with no audio files.
pub const NO_AUDIO_NOTE: &str = "The project has no audio files yet.";
/// Why OK waits while a held merge blocks a format change (Swift
/// `MergeModel.heldHelp`).
pub const HELD_HELP: &str = "Finish or cancel the DAWproject merge waiting to be confirmed first.";

/// The 13 song-info texts; an empty text is no value.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SongText {
    pub title: String,
    pub artist: String,
    pub album: String,
    pub original_artist: String,
    pub composer: String,
    pub songwriter: String,
    pub producer: String,
    pub arranger: String,
    pub year: String,
    pub genre: String,
    pub copyright: String,
    pub website: String,
    pub comment: String,
}

impl SongText {
    pub fn from_row(r: &SongInfoRow) -> Self {
        let t = |v: &Option<String>| v.clone().unwrap_or_default();
        SongText {
            title: t(&r.title),
            artist: t(&r.artist),
            album: t(&r.album),
            original_artist: t(&r.original_artist),
            composer: t(&r.composer),
            songwriter: t(&r.songwriter),
            producer: t(&r.producer),
            arranger: t(&r.arranger),
            year: t(&r.year),
            genre: t(&r.genre),
            copyright: t(&r.copyright),
            website: t(&r.website),
            comment: t(&r.comment),
        }
    }

    /// The texts as the core's row (an empty text is `None`).
    pub fn row(&self) -> SongInfoRow {
        let v = |s: &String| (!s.is_empty()).then(|| s.clone());
        SongInfoRow {
            title: v(&self.title),
            artist: v(&self.artist),
            album: v(&self.album),
            original_artist: v(&self.original_artist),
            composer: v(&self.composer),
            songwriter: v(&self.songwriter),
            producer: v(&self.producer),
            arranger: v(&self.arranger),
            year: v(&self.year),
            genre: v(&self.genre),
            copyright: v(&self.copyright),
            website: v(&self.website),
            comment: v(&self.comment),
        }
    }
}

/// One single-line song-info field: its label (also its placeholder),
/// its accessibility label and the text it edits.
pub struct SongField {
    pub label: &'static str,
    pub accessibility: &'static str,
    pub get: fn(&SongText) -> &String,
    pub get_mut: fn(&mut SongText) -> &mut String,
}

macro_rules! field {
    ($label:literal, $a11y:literal, $f:ident) => {
        SongField {
            label: $label,
            accessibility: $a11y,
            get: |s| &s.$f,
            get_mut: |s| &mut s.$f,
        }
    };
}

/// The 12 single-line fields, in Swift's order; `Comment` follows them.
pub const FIELDS: [SongField; 12] = [
    field!("Title", "Song title", title),
    field!("Artist", "Artist", artist),
    field!("Album", "Album", album),
    field!("Original artist", "Original artist", original_artist),
    field!("Composer", "Composer", composer),
    field!("Songwriter", "Songwriter", songwriter),
    field!("Producer", "Producer", producer),
    field!("Arranger", "Arranger", arranger),
    field!("Year", "Year", year),
    field!("Genre", "Genre", genre),
    field!("Copyright", "Copyright", copyright),
    field!("Website", "Website", website),
];

/// The sheet's line about converting the project's audio files.
pub fn note(media: &[MediaRow]) -> String {
    if media.is_empty() {
        return NO_AUDIO_NOTE.to_owned();
    }
    let seconds: f64 = media
        .iter()
        .map(|m| m.frames as f64 / f64::from(m.sample_rate.max(1)))
        .sum();
    let length = if seconds >= 60.0 {
        format!("{:.0} min", seconds / 60.0)
    } else {
        format!("{seconds:.0} s")
    };
    let files = if media.len() == 1 {
        "1 audio file".to_owned()
    } else {
        format!("{} audio files", media.len())
    };
    format!(
        "Changing the format converts {files} ({length} of audio) into new files (one undo step)."
    )
}

/// The sheet's state: what it loaded and what is edited now.
#[derive(Debug, Clone, PartialEq)]
pub struct ProjectSettings {
    loaded_song: SongText,
    loaded_format: ProjectFormatRow,
    song_read: bool,
    format_read: bool,
    pub rate: u32,
    pub depth: BitDepthRow,
    pub song: SongText,
    /// What a format change would convert.
    pub media_note: String,
}

impl ProjectSettings {
    /// Reads the project's format, song info and media (File › Project
    /// Settings…). Each is read on its own and what was read is kept; a
    /// failed read is the alert "Project Settings Failed" on `d`, and OK
    /// then sends nothing for that part (the format shows 48 kHz /
    /// 24-bit, the texts empty).
    pub fn load(d: &mut DocumentModel) -> Self {
        let media_note = note(&d.media_rows());
        let mut failed = None;
        let format = d.session().project_info().map(|i| i.format);
        let song = d.session().song_info();
        let format_read = format.is_ok();
        let song_read = song.is_ok();
        let loaded_format = format.unwrap_or_else(|e| {
            failed = Some(e);
            FALLBACK_FORMAT
        });
        let loaded_song = match song {
            Ok(s) => SongText::from_row(&s),
            Err(e) => {
                failed.get_or_insert(e);
                SongText::default()
            }
        };
        if let Some(e) = failed {
            d.alert = Some(AlertItem::from_error(&e, FAILED));
        }
        ProjectSettings {
            song_read,
            format_read,
            rate: loaded_format.sample_rate,
            depth: loaded_format.depth,
            song: loaded_song.clone(),
            loaded_song,
            loaded_format,
            media_note,
        }
    }

    /// The song info was read: OK may send it (texts never read would
    /// overwrite the stored ones with empties).
    pub fn song_read(&self) -> bool {
        self.song_read
    }

    /// The format was read: OK may change it.
    pub fn format_read(&self) -> bool {
        self.format_read
    }

    /// The format chosen now.
    pub fn format(&self) -> ProjectFormatRow {
        ProjectFormatRow {
            sample_rate: self.rate,
            depth: self.depth,
        }
    }

    /// The format changed while a DAWproject merge is held: the core
    /// refuses the conversion beside it, so OK waits.
    pub fn format_blocked(&self, d: &DocumentModel) -> bool {
        self.format_read && self.format() != self.loaded_format && d.merge_held()
    }

    /// OK: one `set_song_info` when any text changed, the format job
    /// when the format changed; nothing when nothing changed. A failure
    /// is the alert "Project Settings Failed" and keeps that change to
    /// send. A part that could not be read is never sent.
    pub fn apply(&mut self, d: &mut DocumentModel) {
        if self.song_read && self.song != self.loaded_song {
            match d.session().set_song_info(self.song.row()) {
                Ok(s) => {
                    d.apply(s);
                    self.loaded_song = self.song.clone();
                }
                Err(e) => d.alert = Some(AlertItem::from_error(&e, FAILED)),
            }
        }
        let format = self.format();
        if self.format_read && format != self.loaded_format && d.change_format(format) {
            self.loaded_format = format;
        }
    }
}
