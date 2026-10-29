//! The plugin host (spec §5, §6): keeps one instance per plugin slot of the
//! document, loads new ones (format work off the main thread on the loader
//! thread, the rest on the main thread, Plan Ruling 2), activates them and
//! shelves their processors for the engine, feeds the parameter overlay to
//! them, turns UI gestures into undoable commands, captures state, and
//! restarts plugins that ask for it. Main thread only (it is not `Send`).

use crate::adapt::{PluginEffect, PluginSynth, Returned};
use crate::clap::ClapFormat;
use crate::error::HostError;
use crate::instance::{AudioProcessor, LoadRequest, PluginFormat, PluginInstance, Prepared};
use crate::process::ParamEvent;
use crate::queue::{GestureReceiver, ParamSender, param_queues};
use crate::shelf::{Ready, Shelf, ShelfFactory};
use core_engine::EngineError;
use core_engine::engine::Engine;
use core_engine::schedule::CompileReport;
use core_model::command::{Command, MixerCmd};
use core_model::document::{DestKind, Document};
use core_model::history::History;
use core_model::ids::{PluginInstanceId, SeqId, StateBlobId};
use core_model::mixer::{ParamId, PluginFormat as Format, PluginRef, PluginSlot};
use core_model::selection::ViewFilter;
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::mpsc::{Receiver, Sender, channel};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

/// One parameter gesture from a plugin's own UI: an undoable
/// `SetPluginParams` (spec §4).
#[derive(Debug, Clone, PartialEq)]
pub struct Gesture {
    /// The instance.
    pub slot: PluginInstanceId,
    /// Final value per parameter, in first-touched order.
    pub values: Vec<(ParamId, f64)>,
}

/// What [`PluginHost::idle`] did.
#[derive(Debug, Default)]
pub struct IdleReport {
    /// Instances that finished loading and are shelved.
    pub loaded: Vec<PluginInstanceId>,
    /// Instances that could not load, and why (they play as placeholders).
    pub failed: Vec<(PluginInstanceId, String)>,
    /// Instances that asked to be restarted (the engine must drop them).
    pub restart: Vec<PluginInstanceId>,
    /// Finished UI gestures.
    pub gestures: Vec<Gesture>,
    /// Every parameter event the plugins reported, in order, beside the
    /// grouped gestures, with an `End` for each parameter of a gesture the
    /// host closed itself (plan 5e-1 Ruling 15): automation writes from
    /// them.
    pub param_events: Vec<(PluginInstanceId, ParamEvent)>,
    /// Saved states a plugin rejected on load (it plays with its defaults).
    pub rejected_state: Vec<(PluginInstanceId, String)>,
    /// Instances whose parameter list or values changed outside a gesture
    /// (refresh their parameter lists).
    pub params_changed: Vec<PluginInstanceId>,
    /// Instances whose state changed in a way worth saving (autosave).
    pub state_dirty: Vec<PluginInstanceId>,
    /// The engine owes a load (processors were shelved, an edit waited
    /// for a load, a load failed, a restart is due). From
    /// [`PluginHost::idle`]: the caller loads the engine once `sync`
    /// reports no load in progress. From [`PluginHost::service`]: a load is
    /// still owed after this call (one is in progress, or the engine
    /// refused the last one).
    pub reload: bool,
    /// The engine load `PluginHost::service` ran, if any.
    pub compiled: Option<CompileReport>,
}

impl IdleReport {
    fn merge(&mut self, o: IdleReport) {
        self.loaded.extend(o.loaded);
        self.failed.extend(o.failed);
        self.restart.extend(o.restart);
        self.gestures.extend(o.gestures);
        self.param_events.extend(o.param_events);
        self.rejected_state.extend(o.rejected_state);
        self.params_changed.extend(o.params_changed);
        self.state_dirty.extend(o.state_dirty);
        self.reload |= o.reload;
        if o.compiled.is_some() {
            self.compiled = o.compiled;
        }
    }
}

/// Where a slot stands (for the mixer and the Plugin Manager).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SlotState {
    /// Loading (or waiting for the next `sync` to load it).
    Loading,
    /// Running.
    Live,
    /// A placeholder, and why: it failed to load, is unavailable, or is
    /// held ([`PluginHost::hold`]).
    Failed(String),
}

/// How long an open gesture may go without an event from the plugin before
/// the host closes it (its `End` was lost).
pub const GESTURE_TIMEOUT: Duration = Duration::from_secs(10);

/// What the document wants of a slot.
#[derive(Clone)]
struct Want {
    plugin: PluginRef,
    instrument: bool,
    state: Option<(StateBlobId, Arc<[u8]>)>,
    params: BTreeMap<ParamId, f64>,
}

impl Want {
    fn of(doc: &Document, slot: &PluginSlot, instrument: bool) -> Want {
        Want {
            plugin: slot.plugin.clone(),
            instrument,
            state: slot
                .state
                .and_then(|id| doc.state_blob(id).map(|b| (id, Arc::from(b)))),
            params: slot.params.iter().map(|(k, v)| (*k, *v)).collect(),
        }
    }
}

/// A value the plugin holds that the document does not have yet: from a
/// gesture in the plugin's UI (open, or reported and not yet committed) or
/// from [`PluginHost::set_param`] (a mixer control that commits later).
#[derive(Debug, Clone, Copy, PartialEq)]
struct Pending {
    /// What the plugin holds.
    value: f64,
    /// The document's overlay value for the parameter when `value` was
    /// recorded. While the document still says this, it has not caught up
    /// and the plugin is left alone; any other value is a newer edit (an
    /// undo, a redo, a mixer edit) that replaces `value`.
    doc: Option<f64>,
}

/// A live instance and its main-thread ends.
struct Live {
    inst: Box<dyn PluginInstance>,
    params: ParamSender,
    gestures: GestureReceiver,
    /// The processor is out (on the shelf or in the engine).
    active: bool,
    /// Address of the processor that is out, to recognise it when the
    /// engine gives it back.
    processor: Option<usize>,
    /// The state blob last loaded into it.
    applied_state: Option<StateBlobId>,
    /// Parameter values before the overlay (to restore when an overlay
    /// entry goes away, e.g. on undo).
    base: BTreeMap<ParamId, f64>,
    /// The values it was loaded with when it had no state: restored when an
    /// undo goes back past its first capture (Plan Ruling 15).
    defaults: Option<BTreeMap<ParamId, f64>>,
    /// Overlay values the plugin holds (sent by the host, or set by a
    /// gesture).
    sent: BTreeMap<ParamId, f64>,
    /// Values the document has not caught up with (Plan Ruling 16: never
    /// echoed over).
    pending: BTreeMap<ParamId, Pending>,
    /// Values of the gesture in progress.
    gesture: Vec<(ParamId, f64)>,
    /// Parameters whose gesture is open.
    open: BTreeSet<ParamId>,
    /// When the plugin last reported a parameter event.
    last_event: Option<Instant>,
}

