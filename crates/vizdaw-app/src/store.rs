//! The open documents (Swift: `DocumentStore`). Windows are plain tokens;
//! the GUI layer maps real windows onto them.

use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use vizdaw_ffi::VizError;
use vizdaw_ffi::{
    AafInfoRow, BounceSettingsRow, DawprojectInfoRow, OpenOptions, ProjectFormatRow, RecoveryInfo,
};

use crate::alerts::{AlertItem, error_message};
use crate::document::DocumentModel;
use crate::session::{CoreCalls, DocSession, LiveCore};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct DocId(pub u64);

/// Identifies one window of a document.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct WindowToken(pub u64);

type Make = Box<dyn Fn() -> Result<Arc<dyn DocSession>, VizError> + Send + Sync>;
type OpenFn = Box<dyn Fn(&str) -> Result<Arc<dyn DocSession>, VizError> + Send + Sync>;

/// `CoreCalls` from the three makers slices 1–6 build stores with; every
/// other constructor and file read is refused (`not_scripted`), and the
/// pure format calls answer as the core does.
struct Makers {
    make_empty: Make,
    make_demo: Make,
    open_path: OpenFn,
}

fn not_scripted(call: &str) -> VizError {
    VizError::Model {
        message: format!("{call} is not available in this store"),
    }
}

impl CoreCalls for Makers {
    fn new_empty(&self) -> Result<Arc<dyn DocSession>, VizError> {
        (self.make_empty)()
    }
    fn new_demo(&self) -> Result<Arc<dyn DocSession>, VizError> {
        (self.make_demo)()
    }
    fn new_demo_audio(&self) -> Result<Arc<dyn DocSession>, VizError> {
        Err(not_scripted("new_demo_audio"))
    }
    fn new_project(&self, _format: ProjectFormatRow) -> Result<Arc<dyn DocSession>, VizError> {
        Err(not_scripted("new_project"))
    }
    fn open(&self, path: &str) -> Result<Arc<dyn DocSession>, VizError> {
        (self.open_path)(path)
    }
    fn open_with(
        &self,
        _path: &str,
        _options: OpenOptions,
    ) -> Result<Arc<dyn DocSession>, VizError> {
        Err(not_scripted("open_with"))
    }
    fn open_copy(&self, _path: &str) -> Result<Arc<dyn DocSession>, VizError> {
        Err(not_scripted("open_copy"))
    }
    fn dawproject_info(&self, _path: &str) -> Result<DawprojectInfoRow, VizError> {
        Err(not_scripted("dawproject_info"))
    }
    fn aaf_info(&self, _path: &str) -> Result<AafInfoRow, VizError> {
        Err(not_scripted("aaf_info"))
    }
    fn recovery_check(&self, _path: &str) -> Result<RecoveryInfo, VizError> {
        Err(not_scripted("recovery_check"))
    }
    fn project_rates(&self) -> Vec<u32> {
        LiveCore.project_rates()
    }
    fn format_label(&self, format: ProjectFormatRow) -> String {
        LiveCore.format_label(format)
    }
    fn bounce_settings(&self) -> BounceSettingsRow {
        LiveCore.bounce_settings()
    }
}

pub struct DocumentStore {
    core: Arc<dyn CoreCalls>,
    documents: HashMap<DocId, DocumentModel>,
    next_id: u64,
    key_window: Option<(DocId, WindowToken)>,
    /// Documents whose window was key, most recent first.
    recent_key: Vec<DocId>,
    alert: Option<AlertItem>,
    queued_alerts: VecDeque<AlertItem>,
    last_failure: Option<String>,
}

impl DocumentStore {
    /// A store whose new, demo and open come from these makers (the rest
    /// of `CoreCalls` is refused; see `with_core`).
    pub fn new(make_empty: Make, make_demo: Make, open: OpenFn) -> Self {
        DocumentStore::with_core(Arc::new(Makers {
            make_empty,
            make_demo,
            open_path: open,
        }))
    }

    /// A store over `core`'s constructors and free functions.
    pub fn with_core(core: Arc<dyn CoreCalls>) -> Self {
        DocumentStore {
            core,
            documents: HashMap::new(),
            next_id: 1,
            key_window: None,
            recent_key: Vec::new(),
            alert: None,
            queued_alerts: VecDeque::new(),
            last_failure: None,
        }
    }

    /// A store over real core sessions.
    pub fn live() -> Self {
        DocumentStore::with_core(Arc::new(LiveCore))
    }

    /// The core calls this store makes and reads files with (a clone can
    /// go to a background thread).
    pub fn core(&self) -> Arc<dyn CoreCalls> {
        self.core.clone()
    }

    pub fn document(&self, id: DocId) -> Option<&DocumentModel> {
        self.documents.get(&id)
    }

    pub fn document_mut(&mut self, id: DocId) -> Option<&mut DocumentModel> {
        self.documents.get_mut(&id)
    }

