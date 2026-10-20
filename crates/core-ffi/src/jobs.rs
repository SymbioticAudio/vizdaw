//! Background jobs (plan 5a Ruling 15): imports and conversions run on
//! their own threads with progress and cancellation; the session applies
//! a finished job's result under its lock, as one undo step.

use crate::error::VizError;
use core_media::import::Imported;
use std::collections::BTreeMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicUsize, Ordering};
use std::thread::JoinHandle;

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
    /// Cancelled before it finished (whatever it wrote is removed).
    Cancelled,
}

/// A file in `Media/` a media item is to be relinked to.
#[derive(Debug)]
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
    }
}
