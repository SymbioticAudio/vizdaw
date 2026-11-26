//! File › Export › AAF… and File › Import Project › AAF… (Swift
//! `AafModel`, `AafExportSheet`, `AafImportSheet`; slice 7c Task 5).
//!
//! Export: the options (`Embed audio in the AAF file`, `Bit depth`,
//! `Stereo tracks`, `Start`, `Frame rate`, `Clip gain`, `Track volume and
//! pan`), the core's check of what will not transfer (off the UI thread,
//! only the newest answer lands, [`DocumentModel::take_aaf_check`]) and
//! the core's folder name "<Song> AAF" for the shown sequence. Export…
//! asks for the folder to put it in (panel #9d); the core writes a new
//! folder there, numbered when the name is taken, so nothing is ever
//! written over and there is no replace question (Swift asks none). While
//! a job the core cannot run an export beside is listed (the interchange
//! and media jobs, or a held merge) the export waits ("Waiting for the
//! running job to finish…") instead of failing as Busy. An `ExportAaf`
//! job ("AAF Export Failed"); the sheet stays up with progress and Cancel
//! while it runs and closes when it ends; a refusal or a failed job
//! brings it back with the message, never an alert.
//!
//! Import: the chosen file is read off the UI thread
//! ([`crate::DocumentStore::begin_aaf_import`]; only the newest read
//! lands, a refusal is the alert "Import Failed"), then the sheet shows
//! with Swift's defaults: "Open as a new project", the MIDI file the core
//! found beside it, no folder for missing audio, pairs joined. Import
//! into a new project makes it at the file's rate (the store's made
//! project: an import that fails or is cancelled while it is untouched
//! takes it with it); into this project it is an `ImportAaf` job ("AAF
//! Import Failed").

use vizdaw_ffi::{
    AafDepthRow, AafExportRequestRow, AafFpsRow, AafImportRequestRow, AafInfoRow, AafStereoRow,
    JobKindRow, JobRow, JobStateRow, ReportRow, VizError,
};

use crate::alerts::error_message;
use crate::document::DocumentModel;
use crate::for_daw::{BLOCKING, MEDIA_JOBS};
use crate::session::DocSession;

pub use crate::dawproject::WAITING_TEXT;
pub use crate::for_daw::NO_SEQUENCE;

// ---- Export strings ----

/// The export sheet's title and subtitle.
pub const EXPORT_TITLE: &str = "Export AAF";
pub const EXPORT_SUBTITLE: &str = "Audio, fades and volume; MIDI as MIDI files beside it";
/// The toggles (their accessibility labels are the same).
pub const EMBED: &str = "Embed audio in the AAF file";
pub const CLIP_GAIN: &str = "Clip gain";
pub const AUTOMATION: &str = "Track volume and pan";
/// The pickers (their accessibility labels are the same) and choices.
pub const BIT_DEPTH: &str = "Bit depth";
pub const DEPTHS: [(AafDepthRow, &str); 2] = [
    (AafDepthRow::Int16, "16-bit"),
    (AafDepthRow::Int24, "24-bit"),
];
pub const STEREO_TRACKS: &str = "Stereo tracks";
pub const STEREO: [(AafStereoRow, &str); 2] = [
    (AafStereoRow::MonoPairs, "As left and right mono tracks"),
    (AafStereoRow::Stereo, "As stereo tracks"),
];
pub const START: &str = "Start";
/// `false`: 00:00:00:00; `true`: 01:00:00:00.
pub const STARTS: [(bool, &str); 2] = [(false, "00:00:00:00"), (true, "01:00:00:00")];
pub const FRAME_RATE: &str = "Frame rate";
pub const FPS: [(AafFpsRow, &str); 4] = [
    (AafFpsRow::Fps24, "24"),
    (AafFpsRow::Fps25, "25"),
    (AafFpsRow::Fps2997Drop, "29.97 drop frame"),
    (AafFpsRow::Fps30, "30"),
];
/// The title of a failed export job (its message shows in the sheet).
pub const EXPORT_FAILED: &str = "AAF Export Failed";

