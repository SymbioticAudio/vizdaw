//! The VST3 side of the test binary (host track H1): the same library
//! exports `GetPluginFactory` beside `clap_entry`, and [`crate::vst3_bundle`]
//! wraps it as `VizDAW Test.vst3`. It serves the scanner and instance
//! tests: a factory (`IPluginFactory3`) listing several classes, a split
//! plug-in (component and a separate controller class, connected through
//! `IConnectionPoint`) and a single-component one (the component is its
//! own controller). The components process audio (host track H1 Task 4)
//! and report what the host gave them through the lifecycle counters.
//!
//! | class ID | name | sub-categories | what it does |
//! |---|---|---|---|
//! | [`VST3_GAIN_ID`] | Test Gain | `Fx` | split: a component (stereo main in and out, a stereo sidechain in) and the [`VST3_CONTROLLER_ID`] controller with the [`VST3_PARAMS`]; output = input × Gain (dB) + sidechain; reports Meter (the output's peak) and echoes each Mode value as Secret through its output parameter changes |
//! | [`VST3_SYNTH_ID`] | Test Synth | `Instrument\|Synth` | single component: an event input, two stereo outputs, the same parameters; output 1 is 0.5 while a note is held, output 2 always 0.25; `IMidiMapping` maps CC 1, pitch bend and channel aftertouch to Mod, Bend and Pressure |
//! | [`VST3_CRASHER_ID`] | Test Crasher | `Fx` | crashes (SIGSEGV) in `createInstance` |
//! | [`VST3_HANGER_ID`] | Test Hanger | `Fx` | never returns from `createInstance` |
//! | [`VST3_PROCESS_CRASHER_ID`] | Test Process Crasher | `Fx` | Test Gain that crashes (SIGSEGV) in `process` |
//! | [`VST3_NAN_ID`] | Test NaN | `Fx` | Test Gain whose first output sample of every block is NaN |
//! | [`VST3_BANK_ID`] | Test Bank | `Fx` | single component: Test Gain's buses and parameters plus [`VST3_BANK_SIZE`] more (IDs from [`VST3_BANK_BASE`], plain = normalised), all in its component state |
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
//! The latency and tail every component reports are set through the
//! probe ([`crate::Vst3Probe::set_timing`]).
//!
//! A component's state is its gain (a normalised value, 8 bytes
//! little-endian; [`VST3_COMPONENT_GAIN`] until the processor is given
//! another); a controller given it through `setComponentState` takes it
//! as its Gain value. A controller's own state (`getState`/`setState`) is
//! a tag and its memo, a controller-only value that no parameter and no
//! component state carries (set for new controllers through
//! [`crate::Vst3Probe::set_memo`]), so a round trip of both proves both
//! streams. Both `setState`s refuse anything else and then change
//! nothing. When connected, the
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
use std::sync::atomic::{AtomicBool, AtomicI32, AtomicU32, AtomicU64, Ordering};
use std::thread::ThreadId;
// The bindings' module (named after the SDK's vendor) is only named here.
use vst3::Steinberg as sdk;
use vst3::com_scrape_types::Unknown;
use vst3::{Class, ComPtr, ComRef, ComWrapper, Interface, uid};

use sdk::IBStream_::IStreamSeekMode_;
use sdk::PClassInfo_::ClassCardinality_;
use sdk::PFactoryInfo_::FactoryFlags_;
use sdk::Vst::BusInfo_::BusFlags_;
use sdk::Vst::Event_::EventTypes_;
use sdk::Vst::ParameterInfo_::ParameterFlags_;
use sdk::Vst::{
    AudioBusBuffers, BusDirection, BusDirections_, BusInfo, BusType, BusTypes_, ControllerNumbers_,
    CtrlNumber, Event, IAttributeListTrait, IAudioProcessor, IAudioProcessorTrait, IComponent,
    IComponentHandler, IComponentHandler2, IComponentHandler2Trait, IComponentHandlerTrait,
    IComponentTrait, IConnectionPoint, IConnectionPointTrait, IEditController,
    IEditControllerTrait, IEventList, IEventListTrait, IHostApplication, IHostApplicationTrait,
    IMessage, IMessageTrait, IMidiMapping, IMidiMappingTrait, IParamValueQueueTrait,
    IParameterChanges, IParameterChangesTrait, IoMode, MediaType, MediaTypes_, ParamID, ParamValue,
    ParameterInfo, ProcessData, ProcessModes_, ProcessSetup, RoutingInfo, SpeakerArrangement,
    String128, SymbolicSampleSizes_, TChar,
};
use sdk::{
    FIDString, FUnknown, IBStream, IBStreamTrait, IPlugView, IPluginBaseTrait, IPluginFactory,
    IPluginFactory2Trait, IPluginFactory3, IPluginFactory3Trait, IPluginFactoryTrait, PClassInfo,
    PClassInfo2, PClassInfoW, PFactoryInfo, TBool, TUID, int16, int32, kInvalidArgument,
    kNoInterface, kNotImplemented, kResultFalse, kResultOk, kResultTrue, tresult, uint32,
};

/// Class ID of "Test Gain" (an effect), as the host spells it.
pub const VST3_GAIN_ID: &str = "56495A44565354337E570000ABCD0001";
/// Class ID of "Test Synth" (an instrument).
pub const VST3_SYNTH_ID: &str = "56495A44565354337E570000ABCD0002";
/// Class ID of "Test Crasher" (crashes when created).
pub const VST3_CRASHER_ID: &str = "56495A44565354337E570000ABCD0003";
/// Class ID of "Test Hanger" (hangs when created).
pub const VST3_HANGER_ID: &str = "56495A44565354337E570000ABCD0004";
/// Class ID of "Test Process Crasher" (Test Gain that crashes in `process`).
pub const VST3_PROCESS_CRASHER_ID: &str = "56495A44565354337E570000ABCD0005";
/// Class ID of "Test NaN" (Test Gain that writes NaN audio).
pub const VST3_NAN_ID: &str = "56495A44565354337E570000ABCD0006";
/// Class ID of "Test Bank" (many parameters).
pub const VST3_BANK_ID: &str = "56495A44565354337E570000ABCD0007";
/// The first `ParamID` of Test Bank's extra parameters ("Bank 0"…).
pub const VST3_BANK_BASE: u32 = 1000;
/// How many extra parameters Test Bank has.
pub const VST3_BANK_SIZE: usize = 256;
/// Class ID of the controller class (not a plug-in, never listed).
pub const VST3_CONTROLLER_ID: &str = "56495A44565354337E570000ABCD00FF";

