//! The Audio Unit host (spec §5): AU v2 and v3 components through the
//! AudioUnit C API (Plan Ruling 6). v2 instances are created on the loader
//! thread; v3 instances asynchronously, out of process when the component
//! allows. Parameters use the AU parameter API, plugin-UI gestures the AU
//! event listener, state the `ClassInfo` property as a binary property
//! list. All `unsafe` is in `ffi.rs`.

mod ffi;

use core_engine::graph::ProcessStatus;
use core_host::catalog::Source;
use core_host::{
    AudioPortInfo, AudioProcessor, HostError, InstanceEvents, LoadRequest, ParamEvent, ParamInfo,
    PluginEditor, PluginFormat, PluginInfo, PluginInstance, PluginKind, PortRole, PrepareJob,
    Prepared, ProcessCtx, Tail,
};
use core_model::mixer::{ParamId, PluginFormat as Format, PluginRef};
use ffi::{ComponentInfo, Feed, GestureListener, Unit};
use objc2_audio_toolbox::{
    AudioUnitParameterOptions, kAudioUnitScope_Input, kAudioUnitScope_Output,
    kAudioUnitType_Effect, kAudioUnitType_MusicDevice, kAudioUnitType_MusicEffect,
};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::time::Duration;

/// Component types VizDAW hosts: effects, instruments and MIDI-driven
/// effects.
const TYPES: [u32; 3] = [
    kAudioUnitType_Effect,
    kAudioUnitType_MusicDevice,
    kAudioUnitType_MusicEffect,
];
/// `kAudioUnitParameterUnit_Indexed` and `_Boolean`.
const STEPPED_UNITS: [u32; 2] = [1, 2];

fn fourcc(v: u32) -> String {
    v.to_be_bytes().iter().map(|b| char::from(*b)).collect()
}

fn parse_fourcc(s: &str) -> Option<u32> {
    let b: [u8; 4] = s.as_bytes().try_into().ok()?;
    Some(u32::from_be_bytes(b))
}

/// `type/subtype/manufacturer` four-char codes (spec §4), e.g.
/// `aufx/dely/appl`.
pub fn au_id(kind: u32, subtype: u32, manufacturer: u32) -> String {
    format!(
        "{}/{}/{}",
        fourcc(kind),
        fourcc(subtype),
        fourcc(manufacturer)
    )
}

/// The codes of an AU id.
pub fn parse_au_id(id: &str) -> Option<(u32, u32, u32)> {
    let mut it = id.split('/');
    let (a, b, c) = (it.next()?, it.next()?, it.next()?);
    if it.next().is_some() {
        return None;
    }
    Some((parse_fourcc(a)?, parse_fourcc(b)?, parse_fourcc(c)?))
}

fn version_string(v: u32) -> String {
    format!("{}.{}.{}", v >> 16, (v >> 8) & 0xFF, v & 0xFF)
}

/// An installed component.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuComponent {
    /// Format, id, name, vendor, version.
    pub plugin: PluginRef,
    /// Instrument or effect.
    pub kind: PluginKind,
    /// Implemented with the v3 API.
    pub v3: bool,
}

fn describe(c: &ComponentInfo) -> AuComponent {
    let (vendor, name) = match c.name.split_once(": ") {
        Some((v, n)) => (v.to_string(), n.to_string()),
        None => (String::new(), c.name.clone()),
    };
    AuComponent {
        plugin: PluginRef {
            format: Format::Au,
            id: au_id(c.kind, c.subtype, c.manufacturer),
            name,
            vendor,
            version: version_string(c.version),
        },
        kind: if c.kind == kAudioUnitType_MusicDevice {
            PluginKind::Instrument
        } else {
            PluginKind::Effect
        },
        v3: c.v3,
    }
}

/// Every installed effect and instrument component (no plugin code runs:
/// this only reads the component registry).
pub fn components() -> Vec<AuComponent> {
    ffi::components(&TYPES).iter().map(describe).collect()
}

/// The component of `id`, if installed.
pub fn component(id: &str) -> Option<AuComponent> {
    let (k, s, m) = parse_au_id(id)?;
    ffi::find(k, s, m).map(|c| describe(&c))
}

