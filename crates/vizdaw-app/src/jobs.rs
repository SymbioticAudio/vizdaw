//! Background jobs (Swift: `ProjectMedia.swift` job parts and
//! `ControlJobs`): a document lists the jobs its UI started, polls them
//! from `idle` while any is listed, and answers each ended one through
//! [`answer`], the one answer the UI and the control channel share.

use std::time::Instant;

use core_control::error::ControlError;
use core_control::registry::ArgValue;
use vizdaw_ffi::{
    AafExportRequestRow, AafImportRequestRow, BounceRequestRow, BounceResultRow,
    ClipExportRequestRow, DawprojectExportRequestRow, DawprojectImportRequestRow,
    DawprojectMergeRequestRow, ForDawRequestRow, ImportTargetRow, JobKindRow, JobRow, JobStateRow,
    MediaRow, MidiImportRequestRow, OfflineReasonRow, ProjectFormatRow, ReportRow,
    SmfExportRequestRow, VizError,
};

use crate::alerts::AlertItem;
use crate::document::DocumentModel;
use crate::formats::{int, text};
use crate::session::DocSession;

/// The jobs whose report the UI keeps when they end well (Swift
/// `ProjectMedia.pollJobs`: the MIDI, DAWproject, AAF and For Another DAW
/// imports and exports).
const REPORTING: [JobKindRow; 7] = [
    JobKindRow::ImportMidi,
    JobKindRow::ExportMidi,
    JobKindRow::ExportProject,
    JobKindRow::ExportDawproject,
    JobKindRow::ImportProject,
    JobKindRow::ExportAaf,
    JobKindRow::ImportAaf,
];

/// How often [`DocumentModel::has_copies`] counts the copies again.
pub const COPIES_READ_EVERY: std::time::Duration = std::time::Duration::from_secs(1);

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

/// A job a File command starts, as its typed request: what the control
/// channel parsed (or a sheet filled in), started on a document's
/// session by [`FileJob::start`].
#[derive(Debug, Clone, PartialEq)]
pub enum FileJob {
    /// `media.import`.
    Import {
        paths: Vec<String>,
        target: Option<ImportTargetRow>,
    },
    /// `media.convert`.
    Convert,
    /// `file.project_settings` with a new format.
    Format(ProjectFormatRow),
    /// `media.relink`.
    Relink { media: u64, path: String },
    /// `file.bounce`.
    Bounce(BounceRequestRow),
    /// `file.import_midi`.
    ImportMidi(MidiImportRequestRow),
    /// `file.export_midi`.
    ExportMidi(SmfExportRequestRow),
    /// `file.export_midi_clip`.
    ExportMidiClip(ClipExportRequestRow),
    /// `file.export_for_daw`.
    ForDaw(ForDawRequestRow),
    /// `file.export_dawproject`.
    ExportDawproject(DawprojectExportRequestRow),
    /// `file.import_dawproject` into a new project.
    ImportDawproject(DawprojectImportRequestRow),
    /// `file.import_dawproject` with `mode` merge.
    MergeDawproject(DawprojectMergeRequestRow),
    /// `file.import_aaf`.
    ImportAaf(AafImportRequestRow),
    /// `file.export_aaf`.
    ExportAaf(AafExportRequestRow),
}

impl FileJob {
    /// The kind of job it starts.
    pub fn kind(&self) -> JobKindRow {
        use JobKindRow as K;
        match self {
            FileJob::Import { .. } => K::Import,
            FileJob::Convert => K::Convert,
            FileJob::Format(_) => K::Format,
            FileJob::Relink { .. } => K::Relink,
            FileJob::Bounce(_) => K::Bounce,
            FileJob::ImportMidi(_) => K::ImportMidi,
            FileJob::ExportMidi(_) | FileJob::ExportMidiClip(_) => K::ExportMidi,
            FileJob::ForDaw(_) => K::ExportProject,
            FileJob::ExportDawproject(_) => K::ExportDawproject,
            FileJob::ImportDawproject(_) => K::ImportProject,
            FileJob::MergeDawproject(_) => K::MergeDawproject,
            FileJob::ImportAaf(_) => K::ImportAaf,
            FileJob::ExportAaf(_) => K::ExportAaf,
        }
    }

