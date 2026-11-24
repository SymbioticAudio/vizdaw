//! The VST3 side of the test binary (host track H1): the same library
//! exports `GetPluginFactory` beside `clap_entry`, and [`crate::vst3_bundle`]
//! wraps it as `VizDAW Test.vst3`. It serves the scanner and instance
//! tests: a factory (`IPluginFactory3`) listing several classes, a split
//! plug-in (component and a separate controller class, connected through
//! `IConnectionPoint`) and a single-component one (the component is its
//! own controller). Later tasks give the components audio and state.
//!
//! | class ID | name | sub-categories | what it does |
//! |---|---|---|---|
//! | [`VST3_GAIN_ID`] | Test Gain | `Fx` | split: a component (stereo main in and out, a stereo sidechain in) and the [`VST3_CONTROLLER_ID`] controller with the [`VST3_PARAMS`] |
//! | [`VST3_SYNTH_ID`] | Test Synth | `Instrument\|Synth` | single component: an event input, two stereo outputs, the same parameters |
//! | [`VST3_CRASHER_ID`] | Test Crasher | `Fx` | crashes (SIGSEGV) in `createInstance` |
//! | [`VST3_HANGER_ID`] | Test Hanger | `Fx` | never returns from `createInstance` |
//! | [`VST3_CONTROLLER_ID`] | Test Controller | (controller class) | Test Gain's controller; not a plug-in |
//!
//! Like a plug-in built with the SDK, the factory is only handed out after
//! the module entry (`bundleEntry` on macOS, `ModuleEntry` on Linux) ran.
//!
//! Creating any object also gives the creating thread a thread-exit
//! destructor in this library (a pthread key, as Rust's `std` and many
//! C++ plug-ins register): a host that unloads the library before such a
//! thread exits crashes when the thread does.
//!
//! The Gain component's state is [`VST3_COMPONENT_GAIN`] (a normalised
//! value, 8 bytes little-endian); a controller given it through
//! `setComponentState` takes it as its Gain value. When connected, the
//! component sends the controller a message (made through the host's
//! `IHostApplication::createInstance`) that the controller counts.
//!
//! # Lifecycle probe
//!
//! The library counts its own lifecycle: module entries and exits,
//! initialise and terminate calls, connections, and when objects are
//! released (as a sequence number, so orders can be asserted). A test
//! reads them through [`crate::Vst3Probe`], which `dlopen`s the same binary
//! (the counters live in the loaded image, not in the test's own copy of
//! this crate) and calls the exported `vizdaw_test_vst3_counters`. The
//! probe also sets a mode ([`crate::Vst3Mode`]) that makes the module
//! misbehave, and makes the latest controller call its component handler
//! ([`crate::Vst3Edit`]), on the caller's thread or on another one. The
//! mode and counters are per loaded image; tests that use them load a
//! private copy of the bundle so they never see each other's.

use std::ffi::{CStr, c_char, c_void};
use std::sync::Mutex;
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::thread::ThreadId;
// The bindings' module (named after the SDK's vendor) is only named here.
use vst3::Steinberg as sdk;
use vst3::com_scrape_types::Unknown;
use vst3::{Class, ComPtr, ComRef, ComWrapper, Interface, uid};

use sdk::PClassInfo_::ClassCardinality_;
use sdk::PFactoryInfo_::FactoryFlags_;
use sdk::Vst::BusInfo_::BusFlags_;
use sdk::Vst::ParameterInfo_::ParameterFlags_;
use sdk::Vst::{
    BusDirection, BusDirections_, BusInfo, BusType, BusTypes_, IAttributeListTrait,
    IAudioProcessor, IAudioProcessorTrait, IComponent, IComponentHandler, IComponentHandler2,
    IComponentHandler2Trait, IComponentHandlerTrait, IComponentTrait, IConnectionPoint,
    IConnectionPointTrait, IEditController, IEditControllerTrait, IHostApplication,
    IHostApplicationTrait, IMessage, IMessageTrait, IoMode, MediaType, MediaTypes_, ParamID,
    ParamValue, ParameterInfo, ProcessData, ProcessModes_, ProcessSetup, RoutingInfo,
    SpeakerArrangement, String128, SymbolicSampleSizes_, TChar,
};
use sdk::{
    FIDString, FUnknown, IBStream, IBStreamTrait, IPlugView, IPluginBaseTrait, IPluginFactory,
    IPluginFactory2Trait, IPluginFactory3, IPluginFactory3Trait, IPluginFactoryTrait, PClassInfo,
    PClassInfo2, PClassInfoW, PFactoryInfo, TBool, TUID, int32, kInvalidArgument, kNoInterface,
    kNotImplemented, kResultFalse, kResultOk, kResultTrue, tresult, uint32,
};

/// Class ID of "Test Gain" (an effect), as the host spells it.
pub const VST3_GAIN_ID: &str = "56495A44565354337E570000ABCD0001";
/// Class ID of "Test Synth" (an instrument).
pub const VST3_SYNTH_ID: &str = "56495A44565354337E570000ABCD0002";
/// Class ID of "Test Crasher" (crashes when created).
pub const VST3_CRASHER_ID: &str = "56495A44565354337E570000ABCD0003";
/// Class ID of "Test Hanger" (hangs when created).
pub const VST3_HANGER_ID: &str = "56495A44565354337E570000ABCD0004";
/// Class ID of the controller class (not a plug-in, never listed).
pub const VST3_CONTROLLER_ID: &str = "56495A44565354337E570000ABCD00FF";

const GAIN: TUID = uid(0x5649_5A44, 0x5653_5433, 0x7E57_0000, 0xABCD_0001);
const SYNTH: TUID = uid(0x5649_5A44, 0x5653_5433, 0x7E57_0000, 0xABCD_0002);
const CRASHER: TUID = uid(0x5649_5A44, 0x5653_5433, 0x7E57_0000, 0xABCD_0003);
const HANGER: TUID = uid(0x5649_5A44, 0x5653_5433, 0x7E57_0000, 0xABCD_0004);
const CONTROLLER: TUID = uid(0x5649_5A44, 0x5653_5433, 0x7E57_0000, 0xABCD_00FF);

const AUDIO_MODULE: &str = "Audio Module Class";
const CONTROLLER_CLASS: &str = "Component Controller Class";

/// One parameter of the test controllers.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Vst3ParamDef {
    /// The VST3 `ParamID` (not its index).
    pub id: u32,
    /// Title.
    pub title: &'static str,
    /// Units.
    pub units: &'static str,
    /// `stepCount` (0: continuous).
    pub steps: i32,
    /// Default, normalised.
    pub default: f64,
    /// `ParameterInfo::flags`.
    pub flags: i32,
    /// Plain value at normalised 0.
    pub min: f64,
    /// Plain value at normalised 1.
    pub max: f64,
}

