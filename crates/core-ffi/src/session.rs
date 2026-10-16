//! `Session`: one open document (one window).

use crate::error::VizError;
use crate::guard::{ffi_guard, lock};
use crate::host;
use crate::snapshot::{
    DocSnapshot, LaneRow, TransportState, lane_rows, position_bbu, seconds, seq_id, seq_rows,
    warnings,
};
use core_model::command::Command;
use core_model::command::lane::LaneCmd;
use core_model::command::sequence::SequenceCmd;
use core_model::document::Document;
use core_model::file;
use core_model::history::History;
use core_model::ids::{LaneId, SeqId};
use core_model::selection::ViewFilter;
use core_model::time::Tick;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

/// Package extension.
pub const EXTENSION: &str = "vizdaw";

static NEXT_ID: AtomicU64 = AtomicU64::new(1);

struct Inner {
    history: History,
    path: Option<PathBuf>,
    /// The document as last opened or saved; `dirty` compares against it.
    clean: Document,
    version: u64,
    looped: bool,
    /// Where the next Play starts when this session does not hold the
    /// engine; `None` means bar 1 (or resume when it does).
    cursor: Option<Tick>,
    /// The sequence `cursor` belongs to.
    cursor_seq: Option<SeqId>,
    /// Sticky: the last engine recompile after an edit failed. Cleared by
    /// the next successful recompile or Play.
    reload_fault: Option<String>,
}

impl Inner {
    fn new(doc: Document, path: Option<PathBuf>) -> Inner {
        Inner {
            history: History::new(doc.clone()),
            path,
            clean: doc,
            version: 0,
            looped: false,
            cursor: None,
            cursor_seq: None,
            reload_fault: None,
        }
    }

    /// The sequence a position set without the engine belongs to: the one
    /// last played, else the first top-level sequence.
    fn selected_seq(&self) -> Option<SeqId> {
        let doc = self.doc();
        if let Some(s) = self.cursor_seq.filter(|s| doc.sequence(*s).is_ok()) {
            return Some(s);
        }
        let rows = seq_rows(doc).ok()?;
        rows.iter()
            .find(|r| r.top_level)
            .or_else(|| rows.first())
            .map(|r| SeqId(r.id))
    }

    /// Recompiles the engine after an edit. A failure (or panic, which also
    /// resets the host) never fails the edit: it becomes a sticky fault.
    fn reload_host(&mut self, id: u64) {
        let r = ffi_guard(|| host::reload(id, self.history.doc(), self.looped));
        self.reload_fault = match r {
            Ok(()) => None,
            Err(e) => {
                if let VizError::Internal { .. } = e {
                    host::reset();
                }
                Some(format!("reload failed: {e}"))
            }
        };
    }

    fn doc(&self) -> &Document {
        self.history.doc()
    }

    fn snapshot(&self) -> Result<DocSnapshot, VizError> {
        let doc = self.doc();
        Ok(DocSnapshot {
            version: self.version,
            title: title(self.path.as_deref()),
            dirty: *doc != self.clean,
            undo_label: self.history.undo_label().map(str::to_string),
            redo_label: self.history.redo_label().map(str::to_string),
            sequences: seq_rows(doc)?,
            warnings: warnings(doc),
        })
    }
}

/// "demo" for `/x/demo.vizdaw`, "Untitled" without a path.
fn title(path: Option<&Path>) -> String {
    path.and_then(Path::file_stem)
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "Untitled".to_string())
}

/// Appends `.vizdaw` unless present.
fn with_extension(path: &str) -> PathBuf {
    let p = PathBuf::from(path);
    if p.extension().is_some_and(|e| e == EXTENSION) {
        p
    } else {
        let mut s = p.into_os_string();
        s.push(".");
        s.push(EXTENSION);
        PathBuf::from(s)
    }
}

/// One open document with its undo history and transport.
#[derive(uniffi::Object)]
pub struct Session {
    id: u64,
    inner: Mutex<Inner>,
}

impl std::fmt::Debug for Session {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Session")
            .field("id", &self.id)
            .finish_non_exhaustive()
    }
}

