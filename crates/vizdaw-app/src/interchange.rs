//! The import half of Swift `InterchangeModel` (slice 7c Task 1): File ›
//! Import MIDI File…'s sheet. The file is read off the UI thread
//! ([`crate::DocumentStore::begin_midi_import`]; only the newest read of a
//! document lands, a refusal is the alert "Import Failed"); the read
//! opens the sheet with Swift's defaults: each channel on the core's
//! suggested destination, Split by channel for a type 0 file, Treat as MPE
//! when the file has an MPE configuration, the tempo map off unless the
//! shown sequence has no events (then it is on and locked, Ruling T7-a),
//! and this project as where it goes. Import with this project imports at
//! the playhead into the shown sequence (the selected track, if one, as
//! the drop track) as an `ImportMidi` job ("MIDI Import Failed"); with a
//! new project the store makes a 48 kHz project for it
//! ([`crate::DocumentStore::import_midi_as_new_project`]), every channel
//! on a new built-in synth, the file's tempo map always taken.
//!
//! The export half (slice 7c Task 2): File › Export › MIDI File… and MIDI
//! Clip File…'s sheet ([`MidiExport`]). Its choices start with the
//! selected tracks of the shown sequence (else all); every change asks
//! the core's dry run again, off the UI thread, and only the newest answer
//! lands ("What won't transfer"). Export… writes a `.mid`, one track's
//! `.midi2`, or several tracks' Clip Files into a folder: then the core
//! names the files first and the replace question is asked when any is
//! there, so nothing is written over silently. A refused start or a
//! failed job brings the sheet back with the message, never an alert.

use std::collections::BTreeSet;
use std::path::Path;

use vizdaw_ffi::{
    ChannelMapRow, ClipExportRequestRow, ClipNamesRow, JobKindRow, JobRow, JobStateRow,
    MidiFileInfoRow, MidiFileKindRow, MidiImportRequestRow, ReportRow, SmfExportRequestRow,
    VizError,
};

use crate::alerts::{AlertItem, error_message};
use crate::document::DocumentModel;
use crate::jobs::import_target;
use crate::session::DocSession;
use crate::store::MidiRead;

/// The sheet's title.
pub const IMPORT_TITLE: &str = "Import MIDI File";
/// The "where" picker's label and choices.
pub const IMPORT_INTO: &str = "Import into";
pub const THIS_PROJECT: &str = "This project, at the playhead";
pub const A_NEW_PROJECT: &str = "A new project";
/// Shown instead of the channel list when the file plays no notes.
pub const NO_NOTES: &str = "The file plays no notes on any channel.";
/// The channel list's heading, into this project and into a new one.
pub const WHERE_EACH_CHANNEL_PLAYS: &str = "Where each channel plays";
pub const NEW_PROJECT_CHANNELS: &str = "In a new project each channel plays a new built-in synth.";
/// The destination picker's first item (`dest` none).
pub const NEW_BUILT_IN_SYNTH: &str = "New built-in synth";
/// The toggles, their accessibility labels and help.
pub const SPLIT_BY_CHANNEL: &str = "Split by channel";
pub const SPLIT_LABEL: &str = "Split tracks by channel";
pub const TREAT_AS_MPE: &str = "Treat as MPE";
pub const MPE_HELP: &str =
    "Member-channel notes, bend, CC 74 and pressure become per-note expression";
pub const USE_TEMPO: &str = "Use the file's tempo map";
pub const TEMPO_LOCKED_HELP: &str =
    "A sequence without events always takes the file's tempo and meter";
pub const TEMPO_HELP: &str =
    "Replace this sequence's tempo and meter from the playhead with the file's";
/// The buttons.
pub const CANCEL: &str = "Cancel";
pub const IMPORT: &str = "Import";
/// The title of a refused start into this project, and of a failed job.
pub const MIDI_IMPORT_FAILED: &str = "MIDI Import Failed";
/// Import with no sequence shown.
pub const NO_SEQUENCE: &str = "No sequence is shown.";

