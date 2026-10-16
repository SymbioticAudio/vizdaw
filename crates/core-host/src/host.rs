//! The plugin host (spec §5, §6): keeps one instance per plugin slot of the
//! document, loads new ones (format work off the main thread on the loader
//! thread, the rest on the main thread, Plan Ruling 2), activates them and
//! shelves their processors for the engine, feeds the parameter overlay to
//! them, turns UI gestures into undoable commands, captures state, and
//! restarts plugins that ask for it. Main thread only (it is not `Send`).

use crate::adapt::{PluginEffect, PluginSynth, Returned};
use crate::clap::ClapFormat;
use crate::error::HostError;
use crate::instance::{LoadRequest, PluginFormat, PluginInstance, Prepared};
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
use std::collections::{BTreeMap, VecDeque};
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
    /// Saved states a plugin rejected on load (it plays with its defaults).
    pub rejected_state: Vec<(PluginInstanceId, String)>,
    /// Processors were shelved: the engine should load again.
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
        self.rejected_state.extend(o.rejected_state);
        self.reload |= o.reload;
        if o.compiled.is_some() {
            self.compiled = o.compiled;
        }
    }
}

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

/// A live instance and its main-thread ends.
struct Live {
    inst: Box<dyn PluginInstance>,
    params: ParamSender,
    gestures: GestureReceiver,
    /// The processor is out (on the shelf or in the engine).
    active: bool,
    /// The state blob last loaded into it.
    applied_state: Option<StateBlobId>,
    /// Parameter values before the overlay (to restore when an overlay
    /// entry goes away, e.g. on undo).
    base: BTreeMap<ParamId, f64>,
    /// Overlay values already sent.
    sent: BTreeMap<ParamId, f64>,
    /// Values of the gesture in progress.
    gesture: Vec<(ParamId, f64)>,
    open: usize,
    /// Must be activated again when its processor comes back.
    reactivate: bool,
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

/// A job for the loader thread.
type Job = (PluginInstanceId, u64, crate::instance::PrepareJob);
type Done = (PluginInstanceId, u64, Result<Prepared, HostError>);

/// The plugin host. Create it on the main thread; drop the engine before
/// it (processors in the engine come back through it).
pub struct PluginHost {
    formats: Vec<Box<dyn PluginFormat>>,
    paths: BTreeMap<(Format, String), PathBuf>,
    unavailable: BTreeMap<(Format, String), String>,
    sample_rate: f64,
    max_frames: u32,
    slots: BTreeMap<PluginInstanceId, Slot>,
    /// Instances removed from the document whose processor is still out.
    retiring: BTreeMap<PluginInstanceId, Box<dyn PluginInstance>>,
    /// Prepared loads waiting for their main-thread step.
    finishing: VecDeque<(PluginInstanceId, Prepared)>,
    generation: u64,
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
            sample_rate,
            max_frames: max_frames.max(1),
            slots: BTreeMap::new(),
            retiring: BTreeMap::new(),
            finishing: VecDeque::new(),
            generation: 0,
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

    /// Marks a plugin as unavailable (disabled or blocklisted); its slots
    /// become placeholders showing `why` (spec §9). `None` makes it
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
        self.retiring.len()
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