/// The line naming the folder the export makes.
pub fn writes_folder(name: &str) -> String {
    format!("Writes the folder \u{201c}{name}\u{201d}.")
}

// ---- Import strings ----

/// The import sheet's title.
pub const IMPORT_TITLE: &str = "Import AAF";
/// The `Import into` radio group and its choices.
pub const IMPORT_INTO: &str = "Import into";
pub const OPEN_AS_NEW: &str = "Open as a new project";
pub const ADD_TO_THIS: &str = "Add to this project";
/// The MIDI field (placeholder and accessibility label) and its Choose….
pub const MIDI_BESIDE: &str = "MIDI file beside it";
pub const CHOOSE: &str = "Choose\u{2026}";
pub const CHOOSE_MIDI_LABEL: &str = "Choose the MIDI file";
/// The missing-audio row.
pub const LOOK_FOR_MISSING: &str = "Look for missing audio in\u{2026}";
pub const NOWHERE_ELSE: &str = "Nowhere else";
pub const CHOOSE_FOLDER_LABEL: &str = "Choose a folder to look for missing audio in";
pub const CLEAR: &str = "Clear";
pub const CLEAR_LABEL: &str = "Clear the folder to look for missing audio in";
/// The join toggle (its accessibility label is the same).
pub const JOIN_PAIRS: &str = "Join left and right tracks into stereo";
pub const CANCEL: &str = "Cancel";
pub const IMPORT: &str = "Import";
/// The title of a failed import into this project.
pub const IMPORT_FAILED: &str = "AAF Import Failed";

/// The file's name, rate, tracks and audio files (Swift `describe`):
/// `<file>: <rate> Hz, N track(s), N audio file(s)[, N missing]`.
pub fn describe(info: &AafInfoRow, path: &str) -> String {
    let tracks = if info.tracks == 1 {
        "1 track".to_owned()
    } else {
        format!("{} tracks", info.tracks)
    };
    let files = if info.audio_files == 1 {
        "1 audio file".to_owned()
    } else {
        format!("{} audio files", info.audio_files)
    };
    let offline = if info.offline == 0 {
        String::new()
    } else {
        format!(", {} missing", info.offline)
    };
    let name = std::path::Path::new(path)
        .file_name()
        .map_or_else(|| path.to_owned(), |n| n.to_string_lossy().into_owned());
    format!(
        "{name}: {} Hz, {tracks}, {files}{offline}",
        info.sample_rate
    )
}

/// The last path component of `path` (the missing-audio folder's name).
pub fn folder_name(path: &str) -> String {
    std::path::Path::new(path)
        .file_name()
        .map_or_else(|| path.to_owned(), |n| n.to_string_lossy().into_owned())
}

// ---- Export ----

/// The export's options, with Swift's defaults.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AafExportChoices {
    pub embed: bool,
    pub depth: AafDepthRow,
    pub stereo: AafStereoRow,
    pub hour_start: bool,
    pub fps: AafFpsRow,
    pub clip_gain: bool,
    pub automation: bool,
}

impl Default for AafExportChoices {
    fn default() -> Self {
        AafExportChoices {
            embed: true,
            depth: AafDepthRow::Int24,
            stereo: AafStereoRow::MonoPairs,
            hour_start: false,
            fps: AafFpsRow::Fps25,
            clip_gain: true,
            automation: true,
        }
    }
}

/// A check of the options to run off the UI thread.
#[derive(Debug, Clone, PartialEq)]
pub struct AafCheck {
    pub generation: u64,
    pub request: AafExportRequestRow,
}

/// A check's answer, to land on the document.
#[derive(Debug, Clone, PartialEq)]
pub struct AafChecked {
    pub generation: u64,
    pub answer: Result<ReportRow, VizError>,
}