/// One catalog source per component (path `au:<id>`, its version, no
/// modification time).
pub fn sources() -> Vec<Source> {
    components()
        .into_iter()
        .map(|c| Source {
            format: Format::Au,
            path: PathBuf::from(format!("au:{}", c.plugin.id)),
            version: c.plugin.version,
            mtime: 0,
        })
        .collect()
}

fn locate(id: &str) -> Result<ComponentInfo, HostError> {
    let (k, s, m) =
        parse_au_id(id).ok_or_else(|| HostError::NotFound(format!("bad AU id {id:?}")))?;
    ffi::find(k, s, m).ok_or_else(|| HostError::NotFound(id.to_string()))
}

/// Opens a component: v2 synchronously, v3 asynchronously (out of process
/// first, then in process).
fn open(c: &ComponentInfo) -> Result<Unit, HostError> {
    if !c.v3 {
        return ffi::instantiate_v2(c.component).map_err(HostError::LoadFailed);
    }
    let wait = core_host::scan::TIMEOUT_AU_V3;
    ffi::instantiate_async(c.component, true, wait)
        .or_else(|_| ffi::instantiate_async(c.component, false, wait))
        .map_err(HostError::LoadFailed)
}

/// What the loader made: an open, configured, initialized unit.
struct Opened {
    unit: Unit,
    desc: AuComponent,
    kind: u32,
    config: (f64, u32),
}

/// The AU [`PluginFormat`].
#[derive(Debug, Default)]
pub struct AuFormat;

impl AuFormat {
    /// The format.
    pub fn new() -> AuFormat {
        AuFormat
    }
}

impl PluginFormat for AuFormat {
    fn format(&self) -> Format {
        Format::Au
    }

    fn prepare(&self, req: &LoadRequest) -> PrepareJob {
        let id = req.plugin.id.clone();
        let (sr, max) = (req.sample_rate, req.max_frames);
        Box::new(move || {
            let c = locate(&id)?;
            let unit = open(&c)?;
            configure(&unit, c.kind, sr, max)?;
            unit.initialize().map_err(HostError::ActivateFailed)?;
            Ok(Prepared(Box::new(Opened {
                desc: describe(&c),
                kind: c.kind,
                unit,
                config: (sr, max),
            })))
        })
    }

    fn finish(
        &mut self,
        _req: &LoadRequest,
        prepared: Prepared,
    ) -> Result<Box<dyn PluginInstance>, HostError> {
        let o = prepared
            .0
            .downcast::<Opened>()
            .map_err(|_| HostError::LoadFailed("not an AU load".into()))?;
        Ok(Box::new(AuInstance::new(*o)?))
    }
}

/// Sets every bus to non-interleaved f32 (1 or 2 channels) and the block
/// size.
fn configure(unit: &Unit, kind: u32, sr: f64, max: u32) -> Result<(), HostError> {
    let err = HostError::ActivateFailed;
    unit.set_max_frames(max).map_err(err)?;
    for bus in 0..unit.element_count(kAudioUnitScope_Output) {
        let ch = unit.channels(kAudioUnitScope_Output, bus).clamp(1, 2);
        unit.set_format(kAudioUnitScope_Output, bus, sr, ch)
            .map_err(HostError::ActivateFailed)?;
    }
    // Effects that can take a second input bus have a sidechain (AU bus 1,
    // spec §5); most Apple effects refuse.
    if kind != kAudioUnitType_MusicDevice && unit.element_count(kAudioUnitScope_Input) == 1 {
        let _ = unit.set_element_count(kAudioUnitScope_Input, 2);
    }
    let inputs = unit.element_count(kAudioUnitScope_Input);
    for bus in 0..inputs.min(2) {
        let ch = unit.channels(kAudioUnitScope_Input, bus).clamp(1, 2);
        // A sidechain bus that refuses our format is simply not offered.
        let r = unit.set_format(kAudioUnitScope_Input, bus, sr, ch);
        if bus == 0 && kind != kAudioUnitType_MusicDevice {
            r.map_err(HostError::ActivateFailed)?;
        }
    }
    Ok(())
}

/// The last value the host itself wrote to each watched parameter, so the
/// gesture listener can tell the plugin's UI from an echo of the host's own
/// `AudioUnitSetParameter` (AU v3 units bridged to the C API may notify
/// listeners of those). Fixed at creation: writing is lock- and
/// allocation-free (RT).
#[derive(Debug)]
struct HostWrites {
    /// Watched parameter ids, sorted.
    ids: Box<[u32]>,
    /// `f32` bits of the last value written per id; NaN when none.
    values: Box<[AtomicU32]>,
}