impl Vst3ParamDef {
    /// Normalised to plain: linear, rounded to a step when stepped.
    pub fn to_plain(&self, n: f64) -> f64 {
        let n = n.clamp(0.0, 1.0);
        let n = if self.steps > 0 {
            (n * f64::from(self.steps)).round() / f64::from(self.steps)
        } else {
            n
        };
        self.min + n * (self.max - self.min)
    }

    /// Plain to normalised (the inverse of [`Self::to_plain`]).
    pub fn to_normalized(&self, plain: f64) -> f64 {
        let n = ((plain - self.min) / (self.max - self.min)).clamp(0.0, 1.0);
        if self.steps > 0 {
            (n * f64::from(self.steps)).round() / f64::from(self.steps)
        } else {
            n
        }
    }
}

/// `ParamID` of Gain (dB, -60 to +12, automatable).
pub const VST3_PARAM_GAIN: u32 = 10;
/// `ParamID` of Mode (three steps, 0 to 2, a list).
pub const VST3_PARAM_MODE: u32 = 11;
/// `ParamID` of Meter (read-only).
pub const VST3_PARAM_METER: u32 = 12;
/// `ParamID` of Secret (hidden, not automatable).
pub const VST3_PARAM_SECRET: u32 = 13;

/// The test controllers' parameters, in their order.
pub const VST3_PARAMS: [Vst3ParamDef; 4] = [
    Vst3ParamDef {
        id: VST3_PARAM_GAIN,
        title: "Gain",
        units: "dB",
        steps: 0,
        default: 60.0 / 72.0,
        flags: ParameterFlags_::kCanAutomate,
        min: -60.0,
        max: 12.0,
    },
    Vst3ParamDef {
        id: VST3_PARAM_MODE,
        title: "Mode",
        units: "",
        steps: 2,
        default: 0.0,
        flags: ParameterFlags_::kCanAutomate | ParameterFlags_::kIsList,
        min: 0.0,
        max: 2.0,
    },
    Vst3ParamDef {
        id: VST3_PARAM_METER,
        title: "Meter",
        units: "",
        steps: 0,
        default: 0.0,
        flags: ParameterFlags_::kIsReadOnly,
        min: 0.0,
        max: 1.0,
    },
    Vst3ParamDef {
        id: VST3_PARAM_SECRET,
        title: "Secret",
        units: "",
        steps: 0,
        default: 0.5,
        flags: ParameterFlags_::kIsHidden,
        min: 0.0,
        max: 1.0,
    },
];

/// The Gain component's state: its gain, normalised (8 bytes
/// little-endian), which a controller takes through `setComponentState`.
pub const VST3_COMPONENT_GAIN: f64 = 0.25;

fn param(id: ParamID) -> Option<(usize, &'static Vst3ParamDef)> {
    VST3_PARAMS.iter().enumerate().find(|(_, p)| p.id == id)
}

/// One class of the factory.
struct ClassDef {
    cid: TUID,
    category: &'static str,
    name: &'static str,
    sub_categories: &'static str,
}

const CLASSES: [ClassDef; 5] = [
    ClassDef {
        cid: GAIN,
        category: AUDIO_MODULE,
        name: "Test Gain",
        sub_categories: "Fx",
    },
    ClassDef {
        cid: CONTROLLER,
        category: CONTROLLER_CLASS,
        name: "Test Controller",
        sub_categories: "",
    },
    ClassDef {
        cid: SYNTH,
        category: AUDIO_MODULE,
        name: "Test Synth",
        sub_categories: "Instrument|Synth",
    },
    ClassDef {
        cid: CRASHER,
        category: AUDIO_MODULE,
        name: "Test Crasher",
        sub_categories: "Fx",
    },
    ClassDef {
        cid: HANGER,
        category: AUDIO_MODULE,
        name: "Test Hanger",
        sub_categories: "Fx",
    },
];

const VENDOR: &str = "VizDAW";
const VERSION: &str = "1.0.0";
const SDK_VERSION: &str = "VST 3.8.0";

/// Entries minus exits: more than zero while the module is entered. A
/// count, not a flag: loads of one image in parallel overlap.
static ENTERED: AtomicU32 = AtomicU32::new(0);

/// How the module behaves (see [`crate::Vst3Mode`]).
static MODE: AtomicU32 = AtomicU32::new(0);

fn mode() -> u32 {
    MODE.load(Ordering::SeqCst)
}

/// Indices into [`COUNTERS`] (the order of `vizdaw_test_vst3_counters`).
mod ix {
    pub const ENTRIES: usize = 0;
    pub const EXITS: usize = 1;
    pub const FACTORY_RELEASES: usize = 2;
    pub const COMPONENT_RELEASES: usize = 3;
    pub const LAST_FACTORY_RELEASE: usize = 4;
    pub const LAST_COMPONENT_RELEASE: usize = 5;
    pub const LAST_EXIT: usize = 6;
    pub const CONTROLLER_RELEASES: usize = 7;
    pub const LAST_CONTROLLER_RELEASE: usize = 8;
    pub const COMPONENT_INITS: usize = 9;
    pub const COMPONENT_TERMS: usize = 10;
    pub const LAST_COMPONENT_TERM: usize = 11;
    pub const CONTROLLER_INITS: usize = 12;
    pub const CONTROLLER_TERMS: usize = 13;
    pub const LAST_CONTROLLER_TERM: usize = 14;
    pub const CONNECTS: usize = 15;
    pub const DISCONNECTS: usize = 16;
    pub const LAST_DISCONNECT: usize = 17;
    pub const MESSAGES: usize = 18;
    pub const HANDLER_SETS: usize = 19;
    pub const STATE_SYNCS: usize = 20;
    pub const OFF_THREAD: usize = 21;
    pub const HOST_CONTEXTS: usize = 22;
    pub const SETUPS: usize = 23;
    pub const LAST_PROCESS_MODE: usize = 24;
    pub const THREAD_EXITS: usize = 25;
    pub const N: usize = 26;
}

static COUNTERS: [AtomicU64; ix::N] = [const { AtomicU64::new(0) }; ix::N];
/// Source of event sequence numbers (the first is 1; 0 means "never").
static SEQ: AtomicU64 = AtomicU64::new(0);

fn bump(i: usize) {
    if let Some(c) = COUNTERS.get(i) {
        c.fetch_add(1, Ordering::SeqCst);
    }
}

fn stamp(i: usize) {
    let t = SEQ.fetch_add(1, Ordering::SeqCst) + 1;
    if let Some(c) = COUNTERS.get(i) {
        c.store(t, Ordering::SeqCst);
    }
}

fn store(i: usize, v: u64) {
    if let Some(c) = COUNTERS.get(i) {
        c.store(v, Ordering::SeqCst);
    }
}

fn enter() -> bool {
    if mode() == MODE_ENTRY_FAILS {
        return false;
    }
    bump(ix::ENTRIES);
    ENTERED.fetch_add(1, Ordering::SeqCst);
    true
}

fn exit() -> bool {
    bump(ix::EXITS);
    ENTERED.fetch_sub(1, Ordering::SeqCst);
    stamp(ix::LAST_EXIT);
    true
}