/// One row of the sheet: where a channel's notes go (`dest` none: a new
/// built-in synth).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChannelChoice {
    pub group: u8,
    pub channel: u8,
    pub dest: Option<u64>,
    /// Notes on it in the file.
    pub notes: u64,
}

/// "Channel 10", or "Group 2, channel 10" past the first group (1-based).
pub fn channel_name(c: &ChannelChoice) -> String {
    if c.group == 0 {
        format!("Channel {}", u16::from(c.channel) + 1)
    } else {
        format!(
            "Group {}, channel {}",
            u16::from(c.group) + 1,
            u16::from(c.channel) + 1
        )
    }
}

/// A row's destination picker's accessibility label: "Destination for
/// channel 10".
pub fn destination_label(c: &ChannelChoice) -> String {
    format!("Destination for {}", channel_name(c).to_lowercase())
}

/// "1 note" / "N notes".
pub fn notes_text(notes: u64) -> String {
    if notes == 1 {
        "1 note".to_owned()
    } else {
        format!("{notes} notes")
    }
}

/// The line under the title: "<file>: <kind>, N track(s)".
pub fn describe(info: &MidiFileInfoRow, path: &str) -> String {
    let kind = match info.kind {
        MidiFileKindRow::Smf0 => "Standard MIDI File, type 0",
        MidiFileKindRow::Smf1 => "Standard MIDI File, type 1",
        MidiFileKindRow::Clip => "MIDI Clip File",
    };
    let tracks = if info.tracks == 1 {
        "1 track".to_owned()
    } else {
        format!("{} tracks", info.tracks)
    };
    let name = std::path::Path::new(path)
        .file_name()
        .map_or_else(|| path.to_owned(), |n| n.to_string_lossy().into_owned());
    format!("{name}: {kind}, {tracks}")
}

/// The import sheet's state: the file as read and the choices.
#[derive(Debug, Clone, PartialEq)]
pub struct MidiImport {
    read: MidiRead,
    shown: bool,
    pub choices: Vec<ChannelChoice>,
    pub split_channels: bool,
    pub mpe: bool,
    /// Where: a new project instead of this one.
    pub into_new_project: bool,
    /// The tempo box as ticked (ignored while locked).
    wants_tempo: bool,
}

impl MidiImport {
    /// The sheet for `read`, with Swift's defaults (`landed`).
    fn new(read: MidiRead) -> Self {
        let choices = read
            .info
            .channels
            .iter()
            .map(|c| ChannelChoice {
                group: c.group,
                channel: c.channel,
                dest: c.suggested,
                notes: c.notes,
            })
            .collect();
        MidiImport {
            split_channels: read.info.kind == MidiFileKindRow::Smf0,
            mpe: read.info.mpe_found,
            choices,
            into_new_project: false,
            wants_tempo: false,
            shown: true,
            read,
        }
    }

    pub fn path(&self) -> &str {
        &self.read.path
    }

    pub fn info(&self) -> &MidiFileInfoRow {
        &self.read.info
    }

    pub fn read(&self) -> &MidiRead {
        &self.read
    }

    /// Whether the sheet is up (Swift `importShown`).
    pub fn shown(&self) -> bool {
        self.shown
    }

    /// The request for a new project whose shown sequence is `seq`: every
    /// channel on a new destination (it has none yet), the tempo map
    /// taken.
    pub fn new_project_request(&self, seq: u64) -> MidiImportRequestRow {
        MidiImportRequestRow {
            path: self.read.path.clone(),
            new_project: true,
            seq,
            lane: None,
            tick: 0,
            channel_map: self
                .choices
                .iter()
                .map(|c| ChannelMapRow {
                    group: c.group,
                    channel: c.channel,
                    dest: None,
                })
                .collect(),
            split_channels: Some(self.split_channels),
            mpe: self.mpe,
            use_tempo: true,
        }
    }
}

/// The tempo box as the sheet shows it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TempoBox {
    pub on: bool,
    pub enabled: bool,
    pub help: &'static str,
}