impl HostWrites {
    fn new(mut ids: Vec<u32>) -> HostWrites {
        ids.sort_unstable();
        ids.dedup();
        let values = ids
            .iter()
            .map(|_| AtomicU32::new(f32::NAN.to_bits()))
            .collect();
        HostWrites {
            ids: ids.into_boxed_slice(),
            values,
        }
    }

    fn slot(&self, id: u32) -> Option<&AtomicU32> {
        self.ids
            .binary_search(&id)
            .ok()
            .and_then(|i| self.values.get(i))
    }

    /// Whether the listener watches `id` (a writable parameter).
    fn watches(&self, id: u32) -> bool {
        self.slot(id).is_some()
    }

    /// Notes a value the host wrote. // RT
    fn note(&self, id: u32, value: f32) {
        if let Some(s) = self.slot(id) {
            s.store(value.to_bits(), Ordering::Relaxed);
        }
    }

    /// Whether `value` is what the host last wrote to `id`.
    fn is_echo(&self, id: u32, value: f32) -> bool {
        self.slot(id)
            .is_some_and(|s| s.load(Ordering::Relaxed) == value.to_bits())
    }
}

/// A loaded AU (main-thread half).
pub struct AuInstance {
    // Declared first so it is dropped first: the listener's registrations
    // go before the unit is disposed.
    listener: Option<GestureListener>,
    unit: Arc<Unit>,
    kind: u32,
    info: PluginInfo,
    config: Option<(f64, u32)>,
    active: bool,
    latency: Arc<AtomicBool>,
    writes: Arc<HostWrites>,
}

impl std::fmt::Debug for AuInstance {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AuInstance")
            .field("plugin", &self.info.plugin.id)
            .finish_non_exhaustive()
    }
}

impl AuInstance {
    fn new(mut o: Opened) -> Result<AuInstance, HostError> {
        let latency = Arc::new(AtomicBool::new(false));
        // A unit that cannot report latency changes still works.
        let _ = o.unit.watch_latency(Arc::clone(&latency));
        let mut me = AuInstance {
            unit: Arc::new(o.unit),
            kind: o.kind,
            info: PluginInfo {
                plugin: o.desc.plugin,
                kind: o.desc.kind,
                inputs: Vec::new(),
                outputs: Vec::new(),
                note_input: false,
                params: Vec::new(),
                latency: 0,
                tail: Tail::None,
            },
            config: Some(o.config),
            active: false,
            latency,
            listener: None,
            writes: Arc::new(HostWrites::new(Vec::new())),
        };
        me.read_info();
        // Only parameters a user can change (writable, not meters): a
        // read-only value moving is not a gesture.
        let ids: Vec<u32> = me
            .unit
            .parameter_ids()
            .into_iter()
            .filter(|id| {
                me.unit.parameter_info(*id).is_some_and(|i| {
                    i.4.contains(AudioUnitParameterOptions::Flag_IsWritable)
                        && !i.4.contains(AudioUnitParameterOptions::Flag_MeterReadOnly)
                })
            })
            .collect();
        me.listener = GestureListener::new(&me.unit, &ids).ok();
        me.writes = Arc::new(HostWrites::new(ids));
        Ok(me)
    }

