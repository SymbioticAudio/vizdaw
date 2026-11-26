//! File › Export › For Another DAW… (Swift `ForDawModel`, `ForDawSheet`;
//! slice 7c Task 4): the stems, their format (the rows shared with
//! Bounce, [`crate::audio_format`]), "Add the stems to this project", and
//! the core's check of the choices: where every file starts, the stems'
//! length, its warning and "What won't transfer". The check runs off the
//! UI thread ([`DocumentModel::take_for_daw_check`]); only the newest
//! answer lands. The first check after the sheet opens sends no strips
//! and takes the core's defaults as the stems.
//!
//! Export… asks for a folder (panel #9c) and starts an `ExportProject`
//! job ("Export Failed"). The core writes "<Song> for another DAW" in it,
//! numbered when that name is taken, so nothing there is ever written
//! over (Swift asks no question either). While a job the core cannot
//! run an export beside is listed ([`BLOCKING`], and with Add the media
//! jobs, or a held merge) the export waits ("Waiting for the running
//! import, export or bounce to finish…") and starts once it ends,
//! instead of failing as Busy. The sheet stays up with progress and
//! Cancel while the job runs and closes when it ends; a refusal or a
//! failed job brings it back with the message and the choices as they
//! were, never an alert.

use std::collections::BTreeSet;

use vizdaw_ffi::{
    ForDawCheckRow, ForDawRequestRow, JobKindRow, JobRow, JobStateRow, StripKindRow, StripRow,
    VizError,
};

use crate::alerts::error_message;
use crate::audio_format::{AudioFormat, AudioFormatChoices};
use crate::bounce::duration;
use crate::document::DocumentModel;
use crate::formats::FALLBACK_FORMAT;
use crate::session::DocSession;

/// The sheet's title and subtitle.
pub const TITLE: &str = "Export for Another DAW";
pub const SUBTITLE: &str = "Audio stems and MIDI files for any DAW";
/// The stems list's heading.
pub const STEMS: &str = "Stems";
/// The Add toggle (its accessibility label is the same).
pub const ADD_STEMS: &str = "Add the stems to this project";
/// The waiting row's text.
pub const WAITING_TEXT: &str = "Waiting for the running import, export or bounce to finish\u{2026}";
/// The waiting spinner's accessibility label.
pub const WAITING_LABEL: &str = "Waiting";
/// The running export's progress bar's accessibility label.
pub const PROGRESS_LABEL: &str = "Export progress";
/// The buttons and their accessibility labels.
pub const CANCEL: &str = "Cancel";
pub const CANCEL_WAITING_LABEL: &str = "Cancel the waiting export";
pub const CANCEL_EXPORT_LABEL: &str = "Cancel the export";
pub const EXPORT_LABEL: &str = "Export, then choose where";
/// The title of a failed export job (its message shows in the sheet).
pub const EXPORT_FAILED: &str = "Export Failed";
/// The sheet's error with no sequence shown.
pub const NO_SEQUENCE: &str = "No sequence is shown.";

/// Jobs the core cannot run an export beside (Swift
/// `ForDawModel.blocking`); a held merge blocks too.
pub const BLOCKING: [JobKindRow; 9] = [
    JobKindRow::Bounce,
    JobKindRow::ImportMidi,
    JobKindRow::ExportMidi,
    JobKindRow::ExportProject,
    JobKindRow::ExportDawproject,
    JobKindRow::ImportProject,
    JobKindRow::MergeDawproject,
    JobKindRow::ExportAaf,
    JobKindRow::ImportAaf,
];
/// Jobs that change the media "Add" would add to (Swift `mediaJobs`).
pub const MEDIA_JOBS: [JobKindRow; 4] = [
    JobKindRow::Format,
    JobKindRow::Convert,
    JobKindRow::Relink,
    JobKindRow::ExternalEdit,
];

/// A stem toggle's accessibility label: "Stem for <strip>".
pub fn stem_label(name: &str) -> String {
    format!("Stem for {name}")
}

