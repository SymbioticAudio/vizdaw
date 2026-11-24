//! Background jobs (Swift: `ProjectMedia.swift` job parts and
//! `ControlJobs`): a document lists the jobs its UI started, polls them
//! from `idle` while any is listed, and answers each ended one through
//! [`answer`], the one answer the UI and the control channel share.

use vizdaw_ffi::{
    BounceResultRow, ImportTargetRow, JobKindRow, JobRow, JobStateRow, ProjectFormatRow, VizError,
};

use crate::alerts::AlertItem;
use crate::document::DocumentModel;
use crate::session::DocSession;

/// How long a job's control call waits for the job to end (the server's
/// app timeout is for calls that are not jobs).
pub const JOB_WAIT_MS: u64 = 600_000;

/// How a job is answered (Swift `JobAnswer`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum JobAnswer {
    /// Still running.
    Wait,
    /// Succeeded; the status carries warnings or what it did.
    Ok(Option<String>),
    /// Failed, with this message.
    Failed(String),
}

/// The answer for `j` (Swift `ControlJobs.answer`): an import or
/// conversion that created nothing and had problems failed; a partial
/// success carries its problems as warnings. `bounce` is the bounce's
/// result when `j` is a bounce.
pub fn answer(j: &JobRow, bounce: Option<&BounceResultRow>) -> JobAnswer {
    use JobKindRow as K;
    match j.state {
        JobStateRow::Running => JobAnswer::Wait,
        JobStateRow::Done => {
            // A bounce wrote its files even with problems (clipped
            // samples, a bypassed plugin): ok, with warnings.
            if j.kind == K::Bounce {
                let empty = BounceResultRow {
                    files: vec![],
                    media: vec![],
                };
                return JobAnswer::Ok(bounce_status(bounce.unwrap_or(&empty), &j.problems));
            }
            // The interchange jobs report in their summary; what they
            // created is not media.
            if matches!(
                j.kind,
                K::ImportMidi
                    | K::ExportMidi
                    | K::ExportProject
                    | K::ExportDawproject
                    | K::ImportProject
                    | K::MergeDawproject
                    | K::ExportAaf
                    | K::ImportAaf
            ) {
                return JobAnswer::Ok(j.report.as_ref().map(|r| r.summary.clone()));
            }
            if j.kind != K::Format && j.created.is_empty() && !j.problems.is_empty() {
                return JobAnswer::Failed(j.problems.join("; "));
            }
            JobAnswer::Ok(
                (!j.problems.is_empty()).then(|| format!("warnings: {}", j.problems.join("; "))),
            )
        }
        JobStateRow::Failed | JobStateRow::Cancelled => {
            // A bounce that failed after some renames names what is on
            // disk (renamed files stay); an export names what it wrote.
            let written: Vec<String> = if j.kind == K::Bounce {
                bounce
                    .map(|b| b.files.iter().map(|f| f.path.clone()).collect())
                    .unwrap_or_default()
            } else if matches!(
                j.kind,
                K::ExportMidi | K::ExportProject | K::ExportDawproject | K::ExportAaf
            ) {
                j.written.clone()
            } else {
                vec![]
            };
            let mut parts = vec![
                j.message
                    .clone()
                    .unwrap_or_else(|| "the job was cancelled".into()),
            ];
            parts.extend(j.problems.iter().cloned());
            if !written.is_empty() {
                parts.push(format!("already written: {}", written.join(", ")));
            }
            JobAnswer::Failed(parts.join("; "))
        }
    }
}

/// A bounce's status (Swift `BounceCommands.status`):
/// `wrote <path> (<frames> frames, <rate> Hz, <bits>)`, then
/// `added N media` and `warnings: …`; `None` when there is nothing to say.
pub fn bounce_status(r: &BounceResultRow, problems: &[String]) -> Option<String> {
    let mut parts: Vec<String> = r
        .files
        .iter()
        .map(|f| {
            format!(
                "wrote {} ({} frames, {} Hz, {})",
                f.path, f.frames, f.rate, f.bits
            )
        })
        .collect();
    if !r.media.is_empty() {
        parts.push(format!("added {} media", r.media.len()));
    }
    if !problems.is_empty() {
        parts.push(format!("warnings: {}", problems.join("; ")));
    }
    (!parts.is_empty()).then(|| parts.join("; "))
}