    fn read_info(&mut self) {
        let u = &self.unit;
        let instrument = self.kind == kAudioUnitType_MusicDevice;
        let port = |name: String, ch: u32, role| AudioPortInfo {
            name,
            channels: u16::try_from(ch.clamp(1, 2)).unwrap_or(2),
            role,
        };
        self.info.inputs = if instrument {
            Vec::new()
        } else {
            (0..u.element_count(kAudioUnitScope_Input).min(2))
                .map(|b| {
                    let role = if b == 0 {
                        PortRole::Main
                    } else {
                        PortRole::Sidechain
                    };
                    port(
                        if b == 0 {
                            "In".into()
                        } else {
                            "Sidechain".into()
                        },
                        u.channels(kAudioUnitScope_Input, b),
                        role,
                    )
                })
                .collect()
        };
        self.info.outputs = (0..u.element_count(kAudioUnitScope_Output))
            .map(|b| {
                port(
                    format!("Out {}", b + 1),
                    u.channels(kAudioUnitScope_Output, b),
                    if b == 0 {
                        PortRole::Main
                    } else {
                        PortRole::Aux
                    },
                )
            })
            .collect();
        self.info.note_input = instrument || self.kind == kAudioUnitType_MusicEffect;
        self.info.params = u
            .parameter_ids()
            .into_iter()
            .filter_map(|id| {
                let (name, min, max, def, flags, unit) = u.parameter_info(id)?;
                Some(ParamInfo {
                    id: ParamId(u64::from(id)),
                    name,
                    module: String::new(),
                    min: f64::from(min),
                    max: f64::from(max),
                    default: f64::from(def),
                    stepped: STEPPED_UNITS.contains(&unit),
                    automatable: flags.contains(AudioUnitParameterOptions::Flag_IsWritable),
                    hidden: !flags.contains(AudioUnitParameterOptions::Flag_IsReadable),
                })
            })
            .collect();
        let sr = self.config.map_or(48_000.0, |c| c.0);
        self.info.latency = (u.latency() * sr).round().max(0.0) as u32;
        let tail = u.tail();
        self.info.tail = if tail > 1000.0 {
            Tail::Infinite
        } else if tail > 0.0 {
            Tail::Frames((tail * sr).round() as u32)
        } else {
            Tail::None
        };
    }

    /// Acts like the plugin's own UI changing parameter `id` (begin
    /// gesture, value, end gesture). For tests: Apple's built-in units have
    /// no scriptable UI.
    pub fn simulate_ui_gesture(&self, id: ParamId, value: f64) {
        if let Ok(id) = u32::try_from(id.0) {
            ffi::ui_gesture(&self.unit, id, value as f32);
        }
    }
}

impl PluginInstance for AuInstance {
    fn info(&self) -> &PluginInfo {
        &self.info
    }

    fn activate(
        &mut self,
        sample_rate: f64,
        max_frames: u32,
    ) -> Result<Box<dyn AudioProcessor>, HostError> {
        if self.active {
            return Err(HostError::ActivateFailed("already active".into()));
        }
        let feed = Feed::new(max_frames);
        let refcon = feed.refcon();
        // The input callbacks are set while uninitialized: some units take
        // their connections only at initialization.
        self.unit.uninitialize();
        configure(&self.unit, self.kind, sample_rate, max_frames)?;
        for bus in 0..self.unit.element_count(kAudioUnitScope_Input).min(2) {
            self.unit
                .set_input(bus, refcon)
                .map_err(HostError::ActivateFailed)?;
        }
        self.unit.initialize().map_err(HostError::ActivateFailed)?;
        self.config = Some((sample_rate, max_frames));
        self.read_info();
        self.active = true;
        // Initializing may itself post a latency notification: what matters
        // is a change after this point.
        self.latency.store(false, Ordering::Release);
        Ok(Box::new(AuProcessor {
            unit: Arc::clone(&self.unit),
            writes: Arc::clone(&self.writes),
            feed,
            sample_time: 0.0,
            out_counts: self
                .info
                .outputs
                .iter()
                .map(|p| usize::from(p.channels))
                .collect(),
            max_frames,
        }))
    }

    fn deactivate(&mut self, processor: Box<dyn AudioProcessor>) {
        drop(processor);
        // Uninitialized (on the main thread) it cannot be rendered with the
        // input callbacks that pointed at the processor's feed; `activate`
        // sets them again.
        self.unit.uninitialize();
        self.active = false;
    }

    fn is_active(&self) -> bool {
        self.active
    }

    fn save_state(&mut self) -> Result<Vec<u8>, HostError> {
        self.unit.save_state().map_err(HostError::StateRejected)
    }

    fn load_state(&mut self, data: &[u8]) -> Result<(), HostError> {
        self.unit.load_state(data).map_err(HostError::StateRejected)
    }

    fn set_params(&mut self, values: &[(ParamId, f64)]) -> Result<(), HostError> {
        for (id, v) in values {
            if let Ok(id) = u32::try_from(id.0) {
                self.writes.note(id, *v as f32);
                self.unit.set_param(id, *v as f32, 0);
            }
        }
        Ok(())
    }

    fn param_value(&mut self, id: ParamId) -> Option<f64> {
        self.unit
            .get_param(u32::try_from(id.0).ok()?)
            .map(f64::from)
    }