/// The sheet's choices.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ForDawChoices {
    /// The strips whose stems are written.
    pub stems: BTreeSet<u64>,
    pub format: AudioFormat,
    pub add_to_project: bool,
}

impl Default for ForDawChoices {
    fn default() -> Self {
        ForDawChoices {
            stems: BTreeSet::new(),
            format: AudioFormat::for_project(FALLBACK_FORMAT),
            add_to_project: false,
        }
    }
}

impl AudioFormatChoices for ForDawChoices {
    fn audio_format(&self) -> AudioFormat {
        self.format
    }

    fn set_audio_format(&mut self, f: AudioFormat) {
        self.format = f;
    }
}

/// A check of the choices to run off the UI thread.
#[derive(Debug, Clone, PartialEq)]
pub struct ForDawCheck {
    pub generation: u64,
    pub request: ForDawRequestRow,
}

/// A check's answer, to land on the document.
#[derive(Debug, Clone, PartialEq)]
pub struct ForDawChecked {
    pub generation: u64,
    /// The rate the check was made at (its frames are at that rate).
    pub rate: u32,
    pub answer: Result<ForDawCheckRow, VizError>,
}

impl ForDawCheck {
    /// Asks the core (slow: call it off the UI thread).
    pub fn run(self, s: &dyn DocSession) -> ForDawChecked {
        ForDawChecked {
            generation: self.generation,
            rate: self.request.rate,
            answer: s.export_for_daw_check(self.request),
        }
    }
}

/// The sheet's state on a document (Swift `DocumentModel.forDaw`).
#[derive(Debug, Default)]
pub struct ForDaw {
    shown: bool,
    choices: ForDawChoices,
    /// The strips the stems list offers (every strip but the master).
    strips: Vec<StripRow>,
    /// The first check after showing sends no strips and takes the
    /// core's answer as the stems.
    awaiting_defaults: bool,
    check: Option<ForDawCheckRow>,
    /// The rate `check` was made at.
    check_rate: u32,
    error: Option<String>,
    /// The request kept while a job runs.
    kept: Option<ForDawRequestRow>,
    /// The export job the sheet started (it may have ended).
    job: Option<u64>,
    /// The newest check asked for, and it until the UI takes it.
    generation: u64,
    pending: Option<ForDawCheck>,
}

impl ForDaw {
    /// Whether the sheet is up.
    pub fn shown(&self) -> bool {
        self.shown
    }

    pub fn choices(&self) -> &ForDawChoices {
        &self.choices
    }

    pub fn strips(&self) -> &[StripRow] {
        &self.strips
    }

    /// The core's check of the choices, as last landed.
    pub fn check(&self) -> Option<&ForDawCheckRow> {
        self.check.as_ref()
    }

    /// Why the last check, start or job was refused.
    pub fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }

    /// No answer yet and no refusal: the check is running.
    pub fn checking(&self) -> bool {
        self.check.is_none() && self.error.is_none()
    }

    /// The export is kept until the running job ends.
    pub fn waiting(&self) -> bool {
        self.kept.is_some()
    }

    /// The export job the sheet started (it may have ended).
    pub fn job(&self) -> Option<u64> {
        self.job
    }

    /// The stems' length before the tail at the rate the check was made
    /// at: "0:01.0 + tail", or "" with no check.
    pub fn length(&self) -> String {
        self.check.as_ref().map_or_else(String::new, |c| {
            format!("{} + tail", duration(c.frames, self.check_rate))
        })
    }
}

/// Whether a job of `kind` holds back an export (`add`: one that adds
/// to the project's media).
pub(crate) fn blocks(kind: JobKindRow, add: bool) -> bool {
    BLOCKING.contains(&kind) || (add && MEDIA_JOBS.contains(&kind))
}

impl DocumentModel {
    /// The sheet's state.
    pub fn for_daw(&self) -> &ForDaw {
        &self.for_daw
    }

