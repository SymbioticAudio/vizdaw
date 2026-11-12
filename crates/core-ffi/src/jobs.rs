//! Background jobs (plan 5a Ruling 15): imports and conversions run on
//! their own threads with progress and cancellation; the session applies
//! a finished job's result under its lock, as one undo step.

use crate::error::VizError;
use core_media::import::Imported;
use std::collections::BTreeMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicUsize, Ordering};
use std::thread::JoinHandle;

/// The stack of a job thread.
const JOB_STACK_BYTES: usize = 16 << 20;

/// What a job does.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum JobKindRow {
    /// Import Audio.
    Import,
    /// Convert media to the project format.
    Convert,
    /// Change the project format.
    Format,
    /// Relink media (a chosen file, or a folder search; plan 5b).
    Relink,
    /// Bounce (plan 5f-2).
    Bounce,
    /// Import an external edit (plan 5f-3).
    ExternalEdit,
    /// Import a MIDI file (plan 6a).
    ImportMidi,
    /// Export MIDI files (plan 6a).
    ExportMidi,
    /// Export the project for another DAW (plan 6b).
    ExportProject,
    /// Export a DAWproject file (plan 6d L1).
    ExportDawproject,
}

/// Where a job is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum JobStateRow {
    /// Working.
    Running,
    /// Applied.
    Done,
    /// Nothing changed; `message` says why.
    Failed,
    /// Cancelled; nothing changed.
    Cancelled,
}

/// A job as the app sees it.
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct JobRow {
    /// Job ID.
    pub id: u64,
    /// Kind.
    pub kind: JobKindRow,
    /// 0–1.
    pub progress: f32,
    /// State.
    pub state: JobStateRow,
    /// Why it failed.
    pub message: Option<String>,
    /// Files that failed while the rest went on ("name: why").
    pub problems: Vec<String>,
    /// Media items it created.
    pub created: Vec<u64>,
    /// What a MIDI import or export did (plan 6a).
    #[uniffi(default = None)]
    pub report: Option<crate::interchange::ReportRow>,
    /// The absolute paths an export put in place (plan 6a).
    #[uniffi(default)]
    pub written: Vec<String>,
}

/// Where imported audio goes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Record)]
pub struct ImportTargetRow {
    /// Sequence.
    pub seq: u64,
    /// Lane (`None`: new lanes; Ruling 19).
    pub lane: Option<u64>,
    /// Tick (a negative tick imports at 0).
    pub tick: i64,
}