/// The lifecycle counters, in the order of [`ix`], written to
/// `out[..len]` (at most all of them).
#[unsafe(no_mangle)]
pub extern "C" fn vizdaw_test_vst3_counters(out: *mut u64, len: usize) {
    let v: Vec<u64> = COUNTERS.iter().map(|c| c.load(Ordering::SeqCst)).collect();
    // SAFETY: the caller passes room for `len` values.
    unsafe { std::ptr::copy_nonoverlapping(v.as_ptr(), out, len.min(v.len())) };
}

/// Sets the mode (see [`crate::Vst3Mode`]).
#[unsafe(no_mangle)]
pub extern "C" fn vizdaw_test_vst3_set_mode(mode: u32) {
    MODE.store(mode, Ordering::SeqCst);
}

const MODE_ENTRY_FAILS: u32 = 1;
const MODE_NULL_FACTORY: u32 = 2;
const MODE_COMPONENT_INIT_FAILS: u32 = 3;
const MODE_CONTROLLER_INIT_FAILS: u32 = 4;
const MODE_OFFLINE_REFUSED: u32 = 5;

/// The component handler the latest controller was given.
struct Handler(ComPtr<IComponentHandler>);
// SAFETY: a host's component handler may be called from any thread (the
// SDK allows calls from the processing thread); calling it from another
// thread is what the probe tests.
unsafe impl Send for Handler {}

static HANDLER: Mutex<Option<Handler>> = Mutex::new(None);

fn handler() -> Option<ComPtr<IComponentHandler>> {
    HANDLER.lock().ok()?.as_ref().map(|h| h.0.clone())
}

fn set_handler(h: Option<ComPtr<IComponentHandler>>) {
    if let Ok(mut g) = HANDLER.lock() {
        *g = h.map(Handler);
    }
}

/// Makes the latest controller call its component handler: `kind` 0
/// `beginEdit(id)`, 1 `performEdit(id, value)`, 2 `endEdit(id)`, 3
/// `restartComponent(flags)`, 4 `IComponentHandler2::setDirty(flags != 0)`;
/// on a new thread when `off_thread` is non-zero. Returns the handler's
/// result, or -1 when there is no handler (or no `IComponentHandler2`).
#[unsafe(no_mangle)]
pub extern "C" fn vizdaw_test_vst3_edit(
    kind: u32,
    id: u32,
    value: f64,
    flags: i32,
    off_thread: u8,
) -> i32 {
    let Some(h) = handler() else {
        return -1;
    };
    let call = move |h: &ComPtr<IComponentHandler>| -> i32 {
        // SAFETY: plain calls on the host's live handler.
        unsafe {
            match kind {
                0 => h.beginEdit(id),
                1 => h.performEdit(id, value),
                2 => h.endEdit(id),
                3 => h.restartComponent(flags),
                4 => match h.cast::<IComponentHandler2>() {
                    Some(h2) => h2.setDirty(u8::from(flags != 0)),
                    None => -1,
                },
                _ => kInvalidArgument,
            }
        }
    };
    if off_thread == 0 {
        return call(&h);
    }
    let h = Handler(h);
    std::thread::spawn(move || {
        let h = h;
        call(&h.0)
    })
    .join()
    .unwrap_or(-1)
}

fn copy_str(src: &str, dst: &mut [c_char]) {
    let n = src.len().min(dst.len().saturating_sub(1));
    for (d, s) in dst.iter_mut().zip(src.bytes().take(n)) {
        *d = s as c_char;
    }
    if let Some(end) = dst.get_mut(n) {
        *end = 0;
    }
}

fn copy_wide(src: &str, dst: &mut [u16]) {
    let wide: Vec<u16> = src.encode_utf16().collect();
    let n = wide.len().min(dst.len().saturating_sub(1));
    dst[..n].copy_from_slice(&wide[..n]);
    if let Some(end) = dst.get_mut(n) {
        *end = 0;
    }
}

/// Gives the calling thread a value under a pthread key whose destructor
/// is code in this library; it runs when the thread exits.
#[cfg(unix)]
fn mark_thread() {
    use std::sync::OnceLock;
    extern "C" fn on_thread_exit(_value: *mut c_void) {
        bump(ix::THREAD_EXITS);
    }
    static KEY: OnceLock<Option<libc::pthread_key_t>> = OnceLock::new();
    let key = KEY.get_or_init(|| {
        let mut key: libc::pthread_key_t = 0;
        // SAFETY: `key` is ours to fill; the destructor is a plain
        // `extern "C"` function of this library.
        (unsafe { libc::pthread_key_create(&mut key, Some(on_thread_exit)) } == 0).then_some(key)
    });
    if let Some(key) = key {
        // SAFETY: a live key; any non-null value makes the destructor run.
        unsafe { libc::pthread_setspecific(*key, std::ptr::dangling::<u8>() as *const c_void) };
    }
}

#[cfg(not(unix))]
fn mark_thread() {}

/// Hands out `obj` as interface `iid` through `out` (the factory's and the
/// host's `createInstance` convention).
fn hand_out(obj: Option<ComPtr<FUnknown>>, iid: &[u8; 16], out: *mut *mut c_void) -> tresult {
    let Some(obj) = obj else {
        return kNoInterface;
    };
    // SAFETY: `obj` is live; a successful query adds the reference the
    // caller owns.
    match unsafe { FUnknown::query_interface(obj.as_ptr(), iid) } {
        Some(p) => {
            // SAFETY: `out` is the caller's out pointer.
            unsafe { *out = p };
            kResultOk
        }
        None => kNoInterface,
    }
}

/// One bus of a test component.
struct Bus {
    media: MediaType,
    dir: BusDirection,
    name: &'static str,
    channels: i32,
    kind: BusType,
    default_active: bool,
}

const GAIN_BUSES: [Bus; 3] = [
    Bus {
        media: MediaTypes_::kAudio as MediaType,
        dir: BusDirections_::kInput as BusDirection,
        name: "Input",
        channels: 2,
        kind: BusTypes_::kMain as BusType,
        default_active: true,
    },
    Bus {
        media: MediaTypes_::kAudio as MediaType,
        dir: BusDirections_::kInput as BusDirection,
        name: "Sidechain",
        channels: 2,
        kind: BusTypes_::kAux as BusType,
        default_active: false,
    },
    Bus {
        media: MediaTypes_::kAudio as MediaType,
        dir: BusDirections_::kOutput as BusDirection,
        name: "Output",
        channels: 2,
        kind: BusTypes_::kMain as BusType,
        default_active: true,
    },
];

const SYNTH_BUSES: [Bus; 3] = [
    Bus {
        media: MediaTypes_::kEvent as MediaType,
        dir: BusDirections_::kInput as BusDirection,
        name: "MIDI In",
        channels: 16,
        kind: BusTypes_::kMain as BusType,
        default_active: true,
    },
    Bus {
        media: MediaTypes_::kAudio as MediaType,
        dir: BusDirections_::kOutput as BusDirection,
        name: "Main Out",
        channels: 2,
        kind: BusTypes_::kMain as BusType,
        default_active: true,
    },
    Bus {
        media: MediaTypes_::kAudio as MediaType,
        dir: BusDirections_::kOutput as BusDirection,
        name: "Out 3-4",
        channels: 2,
        kind: BusTypes_::kAux as BusType,
        default_active: false,
    },
];

