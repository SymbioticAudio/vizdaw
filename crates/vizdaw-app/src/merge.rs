//! The Merge DAWproject sheet's model (Swift `MergeModel`; slice 7c
//! Task 6): the Hold job that previews a merge into this project and
//! keeps it until Merge or Cancel, the project's snapshots when the file
//! lost its export ID (read off the UI thread under a generation counter,
//! [`DocumentModel::take_merge_snapshots`]: only the newest lands), and
//! the core's sentence when anything is refused.
//!
//! A merge waits for a running job the core cannot run it beside (the
//! interchange and media jobs) instead of failing as Busy, and a second
//! merge asked for while one runs, is held or shows waits for it (a
//! third is refused with a status line). A held merge counts as running
//! ([`DocumentModel::merge_held`]): what the core would refuse beside it
//! waits, and goes once the hold ends ([`DocumentModel::set_merge_held`]).
//! Merge and Cancel each run once: both end the job and the hold before
//! anything else is sent. The sentences the core owns (refusals, the
//! summary, the conflict lines) are shown as they come.

use std::collections::HashSet;

use vizdaw_ffi::{
    DawprojectInfoRow, DawprojectMergeRequestRow, JobKindRow, JobRow, JobStateRow, MergePreviewRow,
    MergeStepRow, SnapshotRow, VizError,
};

use crate::alerts::error_message;
use crate::document::DocumentModel;
use crate::for_daw::{BLOCKING, MEDIA_JOBS};
use crate::session::DocSession;

/// The sheet's title (its accessibility label too).
pub const TITLE: &str = "Merge DAWproject";
/// While the merge waits for a running job.
pub const WAITING_TEXT: &str = "Waiting for the running job to finish\u{2026}";
/// While the Hold job reads the file, and its progress bar's label.
pub const READING: &str = "Reading the file\u{2026}";
pub const PROGRESS_LABEL: &str = "Merge progress";
/// The snapshot list's question and accessibility label.
pub const WHICH_EXPORT: &str = "Which export did this file come from?";
pub const EXPORTS_LABEL: &str = "Exports of this project";
/// The preview without and with conflicts.
pub const NO_CONFLICTS: &str = "No conflicts.";
pub const CONFLICTS: &str = "Conflicts (VizDAW's version is kept)";
/// The buttons and their accessibility labels.
pub const CANCEL: &str = "Cancel";
pub const CANCEL_HELD_LABEL: &str = "Cancel the merge; nothing changes";
pub const PREVIEW: &str = "Preview";
pub const PREVIEW_LABEL: &str = "Preview the merge with the chosen export";
pub const PREVIEW_AGAIN: &str = "Preview Again";
pub const PREVIEW_AGAIN_LABEL: &str = "Preview the merge again";
pub const MERGE: &str = "Merge";
pub const MERGE_LABEL: &str = "Merge into this project";
/// The status line when a third merge is asked for.
pub const ANOTHER_WAITING: &str = "A merge is waiting; finish it before opening another.";
/// A Hold job the core ended as Cancelled without a message.
pub const CANCELLED: &str = "The merge was cancelled.";
/// What a held merge holds back says this (Swift `MergeModel.heldHelp`).
pub const HELD_HELP: &str = crate::project_settings::HELD_HELP;

/// The conflict list's accessibility label: "Conflicts, N".
pub fn conflicts_label(n: usize) -> String {
    format!("Conflicts, {n}")
}

/// A snapshot's track count: "1 track", "N tracks".
pub fn tracks_text(n: u32) -> String {
    if n == 1 {
        "1 track".into()
    } else {
        format!("{n} tracks")
    }
}

