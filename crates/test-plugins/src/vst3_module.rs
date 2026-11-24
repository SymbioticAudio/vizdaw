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

use std::ffi::{c_char, c_void};
use std::sync::atomic::{AtomicBool, Ordering};
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

/// Set by the module entry, cleared by the module exit.
static ENTERED: AtomicBool = AtomicBool::new(false);

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
    ENTERED.store(true, Ordering::SeqCst);
    true
}

/// The module exit on macOS.
#[cfg(target_os = "macos")]
#[unsafe(no_mangle)]
pub extern "system" fn bundleExit() -> bool {
    ENTERED.store(false, Ordering::SeqCst);
    true
}

/// The module entry on Linux (gets the `dlopen` handle).
#[cfg(target_os = "linux")]
#[unsafe(no_mangle)]
pub extern "system" fn ModuleEntry(_handle: *mut c_void) -> bool {
    ENTERED.store(true, Ordering::SeqCst);
    true
}

/// The module exit on Linux.
#[cfg(target_os = "linux")]
#[unsafe(no_mangle)]
pub extern "system" fn ModuleExit() -> bool {
    ENTERED.store(false, Ordering::SeqCst);
    true
}

/// The factory, or null when the module entry has not run (as with an SDK
/// plug-in, whose factory needs the module initialised).
#[unsafe(no_mangle)]
pub extern "system" fn GetPluginFactory() -> *mut IPluginFactory {
    let needs_entry = cfg!(any(target_os = "macos", target_os = "linux"));
    if needs_entry && !ENTERED.load(Ordering::SeqCst) {
        return std::ptr::null_mut();
    }
    match ComWrapper::new(Factory).to_com_ptr::<IPluginFactory>() {
        Some(p) => p.into_raw(),
        None => std::ptr::null_mut(),
    }
}