    /// File › Export › For Another DAW…: the sheet, its choices reset to
    /// the project's format and the core's stems. Showing it again while
    /// an export waits or runs keeps it, with its progress and Cancel.
    pub fn show_for_daw(&mut self) {
        if !self.for_daw.waiting() && !self.for_daw_running() {
            self.reset_for_daw();
        }
        self.for_daw.shown = true;
    }

    /// The sheet's Cancel (nothing waiting or running): it goes.
    pub fn hide_for_daw(&mut self) {
        self.for_daw.shown = false;
    }

    fn reset_for_daw(&mut self) {
        let f = self
            .session
            .project_info()
            .map_or(FALLBACK_FORMAT, |i| i.format);
        let strips: Vec<StripRow> = self
            .session
            .mixer()
            .map(|m| m.strips)
            .unwrap_or_default()
            .into_iter()
            .filter(|s| s.kind != StripKindRow::Master)
            .collect();
        let d = &mut self.for_daw;
        d.choices = ForDawChoices {
            stems: BTreeSet::new(),
            format: AudioFormat::for_project(f),
            add_to_project: false,
        };
        d.strips = strips;
        d.job = None;
        d.kept = None;
        d.check = None;
        d.error = None;
        d.awaiting_defaults = true;
        self.refresh_for_daw();
    }

    /// Edits the choices; a change while the sheet is up is checked again.
    pub fn edit_for_daw(&mut self, f: impl FnOnce(&mut ForDawChoices)) {
        let before = self.for_daw.choices.clone();
        f(&mut self.for_daw.choices);
        if self.for_daw.shown && self.for_daw.choices != before {
            self.refresh_for_daw();
        }
    }

    /// The request the choices make for `path` ("" for a check); `None`
    /// with no sequence shown.
    pub fn for_daw_request(&self, path: &str) -> Option<ForDawRequestRow> {
        let seq = self.selected_seq()?;
        let d = &self.for_daw;
        let c = &d.choices;
        Some(ForDawRequestRow {
            seq,
            strips: (!d.awaiting_defaults).then(|| c.stems.iter().copied().collect()),
            container: c.format.container,
            depth: c.format.depth,
            rate: c.format.rate,
            dither: c.format.sent_dither(),
            path: path.to_owned(),
            add_to_project: c.add_to_project,
        })
    }

    /// Asks for a check of the choices (the UI runs it, see
    /// [`DocumentModel::take_for_daw_check`]); with no sequence shown the
    /// sheet says so at once and any older check is dropped.
    pub fn refresh_for_daw(&mut self) {
        let request = self.for_daw_request("");
        let d = &mut self.for_daw;
        d.generation += 1;
        match request {
            Some(request) => {
                d.pending = Some(ForDawCheck {
                    generation: d.generation,
                    request,
                });
            }
            None => {
                d.pending = None;
                d.check = None;
                d.error = Some(NO_SEQUENCE.into());
            }
        }
    }

    /// The newest check waiting to run, taken (the UI runs it off its
    /// thread and lands it).
    pub fn take_for_daw_check(&mut self) -> Option<ForDawCheck> {
        self.for_daw.pending.take()
    }

    /// A check's answer: the check (the first one's strips become the
    /// stems), or the refusal as the sheet's error. An answer older than
    /// the newest check is dropped.
    pub fn land_for_daw_check(&mut self, c: ForDawChecked) {
        let d = &mut self.for_daw;
        if c.generation != d.generation {
            return;
        }
        match c.answer {
            Ok(row) => {
                if d.awaiting_defaults {
                    d.awaiting_defaults = false;
                    d.choices.stems = row.strips.iter().copied().collect();
                }
                d.check = Some(row);
                d.check_rate = c.rate;
                d.error = None;
            }
            Err(e) => {
                d.check = None;
                d.error = Some(error_message(&e));
            }
        }
    }