const MONTHS: [&str; 12] = [
    "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];

/// An export's date and time (Swift `.formatted(date: .abbreviated,
/// time: .shortened)` in English): "Jan 15, 2027 at 8:05 AM", at
/// `offset_secs` east of UTC.
pub fn when_at(unix_secs: u64, offset_secs: i64) -> String {
    let t = unix_secs as i64 + offset_secs;
    let date = crate::interchange_commands::merge_date(unix_secs as i64, offset_secs);
    let mut parts = date.split('-').map(|p| p.parse::<usize>().unwrap_or(0));
    let (y, m, d) = (
        parts.next().unwrap_or(0),
        parts.next().unwrap_or(1),
        parts.next().unwrap_or(1),
    );
    let secs = t.rem_euclid(86_400);
    let (h, min) = (secs / 3_600, secs % 3_600 / 60);
    let (h12, ampm) = match h {
        0 => (12, "AM"),
        1..=11 => (h, "AM"),
        12 => (12, "PM"),
        _ => (h - 12, "PM"),
    };
    let month = MONTHS.get(m.wrapping_sub(1)).copied().unwrap_or("");
    format!("{month} {d}, {y} at {h12}:{min:02} {ampm}")
}

/// [`when_at`] in the local time zone.
pub fn when(unix_secs: u64) -> String {
    when_at(
        unix_secs,
        crate::interchange_commands::local_offset(unix_secs as i64),
    )
}

/// A snapshot row's accessibility label: "Export of <when>, N tracks".
pub fn snapshot_label(row: &SnapshotRow) -> String {
    format!(
        "Export of {}, {}",
        when(row.exported_at),
        tracks_text(row.tracks)
    )
}

/// A snapshot listing to run off the UI thread.
#[derive(Debug, Clone, PartialEq)]
pub struct SnapshotsRead {
    pub generation: u64,
}

/// A listing's answer, to land on the document.
#[derive(Debug, Clone, PartialEq)]
pub struct SnapshotsListed {
    pub generation: u64,
    pub answer: Result<Vec<SnapshotRow>, VizError>,
}

impl SnapshotsRead {
    /// Reads the project's snapshots (slow: call it off the UI thread).
    pub fn run(self, s: &dyn DocSession) -> SnapshotsListed {
        SnapshotsListed {
            generation: self.generation,
            answer: s.dawproject_snapshots(),
        }
    }
}

/// The sheet's state on a document (Swift `DocumentModel.merge`).
#[derive(Debug, Default)]
pub struct Merge {
    shown: bool,
    path: String,
    info: Option<DawprojectInfoRow>,
    snapshots: Vec<SnapshotRow>,
    chosen: Option<String>,
    selected: Option<String>,
    preview: Option<MergePreviewRow>,
    error: Option<String>,
    job: Option<u64>,
    held: bool,
    /// The snapshot kept while the merge waits for a running job.
    kept: Option<Option<String>>,
    /// A merge asked for while this one runs, is held or shows.
    next: Option<(String, DawprojectInfoRow)>,
    /// Jobs Cancel let go of: one that finishes anyway has its hold
    /// dropped.
    abandoned: HashSet<u64>,
    generation: u64,
    pending: Option<SnapshotsRead>,
}

impl Merge {
    pub fn shown(&self) -> bool {
        self.shown
    }

    /// The DAWproject file being merged.
    pub fn path(&self) -> &str {
        &self.path
    }

    /// The project's snapshots, newest first, when the file lost its
    /// export ID (empty otherwise).
    pub fn snapshots(&self) -> &[SnapshotRow] {
        &self.snapshots
    }

    /// The export Preview chose.
    pub fn chosen_snapshot(&self) -> Option<&str> {
        self.chosen.as_deref()
    }

    /// The export selected in the list (the newest when listed).
    pub fn selected_snapshot(&self) -> Option<&str> {
        self.selected.as_deref()
    }

    /// The held merge's preview: the counts line and the conflict lines.
    pub fn preview(&self) -> Option<&MergePreviewRow> {
        self.preview.as_ref()
    }

    /// The core's sentence for the last refusal (job, confirm or listing).
    pub fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }

    /// The Hold job the sheet started (running, then held).
    pub fn job(&self) -> Option<u64> {
        self.job
    }

    /// The Hold job finished and the core keeps its merge until Merge or
    /// Cancel.
    pub fn held(&self) -> bool {
        self.held
    }

    /// The merge waits for a running job.
    pub fn waiting(&self) -> bool {
        self.kept.is_some()
    }

    /// Merge is available: a preview is held.
    pub fn can_merge(&self) -> bool {
        self.held && self.preview.is_some()
    }

    /// The snapshot list shows until a choice starts the job.
    pub fn choosing(&self) -> bool {
        !self.snapshots.is_empty() && self.chosen.is_none() && self.job.is_none() && !self.waiting()
    }

    /// The core let the hold go after the preview: Preview Again.
    pub fn can_preview_again(&self) -> bool {
        self.shown && self.job.is_none() && !self.waiting() && !self.held && self.preview.is_some()
    }
}

impl DocumentModel {
    /// The sheet's state.
    pub fn merge(&self) -> &Merge {
        &self.merge
    }