/// A job thread's result, applied by the session.
#[derive(Debug)]
pub enum Work {
    /// Per source file: what it became, or why it failed.
    Import {
        /// `(file name, result)` per source, in order.
        files: Vec<(String, Result<Vec<Imported>, String>)>,
        /// Where regions go.
        target: Option<ImportTargetRow>,
    },
    /// Converted files per media item (Task 11): `(item, result)`.
    Convert {
        /// The kind (Convert or Format).
        kind: JobKindRow,
        /// The format change, if any.
        format: Option<core_model::format::AudioFormat>,
        /// `(media, converted file or error)`.
        files: Vec<(core_model::ids::MediaId, Result<Imported, String>)>,
    },
    /// Relink targets per media item (plan 5b Rulings 9, 10).
    Relink {
        /// `(item, its new file or why not)`.
        files: Vec<(core_model::ids::MediaId, Result<RelinkFile, String>)>,
    },
    /// A bounce (plan 5f-2 Rulings 9, 12): its project imports, the files
    /// it exported (already renamed into place) and its problems.
    Bounce {
        /// Per source: what its project import became (empty without "Add to project").
        files: Vec<(String, Result<Vec<Imported>, String>)>,
        /// Where regions go.
        target: Option<ImportTargetRow>,
        /// The files written outside the project, in source order.
        exported: Vec<crate::bounce::BouncedFileRow>,
        /// Clipped samples, placeholders, plugins without offline mode,
        /// underruns, offline media (Rulings 6–8, 13).
        problems: Vec<String>,
        /// A rename failed after `exported` were in place (Ruling 12): the
        /// job is Failed with this message, its files still reported.
        failed: Option<String>,
    },
    /// An external edit's import (plan 5f-3 Ruling 6).
    ExternalEdit {
        /// The region it relinks.
        event: core_model::ids::EventId,
        /// The item the edit started from (`ExternalEdit { from }`).
        from: core_model::ids::MediaId,
        /// What the copy became (more than one item: a file wider than
        /// stereo), or why not.
        file: Result<Vec<Imported>, String>,
    },
    /// A parsed MIDI file and where it lands (plan 6a), applied as one
    /// undo step.
    ImportMidi {
        /// The file (boxed: it is large), or why it could not be read.
        result: Result<Box<core_io::midi_in::MidiImport>, String>,
        /// Where and how it lands.
        place: core_io::midi_in::Placement,
    },
    /// A MIDI or DAWproject export's files (plans 6a, 6d): already in
    /// place; the row's kind says which export it was.
    ExportMidi {
        /// The report, or why the export failed.
        result: Result<core_io::report::Report, String>,
        /// The files put in place, in order (also when a later one failed).
        written: Vec<String>,
    },
    /// An export for another DAW (plan 6b Ruling 7, F1): its folder is in
    /// place on `Ok`; the "Add" imports are applied only then, and
    /// discarded otherwise.
    ExportForDaw {
        /// Per stem: what its "Add" import became (empty without "Add").
        files: Vec<(String, Result<Vec<Imported>, String>)>,
        /// Where regions go.
        target: Option<ImportTargetRow>,
        /// The report, or why the export failed (`"Cancelled"`: it was
        /// cancelled).
        result: Result<core_io::report::Report, String>,
        /// The export folder, once it is in place.
        written: Vec<String>,
    },
    /// Cancelled before it finished (whatever it wrote is removed).
    Cancelled,
}

/// A file in `Media/` a media item is to be relinked to.
#[derive(Debug, Clone)]
pub struct RelinkFile {
    /// The file.
    pub file: Imported,
    /// The job wrote it (else it was already in `Media/`, and it is never
    /// removed when the relink is not applied).
    pub made: bool,
}

struct Job {
    kind: JobKindRow,
    progress: Arc<AtomicU32>,
    cancel: Arc<AtomicBool>,
    thread: Option<JoinHandle<Work>>,
    row: Option<JobRow>,
}

/// A session's jobs.
#[derive(Default)]
pub struct Jobs {
    next: u64,
    list: BTreeMap<u64, Job>,
    /// While set, job threads wait before they start (test hook:
    /// `Session::debug_hold_jobs`).
    held: Arc<AtomicBool>,
    /// Jobs started and not applied yet (running, or finished and
    /// waiting). Shared with job threads: a relink adopts a file in
    /// `Media/` in place only while no other job is unapplied, so it
    /// never adopts a file such a job wrote and may still remove (Task 6
    /// review m1).
    unapplied: Arc<AtomicUsize>,
}

impl std::fmt::Debug for Jobs {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Jobs")
            .field("count", &self.list.len())
            .finish()
    }
}

impl Jobs {
    /// Starts `work` on a thread; it gets a progress cell (f32 bits) and
    /// the cancel flag.
    pub fn spawn(
        &mut self,
        kind: JobKindRow,
        work: impl FnOnce(&AtomicU32, &AtomicBool) -> Work + Send + 'static,
    ) -> Result<u64, VizError> {
        self.next += 1;
        let id = self.next;
        let progress = Arc::new(AtomicU32::new(0f32.to_bits()));
        let cancel = Arc::new(AtomicBool::new(false));
        let (p, c) = (Arc::clone(&progress), Arc::clone(&cancel));
        let held = Arc::clone(&self.held);
        self.unapplied.fetch_add(1, Ordering::SeqCst);
        let thread = std::thread::Builder::new()
            .name(format!("vizdaw-job-{id}"))
            // A large document's model and export need more than the
            // default 2 MiB (plan 6d Task 7 carry).
            .stack_size(JOB_STACK_BYTES)
            .spawn(move || {
                while held.load(Ordering::Relaxed) && !c.load(Ordering::Relaxed) {
                    std::thread::sleep(std::time::Duration::from_millis(1));
                }
                work(&p, &c)
            })
            .map_err(|e| {
                self.unapplied.fetch_sub(1, Ordering::SeqCst);
                VizError::Io {
                    message: format!("job thread: {e}"),
                }
            })?;
        self.list.insert(
            id,
            Job {
                kind,
                progress,
                cancel,
                thread: Some(thread),
                row: None,
            },
        );
        Ok(id)
    }