    /// The title of the alert (the channel: of the failure, "<title>:
    /// <message>") when the session refuses to start it (Swift's
    /// `.thrown(error, …)` titles per command).
    pub fn refusal_title(&self) -> &'static str {
        match self {
            FileJob::Import { .. }
            | FileJob::ImportMidi(_)
            | FileJob::ImportDawproject(_)
            | FileJob::ImportAaf(_) => "Import Failed",
            FileJob::Convert => "Convert Failed",
            FileJob::Format(_) => "Project Settings Failed",
            FileJob::Relink { .. } => "Relink Failed",
            FileJob::Bounce(_) => "Bounce Failed",
            FileJob::ExportMidi(_)
            | FileJob::ExportMidiClip(_)
            | FileJob::ForDaw(_)
            | FileJob::ExportDawproject(_)
            | FileJob::ExportAaf(_) => "Export Failed",
            FileJob::MergeDawproject(_) => "DAWproject Merge Failed",
        }
    }

    /// Starts it on `s`: the job's ID, or the session's refusal.
    pub fn start(&self, s: &dyn DocSession) -> Result<u64, VizError> {
        match self.clone() {
            FileJob::Import { paths, target } => s.import_audio(paths, target),
            FileJob::Convert => s.convert_media(),
            FileJob::Format(f) => s.set_project_format(f),
            FileJob::Relink { media, path } => s.relink_media(media, path),
            FileJob::Bounce(r) => s.bounce(r),
            FileJob::ImportMidi(r) => s.import_midi(r),
            FileJob::ExportMidi(r) => s.export_midi(r),
            FileJob::ExportMidiClip(r) => s.export_midi_clip(r),
            FileJob::ForDaw(r) => s.export_for_daw(r),
            FileJob::ExportDawproject(r) => s.export_dawproject(r),
            FileJob::ImportDawproject(r) => s.import_dawproject(r),
            FileJob::MergeDawproject(r) => s.merge_dawproject(r),
            FileJob::ImportAaf(r) => s.import_aaf(r),
            FileJob::ExportAaf(r) => s.export_aaf(r),
        }
    }
}

/// The refusal of a `media` that names no media item.
pub const NOT_A_MEDIA_ITEM: &str =
    "media.relink: `media` must name a media item (see state.get media)";

/// `media.import` over the channel: one file (`path`; none is the "Open"
/// dialog), placed at `seq`/`lane`/`tick` when `seq` is given, else into
/// the pool only.
pub fn media_import_request(args: &[(String, ArgValue)]) -> Result<FileJob, ControlError> {
    let id = "media.import";
    let Some(path) = text(id, args, "path")? else {
        return Err(ControlError::dialog("Open", &["path"]));
    };
    let id_arg = |name: &str| -> Result<Option<u64>, ControlError> {
        int(id, args, name)?
            .map(|n| {
                u64::try_from(n).map_err(|_| ControlError::params(id, format!("`{name}` is an ID")))
            })
            .transpose()
    };
    let target = match id_arg("seq")? {
        Some(seq) => Some(ImportTargetRow {
            seq,
            lane: id_arg("lane")?,
            tick: int(id, args, "tick")?.unwrap_or(0),
        }),
        None => None,
    };
    Ok(FileJob::Import {
        paths: vec![path.to_owned()],
        target,
    })
}

