//! Opening files and making projects (Swift `DocumentStore` open parts,
//! `Recovery`, `ControlOpen`, and `InterchangeCommands`' made projects):
//! open routing by extension, the recovery questions, the DAWproject
//! return prompt, the slow file reads, and the projects an import makes.
//!
//! Nothing here shows a prompt or runs a slow read on the calling thread:
//! questions wait in the store for the app (or the control channel) to
//! answer, and reads are [`SlowRead`]s the app runs where it likes and
//! hands back with [`DocumentStore::land`].

use std::collections::HashMap;
use std::path::{Component, Path, PathBuf};

use core_control::error::{ControlError, ErrorCode};
use core_control::request::{CrashAnswer, RecoveryAnswers};
use vizdaw_ffi::snapshot::PluginRefRow;
use vizdaw_ffi::{
    AafImportRequestRow, AafInfoRow, DawprojectInfoRow, MidiFileInfoRow, MidiImportRequestRow,
    OpenOptions, ProjectFormatRow, RecoveryInfo, VizError,
};

use super::{DocId, DocumentStore};
use crate::alerts::{AlertItem, error_message};
use crate::interchange_commands::{
    self as ic, DawprojectRead, IMPORT_AAF, IMPORT_DAWPROJECT, IMPORT_FAILED, IMPORT_MIDI,
    MIDI_PROJECT_RATE, MidiIntoNewProject,
};
use crate::jobs::FileJob;
use crate::mixer::commands::thrown;

/// The title of a refused open.
pub const OPEN_FAILED: &str = "Open Failed";
/// The title of a merge the core refused to start.
pub const MERGE_FAILED: &str = "DAWproject Merge Failed";

/// What an open did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Opened {
    /// The document: made now, or already open (its window comes forward).
    Doc(DocId),
    /// The file is being read first (a DAWproject or AAF file); its
    /// project, if one is made, arrives in [`DocumentStore::take_pending`]
    /// (or a return prompt asks first).
    Reading,
    /// A recovery question waits ([`DocumentStore::open_question`]).
    Asking,
}

// ---------------------------------------------------------------------
// Paths

fn extension(path: &Path) -> String {
    path.extension()
        .map(|e| e.to_string_lossy().to_lowercase())
        .unwrap_or_default()
}

/// A MIDI file Open and drops send to a new project's import (any case).
pub fn is_midi(path: &Path) -> bool {
    matches!(extension(path).as_str(), "mid" | "midi" | "smf" | "midi2")
}

/// A DAWproject file (any case of the extension).
pub fn is_dawproject(path: &Path) -> bool {
    extension(path) == "dawproject"
}

/// An AAF file (any case of the extension).
pub fn is_aaf(path: &Path) -> bool {
    extension(path) == "aaf"
}

/// The project `path` names: a `project.json` inside a `.vizdaw` folder
/// is that folder (owner ruling 2026-11-23: the Linux chooser picks
/// files); anything else is as given.
pub fn project_path(path: &Path) -> PathBuf {
    if path.file_name().is_some_and(|n| n == "project.json")
        && let Some(parent) = path.parent()
        && parent.extension().is_some_and(|e| e == "vizdaw")
    {
        return parent.to_path_buf();
    }
    path.to_path_buf()
}

/// Whether `path` is a saved Co-Producer copy,
/// `<package>/Co-Producer/Copies/<name>.vizdaw`, as given or as its real
/// place (names compared without case, as the core does).
pub fn is_copy(path: &Path) -> bool {
    fn shaped(p: &Path) -> bool {
        let parts: Vec<String> = p
            .components()
            .filter_map(|c| match c {
                Component::Normal(n) => Some(n.to_string_lossy().into_owned()),
                _ => None,
            })
            .collect();
        let n = parts.len();
        if n < 4 {
            return false;
        }
        let last = Path::new(&parts[n - 1]);
        extension(last) == "vizdaw"
            && last.file_stem().is_some_and(|s| !s.is_empty())
            && parts[n - 2].to_lowercase() == "copies"
            && parts[n - 3].to_lowercase() == "co-producer"
    }
    shaped(path) || std::fs::canonicalize(path).is_ok_and(|p| shaped(&p))
}

fn file_name(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// A project's name as the questions show it (the folder without
/// `.vizdaw`).
fn project_name(path: &Path) -> String {
    path.file_stem()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}

// ---------------------------------------------------------------------
// The recovery questions (spec §10)

/// A question the store asks before a project opens.
#[derive(Debug, Clone, PartialEq)]
pub enum OpenQuestion {
    /// "Open with Plugins Off?": VizDAW quit unexpectedly while `project`
    /// was open (`crashed`), or Shift was held while opening it.
    AfterCrash {
        project: String,
        loading: Vec<PluginRefRow>,
        crashed: bool,
    },
    /// "Restore Autosave": a newer autosave of `project` exists.
    RestoreAutosave { project: String },
}

/// The crash question's buttons.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CrashChoice {
    PluginsOff,
    Normally,
    Cancel,
    /// Disable the plugins that were loading, then open normally.
    Disable,
}