/// The failure when a control call's wait expires with the job running
/// (Swift `ControlJobs.expiredMessage`).
pub fn expired_message(j: &JobRow) -> String {
    format!(
        "job {} is still running after {} minutes; it was not cancelled",
        j.id,
        JOB_WAIT_MS / 60_000
    )
}

/// Where Import Audio puts files (Swift `DocumentModel.importTarget`):
/// the selected track of the shown sequence for one file, else new
/// tracks; the transport's tick (0 if before it); no target without a
/// sequence (into the pool only).
pub fn import_target(
    seq: Option<u64>,
    selection: &[vizdaw_ffi::edit::LaneRef],
    files: usize,
    tick: i64,
) -> Option<ImportTargetRow> {
    let seq = seq?;
    let lane = if files == 1 {
        selection.iter().find(|r| r.seq == seq).map(|r| r.lane)
    } else {
        None
    };
    Some(ImportTargetRow {
        seq,
        lane,
        tick: tick.max(0),
    })
}

/// The alert title of a failed job (Swift `ProjectBanner.failureTitle`).
pub fn failure_title(kind: JobKindRow) -> &'static str {
    use JobKindRow as K;
    match kind {
        K::Import => "Import Failed",
        K::Relink => "Relink Failed",
        K::Convert | K::Format => "Conversion Failed",
        K::Bounce => "Bounce Failed",
        K::ExternalEdit => "External Edit Failed",
        K::ImportMidi => "MIDI Import Failed",
        K::ExportMidi => "MIDI Export Failed",
        K::ExportProject => "Export Failed",
        K::ExportDawproject => "DAWproject Export Failed",
        K::ImportProject => "DAWproject Import Failed",
        K::MergeDawproject => "DAWproject Merge Failed",
        K::ExportAaf => "AAF Export Failed",
        K::ImportAaf => "AAF Import Failed",
    }
}

/// A running job's banner text (Swift `ProjectBanner.jobText`).
pub fn job_text(kind: JobKindRow) -> &'static str {
    use JobKindRow as K;
    match kind {
        K::Import => "Importing audio…",
        K::Relink => "Relinking media…",
        K::Convert | K::Format => "Converting media…",
        K::Bounce => "Bouncing…",
        K::ExternalEdit => "Importing the external edit…",
        K::ImportMidi => "Importing the MIDI file…",
        K::ExportMidi => "Exporting MIDI…",
        K::ExportProject => "Exporting for another DAW…",
        K::ExportDawproject => "Exporting DAWproject…",
        K::ImportProject => "Importing DAWproject…",
        K::MergeDawproject => "Merging DAWproject…",
        K::ExportAaf => "Exporting AAF…",
        K::ImportAaf => "Importing AAF…",
    }
}

fn running(id: u64, kind: JobKindRow) -> JobRow {
    JobRow {
        id,
        kind,
        progress: 0.0,
        state: JobStateRow::Running,
        message: None,
        problems: vec![],
        created: vec![],
        report: None,
        written: vec![],
        merge: None,
    }
}

impl DocumentModel {
    /// The jobs this document's UI started that have not ended, as last
    /// read.
    pub fn jobs(&self) -> &[JobRow] {
        &self.jobs
    }

    /// Starts a job with `body` and lists it; a refusal is an alert
    /// titled `title`. Answers the new job's ID.
    pub fn start_job(
        &mut self,
        kind: JobKindRow,
        title: &str,
        body: impl FnOnce(&dyn DocSession) -> Result<u64, VizError>,
    ) -> Option<u64> {
        match self.try_start_job(kind, body) {
            Ok(id) => Some(id),
            Err(e) => {
                self.alert = Some(AlertItem::from_error(&e, title));
                None
            }
        }
    }

    /// [`DocumentModel::start_job`] with the refusal returned instead of
    /// shown (a sheet that shows it itself; Swift's `failure:`).
    pub fn try_start_job(
        &mut self,
        kind: JobKindRow,
        body: impl FnOnce(&dyn DocSession) -> Result<u64, VizError>,
    ) -> Result<u64, VizError> {
        let id = body(self.session.as_ref())?;
        self.jobs.push(running(id, kind));
        self.poll_jobs();
        Ok(id)
    }

    /// Asks job `id` to stop (a banner's or sheet's Cancel); its end
    /// arrives with the next poll.
    pub fn cancel_job(&mut self, id: u64) {
        let _ = self.session.cancel_job(id);
    }

