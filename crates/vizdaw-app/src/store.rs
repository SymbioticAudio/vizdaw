//! The open documents (Swift: `DocumentStore`). Windows are plain tokens;
//! the GUI layer maps real windows onto them.

mod opening;

use std::collections::{HashMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use vizdaw_ffi::VizError;
use vizdaw_ffi::{
    AafInfoRow, BounceSettingsRow, DawprojectInfoRow, OpenOptions, ProjectFormatRow, RecoveryInfo,
};

use crate::alerts::{AlertItem, error_message};
use crate::document::DocumentModel;
use crate::session::{CoreCalls, DocSession, LiveCore};

pub use opening::{
    CrashChoice, DisablePlugins, Landed, MidiRead, OPEN_FAILED, OpenQuestion, Opened, ReturnChoice,
    ReturnPrompt, SlowRead, Spawner, is_aaf, is_copy, is_dawproject, is_midi, project_path,
};

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
    /// Without a format maker a new project is an empty one (Swift).
    fn new_project(&self, _format: ProjectFormatRow) -> Result<Arc<dyn DocSession>, VizError> {
        (self.make_empty)()
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
    /// The alerts waiting to show, oldest first, each with the document
    /// whose window shows it (`None`: the front window).
    alerts: VecDeque<(Option<DocId>, AlertItem)>,
    last_failure: Option<String>,
    default_format: ProjectFormatRow,
    /// Projects made for a file that landed later, waiting for a window.
    pending: VecDeque<DocId>,
    /// Documents the store closed itself, whose windows should close.
    closed: Vec<DocId>,
    /// Projects a channel import made, whose job has not ended.
    made: HashSet<DocId>,
    return_prompt: Option<ReturnPrompt>,
    queued_prompts: VecDeque<ReturnPrompt>,
    /// Opens waiting for the recovery questions, the first asking.
    questions: VecDeque<opening::PendingOpen>,
    reads: opening::Reads,
    shift_held: Box<dyn Fn() -> bool>,
    disable_plugins: opening::DisablePlugins,
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
            alerts: VecDeque::new(),
            last_failure: None,
            default_format: crate::formats::FALLBACK_FORMAT,
            pending: VecDeque::new(),
            closed: vec![],
            made: HashSet::new(),
            return_prompt: None,
            queued_prompts: VecDeque::new(),
            questions: VecDeque::new(),
            reads: opening::Reads::default(),
            shift_held: Box::new(|| false),
            disable_plugins: Box::new(|_| false),
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
    /// single alert channel, naming the document (its window shows it). An import into a project the store made for
    /// its file that failed or was cancelled (polled in `f`) closes the
    /// project while untouched (see [`DocumentStore::take_closed`]).
    /// `None` when `id` is not open.
    pub fn with_document<R>(
        &mut self,
        id: DocId,
        f: impl FnOnce(&mut DocumentModel, &mut DocumentStore) -> R,
    ) -> Option<R> {
        let mut doc = self.documents.remove(&id)?;
        let result = f(&mut doc, self);
        let alert = doc.alert.take();
        self.documents.insert(id, doc);
        if let Some(alert) = alert {
            self.post_document_alert(id, alert);
        }
        // An import into a project made for its file ended badly.
        self.settle_opened_file(id);
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
        self.made.remove(&id);
        self.pending.retain(|d| *d != id);
        self.reads.midi_generation.remove(&id);
        // Its alerts are never lost: the front window shows them.
        for (doc, _) in self.alerts.iter_mut() {
            if *doc == Some(id) {
                *doc = None;
            }
        }
        if let Some(mut doc) = self.documents.remove(&id) {
            if let Some(a) = doc.alert.take() {
                self.alerts.push_back((None, a));
            }
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
    /// none is dropped. A store alert: the front window shows it.
    pub fn post_alert(&mut self, item: AlertItem) {
        self.alerts.push_back((None, item));
    }

    /// As [`post_alert`](Self::post_alert), shown in `doc`'s own window
    /// (the front window once it is closed).
    pub fn post_document_alert(&mut self, doc: DocId, item: AlertItem) {
        let doc = self.documents.contains_key(&doc).then_some(doc);
        self.alerts.push_back((doc, item));
    }

    /// The alert showing now (the oldest waiting), if any: what the
    /// control channel reports.
    pub fn alert(&self) -> Option<&AlertItem> {
        self.alerts.front().map(|(_, a)| a)
    }

    /// The store-level alert, cleared; the next queued one takes its place.
    pub fn take_alert(&mut self) -> Option<AlertItem> {
        self.alerts.pop_front().map(|(_, a)| a)
    }

    /// Every alert waiting, oldest first, with the document whose window
    /// shows it (`None`: the front window).
    pub fn alerts(&self) -> impl Iterator<Item = (Option<DocId>, &AlertItem)> {
        self.alerts.iter().map(|(d, a)| (*d, a))
    }

    /// The waiting alert `id`, removed (the app shows it now).
    pub fn take_alert_id(&mut self, id: u64) -> Option<AlertItem> {
        let i = self.alerts.iter().position(|(_, a)| a.id == id)?;
        self.alerts.remove(i).map(|(_, a)| a)
    }

    /// Puts back an alert the app took but could not show (its window
    /// closed first): it is the first to show again.
    pub fn return_alert(&mut self, doc: Option<DocId>, item: AlertItem) {
        let doc = doc.filter(|d| self.documents.contains_key(d));
        self.alerts.push_front((doc, item));
    }

    /// Moves the alert each document holds (one raised outside
    /// [`with_document`](Self::with_document), the mixer's) to the
    /// store, naming its document.
    pub fn collect_document_alerts(&mut self) {
        let mut ids: Vec<DocId> = self
            .documents
            .iter()
            .filter(|(_, d)| d.alert.is_some())
            .map(|(id, _)| *id)
            .collect();
        ids.sort();
        for id in ids {
            if let Some(a) = self.documents.get_mut(&id).and_then(|d| d.alert.take()) {
                self.alerts.push_back((Some(id), a));
            }
        }
    }
}

/// A path as the file system names it (symlinks, `..` and trailing
/// slashes resolved); a path that does not exist is compared as given.
fn canonical(path: &Path) -> PathBuf {
    std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}
