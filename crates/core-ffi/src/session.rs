//! `Session`: one open document (one window).

use crate::error::VizError;
use crate::guard::{ffi_guard, lock};
use crate::host;
use crate::mixer::{self, DestRow, MixerLevels, MixerSnapshot};
use crate::plugins;
use crate::snapshot::PluginRefRow;
use crate::snapshot::{
    DocSnapshot, IdleUpdate, LaneRow, SlotRow, TransportState, lane_rows, position_bbu, seconds,
    seq_id, seq_rows, warnings,
};
use core_host::PluginHost;
use core_model::command::Command;
use core_model::command::MixerCmd;
use core_model::command::RoutingCmd;
use core_model::command::lane::LaneCmd;
use core_model::command::sequence::SequenceCmd;
use core_model::document::DestKind;
use core_model::document::Document;
use core_model::file;
use core_model::history::History;
use core_model::ids::{DestId, LaneId, PluginInstanceId, SeqId, StripId};
use core_model::mixer::{AuxSend, Output, PluginSlot};
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
    /// A reload could not run (the plugin registry was busy, or Play's
    /// load failed): `idle` runs it once it can.
    reload_owed: bool,
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
            reload_owed: false,
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

    /// After an edit: the plugin host follows the document and the engine
    /// recompiles. A failure (or panic, which also resets the host) never
    /// fails the edit: it becomes a sticky fault.
    fn reload_host(&mut self, id: u64) {
        let (doc, looped) = (self.history.doc(), self.looped);
        let r =
            ffi_guard(|| plugins::with(id, |p| host::reload(id, doc, looped, p)).and_then(|r| r));
        self.reload_owed = matches!(r, Err(VizError::Busy { .. }));
        self.reload_fault = match r {
            Ok(()) => None,
            // Not a fault: `idle` retries.
            Err(VizError::Busy { .. }) => None,
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
    /// A session for `doc`; its plugins start loading on this thread.
    fn with(doc: Document, path: Option<PathBuf>) -> Result<Arc<Session>, VizError> {
        let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
        plugins::create(id)?;
        let _ = plugins::with(id, |p| p.map(|p| p.sync(&doc)));
        Ok(Arc::new(Session {
            id,
            inner: Mutex::new(Inner::new(doc, path)),
        }))
    }

    /// Commits plugin-UI gestures as undo steps (spec §4), then lets the
    /// plugin host and the engine follow (the host must see the committed
    /// document before any undo, 3b `PluginHost::commit_gestures`).
    fn commit_gestures(&self, inner: &mut Inner, gestures: &[core_host::Gesture]) -> bool {
        if gestures.is_empty() {
            return false;
        }
        let r = PluginHost::commit_gestures(&mut inner.history, gestures);
        inner.version += 1;
        inner.reload_host(self.id);
        if let Err(e) = r {
            inner.reload_fault = Some(format!("plugin gesture refused: {e}"));
        }
        true
    }

    /// Applies one command as one undo step and recompiles if playing.
    /// Running plugins it removes are captured first (spec §4), so an undo
    /// brings them back with their state.
    fn execute(&self, cmd: Command) -> Result<DocSnapshot, VizError> {
        let next = core_model::command::apply(lock(&self.inner).doc(), &cmd)
            .ok()
            .map(|n| n.doc);
        if let Some(next) = next {
            self.capture_removed(&next)?;
        }
        let mut inner = lock(&self.inner);
        inner.history.execute(&cmd, &ViewFilter::default())?;
        inner.version += 1;
        inner.reload_host(self.id);
        inner.snapshot()
    }

    /// Captures the state of every running plugin that `next` (the
    /// document an edit, undo or redo is about to make) no longer has
    /// (Plan Ruling 8; the model's `Report.removed` does not list them
    /// all). Plugin code (`save_state` may spin a run loop) runs without
    /// the session lock, against a copy of the history; blobs are
    /// content-addressed, so storing the same bytes in the real history
    /// afterwards gives the ids the host recorded. Nothing is captured
    /// inside an open edit (a drag removes no plugin). `Busy` when a
    /// plugin call is on the stack: the removal is refused rather than
    /// losing the state.
    fn capture_removed(&self, next: &Document) -> Result<(), VizError> {
        let (mut scratch, gone) = {
            let inner = lock(&self.inner);
            let before = mixer::slot_ids(inner.doc());
            let after = mixer::slot_ids(next);
            let gone: Vec<_> = before.difference(&after).copied().collect();
            if gone.is_empty() {
                return Ok(());
            }
            (inner.history.clone(), gone)
        };
        let captured = plugins::with(self.id, |p| {
            let Some(p) = p else { return Vec::new() };
            gone.into_iter()
                // A placeholder has nothing to capture; its saved state
                // stays in the document.
                .filter_map(|id| p.capture(&mut scratch, id).ok().map(|blob| (id, blob)))
                .collect::<Vec<_>>()
        })?;
        let mut inner = lock(&self.inner);
        for (id, blob) in captured {
            if let Some(bytes) = scratch.doc().state_blob(blob) {
                inner.history.capture_state(id, bytes)?;
            }
        }
        Ok(())
    }

    /// Undo or redo, then recompile if playing.
    fn step(&self, undo: bool) -> Result<DocSnapshot, VizError> {
        let next = {
            let inner = lock(&self.inner);
            if undo {
                inner.history.peek_undo().cloned()
            } else {
                inner.history.peek_redo().cloned()
            }
        };
        if let Some(next) = next {
            self.capture_removed(&next)?;
        }
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
        ffi_guard(|| Session::with(Document::new(), None))
    }

    /// An untitled copy of the demo project.
    #[uniffi::constructor]
    pub fn new_demo() -> Result<Arc<Session>, VizError> {
        ffi_guard(|| {
            let doc = core_engine::fixtures::demo_document()?;
            Session::with(doc, None)
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
            Session::with(doc, Some(p))
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
            let looped = inner.looped;
            let played = plugins::with(self.id, |p| {
                host::play(self.id, inner.doc(), id, start, looped, p)
            })
            .and_then(|r| r);
            if let Err(e) = played {
                // Retried by `idle` (the engine may hold stale plugins).
                inner.reload_owed = true;
                return Err(e);
            }
            inner.reload_owed = false;
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
            let state = plugins::with(self.id, |p| host::state(self.id, doc, p)).and_then(|r| r)?;
            Ok(match state {
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

#[uniffi::export]
impl Session {
    /// The plugin timer's step (spec §5 `host_idle`; the app calls it about
    /// 60 times a second on the main thread): finishes plugin loads, runs
    /// plugin callbacks, lets the engine follow, and turns plugin-UI
    /// gestures into undo steps.
    pub fn idle(&self) -> Result<IdleUpdate, VizError> {
        ffi_guard(|| {
            let nothing = || IdleUpdate {
                snapshot: None,
                slots_changed: false,
                params_changed: Vec::new(),
                problems: Vec::new(),
            };
            // A nested run loop inside a plugin call: skip this tick (before
            // taking the session lock, which that call may hold).
            if plugins::is_busy() {
                return Ok(nothing());
            }
            plugins::tick_closing();
            let mut inner = lock(&self.inner);
            if inner.reload_owed {
                inner.reload_host(self.id);
            }
            let report = {
                let doc = inner.doc();
                plugins::with(self.id, |p| p.map(|p| host::service(self.id, doc, p)))
            };
            let Ok(Some(r)) = report else {
                return Ok(nothing());
            };
            let mut problems: Vec<String> = Vec::new();
            for (id, why) in &r.failed {
                problems.push(format!("{}: {why}", slot_name(inner.doc(), *id)));
            }
            for (id, why) in &r.rejected_state {
                problems.push(format!(
                    "{}: saved state refused ({why})",
                    slot_name(inner.doc(), *id)
                ));
            }
            let changed = self.commit_gestures(&mut inner, &r.gestures);
            Ok(IdleUpdate {
                snapshot: if changed {
                    Some(inner.snapshot()?)
                } else {
                    None
                },
                slots_changed: !r.loaded.is_empty()
                    || !r.failed.is_empty()
                    || !r.restart.is_empty()
                    || r.compiled.is_some(),
                params_changed: r.params_changed.iter().map(|i| i.0).collect(),
                problems,
            })
        })
    }

    /// Every plugin slot with its status: instruments' own slots, then
    /// inserts.
    pub fn plugin_slots(&self) -> Result<Vec<SlotRow>, VizError> {
        ffi_guard(|| {
            let inner = lock(&self.inner);
            plugins::with(self.id, |p| plugins::slot_rows(inner.doc(), p))
        })
    }
}

/// Mixer reads and edits (spec §11). Every edit is one undo step; a drag
/// (fader, pan, send level) is bracketed by `begin_edit` / `end_edit` and
/// becomes one step.
#[uniffi::export]
impl Session {
    /// The mixer: strips in display order (master last) with their slots.
    pub fn mixer(&self) -> Result<MixerSnapshot, VizError> {
        ffi_guard(|| {
            // A copy, so the session lock is not held while the registry is.
            let (doc, version) = {
                let inner = lock(&self.inner);
                (inner.doc().clone(), inner.version)
            };
            plugins::with(self.id, |p| mixer::snapshot(&doc, version, p))
        })
    }

    /// Meters, DSP load, latency and fault-bypassed plugins (empty unless
    /// this session holds the engine). The mixer polls it at 30 Hz.
    pub fn levels(&self) -> Result<MixerLevels, VizError> {
        ffi_guard(|| Ok(host::levels(self.id).unwrap_or_else(MixerLevels::empty)))
    }

    /// Opens a continuous edit (a drag): commands until `end_edit` are one
    /// undo step named `label`, and each is heard at once.
    pub fn begin_edit(&self, label: String) -> Result<(), VizError> {
        ffi_guard(|| Ok(lock(&self.inner).history.begin(label)?))
    }

    /// Closes the edit `begin_edit` opened (nothing happens when none is
    /// open).
    pub fn end_edit(&self) -> Result<DocSnapshot, VizError> {
        ffi_guard(|| {
            let mut inner = lock(&self.inner);
            if inner.history.in_transaction() {
                inner.history.commit()?;
                inner.version += 1;
            }
            inner.snapshot()
        })
    }

    /// Adds an aux bus at the end.
    pub fn add_bus(&self, name: String) -> Result<DocSnapshot, VizError> {
        ffi_guard(|| self.execute(Command::Mixer(MixerCmd::AddBus { name, index: None })))
    }

    /// Removes a bus (outputs to it go to master, sends to it go).
    pub fn remove_bus(&self, strip: u64) -> Result<DocSnapshot, VizError> {
        ffi_guard(|| {
            self.execute(Command::Mixer(MixerCmd::RemoveBus {
                strip: StripId(strip),
            }))
        })
    }

    /// Renames a strip.
    pub fn rename_strip(&self, strip: u64, name: String) -> Result<DocSnapshot, VizError> {
        ffi_guard(|| {
            self.execute(Command::Mixer(MixerCmd::RenameStrip {
                strip: StripId(strip),
                name,
            }))
        })
    }

    /// Moves a strip (not master) to display position `index`.
    pub fn move_strip(&self, strip: u64, index: u32) -> Result<DocSnapshot, VizError> {
        ffi_guard(|| {
            self.execute(Command::Mixer(MixerCmd::MoveStrip {
                strip: StripId(strip),
                index: index as usize,
            }))
        })
    }

    /// Sets a fader (dB; −144 is −∞, at most +12).
    pub fn set_gain(&self, strip: u64, gain_db: f64) -> Result<DocSnapshot, VizError> {
        ffi_guard(|| {
            self.execute(Command::Mixer(MixerCmd::SetGain {
                strip: StripId(strip),
                gain_db,
            }))
        })
    }

    /// Sets a pan (−1 … +1).
    pub fn set_pan(&self, strip: u64, pan: f64) -> Result<DocSnapshot, VizError> {
        ffi_guard(|| {
            self.execute(Command::Mixer(MixerCmd::SetPan {
                strip: StripId(strip),
                pan,
            }))
        })
    }

    /// Mutes or unmutes a strip.
    pub fn set_strip_mute(&self, strip: u64, mute: bool) -> Result<DocSnapshot, VizError> {
        ffi_guard(|| {
            self.execute(Command::Mixer(MixerCmd::SetMute {
                strip: StripId(strip),
                mute,
            }))
        })
    }

    /// Solos or unsolos a strip.
    pub fn set_strip_solo(&self, strip: u64, solo: bool) -> Result<DocSnapshot, VizError> {
        ffi_guard(|| {
            self.execute(Command::Mixer(MixerCmd::SetSolo {
                strip: StripId(strip),
                solo,
            }))
        })
    }

    /// Routes a strip's output to bus `bus`, or to master (`None`).
    pub fn set_output(&self, strip: u64, bus: Option<u64>) -> Result<DocSnapshot, VizError> {
        ffi_guard(|| {
            self.execute(Command::Mixer(MixerCmd::SetOutput {
                strip: StripId(strip),
                output: bus.map_or(Output::Master, |b| Output::Bus(StripId(b))),
            }))
        })
    }

    /// Adds a post-fader send at 0 dB to bus `target`.
    pub fn add_send(&self, strip: u64, target: u64) -> Result<DocSnapshot, VizError> {
        ffi_guard(|| {
            self.execute(Command::Mixer(MixerCmd::AddSend {
                strip: StripId(strip),
                send: AuxSend::new(StripId(target)),
            }))
        })
    }

    /// Changes send `index` of a strip.
    pub fn edit_send(
        &self,
        strip: u64,
        index: u32,
        send: crate::mixer::SendRow,
    ) -> Result<DocSnapshot, VizError> {
        ffi_guard(|| {
            self.execute(Command::Mixer(MixerCmd::EditSend {
                strip: StripId(strip),
                index: index as usize,
                send: AuxSend {
                    target: StripId(send.target),
                    level_db: send.level_db,
                    pre_fader: send.pre_fader,
                    enabled: send.enabled,
                },
            }))
        })
    }

    /// Removes send `index` of a strip.
    pub fn remove_send(&self, strip: u64, index: u32) -> Result<DocSnapshot, VizError> {
        ffi_guard(|| {
            self.execute(Command::Mixer(MixerCmd::RemoveSend {
                strip: StripId(strip),
                index: index as usize,
            }))
        })
    }

    /// Inserts `plugin` at `index` of a strip's chain (default: the end);
    /// it starts loading at once.
    pub fn add_insert(
        &self,
        strip: u64,
        index: Option<u32>,
        plugin: PluginRefRow,
    ) -> Result<DocSnapshot, VizError> {
        ffi_guard(|| {
            self.execute(Command::Mixer(MixerCmd::AddInsert {
                strip: StripId(strip),
                index: index.map(|i| i as usize),
                plugin: (&plugin).into(),
            }))
        })
    }

    /// Removes an insert (its state is captured first, so Undo restores
    /// it).
    pub fn remove_insert(&self, slot: u64) -> Result<DocSnapshot, VizError> {
        ffi_guard(|| {
            self.execute(Command::Mixer(MixerCmd::RemoveInsert {
                slot: PluginInstanceId(slot),
            }))
        })
    }

    /// Moves an insert to position `index` of `strip` (drag to reorder).
    pub fn move_insert(&self, slot: u64, strip: u64, index: u32) -> Result<DocSnapshot, VizError> {
        ffi_guard(|| {
            self.execute(Command::Mixer(MixerCmd::MoveInsert {
                slot: PluginInstanceId(slot),
                strip: StripId(strip),
                index: index as usize,
            }))
        })
    }

    /// Bypasses a plugin, or turns it back on.
    pub fn set_bypass(&self, slot: u64, bypass: bool) -> Result<DocSnapshot, VizError> {
        ffi_guard(|| {
            self.execute(Command::Mixer(MixerCmd::SetBypass {
                slot: PluginInstanceId(slot),
                bypass,
            }))
        })
    }

    /// Feeds an insert's sidechain from strip `source` (post-fader), or
    /// from nothing.
    pub fn set_sidechain(&self, slot: u64, source: Option<u64>) -> Result<DocSnapshot, VizError> {
        ffi_guard(|| {
            self.execute(Command::Mixer(MixerCmd::SetSidechain {
                slot: PluginInstanceId(slot),
                source: source.map(StripId),
            }))
        })
    }

    /// Adds a strip for extra output `index` of a multi-output plugin
    /// instrument ("Add Outputs…").
    pub fn add_instrument_output(
        &self,
        dest: u64,
        index: u16,
        name: String,
    ) -> Result<DocSnapshot, VizError> {
        ffi_guard(|| {
            self.execute(Command::Mixer(MixerCmd::AddInstrumentOutput {
                dest: DestId(dest),
                index,
                name,
            }))
        })
    }

    /// Adds a plugin instrument destination named `name` (and its
    /// instrument strip): "New Plugin Instrument…".
    pub fn add_plugin_instrument(
        &self,
        name: String,
        plugin: PluginRefRow,
    ) -> Result<DocSnapshot, VizError> {
        ffi_guard(|| {
            self.execute(Command::Routing(RoutingCmd::AddDestination {
                name,
                kind: DestKind::PluginInstrument {
                    slot: PluginSlot::new(PluginInstanceId(0), (&plugin).into()),
                },
                default_patch: None,
            }))
        })
    }

    /// The instrument list: every destination, with the plugin it plays
    /// (none: a MIDI endpoint or the built-in synth).
    pub fn destinations(&self) -> Result<Vec<DestRow>, VizError> {
        ffi_guard(|| {
            let inner = lock(&self.inner);
            Ok(inner
                .doc()
                .instruments()
                .values()
                .map(|d| DestRow {
                    id: d.id.0,
                    name: d.name.clone(),
                    plugin: match &d.kind {
                        DestKind::PluginInstrument { slot } => Some((&slot.plugin).into()),
                        DestKind::MidiEndpoint { .. } => None,
                    },
                })
                .collect())
        })
    }

    /// Makes destination `dest` play `plugin` (every lane using it now
    /// plays the plugin; one undo step). The same plugin keeps its
    /// instance and state (3a Plan Ruling 10).
    pub fn set_destination_plugin(
        &self,
        dest: u64,
        plugin: PluginRefRow,
    ) -> Result<DocSnapshot, VizError> {
        ffi_guard(|| {
            let default_patch = lock(&self.inner)
                .doc()
                .instruments()
                .get(&DestId(dest))
                .ok_or_else(|| VizError::Model {
                    message: format!("unknown id {}", DestId(dest)),
                })?
                .default_patch;
            self.execute(Command::Routing(RoutingCmd::EditDestination {
                dest: DestId(dest),
                kind: DestKind::PluginInstrument {
                    slot: PluginSlot::new(PluginInstanceId(0), (&plugin).into()),
                },
                default_patch,
            }))
        })
    }
}

/// "Strip — Plugin" for messages about slot `id`.
fn slot_name(doc: &Document, id: core_model::ids::PluginInstanceId) -> String {
    use core_model::document::SlotOwner;
    let plugin = doc
        .plugin_slot(id)
        .map_or_else(|| id.to_string(), |s| s.plugin.name.clone());
    let place = match doc.find_slot(id) {
        Some(SlotOwner::Insert(strip, _)) => doc.mixer().strip(strip).ok().map(|s| s.name.clone()),
        Some(SlotOwner::Instrument(d)) => doc.instruments().get(&d).map(|d| d.name.clone()),
        None => None,
    };
    match place {
        Some(p) => format!("{p} — {plugin}"),
        None => plugin,
    }
}

#[cfg(feature = "test-hooks")]
#[uniffi::export]
impl Session {
    /// The sample rate this session's plugins are activated for (test
    /// hook).
    pub fn debug_plugin_rate(&self) -> Result<f64, VizError> {
        ffi_guard(|| plugins::with(self.id, |p| p.map_or(0.0, |p| p.config().0)))
    }

    /// Sets parameter `param` of slot `slot` as one undo step, like a
    /// plugin-UI gesture (test hook).
    pub fn debug_set_slot_param(
        &self,
        slot: u64,
        param: u64,
        value: f64,
    ) -> Result<DocSnapshot, VizError> {
        ffi_guard(|| {
            self.execute(Command::Mixer(MixerCmd::SetPluginParams {
                slot: PluginInstanceId(slot),
                params: vec![(core_model::mixer::ParamId(param), value)],
            }))
        })
    }

    /// Slot `slot`'s state blob in the document, if it has one (test
    /// hook).
    pub fn debug_slot_state(&self, slot: u64) -> Result<Option<u64>, VizError> {
        ffi_guard(|| {
            let inner = lock(&self.inner);
            Ok(inner
                .doc()
                .plugin_slot(PluginInstanceId(slot))
                .and_then(|s| s.state)
                .map(|b| b.0))
        })
    }

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
        // The engine goes first when this session's plugin host feeds it;
        // queued if a plugin call is running (re-entrancy, plugins.rs).
        plugins::close_session(self.id);
    }
}