impl DocumentModel {
    /// The import sheet's state, once a read landed (shown or not).
    pub fn midi_import(&self) -> Option<&MidiImport> {
        self.midi_import.as_ref()
    }

    /// The MIDI file the import sheet read, once read.
    pub fn midi_read(&self) -> Option<&MidiRead> {
        self.midi_import.as_ref().map(MidiImport::read)
    }

    /// A read landed: the sheet shows it with the defaults.
    pub(crate) fn midi_read_landed(&mut self, read: MidiRead) {
        self.midi_import = Some(MidiImport::new(read));
    }

    /// Edits the sheet's choices.
    pub fn edit_midi_import(&mut self, f: impl FnOnce(&mut MidiImport)) {
        if let Some(m) = self.midi_import.as_mut() {
            f(m);
        }
    }

    /// The sheet goes (Cancel, or Import).
    pub fn hide_midi_import(&mut self) {
        if let Some(m) = self.midi_import.as_mut() {
            m.shown = false;
        }
    }

    /// The core takes the file's tempo into a sequence without events
    /// (Ruling T7-a): the box is on and cannot be turned off.
    pub fn midi_import_tempo_locked(&self) -> bool {
        !self
            .lanes()
            .iter()
            .any(|l| l.midi_event_count > 0 || l.region_count > 0 || l.reference_count > 0)
    }

    /// Whether an import into this project takes the file's tempo map.
    pub fn midi_import_use_tempo(&self) -> bool {
        self.midi_import_tempo_locked() || self.midi_import.as_ref().is_some_and(|m| m.wants_tempo)
    }

    /// Ticks or clears the tempo box (ignored while it is locked).
    pub fn set_midi_import_use_tempo(&mut self, on: bool) {
        if let Some(m) = self.midi_import.as_mut() {
            m.wants_tempo = on;
        }
    }

    /// The tempo box: on and locked for an empty sequence or a new
    /// project, with the help that says why.
    pub fn midi_import_tempo_box(&self) -> TempoBox {
        let fresh = self
            .midi_import
            .as_ref()
            .is_some_and(|m| m.into_new_project);
        let locked = self.midi_import_tempo_locked() || fresh;
        TempoBox {
            on: self.midi_import_use_tempo() || fresh,
            enabled: !locked,
            help: if locked {
                TEMPO_LOCKED_HELP
            } else {
                TEMPO_HELP
            },
        }
    }

    /// The sheet's Import into this project: the shown sequence at the
    /// playhead, the selected track (if one) as the drop track. No
    /// sequence shown is the alert "Import Failed".
    pub fn start_midi_import(&mut self) {
        let Some(m) = self.midi_import.as_ref() else {
            return;
        };
        let target = import_target(
            self.selected_seq(),
            &self.selection().lanes,
            1,
            self.last_transport().tick,
        );
        let Some(target) = target else {
            self.alert = Some(AlertItem::new(
                crate::interchange_commands::IMPORT_FAILED,
                NO_SEQUENCE,
            ));
            return;
        };
        let req = MidiImportRequestRow {
            path: m.read.path.clone(),
            new_project: false,
            seq: target.seq,
            lane: target.lane,
            tick: target.tick,
            channel_map: m
                .choices
                .iter()
                .map(|c| ChannelMapRow {
                    group: c.group,
                    channel: c.channel,
                    dest: c.dest,
                })
                .collect(),
            split_channels: Some(m.split_channels),
            mpe: m.mpe,
            use_tempo: self.midi_import_use_tempo(),
        };
        self.hide_midi_import();
        self.start_job(JobKindRow::ImportMidi, MIDI_IMPORT_FAILED, |s| {
            s.import_midi(req)
        });
    }

    /// Whether the report of an import or export this document's UI ran
    /// waits to be shown (Swift `InterchangeModel.reportShown`; the
    /// report sheet, slice 7c Task 3, shows it and hides it again).
    pub fn report_shown(&self) -> bool {
        self.report_shown
    }