fn buses(
    all: &'static [Bus],
    media: MediaType,
    dir: BusDirection,
) -> impl Iterator<Item = &'static Bus> {
    all.iter().filter(move |b| b.media == media && b.dir == dir)
}

fn bus_count(all: &'static [Bus], media: MediaType, dir: BusDirection) -> int32 {
    buses(all, media, dir).count() as int32
}

fn bus_info(
    all: &'static [Bus],
    media: MediaType,
    dir: BusDirection,
    index: int32,
    out: *mut BusInfo,
) -> tresult {
    let b = usize::try_from(index)
        .ok()
        .and_then(|i| buses(all, media, dir).nth(i));
    // SAFETY: the host passes a struct to fill.
    let (Some(b), Some(out)) = (b, unsafe { out.as_mut() }) else {
        return kInvalidArgument;
    };
    out.mediaType = b.media;
    out.direction = b.dir;
    out.channelCount = b.channels;
    copy_wide(b.name, &mut out.name);
    out.busType = b.kind;
    out.flags = if b.default_active {
        BusFlags_::kDefaultActive
    } else {
        0
    };
    kResultOk
}

/// The parameter values and handler a controller keeps, and the thread
/// that made it (every call should come on that thread).
struct Ctl {
    values: [AtomicU64; 4],
    handler: Mutex<Option<ComPtr<IComponentHandler>>>,
    creator: ThreadId,
}

impl Ctl {
    fn new() -> Ctl {
        Ctl {
            values: VST3_PARAMS.map(|p| AtomicU64::new(p.default.to_bits())),
            handler: Mutex::new(None),
            creator: std::thread::current().id(),
        }
    }

    /// Counts a call that came on another thread than the creator's.
    fn check(&self) {
        if std::thread::current().id() != self.creator {
            bump(ix::OFF_THREAD);
        }
    }

    fn get(&self, id: ParamID) -> f64 {
        param(id)
            .and_then(|(i, _)| self.values.get(i))
            .map_or(0.0, |v| f64::from_bits(v.load(Ordering::SeqCst)))
    }

    fn set(&self, id: ParamID, n: f64) -> tresult {
        match param(id).and_then(|(i, _)| self.values.get(i)) {
            Some(v) => {
                v.store(n.clamp(0.0, 1.0).to_bits(), Ordering::SeqCst);
                kResultOk
            }
            None => kInvalidArgument,
        }
    }

    fn set_handler(&self, h: *mut IComponentHandler) {
        // SAFETY: the host's handler, or null; `to_com_ptr` adds our
        // reference.
        let h = unsafe { ComRef::from_raw(h) }.map(|r| r.to_com_ptr());
        if h.is_some() {
            bump(ix::HANDLER_SETS);
        }
        if let Ok(mut g) = self.handler.lock() {
            *g = h.clone();
        }
        set_handler(h);
    }

    fn drop_handler(&self) {
        if let Ok(mut g) = self.handler.lock() {
            *g = None;
        }
        set_handler(None);
    }

    /// `setComponentState`: the component's gain becomes the Gain value.
    fn take_component_state(&self, state: *mut IBStream) -> tresult {
        // SAFETY: the host's stream, for this call.
        let Some(s) = (unsafe { ComRef::from_raw(state) }) else {
            return kInvalidArgument;
        };
        let mut b = [0u8; 8];
        let mut n: int32 = 0;
        // SAFETY: `b` has room for 8 bytes.
        let r = unsafe { s.read(b.as_mut_ptr() as *mut c_void, 8, &mut n) };
        if r != kResultOk || n != 8 {
            return kResultFalse;
        }
        bump(ix::STATE_SYNCS);
        self.set(VST3_PARAM_GAIN, f64::from_le_bytes(b))
    }
}

/// Implements `IEditController` over a `ctl: Ctl` field.
macro_rules! edit_controller {
    ($t:ty) => {
        impl IEditControllerTrait for $t {
            unsafe fn setComponentState(&self, state: *mut IBStream) -> tresult {
                self.ctl.check();
                self.ctl.take_component_state(state)
            }

            unsafe fn setState(&self, _state: *mut IBStream) -> tresult {
                self.ctl.check();
                kResultOk
            }

            unsafe fn getState(&self, _state: *mut IBStream) -> tresult {
                self.ctl.check();
                kResultOk
            }

            unsafe fn getParameterCount(&self) -> int32 {
                self.ctl.check();
                VST3_PARAMS.len() as int32
            }

            unsafe fn getParameterInfo(&self, index: int32, info: *mut ParameterInfo) -> tresult {
                self.ctl.check();
                let p = usize::try_from(index).ok().and_then(|i| VST3_PARAMS.get(i));
                // SAFETY: the host passes a struct to fill.
                let (Some(p), Some(info)) = (p, unsafe { info.as_mut() }) else {
                    return kInvalidArgument;
                };
                info.id = p.id;
                copy_wide(p.title, &mut info.title);
                copy_wide(p.title, &mut info.shortTitle);
                copy_wide(p.units, &mut info.units);
                info.stepCount = p.steps;
                info.defaultNormalizedValue = p.default;
                info.unitId = 0;
                info.flags = p.flags;
                kResultOk
            }

            unsafe fn getParamStringByValue(
                &self,
                id: ParamID,
                value: ParamValue,
                string: *mut String128,
            ) -> tresult {
                self.ctl.check();
                // SAFETY: the host passes a string to fill.
                let (Some((_, p)), Some(s)) = (param(id), unsafe { string.as_mut() }) else {
                    return kInvalidArgument;
                };
                copy_wide(&format!("{:.2}", p.to_plain(value)), s);
                kResultOk
            }

            unsafe fn getParamValueByString(
                &self,
                _id: ParamID,
                _string: *mut TChar,
                _value: *mut ParamValue,
            ) -> tresult {
                self.ctl.check();
                kNotImplemented
            }

            unsafe fn normalizedParamToPlain(&self, id: ParamID, value: ParamValue) -> ParamValue {
                self.ctl.check();
                param(id).map_or(value, |(_, p)| p.to_plain(value))
            }

            unsafe fn plainParamToNormalized(&self, id: ParamID, plain: ParamValue) -> ParamValue {
                self.ctl.check();
                param(id).map_or(plain, |(_, p)| p.to_normalized(plain))
            }

            unsafe fn getParamNormalized(&self, id: ParamID) -> ParamValue {
                self.ctl.check();
                self.ctl.get(id)
            }

            unsafe fn setParamNormalized(&self, id: ParamID, value: ParamValue) -> tresult {
                self.ctl.check();
                self.ctl.set(id, value)
            }

            unsafe fn setComponentHandler(&self, handler: *mut IComponentHandler) -> tresult {
                self.ctl.check();
                self.ctl.set_handler(handler);
                kResultOk
            }

            unsafe fn createView(&self, _name: FIDString) -> *mut IPlugView {
                self.ctl.check();
                std::ptr::null_mut()
            }
        }
    };
}