/// `media.relink` over the channel: `media` (an item `doc`'s session
/// lists) relinked to `path` (none is the "Relink" dialog).
pub fn relink_request(
    args: &[(String, ArgValue)],
    doc: &DocumentModel,
) -> Result<FileJob, ControlError> {
    let id = "media.relink";
    let Some(path) = text(id, args, "path")? else {
        return Err(ControlError::dialog("Relink", &["path"]));
    };
    let media = int(id, args, "media")?
        .and_then(|n| u64::try_from(n).ok())
        .filter(|m| {
            doc.session()
                .media()
                .is_ok_and(|rows| rows.iter().any(|r| r.id == *m))
        })
        .ok_or_else(|| {
            ControlError::new(
                core_control::error::ErrorCode::InvalidParams,
                NOT_A_MEDIA_ITEM,
            )
        })?;
    Ok(FileJob::Relink {
        media,
        path: path.to_owned(),
    })
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
    /// warnings a Problems banner; a cancelled job ends quietly. An import
    /// into a project the store made for its file ends at the store
    /// instead ([`DocumentStore::with_document`](crate::DocumentStore::with_document)
    /// closes the project while untouched). A job the
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
            let opens = matches!(
                r.kind,
                JobKindRow::ImportMidi | JobKindRow::ImportProject | JobKindRow::ImportAaf
            );
            match self.job_answer(&r) {
                JobAnswer::Wait => still.push(r),
                JobAnswer::Ok(_) => {
                    if !r.problems.is_empty() {
                        self.job_problems = r.problems.clone();
                    }
                    // An import or export the UI started keeps its report,
                    // to be shown (Swift `InterchangeModel.jobEnded`; the
                    // report sheet shows it while `report_shown`).
                    if REPORTING.contains(&r.kind)
                        && let Some(report) = &r.report
                    {
                        self.last_report = Some(report.clone());
                        self.report_shown = true;
                    }
                    // The file is in: the project stays whatever happens next.
                    if opens {
                        self.opened_file = false;
                    }
                    self.refresh_after_job();
                }
                JobAnswer::Failed(message) => {
                    if opens && self.opened_file {
                        // The store made this project for the file: it
                        // closes it, or shows the alert ("Import Failed").
                        self.opened_file = false;
                        self.opened_file_end =
                            Some((r.state != JobStateRow::Cancelled).then_some(message));
                    } else if r.state != JobStateRow::Cancelled
                        // A MIDI export's failure goes back to its sheet.
                        && !self.midi_export_job_failed(&r, &message)
                    {
                        self.alert = Some(AlertItem::new(failure_title(r.kind), message));
                    }
                    self.refresh_after_job();
                }
            }
        }
        // A job started while polling (none yet) would be in `self.jobs`.
        still.append(&mut self.jobs);
        self.jobs = still;
        // The Bounce sheet closes once its job ended.
        self.bounce_poll();
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

    /// The report of the last import or export this document's UI ran
    /// (File › Last Import or Export Report… is enabled with one).
    pub fn last_report(&self) -> Option<&ReportRow> {
        self.last_report.as_ref()
    }

    /// A DAWproject merge is held, waiting to be confirmed: Relink waits
    /// (Swift `mergeHeld`, plan 6e Task 11).
    pub fn merge_held(&self) -> bool {
        self.merge_held
    }

    /// The merge sheet holds a merge, or lets it go (slice 7c).
    pub fn set_merge_held(&mut self, held: bool) {
        self.merge_held = held;
    }

    /// The project's media, read again when the media version moved; a
    /// failed read keeps the last rows and is tried again next time
    /// (Swift `MediaModel.rows`).
    pub fn media_rows(&self) -> Vec<MediaRow> {
        let mut cache = self.media_rows.lock().unwrap_or_else(|e| e.into_inner());
        if let Some((v, rows)) = cache.as_ref()
            && *v == self.media_version
        {
            return rows.clone();
        }
        match self.session.media() {
            Ok(rows) => {
                *cache = Some((self.media_version, rows.clone()));
                rows
            }
            Err(_) => cache.as_ref().map(|(_, r)| r.clone()).unwrap_or_default(),
        }
    }

    /// The offline items the Relink sheet lists: missing, changed length,
    /// changed (another format or an unreadable file get the Convert
    /// banner instead; Swift `MediaModel.relinkable`).
    pub fn relinkable(&self) -> Vec<MediaRow> {
        self.media_rows()
            .into_iter()
            .filter(|r| {
                matches!(
                    r.offline,
                    Some(
                        OfflineReasonRow::Missing
                            | OfflineReasonRow::Length
                            | OfflineReasonRow::Changed
                    )
                )
            })
            .collect()
    }

    /// The project is saved and has Co-Producer copies (File › Open
    /// Co-Producer Copy… is enabled). The copies are counted at most once
    /// per [`COPIES_READ_EVERY`]: the menus ask on every frame.
    pub fn has_copies(&self) -> bool {
        if self.path().is_none() {
            return false;
        }
        let mut read = self.copies_read.lock().unwrap_or_else(|e| e.into_inner());
        let now = Instant::now();
        if let Some((at, has)) = *read
            && now.duration_since(at) < COPIES_READ_EVERY
        {
            return has;
        }
        let has = self
            .session
            .co_producer_copies_size()
            .is_ok_and(|c| c.count > 0);
        *read = Some((now, has));
        has
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