/// A removed instance whose processor is still out.
struct Retired {
    plugin: PluginRef,
    instrument: bool,
    live: Box<Live>,
}

enum State {
    /// Waiting for the loader or for the main-thread step.
    Loading(u64),
    /// Running.
    Live(Box<Live>),
    /// Could not load (or is disabled), with why.
    Failed(String),
}

struct Slot {
    want: Want,
    state: State,
}

/// An engine load `service` ran that the engine refused (an error, or a
/// graph it could not plan), and what it was given.
struct Refused {
    doc: Document,
    seq: SeqId,
    block: u32,
}

/// A job for the loader thread.
type Job = (PluginInstanceId, u64, crate::instance::PrepareJob);
type Done = (PluginInstanceId, u64, Result<Prepared, HostError>);

/// The plugin host. Create it on the main thread; drop the engine before
/// it (processors in the engine come back through it).
///
/// What the app must do around it:
/// - Call [`PluginHost::sync`] after every committed edit, and load the
///   engine itself only when it returns true; otherwise
///   [`PluginHost::service`] loads it once the loads are done.
/// - Call [`PluginHost::reconfigure`] when the device's sample rate
///   changes: `service` follows only a grown block size, and the shelf
///   does not check the rate processors were activated at.
/// - Call [`PluginHost::capture`] before committing the removal of a
///   plugin whose state must survive (an undo of the removal restores the
///   captured state; `sync` cannot capture, it has no `History`).
/// - After [`PluginHost::commit_gestures`], call `sync` before an undo,
///   so the host knows the document has the gesture's values and sends the
///   undone ones.
/// - When [`PluginHost::unloads_due`] (a plugin was made unavailable while
///   running), call [`PluginHost::unload_unavailable`]; the engine then
///   drops the instances of [`PluginHost::take_evicted`] and loads
///   (`service` does both).
pub struct PluginHost {
    formats: Vec<Box<dyn PluginFormat>>,
    paths: BTreeMap<(Format, String), PathBuf>,
    unavailable: BTreeMap<(Format, String), String>,
    /// Plugins the user turned off ([`PluginHost::set_disabled`]): their
    /// running instances are unloaded, not only their next loads.
    disabled: BTreeSet<(Format, String)>,
    sample_rate: f64,
    max_frames: u32,
    slots: BTreeMap<PluginInstanceId, Slot>,
    /// Instances removed from the document whose processor is still out
    /// (several per id when an id was reused), oldest first.
    retiring: BTreeMap<PluginInstanceId, Vec<Retired>>,
    /// The report of a `service` whose engine load failed, given out by
    /// the next `idle` or `service` (nothing in it is lost, and the next
    /// call hands it out even when the load fails again).
    carry: Option<IdleReport>,
    /// The engine owes a load: set when processors are shelved, when
    /// `sync` defers an edit, when a load fails and when a restart is due;
    /// cleared only by a load in `service` that published a graph (or when
    /// `idle` handed it to its caller with no load in progress).
    owed_load: bool,
    /// The last load `service` ran was refused: it is not run again until
    /// the document, the sequence or the block size changes, a load
    /// finishes or a restart is due.
    refused: Option<Refused>,
    /// Instances whose processor came back while the load was refused:
    /// deactivated, and activated again only when the refusal ends (no
    /// activation churn for an identical load).
    parked: BTreeSet<PluginInstanceId>,
    /// Prepared loads (with their generation) waiting for their
    /// main-thread step.
    finishing: VecDeque<(PluginInstanceId, u64, Prepared)>,
    generation: u64,
    /// Slots kept from loading (safe mode), with the reason they show.
    held: BTreeMap<PluginInstanceId, String>,
    /// Instances [`PluginHost::unload_unavailable`] unloaded: the engine
    /// keeps a running instance of a slot it still has, so it must be told
    /// to drop them ([`PluginHost::take_evicted`]; `service` does).
    evicted: BTreeSet<PluginInstanceId>,
    /// Gestures closed by [`PluginHost::end_gestures`], reported by the
    /// next `idle` or `service`.
    ended: Vec<Gesture>,
    /// Every parameter event read since the last report, in the order
    /// read (with the ends the host made itself), reported by the next
    /// `idle` or `service` as `param_events`.
    raw: Vec<(PluginInstanceId, ParamEvent)>,
    gesture_timeout: Duration,
    shelf: Shelf,
    back_tx: Sender<Returned>,
    back_rx: Receiver<Returned>,
    jobs: Option<Sender<Job>>,
    done: Receiver<Done>,
    loader: Option<JoinHandle<()>>,
}

impl std::fmt::Debug for PluginHost {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PluginHost")
            .field("slots", &self.slots.len())
            .finish_non_exhaustive()
    }
}