/// Implements a do-nothing `IAudioProcessor` (Task 4 gives it audio) that
/// counts `setupProcessing` and keeps the process mode it was given.
macro_rules! audio_processor {
    ($t:ty) => {
        impl IAudioProcessorTrait for $t {
            unsafe fn setBusArrangements(
                &self,
                _inputs: *mut SpeakerArrangement,
                _num_ins: int32,
                _outputs: *mut SpeakerArrangement,
                _num_outs: int32,
            ) -> tresult {
                kResultOk
            }

            unsafe fn getBusArrangement(
                &self,
                _dir: BusDirection,
                _index: int32,
                arr: *mut SpeakerArrangement,
            ) -> tresult {
                // SAFETY: the host passes a value to fill.
                match unsafe { arr.as_mut() } {
                    Some(a) => {
                        // Left and right.
                        *a = 3;
                        kResultOk
                    }
                    None => kInvalidArgument,
                }
            }

            unsafe fn canProcessSampleSize(&self, size: int32) -> tresult {
                if size == SymbolicSampleSizes_::kSample32 as int32 {
                    kResultTrue
                } else {
                    kResultFalse
                }
            }

            unsafe fn getLatencySamples(&self) -> uint32 {
                0
            }

            unsafe fn setupProcessing(&self, setup: *mut ProcessSetup) -> tresult {
                // SAFETY: the host's setup, for this call.
                let Some(s) = (unsafe { setup.as_ref() }) else {
                    return kInvalidArgument;
                };
                bump(ix::SETUPS);
                store(ix::LAST_PROCESS_MODE, s.processMode as u64);
                if mode() == MODE_OFFLINE_REFUSED
                    && s.processMode == ProcessModes_::kOffline as int32
                {
                    return kResultFalse;
                }
                kResultOk
            }

            unsafe fn setProcessing(&self, _state: TBool) -> tresult {
                kResultOk
            }

            unsafe fn process(&self, _data: *mut ProcessData) -> tresult {
                kResultOk
            }

            unsafe fn getTailSamples(&self) -> uint32 {
                0
            }
        }
    };
}

/// The component's side of `IComponent` that does not depend on buses.
macro_rules! component_common {
    () => {
        unsafe fn setIoMode(&self, _mode: IoMode) -> tresult {
            kResultOk
        }

        unsafe fn getRoutingInfo(&self, _in: *mut RoutingInfo, _out: *mut RoutingInfo) -> tresult {
            kNotImplemented
        }

        unsafe fn setActive(&self, _state: TBool) -> tresult {
            kResultOk
        }

        unsafe fn setState(&self, _state: *mut IBStream) -> tresult {
            kResultOk
        }
    };
}

fn component_initialize(context: *mut FUnknown) -> (tresult, Option<ComPtr<IHostApplication>>) {
    bump(ix::COMPONENT_INITS);
    if mode() == MODE_COMPONENT_INIT_FAILS {
        return (kResultFalse, None);
    }
    // SAFETY: the host's context, or null.
    let host = unsafe { ComRef::from_raw(context) }.and_then(|c| c.cast::<IHostApplication>());
    (kResultOk, host)
}

fn component_terminate() {
    bump(ix::COMPONENT_TERMS);
    stamp(ix::LAST_COMPONENT_TERM);
}

/// Test Gain's component: buses and a processor; its controller is the
/// separate [`Controller`] class, connected through `IConnectionPoint`.
struct Component {
    host: Mutex<Option<ComPtr<IHostApplication>>>,
    peer: Mutex<Option<ComPtr<IConnectionPoint>>>,
}

impl Drop for Component {
    fn drop(&mut self) {
        bump(ix::COMPONENT_RELEASES);
        stamp(ix::LAST_COMPONENT_RELEASE);
    }
}

impl Class for Component {
    type Interfaces = (IComponent, IAudioProcessor, IConnectionPoint);
}

impl IPluginBaseTrait for Component {
    unsafe fn initialize(&self, context: *mut FUnknown) -> tresult {
        let (r, host) = component_initialize(context);
        if let Ok(mut g) = self.host.lock() {
            *g = host;
        }
        r
    }

    unsafe fn terminate(&self) -> tresult {
        component_terminate();
        if let Ok(mut g) = self.host.lock() {
            *g = None;
        }
        kResultOk
    }
}

impl IComponentTrait for Component {
    unsafe fn getControllerClassId(&self, class_id: *mut TUID) -> tresult {
        if class_id.is_null() {
            return kInvalidArgument;
        }
        // SAFETY: the host passes room for one TUID.
        unsafe { *class_id = CONTROLLER };
        kResultOk
    }

    unsafe fn getBusCount(&self, media: MediaType, dir: BusDirection) -> int32 {
        bus_count(&GAIN_BUSES, media, dir)
    }

    unsafe fn getBusInfo(
        &self,
        media: MediaType,
        dir: BusDirection,
        index: int32,
        bus: *mut BusInfo,
    ) -> tresult {
        bus_info(&GAIN_BUSES, media, dir, index, bus)
    }

    unsafe fn activateBus(
        &self,
        media: MediaType,
        dir: BusDirection,
        index: int32,
        _state: TBool,
    ) -> tresult {
        if index >= 0 && index < bus_count(&GAIN_BUSES, media, dir) {
            kResultOk
        } else {
            kInvalidArgument
        }
    }

    unsafe fn getState(&self, state: *mut IBStream) -> tresult {
        // SAFETY: the host's stream, for this call.
        let Some(s) = (unsafe { ComRef::from_raw(state) }) else {
            return kInvalidArgument;
        };
        let mut b = VST3_COMPONENT_GAIN.to_le_bytes();
        let mut n: int32 = 0;
        // SAFETY: `b` holds 8 bytes.
        let r = unsafe { s.write(b.as_mut_ptr() as *mut c_void, 8, &mut n) };
        if r == kResultOk && n == 8 {
            kResultOk
        } else {
            kResultFalse
        }
    }

    component_common!();
}

audio_processor!(Component);