impl OpenQuestion {
    /// The prompt's message.
    pub fn message(&self) -> String {
        match self {
            OpenQuestion::AfterCrash {
                project,
                loading,
                crashed,
            } => {
                if !crashed {
                    format!("Open \u{201c}{project}\u{201d} with plugins off?")
                } else if loading.is_empty() {
                    "VizDAW quit unexpectedly while this project was open.".into()
                } else {
                    format!(
                        "VizDAW quit unexpectedly while this project was open, while loading {}.",
                        names(loading)
                    )
                }
            }
            OpenQuestion::RestoreAutosave { project } => {
                format!("\u{201c}{project}\u{201d} has an autosave newer than its last save.")
            }
        }
    }

    /// The prompt's informative text.
    pub fn informative(&self) -> &'static str {
        match self {
            OpenQuestion::AfterCrash { .. } => {
                "With plugins off, every plugin is a placeholder you can load one by one."
            }
            OpenQuestion::RestoreAutosave { .. } => {
                "Restoring opens the autosaved version as an unsaved change."
            }
        }
    }

    /// The buttons, in order: the crash question's answer
    /// [`CrashChoice::PluginsOff`], `Normally`, `Cancel`, then `Disable`
    /// (only after a crash with plugins loading); the autosave question's
    /// restore, then open last saved.
    pub fn buttons(&self) -> Vec<String> {
        match self {
            OpenQuestion::AfterCrash {
                loading, crashed, ..
            } => {
                let mut b: Vec<String> = ["Open with Plugins Off", "Open Normally", "Cancel"]
                    .map(String::from)
                    .to_vec();
                if *crashed && !loading.is_empty() {
                    b.push(format!("Disable {}", names(loading)));
                }
                b
            }
            OpenQuestion::RestoreAutosave { .. } => {
                vec!["Restore Autosave".into(), "Open Last Saved".into()]
            }
        }
    }

    /// What the control channel notes instead of asking (Swift
    /// `RecoveryAlerts` under the channel): the crash question is
    /// answered Cancel, the autosave question "Open Last Saved".
    pub fn channel_note(&self) -> AlertItem {
        match self {
            OpenQuestion::AfterCrash { project, .. } => AlertItem::new(
                "Open with Plugins Off?",
                format!("{project} was not opened; open it with app.open and `plugins`."),
            ),
            OpenQuestion::RestoreAutosave { project } => AlertItem::new(
                "Restore Autosave",
                format!(
                    "{project} was opened from its last save; close it and app.open with \
                     `restore_autosave: true` to restore the autosave."
                ),
            ),
        }
    }
}

fn names(loading: &[PluginRefRow]) -> String {
    loading
        .iter()
        .map(|p| p.name.as_str())
        .collect::<Vec<_>>()
        .join(", ")
}

/// A project open waiting for its questions' answers.
pub(super) struct PendingOpen {
    path: PathBuf,
    info: RecoveryInfo,
    options: OpenOptions,
    question: OpenQuestion,
}

// ---------------------------------------------------------------------
// The DAWproject return prompt (plan 6e Rulings 13, 14)

/// A DAWproject file that came from VizDAW, waiting for the return
/// prompt's answer.
#[derive(Debug, Clone, PartialEq)]
pub struct ReturnPrompt {
    /// The file.
    pub path: String,
    /// Its header, as the core read it.
    pub info: DawprojectInfoRow,
    /// The open project it came from (its project ID, else a snapshot of
    /// its export), the most recently active when several match.
    pub matched: Option<DocId>,
    /// That project's title, which the prompt shows.
    pub match_name: Option<String>,
}

/// The return prompt's answers this slice offers (Merge With Existing
/// Project… is slice 7c).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReturnChoice {
    NewProject,
    Cancel,
}

impl ReturnPrompt {
    pub const MESSAGE: &'static str = "This DAWproject came from a VizDAW project.";
    pub const NEW_PROJECT: &'static str = "New Project";
    pub const MERGE: &'static str = "Merge With Existing Project\u{2026}";
    pub const CANCEL: &'static str = "Cancel";

    /// The prompt's informative text.
    pub fn informative(&self) -> String {
        match &self.match_name {
            Some(n) => format!("It matches \u{201c}{n}\u{201d}, which is open."),
            None => "Merge it into the project it came from, or open it as a new project.".into(),
        }
    }

    /// What the control channel notes when it answers Cancel.
    pub fn channel_note() -> AlertItem {
        AlertItem::new(
            "DAWproject Not Merged",
            "A DAWproject from VizDAW was opened; it was not merged.",
        )
    }
}

// ---------------------------------------------------------------------
// Slow reads

/// A file read the store needs (it parses the file: never on the UI
/// thread). The app runs it ([`SlowRead::run`], on any thread) and hands
/// the answer back with [`DocumentStore::land`].
pub struct SlowRead {
    ticket: u64,
    work: Box<dyn FnOnce() -> ReadAnswer + Send>,
}

impl SlowRead {
    /// Runs the read (slow): what [`DocumentStore::land`] takes.
    pub fn run(self) -> Landed {
        Landed {
            ticket: self.ticket,
            answer: (self.work)(),
        }
    }
}

