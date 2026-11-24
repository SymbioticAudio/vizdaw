//! A VST3 plug-in instance on the main thread (host track H1 Task 3): its
//! component and controller ([`Plugin`]), the component handler that
//! queues what the plug-in reports, its [`PluginInfo`] from the buses and
//! the controller's parameters, and parameter values in plain units.
//!
//! The core host works in plain values (`ParamInfo::min..max`); VST3
//! controllers and processors work in normalised `[0, 1]` values. The
//! instance converts with the controller (`normalizedParamToPlain`,
//! `plainParamToNormalized`), always on the main thread: a plug-in's edits
//! arrive normalised through the component handler, from any thread, and
//! are converted only when [`PluginInstance::gestures`] reads them.
//!
//! Activation (host track H1 Task 4) follows the SDK's order: the
//! plug-in's own bus arrangements confirmed, `setupProcessing` (in the
//! stored render mode, at the host's rate and block size), every audio
//! bus and the first event input turned on, then `setActive(true)`; the
//! [`Vst3Processor`] calls `setProcessing` on the audio thread.
//! Deactivation undoes it in reverse. Values reach the component as
//! follows:
//! - set while inactive ([`PluginInstance::set_params`]) or edited by the
//!   controller while no processor is out: with the next processor's
//!   first block;
//! - edited by the controller while a processor is out: with the
//!   processor's next block (an edit is routed by where the component
//!   will next read, when it is drained; one the processor never took
//!   goes to the next first block);
//! - host values while active: through the processor's context, as
//!   plain values it converts.

use super::ffi::{ComponentHandler, EDIT_QUEUE, Edit, Module, Plugin};
use super::process_ffi::RtData;
use super::processor::{ParamMap, ProcessorSetup, Vst3Processor};
use super::{class_id_hex, module, parse_class_id, state};
use crate::error::HostError;
use crate::info::{AudioPortInfo, ParamInfo, PluginInfo, PluginKind, PortRole, Tail};
use crate::instance::{
    AudioProcessor, InstanceEvents, LoadRequest, OfflineRender, PluginEditor, PluginFormat,
    PluginInstance, PrepareJob, Prepared,
};
use crate::process::ParamEvent;
use core_model::mixer::{ParamId, PluginFormat as Format};
use crossbeam_queue::ArrayQueue;
use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::rc::{Rc, Weak};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use vst3::ComWrapper;

/// `restartComponent` flags (the SDK's `RestartFlags`).
mod restart {
    pub const RELOAD_COMPONENT: i32 = 1;
    pub const IO_CHANGED: i32 = 2;
    pub const PARAM_VALUES_CHANGED: i32 = 4;
    pub const LATENCY_CHANGED: i32 = 8;
    pub const PARAM_TITLES_CHANGED: i32 = 16;
    pub const IO_TITLES_CHANGED: i32 = 128;
    pub const PARAM_ID_MAPPING_CHANGED: i32 = 2048;
}

/// The rate and block size `setupProcessing` is given before the first
/// activation says otherwise (the scanner's check uses the same).
const DEFAULT_SETUP: (f64, u32) = (48_000.0, 512);

/// Most values the processor hands back to the controller between two
/// main-thread drains (more are dropped: the controller shows a value
/// late, nothing else is lost).
const ECHO_QUEUE: usize = 4096;

/// `getTailSamples`' "infinite" (the SDK's `kInfiniteTail`).
const INFINITE_TAIL: u32 = u32::MAX;

/// The main thread's ends of an active processor.
#[derive(Debug)]
struct Active {
    /// Controller edits for the processor (normalised).
    edits: Arc<ArrayQueue<(u32, f64)>>,
    /// The edit a dropped processor had popped but not sent.
    returned: Arc<ArrayQueue<(u32, f64)>>,
    /// Values for the controller from the processor (normalised).
    echoes: Arc<ArrayQueue<(u32, f64)>>,
    /// Cleared when the processor is dropped (by `deactivate` or not).
    alive: Arc<AtomicBool>,
    /// Set by a state load (the processor drops what it has from before).
    stale: Arc<AtomicBool>,
}