impl AafCheck {
    /// Asks the core (slow: call it off the UI thread).
    pub fn run(self, s: &dyn DocSession) -> AafChecked {
        AafChecked {
            generation: self.generation,
            answer: s.export_aaf_check(self.request),
        }
    }
}

/// The export sheet's state on a document (Swift `AafModel`'s export).
#[derive(Debug, Default)]
pub struct AafExport {
    shown: bool,
    choices: AafExportChoices,
    wont_transfer: Option<ReportRow>,
    error: Option<String>,
    /// The core's folder name for the shown sequence.
    default_name: String,
    kept: Option<AafExportRequestRow>,
    job: Option<u64>,
    generation: u64,
    pending: Option<AafCheck>,
}

impl AafExport {
    pub fn shown(&self) -> bool {
        self.shown
    }

    pub fn choices(&self) -> &AafExportChoices {
        &self.choices
    }

    /// What the export would not carry over, from the core's check.
    pub fn wont_transfer(&self) -> Option<&ReportRow> {
        self.wont_transfer.as_ref()
    }

    /// Why the last check, start or job was refused.
    pub fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }

    /// No answer yet and no refusal: the check is running.
    pub fn checking(&self) -> bool {
        self.wont_transfer.is_none() && self.error.is_none()
    }

    /// The core's folder name ("" when it gave none).
    pub fn default_name(&self) -> &str {
        &self.default_name
    }

    /// The export is kept until the running job ends.
    pub fn waiting(&self) -> bool {
        self.kept.is_some()
    }

    pub fn job(&self) -> Option<u64> {
        self.job
    }
}

// ---- Import ----

/// The AAF file the import sheet shows, as read.
#[derive(Debug, Clone, PartialEq)]
pub struct AafRead {
    pub path: String,
    pub info: AafInfoRow,
}

/// The import sheet's state: the file as read and the choices.
#[derive(Debug, Clone, PartialEq)]
pub struct AafImport {
    read: AafRead,
    shown: bool,
    /// "Open as a new project" (true, the default) or "Add to this
    /// project".
    pub new_project: bool,
    /// "Join left and right tracks into stereo".
    pub join_pairs: bool,
    /// "MIDI file beside it": filled from the file's `midi_beside`; empty
    /// or `None` sends none (the core then reads the one beside it).
    pub midi_path: Option<String>,
    /// "Look for missing audio in…": an absolute folder, or `None`.
    pub media_folder: Option<String>,
}

impl AafImport {
    /// The sheet for `read`, with Swift's defaults.
    fn new(read: AafRead) -> Self {
        AafImport {
            new_project: true,
            join_pairs: true,
            midi_path: read.info.midi_beside.clone(),
            media_folder: None,
            shown: true,
            read,
        }
    }

    pub fn path(&self) -> &str {
        &self.read.path
    }

    pub fn info(&self) -> &AafInfoRow {
        &self.read.info
    }

    /// Whether the sheet is up (Swift `importShown`).
    pub fn shown(&self) -> bool {
        self.shown
    }

    /// The request Import sends (Swift `startImport`).
    pub fn request(&self) -> AafImportRequestRow {
        let given = |p: &Option<String>| p.clone().filter(|p| !p.is_empty());
        AafImportRequestRow {
            path: self.read.path.clone(),
            new_project: self.new_project,
            midi_path: given(&self.midi_path),
            media_folder: given(&self.media_folder),
            join_pairs: self.join_pairs,
        }
    }
}

impl DocumentModel {
    // ---- Export ----

    /// The export sheet's state.
    pub fn aaf_export(&self) -> &AafExport {
        &self.aaf_export
    }

    /// File › Export › AAF…: the sheet, with the core's folder name and a
    /// fresh check (the options are kept). Showing it again while an
    /// export waits or runs keeps it, with its progress and Cancel.
    pub fn show_aaf_export(&mut self) {
        if !self.aaf_export.waiting() && !self.aaf_export_running() {
            // A name from the text of the model: no file is looked at.
            let name = self
                .selected_seq()
                .and_then(|seq| self.session.aaf_folder_name(seq).ok())
                .unwrap_or_default();
            let a = &mut self.aaf_export;
            a.job = None;
            a.kept = None;
            a.error = None;
            a.wont_transfer = None;
            a.default_name = name;
            self.refresh_aaf_export();
        }
        self.aaf_export.shown = true;
    }