    /// The Hold job is still listed as running.
    pub fn merge_running(&self) -> bool {
        self.merge
            .job
            .is_some_and(|id| self.jobs.iter().any(|j| j.id == id))
    }

    /// The running Hold job's progress (0–1).
    pub fn merge_progress(&self) -> Option<f32> {
        let id = self.merge.job?;
        self.jobs.iter().find(|j| j.id == id).map(|j| j.progress)
    }

    /// Shows the sheet for `path` (Swift `begin`). Without an export ID
    /// the project's snapshots are listed off the UI thread first;
    /// otherwise the Hold job starts. While another merge runs, is held
    /// or shows, this one waits for it; a third is refused with a status
    /// line, never replacing the second.
    pub fn begin_merge(&mut self, path: &str, info: DawprojectInfoRow) {
        let g = &mut self.merge;
        if g.job.is_some() || g.waiting() || g.shown {
            if g.next.is_some() {
                self.status = Some(ANOTHER_WAITING.into());
            } else {
                g.next = Some((path.to_owned(), info));
            }
            return;
        }
        g.generation += 1;
        g.path = path.to_owned();
        g.chosen = None;
        g.shown = true;
        g.snapshots = vec![];
        g.selected = None;
        g.preview = None;
        g.error = None;
        g.kept = None;
        g.pending = None;
        let listing = info.export_id.is_none();
        g.info = Some(info);
        self.set_held(false);
        if listing {
            self.merge.pending = Some(SnapshotsRead {
                generation: self.merge.generation,
            });
        } else {
            self.start_merge(None);
        }
    }

    /// The snapshot listing waiting to run, taken.
    pub fn take_merge_snapshots(&mut self) -> Option<SnapshotsRead> {
        self.merge.pending.take()
    }

    /// A listing's answer: none to choose starts the Hold job at once
    /// (the core's refusal says why); else the newest is selected. An
    /// answer older than the newest begin, or after the sheet went, is
    /// dropped.
    pub fn land_merge_snapshots(&mut self, l: SnapshotsListed) {
        if l.generation != self.merge.generation || !self.merge.shown {
            return;
        }
        match l.answer {
            Ok(rows) if rows.is_empty() => self.start_merge(None),
            Ok(rows) => {
                self.merge.selected = rows.first().map(|r| r.export_id.clone());
                self.merge.snapshots = rows;
            }
            Err(e) => self.merge.error = Some(error_message(&e)),
        }
    }

    /// Runs the waiting listing here and lands it (tests).
    pub fn run_merge_snapshots(&mut self) {
        if let Some(r) = self.take_merge_snapshots() {
            let listed = r.run(self.session.as_ref());
            self.land_merge_snapshots(listed);
        }
    }

    /// Selects an export in the list.
    pub fn select_merge_snapshot(&mut self, id: Option<&str>) {
        self.merge.selected = id.map(str::to_owned);
    }

    /// Chooses export `id`, which starts the Hold job (Swift
    /// `chosenSnapshot`'s setter).
    pub fn choose_merge_snapshot(&mut self, id: &str) {
        let g = &self.merge;
        if g.chosen.as_deref() == Some(id) || !g.shown || g.job.is_some() || g.waiting() {
            return;
        }
        self.merge.chosen = Some(id.to_owned());
        self.start_merge(Some(id.to_owned()));
    }

    /// Preview: the selected export is chosen.
    pub fn preview_selected_merge(&mut self) {
        if let Some(id) = self.merge.selected.clone() {
            self.choose_merge_snapshot(&id);
        }
    }

    /// Preview Again: a new Hold job for the same file and snapshot.
    pub fn preview_merge_again(&mut self) {
        if !self.merge.can_preview_again() {
            return;
        }
        self.merge.preview = None;
        self.merge.error = None;
        let chosen = self.merge.chosen.clone();
        self.start_merge(chosen);
    }

    /// Whether a job the core refuses a merge beside (Busy) is listed.
    fn merge_blocked(&self) -> bool {
        self.jobs
            .iter()
            .any(|j| BLOCKING.contains(&j.kind) || MEDIA_JOBS.contains(&j.kind))
    }

    fn start_merge(&mut self, snapshot: Option<String>) {
        if self.merge_blocked() {
            self.merge.kept = Some(snapshot);
        } else {
            self.send_merge(snapshot);
        }
    }

