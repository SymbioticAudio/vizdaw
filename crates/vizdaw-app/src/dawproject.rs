//! File › Export › DAWproject… (Swift `DawprojectModel`,
//! `DawprojectSheet`; slice 7c Task 4): the region gain choice, the
//! core's check of what will not transfer (off the UI thread, only the
//! newest answer lands, [`DocumentModel::take_dawproject_check`]) and the
//! core's file name for the shown sequence. Export… asks for the file
//! (panel #13, named by the core); the panel confirmed replacing only
//! what was at the path when it answered, so `replace` is sent only then
//! (a file that appears while the export waits is the core's refusal,
//! never replaced unasked). While a job the core cannot run an export
//! beside is listed (the interchange and media jobs, or a held merge)
//! the export waits ("Waiting for the running job to finish…") instead
//! of failing as Busy. An `ExportDawproject` job ("DAWproject Export
//! Failed"); the sheet stays up with progress and Cancel while it runs
//! and closes when it ends; a refusal or a failed job brings it back with
//! the message, never an alert.

use vizdaw_ffi::{
    DawprojectExportRequestRow, JobKindRow, JobRow, JobStateRow, RegionGainRow, ReportRow, VizError,
};

use crate::alerts::error_message;
use crate::document::DocumentModel;
use crate::for_daw::{BLOCKING, MEDIA_JOBS, NO_SEQUENCE};
use crate::session::DocSession;

/// The sheet's title and subtitle.
pub const TITLE: &str = "Export DAWproject";
pub const SUBTITLE: &str = "Most complete: tracks, mix, plug-ins, automation";
/// The region gain picker (its accessibility label is the same) and its
/// choices.
pub const REGION_GAIN: &str = "Region gain";
pub const APPLY: &str = "Apply to the audio";
pub const LEAVE_OUT: &str = "Leave out";
/// The waiting row's text.
pub const WAITING_TEXT: &str = "Waiting for the running job to finish\u{2026}";
/// The title of a failed export job (its message shows in the sheet).
pub const EXPORT_FAILED: &str = "DAWproject Export Failed";

/// A check of the choice to run off the UI thread.
#[derive(Debug, Clone, PartialEq)]
pub struct DawprojectCheck {
    pub generation: u64,
    pub request: DawprojectExportRequestRow,
}

/// A check's answer, to land on the document.
#[derive(Debug, Clone, PartialEq)]
pub struct DawprojectChecked {
    pub generation: u64,
    pub answer: Result<ReportRow, VizError>,
}

impl DawprojectCheck {
    /// Asks the core (slow: call it off the UI thread).
    pub fn run(self, s: &dyn DocSession) -> DawprojectChecked {
        DawprojectChecked {
            generation: self.generation,
            answer: s.export_dawproject_check(self.request),
        }
    }
}

/// The sheet's state on a document (Swift `DocumentModel.dawproject`).
#[derive(Debug)]
pub struct Dawproject {
    shown: bool,
    region_gain: RegionGainRow,
    wont_transfer: Option<ReportRow>,
    error: Option<String>,
    /// The core's file name for the shown sequence.
    default_name: String,
    kept: Option<DawprojectExportRequestRow>,
    job: Option<u64>,
    generation: u64,
    pending: Option<DawprojectCheck>,
}

impl Default for Dawproject {
    fn default() -> Self {
        Dawproject {
            shown: false,
            region_gain: RegionGainRow::Apply,
            wont_transfer: None,
            error: None,
            default_name: String::new(),
            kept: None,
            job: None,
            generation: 0,
            pending: None,
        }
    }
}

impl Dawproject {
    pub fn shown(&self) -> bool {
        self.shown
    }

