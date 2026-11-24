//! The VST3 side of the test binary (host track H1): the same library
//! exports `GetPluginFactory` beside `clap_entry`, and [`crate::vst3_bundle`]
//! wraps it as `VizDAW Test.vst3`. This first cut serves the scanner tests:
//! a factory (`IPluginFactory3`) listing several classes and components
//! that only answer `IComponent`. Later tasks give the components audio,
//! parameters and state.
//!
//! | class ID | name | sub-categories | what it does |
//! |---|---|---|---|
//! | [`VST3_GAIN_ID`] | Test Gain | `Fx` | a stub component |
//! | [`VST3_SYNTH_ID`] | Test Synth | `Instrument\|Synth` | a stub component |
//! | [`VST3_CRASHER_ID`] | Test Crasher | `Fx` | crashes (SIGSEGV) in `createInstance` |
//! | [`VST3_HANGER_ID`] | Test Hanger | `Fx` | never returns from `createInstance` |
//! | [`VST3_CONTROLLER_ID`] | Test Controller | (controller class) | not a plug-in; not creatable yet |
//!
//! Like a plug-in built with the SDK, the factory is only handed out after
//! the module entry (`bundleEntry` on macOS, `ModuleEntry` on Linux) ran.
//!
//! # Lifecycle probe
//!
//! The library counts its own module lifecycle: entries, exits, and when
//! the factory and components are released (as a sequence number, so
//! "released before the exit" can be asserted). A test reads them through
//! [`crate::Vst3Probe`], which `dlopen`s the same binary (the counters live
//! in the loaded image, not in the test's own copy of this crate) and
//! calls the exported `vizdaw_test_vst3_counters`. The probe also sets a
//! mode, read at the next entry or factory request, to make the module
//! misbehave: [`crate::Vst3Mode::EntryFails`] (the entry returns false) and
//! [`crate::Vst3Mode::NullFactory`] (a good entry, then no factory). The
//! mode and counters are per loaded image; tests that use them load a
//! private copy of the bundle so they never see each other's.

use std::ffi::{c_char, c_void};
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use vst3::Steinberg::Vst::{
    BusDirection, BusInfo, IComponent, IComponentTrait, IoMode, MediaType, RoutingInfo,
};
use vst3::Steinberg::{
    FIDString, FUnknown, IBStream, IPluginBaseTrait, IPluginFactory, IPluginFactory2Trait,
    IPluginFactory3, IPluginFactory3Trait, IPluginFactoryTrait, PClassInfo, PClassInfo2,
    PClassInfoW, PFactoryInfo, TBool, TUID, int32, kInvalidArgument, kNoInterface, kNotImplemented,
    kResultOk, tresult,
};
use vst3::Steinberg::{PClassInfo_::ClassCardinality_, PFactoryInfo_::FactoryFlags_};
use vst3::{Class, ComWrapper, Interface, uid};

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

/// How the module behaves (see [`crate::Vst3Mode`]); read at entry and at
/// the factory request.
static MODE: AtomicU32 = AtomicU32::new(0);
const MODE_ENTRY_FAILS: u32 = 1;
const MODE_NULL_FACTORY: u32 = 2;

static ENTRIES: AtomicU64 = AtomicU64::new(0);
static EXITS: AtomicU64 = AtomicU64::new(0);
static FACTORY_RELEASES: AtomicU64 = AtomicU64::new(0);
static COMPONENT_RELEASES: AtomicU64 = AtomicU64::new(0);
/// Source of event sequence numbers (the first is 1; 0 means "never").
static SEQ: AtomicU64 = AtomicU64::new(0);
static LAST_FACTORY_RELEASE: AtomicU64 = AtomicU64::new(0);
static LAST_COMPONENT_RELEASE: AtomicU64 = AtomicU64::new(0);
static LAST_EXIT: AtomicU64 = AtomicU64::new(0);

fn tick() -> u64 {
    SEQ.fetch_add(1, Ordering::SeqCst) + 1
}

fn enter() -> bool {
    if MODE.load(Ordering::SeqCst) == MODE_ENTRY_FAILS {
        return false;
    }
    ENTRIES.fetch_add(1, Ordering::SeqCst);
    ENTERED.fetch_add(1, Ordering::SeqCst);
    true
}

fn exit() -> bool {
    EXITS.fetch_add(1, Ordering::SeqCst);
    ENTERED.fetch_sub(1, Ordering::SeqCst);
    LAST_EXIT.store(tick(), Ordering::SeqCst);
    true
}