    /// Runs `f` with document `id` and the rest of the store (for calls that
    /// need both, such as `TransportModel::play_stop`, whose alerts go to
    /// the store). The document is out of the store while `f` runs, so `f`
    /// must not open, close or look up documents; it is put back after.
    /// An alert the document raised (save, undo) moves to the store, the
    /// single alert channel. `None` when `id` is not open.
    pub fn with_document<R>(
        &mut self,
        id: DocId,
        f: impl FnOnce(&mut DocumentModel, &mut DocumentStore) -> R,
    ) -> Option<R> {
        let mut doc = self.documents.remove(&id)?;
        let result = f(&mut doc, self);
        if let Some(alert) = doc.alert.take() {
            self.post_alert(alert);
        }
        self.documents.insert(id, doc);
        Some(result)
    }

    /// Document `id`'s mixer, made when first asked (one per document,
    /// gone with it). An alert it raises stays on the document.
    pub fn mixer(&mut self, id: DocId) -> Option<&crate::mixer::MixerModel> {
        let doc = self.documents.get_mut(&id)?;
        doc.ensure_mixer();
        doc.mixer()
    }

    pub fn count(&self) -> usize {
        self.documents.len()
    }

    pub fn new_empty(&mut self) -> Option<DocId> {
        self.add("New Document Failed", |s| s.core.new_empty())
    }

    pub fn new_demo(&mut self) -> Option<DocId> {
        self.add("Demo Project Failed", |s| s.core.new_demo())
    }

    /// Opens a `.vizdaw` package. A refusal or failure is also posted as an
    /// alert. A project that is already open answers its existing document.
    pub fn open(&mut self, path: &Path) -> Result<DocId, String> {
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        if path.extension().and_then(|e| e.to_str()) != Some("vizdaw") {
            let message = format!("{name} is not a .vizdaw project.");
            self.post_alert(AlertItem::new("Open Failed", message.clone()));
            return Err(message);
        }
        if let Some((id, _)) = self
            .documents
            .iter()
            .find(|(_, d)| d.path().map(|p| canonical(&p)) == Some(canonical(path)))
        {
            return Ok(*id);
        }
        let text = path.to_string_lossy().into_owned();
        match self.add("Open Failed", |s| s.core.open(&text)) {
            Some(id) => Ok(id),
            None => Err(self
                .last_failure
                .take()
                .unwrap_or_else(|| "open failed".into())),
        }
    }

    fn add(
        &mut self,
        title: &str,
        make: impl FnOnce(&Self) -> Result<Arc<dyn DocSession>, VizError>,
    ) -> Option<DocId> {
        let made = make(self).and_then(DocumentModel::new);
        match made {
            Ok(model) => {
                let id = DocId(self.next_id);
                self.next_id += 1;
                self.documents.insert(id, model);
                Some(id)
            }
            Err(e) => {
                self.last_failure = Some(error_message(&e));
                self.post_alert(AlertItem::from_error(&e, title));
                None
            }
        }
    }

    /// A window of `doc` took key.
    pub fn focus_document(&mut self, doc: DocId, window: WindowToken) {
        // A closing window can take key after its document went.
        if !self.documents.contains_key(&doc) {
            return;
        }
        self.key_window = Some((doc, window));
        self.recent_key.retain(|d| *d != doc);
        self.recent_key.insert(0, doc);
    }

    /// The document whose own window is key.
    pub fn key_document(&self) -> Option<DocId> {
        self.key_window.map(|(d, _)| d)
    }

    /// The window that is key, with its document (a document's own
    /// window or its Mixer window).
    pub fn key_window(&self) -> Option<(DocId, WindowToken)> {
        self.key_window
    }

    /// The front-most document still open: the one key last.
    pub fn front(&self) -> Option<DocId> {
        self.recent_key
            .iter()
            .copied()
            .find(|d| self.documents.contains_key(d))
    }

    /// A window closed: its document goes. Closing twice is harmless.
    pub fn close(&mut self, id: DocId) {
        if self.key_window.map(|(d, _)| d) == Some(id) {
            self.key_window = None;
        }
        self.recent_key.retain(|d| *d != id);
        if let Some(mut doc) = self.documents.remove(&id) {
            doc.audition_off_all();
            // No mixer touch outlives its document.
            doc.end_mixer_drags(std::time::Instant::now());
            let session = doc.session();
            if session.transport().map(|t| t.playing).unwrap_or(false) {
                let _ = session.stop();
            }
            let _ = session.close();
        }
    }

    pub fn close_all(&mut self) {
        let ids: Vec<DocId> = self.documents.keys().copied().collect();
        for id in ids {
            self.close(id);
        }
    }

    /// Shows `item` now, or after the alerts already showing or waiting;
    /// none is dropped.
    pub fn post_alert(&mut self, item: AlertItem) {
        if self.alert.is_none() && self.queued_alerts.is_empty() {
            self.alert = Some(item);
        } else {
            self.queued_alerts.push_back(item);
        }
    }

    /// The alert showing now, if any.
    pub fn alert(&self) -> Option<&AlertItem> {
        self.alert.as_ref()
    }

    /// The store-level alert, cleared; the next queued one takes its place.
    pub fn take_alert(&mut self) -> Option<AlertItem> {
        let current = self.alert.take().or_else(|| self.queued_alerts.pop_front());
        self.alert = self.queued_alerts.pop_front();
        current
    }
}

/// A path as the file system names it (symlinks, `..` and trailing
/// slashes resolved); a path that does not exist is compared as given.
fn canonical(path: &Path) -> PathBuf {
    std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}
