//! `Session`: one open document (one window).

use crate::error::VizError;
use crate::guard::{ffi_guard, lock};
use crate::snapshot::{DocSnapshot, LaneRow, lane_rows, seq_rows, warnings};
use core_model::document::Document;
use core_model::file;
use core_model::history::History;
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
}

impl Inner {
    fn new(doc: Document, path: Option<PathBuf>) -> Inner {
        Inner {
            history: History::new(doc.clone()),
            path,
            clean: doc,
            version: 0,
        }
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
}