/// A read's answer, for [`DocumentStore::land`].
pub struct Landed {
    ticket: u64,
    answer: ReadAnswer,
}

enum ReadAnswer {
    Dawproject(Result<DawprojectInfoRow, VizError>),
    Aaf(Result<AafInfoRow, VizError>),
    Midi(Result<MidiFileInfoRow, VizError>),
}

/// What a read was for.
pub(super) enum ReadFor {
    OpenDawproject(String),
    OpenAaf(String),
    /// The import sheet's read of document `doc`, the `generation`th.
    MidiImport {
        doc: DocId,
        generation: u64,
        path: String,
    },
}

/// The MIDI file the import sheet shows (slice 7c), as read.
#[derive(Debug, Clone, PartialEq)]
pub struct MidiRead {
    pub path: String,
    pub info: MidiFileInfoRow,
}

/// Disables plugins in the Plugin Manager (false: it could not).
pub type DisablePlugins = Box<dyn Fn(&[PluginRefRow]) -> bool>;

/// Runs a read where the app wants it (GPUI: its background executor).
pub type Spawner = Box<dyn FnMut(SlowRead)>;

/// The reads in flight and how they run.
#[derive(Default)]
pub(super) struct Reads {
    pub(super) next: u64,
    pub(super) waiting: HashMap<u64, ReadFor>,
    /// Reads made while no spawner is set, oldest first.
    pub(super) queued: Vec<SlowRead>,
    pub(super) spawner: Option<Spawner>,
    /// The newest import-sheet read per document.
    pub(super) midi_generation: HashMap<DocId, u64>,
}

impl crate::DocumentModel {
    /// The MIDI file the import sheet reads, once read (slice 7c).
    pub fn midi_read(&self) -> Option<&MidiRead> {
        self.midi_read.as_ref()
    }
}

/// A store alert as a control failure ("<title>: <message>").
fn alert_failure(a: &AlertItem) -> ControlError {
    ControlError::new(ErrorCode::Failed, format!("{}: {}", a.title, a.message))
}

impl DocumentStore {
    // -----------------------------------------------------------------
    // New

    /// The format new projects are made in (File › New's initial choice;
    /// the depth of the projects an import makes). The app sets it from
    /// its preferences.
    pub fn default_format(&self) -> ProjectFormatRow {
        self.default_format
    }

    pub fn set_default_format(&mut self, f: ProjectFormatRow) {
        self.default_format = f;
    }

    /// A new project in `format` (File › New's format choice, `file.new`).
    pub fn new_project(&mut self, format: ProjectFormatRow) -> Option<DocId> {
        self.add("New Document Failed", |s| s.core.new_project(format))
    }

    /// The demo with audio.
    pub fn new_demo_audio(&mut self) -> Option<DocId> {
        self.add("Demo Project Failed", |s| s.core.new_demo_audio())
    }

    /// A project an import makes: at `rate`, at the default depth.
    fn made_format(&self, rate: u32) -> ProjectFormatRow {
        ProjectFormatRow {
            sample_rate: rate,
            depth: self.default_format.depth,
        }
    }

    // -----------------------------------------------------------------
    // Open (the UI's: File › Open, Import Project, launch, Finder)

    /// Opens `path`: a MIDI file makes a new project and imports into it;
    /// a DAWproject or AAF file is read first ([`Opened::Reading`]); a
    /// `project.json` inside a `.vizdaw` folder is that project; a saved
    /// Co-Producer copy opens as a new untitled project; a `.vizdaw`
    /// package opens after the recovery questions, if any
    /// ([`Opened::Asking`]); one already open answers its document. A
    /// refusal or failure is also posted as an alert.
    pub fn open(&mut self, path: &Path) -> Result<Opened, String> {
        if is_midi(path) {
            return self.open_midi(path).map(Opened::Doc);
        }
        if is_dawproject(path) {
            self.open_dawproject(&path.to_string_lossy());
            return Ok(Opened::Reading);
        }
        if is_aaf(path) {
            self.open_aaf(&path.to_string_lossy());
            return Ok(Opened::Reading);
        }
        let path = project_path(path);
        // A saved Co-Producer copy opens as a new untitled project, never
        // in place (plan 8a Ruling 9).
        if is_copy(&path) {
            return self.open_copy(&path).map(Opened::Doc);
        }
        if path.extension().and_then(|e| e.to_str()) != Some("vizdaw") {
            let message = format!("{} is not a .vizdaw project.", file_name(&path));
            self.post_alert(AlertItem::new(OPEN_FAILED, message.clone()));
            return Err(message);
        }
        if let Some(id) = self.document_for_path(&path) {
            return Ok(Opened::Doc(id));
        }
        let info = self.core.recovery_check(&path.to_string_lossy()).ok();
        let Some(info) = info else {
            return self.open_now(&path, None).map(Opened::Doc);
        };
        let project = project_name(&path);
        let question = if info.crashed || (self.shift_held)() {
            OpenQuestion::AfterCrash {
                project,
                loading: info.loading.clone(),
                crashed: info.crashed,
            }
        } else if info.autosave.is_some() {
            OpenQuestion::RestoreAutosave { project }
        } else {
            return self.open_now(&path, None).map(Opened::Doc);
        };
        self.questions.push_back(PendingOpen {
            path,
            info,
            options: plain(),
            question,
        });
        Ok(Opened::Asking)
    }