    /// The sheet's Cancel (nothing waiting or running): it goes.
    pub fn hide_aaf_export(&mut self) {
        self.aaf_export.shown = false;
    }

    /// Edits the options; a change while the sheet is up is checked again.
    pub fn edit_aaf_export(&mut self, f: impl FnOnce(&mut AafExportChoices)) {
        let before = self.aaf_export.choices;
        f(&mut self.aaf_export.choices);
        if self.aaf_export.choices != before && self.aaf_export.shown {
            self.refresh_aaf_export();
        }
    }

    /// The request for folder `path` ("" for a check). `None` with no
    /// sequence shown.
    pub fn aaf_export_request(&self, path: &str) -> Option<AafExportRequestRow> {
        let c = &self.aaf_export.choices;
        Some(AafExportRequestRow {
            seq: self.selected_seq()?,
            path: path.to_owned(),
            embed: c.embed,
            depth: c.depth,
            stereo: c.stereo,
            hour_start: c.hour_start,
            fps: c.fps,
            clip_gain: c.clip_gain,
            automation: c.automation,
        })
    }

    /// Asks for a check (the UI runs it, see
    /// [`DocumentModel::take_aaf_check`]); with no sequence shown the
    /// sheet says so at once and any older check is dropped.
    pub fn refresh_aaf_export(&mut self) {
        let request = self.aaf_export_request("");
        let a = &mut self.aaf_export;
        a.generation += 1;
        match request {
            Some(request) => {
                a.pending = Some(AafCheck {
                    generation: a.generation,
                    request,
                });
            }
            None => {
                a.pending = None;
                a.wont_transfer = None;
                a.error = Some(NO_SEQUENCE.into());
            }
        }
    }

    /// The newest check waiting to run, taken.
    pub fn take_aaf_check(&mut self) -> Option<AafCheck> {
        self.aaf_export.pending.take()
    }

    /// A check's answer: what won't transfer, or the refusal as the
    /// sheet's error. An answer older than the newest check is dropped.
    pub fn land_aaf_check(&mut self, c: AafChecked) {
        let a = &mut self.aaf_export;
        if c.generation != a.generation {
            return;
        }
        match c.answer {
            Ok(r) => {
                a.wont_transfer = Some(r);
                a.error = None;
            }
            Err(e) => {
                a.wont_transfer = None;
                a.error = Some(error_message(&e));
            }
        }
    }

    /// Runs the waiting check here and lands it (tests).
    pub fn run_aaf_check(&mut self) {
        if let Some(c) = self.take_aaf_check() {
            let checked = c.run(self.session.as_ref());
            self.land_aaf_check(checked);
        }
    }

    /// The export job is still listed as running.
    pub fn aaf_export_running(&self) -> bool {
        self.aaf_export
            .job
            .is_some_and(|id| self.jobs.iter().any(|j| j.id == id))
    }

    /// The running export's progress (0–1).
    pub fn aaf_export_progress(&self) -> Option<f32> {
        let id = self.aaf_export.job?;
        self.jobs.iter().find(|j| j.id == id).map(|j| j.progress)
    }

    /// Whether a job the core refuses an export beside (Busy) is listed,
    /// or a merge is held.
    fn aaf_export_blocked(&self) -> bool {
        self.merge_held
            || self
                .jobs
                .iter()
                .any(|j| BLOCKING.contains(&j.kind) || MEDIA_JOBS.contains(&j.kind))
    }