/// A loaded VST3 plug-in (main-thread half). Not `Send`: the VST3 SDK
/// ties a component and controller to the thread that made them.
pub struct Vst3Instance {
    /// Declared first: dropped (disconnected, terminated, released)
    /// before the module it came from.
    plugin: Plugin,
    handler: ComWrapper<ComponentHandler>,
    /// The plug-in's class ID (its state blobs carry it).
    class_id: [u8; 16],
    info: PluginInfo,
    /// Set while the component is active (`setActive(true)` and a
    /// processor handed out).
    active: Option<Active>,
    /// Render offline (`kOffline`) at the next `setupProcessing`.
    offline: bool,
    /// The rate and block size of the latest `setupProcessing`.
    setup: (f64, u32),
    /// Normalised values the controller has and the component has not
    /// been sent yet (set while inactive; the processor sends them with
    /// its first block), by `ParamID`.
    pending: BTreeMap<u32, f64>,
    /// Gesture begins and ends read (and their values dropped) by a state
    /// load, for the next [`PluginInstance::gestures`].
    kept_gestures: Vec<Edit>,
    /// Declared last: the module outlives every object made from it.
    _module: Rc<Module>,
}

impl std::fmt::Debug for Vst3Instance {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Vst3Instance")
            .field("plugin", &self.info.plugin.id)
            .finish_non_exhaustive()
    }
}

/// A VST3 `ParamID` from the host's ID (they are 32-bit).
fn vst_id(id: ParamId) -> Option<u32> {
    u32::try_from(id.0).ok()
}

impl Vst3Instance {
    /// Loads the bundle at `path` and creates plug-in `id` (a class ID)
    /// from it (main thread). The instance keeps the module loaded.
    pub fn load(path: &Path, id: &str) -> Result<Vst3Instance, HostError> {
        let module = Rc::new(Module::load(path)?);
        Vst3Instance::with_module(module, id)
    }

    /// Creates plug-in `id` from a loaded module.
    fn with_module(module: Rc<Module>, id: &str) -> Result<Vst3Instance, HostError> {
        let cid = parse_class_id(id).ok_or_else(|| HostError::NotFound(id.to_string()))?;
        let hex = class_id_hex(&cid);
        let (plugin_ref, kind) = module::plugins(&module.classes(), &module.vendor())
            .into_iter()
            .find(|(p, _)| p.id == hex)
            .ok_or_else(|| HostError::NotFound(format!("{id} in the bundle")))?;
        let handler = ComWrapper::new(ComponentHandler::new());
        let plugin = Plugin::create(&module, &cid, &handler)?;
        let mut me = Vst3Instance {
            plugin,
            handler,
            class_id: cid,
            info: PluginInfo {
                plugin: plugin_ref,
                kind,
                inputs: Vec::new(),
                outputs: Vec::new(),
                note_input: false,
                params: Vec::new(),
                latency: 0,
                tail: Tail::None,
            },
            active: None,
            offline: false,
            setup: DEFAULT_SETUP,
            pending: BTreeMap::new(),
            kept_gestures: Vec::new(),
            _module: module,
        };
        me.read_info();
        Ok(me)
    }

    /// Reads buses and parameters (main thread).
    fn read_info(&mut self) {
        let ports = |input: bool| -> Vec<AudioPortInfo> {
            let mut main_taken = false;
            let mut sidechain_taken = false;
            self.plugin
                .audio_buses(input)
                .into_iter()
                .map(|b| {
                    let role = if !b.aux && !main_taken {
                        main_taken = true;
                        PortRole::Main
                    } else if input && b.aux && !sidechain_taken {
                        sidechain_taken = true;
                        PortRole::Sidechain
                    } else {
                        PortRole::Aux
                    };
                    AudioPortInfo {
                        name: b.name,
                        channels: u16::try_from(b.channels.max(0)).unwrap_or(u16::MAX),
                        role,
                    }
                })
                .collect()
        };
        let inputs = ports(true);
        let outputs = ports(false);
        let note_input = !self.plugin.event_inputs().is_empty();
        let params = self
            .plugin
            .params()
            .into_iter()
            .map(|p| ParamInfo {
                id: ParamId(u64::from(p.id)),
                name: p.title,
                module: String::new(),
                min: self.plugin.to_plain(p.id, 0.0),
                max: self.plugin.to_plain(p.id, 1.0),
                default: self.plugin.to_plain(p.id, p.default_normalized),
                stepped: p.steps > 0,
                automatable: p.can_automate,
                hidden: p.hidden,
                read_only: p.read_only,
            })
            .collect();
        if note_input && inputs.is_empty() {
            self.info.kind = PluginKind::Instrument;
        }
        self.info.inputs = inputs;
        self.info.outputs = outputs;
        self.info.note_input = note_input;
        self.info.params = params;
    }