    /// The report sheet went.
    pub fn hide_report(&mut self) {
        self.report_shown = false;
    }

    /// What the export that made the report wrote (the report sheet's
    /// Show in Finder; empty for an import's or an older report).
    pub fn report_written(&self) -> &[String] {
        &self.report_written
    }

    /// File › Last Import or Export Report… (Swift `showLastReport`): the
    /// session's last report (it may be one the channel made), else the
    /// UI's own; with none, the status line says so. A report other than
    /// the one kept wrote nothing this sheet can show.
    pub fn show_last_report(&mut self) {
        let last = self
            .session
            .interchange_report()
            .ok()
            .flatten()
            .or_else(|| self.last_report.clone());
        let Some(last) = last else {
            self.status = Some(crate::report_text::NO_REPORT_YET.into());
            return;
        };
        if self.last_report.as_ref() != Some(&last) {
            self.report_written.clear();
        }
        self.last_report = Some(last);
        self.report_shown = true;
    }

    /// A merge's report (Swift `InterchangeModel.show`): shown as an
    /// import's is, with nothing written.
    pub fn show_report(&mut self, report: ReportRow) {
        self.last_report = Some(report);
        self.report_written.clear();
        self.report_shown = true;
    }
}

// ---------------------------------------------------------------------
// Export (Swift `InterchangeModel`'s export half, `MidiExportSheet`)

/// The sheets' titles and subtitles.
pub const SMF_TITLE: &str = "Export MIDI File";
pub const CLIP_TITLE: &str = "Export MIDI Clip File";
pub const SMF_SUBTITLE: &str = "Notes, controllers, tempo and markers as a Standard MIDI File";
pub const CLIP_SUBTITLE: &str = "One track, lossless, as a MIDI 2.0 Clip File";
/// The Type picker: its title, accessibility label and choices.
pub const TYPE: &str = "Type";
pub const TYPE_LABEL: &str = "MIDI file type";
pub const TYPE_1: &str = "Type 1 (one MIDI track per track)";
pub const TYPE_0: &str = "Type 0 (everything on one MIDI track)";
/// The Resolution picker: its title, accessibility label and choices.
pub const RESOLUTION: &str = "Resolution";
pub const RESOLUTION_LABEL: &str = "Resolution in ticks per quarter note";
pub const RESOLUTIONS: [(u16, &str); 3] =
    [(480, "480 PPQ"), (960, "960 PPQ"), (15_360, "15 360 PPQ")];
/// The track list's heading.
pub const TRACKS: &str = "Tracks";
pub const WRITE_MPE: &str = "Write per-note expression as MPE";
pub const TIME_SELECTION_ONLY: &str = "Time selection only";
/// Under the toggles of a clip export of several tracks.
pub const EACH_TRACK_ONE_FILE: &str = "Each track becomes one file in the folder you choose.";
/// The dry run's heading, its empty answer and its spinner's label.
pub const WHAT_WONT_TRANSFER: &str = "What won't transfer";
pub const EVERYTHING_TRANSFERS: &str = "Everything transfers.";
pub const CHECKING: &str = "Checking what won't transfer";
/// The default button (it asks where first).
pub const EXPORT: &str = "Export\u{2026}";
/// The title of a failed export job (its message shows in the sheet).
pub const MIDI_EXPORT_FAILED: &str = "MIDI Export Failed";

/// A track row's toggle's accessibility label: "Export track <name>".
pub fn export_track_label(name: &str) -> String {
    format!("Export track {name}")
}

/// Which export the sheet is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ExportKind {
    /// A Standard MIDI File (File › Export › MIDI File…).
    #[default]
    Smf,
    /// MIDI 2.0 Clip Files (File › Export › MIDI Clip File…).
    Clip,
}