impl PluginHost {
    /// A host with the CLAP format, activating plugins at `sample_rate`
    /// for blocks of up to `max_frames`; starts the loader thread.
    pub fn new(sample_rate: f64, max_frames: u32) -> Result<PluginHost, HostError> {
        let (jobs, job_rx) = channel::<Job>();
        let (done_tx, done) = channel::<Done>();
        let loader = std::thread::Builder::new()
            .name("vizdaw-plugin-loader".into())
            .spawn(move || {
                while let Ok((id, generation, job)) = job_rx.recv() {
                    let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(job))
                        .unwrap_or_else(|_| {
                            Err(HostError::LoadFailed("the loader panicked".into()))
                        });
                    if done_tx.send((id, generation, r)).is_err() {
                        break;
                    }
                }
            })
            .map_err(|e| HostError::LoadFailed(format!("cannot start the loader: {e}")))?;
        let (back_tx, back_rx) = channel();
        Ok(PluginHost {
            formats: vec![Box::new(ClapFormat::new())],
            paths: BTreeMap::new(),
            unavailable: BTreeMap::new(),
            disabled: BTreeSet::new(),
            sample_rate,
            max_frames: max_frames.max(1),
            slots: BTreeMap::new(),
            retiring: BTreeMap::new(),
            carry: None,
            owed_load: false,
            refused: None,
            parked: BTreeSet::new(),
            finishing: VecDeque::new(),
            generation: 0,
            held: BTreeMap::new(),
            evicted: BTreeSet::new(),
            ended: Vec::new(),
            raw: Vec::new(),
            gesture_timeout: GESTURE_TIMEOUT,
            shelf: Shelf::new(),
            back_tx,
            back_rx,
            jobs: Some(jobs),
            done,
            loader: Some(loader),
        })
    }

    /// Adds a format (AU on macOS).
    pub fn add_format(&mut self, f: Box<dyn PluginFormat>) {
        self.formats.push(f);
    }

    /// Says where plugin `id` of `format` lives (the catalog does this).
    pub fn register_path(&mut self, format: Format, id: &str, path: PathBuf) {
        self.paths.insert((format, id.to_string()), path);
    }

    /// Marks a plugin as unavailable (disabled, blocklisted or missing);
    /// its slots become placeholders showing `why` (spec §9) when they
    /// next load: running instances keep running, unless the user turned
    /// the plugin off ([`PluginHost::set_disabled`]). `None` makes it
    /// available again (its slots load at the next `sync`).
    pub fn set_unavailable(&mut self, format: Format, id: &str, why: Option<String>) {
        let key = (format, id.to_string());
        match why {
            Some(w) => {
                self.unavailable.insert(key, w);
            }
            None => {
                self.unavailable.remove(&key);
                for s in self.slots.values_mut() {
                    if s.want.plugin.format == format
                        && s.want.plugin.id == id
                        && matches!(s.state, State::Failed(_))
                    {
                        // `sync` loads it again.
                        s.state = State::Failed(String::new());
                    }
                }
            }
        }
    }

    /// Records whether the user turned a plugin off (the Plugin Manager's
    /// preference, spec §9). Together with [`PluginHost::set_unavailable`]
    /// this makes its running instances due for
    /// [`PluginHost::unload_unavailable`]; a plugin unavailable for another
    /// reason (blocklisted by a rescan, missing) keeps its running
    /// instances until they next load.
    pub fn set_disabled(&mut self, format: Format, id: &str, disabled: bool) {
        let key = (format, id.to_string());
        if disabled {
            self.disabled.insert(key);
        } else {
            self.disabled.remove(&key);
        }
    }

    /// The engine's insert factory (install it with
    /// `Engine::set_insert_factory`).
    pub fn factory(&self) -> ShelfFactory {
        self.shelf.factory()
    }

    /// Rate and block size instances are activated for.
    pub fn config(&self) -> (f64, u32) {
        (self.sample_rate, self.max_frames)
    }

    /// Loads still in progress.
    pub fn pending(&self) -> usize {
        self.slots
            .values()
            .filter(|s| matches!(s.state, State::Loading(_)))
            .count()
    }

    /// Live instances (loaded, not failed).
    pub fn live_count(&self) -> usize {
        self.slots
            .values()
            .filter(|s| matches!(s.state, State::Live(_)))
            .count()
    }

    /// Removed instances whose processor the engine has not given back.
    pub fn retiring(&self) -> usize {
        self.retiring.values().map(Vec::len).sum()
    }

    /// Instance `id`, if loaded (for its editor and parameter list).
    pub fn instance(&mut self, id: PluginInstanceId) -> Option<&mut dyn PluginInstance> {
        match self.slots.get_mut(&id).map(|s| &mut s.state) {
            Some(State::Live(l)) => Some(l.inst.as_mut()),
            _ => None,
        }
    }

    /// Why slot `id` has no instance, if it has none.
    pub fn failure(&self, id: PluginInstanceId) -> Option<&str> {
        match self.slots.get(&id).map(|s| &s.state) {
            Some(State::Failed(w)) => Some(w),
            _ => None,
        }
    }

    /// Where slot `id` stands, if the document has it.
    pub fn state(&self, id: PluginInstanceId) -> Option<SlotState> {
        Some(match &self.slots.get(&id)?.state {
            State::Loading(_) => SlotState::Loading,
            State::Live(_) => SlotState::Live,
            State::Failed(w) if w.is_empty() => SlotState::Loading,
            State::Failed(w) => SlotState::Failed(w.clone()),
        })
    }

    /// The plugins loading now (the crash marker names them, spec §10).
    pub fn loading(&self) -> Vec<(PluginInstanceId, PluginRef)> {
        self.slots
            .iter()
            .filter(|(_, s)| matches!(s.state, State::Loading(_)))
            .map(|(id, s)| (*id, s.want.plugin.clone()))
            .collect()
    }

    /// The plugins a [`sync`](Self::sync) of `doc` would start loading
    /// now: slots it gains (or whose plugin changed), and slots waiting
    /// to load again, minus held and unavailable ones. The crash marker
    /// names them before the sync runs any plugin code (spec §10).
    pub fn loads_needed(&self, doc: &Document) -> Vec<PluginRef> {
        let mut wanted: Vec<(PluginInstanceId, &PluginSlot, bool)> = Vec::new();
        for d in doc.instruments().values() {
            if let DestKind::PluginInstrument { slot } = &d.kind {
                wanted.push((slot.id, slot, true));
            }
        }
        for (_, _, slot) in doc.mixer().inserts() {
            wanted.push((slot.id, slot, false));
        }
        wanted
            .into_iter()
            .filter(|(id, slot, instrument)| {
                let p = &slot.plugin;
                let key = (p.format, p.id.clone());
                if self.held.contains_key(id)
                    || self.unavailable.contains_key(&key)
                    || !self.formats.iter().any(|f| f.format() == p.format)
                {
                    return false;
                }
                match self.slots.get(id) {
                    None => true,
                    Some(s) => {
                        let same = s.want.plugin.format == p.format
                            && s.want.plugin.id == p.id
                            && s.want.instrument == *instrument;
                        !same || matches!(&s.state, State::Failed(w) if w.is_empty())
                    }
                }
            })
            .map(|(_, slot, _)| slot.plugin.clone())
            .collect()
    }

    /// Keeps slot `id` from loading (safe mode, spec §10): until released
    /// it plays as a placeholder showing `why`. `None` releases it; it
    /// loads at the next `sync`. A slot already running is not stopped.
    pub fn hold(&mut self, id: PluginInstanceId, why: Option<String>) {
        match why {
            Some(w) => {
                self.held.insert(id, w);
            }
            None => {
                if self.held.remove(&id).is_some() {
                    if let Some(s) = self.slots.get_mut(&id) {
                        if matches!(s.state, State::Failed(_)) {
                            s.state = State::Failed(String::new());
                        }
                    }
                }
            }
        }
    }

    /// The slots kept from loading.
    pub fn held(&self) -> Vec<PluginInstanceId> {
        self.held.keys().copied().collect()
    }

    /// How long an open gesture may go without an event before it is
    /// closed ([`GESTURE_TIMEOUT`] by default).
    pub fn set_gesture_timeout(&mut self, timeout: Duration) {
        self.gesture_timeout = timeout;
    }

    /// Closes instance `id`'s open gesture (its editor window closed): what
    /// it changed so far is reported by the next `idle` or `service`.
    pub fn end_gestures(&mut self, id: PluginInstanceId) {
        let Some(Slot { want, state }) = self.slots.get_mut(&id) else {
            return;
        };
        let State::Live(live) = state else {
            return;
        };
        collect_gestures(id, live, &want.params, &mut self.ended, &mut self.raw);
        end_open(id, live, &mut self.raw);
        live.open.clear();
        flush_gesture(id, live, &mut self.ended);
    }

    /// Sets a parameter now (a mixer control): sent to the running plugin;
    /// the app also records it in the overlay with `SetPluginParams`. Until
    /// the document has that value (or a newer edit of the parameter),
    /// `sync` leaves the plugin's value alone.
    pub fn set_param(&mut self, id: PluginInstanceId, param: ParamId, value: f64) {
        let Some(Slot { want, state }) = self.slots.get_mut(&id) else {
            return;
        };
        if let State::Live(l) = state {
            if push(l, param, value) {
                l.sent.insert(param, value);
                l.pending.insert(
                    param,
                    Pending {
                        value,
                        doc: want.params.get(&param).copied(),
                    },
                );
            }
        }
    }

    /// Call after every committed edit (and on open): starts loading the
    /// slots `doc` gained, drops the ones it lost, and sends overlay and
    /// state changes to running instances. Returns true when no load is in
    /// progress, i.e. the engine may load `doc` now.
    pub fn sync(&mut self, doc: &Document) -> bool {
        let mut wanted: BTreeMap<PluginInstanceId, Want> = BTreeMap::new();
        for d in doc.instruments().values() {
            if let DestKind::PluginInstrument { slot } = &d.kind {
                wanted.insert(slot.id, Want::of(doc, slot, true));
            }
        }
        for (_, _, slot) in doc.mixer().inserts() {
            wanted.insert(slot.id, Want::of(doc, slot, false));
        }
        // Slots that went, or now hold another plugin.
        let gone: Vec<PluginInstanceId> = self
            .slots
            .iter()
            .filter(|(id, s)| {
                wanted.get(id).is_none_or(|w| {
                    w.plugin.format != s.want.plugin.format
                        || w.plugin.id != s.want.plugin.id
                        || w.instrument != s.want.instrument
                })
            })
            .map(|(id, _)| *id)
            .collect();
        for id in gone {
            self.remove(id);
        }
        for (id, want) in wanted {
            let key = (want.plugin.format, want.plugin.id.clone());
            let unavailable = self.unavailable.get(&key).cloned();
            match self.slots.get_mut(&id) {
                Some(s) => {
                    s.want = want;
                    match &mut s.state {
                        State::Live(l) => apply_want(l, &s.want),
                        State::Failed(w) if w.is_empty() => {
                            self.start_load(id);
                        }
                        _ => {}
                    }
                }
                None => {
                    // Back before the engine gave the processor of the
                    // removed instance back (an undo): the engine still
                    // plays it, so keep using it.
                    if let Some(mut live) = self.revive(id, &want) {
                        apply_want(&mut live, &want);
                        self.slots.insert(
                            id,
                            Slot {
                                want,
                                state: State::Live(live),
                            },
                        );
                        continue;
                    }
                    self.slots.insert(
                        id,
                        Slot {
                            want,
                            state: State::Failed(String::new()),
                        },
                    );
                    match self.held.get(&id).cloned().or(unavailable) {
                        Some(why) => self.fail(id, why),
                        None => self.start_load(id),
                    }
                }
            }
        }
        let ready = self.pending() == 0;
        if !ready {
            // The caller defers its engine load; `service` runs it once
            // the loads are done, whether they succeed or fail.
            self.owed_load = true;
        }
        ready
    }

    fn fail(&mut self, id: PluginInstanceId, why: String) {
        self.shelf.set_reason(id, why.clone());
        if let Some(s) = self.slots.get_mut(&id) {
            if let State::Live(l) = &s.state {
                end_open(id, l, &mut self.raw);
            }
            s.state = State::Failed(why);
        }
    }

    fn start_load(&mut self, id: PluginInstanceId) {
        let Some(want) = self.slots.get(&id).map(|s| s.want.clone()) else {
            return;
        };
        if let Some(why) = self.held.get(&id).cloned() {
            self.fail(id, why);
            return;
        }
        if let Some(why) = self
            .unavailable
            .get(&(want.plugin.format, want.plugin.id.clone()))
            .cloned()
        {
            self.fail(id, why);
            return;
        }
        let req = self.request(&want.plugin);
        let Some(format) = self
            .formats
            .iter()
            .find(|f| f.format() == want.plugin.format)
        else {
            self.fail(
                id,
                format!("{:?} plugins are not supported here", want.plugin.format),
            );
            return;
        };
        let job = format.prepare(&req);
        self.generation += 1;
        let generation = self.generation;
        self.shelf
            .set_reason(id, format!("{} is loading", want.plugin.name));
        if let Some(s) = self.slots.get_mut(&id) {
            s.state = State::Loading(generation);
        }
        let sent = self
            .jobs
            .as_ref()
            .is_some_and(|j| j.send((id, generation, job)).is_ok());
        if !sent {
            self.fail(id, "the plugin loader stopped".into());
        }
    }

    fn request(&self, plugin: &PluginRef) -> LoadRequest {
        LoadRequest {
            plugin: plugin.clone(),
            path: self.paths.get(&(plugin.format, plugin.id.clone())).cloned(),
            sample_rate: self.sample_rate,
            max_frames: self.max_frames,
        }
    }

    /// Drops slot `id`: an instance whose processor is out waits in
    /// `retiring` for it.
    fn remove(&mut self, id: PluginInstanceId) {
        self.shelf.clear_reason(id);
        self.parked.remove(&id);
        let Some(Slot { want, state }) = self.slots.remove(&id) else {
            return;
        };
        if let State::Live(mut l) = state {
            end_open(id, &l, &mut self.raw);
            // The editor goes before the instance does (moved to `retiring`
            // it could no longer be reached): the app frees the parent view
            // once the slot is gone, and a plugin GUI must not outlive it.
            if let Some(e) = l.inst.editor() {
                e.close();
            }
            let shelved = self.shelf.take_unwanted(|k| k != id);
            let mut back = false;
            for p in shelved.into_iter().filter_map(Ready::into_processor) {
                back = true;
                l.inst.deactivate(p);
                l.active = false;
                l.processor = None;
            }
            if l.active && !back {
                self.retiring.entry(id).or_default().push(Retired {
                    plugin: want.plugin,
                    instrument: want.instrument,
                    live: l,
                });
            }
        }
    }

    /// Takes back a removed instance of `want`'s plugin whose processor is
    /// still out, if there is one.
    fn revive(&mut self, id: PluginInstanceId, want: &Want) -> Option<Box<Live>> {
        let list = self.retiring.get_mut(&id)?;
        let i = list.iter().rposition(|r| {
            r.plugin.format == want.plugin.format
                && r.plugin.id == want.plugin.id
                && r.instrument == want.instrument
        })?;
        let r = (i < list.len()).then(|| list.remove(i));
        if list.is_empty() {
            self.retiring.remove(&id);
        }
        // Shown only if the engine already dropped it: then it comes back,
        // is activated again and shelved.
        self.shelf.set_reason(id, "restarting");
        r.map(|r| r.live)
    }

    /// Takes the removed instance that owns `processor` (or, failing a
    /// match, the oldest one of `id` unless the live slot owns it).
    fn take_retired(&mut self, id: PluginInstanceId, processor: usize) -> Option<Retired> {
        let live_owns = matches!(
            self.slots.get(&id).map(|s| &s.state),
            Some(State::Live(l)) if l.processor == Some(processor)
        );
        let list = self.retiring.get_mut(&id)?;
        let i = list
            .iter()
            .position(|r| r.live.processor == Some(processor))
            .or((!live_owns).then_some(0))?;
        let r = (i < list.len()).then(|| list.remove(i));
        if list.is_empty() {
            self.retiring.remove(&id);
        }
        r
    }

    /// Main-thread servicing (`host_idle`, 60 Hz; spec §5): finishes one
    /// load, deactivates processors the engine gave back, runs plugin
    /// callbacks, and collects restarts and gestures. Use it without an
    /// engine (or with one the caller loads itself); with an engine,
    /// [`PluginHost::service`] calls it. `reload` in its report hands the
    /// owed engine load to the caller: once no load is in progress it is
    /// reported once and then forgotten.
    pub fn idle(&mut self) -> IdleReport {
        let mut r = self.tick();
        r.reload = self.owed_load;
        if self.pending() == 0 && self.finishing.is_empty() {
            self.owed_load = false;
        }
        r
    }

    /// [`PluginHost::idle`]'s work, leaving the owed load to the caller.
    fn tick(&mut self) -> IdleReport {
        let mut r = self.carry.take().unwrap_or_default();
        r.gestures.append(&mut self.ended);
        while let Ok((id, generation, result)) = self.done.try_recv() {
            if !self.is_loading(id, generation) {
                continue;
            }
            match result {
                Ok(p) => self.finishing.push_back((id, generation, p)),
                Err(e) => {
                    let why = format!("failed: {e}");
                    r.failed.push((id, why.clone()));
                    self.fail(id, why);
                    // Its placeholder now shows why; an edit may have
                    // waited for it.
                    self.owed_load = true;
                }
            }
        }
        // One load per call; a prepared load whose slot was removed or
        // started loading again since is dropped.
        while let Some((id, generation, prepared)) = self.finishing.pop_front() {
            if self.is_loading(id, generation) {
                self.finish(id, prepared, &mut r);
                break;
            }
        }
        while let Ok(back) = self.back_rx.try_recv() {
            self.returned(back, &mut r);
        }
        let ids: Vec<PluginInstanceId> = self.slots.keys().copied().collect();
        for id in ids {
            self.service_instance(id, &mut r);
        }
        r.param_events.append(&mut self.raw);
        r
    }

    /// Whether slot `id` waits for load `generation`.
    fn is_loading(&self, id: PluginInstanceId, generation: u64) -> bool {
        matches!(
            self.slots.get(&id).map(|s| &s.state),
            Some(State::Loading(g)) if *g == generation
        )
    }

    fn finish(&mut self, id: PluginInstanceId, prepared: Prepared, r: &mut IdleReport) {
        let Some(want) = self.slots.get(&id).map(|s| s.want.clone()) else {
            return;
        };
        let req = self.request(&want.plugin);
        let made = match self
            .formats
            .iter_mut()
            .find(|f| f.format() == want.plugin.format)
        {
            Some(f) => f.finish(&req, prepared),
            None => Err(HostError::NotFound(want.plugin.id.clone())),
        };
        let inst = match made {
            Ok(i) => i,
            Err(e) => {
                let why = format!("failed: {e}");
                r.failed.push((id, why.clone()));
                self.fail(id, why);
                return;
            }
        };
        let ((params, gestures), _) = param_queues();
        let mut live = Box::new(Live {
            inst,
            params,
            gestures,
            active: false,
            processor: None,
            applied_state: None,
            base: BTreeMap::new(),
            defaults: None,
            sent: BTreeMap::new(),
            pending: BTreeMap::new(),
            gesture: Vec::new(),
            open: BTreeSet::new(),
            last_event: None,
        });
        // State first, then the overlay (spec §4).
        if let Some((sid, bytes)) = &want.state {
            match live.inst.load_state(bytes) {
                Ok(()) => live.applied_state = Some(*sid),
                Err(e) => r.rejected_state.push((id, e.to_string())),
            }
        }
        live.base = current_values(live.inst.as_mut());
        if live.applied_state.is_none() {
            live.defaults = Some(live.base.clone());
        }
        let overlay: Vec<(ParamId, f64)> = want.params.iter().map(|(k, v)| (*k, *v)).collect();
        if live.inst.set_params(&overlay).is_ok() {
            live.sent = want.params.clone();
        }
        match self.activate(id, &mut live, want.instrument) {
            Ok(()) => {
                r.loaded.push(id);
                r.reload = true;
                self.owed_load = true;
                if let Some(s) = self.slots.get_mut(&id) {
                    s.state = State::Live(live);
                }
            }
            Err(e) => {
                let why = format!("failed: {e}");
                r.failed.push((id, why.clone()));
                self.fail(id, why);
            }
        }
    }

    /// Activates `live` and shelves its processor with fresh queues.
    fn activate(
        &mut self,
        id: PluginInstanceId,
        live: &mut Live,
        instrument: bool,
    ) -> Result<(), HostError> {
        let proc = live.inst.activate(self.sample_rate, self.max_frames)?;
        let ((params, gestures), audio) = param_queues();
        live.params = params;
        live.gestures = gestures;
        live.active = true;
        live.processor = Some(address(proc.as_ref()));
        let info = live.inst.info().clone();
        let back = self.back_tx.clone();
        let ready = if instrument {
            Ready::Instrument(PluginSynth::new(
                id,
                proc,
                &info,
                self.max_frames,
                audio,
                back,
            ))
        } else {
            Ready::Effect(PluginEffect::new(
                id,
                proc,
                &info,
                self.max_frames,
                audio,
                back,
            ))
        };
        if let Some(old) = self.shelf.put(id, &info.plugin, self.max_frames, ready) {
            // Unreachable: a slot shelves one processor at a time.
            drop(old);
        }
        // Values the engine never saw must reach the new processor.
        let sent: Vec<(ParamId, f64)> = live.sent.iter().map(|(k, v)| (*k, *v)).collect();
        for (k, v) in sent {
            live.params.send(k, v);
        }
        Ok(())
    }

    fn returned(&mut self, back: Returned, r: &mut IdleReport) {
        let Returned { id, processor } = back;
        if let Some(mut r) = self.take_retired(id, address(processor.as_ref())) {
            r.live.inst.deactivate(processor);
            return;
        }
        let Some((instrument, doc)) = self
            .slots
            .get(&id)
            .map(|s| (s.want.instrument, s.want.params.clone()))
        else {
            drop(processor);
            return;
        };
        let Some(State::Live(mut live)) = self
            .slots
            .get_mut(&id)
            .map(|s| std::mem::replace(&mut s.state, State::Failed(String::new())))
        else {
            drop(processor);
            return;
        };
        live.inst.deactivate(processor);
        live.active = false;
        live.processor = None;
        // Its gesture queue goes with it: report what is in it, and close
        // a gesture the deactivation cut off (its `End` never comes).
        collect_gestures(id, &mut live, &doc, &mut r.gestures, &mut self.raw);
        end_open(id, &live, &mut self.raw);
        live.open.clear();
        flush_gesture(id, &mut live, &mut r.gestures);
        if self.refused.is_some() {
            // The engine refused the load that would have used it: wait
            // for a change instead of activating it for the same load.
            self.parked.insert(id);
            if let Some(s) = self.slots.get_mut(&id) {
                s.state = State::Live(live);
            }
            return;
        }
        // A processor comes back only when the engine dropped it: a
        // restart or a new configuration. Activate it again.
        match self.activate(id, &mut live, instrument) {
            Ok(()) => {
                r.reload = true;
                self.owed_load = true;
                if let Some(s) = self.slots.get_mut(&id) {
                    s.state = State::Live(live);
                }
            }
            Err(e) => {
                let why = format!("failed: {e}");
                r.failed.push((id, why.clone()));
                self.fail(id, why);
            }
        }
    }

    fn service_instance(&mut self, id: PluginInstanceId, r: &mut IdleReport) {
        let Some(Slot { want, state }) = self.slots.get_mut(&id) else {
            return;
        };
        let State::Live(live) = state else {
            return;
        };
        let ev = live.inst.idle();
        collect_gestures(id, live, &want.params, &mut r.gestures, &mut self.raw);
        let stale = live
            .last_event
            .is_some_and(|t| t.elapsed() >= self.gesture_timeout);
        if !live.open.is_empty() && stale {
            // Its `End` was lost: what it changed so far is one gesture.
            end_open(id, live, &mut self.raw);
            live.open.clear();
            flush_gesture(id, live, &mut r.gestures);
        }
        if ev.params_changed {
            r.params_changed.push(id);
        }
        if ev.state_dirty {
            r.state_dirty.push(id);
        }
        if ev.restart || ev.latency_changed {
            self.shelf.set_reason(id, "restarting");
            // Still on the shelf: restart it here; otherwise the engine
            // must give it back first.
            let shelved = self.shelf.take_unwanted(|k| k != id);
            if shelved.is_empty() {
                r.restart.push(id);
            } else {
                for processor in shelved.into_iter().filter_map(Ready::into_processor) {
                    let _ = self.back_tx.send(Returned { id, processor });
                }
            }
        }
    }

    /// Changes the rate or block size: every instance is activated again
    /// (those in the engine once it gives them back; `service` asks it to).
    /// The app calls it on a sample-rate change; `service` calls it only
    /// for a grown block size.
    pub fn reconfigure(&mut self, sample_rate: f64, max_frames: u32) -> Vec<PluginInstanceId> {
        self.sample_rate = sample_rate;
        self.max_frames = max_frames.max(1);
        // A new configuration ends a refusal (parked instances are
        // activated for it by the next `service`).
        self.refused = None;
        let mut out_in_engine = Vec::new();
        let ids: Vec<PluginInstanceId> = self.slots.keys().copied().collect();
        for id in ids {
            let Some(State::Live(live)) = self.slots.get_mut(&id).map(|s| &mut s.state) else {
                continue;
            };
            if !live.active {
                continue;
            }
            self.shelf.set_reason(id, "restarting");
            let shelved = self.shelf.take_unwanted(|k| k != id);
            if shelved.is_empty() {
                out_in_engine.push(id);
            }
            for processor in shelved.into_iter().filter_map(Ready::into_processor) {
                let _ = self.back_tx.send(Returned { id, processor });
            }
        }
        out_in_engine
    }

    /// The main-thread timer's step with an engine: [`PluginHost::idle`]'s
    /// work, then asks the engine to drop restarting instances, follows a
    /// grown block size, syncs `doc` and loads it when the engine owes a
    /// load and no load is in progress.
    ///
    /// A load the engine refuses (an error, or `Ok` with
    /// `graph.error` set) is reported once (the `Err`, or `compiled`) and
    /// not run again until something changes: `doc` (an edit), `seq`, the
    /// block size, a plugin load finishing or failing, or a restart. What
    /// the report of a failed load held (gestures, loads, failures) is
    /// handed out by the next call.
    pub fn service(
        &mut self,
        engine: &mut Engine,
        doc: &Document,
        seq: SeqId,
    ) -> Result<IdleReport, EngineError> {
        // What a failed load left is handed out below, but is not news:
        // only this call's events end a refusal.
        let carried = self.carry.take();
        let mut r = self.tick();
        if engine.block_frames() > self.max_frames {
            r.restart
                .extend(self.reconfigure(self.sample_rate, engine.block_frames()));
        }
        for id in &r.restart {
            engine.restart_instance(*id);
        }
        let evicted = self.take_evicted();
        for id in &evicted {
            engine.restart_instance(*id);
        }
        if !r.restart.is_empty() || !evicted.is_empty() {
            self.owed_load = true;
        }
        let ready = self.sync(doc);
        // The ends of gestures the sync closed (a slot that went).
        r.param_events.append(&mut self.raw);
        let changed = self.refused.as_ref().is_some_and(|f| {
            !r.loaded.is_empty()
                || !r.failed.is_empty()
                || !r.restart.is_empty()
                || !evicted.is_empty()
                || f.seq != seq
                || f.block != engine.block_frames()
                // Compared only while a load is refused.
                || f.doc != *doc
        });
        if changed {
            self.refused = None;
        }
        if let Some(mut c) = carried {
            c.merge(r);
            r = c;
        }
        if self.refused.is_none() && !self.parked.is_empty() {
            self.unpark(&mut r);
        }
        if ready && self.owed_load && self.refused.is_none() {
            match engine.load(doc, seq) {
                Ok(c) => {
                    if c.graph.error.is_some() {
                        self.refuse(doc, seq, engine);
                    } else {
                        self.owed_load = false;
                    }
                    r.compiled = Some(c);
                }
                Err(e) => {
                    // `Busy` passes: retried by the next call. Anything
                    // else waits for a change.
                    if !matches!(e, EngineError::Busy) {
                        self.refuse(doc, seq, engine);
                    }
                    self.carry = Some(r);
                    return Err(e);
                }
            }
        }
        r.reload = self.owed_load;
        Ok(r)
    }

    fn refuse(&mut self, doc: &Document, seq: SeqId, engine: &Engine) {
        self.refused = Some(Refused {
            doc: doc.clone(),
            seq,
            block: engine.block_frames(),
        });
    }

    /// Activates the instances parked while a load was refused.
    fn unpark(&mut self, r: &mut IdleReport) {
        for id in std::mem::take(&mut self.parked) {
            let Some(instrument) = self
                .slots
                .get(&id)
                .filter(|s| matches!(&s.state, State::Live(l) if !l.active))
                .map(|s| s.want.instrument)
            else {
                continue;
            };
            let Some(State::Live(mut live)) = self
                .slots
                .get_mut(&id)
                .map(|s| std::mem::replace(&mut s.state, State::Failed(String::new())))
            else {
                continue;
            };
            match self.activate(id, &mut live, instrument) {
                Ok(()) => {
                    self.owed_load = true;
                    if let Some(s) = self.slots.get_mut(&id) {
                        s.state = State::Live(live);
                    }
                }
                Err(e) => {
                    let why = format!("failed: {e}");
                    r.failed.push((id, why.clone()));
                    self.fail(id, why);
                }
            }
        }
    }

    /// Calls [`PluginHost::idle`] until no load is in progress (or
    /// `timeout` passes); merges the reports. For tests and command-line
    /// hosts.
    pub fn wait_ready(&mut self, timeout: Duration) -> IdleReport {
        let start = Instant::now();
        let mut all = IdleReport::default();
        loop {
            all.merge(self.idle());
            if self.pending() == 0 && self.finishing.is_empty() {
                return all;
            }
            if start.elapsed() >= timeout {
                return all;
            }
            std::thread::sleep(Duration::from_millis(1));
        }
    }

    /// Stores instance `id`'s state through `History::capture_state` (spec
    /// §4: on save, autosave and before removal; no undo step). The app
    /// calls it before it commits a removal whose plugin state must be
    /// kept: the host cannot capture an instance after `sync` removed it.
    pub fn capture(
        &mut self,
        history: &mut History,
        id: PluginInstanceId,
    ) -> Result<StateBlobId, HostError> {
        let Some(State::Live(live)) = self.slots.get_mut(&id).map(|s| &mut s.state) else {
            return Err(HostError::NotFound(format!("instance {id} is not loaded")));
        };
        let bytes = live.inst.save_state()?;
        let blob = history
            .capture_state(id, &bytes)
            .map_err(|e| HostError::StateRejected(e.to_string()))?;
        live.applied_state = Some(blob);
        // The overlay, and values the document had not caught up with, are
        // now part of the state.
        let sent = std::mem::take(&mut live.sent);
        live.base.extend(sent);
        live.pending.clear();
        if let Some(s) = self.slots.get_mut(&id) {
            s.want.state = history.doc().state_blob(blob).map(|b| (blob, Arc::from(b)));
            s.want.params.clear();
        }
        Ok(blob)
    }

    /// Whether a slot still runs (or is loading) a plugin the user turned
    /// off since (unavailable and [`PluginHost::set_disabled`]): then
    /// [`PluginHost::unload_unavailable`] is due.
    pub fn unloads_due(&self) -> bool {
        self.slots.values().any(|s| self.unloads(s))
    }

    fn unloads(&self, s: &Slot) -> bool {
        let key = (s.want.plugin.format, s.want.plugin.id.clone());
        matches!(s.state, State::Live(_) | State::Loading(_))
            && self.disabled.contains(&key)
            && self.unavailable.contains_key(&key)
    }

    /// Unloads every instance of a plugin the user turned off (spec §9:
    /// turning a plugin off stops it in every open project; a blocklisted
    /// or missing plugin is left running). Each running
    /// one has its state captured first, through
    /// [`PluginHost::capture`] (no undo step), so the document keeps its
    /// settings; then it goes the way of a removed instance (editor
    /// closed, processor given back by the engine before it is dropped)
    /// and its slot stays as a placeholder showing why. A load in progress
    /// is abandoned. The engine owes a load and must drop the unloaded
    /// instances first ([`PluginHost::take_evicted`]);
    /// [`PluginHost::service`] does both. Returns the running instances
    /// unloaded with what capturing gave: an instance whose state could not
    /// be captured is unloaded all the same (its last saved state stays in
    /// the document).
    pub fn unload_unavailable(
        &mut self,
        history: &mut History,
    ) -> Vec<(PluginInstanceId, Result<StateBlobId, HostError>)> {
        let due: Vec<PluginInstanceId> = self
            .slots
            .iter()
            .filter(|(_, s)| self.unloads(s))
            .map(|(id, _)| *id)
            .collect();
        if !due.is_empty() {
            // Placeholders now show why.
            self.owed_load = true;
        }
        let mut out = Vec::new();
        for id in due {
            let Some(slot) = self.slots.get(&id) else {
                continue;
            };
            let live = matches!(slot.state, State::Live(_));
            let key = (slot.want.plugin.format, slot.want.plugin.id.clone());
            let why = self
                .held
                .get(&id)
                .or_else(|| self.unavailable.get(&key))
                .cloned()
                .unwrap_or_default();
            if live {
                out.push((id, self.capture(history, id)));
                self.evicted.insert(id);
            }
            // `capture` updated what the slot wants (the captured state).
            let Some(want) = self.slots.get(&id).map(|s| s.want.clone()) else {
                continue;
            };
            self.remove(id);
            self.slots.insert(
                id,
                Slot {
                    want,
                    state: State::Failed(String::new()),
                },
            );
            self.fail(id, why);
        }
        out
    }

    /// The instances [`PluginHost::unload_unavailable`] unloaded since the
    /// last call, which the engine fed by this host must drop
    /// (`Engine::restart_instance` before its next load; otherwise it keeps
    /// playing them). [`PluginHost::service`] does this itself; a caller
    /// that loads the engine on its own calls it first.
    pub fn take_evicted(&mut self) -> Vec<PluginInstanceId> {
        std::mem::take(&mut self.evicted).into_iter().collect()
    }

    /// Captures every live instance with state (before saving).
    pub fn capture_all(&mut self, history: &mut History) -> Vec<(PluginInstanceId, HostError)> {
        let ids: Vec<PluginInstanceId> = self.slots.keys().copied().collect();
        ids.into_iter()
            .filter_map(|id| match self.capture(history, id) {
                Ok(_) => None,
                Err(HostError::NotFound(_)) => None,
                Err(e) => Some((id, e)),
            })
            .collect()
    }

    /// Commits gestures as undoable `SetPluginParams` commands. Call
    /// [`PluginHost::sync`] afterwards (before any undo), so the plugin's
    /// values count as committed.
    pub fn commit_gestures(history: &mut History, gestures: &[Gesture]) -> core_model::Result<()> {
        for g in gestures {
            history.execute(
                &Command::Mixer(MixerCmd::SetPluginParams {
                    slot: g.slot,
                    params: g.values.clone(),
                }),
                &ViewFilter::default(),
            )?;
        }
        Ok(())
    }
}