const GAIN: TUID = uid(0x5649_5A44, 0x5653_5433, 0x7E57_0000, 0xABCD_0001);
const SYNTH: TUID = uid(0x5649_5A44, 0x5653_5433, 0x7E57_0000, 0xABCD_0002);
const CRASHER: TUID = uid(0x5649_5A44, 0x5653_5433, 0x7E57_0000, 0xABCD_0003);
const HANGER: TUID = uid(0x5649_5A44, 0x5653_5433, 0x7E57_0000, 0xABCD_0004);
const PROCESS_CRASHER: TUID = uid(0x5649_5A44, 0x5653_5433, 0x7E57_0000, 0xABCD_0005);
const NAN: TUID = uid(0x5649_5A44, 0x5653_5433, 0x7E57_0000, 0xABCD_0006);
const BANK: TUID = uid(0x5649_5A44, 0x5653_5433, 0x7E57_0000, 0xABCD_0007);
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
/// `ParamID` of Mod (hidden): Test Synth maps MIDI CC 1 to it.
pub const VST3_PARAM_MOD: u32 = 14;
/// `ParamID` of Bend (hidden): Test Synth maps pitch bend to it.
pub const VST3_PARAM_BEND: u32 = 15;
/// `ParamID` of Pressure (hidden): Test Synth maps channel aftertouch to it.
pub const VST3_PARAM_PRESSURE: u32 = 16;