impl ExportKind {
    /// The sheet's title.
    pub fn title(self) -> &'static str {
        match self {
            ExportKind::Smf => SMF_TITLE,
            ExportKind::Clip => CLIP_TITLE,
        }
    }

    pub fn subtitle(self) -> &'static str {
        match self {
            ExportKind::Smf => SMF_SUBTITLE,
            ExportKind::Clip => CLIP_SUBTITLE,
        }
    }
}

/// The sheet's choices.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MidiExportChoices {
    pub type0: bool,
    pub ppq: u16,
    /// The tracks to export.
    pub lanes: BTreeSet<u64>,
    /// Write per-note expression as MPE (a Standard MIDI File).
    pub mpe: bool,
    /// Only the time selection (a clip export).
    pub use_selection: bool,
}

impl Default for MidiExportChoices {
    fn default() -> Self {
        MidiExportChoices {
            type0: false,
            ppq: 960,
            lanes: BTreeSet::new(),
            mpe: false,
            use_selection: false,
        }
    }
}

/// "N files already exist in <folder>. Replace them?" ("1 file …
/// Replace it?"), asked before a clip export into a folder writes over
/// files there.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReplaceQuestion {
    pub folder: String,
    /// The core's names of the files already there.
    pub taken: Vec<String>,
    /// The export the names were asked for: Replace starts exactly this.
    pub request: ClipExportRequestRow,
}

impl ReplaceQuestion {
    pub fn count(&self) -> usize {
        self.taken.len()
    }

    /// The question's title (it has no message).
    pub fn message(&self) -> String {
        let name = Path::new(&self.folder)
            .file_name()
            .map_or_else(|| self.folder.clone(), |n| n.to_string_lossy().into_owned());
        if self.count() == 1 {
            format!("1 file already exists in {name}. Replace it?")
        } else {
            format!(
                "{} files already exist in {name}. Replace them?",
                self.count()
            )
        }
    }
}

/// A dry run's request.
#[derive(Debug, Clone, PartialEq)]
pub enum ExportCheckRequest {
    Smf(SmfExportRequestRow),
    Clip(ClipExportRequestRow),
}

/// A dry run of the sheet's choices to run off the UI thread.
#[derive(Debug, Clone, PartialEq)]
pub struct ExportCheck {
    pub generation: u64,
    pub request: ExportCheckRequest,
}

/// A dry run's answer, to land on the document.
#[derive(Debug, Clone, PartialEq)]
pub struct ExportChecked {
    pub generation: u64,
    pub answer: Result<ReportRow, VizError>,
}

impl ExportCheck {
    /// Asks the core (slow: call it off the UI thread).
    pub fn run(self, s: &dyn DocSession) -> ExportChecked {
        let answer = match self.request {
            ExportCheckRequest::Smf(r) => s.export_midi_check(r),
            ExportCheckRequest::Clip(r) => s.export_midi_clip_check(r),
        };
        ExportChecked {
            generation: self.generation,
            answer,
        }
    }
}

/// The question to the core which clip files a folder export writes and
/// which of them are there already, to run off the UI thread.
#[derive(Debug, Clone, PartialEq)]
pub struct ClipNamesAsk {
    pub generation: u64,
    pub folder: String,
    pub request: ClipExportRequestRow,
}

/// Its answer, to land on the document.
#[derive(Debug, Clone, PartialEq)]
pub struct ClipNamesAnswer {
    pub generation: u64,
    pub folder: String,
    /// The export the names were asked for.
    pub request: ClipExportRequestRow,
    pub answer: Result<ClipNamesRow, VizError>,
}

impl ClipNamesAsk {
    /// Asks the core (it looks at the folder: call it off the UI thread).
    pub fn run(self, s: &dyn DocSession) -> ClipNamesAnswer {
        ClipNamesAnswer {
            generation: self.generation,
            folder: self.folder,
            answer: s.export_midi_clip_names(self.request.clone()),
            request: self.request,
        }
    }
}

/// An export to start.
enum ExportJob {
    Smf(SmfExportRequestRow),
    Clip(ClipExportRequestRow),
}