    /// Opens the package at `path` with `options` (`None`: as saved).
    fn open_now(&mut self, path: &Path, options: Option<OpenOptions>) -> Result<DocId, String> {
        // Asked meanwhile and opened another way: that document.
        if let Some(id) = self.document_for_path(path) {
            return Ok(id);
        }
        let text = path.to_string_lossy().into_owned();
        let made = match options {
            Some(o) if o != plain() => self.add(OPEN_FAILED, |s| s.core.open_with(&text, o)),
            _ => self.add(OPEN_FAILED, |s| s.core.open(&text)),
        };
        made.ok_or_else(|| {
            self.last_failure
                .take()
                .unwrap_or_else(|| "open failed".into())
        })
    }

    /// File › Open Co-Producer Copy…: the copy at `path` as a new untitled
    /// project; anything else is refused with an alert.
    pub fn open_copy(&mut self, path: &Path) -> Result<DocId, String> {
        if !is_copy(path) {
            let message = "That is not a Co-Producer copy.".to_owned();
            self.post_alert(AlertItem::new(OPEN_FAILED, message.clone()));
            return Err(message);
        }
        let text = path.to_string_lossy().into_owned();
        self.add(OPEN_FAILED, |s| s.core.open_copy(&text))
            .ok_or_else(|| {
                self.last_failure
                    .take()
                    .unwrap_or_else(|| "open failed".into())
            })
    }

    /// The question the open waiting first asks.
    pub fn open_question(&self) -> Option<&OpenQuestion> {
        self.questions.front().map(|p| &p.question)
    }

    /// The crash question's answer: the project opens (its document), or
    /// the autosave question follows (`None`), or Cancel opens nothing.
    pub fn answer_crash(&mut self, choice: CrashChoice) -> Option<DocId> {
        if !matches!(self.open_question(), Some(OpenQuestion::AfterCrash { .. })) {
            return None;
        }
        let mut p = self.questions.pop_front()?;
        match choice {
            CrashChoice::Cancel => return None,
            CrashChoice::PluginsOff => p.options.plugins_off = true,
            CrashChoice::Normally => {}
            // Never open normally with the crashing plugin still enabled.
            CrashChoice::Disable => {
                if !(self.disable_plugins)(&p.info.loading) {
                    p.options.plugins_off = true;
                }
            }
        }
        if p.info.autosave.is_some() {
            p.question = OpenQuestion::RestoreAutosave {
                project: project_name(&p.path),
            };
            self.questions.push_front(p);
            return None;
        }
        self.open_now(&p.path, Some(p.options)).ok()
    }

    /// The autosave question's answer (`restore`: the autosave): the
    /// project opens.
    pub fn answer_autosave(&mut self, restore: bool) -> Option<DocId> {
        if !matches!(
            self.open_question(),
            Some(OpenQuestion::RestoreAutosave { .. })
        ) {
            return None;
        }
        let mut p = self.questions.pop_front()?;
        if restore {
            p.options.autosave = p.info.autosave.clone();
        }
        self.open_now(&p.path, Some(p.options)).ok()
    }

    /// Makes Shift-held read as the app sees it (Shift asks the crash
    /// question without a crash).
    pub fn set_shift_held(&mut self, held: Box<dyn Fn() -> bool>) {
        self.shift_held = held;
    }

    /// How the crash question's Disable disables plugins (false: it could
    /// not, and the project opens with plugins off).
    pub fn set_disable_plugins(&mut self, disable: DisablePlugins) {
        self.disable_plugins = disable;
    }

    /// Under the control channel nothing asks (Swift): every waiting
    /// return prompt is answered Cancel, a crash question Cancel and an
    /// autosave question Open Last Saved (that project opens, into
    /// [`DocumentStore::take_pending`]). Answers the notes the channel
    /// reports.
    pub fn answer_for_channel(&mut self) -> Vec<AlertItem> {
        let mut notes = vec![];
        while self.return_prompt.is_some() {
            notes.push(ReturnPrompt::channel_note());
            self.answer_return(ReturnChoice::Cancel);
        }
        while let Some(q) = self.open_question().cloned() {
            notes.push(q.channel_note());
            match q {
                OpenQuestion::AfterCrash { .. } => {
                    self.answer_crash(CrashChoice::Cancel);
                }
                OpenQuestion::RestoreAutosave { .. } => {
                    if let Some(id) = self.answer_autosave(false) {
                        self.pending.push_back(id);
                    }
                }
            }
        }
        notes
    }

    // -----------------------------------------------------------------
    // Open over the control channel (`app.open`)