    /// Normalised values set while inactive that the processor has not
    /// been sent yet, by `ParamID` (taken: the processor sends them with
    /// its first block).
    pub(crate) fn take_pending(&mut self) -> Vec<(u32, f64)> {
        std::mem::take(&mut self.pending).into_iter().collect()
    }

    /// Hands the processor's values to the controller (main thread).
    fn drain_echoes(&mut self) {
        let Some(a) = &self.active else {
            return;
        };
        while let Some((id, n)) = a.echoes.pop() {
            self.plugin.set_normalized(id, n);
        }
    }

    /// Ends the active state once its processor is gone: the controller
    /// gets the processor's last values, edits the processor never took
    /// wait for the next first block, and the component is deactivated.
    fn finish_active(&mut self) {
        self.drain_echoes();
        let Some(a) = self.active.take() else {
            return;
        };
        // What the processor never took, oldest first (later entries win)…
        let mut untaken = BTreeMap::new();
        while let Some((id, n)) = a.returned.pop().or_else(|| a.edits.pop()) {
            untaken.insert(id, n);
        }
        // …under the spills already in `pending`, which are newer than
        // anything queued (a spill happens only when the queue is full,
        // and later edits wait behind it).
        for (id, n) in untaken {
            self.pending.entry(id).or_insert(n);
        }
        self.plugin.set_active(false);
        self.plugin.activate_buses(false);
    }

    /// Finishes the active state of a processor dropped without
    /// [`PluginInstance::deactivate`].
    fn reclaim(&mut self) {
        if self
            .active
            .as_ref()
            .is_some_and(|a| !a.alive.load(Ordering::Acquire))
        {
            self.finish_active();
        }
    }

    /// Reads latency and tail (after `setActive(true)`).
    fn read_timing(&mut self) {
        self.info.latency = self.plugin.latency();
        self.info.tail = match self.plugin.tail() {
            0 => Tail::None,
            INFINITE_TAIL => Tail::Infinite,
            n => Tail::Frames(n),
        };
    }

    /// Edits the component handler dropped because its queue was full
    /// (the plug-in reported more than it holds between two reads).
    pub fn dropped_edits(&self) -> u64 {
        self.handler.dropped()
    }
}

impl PluginInstance for Vst3Instance {
    fn info(&self) -> &PluginInfo {
        &self.info
    }

    fn activate(
        &mut self,
        sample_rate: f64,
        max_frames: u32,
    ) -> Result<Box<dyn AudioProcessor>, HostError> {
        self.reclaim();
        if self.active.is_some() {
            return Err(HostError::ActivateFailed("already active".into()));
        }
        let max_frames = max_frames.max(1);
        let failed = |why: &str| Err(HostError::ActivateFailed(why.into()));
        let Some(processor) = self.plugin.audio_processor() else {
            return failed("the plug-in has no audio processor");
        };
        // The SDK's order: arrangements, then the processing setup.
        self.plugin.keep_arrangements();
        // The stored render mode (`set_offline`) at the host's setup.
        if self
            .plugin
            .setup_processing(self.offline, sample_rate, max_frames)
            != Some(true)
        {
            return failed("the plug-in refused the processing setup");
        }
        self.setup = (sample_rate, max_frames);
        if !self.plugin.can_process_f32() {
            return failed("the plug-in cannot process 32-bit samples");
        }
        // Buses may have changed while inactive.
        self.read_info();
        self.plugin.activate_buses(true);
        if !self.plugin.set_active(true) {
            self.plugin.activate_buses(false);
            return failed("the plug-in did not activate");
        }
        self.read_timing();
        let maps = self
            .plugin
            .params()
            .into_iter()
            .map(|p| {
                ParamMap::sample(p.id, p.steps, !p.read_only, |n| {
                    self.plugin.to_plain(p.id, n)
                })
            })
            .collect();
        let midi = if self.info.note_input {
            self.plugin.midi_mapping()
        } else {
            Vec::new()
        };
        let in_counts: Vec<u16> = self.info.inputs.iter().map(|p| p.channels).collect();
        let out_counts: Vec<u16> = self.info.outputs.iter().map(|p| p.channels).collect();
        let first = self.take_pending();
        let active = Active {
            edits: Arc::new(ArrayQueue::new(EDIT_QUEUE)),
            // Room for the first block's values and the held edit, which
            // a processor dropped early hands back.
            returned: Arc::new(ArrayQueue::new(1 + first.len())),
            echoes: Arc::new(ArrayQueue::new(ECHO_QUEUE)),
            alive: Arc::new(AtomicBool::new(true)),
            stale: Arc::new(AtomicBool::new(false)),
        };
        let setup = ProcessorSetup {
            data: RtData::new(processor, &in_counts, &out_counts, first.len()),
            sample_rate,
            max_frames,
            offline: self.offline,
            maps,
            midi,
            first,
            edits: active.edits.clone(),
            returned: active.returned.clone(),
            echoes: active.echoes.clone(),
            alive: active.alive.clone(),
            stale: active.stale.clone(),
        };
        let proc = Vst3Processor::new(setup, &self.info);
        self.active = Some(active);
        Ok(Box::new(proc))
    }