impl Session {
    fn with(doc: Document, path: Option<PathBuf>) -> Arc<Session> {
        Arc::new(Session {
            id: NEXT_ID.fetch_add(1, Ordering::Relaxed),
            inner: Mutex::new(Inner::new(doc, path)),
        })
    }

    /// Applies one command as one undo step and recompiles if playing.
    fn execute(&self, cmd: Command) -> Result<DocSnapshot, VizError> {
        let mut inner = lock(&self.inner);
        inner.history.execute(&cmd, &ViewFilter::default())?;
        inner.version += 1;
        inner.reload_host(self.id);
        inner.snapshot()
    }

    /// Undo or redo, then recompile if playing.
    fn step(&self, undo: bool) -> Result<DocSnapshot, VizError> {
        let mut inner = lock(&self.inner);
        if undo {
            inner.history.undo()?;
        } else {
            inner.history.redo()?;
        }
        inner.version += 1;
        inner.reload_host(self.id);
        inner.snapshot()
    }

    /// Runs a transport call; an `Internal` error drops the engine host so
    /// the next Play starts clean.
    fn transport_call<T>(&self, f: impl FnOnce() -> Result<T, VizError>) -> Result<T, VizError> {
        let r = ffi_guard(f);
        if let Err(VizError::Internal { .. }) = &r {
            host::reset();
        }
        r
    }
}

#[uniffi::export]
impl Session {
    /// An untitled, empty document.
    #[uniffi::constructor]
    pub fn new_empty() -> Result<Arc<Session>, VizError> {
        ffi_guard(|| Ok(Session::with(Document::new(), None)))
    }

    /// An untitled copy of the demo project.
    #[uniffi::constructor]
    pub fn new_demo() -> Result<Arc<Session>, VizError> {
        ffi_guard(|| {
            let doc = core_engine::fixtures::demo_document()?;
            Ok(Session::with(doc, None))
        })
    }

    /// Opens the `.vizdaw` package at `path`.
    #[uniffi::constructor]
    pub fn open(path: String) -> Result<Arc<Session>, VizError> {
        ffi_guard(|| {
            let p = PathBuf::from(&path);
            if !p.join(file::PROJECT_FILE).is_file() {
                return Err(VizError::NotFound { path });
            }
            let doc = file::load(&p)?;
            Ok(Session::with(doc, Some(p)))
        })
    }

    /// Saves to the current path; `Io` when untitled.
    pub fn save(&self) -> Result<(), VizError> {
        ffi_guard(|| {
            let mut inner = lock(&self.inner);
            let path = inner.path.clone().ok_or_else(|| VizError::Io {
                message: "the document has no file yet; use Save As".into(),
            })?;
            file::save(inner.doc(), &path)?;
            inner.clean = inner.doc().clone();
            Ok(())
        })
    }

    /// Saves to `path` (`.vizdaw` appended if missing) and makes it the
    /// current path.
    pub fn save_as(&self, path: String) -> Result<(), VizError> {
        ffi_guard(|| {
            let mut inner = lock(&self.inner);
            let p = with_extension(&path);
            file::save(inner.doc(), &p)?;
            inner.clean = inner.doc().clone();
            inner.path = Some(p);
            Ok(())
        })
    }

    /// The current package path.
    pub fn path(&self) -> Result<Option<String>, VizError> {
        ffi_guard(|| {
            let inner = lock(&self.inner);
            Ok(inner
                .path
                .as_ref()
                .map(|p| p.to_string_lossy().into_owned()))
        })
    }

    /// The document at sequence level.
    pub fn snapshot(&self) -> Result<DocSnapshot, VizError> {
        ffi_guard(|| lock(&self.inner).snapshot())
    }

    /// The lanes of sequence `seq`.
    pub fn lanes(&self, seq: u64) -> Result<Vec<LaneRow>, VizError> {
        ffi_guard(|| lane_rows(lock(&self.inner).doc(), seq))
    }

    /// Renames sequence `seq` (one undo step).
    pub fn rename_sequence(&self, seq: u64, name: String) -> Result<DocSnapshot, VizError> {
        ffi_guard(|| {
            self.execute(Command::Sequence(SequenceCmd::Rename {
                seq: SeqId(seq),
                name,
            }))
        })
    }