/// Sends `value` to a running instance (or sets it while inactive); false
/// when it did not fit the queue (the caller sends it again later).
fn push(l: &mut Live, param: ParamId, value: f64) -> bool {
    if l.active {
        l.params.send(param, value)
    } else {
        l.inst.set_params(&[(param, value)]).is_ok()
    }
}

/// The address of a processor, which identifies it while it is out.
fn address(p: &dyn AudioProcessor) -> usize {
    std::ptr::from_ref(p).cast::<()>().addr()
}

/// Groups the plugin's parameter events into gestures (Plan Ruling 16):
/// from the first `Begin` to the `End` that closes the last open
/// parameter; a value while nothing is open is a one-value gesture. A
/// second `Begin` of an open parameter means its `End` was lost: the
/// gesture so far is closed first. An `End` of a parameter that is not
/// open only closes when nothing else is open. Every value counts as sent
/// and is pending until the document has it. Every event read is also
/// appended to `raw`, in order (plan 5e-1 Ruling 15).
fn collect_gestures(
    id: PluginInstanceId,
    l: &mut Live,
    doc: &BTreeMap<ParamId, f64>,
    out: &mut Vec<Gesture>,
    raw: &mut Vec<(PluginInstanceId, ParamEvent)>,
) {
    let mut events = l.gestures.drain();
    events.extend(l.inst.gestures());
    if !events.is_empty() {
        l.last_event = Some(Instant::now());
    }
    raw.extend(events.iter().map(|e| (id, *e)));
    for e in events {
        match e {
            ParamEvent::Begin(p) => {
                if !l.open.insert(p) {
                    flush_gesture(id, l, out);
                }
            }
            ParamEvent::Value(p, v) => {
                l.sent.insert(p, v);
                l.pending.insert(
                    p,
                    Pending {
                        value: v,
                        doc: doc.get(&p).copied(),
                    },
                );
                match l.gesture.iter_mut().find(|(k, _)| *k == p) {
                    Some(x) => x.1 = v,
                    None => l.gesture.push((p, v)),
                }
                if l.open.is_empty() {
                    flush_gesture(id, l, out);
                }
            }
            ParamEvent::End(p) => {
                l.open.remove(&p);
                if l.open.is_empty() {
                    flush_gesture(id, l, out);
                }
            }
        }
    }
}