impl IConnectionPointTrait for Component {
    unsafe fn connect(&self, other: *mut IConnectionPoint) -> tresult {
        // SAFETY: the host passes the peer, or null.
        let Some(peer) = (unsafe { ComRef::from_raw(other) }).map(|r| r.to_com_ptr()) else {
            return kInvalidArgument;
        };
        bump(ix::CONNECTS);
        // Says hello through a message the host makes.
        let host = self.host.lock().ok().and_then(|g| g.clone());
        if let Some(host) = host {
            let mut cid: TUID = IMessage::IID.map(|b| b as c_char);
            let mut iid = cid;
            let mut obj: *mut c_void = std::ptr::null_mut();
            // SAFETY: the host fills `obj` with a message we own, or fails.
            let r = unsafe { host.createInstance(&mut cid, &mut iid, &mut obj) };
            // SAFETY: as above.
            let msg = (r == kResultOk)
                .then(|| unsafe { ComPtr::from_raw(obj as *mut IMessage) })
                .flatten();
            if let Some(msg) = msg {
                // SAFETY: plain calls on the host's live message and its
                // attribute list (owned by the message).
                unsafe {
                    msg.setMessageID(c"hello".as_ptr());
                    if let Some(attrs) = ComRef::from_raw(msg.getAttributes()) {
                        attrs.setInt(c"value".as_ptr(), 42);
                    }
                    peer.notify(msg.as_ptr());
                }
            }
        }
        if let Ok(mut g) = self.peer.lock() {
            *g = Some(peer);
        }
        kResultOk
    }

    unsafe fn disconnect(&self, _other: *mut IConnectionPoint) -> tresult {
        bump(ix::DISCONNECTS);
        stamp(ix::LAST_DISCONNECT);
        if let Ok(mut g) = self.peer.lock() {
            *g = None;
        }
        kResultOk
    }

    unsafe fn notify(&self, _message: *mut IMessage) -> tresult {
        kResultOk
    }
}

/// Test Gain's controller class.
struct Controller {
    ctl: Ctl,
    peer: Mutex<Option<ComPtr<IConnectionPoint>>>,
}

impl Drop for Controller {
    fn drop(&mut self) {
        bump(ix::CONTROLLER_RELEASES);
        stamp(ix::LAST_CONTROLLER_RELEASE);
    }
}

impl Class for Controller {
    type Interfaces = (IEditController, IConnectionPoint);
}

impl IPluginBaseTrait for Controller {
    unsafe fn initialize(&self, _context: *mut FUnknown) -> tresult {
        self.ctl.check();
        bump(ix::CONTROLLER_INITS);
        if mode() == MODE_CONTROLLER_INIT_FAILS {
            return kResultFalse;
        }
        kResultOk
    }

    unsafe fn terminate(&self) -> tresult {
        self.ctl.check();
        bump(ix::CONTROLLER_TERMS);
        stamp(ix::LAST_CONTROLLER_TERM);
        self.ctl.drop_handler();
        kResultOk
    }
}

edit_controller!(Controller);

impl IConnectionPointTrait for Controller {
    unsafe fn connect(&self, other: *mut IConnectionPoint) -> tresult {
        self.ctl.check();
        // SAFETY: the host passes the peer, or null.
        let Some(peer) = (unsafe { ComRef::from_raw(other) }).map(|r| r.to_com_ptr()) else {
            return kInvalidArgument;
        };
        bump(ix::CONNECTS);
        if let Ok(mut g) = self.peer.lock() {
            *g = Some(peer);
        }
        kResultOk
    }

    unsafe fn disconnect(&self, _other: *mut IConnectionPoint) -> tresult {
        self.ctl.check();
        bump(ix::DISCONNECTS);
        stamp(ix::LAST_DISCONNECT);
        if let Ok(mut g) = self.peer.lock() {
            *g = None;
        }
        kResultOk
    }

    unsafe fn notify(&self, message: *mut IMessage) -> tresult {
        self.ctl.check();
        // SAFETY: the sender's live message, for this call.
        let Some(msg) = (unsafe { ComRef::from_raw(message) }) else {
            return kInvalidArgument;
        };
        // SAFETY: plain calls on the message and its attribute list.
        unsafe {
            let id = msg.getMessageID();
            if id.is_null() || CStr::from_ptr(id) != c"hello" {
                return kResultFalse;
            }
            let mut v: i64 = 0;
            if let Some(attrs) = ComRef::from_raw(msg.getAttributes()) {
                if attrs.getInt(c"value".as_ptr(), &mut v) == kResultOk && v == 42 {
                    bump(ix::MESSAGES);
                }
            }
        }
        kResultOk
    }
}

/// Test Synth: a single component (its own controller).
struct Synth {
    ctl: Ctl,
}

impl Drop for Synth {
    fn drop(&mut self) {
        bump(ix::COMPONENT_RELEASES);
        stamp(ix::LAST_COMPONENT_RELEASE);
    }
}

impl Class for Synth {
    type Interfaces = (IComponent, IAudioProcessor, IEditController);
}

impl IPluginBaseTrait for Synth {
    unsafe fn initialize(&self, context: *mut FUnknown) -> tresult {
        self.ctl.check();
        component_initialize(context).0
    }

    unsafe fn terminate(&self) -> tresult {
        self.ctl.check();
        component_terminate();
        self.ctl.drop_handler();
        kResultOk
    }
}

impl IComponentTrait for Synth {
    unsafe fn getControllerClassId(&self, _class_id: *mut TUID) -> tresult {
        kResultFalse
    }

    unsafe fn getBusCount(&self, media: MediaType, dir: BusDirection) -> int32 {
        bus_count(&SYNTH_BUSES, media, dir)
    }

    unsafe fn getBusInfo(
        &self,
        media: MediaType,
        dir: BusDirection,
        index: int32,
        bus: *mut BusInfo,
    ) -> tresult {
        bus_info(&SYNTH_BUSES, media, dir, index, bus)
    }

    unsafe fn activateBus(
        &self,
        media: MediaType,
        dir: BusDirection,
        index: int32,
        _state: TBool,
    ) -> tresult {
        if index >= 0 && index < bus_count(&SYNTH_BUSES, media, dir) {
            kResultOk
        } else {
            kInvalidArgument
        }
    }

    unsafe fn getState(&self, _state: *mut IBStream) -> tresult {
        kResultOk
    }

    component_common!();
}

audio_processor!(Synth);
edit_controller!(Synth);

struct Factory;

impl Drop for Factory {
    fn drop(&mut self) {
        bump(ix::FACTORY_RELEASES);
        stamp(ix::LAST_FACTORY_RELEASE);
    }
}

impl Class for Factory {
    type Interfaces = (IPluginFactory3,);
}

fn class(index: int32) -> Option<&'static ClassDef> {
    usize::try_from(index).ok().and_then(|i| CLASSES.get(i))
}

impl IPluginFactoryTrait for Factory {
    unsafe fn getFactoryInfo(&self, info: *mut PFactoryInfo) -> tresult {
        // SAFETY: the host passes a valid struct to fill.
        let Some(info) = (unsafe { info.as_mut() }) else {
            return kInvalidArgument;
        };
        copy_str(VENDOR, &mut info.vendor);
        copy_str("", &mut info.url);
        copy_str("", &mut info.email);
        info.flags = FactoryFlags_::kUnicode as int32;
        kResultOk
    }

    unsafe fn countClasses(&self) -> int32 {
        CLASSES.len() as int32
    }