    /// Sets a parameter now (a mixer control): sent to the running plugin;
    /// the app also records it in the overlay with `SetPluginParams`.
    pub fn set_param(&mut self, id: PluginInstanceId, param: ParamId, value: f64) {
        if let Some(State::Live(l)) = self.slots.get_mut(&id).map(|s| &mut s.state) {
            send(l, param, value);
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
                    self.slots.insert(
                        id,
                        Slot {
                            want,
                            state: State::Failed(String::new()),
                        },
                    );
                    match unavailable {
                        Some(why) => self.fail(id, why),
                        None => self.start_load(id),
                    }
                }
            }
        }
        self.pending() == 0
    }

    fn fail(&mut self, id: PluginInstanceId, why: String) {
        self.shelf.set_reason(id, why.clone());
        if let Some(s) = self.slots.get_mut(&id) {
            s.state = State::Failed(why);
        }
    }

    fn start_load(&mut self, id: PluginInstanceId) {
        let Some(want) = self.slots.get(&id).map(|s| s.want.clone()) else {
            return;
        };
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
        let Some(s) = self.slots.remove(&id) else {
            return;
        };
        if let State::Live(mut l) = s.state {
            let shelved = self.shelf.take_unwanted(|k| k != id);
            let mut back = false;
            for p in shelved.into_iter().filter_map(Ready::into_processor) {
                back = true;
                l.inst.deactivate(p);
            }
            if l.active && !back {
                self.retiring.insert(id, l.inst);
            }
        }
    }

    /// Main-thread servicing (`host_idle`, 60 Hz; spec §5): finishes one
    /// load, deactivates processors the engine gave back, runs plugin
    /// callbacks, and collects restarts and gestures.
    pub fn idle(&mut self) -> IdleReport {
        let mut r = IdleReport::default();
        while let Ok((id, generation, result)) = self.done.try_recv() {
            let current = matches!(
                self.slots.get(&id).map(|s| &s.state),
                Some(State::Loading(g)) if *g == generation
            );
            if !current {
                continue;
            }
            match result {
                Ok(p) => self.finishing.push_back((id, p)),
                Err(e) => {
                    let why = format!("failed: {e}");
                    r.failed.push((id, why.clone()));
                    self.fail(id, why);
                }
            }
        }
        if let Some((id, prepared)) = self.finishing.pop_front() {
            self.finish(id, prepared, &mut r);
        }
        while let Ok(back) = self.back_rx.try_recv() {
            self.returned(back, &mut r);
        }
        let ids: Vec<PluginInstanceId> = self.slots.keys().copied().collect();
        for id in ids {
            self.service_instance(id, &mut r);
        }
        r
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
            applied_state: None,
            base: BTreeMap::new(),
            sent: BTreeMap::new(),
            gesture: Vec::new(),
            open: 0,
            reactivate: false,
        });
        // State first, then the overlay (spec §4).
        if let Some((sid, bytes)) = &want.state {
            match live.inst.load_state(bytes) {
                Ok(()) => live.applied_state = Some(*sid),
                Err(e) => r.rejected_state.push((id, e.to_string())),
            }
        }
        live.base = current_values(live.inst.as_mut());
        let overlay: Vec<(ParamId, f64)> = want.params.iter().map(|(k, v)| (*k, *v)).collect();
        if live.inst.set_params(&overlay).is_ok() {
            live.sent = want.params.clone();
        }
        match self.activate(id, &mut live, want.instrument) {
            Ok(()) => {
                r.loaded.push(id);
                r.reload = true;
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
        live.reactivate = false;
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
        if let Some(mut inst) = self.retiring.remove(&id) {
            inst.deactivate(processor);
            return;
        }
        let instrument = self.slots.get(&id).is_some_and(|s| s.want.instrument);
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
        // A processor comes back only when the engine dropped it: a
        // restart or a new configuration. Activate it again.
        match self.activate(id, &mut live, instrument) {
            Ok(()) => {
                r.reload = true;
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
        let Some(State::Live(live)) = self.slots.get_mut(&id).map(|s| &mut s.state) else {
            return;
        };
        let ev = live.inst.idle();
        for e in live.gestures.drain() {
            match e {
                ParamEvent::Begin(_) => live.open += 1,
                ParamEvent::Value(p, v) => {
                    live.sent.insert(p, v);
                    match live.gesture.iter_mut().find(|(k, _)| *k == p) {
                        Some(x) => x.1 = v,
                        None => live.gesture.push((p, v)),
                    }
                    if live.open == 0 {
                        r.gestures.push(Gesture {
                            slot: id,
                            values: std::mem::take(&mut live.gesture),
                        });
                    }
                }
                ParamEvent::End(_) => {
                    live.open = live.open.saturating_sub(1);
                    if live.open == 0 && !live.gesture.is_empty() {
                        r.gestures.push(Gesture {
                            slot: id,
                            values: std::mem::take(&mut live.gesture),
                        });
                    }
                }
            }
        }
        if ev.restart || ev.latency_changed {
            live.reactivate = true;
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
    pub fn reconfigure(&mut self, sample_rate: f64, max_frames: u32) -> Vec<PluginInstanceId> {
        self.sample_rate = sample_rate;
        self.max_frames = max_frames.max(1);
        let mut out_in_engine = Vec::new();
        let ids: Vec<PluginInstanceId> = self.slots.keys().copied().collect();
        for id in ids {
            let Some(State::Live(live)) = self.slots.get_mut(&id).map(|s| &mut s.state) else {
                continue;
            };
            if !live.active {
                continue;
            }
            live.reactivate = true;
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

    /// The main-thread timer's step with an engine: [`PluginHost::idle`],
    /// then asks the engine to drop restarting instances, follows a grown
    /// block size, syncs `doc` and loads it again when processors were
    /// shelved or restarts are due and no load is in progress.
    pub fn service(
        &mut self,
        engine: &mut Engine,
        doc: &Document,
        seq: SeqId,
    ) -> Result<IdleReport, EngineError> {
        let mut r = self.idle();
        if engine.block_frames() > self.max_frames {
            r.restart
                .extend(self.reconfigure(self.sample_rate, engine.block_frames()));
        }
        for id in &r.restart {
            engine.restart_instance(*id);
        }
        let ready = self.sync(doc);
        if ready && (r.reload || !r.restart.is_empty()) {
            r.compiled = Some(engine.load(doc, seq)?);
        }
        Ok(r)
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
    /// §4: on save, autosave and before removal; no undo step).
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
        // The overlay is now part of the state.
        let sent = std::mem::take(&mut live.sent);
        live.base.extend(sent);
        if let Some(s) = self.slots.get_mut(&id) {
            s.want.state = history.doc().state_blob(blob).map(|b| (blob, Arc::from(b)));
            s.want.params.clear();
        }
        Ok(blob)
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

    /// Commits gestures as undoable `SetPluginParams` commands.
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

/// Sends `value` to a running instance (or sets it while inactive) and
/// remembers it; a value that did not fit the queue is not remembered, so
/// the next `sync` sends it again.
fn send(l: &mut Live, param: ParamId, value: f64) {
    let ok = if l.active {
        l.params.send(param, value)
    } else {
        l.inst.set_params(&[(param, value)]).is_ok()
    };
    if ok {
        l.sent.insert(param, value);
    }
}

/// Brings a running instance in line with what the document wants: a new
/// state blob is loaded (an undo past a capture), then overlay values that
/// changed are sent and removed ones go back to their base values.
fn apply_want(l: &mut Live, want: &Want) {
    let want_state = want.state.as_ref().map(|(id, _)| *id);
    if want_state != l.applied_state {
        if let Some((sid, bytes)) = &want.state {
            if l.inst.load_state(bytes).is_ok() {
                l.applied_state = Some(*sid);
                l.base = current_values(l.inst.as_mut());
                l.sent.clear();
            }
        }
    }
    let removed: Vec<ParamId> = l
        .sent
        .keys()
        .filter(|k| !want.params.contains_key(k))
        .copied()
        .collect();
    for p in removed {
        if let Some(v) = l.base.get(&p).copied() {
            send(l, p, v);
        }
        l.sent.remove(&p);
    }
    for (p, v) in &want.params {
        if l.sent.get(p) != Some(v) {
            send(l, *p, *v);
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
            if let Some(mut inst) = self.retiring.remove(&id) {
                inst.deactivate(processor);
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