/// Where the sheet's Export… asks to write.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExportDestination {
    /// A save panel (#10) suggesting `<name>.<ext>`; the panel (or the
    /// app's own question) confirms a replace.
    File { name: String, ext: &'static str },
    /// A folder (#9b) for several clip tracks; names already there are
    /// asked about first.
    Folder,
}

/// The export sheet's state (Swift `InterchangeModel`'s export half).
#[derive(Debug, Default)]
pub struct MidiExport {
    kind: ExportKind,
    shown: bool,
    choices: MidiExportChoices,
    /// What the export would not carry over, from the core's dry run.
    wont_transfer: Option<ReportRow>,
    /// Why the last dry run, start or job was refused.
    error: Option<String>,
    replace_question: Option<ReplaceQuestion>,
    /// The newest dry run asked for, and it until the UI takes it.
    check_generation: u64,
    pending_check: Option<ExportCheck>,
    /// The newest names question asked, and it until the UI takes it.
    names_generation: u64,
    pending_names: Option<ClipNamesAsk>,
}

impl MidiExport {
    pub fn kind(&self) -> ExportKind {
        self.kind
    }

    /// Whether the sheet is up (Swift `exportShown`).
    pub fn shown(&self) -> bool {
        self.shown
    }

    pub fn choices(&self) -> &MidiExportChoices {
        &self.choices
    }

    pub fn wont_transfer(&self) -> Option<&ReportRow> {
        self.wont_transfer.as_ref()
    }

    pub fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }

    /// No answer yet and no refusal: the dry run is running.
    pub fn checking(&self) -> bool {
        self.wont_transfer.is_none() && self.error.is_none()
    }

    /// The replace question waiting for its answer.
    pub fn replace_question(&self) -> Option<&ReplaceQuestion> {
        self.replace_question.as_ref()
    }
}

impl DocumentModel {
    /// The export sheet's state.
    pub fn midi_export(&self) -> &MidiExport {
        &self.midi_export
    }

    /// File › Export › MIDI File… or MIDI Clip File…: the sheet with the
    /// choices reset (the selected tracks of the shown sequence, else
    /// all), and its dry run.
    pub fn show_midi_export(&mut self, kind: ExportKind) {
        let all: Vec<u64> = self.lanes().iter().map(|l| l.id).collect();
        let seq = self.selected_seq();
        let picked: BTreeSet<u64> = self
            .selection()
            .lanes
            .iter()
            .filter(|r| Some(r.seq) == seq && all.contains(&r.lane))
            .map(|r| r.lane)
            .collect();
        let e = &mut self.midi_export;
        e.error = None;
        e.replace_question = None;
        e.names_generation += 1;
        e.pending_names = None;
        e.kind = kind;
        e.choices = MidiExportChoices {
            lanes: if picked.is_empty() {
                all.into_iter().collect()
            } else {
                picked
            },
            ..MidiExportChoices::default()
        };
        e.shown = true;
        self.refresh_midi_export();
    }

    /// Edits the choices; a change while the sheet is up is checked again.
    pub fn edit_midi_export(&mut self, f: impl FnOnce(&mut MidiExportChoices)) {
        let before = self.midi_export.choices.clone();
        f(&mut self.midi_export.choices);
        if self.midi_export.shown && self.midi_export.choices != before {
            self.refresh_midi_export();
        }
    }

    /// The sheet's Cancel: a names question still in flight is dropped
    /// (its late answer starts nothing and asks nothing).
    pub fn hide_midi_export(&mut self) {
        let e = &mut self.midi_export;
        e.shown = false;
        e.names_generation += 1;
        e.pending_names = None;
        e.replace_question = None;
    }

    /// A time selection exists to export.
    pub fn midi_export_has_selection(&self) -> bool {
        self.selection().time_range.is_some_and(|t| t.start < t.end)
    }