    unsafe fn getClassInfo(&self, index: int32, info: *mut PClassInfo) -> tresult {
        // SAFETY: the host passes a valid struct to fill.
        let (Some(c), Some(info)) = (class(index), unsafe { info.as_mut() }) else {
            return kInvalidArgument;
        };
        info.cid = c.cid;
        info.cardinality = ClassCardinality_::kManyInstances as int32;
        copy_str(c.category, &mut info.category);
        copy_str(c.name, &mut info.name);
        kResultOk
    }

    unsafe fn createInstance(
        &self,
        cid: FIDString,
        iid: FIDString,
        obj: *mut *mut c_void,
    ) -> tresult {
        if cid.is_null() || iid.is_null() || obj.is_null() {
            return kInvalidArgument;
        }
        // SAFETY: both IDs point at 16 bytes (the VST3 calling convention).
        let cid = unsafe { *(cid as *const TUID) };
        // SAFETY: as above.
        let iid = unsafe { *(iid as *const [u8; 16]) };
        // SAFETY: `obj` is the host's out pointer.
        unsafe { *obj = std::ptr::null_mut() };
        if cid == CRASHER {
            // SAFETY: undefined behaviour by design: a deliberate null write
            // that raises SIGSEGV, for the scanner's crash tests.
            unsafe { std::ptr::null_mut::<u8>().write_volatile(1) };
        }
        if cid == HANGER {
            loop {
                std::thread::sleep(std::time::Duration::from_secs(1));
            }
        }
        mark_thread();
        let made = if cid == GAIN {
            ComWrapper::new(Component {
                host: Mutex::new(None),
                peer: Mutex::new(None),
            })
            .to_com_ptr::<FUnknown>()
        } else if cid == SYNTH {
            ComWrapper::new(Synth { ctl: Ctl::new() }).to_com_ptr::<FUnknown>()
        } else if cid == CONTROLLER {
            ComWrapper::new(Controller {
                ctl: Ctl::new(),
                peer: Mutex::new(None),
            })
            .to_com_ptr::<FUnknown>()
        } else {
            return kInvalidArgument;
        };
        hand_out(made, &iid, obj)
    }
}

impl IPluginFactory2Trait for Factory {
    unsafe fn getClassInfo2(&self, index: int32, info: *mut PClassInfo2) -> tresult {
        // SAFETY: the host passes a valid struct to fill.
        let (Some(c), Some(info)) = (class(index), unsafe { info.as_mut() }) else {
            return kInvalidArgument;
        };
        info.cid = c.cid;
        info.cardinality = ClassCardinality_::kManyInstances as int32;
        copy_str(c.category, &mut info.category);
        copy_str(c.name, &mut info.name);
        info.classFlags = 0;
        copy_str(c.sub_categories, &mut info.subCategories);
        copy_str(VENDOR, &mut info.vendor);
        copy_str(VERSION, &mut info.version);
        copy_str(SDK_VERSION, &mut info.sdkVersion);
        kResultOk
    }
}

impl IPluginFactory3Trait for Factory {
    unsafe fn getClassInfoUnicode(&self, index: int32, info: *mut PClassInfoW) -> tresult {
        // SAFETY: the host passes a valid struct to fill.
        let (Some(c), Some(info)) = (class(index), unsafe { info.as_mut() }) else {
            return kInvalidArgument;
        };
        info.cid = c.cid;
        info.cardinality = ClassCardinality_::kManyInstances as int32;
        copy_str(c.category, &mut info.category);
        copy_wide(c.name, &mut info.name);
        info.classFlags = 0;
        copy_str(c.sub_categories, &mut info.subCategories);
        copy_wide(VENDOR, &mut info.vendor);
        copy_wide(VERSION, &mut info.version);
        copy_wide(SDK_VERSION, &mut info.sdkVersion);
        kResultOk
    }

    unsafe fn setHostContext(&self, context: *mut FUnknown) -> tresult {
        // SAFETY: the host's context, or null.
        let host = unsafe { ComRef::from_raw(context) }.and_then(|c| c.cast::<IHostApplication>());
        if host.is_some() {
            bump(ix::HOST_CONTEXTS);
        }
        kResultOk
    }
}

/// The module entry on macOS (the SDK's spelling).
#[cfg(target_os = "macos")]
#[unsafe(no_mangle)]
pub extern "system" fn bundleEntry(_bundle: *mut c_void) -> bool {
    enter()
}

/// The module exit on macOS.
#[cfg(target_os = "macos")]
#[unsafe(no_mangle)]
pub extern "system" fn bundleExit() -> bool {
    exit()
}

/// The module entry on Linux (gets the `dlopen` handle).
#[cfg(target_os = "linux")]
#[unsafe(no_mangle)]
pub extern "system" fn ModuleEntry(_handle: *mut c_void) -> bool {
    enter()
}

/// The module exit on Linux.
#[cfg(target_os = "linux")]
#[unsafe(no_mangle)]
pub extern "system" fn ModuleExit() -> bool {
    exit()
}

/// The factory, or null when the module entry has not run (as with an SDK
/// plug-in, whose factory needs the module initialised).
#[unsafe(no_mangle)]
pub extern "system" fn GetPluginFactory() -> *mut IPluginFactory {
    let needs_entry = cfg!(any(target_os = "macos", target_os = "linux"));
    if (needs_entry && ENTERED.load(Ordering::SeqCst) == 0) || mode() == MODE_NULL_FACTORY {
        return std::ptr::null_mut();
    }
    match ComWrapper::new(Factory).to_com_ptr::<IPluginFactory>() {
        Some(p) => p.into_raw(),
        None => std::ptr::null_mut(),
    }
}

/// How the test module misbehaves (see the module docs).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Vst3Mode {
    /// Behaves.
    Normal = 0,
    /// The module entry returns false.
    EntryFails = 1,
    /// A good entry, then `GetPluginFactory` returns null.
    NullFactory = 2,
    /// Components' `initialize` fails.
    ComponentInitFails = 3,
    /// The controller class's `initialize` fails (after the component's
    /// succeeded).
    ControllerInitFails = 4,
    /// `setupProcessing` refuses the offline process mode.
    OfflineRefused = 5,
}

/// A component-handler call the probe makes the latest controller do.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Vst3Edit {
    /// `beginEdit(id)`.
    Begin(u32),
    /// `performEdit(id, normalised value)`.
    Perform(u32, f64),
    /// `endEdit(id)`.
    End(u32),
    /// `restartComponent(flags)` (see [`vst3_restart`]).
    Restart(i32),
    /// `IComponentHandler2::setDirty`.
    Dirty(bool),
}

/// `restartComponent` flags (the SDK's `RestartFlags`).
pub mod vst3_restart {
    /// `kReloadComponent`.
    pub const RELOAD_COMPONENT: i32 = 1;
    /// `kIoChanged`.
    pub const IO_CHANGED: i32 = 2;
    /// `kParamValuesChanged`.
    pub const PARAM_VALUES_CHANGED: i32 = 4;
    /// `kLatencyChanged`.
    pub const LATENCY_CHANGED: i32 = 8;
    /// `kParamTitlesChanged`.
    pub const PARAM_TITLES_CHANGED: i32 = 16;
}