    /// Runs the waiting check here and lands it (tests; the UI runs it
    /// off its thread).
    pub fn run_for_daw_check(&mut self) {
        if let Some(c) = self.take_for_daw_check() {
            let checked = c.run(self.session.as_ref());
            self.land_for_daw_check(checked);
        }
    }

    /// The export job is still listed as running.
    pub fn for_daw_running(&self) -> bool {
        self.for_daw
            .job
            .is_some_and(|id| self.jobs.iter().any(|j| j.id == id))
    }

    /// The running export's progress (0–1).
    pub fn for_daw_progress(&self) -> Option<f32> {
        let id = self.for_daw.job?;
        self.jobs.iter().find(|j| j.id == id).map(|j| j.progress)
    }

    /// Whether a job the export cannot run beside is listed (or a merge
    /// is held).
    fn for_daw_blocked(&self, req: &ForDawRequestRow) -> bool {
        self.merge_held || self.jobs.iter().any(|j| blocks(j.kind, req.add_to_project))
    }

    /// The folder panel's answer: exports into `folder`, or keeps the
    /// request until the running job ends. One export at a time from the
    /// sheet: nothing while its export runs.
    pub fn start_for_daw(&mut self, folder: &str) {
        let Some(req) = self.for_daw_request(folder) else {
            self.for_daw.error = Some(NO_SEQUENCE.into());
            return;
        };
        if self.for_daw_running() {
            return;
        }
        self.for_daw.error = None;
        if self.for_daw_blocked(&req) {
            self.for_daw.kept = Some(req);
        } else {
            self.begin_for_daw(req);
        }
    }

    fn begin_for_daw(&mut self, req: ForDawRequestRow) {
        self.for_daw.kept = None;
        match self.session.export_for_daw(req) {
            Ok(id) => {
                // Known before the first poll: a job that ends at once
                // closes the sheet, or brings its message to it.
                self.for_daw.job = Some(id);
                self.list_job(id, JobKindRow::ExportProject);
                self.poll_jobs();
            }
            Err(e) => self.for_daw_failed(error_message(&e)),
        }
    }

    /// The sheet's Cancel while waiting: the kept export is dropped and
    /// nothing is sent.
    pub fn cancel_for_daw_waiting(&mut self) {
        self.for_daw.kept = None;
    }

    /// The sheet's Cancel while the export runs: its end (quiet) arrives
    /// with the next poll and closes the sheet.
    pub fn cancel_for_daw(&mut self) {
        if let Some(id) = self.for_daw.job {
            self.cancel_job(id);
        }
    }

    /// The sheet again, with the choices as they were and `message`. A
    /// check still in flight is dropped, so it cannot erase the message.
    fn for_daw_failed(&mut self, message: String) {
        let d = &mut self.for_daw;
        d.generation += 1;
        d.pending = None;
        d.error = Some(message);
        d.shown = true;
    }

    /// A finished or cancelled export of the sheet's closes the sheet.
    pub(crate) fn for_daw_job_done(&mut self, job: &JobRow) {
        if job.kind == JobKindRow::ExportProject && Some(job.id) == self.for_daw.job {
            self.for_daw.shown = false;
        }
    }

    /// The sheet's export failed: the sheet opens again with the message.
    /// False for anything else (a cancelled one is quiet).
    pub(crate) fn for_daw_job_failed(&mut self, job: &JobRow, message: &str) -> bool {
        if job.kind != JobKindRow::ExportProject
            || Some(job.id) != self.for_daw.job
            || job.state == JobStateRow::Cancelled
        {
            return false;
        }
        self.for_daw_failed(message.to_owned());
        true
    }

    /// A blocking job ended (the job list is settled): the kept request
    /// starts if nothing blocks it any more.
    pub(crate) fn for_daw_job_ended(&mut self) {
        let Some(req) = self.for_daw.kept.clone() else {
            return;
        };
        if !self.for_daw_blocked(&req) {
            self.begin_for_daw(req);
        }
    }
}