    fn deactivate(&mut self, processor: Box<dyn AudioProcessor>) {
        // Dropping the processor stops processing; then the component is
        // deactivated here.
        drop(processor);
        self.finish_active();
    }

    fn is_active(&self) -> bool {
        self.active.is_some()
    }

    /// Both streams in the `.vstpreset` layout ([`state`]); active or
    /// not, as the CLAP host saves.
    fn save_state(&mut self) -> Result<Vec<u8>, HostError> {
        self.reclaim();
        state::save(&self.plugin, &self.class_id)
    }

    /// Loads a blob from [`PluginInstance::save_state`] (or a
    /// `.vstpreset` of this class); active or not, as the CLAP host
    /// loads. Values set or edited before a load that the component (or
    /// the controller) has not been given are dropped: they would land on
    /// the loaded state. That is `pending`, the processor's queues, what
    /// the processor itself still has (its unsent first block and held
    /// edit, dropped at its next block) and controller edits not read
    /// yet, whose gestures still reach the host, without their values.
    fn load_state(&mut self, data: &[u8]) -> Result<(), HostError> {
        self.reclaim();
        state::load(&self.plugin, &self.class_id, data)?;
        self.pending.clear();
        let unread = self.handler.take_edits();
        self.kept_gestures.extend(
            unread
                .into_iter()
                .filter(|e| !matches!(e, Edit::Perform(..))),
        );
        if let Some(a) = &self.active {
            a.stale.store(true, Ordering::Release);
            while a.edits.pop().is_some() {}
            while a.returned.pop().is_some() {}
            while a.echoes.pop().is_some() {}
        }
        Ok(())
    }

    fn set_params(&mut self, values: &[(ParamId, f64)]) -> Result<(), HostError> {
        self.reclaim();
        if self.active.is_some() {
            return Err(HostError::StateRejected(
                "parameters of an active plugin go through its processor".into(),
            ));
        }
        for (id, plain) in values {
            // Only parameters the plug-in lists, and not the ones it only
            // reports.
            if !self
                .info
                .param(*id)
                .is_some_and(|p| !p.read_only && plain.is_finite())
            {
                continue;
            }
            let Some(vid) = vst_id(*id) else {
                continue;
            };
            // A controller may map a value to NaN; that is never sent.
            let Some(n) = Some(self.plugin.to_normalized(vid, *plain))
                .filter(|n| n.is_finite())
                .map(|n| n.clamp(0.0, 1.0))
            else {
                continue;
            };
            if self.plugin.set_normalized(vid, n) {
                self.pending.insert(vid, n);
            }
        }
        Ok(())
    }

    fn param_value(&mut self, id: ParamId) -> Option<f64> {
        self.info.param(id)?;
        let vid = vst_id(id)?;
        let n = self.plugin.normalized(vid)?;
        Some(self.plugin.to_plain(vid, n))
    }

    fn editor(&mut self) -> Option<&mut dyn PluginEditor> {
        None
    }

    fn idle(&mut self) -> InstanceEvents {
        self.reclaim();
        self.drain_echoes();
        let flags = self.handler.take_restart();
        let has = |f: i32| flags & f != 0;
        let io = has(restart::IO_CHANGED);
        let reread = io
            || has(restart::PARAM_TITLES_CHANGED)
            || has(restart::PARAM_ID_MAPPING_CHANGED)
            || has(restart::IO_TITLES_CHANGED);
        if reread && self.active.is_none() {
            self.read_info();
        }
        InstanceEvents {
            restart: has(restart::RELOAD_COMPONENT) || io,
            latency_changed: has(restart::LATENCY_CHANGED),
            params_changed: io
                || has(restart::PARAM_VALUES_CHANGED)
                || has(restart::PARAM_TITLES_CHANGED)
                || has(restart::PARAM_ID_MAPPING_CHANGED),
            state_dirty: self.handler.take_dirty(),
            editor_resize: None,
        }
    }