    /// `app.open` (Swift `ControlOpen.open`): no question is asked; the
    /// call's `plugins` and `restore_autosave` answer them, and one
    /// asked without its answer refuses the call naming the dialog. A
    /// DAWproject or AAF file is refused (their commands answer the job);
    /// a project already open is Busy.
    pub fn open_answered(
        &mut self,
        path: &Path,
        answers: &RecoveryAnswers,
    ) -> Result<DocId, ControlError> {
        if is_dawproject(path) {
            return Err(ControlError::new(
                ErrorCode::InvalidParams,
                "Use file.import_dawproject to open a DAWproject file.",
            ));
        }
        if is_aaf(path) {
            return Err(ControlError::new(
                ErrorCode::InvalidParams,
                "Use file.import_aaf to open an AAF file.",
            ));
        }
        let project = project_path(path);
        // An open project is never opened twice, and no window is fronted.
        if self.document_for_path(&project).is_some() {
            return Err(ControlError::new(
                ErrorCode::Busy,
                format!("{} is already open in another window.", file_name(&project)),
            ));
        }
        let before = self.alert_ids();
        let outcome = if is_midi(path)
            || is_copy(&project)
            || project.extension().and_then(|e| e.to_str()) != Some("vizdaw")
        {
            self.open(path).map(|o| match o {
                Opened::Doc(id) => Some(id),
                // Neither reads nor asks for these paths.
                Opened::Reading | Opened::Asking => None,
            })
        } else {
            let options = self.answered_options(&project, answers)?;
            self.open_now(&project, Some(options)).map(Some)
        };
        match outcome {
            Ok(Some(id)) => Ok(id),
            _ => Err(match self.take_new_alert(&before) {
                Some(a) => alert_failure(&a),
                None => ControlError::new(
                    ErrorCode::Failed,
                    format!("Opening {} was cancelled.", path.display()),
                ),
            }),
        }
    }

    /// The options `answers` open `path` with, or the dialog an
    /// unanswered question needs.
    fn answered_options(
        &mut self,
        path: &Path,
        answers: &RecoveryAnswers,
    ) -> Result<OpenOptions, ControlError> {
        let mut options = plain();
        let Ok(info) = self.core.recovery_check(&path.to_string_lossy()) else {
            return Ok(options);
        };
        let asks_crash = info.crashed || (self.shift_held)();
        // The autosave question is settled before anything opens: opening
        // to find out would load plugins and consume the crash marker.
        if answers.restore_autosave.is_none()
            && info.autosave.is_some()
            && !(asks_crash && answers.plugins.is_none())
        {
            return Err(ControlError::dialog(
                "Restore Autosave",
                &["restore_autosave"],
            ));
        }
        if asks_crash {
            match answers.plugins {
                None => return Err(ControlError::dialog("Open with Plugins Off?", &["plugins"])),
                Some(CrashAnswer::Off) => options.plugins_off = true,
                Some(CrashAnswer::Normal) => {}
                Some(CrashAnswer::Disable) => {
                    if !(self.disable_plugins)(&info.loading) {
                        options.plugins_off = true;
                    }
                }
            }
        }
        if info.autosave.is_some() && answers.restore_autosave == Some(true) {
            options.autosave = info.autosave.clone();
        }
        Ok(options)
    }

    fn alert_ids(&self) -> Vec<u64> {
        self.alert
            .iter()
            .chain(self.queued_alerts.iter())
            .map(|a| a.id)
            .collect()
    }

    /// The newest alert posted since `before` was taken, removed.
    fn take_new_alert(&mut self, before: &[u64]) -> Option<AlertItem> {
        if let Some(a) = &self.alert
            && !before.contains(&a.id)
        {
            return self.take_alert();
        }
        let i = self
            .queued_alerts
            .iter()
            .rposition(|a| !before.contains(&a.id))?;
        self.queued_alerts.remove(i)
    }

    // -----------------------------------------------------------------
    // Projects made for a file the UI opened

    /// A new [`MIDI_PROJECT_RATE`] project importing the MIDI file at
    /// `path` with the defaults; a refusal leaves nothing behind and
    /// alerts "Import Failed".
    fn open_midi(&mut self, path: &Path) -> Result<DocId, String> {
        let path = path.to_string_lossy().into_owned();
        self.import_for_open(MIDI_PROJECT_RATE, |d| {
            FileJob::ImportMidi(MidiImportRequestRow {
                path,
                new_project: true,
                seq: d.selected_seq().unwrap_or(0),
                lane: None,
                tick: 0,
                channel_map: vec![],
                split_channels: None,
                mpe: false,
                use_tempo: true,
            })
        })
    }