    /// Where Export… asks to write: a `.mid` named after the song, one
    /// track's `.midi2` named after it, or a folder for several tracks.
    pub fn midi_export_destination(&self) -> ExportDestination {
        let e = &self.midi_export;
        let title = self.snapshot().title.clone();
        match e.kind {
            ExportKind::Smf => ExportDestination::File {
                name: title,
                ext: "mid",
            },
            ExportKind::Clip if e.choices.lanes.len() == 1 => {
                let id = e.choices.lanes.first().copied();
                let name = self
                    .lanes()
                    .iter()
                    .find(|l| Some(l.id) == id)
                    .map_or(title, |l| l.name.clone());
                ExportDestination::File { name, ext: "midi2" }
            }
            ExportKind::Clip => ExportDestination::Folder,
        }
    }

    fn smf_request(&self, seq: u64, path: &str, replace: bool) -> SmfExportRequestRow {
        let c = &self.midi_export.choices;
        SmfExportRequestRow {
            seq,
            lanes: c.lanes.iter().copied().collect(),
            path: path.to_owned(),
            replace,
            type0: c.type0,
            ppq: c.ppq,
            mpe: c.mpe,
        }
    }

    fn clip_request(&self, seq: u64, path: &str, replace: bool) -> ClipExportRequestRow {
        let c = &self.midi_export.choices;
        let range = self
            .selection()
            .time_range
            .filter(|t| c.use_selection && t.start < t.end);
        ClipExportRequestRow {
            seq,
            lanes: c.lanes.iter().copied().collect(),
            path: path.to_owned(),
            replace,
            start: range.map(|t| t.start),
            end: range.map(|t| t.end),
        }
    }

    /// Asks for a dry run of the choices (the UI runs it, see
    /// [`DocumentModel::take_midi_export_check`]); with no sequence shown
    /// the sheet says so at once.
    fn refresh_midi_export(&mut self) {
        let Some(seq) = self.selected_seq() else {
            let e = &mut self.midi_export;
            e.wont_transfer = None;
            e.pending_check = None;
            e.error = Some(NO_SEQUENCE.into());
            return;
        };
        let request = match self.midi_export.kind {
            ExportKind::Smf => ExportCheckRequest::Smf(self.smf_request(seq, "", false)),
            ExportKind::Clip => ExportCheckRequest::Clip(self.clip_request(seq, "", false)),
        };
        let e = &mut self.midi_export;
        e.check_generation += 1;
        e.pending_check = Some(ExportCheck {
            generation: e.check_generation,
            request,
        });
    }

    /// The newest dry run waiting to run, taken (the UI runs it off its
    /// thread and lands it).
    pub fn take_midi_export_check(&mut self) -> Option<ExportCheck> {
        self.midi_export.pending_check.take()
    }

    /// A dry run's answer: what won't transfer, or the refusal as the
    /// sheet's error. An answer older than the newest dry run is dropped.
    pub fn land_midi_export_check(&mut self, c: ExportChecked) {
        let e = &mut self.midi_export;
        if c.generation != e.check_generation {
            return;
        }
        match c.answer {
            Ok(r) => {
                e.wont_transfer = Some(r);
                e.error = None;
            }
            Err(err) => {
                e.wont_transfer = None;
                e.error = Some(error_message(&err));
            }
        }
    }

    /// Runs the waiting dry run here and lands it (tests; the UI runs it
    /// off its thread).
    pub fn run_midi_export_check(&mut self) {
        if let Some(c) = self.take_midi_export_check() {
            let checked = c.run(self.session.as_ref());
            self.land_midi_export_check(checked);
        }
    }

    /// The folder panel's answer for several clip tracks: the core names
    /// the files and says which are taken (asked off the UI thread, see
    /// [`DocumentModel::take_clip_names`]). None taken, the export starts
    /// without replacing (a file that appears meanwhile is the core's
    /// refusal in the sheet); else the replace question is asked first.
    pub fn export_midi_to_folder(&mut self, folder: &str) {
        let Some(seq) = self.selected_seq() else {
            self.midi_export.error = Some(NO_SEQUENCE.into());
            return;
        };
        let request = self.clip_request(seq, folder, false);
        let e = &mut self.midi_export;
        e.names_generation += 1;
        e.pending_names = Some(ClipNamesAsk {
            generation: e.names_generation,
            folder: folder.to_owned(),
            request,
        });
    }