    fn gestures(&mut self) -> Vec<ParamEvent> {
        self.reclaim();
        // While a processor runs, `pending` holds only edits its full queue
        // did not take: they go first, before newer edits.
        if let Some(a) = &self.active {
            while let Some((&id, &n)) = self.pending.first_key_value() {
                if a.edits.push((id, n)).is_err() {
                    break;
                }
                self.pending.remove(&id);
            }
        }
        let mut edits = std::mem::take(&mut self.kept_gestures);
        edits.extend(self.handler.take_edits());
        let mut out = Vec::with_capacity(edits.len());
        for e in edits {
            out.push(match e {
                Edit::Begin(id) => ParamEvent::Begin(ParamId(u64::from(id))),
                Edit::End(id) => ParamEvent::End(ParamId(u64::from(id))),
                // A controller's NaN or infinity is never sent anywhere.
                Edit::Perform(_, n) if !n.is_finite() => continue,
                Edit::Perform(id, n) => {
                    // The component gets the edit where it reads next: the
                    // running processor's next block, else the next first
                    // block. One the full queue refused waits in `pending`
                    // (retried above), and so do the edits after it while
                    // any waits, so no older value lands after a newer one.
                    let to_processor = self
                        .active
                        .as_ref()
                        .is_some_and(|a| self.pending.is_empty() && a.edits.push((id, n)).is_ok());
                    if !to_processor {
                        self.pending.insert(id, n);
                    }
                    ParamEvent::Value(ParamId(u64::from(id)), self.plugin.to_plain(id, n))
                }
            });
        }
        out
    }

    fn set_offline(&mut self, on: bool) -> OfflineRender {
        self.reclaim();
        if self.active.is_some() {
            // `setupProcessing` needs an inactive component.
            return OfflineRender::Refused;
        }
        let (rate, frames) = self.setup;
        match self.plugin.setup_processing(on, rate, frames) {
            Some(true) => {
                self.offline = on;
                OfflineRender::Taken
            }
            // Refused: the plug-in keeps the mode it last took, which the
            // next activation sets up again.
            Some(false) | None => OfflineRender::Refused,
        }
    }
}

impl Drop for Vst3Instance {
    fn drop(&mut self) {
        // An instance dropped while active is deactivated first (the
        // host deactivates before dropping; this covers the rest).
        if self.active.take().is_some() {
            self.plugin.set_active(false);
        }
    }
}

/// The VST3 format: makes [`Vst3Instance`]s, loading each bundle's module
/// once and sharing it while any instance from it lives (the module exits
/// when the last one goes). Loading runs on the main thread, as for CLAP
/// (Plan Ruling 2): a module's entry and factory are main-thread calls.
#[derive(Debug, Default)]
pub struct Vst3Format {
    modules: HashMap<PathBuf, Weak<Module>>,
}

impl Vst3Format {
    /// A format with no module loaded.
    pub fn new() -> Vst3Format {
        Vst3Format::default()
    }

    /// The bundle's module: the one an instance still holds, else a fresh
    /// load.
    fn module(&mut self, path: &Path) -> Result<Rc<Module>, HostError> {
        self.modules.retain(|_, m| m.strong_count() > 0);
        if let Some(m) = self.modules.get(path).and_then(Weak::upgrade) {
            return Ok(m);
        }
        let m = Rc::new(Module::load(path)?);
        self.modules.insert(path.to_path_buf(), Rc::downgrade(&m));
        Ok(m)
    }
}

impl PluginFormat for Vst3Format {
    fn format(&self) -> Format {
        Format::Vst3
    }

    fn prepare(&self, req: &LoadRequest) -> PrepareJob {
        let path = req.path.clone();
        let id = req.plugin.id.clone();
        Box::new(move || {
            let path = path.ok_or_else(|| HostError::NotFound(id.clone()))?;
            if !path.exists() {
                return Err(HostError::NotFound(format!("{id} ({})", path.display())));
            }
            Ok(Prepared(Box::new(path)))
        })
    }

    fn finish(
        &mut self,
        req: &LoadRequest,
        prepared: Prepared,
    ) -> Result<Box<dyn PluginInstance>, HostError> {
        let path = prepared
            .0
            .downcast::<PathBuf>()
            .map_err(|_| HostError::LoadFailed("not a VST3 load".into()))?;
        let module = self.module(&path)?;
        Ok(Box::new(Vst3Instance::with_module(module, &req.plugin.id)?))
    }
}