    /// Makes a project at `rate` for a file the UI opened and starts
    /// `job` there, listed on the document. A refusal to start closes it
    /// and alerts "Import Failed"; a job that then fails or is cancelled
    /// while the project is untouched closes it too
    /// ([`DocumentStore::with_document`]).
    fn import_for_open(
        &mut self,
        rate: u32,
        job: impl FnOnce(&crate::DocumentModel) -> FileJob,
    ) -> Result<DocId, String> {
        let format = self.made_format(rate);
        let Some(id) = self.new_project(format) else {
            return Err(self
                .last_failure
                .take()
                .unwrap_or_else(|| "no project was made".into()));
        };
        let doc = self.documents.get_mut(&id).expect("just made");
        doc.opened_file = true;
        let job = job(doc);
        if let Err(e) = doc.try_start_job(job.kind(), |s| job.start(s)) {
            self.close(id);
            let message = error_message(&e);
            self.post_alert(AlertItem::new(IMPORT_FAILED, message.clone()));
            return Err(message);
        }
        // A job that ended at once has already taken the project.
        if self.settle_opened_file(id) {
            return Err(self
                .alert
                .as_ref()
                .map(|a| a.message.clone())
                .unwrap_or_else(|| "the import was cancelled".into()));
        }
        Ok(id)
    }

    /// Document `id`'s import for the file it was made for ended badly:
    /// untouched, it closes (its windows close: [`DocumentStore::take_closed`])
    /// and the alert is the store's; edited, it stays with the alert.
    /// Answers whether it closed.
    pub(super) fn settle_opened_file(&mut self, id: DocId) -> bool {
        let Some(doc) = self.documents.get_mut(&id) else {
            return false;
        };
        let Some(end) = doc.opened_file_end.take() else {
            return false;
        };
        let alert = end.map(|m| AlertItem::new(IMPORT_FAILED, m));
        if doc.can_undo() {
            if let Some(a) = alert {
                self.post_alert(a);
            }
            return false;
        }
        self.close(id);
        self.closed.push(id);
        if let Some(a) = alert {
            self.post_alert(a);
        }
        true
    }

    /// Projects made for a file whose window should open (a read landed),
    /// oldest first, removed.
    pub fn take_pending(&mut self) -> Option<DocId> {
        self.pending.pop_front()
    }

    /// Documents the store closed itself (an import into a project made
    /// for its file failed), whose windows should close; removed.
    pub fn take_closed(&mut self) -> Vec<DocId> {
        std::mem::take(&mut self.closed)
    }

    // -----------------------------------------------------------------
    // DAWproject and AAF opens (read first)

    /// Starts opening a DAWproject file: it is read (a [`SlowRead`]),
    /// then a file from VizDAW raises the return prompt and any other
    /// makes a project at its rate importing it.
    pub fn open_dawproject(&mut self, path: &str) {
        let core = self.core.clone();
        let p = path.to_owned();
        self.read(ReadFor::OpenDawproject(path.to_owned()), move || {
            ReadAnswer::Dawproject(core.dawproject_info(&p))
        });
    }

    /// Starts opening an AAF file: it is read, then a project at its rate
    /// imports it with the defaults (a new project, the MIDI file beside
    /// it, no media folder, pairs joined).
    pub fn open_aaf(&mut self, path: &str) {
        let core = self.core.clone();
        let p = path.to_owned();
        self.read(ReadFor::OpenAaf(path.to_owned()), move || {
            ReadAnswer::Aaf(core.aaf_info(&p))
        });
    }

    /// The import sheet's read of the MIDI file at `path` for document
    /// `doc` (slice 7c): a newer read replaces it; a refusal is the alert
    /// "Import Failed".
    pub fn begin_midi_import(&mut self, doc: DocId, path: &str) {
        let Some(d) = self.documents.get(&doc) else {
            return;
        };
        let session = d.session.clone();
        let generation = {
            let g = self.reads.midi_generation.entry(doc).or_default();
            *g += 1;
            *g
        };
        let p = path.to_owned();
        self.read(
            ReadFor::MidiImport {
                doc,
                generation,
                path: path.to_owned(),
            },
            move || ReadAnswer::Midi(session.midi_file_info(p)),
        );
    }

    fn read(&mut self, purpose: ReadFor, work: impl FnOnce() -> ReadAnswer + Send + 'static) {
        let ticket = self.reads.next;
        self.reads.next += 1;
        self.reads.waiting.insert(ticket, purpose);
        let read = SlowRead {
            ticket,
            work: Box::new(work),
        };
        match self.reads.spawner.as_mut() {
            Some(spawn) => spawn(read),
            None => self.reads.queued.push(read),
        }
    }

    /// Where reads run from now on (GPUI: its background executor, landing
    /// on the main thread); without one they wait in
    /// [`DocumentStore::take_reads`].
    pub fn set_spawner(&mut self, spawner: Spawner) {
        self.reads.spawner = Some(spawner);
        for r in std::mem::take(&mut self.reads.queued) {
            if let Some(spawn) = self.reads.spawner.as_mut() {
                spawn(r);
            }
        }
    }

    /// The reads waiting for a spawner, oldest first, removed.
    pub fn take_reads(&mut self) -> Vec<SlowRead> {
        std::mem::take(&mut self.reads.queued)
    }

    /// Runs every waiting read here and lands it (tests; a read is slow).
    pub fn run_reads(&mut self) {
        while !self.reads.queued.is_empty() {
            for r in self.take_reads() {
                let landed = r.run();
                self.land(landed);
            }
        }
    }

    /// Whether a read has not landed yet.
    pub fn reading(&self) -> bool {
        !self.reads.waiting.is_empty()
    }