    /// The folder panel's answer: the core writes "<Song> AAF" in
    /// `folder` (never over one that is there), now or once the running
    /// job ends; nothing while the sheet's export runs.
    pub fn start_aaf_export(&mut self, folder: &str) {
        let Some(req) = self.aaf_export_request(folder) else {
            self.aaf_export.error = Some(NO_SEQUENCE.into());
            return;
        };
        if self.aaf_export_running() {
            return;
        }
        self.aaf_export.error = None;
        if self.aaf_export_blocked() {
            self.aaf_export.kept = Some(req);
        } else {
            self.begin_aaf_export(req);
        }
    }

    fn begin_aaf_export(&mut self, req: AafExportRequestRow) {
        self.aaf_export.kept = None;
        match self.session.export_aaf(req) {
            Ok(id) => {
                // Listed before the first poll, so a job that ends there
                // still reaches the sheet.
                self.aaf_export.job = Some(id);
                self.list_job(id, JobKindRow::ExportAaf);
                self.poll_jobs();
            }
            Err(e) => self.aaf_export_failed(error_message(&e)),
        }
    }

    /// The sheet's Cancel while waiting: nothing is sent.
    pub fn cancel_aaf_export_waiting(&mut self) {
        self.aaf_export.kept = None;
    }

    /// The sheet's Cancel while the export runs.
    pub fn cancel_aaf_export(&mut self) {
        if let Some(id) = self.aaf_export.job {
            self.cancel_job(id);
        }
    }

    /// The sheet again with `message`; a check in flight is dropped so it
    /// cannot erase it.
    fn aaf_export_failed(&mut self, message: String) {
        let a = &mut self.aaf_export;
        a.generation += 1;
        a.pending = None;
        a.error = Some(message);
        a.shown = true;
    }

    /// A finished or cancelled export of the sheet's closes the sheet.
    pub(crate) fn aaf_export_job_done(&mut self, job: &JobRow) {
        if job.kind == JobKindRow::ExportAaf && Some(job.id) == self.aaf_export.job {
            self.aaf_export.shown = false;
        }
    }

    /// The sheet's export failed: the sheet again with the message, and
    /// no report. False for anything else.
    pub(crate) fn aaf_export_job_failed(&mut self, job: &JobRow, message: &str) -> bool {
        if job.kind != JobKindRow::ExportAaf
            || Some(job.id) != self.aaf_export.job
            || job.state == JobStateRow::Cancelled
        {
            return false;
        }
        self.aaf_export_failed(message.to_owned());
        true
    }

    /// A blocking job ended: the kept request starts if nothing blocks it.
    pub(crate) fn aaf_export_job_ended(&mut self) {
        let Some(req) = self.aaf_export.kept.clone() else {
            return;
        };
        if !self.aaf_export_blocked() {
            self.begin_aaf_export(req);
        }
    }

    // ---- Import ----

    /// The import sheet's state, once a read landed (shown or not).
    pub fn aaf_import(&self) -> Option<&AafImport> {
        self.aaf_import.as_ref()
    }

    /// A read landed: the sheet shows it with the defaults.
    pub(crate) fn aaf_read_landed(&mut self, read: AafRead) {
        self.aaf_import = Some(AafImport::new(read));
    }

    /// Edits the sheet's choices.
    pub fn edit_aaf_import(&mut self, f: impl FnOnce(&mut AafImport)) {
        if let Some(a) = self.aaf_import.as_mut() {
            f(a);
        }
    }

    /// The sheet goes (Cancel, or Import).
    pub fn hide_aaf_import(&mut self) {
        if let Some(a) = self.aaf_import.as_mut() {
            a.shown = false;
        }
    }

    /// Import with "Add to this project": the sheet goes and this
    /// document's session imports the file (a refusal is the alert "AAF
    /// Import Failed"). The store's [`crate::DocumentStore::start_aaf_import`]
    /// sends here or makes the new project.
    pub(crate) fn start_aaf_import_here(&mut self, req: AafImportRequestRow) {
        self.hide_aaf_import();
        self.start_job(JobKindRow::ImportAaf, IMPORT_FAILED, |s| s.import_aaf(req));
    }
}