/// The test controllers' parameters, in their order.
pub const VST3_PARAMS: [Vst3ParamDef; 7] = [
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
    Vst3ParamDef {
        id: VST3_PARAM_MOD,
        title: "Mod",
        units: "",
        steps: 0,
        default: 0.0,
        flags: ParameterFlags_::kIsHidden,
        min: 0.0,
        max: 1.0,
    },
    Vst3ParamDef {
        id: VST3_PARAM_BEND,
        title: "Bend",
        units: "",
        steps: 0,
        default: 0.5,
        flags: ParameterFlags_::kIsHidden,
        min: 0.0,
        max: 1.0,
    },
    Vst3ParamDef {
        id: VST3_PARAM_PRESSURE,
        title: "Pressure",
        units: "",
        steps: 0,
        default: 0.0,
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

const CLASSES: [ClassDef; 8] = [
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
    ClassDef {
        cid: PROCESS_CRASHER,
        category: AUDIO_MODULE,
        name: "Test Process Crasher",
        sub_categories: "Fx",
    },
    ClassDef {
        cid: NAN,
        category: AUDIO_MODULE,
        name: "Test NaN",
        sub_categories: "Fx",
    },
    ClassDef {
        cid: BANK,
        category: AUDIO_MODULE,
        name: "Test Bank",
        sub_categories: "Fx",
    },
];

/// The index of Test Bank's extra parameter `id`, if it is one.
fn bank_index(id: ParamID) -> Option<usize> {
    id.checked_sub(VST3_BANK_BASE)
        .and_then(|k| usize::try_from(k).ok())
        .filter(|k| *k < VST3_BANK_SIZE)
}

/// Atomics for Test Bank's extra values (none for the other classes).
fn bank_cells(bank: bool) -> Box<[AtomicU64]> {
    let n = if bank { VST3_BANK_SIZE } else { 0 };
    (0..n).map(|_| AtomicU64::new(0)).collect()
}

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
    pub const SET_ACTIVE_ON: usize = 26;
    pub const SET_ACTIVE_OFF: usize = 27;
    pub const LAST_SET_ACTIVE_ON: usize = 28;
    pub const LAST_SET_ACTIVE_OFF: usize = 29;
    pub const PROCESSING_ON: usize = 30;
    pub const PROCESSING_OFF: usize = 31;
    pub const LAST_PROCESSING_ON: usize = 32;
    pub const LAST_PROCESSING_OFF: usize = 33;
    pub const LAST_SETUP: usize = 34;
    pub const LAST_SETUP_RATE: usize = 35;
    pub const LAST_SETUP_BLOCK: usize = 36;
    pub const ARRANGEMENTS: usize = 37;
    pub const BUSES_ON: usize = 38;
    pub const PROCESS_CALLS: usize = 39;
    pub const FIRST_PROCESS: usize = 40;
    pub const BAD_PROCESS: usize = 41;
    pub const LAST_FRAMES: usize = 42;
    pub const LAST_DATA_MODE: usize = 43;
    pub const LAST_TEMPO: usize = 44;
    pub const LAST_MUSIC: usize = 45;
    pub const LAST_BAR: usize = 46;
    pub const LAST_STATE: usize = 47;
    pub const LAST_PROJECT_SAMPLES: usize = 48;
    pub const LAST_CONT_SAMPLES: usize = 49;
    pub const PARAM_POINTS: usize = 50;
    pub const PARAM_SEEN: usize = 51;
    pub const LAST_PARAM_OFFSET: usize = 52;
    /// One per entry of `VST3_PARAMS` (7).
    pub const PARAM_LAST: usize = 53;
    pub const NOTE_ONS: usize = 60;
    pub const NOTE_OFFS: usize = 61;
    pub const POLY_PRESSURES: usize = 62;
    pub const LAST_NOTE: usize = 63;
    pub const LAST_VELOCITY: usize = 64;
    pub const SIDECHAIN_BLOCKS: usize = 65;
    pub const CLAP_INITS: usize = 66;
    pub const CLAP_DEINITS: usize = 67;
    pub const LAST_CONTEXT_RATE: usize = 68;
    pub const SIDECHAIN_ON: usize = 69;
    pub const LAST_ARRANGEMENT: usize = 70;
    pub const COMPONENT_SET_STATES: usize = 71;
    pub const LAST_COMPONENT_SET_STATE: usize = 72;
    pub const LAST_STATE_SYNC: usize = 73;
    pub const CONTROLLER_SET_STATES: usize = 74;
    pub const LAST_CONTROLLER_SET_STATE: usize = 75;
    pub const LAST_MEMO: usize = 76;
    pub const N: usize = 77;
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

/// The CLAP entry's `init` (the same library is the CLAP test bundle).
pub(crate) fn clap_init() {
    bump(ix::CLAP_INITS);
}

/// The CLAP entry's `deinit`.
pub(crate) fn clap_deinit() {
    bump(ix::CLAP_DEINITS);
}

/// The latency every component reports (`getLatencySamples`).
static LATENCY: AtomicU32 = AtomicU32::new(0);
/// The tail every component reports (`getTailSamples`).
static TAIL: AtomicU32 = AtomicU32::new(0);

/// Sets the latency and tail every component reports from now on.
#[unsafe(no_mangle)]
pub extern "C" fn vizdaw_test_vst3_set_timing(latency: u32, tail: u32) {
    LATENCY.store(latency, Ordering::SeqCst);
    TAIL.store(tail, Ordering::SeqCst);
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

/// The controller-only value ("memo") controllers made from now on start
/// with (see the module docs).
static NEXT_MEMO: AtomicU64 = AtomicU64::new(0);

/// Sets the memo controllers made from now on start with.
#[unsafe(no_mangle)]
pub extern "C" fn vizdaw_test_vst3_set_memo(memo: u64) {
    NEXT_MEMO.store(memo, Ordering::SeqCst);
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
const MODE_REALTIME_REFUSED: u32 = 6;
const MODE_NAN_OUTPUT: u32 = 7;
const MODE_COMPONENT_STATE_REFUSED: u32 = 8;
const MODE_CONTROLLER_STATE_REFUSED: u32 = 9;
const MODE_STATE_SEEKS_PAST_END: u32 = 10;

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
pub(crate) fn mark_thread() {
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
pub(crate) fn mark_thread() {}

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
    values: [AtomicU64; VST3_PARAMS.len()],
    /// Test Bank's extra values (empty for the other classes).
    bank: Box<[AtomicU64]>,
    /// A controller-only value (no parameter, not in the component's
    /// state): only the controller's own state carries it.
    memo: AtomicU64,
    handler: Mutex<Option<ComPtr<IComponentHandler>>>,
    creator: ThreadId,
}

impl Ctl {
    fn new(bank: bool) -> Ctl {
        Ctl {
            values: VST3_PARAMS.map(|p| AtomicU64::new(p.default.to_bits())),
            bank: bank_cells(bank),
            memo: AtomicU64::new(NEXT_MEMO.load(Ordering::SeqCst)),
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

    /// The cell of parameter `id`, if this controller has it.
    fn cell(&self, id: ParamID) -> Option<&AtomicU64> {
        match bank_index(id) {
            Some(k) => self.bank.get(k),
            None => param(id).and_then(|(i, _)| self.values.get(i)),
        }
    }

    fn count(&self) -> int32 {
        (VST3_PARAMS.len() + self.bank.len()) as int32
    }

    /// Fills `info` for parameter `index` (the bank's after the others).
    fn info(&self, index: int32, info: &mut ParameterInfo) -> tresult {
        let Ok(i) = usize::try_from(index) else {
            return kInvalidArgument;
        };
        if let Some(p) = VST3_PARAMS.get(i) {
            info.id = p.id;
            copy_wide(p.title, &mut info.title);
            copy_wide(p.title, &mut info.shortTitle);
            copy_wide(p.units, &mut info.units);
            info.stepCount = p.steps;
            info.defaultNormalizedValue = p.default;
            info.unitId = 0;
            info.flags = p.flags;
            return kResultOk;
        }
        let k = i - VST3_PARAMS.len();
        if k >= self.bank.len() {
            return kInvalidArgument;
        }
        info.id = VST3_BANK_BASE + k as u32;
        let title = format!("Bank {k}");
        copy_wide(&title, &mut info.title);
        copy_wide(&title, &mut info.shortTitle);
        copy_wide("", &mut info.units);
        info.stepCount = 0;
        info.defaultNormalizedValue = 0.0;
        info.unitId = 0;
        info.flags = ParameterFlags_::kCanAutomate;
        kResultOk
    }

    fn get(&self, id: ParamID) -> f64 {
        self.cell(id)
            .map_or(0.0, |v| f64::from_bits(v.load(Ordering::SeqCst)))
    }

    fn set(&self, id: ParamID, n: f64) -> tresult {
        match self.cell(id) {
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
        let gain = f64::from_le_bytes(b);
        // Test Bank's values follow the gain.
        for cell in self.bank.iter() {
            // SAFETY: `b` has room for 8 bytes.
            let r = unsafe { s.read(b.as_mut_ptr() as *mut c_void, 8, &mut n) };
            if r != kResultOk || n != 8 {
                return kResultFalse;
            }
            cell.store(f64::from_le_bytes(b).to_bits(), Ordering::SeqCst);
        }
        bump(ix::STATE_SYNCS);
        stamp(ix::LAST_STATE_SYNC);
        self.set(VST3_PARAM_GAIN, gain)
    }

    /// The controller's own state: [`CONTROLLER_STATE_TAG`] and the memo
    /// (8 bytes little-endian).
    fn get_state(&self, state: *mut IBStream) -> tresult {
        // SAFETY: the host's stream, for this call.
        let Some(s) = (unsafe { ComRef::from_raw(state) }) else {
            return kInvalidArgument;
        };
        let mut b = [0u8; 12];
        b[..4].copy_from_slice(CONTROLLER_STATE_TAG);
        b[4..].copy_from_slice(&self.memo.load(Ordering::SeqCst).to_le_bytes());
        let mut n: int32 = 0;
        // SAFETY: `b` holds 12 bytes.
        let r = unsafe { s.write(b.as_mut_ptr() as *mut c_void, 12, &mut n) };
        if r == kResultOk && n == 12 {
            kResultOk
        } else {
            kResultFalse
        }
    }

    /// Takes the controller's own state ([`Ctl::get_state`]); anything
    /// else is refused and changes nothing.
    fn set_state(&self, state: *mut IBStream) -> tresult {
        bump(ix::CONTROLLER_SET_STATES);
        stamp(ix::LAST_CONTROLLER_SET_STATE);
        if mode() == MODE_CONTROLLER_STATE_REFUSED {
            return kResultFalse;
        }
        // SAFETY: the host's stream, for this call.
        let Some(s) = (unsafe { ComRef::from_raw(state) }) else {
            return kInvalidArgument;
        };
        let mut b = [0u8; 12];
        let mut n: int32 = 0;
        // SAFETY: `b` has room for 12 bytes.
        let r = unsafe { s.read(b.as_mut_ptr() as *mut c_void, 12, &mut n) };
        if r != kResultOk || n != 12 || &b[..4] != CONTROLLER_STATE_TAG {
            return kResultFalse;
        }
        let memo = u64::from_le_bytes(b[4..].try_into().unwrap_or_default());
        self.memo.store(memo, Ordering::SeqCst);
        store(ix::LAST_MEMO, memo);
        kResultOk
    }
}

/// The first bytes of a controller's own state.
const CONTROLLER_STATE_TAG: &[u8; 4] = b"VZC1";
/// What [`Vst3Mode::StateSeeksPastEnd`] writes after the gap.
const SEEK_TAG: &[u8; 4] = b"VZND";

/// Implements `IEditController` over a `ctl: Ctl` field.
macro_rules! edit_controller {
    ($t:ty) => {
        impl IEditControllerTrait for $t {
            unsafe fn setComponentState(&self, state: *mut IBStream) -> tresult {
                self.ctl.check();
                self.ctl.take_component_state(state)
            }

            unsafe fn setState(&self, state: *mut IBStream) -> tresult {
                self.ctl.check();
                self.ctl.set_state(state)
            }

            unsafe fn getState(&self, state: *mut IBStream) -> tresult {
                self.ctl.check();
                self.ctl.get_state(state)
            }

            unsafe fn getParameterCount(&self) -> int32 {
                self.ctl.check();
                self.ctl.count()
            }

            unsafe fn getParameterInfo(&self, index: int32, info: *mut ParameterInfo) -> tresult {
                self.ctl.check();
                // SAFETY: the host passes a struct to fill.
                match unsafe { info.as_mut() } {
                    Some(info) => self.ctl.info(index, info),
                    None => kInvalidArgument,
                }
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

/// A raw channel pointer of bus `b` (of `count`), or `None`.
fn channel_ptr(buses: *mut AudioBusBuffers, count: int32, b: usize, ch: usize) -> Option<*mut f32> {
    if b >= usize::try_from(count).unwrap_or(0) || buses.is_null() {
        return None;
    }
    // SAFETY: the host passes `count` buses at `buses`.
    let bus = unsafe { &*buses.add(b) };
    if ch >= usize::try_from(bus.numChannels).unwrap_or(0) {
        return None;
    }
    // SAFETY: 32-bit processing, the only sample size this module takes.
    let chans = unsafe { bus.__field0.channelBuffers32 };
    if chans.is_null() {
        return None;
    }
    // SAFETY: the bus has `numChannels` channel pointers.
    let p = unsafe { *chans.add(ch) };
    (!p.is_null()).then_some(p)
}

/// `Event::type` values (the SDK's `EventTypes`).
const NOTE_ON: u16 = EventTypes_::kNoteOnEvent as u16;
const NOTE_OFF: u16 = EventTypes_::kNoteOffEvent as u16;
const POLY_PRESSURE: u16 = EventTypes_::kPolyPressureEvent as u16;

/// Linear gain of a normalised Gain value (dB).
fn gain_factor(n: f64) -> f32 {
    10f64.powf(VST3_PARAMS[0].to_plain(n) / 20.0) as f32
}

/// A component's processing state (`IAudioProcessor` and the
/// `IComponent` calls around it).
struct Proc {
    /// Gain, normalised (Test Gain's audio).
    gain: AtomicU64,
    active: AtomicBool,
    processing: AtomicBool,
    /// Notes held (Test Synth's audio).
    notes: AtomicI32,
    synth: bool,
    /// Crashes in `process` (Test Process Crasher).
    crash: bool,
    /// Writes NaN as each output channel's first sample (Test NaN).
    nan: bool,
    /// Test Bank's extra values, as the processor was given them (empty
    /// for the other classes); in its state after the gain.
    bank: Box<[AtomicU64]>,
}

impl Proc {
    fn new(synth: bool, crash: bool, nan: bool, bank: bool) -> Proc {
        Proc {
            gain: AtomicU64::new(VST3_COMPONENT_GAIN.to_bits()),
            active: AtomicBool::new(false),
            processing: AtomicBool::new(false),
            notes: AtomicI32::new(0),
            synth,
            crash,
            nan,
            bank: bank_cells(bank),
        }
    }

    fn set_active(&self, on: TBool) -> tresult {
        let on = on != 0;
        self.active.store(on, Ordering::SeqCst);
        if on {
            bump(ix::SET_ACTIVE_ON);
            stamp(ix::LAST_SET_ACTIVE_ON);
        } else {
            bump(ix::SET_ACTIVE_OFF);
            stamp(ix::LAST_SET_ACTIVE_OFF);
            self.notes.store(0, Ordering::SeqCst);
        }
        kResultOk
    }

    /// The component's state: its gain, normalised (8 bytes
    /// little-endian); under [`Vst3Mode::StateSeeksPastEnd`] then a seek
    /// 4 bytes past the end and [`SEEK_TAG`] (16 bytes, a zero gap).
    fn get_state(&self, state: *mut IBStream) -> tresult {
        // SAFETY: the host's stream, for this call.
        let Some(s) = (unsafe { ComRef::from_raw(state) }) else {
            return kInvalidArgument;
        };
        let mut b = f64::from_bits(self.gain.load(Ordering::SeqCst)).to_le_bytes();
        let mut n: int32 = 0;
        // SAFETY: `b` holds 8 bytes.
        let r = unsafe { s.write(b.as_mut_ptr() as *mut c_void, 8, &mut n) };
        if r != kResultOk || n != 8 {
            return kResultFalse;
        }
        for cell in self.bank.iter() {
            let mut b = f64::from_bits(cell.load(Ordering::SeqCst)).to_le_bytes();
            // SAFETY: `b` holds 8 bytes.
            let r = unsafe { s.write(b.as_mut_ptr() as *mut c_void, 8, &mut n) };
            if r != kResultOk || n != 8 {
                return kResultFalse;
            }
        }
        if mode() == MODE_STATE_SEEKS_PAST_END {
            let mut at = 0;
            let mut tag = *SEEK_TAG;
            // SAFETY: plain calls on the host's stream; `tag` holds 4 bytes.
            let ok = unsafe {
                s.seek(4, IStreamSeekMode_::kIBSeekCur as int32, &mut at) == kResultOk
                    && at == 12
                    && s.write(tag.as_mut_ptr() as *mut c_void, 4, &mut n) == kResultOk
                    && n == 4
            };
            if !ok {
                return kResultFalse;
            }
        }
        kResultOk
    }

    /// Takes a state [`Proc::get_state`] wrote; anything else (short, not
    /// a normalised value) is refused and changes nothing. Under
    /// [`Vst3Mode::StateSeeksPastEnd`] it also reads the gap and the tag,
    /// reads past the end (0 bytes) and seeks past it (to the end).
    fn set_state(&self, state: *mut IBStream) -> tresult {
        bump(ix::COMPONENT_SET_STATES);
        stamp(ix::LAST_COMPONENT_SET_STATE);
        if mode() == MODE_COMPONENT_STATE_REFUSED {
            return kResultFalse;
        }
        // SAFETY: the host's stream, for this call.
        let Some(s) = (unsafe { ComRef::from_raw(state) }) else {
            return kInvalidArgument;
        };
        let mut b = [0u8; 8];
        let mut n: int32 = 0;
        // SAFETY: `b` has room for 8 bytes.
        let r = unsafe { s.read(b.as_mut_ptr() as *mut c_void, 8, &mut n) };
        let gain = f64::from_le_bytes(b);
        if r != kResultOk || n != 8 || !(0.0..=1.0).contains(&gain) {
            return kResultFalse;
        }
        let mut bank = vec![0.0f64; self.bank.len()];
        for v in bank.iter_mut() {
            // SAFETY: `b` has room for 8 bytes.
            let r = unsafe { s.read(b.as_mut_ptr() as *mut c_void, 8, &mut n) };
            if r != kResultOk || n != 8 {
                return kResultFalse;
            }
            *v = f64::from_le_bytes(b);
        }
        if mode() == MODE_STATE_SEEKS_PAST_END {
            let mut rest = [0u8; 12];
            let mut at = 0;
            // SAFETY: plain calls on the host's stream; `rest` has room
            // for 12 bytes.
            let ok = unsafe {
                s.read(rest.as_mut_ptr() as *mut c_void, 12, &mut n) == kResultOk
                    && n == 8
                    && rest[..4] == [0; 4]
                    && &rest[4..8] == SEEK_TAG
                    && s.read(rest.as_mut_ptr() as *mut c_void, 4, &mut n) == kResultOk
                    && n == 0
                    && s.seek(100, IStreamSeekMode_::kIBSeekEnd as int32, &mut at) == kResultOk
                    && at == 16
            };
            if !ok {
                return kResultFalse;
            }
        }
        self.gain.store(gain.to_bits(), Ordering::SeqCst);
        for (cell, v) in self.bank.iter().zip(bank) {
            cell.store(v.to_bits(), Ordering::SeqCst);
        }
        kResultOk
    }

    fn set_processing(&self, on: TBool) -> tresult {
        let on = on != 0;
        self.processing.store(on, Ordering::SeqCst);
        if on {
            bump(ix::PROCESSING_ON);
            stamp(ix::LAST_PROCESSING_ON);
        } else {
            bump(ix::PROCESSING_OFF);
            stamp(ix::LAST_PROCESSING_OFF);
        }
        kResultOk
    }

    fn setup(&self, setup: *mut ProcessSetup) -> tresult {
        // SAFETY: the host's setup, for this call.
        let Some(s) = (unsafe { setup.as_ref() }) else {
            return kInvalidArgument;
        };
        bump(ix::SETUPS);
        stamp(ix::LAST_SETUP);
        if self.active.load(Ordering::SeqCst) {
            // The SDK allows `setupProcessing` only while inactive.
            bump(ix::BAD_PROCESS);
        }
        store(ix::LAST_PROCESS_MODE, s.processMode as u64);
        store(ix::LAST_SETUP_RATE, s.sampleRate.to_bits());
        store(ix::LAST_SETUP_BLOCK, s.maxSamplesPerBlock as u64);
        let refused = match mode() {
            MODE_OFFLINE_REFUSED => Some(ProcessModes_::kOffline as int32),
            MODE_REALTIME_REFUSED => Some(ProcessModes_::kRealtime as int32),
            _ => None,
        };
        if refused == Some(s.processMode) {
            return kResultFalse;
        }
        kResultOk
    }

    /// Reads the input parameter changes into the counters; returns the
    /// last Mode value, if one came.
    fn read_params(&self, changes: *mut IParameterChanges) -> Option<f64> {
        // SAFETY: the host's list (or null), for this call.
        let changes = unsafe { ComRef::from_raw(changes) }?;
        let mut mode_in = None;
        // SAFETY: plain calls on the host's list and its queues.
        unsafe {
            for i in 0..changes.getParameterCount() {
                let Some(q) = ComRef::from_raw(changes.getParameterData(i)) else {
                    continue;
                };
                let id = q.getParameterId();
                for j in 0..q.getPointCount() {
                    let (mut offset, mut v) = (0, 0.0);
                    if q.getPoint(j, &mut offset, &mut v) != kResultOk {
                        continue;
                    }
                    bump(ix::PARAM_POINTS);
                    store(ix::LAST_PARAM_OFFSET, offset as u64);
                    if let Some((k, _)) = param(id) {
                        store(ix::PARAM_LAST + k, v.to_bits());
                        if let Some(c) = COUNTERS.get(ix::PARAM_SEEN) {
                            c.fetch_or(1 << k, Ordering::SeqCst);
                        }
                    }
                    if id == VST3_PARAM_GAIN {
                        self.gain.store(v.to_bits(), Ordering::SeqCst);
                    }
                    if let Some(c) = bank_index(id).and_then(|k| self.bank.get(k)) {
                        c.store(v.to_bits(), Ordering::SeqCst);
                    }
                    if id == VST3_PARAM_MODE {
                        mode_in = Some(v);
                    }
                }
            }
        }
        mode_in
    }

    fn read_events(&self, events: *mut IEventList) {
        // SAFETY: the host's list (or null), for this call.
        let Some(events) = (unsafe { ComRef::from_raw(events) }) else {
            return;
        };
        // SAFETY: plain calls; `e` is ours to fill and read as its type
        // says.
        unsafe {
            for i in 0..events.getEventCount() {
                let mut e: Event = std::mem::zeroed();
                if events.getEvent(i, &mut e) != kResultOk {
                    continue;
                }
                match e.r#type {
                    NOTE_ON => {
                        let on = e.__field0.noteOn;
                        bump(ix::NOTE_ONS);
                        self.notes.fetch_add(1, Ordering::SeqCst);
                        let note = (on.pitch as u64 & 0xFF)
                            | ((on.channel as u64 & 0xFF) << 8)
                            | ((e.sampleOffset as u64 & 0xFFFF) << 16);
                        store(ix::LAST_NOTE, note);
                        store(ix::LAST_VELOCITY, f64::from(on.velocity).to_bits());
                    }
                    NOTE_OFF => {
                        bump(ix::NOTE_OFFS);
                        self.notes.fetch_sub(1, Ordering::SeqCst);
                    }
                    POLY_PRESSURE => {
                        bump(ix::POLY_PRESSURES);
                    }
                    _ => {}
                }
            }
        }
    }

    fn process(&self, data: *mut ProcessData) -> tresult {
        // SAFETY: the host's process data, for this call.
        let Some(d) = (unsafe { data.as_ref() }) else {
            return kInvalidArgument;
        };
        if self.crash {
            // SAFETY: undefined behaviour by design: a deliberate null
            // write that raises SIGSEGV, for the scanner's crash tests.
            unsafe { std::ptr::null_mut::<u8>().write_volatile(1) };
        }
        bump(ix::PROCESS_CALLS);
        if let Some(c) = COUNTERS.get(ix::FIRST_PROCESS) {
            let t = SEQ.fetch_add(1, Ordering::SeqCst) + 1;
            let _ = c.compare_exchange(0, t, Ordering::SeqCst, Ordering::SeqCst);
        }
        if !self.active.load(Ordering::SeqCst)
            || !self.processing.load(Ordering::SeqCst)
            || d.symbolicSampleSize != SymbolicSampleSizes_::kSample32 as int32
        {
            bump(ix::BAD_PROCESS);
        }
        store(ix::LAST_FRAMES, d.numSamples as u64);
        store(ix::LAST_DATA_MODE, d.processMode as u64);
        // SAFETY: the host's context (or null), for this call.
        if let Some(c) = unsafe { d.processContext.as_ref() } {
            store(ix::LAST_TEMPO, c.tempo.to_bits());
            store(ix::LAST_MUSIC, c.projectTimeMusic.to_bits());
            store(ix::LAST_BAR, c.barPositionMusic.to_bits());
            store(ix::LAST_STATE, u64::from(c.state));
            store(ix::LAST_PROJECT_SAMPLES, c.projectTimeSamples as u64);
            store(ix::LAST_CONT_SAMPLES, c.continousTimeSamples as u64);
            store(ix::LAST_CONTEXT_RATE, c.sampleRate.to_bits());
        }
        let mode_in = self.read_params(d.inputParameterChanges);
        self.read_events(d.inputEvents);
        let n = usize::try_from(d.numSamples).unwrap_or(0);
        let mut peak = 0.0f32;
        if self.synth {
            let held = self.notes.load(Ordering::SeqCst) > 0;
            for (bus, level) in [(0, if held { 0.5 } else { 0.0 }), (1, 0.25)] {
                for ch in 0..2 {
                    if let Some(out) = channel_ptr(d.outputs, d.numOutputs, bus, ch) {
                        for i in 0..n {
                            // SAFETY: an output channel of `n` samples.
                            unsafe { out.add(i).write(level) };
                        }
                    }
                }
            }
        } else {
            let g = gain_factor(f64::from_bits(self.gain.load(Ordering::SeqCst)));
            if channel_ptr(d.inputs, d.numInputs, 1, 0).is_some() {
                bump(ix::SIDECHAIN_BLOCKS);
            }
            for ch in 0..2 {
                let input = channel_ptr(d.inputs, d.numInputs, 0, ch);
                let side = channel_ptr(d.inputs, d.numInputs, 1, ch);
                let Some(out) = channel_ptr(d.outputs, d.numOutputs, 0, ch) else {
                    continue;
                };
                for i in 0..n {
                    // SAFETY: channels of `n` samples; each sample is read
                    // before the output is written (in place is allowed).
                    unsafe {
                        let x = input.map_or(0.0, |p| p.add(i).read());
                        let s = side.map_or(0.0, |p| p.add(i).read());
                        let y = if self.nan && i == 0 {
                            f32::NAN
                        } else {
                            x * g + s
                        };
                        out.add(i).write(y);
                        if y.is_finite() {
                            peak = peak.max(y.abs());
                        }
                    }
                }
            }
        }
        // SAFETY: the host's output list (or null), for this call.
        if let Some(out) = unsafe { ComRef::from_raw(d.outputParameterChanges) } {
            let nan = mode() == MODE_NAN_OUTPUT;
            let meter = Some((
                VST3_PARAM_METER,
                if nan {
                    f64::NAN
                } else {
                    f64::from(peak.min(1.0))
                },
            ));
            let secret = mode_in.map(|m| (VST3_PARAM_SECRET, if nan { f64::NAN } else { m }));
            for (id, v) in [meter, secret].into_iter().flatten() {
                let mut index = 0;
                // SAFETY: plain calls on the host's list and the queue it
                // hands out.
                unsafe {
                    if let Some(q) = ComRef::from_raw(out.addParameterData(&id, &mut index)) {
                        q.addPoint(0, v, &mut index);
                    }
                }
            }
        }
        kResultOk
    }
}

/// Implements `IAudioProcessor` over a `proc: Proc` field.
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
                bump(ix::ARRANGEMENTS);
                stamp(ix::LAST_ARRANGEMENT);
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
                LATENCY.load(Ordering::SeqCst)
            }

            unsafe fn setupProcessing(&self, setup: *mut ProcessSetup) -> tresult {
                self.proc.setup(setup)
            }

            unsafe fn setProcessing(&self, state: TBool) -> tresult {
                self.proc.set_processing(state)
            }

            unsafe fn process(&self, data: *mut ProcessData) -> tresult {
                self.proc.process(data)
            }

            unsafe fn getTailSamples(&self) -> uint32 {
                TAIL.load(Ordering::SeqCst)
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

        unsafe fn setActive(&self, state: TBool) -> tresult {
            self.proc.set_active(state)
        }

        unsafe fn setState(&self, state: *mut IBStream) -> tresult {
            self.proc.set_state(state)
        }

        unsafe fn getState(&self, state: *mut IBStream) -> tresult {
            self.proc.get_state(state)
        }
    };
}

/// Counts `activateBus(…, true)`, and the audio input bus 1 (Test Gain's
/// sidechain) on its own.
fn bus_activated(media: MediaType, dir: BusDirection, index: int32, state: TBool) {
    if state == 0 {
        return;
    }
    bump(ix::BUSES_ON);
    if media == MediaTypes_::kAudio as MediaType
        && dir == BusDirections_::kInput as BusDirection
        && index == 1
    {
        bump(ix::SIDECHAIN_ON);
    }
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
    proc: Proc,
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
        state: TBool,
    ) -> tresult {
        if index >= 0 && index < bus_count(&GAIN_BUSES, media, dir) {
            bus_activated(media, dir, index, state);
            kResultOk
        } else {
            kInvalidArgument
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

/// Test Synth and Test Bank: a single component (its own controller).
struct Synth {
    ctl: Ctl,
    proc: Proc,
    /// [`SYNTH_BUSES`], or [`GAIN_BUSES`] for Test Bank.
    buses: &'static [Bus],
}

impl Drop for Synth {
    fn drop(&mut self) {
        bump(ix::COMPONENT_RELEASES);
        stamp(ix::LAST_COMPONENT_RELEASE);
    }
}

impl Class for Synth {
    type Interfaces = (IComponent, IAudioProcessor, IEditController, IMidiMapping);
}

impl IMidiMappingTrait for Synth {
    unsafe fn getMidiControllerAssignment(
        &self,
        bus: int32,
        _channel: int16,
        ctrl: CtrlNumber,
        id: *mut ParamID,
    ) -> tresult {
        self.ctl.check();
        let target = match i32::from(ctrl) {
            1 => VST3_PARAM_MOD,
            c if c == ControllerNumbers_::kPitchBend as i32 => VST3_PARAM_BEND,
            c if c == ControllerNumbers_::kAfterTouch as i32 => VST3_PARAM_PRESSURE,
            _ => return kResultFalse,
        };
        if bus != 0 || id.is_null() {
            return kResultFalse;
        }
        // SAFETY: the host's out value.
        unsafe { *id = target };
        kResultTrue
    }
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
        bus_count(self.buses, media, dir)
    }

    unsafe fn getBusInfo(
        &self,
        media: MediaType,
        dir: BusDirection,
        index: int32,
        bus: *mut BusInfo,
    ) -> tresult {
        bus_info(self.buses, media, dir, index, bus)
    }

    unsafe fn activateBus(
        &self,
        media: MediaType,
        dir: BusDirection,
        index: int32,
        state: TBool,
    ) -> tresult {
        if index >= 0 && index < bus_count(self.buses, media, dir) {
            bus_activated(media, dir, index, state);
            kResultOk
        } else {
            kInvalidArgument
        }
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
        let made = if cid == GAIN || cid == PROCESS_CRASHER || cid == NAN {
            ComWrapper::new(Component {
                host: Mutex::new(None),
                peer: Mutex::new(None),
                proc: Proc::new(false, cid == PROCESS_CRASHER, cid == NAN, false),
            })
            .to_com_ptr::<FUnknown>()
        } else if cid == SYNTH {
            ComWrapper::new(Synth {
                ctl: Ctl::new(false),
                proc: Proc::new(true, false, false, false),
                buses: &SYNTH_BUSES,
            })
            .to_com_ptr::<FUnknown>()
        } else if cid == BANK {
            ComWrapper::new(Synth {
                ctl: Ctl::new(true),
                proc: Proc::new(false, false, false, true),
                buses: &GAIN_BUSES,
            })
            .to_com_ptr::<FUnknown>()
        } else if cid == CONTROLLER {
            ComWrapper::new(Controller {
                ctl: Ctl::new(false),
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
    /// `setupProcessing` refuses the real-time process mode.
    RealtimeRefused = 6,
    /// `process` reports NaN for Meter and Secret in its output
    /// parameter changes.
    NanOutput = 7,
    /// Components refuse `setState` (and change nothing).
    ComponentStateRefused = 8,
    /// Controllers refuse `setState` (and change nothing).
    ControllerStateRefused = 9,
    /// Components write their state with a seek past the end (a 4-byte
    /// gap, then a tag) and, reading it, read the gap and the tag, read
    /// past the end and seek past it, refusing a stream that does not
    /// behave as the SDK's streams do.
    StateSeeksPastEnd = 10,
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
    /// `setActive(true)` calls.
    pub set_active_on: u64,
    /// `setActive(false)` calls.
    pub set_active_off: u64,
    /// When the latest `setActive(true)` came.
    pub last_set_active_on: u64,
    /// When the latest `setActive(false)` came.
    pub last_set_active_off: u64,
    /// `setProcessing(true)` calls.
    pub processing_on: u64,
    /// `setProcessing(false)` calls.
    pub processing_off: u64,
    /// When the latest `setProcessing(true)` came.
    pub last_processing_on: u64,
    /// When the latest `setProcessing(false)` came.
    pub last_processing_off: u64,
    /// When the latest `setupProcessing` came.
    pub last_setup: u64,
    /// The latest `setupProcessing` sample rate (`f64` bits).
    pub last_setup_rate: u64,
    /// The latest `setupProcessing` largest block.
    pub last_setup_block: u64,
    /// `setBusArrangements` calls.
    pub arrangements: u64,
    /// When the latest `setBusArrangements` came.
    pub last_arrangement: u64,
    /// `activateBus(…, true)` calls.
    pub buses_on: u64,
    /// `process` calls.
    pub process_calls: u64,
    /// When the first `process` call came.
    pub first_process: u64,
    /// Calls out of order: `process` while inactive or not processing, or
    /// not in 32-bit samples; `setupProcessing` while active.
    pub bad_process: u64,
    /// The latest block's `numSamples`.
    pub last_frames: u64,
    /// The latest block's `processMode`.
    pub last_data_mode: u64,
    /// The latest `ProcessContext::tempo` (`f64` bits).
    pub last_tempo: u64,
    /// The latest `ProcessContext::projectTimeMusic` (`f64` bits).
    pub last_music: u64,
    /// The latest `ProcessContext::barPositionMusic` (`f64` bits).
    pub last_bar: u64,
    /// The latest `ProcessContext::state` flags.
    pub last_state: u64,
    /// The latest `ProcessContext::projectTimeSamples`.
    pub last_project_samples: u64,
    /// The latest `ProcessContext::continousTimeSamples`.
    pub last_cont_samples: u64,
    /// The latest `ProcessContext::sampleRate` (`f64` bits).
    pub last_context_rate: u64,
    /// Input parameter points read, in all.
    pub param_points: u64,
    /// Which of [`VST3_PARAMS`] (bit = index) got an input point.
    pub param_seen: u64,
    /// The latest input point's sample offset.
    pub last_param_offset: u64,
    /// The latest input point of each of [`VST3_PARAMS`] (`f64` bits).
    pub param_last: [u64; 7],
    /// Note-on events.
    pub note_ons: u64,
    /// Note-off events.
    pub note_offs: u64,
    /// Poly-pressure events.
    pub poly_pressures: u64,
    /// The latest note-on: pitch, channel << 8, sample offset << 16.
    pub last_note: u64,
    /// The latest note-on's velocity (`f64` bits).
    pub last_velocity: u64,
    /// Blocks in which Test Gain got sidechain buffers.
    pub sidechain_blocks: u64,
    /// `activateBus` of audio input bus 1 (the sidechain), on.
    pub sidechain_on: u64,
    /// CLAP entry `init` calls (the library as the CLAP test bundle).
    pub clap_inits: u64,
    /// CLAP entry `deinit` calls.
    pub clap_deinits: u64,
    /// Component `setState` calls (also refused ones).
    pub component_set_states: u64,
    /// When the latest component `setState` came.
    pub last_component_set_state: u64,
    /// When the latest `setComponentState` read the component's state.
    pub last_state_sync: u64,
    /// Controller `setState` calls (also refused ones).
    pub controller_set_states: u64,
    /// When the latest controller `setState` came.
    pub last_controller_set_state: u64,
    /// The memo the latest controller `setState` took.
    pub last_memo: u64,
}

impl Vst3Counters {
    /// The latest normalised input point of parameter `id`, if one came.
    pub fn param_in(&self, id: u32) -> Option<f64> {
        let (k, _) = param(id)?;
        (self.param_seen & (1 << k) != 0)
            .then(|| self.param_last.get(k).map(|b| f64::from_bits(*b)))
            .flatten()
    }
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

    /// Opens the binary of the CLAP test bundle at `bundle` (a copy of
    /// [`crate::bundle`]): the same library, so the same counters. Does
    /// not run the CLAP entry. Panics (test helper) when it cannot.
    pub fn open_clap(bundle: &std::path::Path) -> Vst3Probe {
        let binary = if cfg!(target_os = "macos") {
            bundle.join("Contents").join("MacOS").join("VizDAW Test")
        } else {
            bundle.to_path_buf()
        };
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
            set_active_on: v[ix::SET_ACTIVE_ON],
            set_active_off: v[ix::SET_ACTIVE_OFF],
            last_set_active_on: v[ix::LAST_SET_ACTIVE_ON],
            last_set_active_off: v[ix::LAST_SET_ACTIVE_OFF],
            processing_on: v[ix::PROCESSING_ON],
            processing_off: v[ix::PROCESSING_OFF],
            last_processing_on: v[ix::LAST_PROCESSING_ON],
            last_processing_off: v[ix::LAST_PROCESSING_OFF],
            last_setup: v[ix::LAST_SETUP],
            last_setup_rate: v[ix::LAST_SETUP_RATE],
            last_setup_block: v[ix::LAST_SETUP_BLOCK],
            arrangements: v[ix::ARRANGEMENTS],
            last_arrangement: v[ix::LAST_ARRANGEMENT],
            buses_on: v[ix::BUSES_ON],
            process_calls: v[ix::PROCESS_CALLS],
            first_process: v[ix::FIRST_PROCESS],
            bad_process: v[ix::BAD_PROCESS],
            last_frames: v[ix::LAST_FRAMES],
            last_data_mode: v[ix::LAST_DATA_MODE],
            last_tempo: v[ix::LAST_TEMPO],
            last_music: v[ix::LAST_MUSIC],
            last_bar: v[ix::LAST_BAR],
            last_state: v[ix::LAST_STATE],
            last_project_samples: v[ix::LAST_PROJECT_SAMPLES],
            last_cont_samples: v[ix::LAST_CONT_SAMPLES],
            last_context_rate: v[ix::LAST_CONTEXT_RATE],
            param_points: v[ix::PARAM_POINTS],
            param_seen: v[ix::PARAM_SEEN],
            last_param_offset: v[ix::LAST_PARAM_OFFSET],
            param_last: std::array::from_fn(|k| v[ix::PARAM_LAST + k]),
            note_ons: v[ix::NOTE_ONS],
            note_offs: v[ix::NOTE_OFFS],
            poly_pressures: v[ix::POLY_PRESSURES],
            last_note: v[ix::LAST_NOTE],
            last_velocity: v[ix::LAST_VELOCITY],
            sidechain_blocks: v[ix::SIDECHAIN_BLOCKS],
            sidechain_on: v[ix::SIDECHAIN_ON],
            clap_inits: v[ix::CLAP_INITS],
            clap_deinits: v[ix::CLAP_DEINITS],
            component_set_states: v[ix::COMPONENT_SET_STATES],
            last_component_set_state: v[ix::LAST_COMPONENT_SET_STATE],
            last_state_sync: v[ix::LAST_STATE_SYNC],
            controller_set_states: v[ix::CONTROLLER_SET_STATES],
            last_controller_set_state: v[ix::LAST_CONTROLLER_SET_STATE],
            last_memo: v[ix::LAST_MEMO],
        }
    }

    /// Sets the memo (a controller-only value, carried only by the
    /// controller's own state) controllers made from now on start with.
    pub fn set_memo(&self, memo: u64) {
        // SAFETY: the exported function takes a `u64`.
        let f = unsafe {
            self.0
                .get::<extern "C" fn(u64)>(b"vizdaw_test_vst3_set_memo\0")
        }
        .unwrap();
        f(memo);
    }

    /// Sets the latency and tail (frames; `u32::MAX` is an infinite tail)
    /// every component reports from now on.
    pub fn set_timing(&self, latency: u32, tail: u32) {
        // SAFETY: the exported function takes two `u32`s.
        let f = unsafe {
            self.0
                .get::<extern "C" fn(u32, u32)>(b"vizdaw_test_vst3_set_timing\0")
        }
        .unwrap();
        f(latency, tail);
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