/// The lifecycle counters, as `[entries, exits, factory releases,
/// component releases, last factory release, last component release, last
/// exit]` (the last three are sequence numbers, 0 for never), written to
/// `out[..7]`.
#[unsafe(no_mangle)]
pub extern "C" fn vizdaw_test_vst3_counters(out: *mut u64) {
    let v = [
        ENTRIES.load(Ordering::SeqCst),
        EXITS.load(Ordering::SeqCst),
        FACTORY_RELEASES.load(Ordering::SeqCst),
        COMPONENT_RELEASES.load(Ordering::SeqCst),
        LAST_FACTORY_RELEASE.load(Ordering::SeqCst),
        LAST_COMPONENT_RELEASE.load(Ordering::SeqCst),
        LAST_EXIT.load(Ordering::SeqCst),
    ];
    // SAFETY: the caller passes room for seven values.
    unsafe { std::ptr::copy_nonoverlapping(v.as_ptr(), out, v.len()) };
}

/// Sets the mode (0 normal, 1 entry fails, 2 null factory).
#[unsafe(no_mangle)]
pub extern "C" fn vizdaw_test_vst3_set_mode(mode: u32) {
    MODE.store(mode, Ordering::SeqCst);
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

/// A component that answers `IComponent` and does nothing else yet.
struct Component;

impl Drop for Component {
    fn drop(&mut self) {
        COMPONENT_RELEASES.fetch_add(1, Ordering::SeqCst);
        LAST_COMPONENT_RELEASE.store(tick(), Ordering::SeqCst);
    }
}

impl Class for Component {
    type Interfaces = (IComponent,);
}

impl IPluginBaseTrait for Component {
    unsafe fn initialize(&self, _context: *mut FUnknown) -> tresult {
        kResultOk
    }

    unsafe fn terminate(&self) -> tresult {
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

    unsafe fn setIoMode(&self, _mode: IoMode) -> tresult {
        kResultOk
    }

    unsafe fn getBusCount(&self, _type: MediaType, _dir: BusDirection) -> int32 {
        0
    }

    unsafe fn getBusInfo(
        &self,
        _type: MediaType,
        _dir: BusDirection,
        _index: int32,
        _bus: *mut BusInfo,
    ) -> tresult {
        kInvalidArgument
    }

    unsafe fn getRoutingInfo(&self, _in: *mut RoutingInfo, _out: *mut RoutingInfo) -> tresult {
        kNotImplemented
    }

    unsafe fn activateBus(
        &self,
        _type: MediaType,
        _dir: BusDirection,
        _index: int32,
        _state: TBool,
    ) -> tresult {
        kInvalidArgument
    }

    unsafe fn setActive(&self, _state: TBool) -> tresult {
        kResultOk
    }

    unsafe fn setState(&self, _state: *mut IBStream) -> tresult {
        kResultOk
    }

    unsafe fn getState(&self, _state: *mut IBStream) -> tresult {
        kResultOk
    }
}

struct Factory;

impl Drop for Factory {
    fn drop(&mut self) {
        FACTORY_RELEASES.fetch_add(1, Ordering::SeqCst);
        LAST_FACTORY_RELEASE.store(tick(), Ordering::SeqCst);
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
        if cid != GAIN && cid != SYNTH {
            return kInvalidArgument;
        }
        if iid != IComponent::IID && iid != <FUnknown as Interface>::IID {
            return kNoInterface;
        }
        match ComWrapper::new(Component).to_com_ptr::<IComponent>() {
            Some(p) => {
                // SAFETY: as above; the host owns the reference it gets.
                unsafe { *obj = p.into_raw() as *mut c_void };
                kResultOk
            }
            None => kNoInterface,
        }
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

    unsafe fn setHostContext(&self, _context: *mut FUnknown) -> tresult {
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
    if (needs_entry && ENTERED.load(Ordering::SeqCst) == 0)
        || MODE.load(Ordering::SeqCst) == MODE_NULL_FACTORY
    {
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
}

/// The module's lifecycle counters (see the module docs). The `last_*`
/// fields are sequence numbers of the latest such event (0: never), from
/// one clock, so they order events.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Vst3Counters {
    /// Module entries that succeeded.
    pub entries: u64,
    /// Module exits.
    pub exits: u64,
    /// Factories released.
    pub factory_releases: u64,
    /// Components released.
    pub component_releases: u64,
    /// When the latest factory was released.
    pub last_factory_release: u64,
    /// When the latest component was released.
    pub last_component_release: u64,
    /// When the latest exit ran.
    pub last_exit: u64,
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

    /// Sets how the module behaves from the next entry on.
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
        // SAFETY: the exported function fills seven values.
        let f = unsafe {
            self.0
                .get::<extern "C" fn(*mut u64)>(b"vizdaw_test_vst3_counters\0")
        }
        .unwrap();
        let mut v = [0u64; 7];
        f(v.as_mut_ptr());
        Vst3Counters {
            entries: v[0],
            exits: v[1],
            factory_releases: v[2],
            component_releases: v[3],
            last_factory_release: v[4],
            last_component_release: v[5],
            last_exit: v[6],
        }
    }
}