    pub fn region_gain(&self) -> RegionGainRow {
        self.region_gain
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

    /// The core's file name ("" when it gave none).
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

impl DocumentModel {
    /// The sheet's state.
    pub fn dawproject(&self) -> &Dawproject {
        &self.dawproject
    }

    /// File › Export › DAWproject…: the sheet, with the core's name and
    /// a fresh check. Showing it again while an export waits or runs keeps
    /// it, with its progress and Cancel.
    pub fn show_dawproject(&mut self) {
        if !self.dawproject.waiting() && !self.dawproject_running() {
            let name = self
                .selected_seq()
                .and_then(|seq| self.session.dawproject_name(seq).ok())
                .unwrap_or_default();
            let d = &mut self.dawproject;
            d.job = None;
            d.kept = None;
            d.error = None;
            d.wont_transfer = None;
            d.default_name = name;
            self.refresh_dawproject();
        }
        self.dawproject.shown = true;
    }

    /// The sheet's Cancel (nothing waiting or running): it goes.
    pub fn hide_dawproject(&mut self) {
        self.dawproject.shown = false;
    }

    /// The region gain choice; a change while the sheet is up is checked
    /// again.
    pub fn set_dawproject_region_gain(&mut self, gain: RegionGainRow) {
        let changed = self.dawproject.region_gain != gain;
        self.dawproject.region_gain = gain;
        if changed && self.dawproject.shown {
            self.refresh_dawproject();
        }
    }

    /// The request for `path` ("" for a check); the blocks follow the
    /// editor's block rule. `None` with no sequence shown.
    pub fn dawproject_request(
        &self,
        path: &str,
        replace: bool,
    ) -> Option<DawprojectExportRequestRow> {
        Some(DawprojectExportRequestRow {
            seq: self.selected_seq()?,
            path: path.to_owned(),
            replace,
            region_gain: self.dawproject.region_gain,
            blocks: self.layout().block_rule.rule(),
        })
    }

    /// Asks for a check (the UI runs it, see
    /// [`DocumentModel::take_dawproject_check`]); with no sequence shown
    /// the sheet says so at once and any older check is dropped.
    pub fn refresh_dawproject(&mut self) {
        let request = self.dawproject_request("", false);
        let d = &mut self.dawproject;
        d.generation += 1;
        match request {
            Some(request) => {
                d.pending = Some(DawprojectCheck {
                    generation: d.generation,
                    request,
                });
            }
            None => {
                d.pending = None;
                d.wont_transfer = None;
                d.error = Some(NO_SEQUENCE.into());
            }
        }
    }

    /// The newest check waiting to run, taken.
    pub fn take_dawproject_check(&mut self) -> Option<DawprojectCheck> {
        self.dawproject.pending.take()
    }

    /// A check's answer: what won't transfer, or the refusal as the
    /// sheet's error. An answer older than the newest check is dropped.
    pub fn land_dawproject_check(&mut self, c: DawprojectChecked) {
        let d = &mut self.dawproject;
        if c.generation != d.generation {
            return;
        }
        match c.answer {
            Ok(r) => {
                d.wont_transfer = Some(r);
                d.error = None;
            }
            Err(e) => {
                d.wont_transfer = None;
                d.error = Some(error_message(&e));
            }
        }
    }

    /// Runs the waiting check here and lands it (tests).
    pub fn run_dawproject_check(&mut self) {
        if let Some(c) = self.take_dawproject_check() {
            let checked = c.run(self.session.as_ref());
            self.land_dawproject_check(checked);
        }
    }

    /// The export job is still listed as running.
    pub fn dawproject_running(&self) -> bool {
        self.dawproject
            .job
            .is_some_and(|id| self.jobs.iter().any(|j| j.id == id))
    }

    /// The running export's progress (0–1).
    pub fn dawproject_progress(&self) -> Option<f32> {
        let id = self.dawproject.job?;
        self.jobs.iter().find(|j| j.id == id).map(|j| j.progress)
    }

    /// Whether a job the core refuses an export beside (Busy) is listed,
    /// or a merge is held.
    fn dawproject_blocked(&self) -> bool {
        self.merge_held
            || self
                .jobs
                .iter()
                .any(|j| BLOCKING.contains(&j.kind) || MEDIA_JOBS.contains(&j.kind))
    }

    /// The save panel's answer: `path`, replacing only when `taken` (the
    /// path was taken when the panel confirmed, Swift `confirmed`).
    /// Exports, or keeps the request until the running job ends; nothing
    /// while the sheet's export runs.
    pub fn start_dawproject(&mut self, path: &str, taken: bool) {
        let Some(req) = self.dawproject_request(path, taken) else {
            self.dawproject.error = Some(NO_SEQUENCE.into());
            return;
        };
        if self.dawproject_running() {
            return;
        }
        self.dawproject.error = None;
        if self.dawproject_blocked() {
            self.dawproject.kept = Some(req);
        } else {
            self.begin_dawproject(req);
        }
    }

    fn begin_dawproject(&mut self, req: DawprojectExportRequestRow) {
        self.dawproject.kept = None;
        match self.session.export_dawproject(req) {
            Ok(id) => {
                self.dawproject.job = Some(id);
                self.list_job(id, JobKindRow::ExportDawproject);
                self.poll_jobs();
            }
            Err(e) => self.dawproject_failed(error_message(&e)),
        }
    }

    /// The sheet's Cancel while waiting: nothing is sent.
    pub fn cancel_dawproject_waiting(&mut self) {
        self.dawproject.kept = None;
    }

    /// The sheet's Cancel while the export runs.
    pub fn cancel_dawproject(&mut self) {
        if let Some(id) = self.dawproject.job {
            self.cancel_job(id);
        }
    }

    /// The sheet again with `message`; a check in flight is dropped so it
    /// cannot erase it.
    fn dawproject_failed(&mut self, message: String) {
        let d = &mut self.dawproject;
        d.generation += 1;
        d.pending = None;
        d.error = Some(message);
        d.shown = true;
    }

    /// A finished or cancelled export of the sheet's closes the sheet.
    pub(crate) fn dawproject_job_done(&mut self, job: &JobRow) {
        if job.kind == JobKindRow::ExportDawproject && Some(job.id) == self.dawproject.job {
            self.dawproject.shown = false;
        }
    }

    /// The sheet's export failed: the sheet again with the message, and
    /// no report. False for anything else.
    pub(crate) fn dawproject_job_failed(&mut self, job: &JobRow, message: &str) -> bool {
        if job.kind != JobKindRow::ExportDawproject
            || Some(job.id) != self.dawproject.job
            || job.state == JobStateRow::Cancelled
        {
            return false;
        }
        self.dawproject_failed(message.to_owned());
        true
    }

    /// A blocking job ended: the kept request starts if nothing blocks it.
    pub(crate) fn dawproject_job_ended(&mut self) {
        let Some(req) = self.dawproject.kept.clone() else {
            return;
        };
        if !self.dawproject_blocked() {
            self.begin_dawproject(req);
        }
    }
}