    /// The names question waiting to run, taken.
    pub fn take_clip_names(&mut self) -> Option<ClipNamesAsk> {
        self.midi_export.pending_names.take()
    }

    /// The names' answer: the export starts when none is taken, the
    /// replace question waits when some are, a refusal is the sheet's
    /// error. An answer older than the newest question (or the sheet
    /// reset since) is dropped.
    pub fn land_clip_names(&mut self, a: ClipNamesAnswer) {
        if a.generation != self.midi_export.names_generation || !self.midi_export.shown {
            return;
        }
        match a.answer {
            Err(err) => self.midi_export.error = Some(error_message(&err)),
            // The export the names were asked for, as asked.
            Ok(names) if names.taken.is_empty() => {
                self.start_export_job(ExportJob::Clip(a.request))
            }
            Ok(names) => {
                self.midi_export.replace_question = Some(ReplaceQuestion {
                    folder: a.folder,
                    taken: names.taken,
                    request: a.request,
                });
            }
        }
    }

    /// Runs the waiting names question here and lands it (tests).
    pub fn run_clip_names(&mut self) {
        if let Some(a) = self.take_clip_names() {
            let answer = a.run(self.session.as_ref());
            self.land_clip_names(answer);
        }
    }

    /// The replace question's answer: Replace exports exactly the
    /// request the question counted, replacing them; Cancel writes nothing
    /// and the sheet stays. Nothing once the sheet went.
    pub fn answer_midi_export_replace(&mut self, replace: bool) {
        let Some(q) = self.midi_export.replace_question.take() else {
            return;
        };
        if replace && self.midi_export.shown {
            self.start_export_job(ExportJob::Clip(ClipExportRequestRow {
                replace: true,
                ..q.request
            }));
        }
    }

    /// Starts the export to `path` (the save panel's answer, which
    /// confirmed any replace: `replace` true; or a folder), as an
    /// `ExportMidi` job. The sheet goes first: a refusal, or a job that
    /// fails (at once or later), brings it back with the message, never an
    /// alert.
    pub fn start_midi_export(&mut self, path: &str, replace: bool) {
        let Some(seq) = self.selected_seq() else {
            self.midi_export.error = Some(NO_SEQUENCE.into());
            return;
        };
        let job = match self.midi_export.kind {
            ExportKind::Smf => ExportJob::Smf(self.smf_request(seq, path, replace)),
            ExportKind::Clip => ExportJob::Clip(self.clip_request(seq, path, replace)),
        };
        self.start_export_job(job);
    }

    /// Starts `job` as an `ExportMidi` job (see
    /// [`DocumentModel::start_midi_export`]).
    fn start_export_job(&mut self, job: ExportJob) {
        self.midi_export.error = None;
        // Hidden before the job starts: a failure at its first poll shows
        // the sheet again, and it must stay up.
        self.midi_export.shown = false;
        let started = match job {
            ExportJob::Smf(req) => {
                self.try_start_job(JobKindRow::ExportMidi, |s| s.export_midi(req))
            }
            ExportJob::Clip(req) => {
                self.try_start_job(JobKindRow::ExportMidi, |s| s.export_midi_clip(req))
            }
        };
        if let Err(err) = started {
            self.midi_export.error = Some(error_message(&err));
            self.midi_export.shown = true;
        }
    }

    /// A MIDI export job of this document's UI failed: the sheet shows
    /// again with the message (Swift `jobFailed`). False for anything
    /// else (it is an alert, or a quiet cancel).
    pub(crate) fn midi_export_job_failed(&mut self, job: &JobRow, message: &str) -> bool {
        if job.kind != JobKindRow::ExportMidi || job.state == JobStateRow::Cancelled {
            return false;
        }
        self.midi_export.error = Some(message.to_owned());
        self.midi_export.shown = true;
        true
    }
}