    /// [`answer`] for `r`, with the bounce's files when `r` is a bounce;
    /// the app's polling and the control channel both answer through
    /// here.
    pub fn job_answer(&self, r: &JobRow) -> JobAnswer {
        let bounced = if r.kind == JobKindRow::Bounce {
            self.session.bounce_result(r.id).ok().flatten()
        } else {
            None
        };
        answer(r, bounced.as_ref())
    }

    /// Reads every listed job (`idle` calls this while any is listed).
    /// An ended one was applied by the session and refreshes the
    /// document: a failure is an alert titled by [`failure_title`],
    /// warnings a Problems banner; a cancelled job ends quietly. A job the
    /// session no longer knows is dropped (only Busy is retried), so the
    /// polling stops once none is left.
    pub fn poll_jobs(&mut self) {
        let listed = std::mem::take(&mut self.jobs);
        let mut still = Vec::with_capacity(listed.len());
        for j in listed {
            let r = match self.session.job(j.id) {
                Ok(r) => r,
                Err(VizError::Busy { .. }) => {
                    still.push(j);
                    continue;
                }
                Err(_) => continue,
            };
            match self.job_answer(&r) {
                JobAnswer::Wait => still.push(r),
                JobAnswer::Ok(_) => {
                    if !r.problems.is_empty() {
                        self.job_problems = r.problems.clone();
                    }
                    self.refresh_after_job();
                }
                JobAnswer::Failed(message) => {
                    if r.state != JobStateRow::Cancelled {
                        self.alert = Some(AlertItem::new(failure_title(r.kind), message));
                    }
                    self.refresh_after_job();
                }
            }
        }
        // A job started while polling (none yet) would be in `self.jobs`.
        still.append(&mut self.jobs);
        self.jobs = still;
    }

    /// After a job applied: the document, its tracks and the media rows
    /// are new.
    pub fn refresh_after_job(&mut self) {
        if let Ok(s) = self.session.snapshot() {
            self.apply(s);
        }
        self.media_version += 1;
        self.refresh_project_info();
    }

    /// Bumped when the media pool or its offline state changed (views
    /// showing media read it again).
    pub fn media_version(&self) -> u64 {
        self.media_version
    }

    /// The project's format and what does not match it, as last read.
    pub fn project_info(&self) -> Option<&vizdaw_ffi::ProjectInfoRow> {
        self.project_info.as_ref()
    }

    pub fn refresh_project_info(&mut self) {
        self.project_info = self.session.project_info().ok();
    }

    /// File › Import Audio…: imports `paths` as a job, at
    /// [`import_target`] of the shown sequence, selection and transport.
    pub fn import_audio(&mut self, paths: Vec<String>) {
        if paths.is_empty() {
            return;
        }
        let target = import_target(
            self.selected_seq(),
            &self.selection().lanes,
            paths.len(),
            self.last_transport().tick,
        );
        self.import_audio_at(paths, target);
    }

    /// Imports `paths` at `target` (`None`: into the pool only).
    pub fn import_audio_at(&mut self, paths: Vec<String>, target: Option<ImportTargetRow>) {
        if paths.is_empty() {
            return;
        }
        self.start_job(JobKindRow::Import, "Import Failed", |s| {
            s.import_audio(paths, target)
        });
    }

    /// Project Settings: converts every file into `format` (a job).
    /// Answers whether the job started (a refusal is an alert).
    pub fn change_format(&mut self, format: ProjectFormatRow) -> bool {
        self.start_job(JobKindRow::Format, "Project Settings Failed", |s| {
            s.set_project_format(format)
        })
        .is_some()
    }

    /// The mismatched-media banner's Convert.
    pub fn convert_media(&mut self) {
        self.start_job(JobKindRow::Convert, "Convert Failed", |s| s.convert_media());
    }

    /// The device-rate banner's button: the project takes the device's
    /// rate at its depth.
    pub fn change_rate_to_device(&mut self) {
        let Some(i) = &self.project_info else { return };
        if !i.rate_mismatch || i.device_rate <= 0.0 {
            return;
        }
        let format = ProjectFormatRow {
            sample_rate: i.device_rate.round() as u32,
            depth: i.format.depth,
        };
        self.change_format(format);
    }
}