    /// True while a job has not been applied yet (running, or finished
    /// and waiting for the next `idle`/`job`).
    pub fn busy(&self) -> bool {
        self.list.values().any(|j| j.row.is_none())
    }

    /// True while a job of one of `kinds` has not been applied yet.
    pub fn pending(&self, kinds: &[JobKindRow]) -> bool {
        self.list
            .values()
            .any(|j| j.row.is_none() && kinds.contains(&j.kind))
    }

    /// True when no job thread is still working (finished jobs may still
    /// wait to be applied; test hook).
    pub fn settled(&self) -> bool {
        self.list
            .values()
            .all(|j| j.thread.as_ref().is_none_or(JoinHandle::is_finished))
    }

    /// Holds new and waiting job threads before they start (`true`) or
    /// lets them go (test hook).
    pub fn set_held(&self, on: bool) {
        self.held.store(on, Ordering::Relaxed);
    }

    /// Asks job `id` to stop.
    pub fn cancel(&self, id: u64) {
        if let Some(j) = self.list.get(&id) {
            j.cancel.store(true, Ordering::Relaxed);
        }
    }

    /// The finished jobs' work, to apply (each once; a panicked thread
    /// becomes a failure row at once).
    pub fn finished(&mut self) -> Vec<(u64, JobKindRow, Work)> {
        let mut out = Vec::new();
        for (id, j) in self.list.iter_mut() {
            if j.thread.as_ref().is_some_and(JoinHandle::is_finished) {
                match j.thread.take().map(JoinHandle::join) {
                    Some(Ok(w)) => out.push((*id, j.kind, w)),
                    _ => {
                        self.unapplied.fetch_sub(1, Ordering::SeqCst);
                        j.row = Some(row(
                            *id,
                            j.kind,
                            JobStateRow::Failed,
                            Some("the job panicked".into()),
                            vec![],
                            vec![],
                        ));
                    }
                }
            }
        }
        out
    }

    /// Records how job `id` ended.
    pub fn finish(&mut self, id: u64, row: JobRow) {
        if let Some(j) = self.list.get_mut(&id) {
            if j.row.is_none() {
                self.unapplied.fetch_sub(1, Ordering::SeqCst);
            }
            j.row = Some(row);
        }
    }

    /// The count of unapplied jobs, for job threads (see `unapplied`).
    pub fn unapplied(&self) -> Arc<AtomicUsize> {
        Arc::clone(&self.unapplied)
    }

    /// Job `id` as a row.
    pub fn row(&self, id: u64) -> Option<JobRow> {
        let j = self.list.get(&id)?;
        Some(j.row.clone().unwrap_or_else(|| {
            let p = f32::from_bits(j.progress.load(Ordering::Relaxed));
            let mut r = row(id, j.kind, JobStateRow::Running, None, vec![], vec![]);
            r.progress = p;
            r
        }))
    }

    /// Cancels every job and waits for the threads (the session closes).
    pub fn shutdown(&mut self) -> Vec<Work> {
        for j in self.list.values() {
            j.cancel.store(true, Ordering::Relaxed);
        }
        self.list
            .values_mut()
            .filter_map(|j| j.thread.take()?.join().ok())
            .collect()
    }
}

/// A row with every field given.
pub fn row(
    id: u64,
    kind: JobKindRow,
    state: JobStateRow,
    message: Option<String>,
    problems: Vec<String>,
    created: Vec<u64>,
) -> JobRow {
    JobRow {
        id,
        kind,
        progress: if state == JobStateRow::Running {
            0.0
        } else {
            1.0
        },
        state,
        message,
        problems,
        created,
        report: None,
        written: Vec::new(),
    }
}