/// The module's lifecycle counters (see the module docs). The `last_*`
/// fields are sequence numbers of the latest such event (0: never), from
/// one clock, so they order events.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Vst3Counters {
    /// Module entries that succeeded.
    pub entries: u64,
    /// Module exits.
    pub exits: u64,
    /// Factories released.
    pub factory_releases: u64,
    /// Components released (a single component counts here only).
    pub component_releases: u64,
    /// When the latest factory was released.
    pub last_factory_release: u64,
    /// When the latest component was released.
    pub last_component_release: u64,
    /// When the latest exit ran.
    pub last_exit: u64,
    /// Controller-class objects released.
    pub controller_releases: u64,
    /// When the latest controller was released.
    pub last_controller_release: u64,
    /// Component `initialize` calls (also failed ones).
    pub component_inits: u64,
    /// Component `terminate` calls.
    pub component_terms: u64,
    /// When the latest component terminated.
    pub last_component_term: u64,
    /// Controller-class `initialize` calls (also failed ones).
    pub controller_inits: u64,
    /// Controller-class `terminate` calls.
    pub controller_terms: u64,
    /// When the latest controller terminated.
    pub last_controller_term: u64,
    /// `IConnectionPoint::connect` calls, either side.
    pub connects: u64,
    /// `IConnectionPoint::disconnect` calls, either side.
    pub disconnects: u64,
    /// When the latest disconnect ran.
    pub last_disconnect: u64,
    /// Hello messages the controller got from its component.
    pub messages: u64,
    /// Non-null `setComponentHandler` calls.
    pub handler_sets: u64,
    /// `setComponentState` calls that read the component's state.
    pub state_syncs: u64,
    /// Controller calls made on another thread than the one that created it.
    pub off_thread_calls: u64,
    /// `setHostContext` calls with an `IHostApplication`.
    pub host_contexts: u64,
    /// `setupProcessing` calls.
    pub setups: u64,
    /// The latest `setupProcessing` process mode (0 real time, 2 offline).
    pub last_process_mode: u64,
    /// Thread-exit destructors run (threads that created an object and
    /// then exited).
    pub thread_exits: u64,
}

/// A handle on the test module's loaded image, for reading its lifecycle
/// counters and setting its mode. It keeps the image loaded, so the
/// counters outlive the host's own load and unload. Open it on a private
/// copy of the bundle (`copy_vst3_bundle`) so parallel tests do not share
/// an image; open it before the host loads the bundle.
pub struct Vst3Probe(libloading::Library);

impl Vst3Probe {
    /// Opens the binary of the VST3 bundle at `bundle`. Does not run the
    /// module entry. Panics (test helper) when it cannot.
    pub fn open(bundle: &std::path::Path) -> Vst3Probe {
        let (dir, file) = if cfg!(target_os = "macos") {
            ("MacOS".to_string(), "VizDAW Test".to_string())
        } else {
            let stem = bundle.file_stem().unwrap().to_string_lossy().into_owned();
            (
                format!("{}-linux", std::env::consts::ARCH),
                format!("{stem}.so"),
            )
        };
        let binary = bundle.join("Contents").join(dir).join(file);
        // SAFETY: the library is this crate's own test module.
        Vst3Probe(unsafe { libloading::Library::new(&binary) }.unwrap())
    }

    /// Sets how the module behaves from now on.
    pub fn set_mode(&self, mode: Vst3Mode) {
        // SAFETY: the exported function takes a `u32`.
        let f = unsafe {
            self.0
                .get::<extern "C" fn(u32)>(b"vizdaw_test_vst3_set_mode\0")
        }
        .unwrap();
        f(mode as u32);
    }

    /// The counters now.
    pub fn counters(&self) -> Vst3Counters {
        // SAFETY: the exported function fills `len` values.
        let f = unsafe {
            self.0
                .get::<extern "C" fn(*mut u64, usize)>(b"vizdaw_test_vst3_counters\0")
        }
        .unwrap();
        let mut v = [0u64; ix::N];
        f(v.as_mut_ptr(), v.len());
        Vst3Counters {
            entries: v[ix::ENTRIES],
            exits: v[ix::EXITS],
            factory_releases: v[ix::FACTORY_RELEASES],
            component_releases: v[ix::COMPONENT_RELEASES],
            last_factory_release: v[ix::LAST_FACTORY_RELEASE],
            last_component_release: v[ix::LAST_COMPONENT_RELEASE],
            last_exit: v[ix::LAST_EXIT],
            controller_releases: v[ix::CONTROLLER_RELEASES],
            last_controller_release: v[ix::LAST_CONTROLLER_RELEASE],
            component_inits: v[ix::COMPONENT_INITS],
            component_terms: v[ix::COMPONENT_TERMS],
            last_component_term: v[ix::LAST_COMPONENT_TERM],
            controller_inits: v[ix::CONTROLLER_INITS],
            controller_terms: v[ix::CONTROLLER_TERMS],
            last_controller_term: v[ix::LAST_CONTROLLER_TERM],
            connects: v[ix::CONNECTS],
            disconnects: v[ix::DISCONNECTS],
            last_disconnect: v[ix::LAST_DISCONNECT],
            messages: v[ix::MESSAGES],
            handler_sets: v[ix::HANDLER_SETS],
            state_syncs: v[ix::STATE_SYNCS],
            off_thread_calls: v[ix::OFF_THREAD],
            host_contexts: v[ix::HOST_CONTEXTS],
            setups: v[ix::SETUPS],
            last_process_mode: v[ix::LAST_PROCESS_MODE],
            thread_exits: v[ix::THREAD_EXITS],
        }
    }

    fn call(&self, e: Vst3Edit, off_thread: bool) -> i32 {
        // SAFETY: the exported function's signature.
        let f = unsafe {
            self.0
                .get::<extern "C" fn(u32, u32, f64, i32, u8) -> i32>(b"vizdaw_test_vst3_edit\0")
        }
        .unwrap();
        let (kind, id, value, flags) = match e {
            Vst3Edit::Begin(id) => (0, id, 0.0, 0),
            Vst3Edit::Perform(id, v) => (1, id, v, 0),
            Vst3Edit::End(id) => (2, id, 0.0, 0),
            Vst3Edit::Restart(flags) => (3, 0, 0.0, flags),
            Vst3Edit::Dirty(on) => (4, 0, 0.0, i32::from(on)),
        };
        f(kind, id, value, flags, u8::from(off_thread))
    }

    /// Makes the latest controller call its handler on this thread;
    /// returns the handler's result (-1: no handler).
    pub fn edit(&self, e: Vst3Edit) -> i32 {
        self.call(e, false)
    }

    /// As [`Self::edit`], from a new thread (as a plug-in's processing
    /// thread would).
    pub fn edit_off_thread(&self, e: Vst3Edit) -> i32 {
        self.call(e, true)
    }
}