    /// A read's answer: applied when the store still wants it; an
    /// answer for a document that closed, or older than a newer read of
    /// the same sheet, is dropped.
    pub fn land(&mut self, landed: Landed) {
        let Some(purpose) = self.reads.waiting.remove(&landed.ticket) else {
            return;
        };
        match (purpose, landed.answer) {
            (ReadFor::OpenDawproject(path), ReadAnswer::Dawproject(r)) => {
                self.dawproject_read(path, r)
            }
            (ReadFor::OpenAaf(path), ReadAnswer::Aaf(r)) => match r {
                Err(e) => self.post_alert(AlertItem::from_error(&e, IMPORT_FAILED)),
                Ok(row) => {
                    let req = AafImportRequestRow {
                        path,
                        new_project: true,
                        midi_path: None,
                        media_folder: None,
                        join_pairs: true,
                    };
                    self.import_landed(row.sample_rate, FileJob::ImportAaf(req));
                }
            },
            (
                ReadFor::MidiImport {
                    doc,
                    generation,
                    path,
                },
                ReadAnswer::Midi(r),
            ) => {
                if self.reads.midi_generation.get(&doc) != Some(&generation) {
                    return;
                }
                let Some(d) = self.documents.get_mut(&doc) else {
                    return;
                };
                match r {
                    Err(e) => self.post_alert(AlertItem::from_error(&e, IMPORT_FAILED)),
                    Ok(info) => d.midi_read = Some(MidiRead { path, info }),
                }
            }
            _ => {}
        }
    }

    /// A project made for a file once read; its window opens
    /// ([`DocumentStore::take_pending`]).
    fn import_landed(&mut self, rate: u32, job: FileJob) {
        if let Ok(id) = self.import_for_open(rate, |_| job) {
            self.pending.push_back(id);
        }
    }

    /// The DAWproject file was read: one from VizDAW asks first (the
    /// return prompt); any other makes its project at once.
    fn dawproject_read(&mut self, path: String, r: Result<DawprojectInfoRow, VizError>) {
        let row = match r {
            Err(e) => return self.post_alert(AlertItem::from_error(&e, IMPORT_FAILED)),
            Ok(row) => row,
        };
        if !row.from_vizdaw {
            return self.import_landed(row.sample_rate, ic::dawproject_import_job(&path));
        }
        let matched = self
            .document_for_project_id(row.project_id.as_deref())
            .or_else(|| self.document_holding(row.export_id.as_deref()));
        let prompt = ReturnPrompt {
            path,
            match_name: matched
                .and_then(|d| self.documents.get(&d).map(|m| m.snapshot().title.clone())),
            matched,
            info: row,
        };
        if self.return_prompt.is_none() {
            self.return_prompt = Some(prompt);
        } else {
            self.queued_prompts.push_back(prompt);
        }
    }

    /// The return prompt showing, if any.
    pub fn return_prompt(&self) -> Option<&ReturnPrompt> {
        self.return_prompt.as_ref()
    }

    /// The return prompt's answer. New Project: a project at the file's
    /// rate imports it. Cancel: nothing. The next waiting prompt follows.
    pub fn answer_return(&mut self, choice: ReturnChoice) {
        let Some(p) = self.return_prompt.take() else {
            return;
        };
        self.return_prompt = self.queued_prompts.pop_front();
        match choice {
            ReturnChoice::Cancel => {}
            ReturnChoice::NewProject => {
                self.import_landed(p.info.sample_rate, ic::dawproject_import_job(&p.path))
            }
        }
    }

    // -----------------------------------------------------------------
    // Finding projects

    /// Open documents, the most recently active first: those whose
    /// window was key, most recent first, then the rest, newest first.
    fn by_activity(&self) -> Vec<DocId> {
        let mut out: Vec<DocId> = self
            .recent_key
            .iter()
            .copied()
            .filter(|d| self.documents.contains_key(d))
            .collect();
        let mut rest: Vec<DocId> = self
            .documents
            .keys()
            .copied()
            .filter(|d| !out.contains(d))
            .collect();
        rest.sort_by(|a, b| b.cmp(a));
        out.extend(rest);
        out
    }

    /// The most recently active open document whose project ID is `id`.
    pub fn document_for_project_id(&self, id: Option<&str>) -> Option<DocId> {
        let id = id?;
        self.by_activity().into_iter().find(|d| {
            self.documents[d]
                .session()
                .project_id()
                .ok()
                .flatten()
                .as_deref()
                == Some(id)
        })
    }

    /// The most recently active open document holding the snapshot of
    /// export `export_id` (one lstat per document asked).
    pub fn document_holding(&self, export_id: Option<&str>) -> Option<DocId> {
        let export_id = export_id?;
        self.by_activity().into_iter().find(|d| {
            self.documents[d]
                .session()
                .has_snapshot(export_id.to_owned())
                .unwrap_or(false)
        })
    }

    // -----------------------------------------------------------------
    // Projects the control channel's imports make

    /// Whether a channel import made project `doc` for a job not yet
    /// ended (the channel shows its window, as `file.new` does).
    pub fn made_project(&self, doc: DocId) -> bool {
        self.made.contains(&doc)
    }