    fn editor(&mut self) -> Option<&mut dyn PluginEditor> {
        // AU editors (view controllers, Cocoa UI) arrive with the app's
        // plugin windows (3c, Plan Ruling 7).
        None
    }

    fn idle(&mut self) -> InstanceEvents {
        // Units post the notification freely (on initialization, on any
        // reconfiguration): only a different latency is a change.
        let notified = self.latency.swap(false, Ordering::AcqRel);
        let latency_changed = notified && {
            let sr = self.config.map_or(48_000.0, |c| c.0);
            let now = (self.unit.latency() * sr).round().max(0.0) as u32;
            now != self.info.latency
        };
        InstanceEvents {
            latency_changed,
            ..InstanceEvents::default()
        }
    }

    fn gestures(&mut self) -> Vec<ParamEvent> {
        let Some(l) = &self.listener else {
            return Vec::new();
        };
        let writes = &self.writes;
        l.take()
            .into_iter()
            .filter(|(_, id, _)| writes.watches(*id))
            .filter_map(|(kind, raw, value)| {
                let id = ParamId(u64::from(raw));
                match kind {
                    // An echo of the host's own write is not the user.
                    0 if writes.is_echo(raw, value) => None,
                    0 => Some(ParamEvent::Value(id, f64::from(value))),
                    1 => Some(ParamEvent::Begin(id)),
                    2 => Some(ParamEvent::End(id)),
                    _ => None,
                }
            })
            .collect()
    }
}

/// The real-time half of an [`AuInstance`].
pub struct AuProcessor {
    unit: Arc<Unit>,
    writes: Arc<HostWrites>,
    feed: Feed,
    sample_time: f64,
    out_counts: Vec<usize>,
    max_frames: u32,
}

impl std::fmt::Debug for AuProcessor {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AuProcessor").finish_non_exhaustive()
    }
}

impl AudioProcessor for AuProcessor {
    fn process(&mut self, ctx: &mut ProcessCtx<'_>) -> ProcessStatus {
        let n = ctx.frames.min(self.max_frames);
        for p in ctx.params_in {
            if let Ok(id) = u32::try_from(p.id.0) {
                self.writes.note(id, p.value as f32);
                self.unit
                    .set_param(id, p.value as f32, p.frame.min(n.saturating_sub(1)));
            }
        }
        for e in ctx.events {
            let [s, d1, d2] = e.bytes;
            self.unit.midi(s, d1, d2, e.frame.min(n.saturating_sub(1)));
        }
        self.feed.set(
            [
                (ctx.input.as_mut_ptr(), ctx.input.len()),
                (ctx.sidechain.as_mut_ptr(), ctx.sidechain.len()),
            ],
            n as usize,
        );
        let mut ok = true;
        let mut rest: &mut [Box<[f32]>] = ctx.outputs;
        for (bus, count) in self.out_counts.iter().enumerate() {
            let take = (*count).min(rest.len());
            let (port, tail) = std::mem::take(&mut rest).split_at_mut(take);
            rest = tail;
            ok &= self.unit.render(bus as u32, self.sample_time, n, port);
        }
        self.feed.set([(std::ptr::null_mut(), 0); 2], 0);
        self.sample_time += f64::from(n);
        if ok {
            ProcessStatus::Ok
        } else {
            ProcessStatus::Error
        }
    }

    fn reset(&mut self) {
        self.unit.reset();
    }
}

/// Loads a component for the scanner and the tests: instantiate, configure
/// and initialize at `sample_rate` / `max_frames`.
pub fn load(id: &str, sample_rate: f64, max_frames: u32) -> Result<AuInstance, HostError> {
    let f = AuFormat::new();
    let req = LoadRequest {
        plugin: PluginRef {
            format: Format::Au,
            id: id.to_string(),
            name: id.to_string(),
            vendor: String::new(),
            version: String::new(),
        },
        path: None,
        sample_rate,
        max_frames,
    };
    let prepared = (f.prepare(&req))()?;
    let o = prepared
        .0
        .downcast::<Opened>()
        .map_err(|_| HostError::LoadFailed("not an AU load".into()))?;
    AuInstance::new(*o)
}

/// How long the scanner waits for an AU.
pub fn scan_timeout(v3: bool) -> Duration {
    if v3 {
        core_host::scan::TIMEOUT_AU_V3
    } else {
        core_host::scan::TIMEOUT
    }
}