/// Reports an `End` for every parameter of `l`'s open gesture: called
/// wherever the host closes a gesture itself, before `open` is cleared or
/// the instance goes (pre-flight I7, plan 5e-1 Ruling 15), so whoever
/// follows the raw events releases its touches.
fn end_open(id: PluginInstanceId, l: &Live, raw: &mut Vec<(PluginInstanceId, ParamEvent)>) {
    raw.extend(l.open.iter().map(|p| (id, ParamEvent::End(*p))));
}

/// Reports the gesture so far, if it has values.
fn flush_gesture(id: PluginInstanceId, l: &mut Live, out: &mut Vec<Gesture>) {
    if !l.gesture.is_empty() {
        out.push(Gesture {
            slot: id,
            values: std::mem::take(&mut l.gesture),
        });
    }
}

/// Brings a running instance in line with what the document wants: a new
/// state blob is loaded (an undo past a capture; no blob restores the
/// values it was loaded with), then overlay values that changed are sent
/// and removed ones go back to their base values. Values the plugin holds
/// that the document has not caught up with are left alone (Plan Ruling
/// 16) until it has them or a newer edit of that parameter replaces them.
/// What did not fit the queue is sent again by the next call (Plan Ruling
/// 15).
fn apply_want(l: &mut Live, want: &Want) {
    let want_state = want.state.as_ref().map(|(id, _)| *id);
    if want_state != l.applied_state {
        match &want.state {
            Some((sid, bytes)) => {
                if l.inst.load_state(bytes).is_ok() {
                    l.applied_state = Some(*sid);
                    l.base = current_values(l.inst.as_mut());
                    l.sent.clear();
                    l.pending.clear();
                }
            }
            None => {
                if let Some(defaults) = l.defaults.clone() {
                    let changed: Vec<(ParamId, f64)> = defaults
                        .iter()
                        .filter(|(p, v)| l.base.get(p) != Some(v))
                        .map(|(p, v)| (*p, *v))
                        .collect();
                    let mut all = true;
                    for (p, v) in changed {
                        all &= push(l, p, v);
                    }
                    if all {
                        l.applied_state = None;
                        l.base = defaults;
                        l.pending.clear();
                    }
                }
            }
        }
    }
    l.pending.retain(|p, pend| {
        let doc = want.params.get(p).copied();
        doc == pend.doc && doc != Some(pend.value)
    });
    let removed: Vec<ParamId> = l
        .sent
        .keys()
        .filter(|k| !want.params.contains_key(k) && !l.pending.contains_key(k))
        .copied()
        .collect();
    for p in removed {
        let done = match l.base.get(&p).copied() {
            Some(v) => push(l, p, v),
            None => true,
        };
        if done {
            l.sent.remove(&p);
        }
    }
    for (p, v) in &want.params {
        if !l.pending.contains_key(p) && l.sent.get(p) != Some(v) && push(l, *p, *v) {
            l.sent.insert(*p, *v);
        }
    }
}

fn current_values(inst: &mut dyn PluginInstance) -> BTreeMap<ParamId, f64> {
    let ids: Vec<ParamId> = inst.info().params.iter().map(|p| p.id).collect();
    ids.into_iter()
        .filter_map(|id| inst.param_value(id).map(|v| (id, v)))
        .collect()
}

impl Drop for PluginHost {
    fn drop(&mut self) {
        // Shelved processors and those the engine gave back are deactivated
        // with their instances; the engine must already be gone.
        let mut back: Vec<Returned> = self
            .shelf
            .take_unwanted(|_| false)
            .into_iter()
            .filter_map(|r| {
                let id = r.id();
                r.into_processor()
                    .map(|processor| Returned { id, processor })
            })
            .collect();
        back.extend(self.back_rx.try_iter());
        for Returned { id, processor } in back {
            if let Some(mut r) = self.take_retired(id, address(processor.as_ref())) {
                r.live.inst.deactivate(processor);
            } else if let Some(State::Live(l)) = self.slots.get_mut(&id).map(|s| &mut s.state) {
                l.inst.deactivate(processor);
                l.active = false;
            }
        }
        self.slots.clear();
        self.jobs = None;
        if let Some(h) = self.loader.take() {
            let _ = h.join();
        }
    }
}