    /// Mutes or unmutes lane `lane` of sequence `seq` (one undo step).
    pub fn set_lane_mute(&self, seq: u64, lane: u64, mute: bool) -> Result<DocSnapshot, VizError> {
        ffi_guard(|| {
            self.execute(Command::Lane(LaneCmd::SetFlags {
                seq: SeqId(seq),
                lane: LaneId(lane),
                mute: Some(mute),
                solo: None,
                arm: None,
            }))
        })
    }

    /// Reverts the last step.
    pub fn undo(&self) -> Result<DocSnapshot, VizError> {
        ffi_guard(|| self.step(true))
    }

    /// Reapplies the last undone step.
    pub fn redo(&self) -> Result<DocSnapshot, VizError> {
        ffi_guard(|| self.step(false))
    }

    /// Plays sequence `seq`, stopping any other session. Resumes where
    /// this session stopped when the same sequence is still loaded.
    pub fn play(&self, seq: u64) -> Result<(), VizError> {
        self.transport_call(|| {
            let mut inner = lock(&self.inner);
            let id = seq_id(inner.doc(), seq)?;
            // A position set while another session held the engine.
            let start = match (inner.cursor, inner.cursor_seq) {
                (Some(t), None) => Some(t),
                (Some(t), Some(s)) if s == id => Some(t),
                _ => None,
            };
            host::play(self.id, inner.doc(), id, start, inner.looped)?;
            inner.reload_fault = None;
            inner.cursor = None;
            inner.cursor_seq = Some(id);
            Ok(())
        })
    }

    /// Stops; the position is kept.
    pub fn stop(&self) -> Result<(), VizError> {
        self.transport_call(|| host::stop(self.id))
    }

    /// Moves the position to `tick`.
    pub fn locate(&self, tick: i64) -> Result<(), VizError> {
        self.transport_call(|| {
            let mut inner = lock(&self.inner);
            if !host::locate(self.id, Tick(tick))? {
                inner.cursor = Some(Tick(tick));
                inner.cursor_seq = inner.selected_seq();
            }
            Ok(())
        })
    }

    /// Loops from bar 1 to the end of the sequence, or stops looping.
    pub fn set_loop(&self, on: bool) -> Result<(), VizError> {
        self.transport_call(|| {
            let mut inner = lock(&self.inner);
            inner.looped = on;
            host::set_loop(self.id, on)
        })
    }

    /// Where the transport stands.
    pub fn transport(&self) -> Result<TransportState, VizError> {
        self.transport_call(|| {
            let inner = lock(&self.inner);
            let doc = inner.doc();
            Ok(match host::state(self.id) {
                Some(mut h) => TransportState {
                    playing: h.playing,
                    seq: Some(h.seq.0),
                    tick: h.tick.0,
                    position_bbu: position_bbu(doc, h.seq, h.tick),
                    seconds: seconds(doc, h.seq, h.tick),
                    sample_rate: h.sample_rate,
                    device: h.device,
                    faults: {
                        h.faults.extend(inner.reload_fault.clone());
                        h.faults
                    },
                    report: h.report,
                },
                None => {
                    let tick = inner.cursor.unwrap_or(Tick::ZERO);
                    let seq = inner.cursor_seq;
                    TransportState {
                        playing: false,
                        seq: seq.map(|s| s.0),
                        tick: tick.0,
                        position_bbu: seq
                            .map_or_else(|| "1.1.0".to_string(), |s| position_bbu(doc, s, tick)),
                        seconds: seq.map_or(0.0, |s| seconds(doc, s, tick)),
                        sample_rate: 0.0,
                        device: String::new(),
                        faults: inner.reload_fault.iter().cloned().collect(),
                        report: Vec::new(),
                    }
                }
            })
        })
    }
}

#[cfg(feature = "test-hooks")]
#[uniffi::export]
impl Session {
    /// Panics while holding the session lock (test hook): poisons it.
    pub fn debug_panic_locked(&self) -> Result<(), VizError> {
        ffi_guard(|| -> Result<(), VizError> {
            let _inner = lock(&self.inner);
            #[allow(clippy::panic)]
            {
                panic!("panic while locked")
            }
        })
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        host::release(self.id);
    }
}