    fn send_merge(&mut self, snapshot: Option<String>) {
        let g = &mut self.merge;
        g.kept = None;
        g.error = None;
        let req = DawprojectMergeRequestRow {
            path: g.path.clone(),
            snapshot,
            step: MergeStepRow::Hold,
            date: crate::interchange_commands::merge_date_today(),
        };
        match self.session.merge_dawproject(req) {
            Ok(id) => {
                self.merge.job = Some(id);
                self.list_job(id, JobKindRow::MergeDawproject);
                self.poll_jobs();
            }
            // A refusal (recording, an open edit) shows in the sheet.
            Err(e) => self.merge.error = Some(error_message(&e)),
        }
    }

    /// Merge: the held merge is swapped in, its report opens and the
    /// sheet closes. A refusal stays in the sheet with the core's
    /// sentence; one other than Busy means the core let the hold go, so
    /// Merge is no longer offered.
    pub fn confirm_merge(&mut self) {
        let (Some(job), true) = (self.merge.job, self.merge.held) else {
            return;
        };
        match self.session.confirm_merge(job) {
            Ok(report) => {
                self.merge.job = None;
                self.set_held(false);
                self.refresh_after_job();
                self.show_report(report);
                self.close_merge();
            }
            Err(e) => {
                self.merge.error = Some(error_message(&e));
                if matches!(e, VizError::Busy { .. }) {
                    return;
                }
                self.merge.job = None;
                self.merge.held = false;
                self.set_merge_held(false);
            }
        }
    }

    /// Cancel (the button, Esc): a running job is cancelled, a held merge
    /// let go (with its media); the sheet closes. Nothing in the project
    /// changes.
    pub fn cancel_merge(&mut self) {
        if let Some(job) = self.merge.job {
            if self.merge.held {
                // Its only refusal is "no merge is waiting": no hold is left.
                let _ = self.session.cancel_merge(job);
            } else if self.merge_running() {
                self.merge.abandoned.insert(job);
                self.cancel_job(job);
            }
        }
        self.merge.job = None;
        self.merge.held = false;
        self.close_merge();
    }

    /// The sheet closes; a listing in flight is dropped, what waited for
    /// the hold goes, and a merge asked for meanwhile begins.
    fn close_merge(&mut self) {
        let g = &mut self.merge;
        g.generation += 1;
        g.pending = None;
        g.kept = None;
        g.shown = false;
        self.set_merge_held(false);
        if let Some((path, info)) = self.merge.next.take() {
            self.begin_merge(&path, info);
        }
    }

    fn set_held(&mut self, held: bool) {
        self.merge.held = held;
        self.merge_held = held;
    }

    /// A merge job ended Done or Cancelled (polling): the Hold job's
    /// preview is shown and the merge is held. A done Hold job is a
    /// preview, not a report: nothing else opens.
    pub(crate) fn merge_job_done(&mut self, r: &JobRow) {
        if r.kind != JobKindRow::MergeDawproject {
            return;
        }
        if self.merge.abandoned.remove(&r.id) {
            // Cancelled too late: the core holds it; let it go.
            if r.state == JobStateRow::Done {
                let _ = self.session.cancel_merge(r.id);
            }
            return;
        }
        if Some(r.id) != self.merge.job {
            return;
        }
        if r.state == JobStateRow::Cancelled {
            // The core ended the hold: say so, or the sheet shows nothing.
            self.merge.job = None;
            self.set_held(false);
            self.merge.error = Some(r.message.clone().unwrap_or_else(|| CANCELLED.into()));
            return;
        }
        self.merge.preview = r.merge.clone();
        self.set_held(true);
    }

    /// The Hold job failed: the core's sentence shows in the sheet, not
    /// an alert. Answers whether it did.
    pub(crate) fn merge_job_failed(&mut self, r: &JobRow, message: &str) -> bool {
        if r.kind != JobKindRow::MergeDawproject {
            return false;
        }
        if self.merge.abandoned.remove(&r.id) {
            return true;
        }
        if Some(r.id) != self.merge.job || r.state == JobStateRow::Cancelled {
            return false;
        }
        self.merge.job = None;
        self.set_held(false);
        self.merge.error = Some(message.to_owned());
        true
    }

    /// A job ended: a waiting merge starts if nothing blocks it any more.
    pub(crate) fn merge_job_ended(&mut self) {
        let Some(snapshot) = self.merge.kept.clone() else {
            return;
        };
        if !self.merge_blocked() {
            self.send_merge(snapshot);
        }
    }
}