    /// The channel's job on `doc` ended. A project a command made, whose
    /// job failed or was cancelled, goes by `close` while nothing was
    /// done in it.
    pub fn job_ended(&mut self, doc: DocId, failed: bool, close: impl FnOnce(&mut Self, DocId)) {
        if !self.made.remove(&doc) || !failed {
            return;
        }
        if self.documents.get(&doc).is_some_and(|m| !m.can_undo()) {
            close(self, doc);
        }
    }

    /// Makes the project a channel import imports into (at `rate`, the
    /// default depth) and starts `job` there: the project and the job's
    /// ID. A refusal to start closes the project again.
    fn import_into_made(
        &mut self,
        command: &str,
        rate: u32,
        job: impl FnOnce(&crate::DocumentModel) -> FileJob,
    ) -> Result<(DocId, u64), ControlError> {
        let before = self.alert_ids();
        let Some(id) = self.new_project(self.made_format(rate)) else {
            return Err(match self.take_new_alert(&before) {
                Some(a) => alert_failure(&a),
                None => {
                    ControlError::new(ErrorCode::Failed, format!("{command} made no document."))
                }
            });
        };
        let doc = &self.documents[&id];
        let job = job(doc);
        match job.start(doc.session().as_ref()) {
            Ok(j) => {
                self.made.insert(id);
                Ok((id, j))
            }
            Err(e) => {
                // Refused before a job started (e.g. Busy): nothing was
                // done in it.
                self.close(id);
                Err(thrown(&e, IMPORT_FAILED))
            }
        }
    }

    /// `file.import_midi` with `new_project`, once the file was read: a
    /// [`MIDI_PROJECT_RATE`] project importing it (into its shown
    /// sequence unless `seq` was given).
    pub fn import_midi_into_new_project(
        &mut self,
        m: &MidiIntoNewProject,
    ) -> Result<(DocId, u64), ControlError> {
        self.import_into_made(IMPORT_MIDI, MIDI_PROJECT_RATE, |d| {
            FileJob::ImportMidi(m.request_for(d.selected_seq()))
        })
    }

    /// `file.import_dawproject` into a new project at the file's `rate`.
    pub fn import_dawproject_into_new_project(
        &mut self,
        path: &str,
        rate: u32,
    ) -> Result<(DocId, u64), ControlError> {
        self.import_into_made(IMPORT_DAWPROJECT, rate, |_| ic::dawproject_import_job(path))
    }

    /// `file.import_aaf` into a new project at the file's `rate`.
    pub fn import_aaf_into_new_project(
        &mut self,
        req: AafImportRequestRow,
        rate: u32,
    ) -> Result<(DocId, u64), ControlError> {
        self.import_into_made(IMPORT_AAF, rate, |_| FileJob::ImportAaf(req))
    }

    /// `file.import_dawproject` with `mode` merge, once the file is read
    /// as `info`: into `project` (opened as `app.open` opens it), else the
    /// open project the file came from (by project ID, then by snapshot),
    /// else the "Open" dialog. A project this opened goes again if the
    /// merge is refused before its job, or (by
    /// [`DocumentStore::job_ended`]) by it, while untouched.
    pub fn merge_dawproject(
        &mut self,
        read: &DawprojectRead,
        info: &DawprojectInfoRow,
        date: String,
    ) -> Result<(DocId, u64), ControlError> {
        let (target, opened) = match &read.project {
            Some(project) => {
                let p = Path::new(project);
                ic::check_merge_project(project, self.document_for_path(p).is_some())?;
                let id = self.open_answered(p, &RecoveryAnswers::default())?;
                (id, true)
            }
            None => {
                let found = self
                    .document_for_project_id(info.project_id.as_deref())
                    .or_else(|| self.document_holding(info.export_id.as_deref()))
                    .ok_or_else(ic::merge_target_dialog)?;
                (found, false)
            }
        };
        let Some(m) = self.documents.get(&target) else {
            return Err(ControlError::new(
                ErrorCode::NoDocument,
                format!("{IMPORT_DAWPROJECT}: the project closed."),
            ));
        };
        let job = read.merge_job(date);
        match job.start(m.session().as_ref()) {
            Ok(j) => {
                if opened {
                    self.made.insert(target);
                }
                Ok((target, j))
            }
            Err(e) => {
                // Refused before a job started (e.g. Busy): nothing was
                // done in a project this opened.
                if opened && !m.can_undo() {
                    self.close(target);
                }
                Err(thrown(&e, MERGE_FAILED))
            }
        }
    }

    /// The open document saved at `path` (same file, symbolic links and
    /// `..` resolved), if any.
    pub fn document_for_path(&self, path: &Path) -> Option<DocId> {
        let want = super::canonical(path);
        self.documents
            .iter()
            .find(|(_, d)| d.path().map(|p| super::canonical(&p)) == Some(want.clone()))
            .map(|(id, _)| *id)
    }
}

fn plain() -> OpenOptions {
    OpenOptions {
        plugins_off: false,
        autosave: None,
    }
}
