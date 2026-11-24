//! The VST3 host's `unsafe` calls: opening a module's library, its module
//! entry and exit, the factory's COM calls, the host's own COM objects
//! (`IHostApplication`, `IMessage`, `IAttributeList`, `IBStream`, the
//! component handler), and a plug-in's component and controller. Module
//! loading mirrors the SDK's `public.sdk/source/vst/hosting/module_mac.mm`
//! and `module_linux.cpp`: on macOS `bundleEntry(CFBundleRef)` (or the
//! older `BundleEntry`) runs before `GetPluginFactory` and `bundleExit`
//! after the factory is released; on Linux `ModuleEntry(handle)` and
//! `ModuleExit`. A plug-in is set up as the SDK's `plugprovider.cpp` does.

use super::module::{ClassInfo, binary_path, c_text, w_text};
use crate::error::HostError;
use crossbeam_queue::ArrayQueue;
use std::collections::HashMap;
use std::ffi::{CStr, CString, c_char, c_void};
use std::mem::ManuallyDrop;
use std::path::Path;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicI32, AtomicU64, Ordering};
use vst3::com_scrape_types::Unknown;
use vst3::{Class, ComPtr, ComWrapper, Interface};

/// The `vst3` crate's binding module, named after the SDK's vendor. This
/// re-export is the only place the name appears; everything else in the
/// crate says `sdk`.
pub(crate) use vst3::Steinberg as sdk;

use sdk::IBStream_::IStreamSeekMode_;
use sdk::Vst::BusInfo_::BusFlags_;
use sdk::Vst::ParameterInfo_::ParameterFlags_;
use sdk::Vst::{
    BusInfo, BusTypes_, IAttributeList, IAttributeListTrait, IAudioProcessor, IAudioProcessorTrait,
    IComponent, IComponentHandler, IComponentHandler2, IComponentHandler2Trait,
    IComponentHandlerTrait, IComponentTrait, IConnectionPoint, IConnectionPointTrait,
    IEditController, IEditControllerTrait, IHostApplication, IHostApplicationTrait, IMessage,
    IMessageTrait, MediaTypes_, ParamID, ParamValue, ParameterInfo, ProcessModes_, ProcessSetup,
    String128, SymbolicSampleSizes_, TChar,
};
use sdk::Vst::{
    ControllerNumbers_, IMidiMapping, IMidiMappingTrait, SpeakerArr, SpeakerArrangement, kNoParamId,
};
use sdk::{
    FIDString, FUnknown, IBStream, IBStreamTrait, IPluginBaseTrait, IPluginFactory,
    IPluginFactory2, IPluginFactory2Trait, IPluginFactory3, IPluginFactory3Trait,
    IPluginFactoryTrait, ISizeableStream, ISizeableStreamTrait, PClassInfo, PClassInfo2,
    PClassInfoW, PFactoryInfo, TBool, TUID, int32, int64, kInvalidArgument, kNoInterface,
    kNotImplemented, kResultFalse, kResultOk, tresult, uint32,
};

/// Most classes read from one factory (a guard against a bogus count).
const MAX_CLASSES: i32 = 4096;

type GetFactory = unsafe extern "system" fn() -> *mut IPluginFactory;
type ModuleExit = unsafe extern "system" fn() -> bool;

#[cfg(target_os = "macos")]
mod cf {
    //! The CoreFoundation calls that make the `CFBundleRef` a bundle entry
    //! receives (plug-ins find their resources through it).
    use std::ffi::c_void;

    #[link(name = "CoreFoundation", kind = "framework")]
    unsafe extern "C" {
        fn CFURLCreateFromFileSystemRepresentation(
            allocator: *const c_void,
            buffer: *const u8,
            length: isize,
            is_directory: u8,
        ) -> *const c_void;
        fn CFBundleCreate(allocator: *const c_void, url: *const c_void) -> *mut c_void;
        fn CFRelease(cf: *const c_void);
    }

    /// An owned `CFBundleRef` (released on drop).
    pub(super) struct Bundle(pub(super) *mut c_void);

    impl Bundle {
        /// The bundle at `path`, or a null one when CoreFoundation does not
        /// see a bundle there.
        pub(super) fn create(path: &std::path::Path) -> Bundle {
            use std::os::unix::ffi::OsStrExt;
            let bytes = path.as_os_str().as_bytes();
            let Ok(len) = isize::try_from(bytes.len()) else {
                return Bundle(std::ptr::null_mut());
            };
            // SAFETY: `bytes` is valid for `len` bytes; a null allocator is
            // the default one. The URL is released below.
            let url = unsafe {
                CFURLCreateFromFileSystemRepresentation(std::ptr::null(), bytes.as_ptr(), len, 1)
            };
            if url.is_null() {
                return Bundle(std::ptr::null_mut());
            }
            // SAFETY: `url` is a valid CFURL; CFBundleCreate retains what it
            // needs, so the URL can be released at once.
            let bundle = unsafe { CFBundleCreate(std::ptr::null(), url) };
            // SAFETY: we own the URL (a Create call).
            unsafe { CFRelease(url) };
            Bundle(bundle)
        }
    }

    impl Drop for Bundle {
        fn drop(&mut self) {
            if !self.0.is_null() {
                // SAFETY: we own the bundle (a Create call).
                unsafe { CFRelease(self.0) };
            }
        }
    }
}

/// A loaded VST3 module: its library (entered), and its factory. Dropping
/// it releases the factory, then runs the module exit; every COM object
/// made from the factory must be released first.
///
/// The library itself is never unloaded once opened, on any platform
/// (host track H1 Task 3 fix, Linux TLS): a plug-in may have given a
/// thread a thread-exit destructor in its own code (Rust's `std`, many
/// C++ runtimes), and a thread that exits after an unload runs code that
/// is gone. Loading the bundle again reuses the open image and runs its
/// entry again, as a module that counts entries expects.
pub(crate) struct Module {
    factory: Option<ComPtr<IPluginFactory>>,
    exit: Option<ModuleExit>,
    /// The host context the factory and every object made from it get.
    host: ComWrapper<HostApp>,
    // Never closed (see above); (macOS) the bundle reference the entry got
    // is released after `drop` ran the exit.
    _library: ManuallyDrop<libloading::Library>,
    #[cfg(target_os = "macos")]
    _bundle: cf::Bundle,
}

impl std::fmt::Debug for Module {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Module").finish_non_exhaustive()
    }
}

fn load_failed(path: &Path, why: impl std::fmt::Display) -> HostError {
    HostError::LoadFailed(format!("{}: {why}", path.display()))
}

impl Module {
    /// Loads the VST3 bundle at `bundle` into this process: opens its
    /// binary, runs the module entry and takes the factory.
    pub(crate) fn load(bundle: &Path) -> Result<Module, HostError> {
        let binary = binary_path(bundle)?;
        #[cfg(target_os = "macos")]
        let (library, entered, cf_bundle) = {
            // SAFETY: loading a plug-in binary runs its initialisers;
            // plug-ins are untrusted code by design (spec §2), which is why
            // the scanner loads unknown bundles in a separate process first.
            let library = ManuallyDrop::new(
                unsafe { libloading::Library::new(&binary) }.map_err(|e| load_failed(bundle, e))?,
            );
            let cf_bundle = cf::Bundle::create(bundle);
            type Entry = unsafe extern "system" fn(*mut c_void) -> bool;
            // SAFETY: the SDK declares `bool bundleEntry(CFBundleRef)`.
            let entry = unsafe { library.get::<Entry>(b"bundleEntry\0") }
                .or_else(|_| unsafe { library.get::<Entry>(b"BundleEntry\0") })
                .map(|s| *s)
                .map_err(|_| load_failed(bundle, "no bundleEntry function"))?;
            // SAFETY: the entry gets the bundle reference it expects; it
            // stays valid until after the exit (field order of `Module`).
            let entered = unsafe { entry(cf_bundle.0) };
            (library, entered, cf_bundle)
        };
        #[cfg(not(target_os = "macos"))]
        let (library, entered) = {
            use libloading::os::unix::{Library, RTLD_LAZY, RTLD_LOCAL};
            // SAFETY: as above (macOS).
            let library = ManuallyDrop::new(
                unsafe { Library::open(Some(&binary), RTLD_LAZY | RTLD_LOCAL) }
                    .map_err(|e| load_failed(bundle, e))?,
            );
            type Entry = unsafe extern "system" fn(*mut c_void) -> bool;
            // SAFETY: the SDK declares `bool ModuleEntry(void* sharedLibraryHandle)`.
            let entry = unsafe { library.get::<Entry>(b"ModuleEntry\0") }
                .map(|s| *s)
                .map_err(|_| load_failed(bundle, "no ModuleEntry function"))?;
            // The entry gets the `dlopen` handle, as in the SDK.
            let handle = ManuallyDrop::into_inner(library).into_raw();
            // SAFETY: `handle` came from `into_raw` just now.
            let library = unsafe { Library::from_raw(handle) };
            // SAFETY: the handle is the open library's.
            let entered = unsafe { entry(handle) };
            (
                ManuallyDrop::new(libloading::Library::from(library)),
                entered,
            )
        };
        if !entered {
            return Err(load_failed(bundle, "the module entry failed"));
        }
        // SAFETY: as the entry above: names the SDK defines.
        let exit_name: &[u8] = if cfg!(target_os = "macos") {
            b"bundleExit\0"
        } else {
            b"ModuleExit\0"
        };
        let exit = unsafe { library.get::<ModuleExit>(exit_name) }
            .ok()
            .map(|s| *s)
            .or_else(|| {
                // SAFETY: the older macOS spelling, same signature.
                cfg!(target_os = "macos")
                    .then(|| unsafe { library.get::<ModuleExit>(b"BundleExit\0") }.ok())
                    .flatten()
                    .map(|s| *s)
            });
        let mut module = Module {
            factory: None,
            exit,
            host: ComWrapper::new(HostApp),
            _library: library,
            #[cfg(target_os = "macos")]
            _bundle: cf_bundle,
        };
        // SAFETY: `GetPluginFactory` takes nothing and returns a factory
        // with a reference for the caller, or null.
        let get = unsafe { module._library.get::<GetFactory>(b"GetPluginFactory\0") }
            .map(|s| *s)
            .map_err(|_| load_failed(bundle, "no GetPluginFactory function"))?;
        // SAFETY: as above; `from_raw` takes over the returned reference.
        let factory = unsafe { ComPtr::from_raw(get()) }
            .ok_or_else(|| load_failed(bundle, "no plug-in factory"))?;
        // A factory that takes a host context gets it before any class is
        // created (the SDK's `PluginFactory::setHostContext`).
        if let Some(f3) = factory.cast::<IPluginFactory3>() {
            // SAFETY: the context outlives the factory (a field of
            // `module`, released after it).
            unsafe { f3.setHostContext(module.host_context()) };
        }
        module.factory = Some(factory);
        Ok(module)
    }

    /// The host context (`IHostApplication`) as the `FUnknown` that
    /// `initialize` and `setHostContext` take. Valid while the module is.
    pub(crate) fn host_context(&self) -> *mut FUnknown {
        self.host
            .as_com_ref::<IHostApplication>()
            .map_or(std::ptr::null_mut(), |r| r.as_ptr() as *mut FUnknown)
    }

    fn factory(&self) -> Option<&ComPtr<IPluginFactory>> {
        self.factory.as_ref()
    }

    /// The factory's vendor ("" when it has none).
    pub(crate) fn vendor(&self) -> String {
        let Some(f) = self.factory() else {
            return String::new();
        };
        // SAFETY: an all-zero PFactoryInfo is valid (arrays and an int).
        let mut info: PFactoryInfo = unsafe { std::mem::zeroed() };
        // SAFETY: the factory fills the struct we own.
        if unsafe { f.getFactoryInfo(&mut info) } == kResultOk {
            c_text(&info.vendor)
        } else {
            String::new()
        }
    }

    /// Every class the factory lists, in its order.
    pub(crate) fn classes(&self) -> Vec<ClassInfo> {
        let Some(f) = self.factory() else {
            return Vec::new();
        };
        let f3 = f.cast::<IPluginFactory3>();
        let f2 = f.cast::<IPluginFactory2>();
        // SAFETY: a plain query on a live factory.
        let count = unsafe { f.countClasses() }.clamp(0, MAX_CLASSES);
        let mut out = Vec::new();
        for i in 0..count {
            if let Some(f3) = &f3 {
                // SAFETY: all-zero is valid for these plain structs.
                let mut w: PClassInfoW = unsafe { std::mem::zeroed() };
                // SAFETY: the factory fills the struct we own.
                if unsafe { f3.getClassInfoUnicode(i, &mut w) } == kResultOk {
                    out.push(ClassInfo {
                        cid: w.cid.map(|b| b as u8),
                        category: c_text(&w.category),
                        name: w_text(&w.name),
                        sub_categories: c_text(&w.subCategories),
                        vendor: w_text(&w.vendor),
                        version: w_text(&w.version),
                    });
                    continue;
                }
            }
            if let Some(f2) = &f2 {
                // SAFETY: as above.
                let mut c2: PClassInfo2 = unsafe { std::mem::zeroed() };
                // SAFETY: as above.
                if unsafe { f2.getClassInfo2(i, &mut c2) } == kResultOk {
                    out.push(ClassInfo {
                        cid: c2.cid.map(|b| b as u8),
                        category: c_text(&c2.category),
                        name: c_text(&c2.name),
                        sub_categories: c_text(&c2.subCategories),
                        vendor: c_text(&c2.vendor),
                        version: c_text(&c2.version),
                    });
                    continue;
                }
            }
            // SAFETY: as above.
            let mut c1: PClassInfo = unsafe { std::mem::zeroed() };
            // SAFETY: as above.
            if unsafe { f.getClassInfo(i, &mut c1) } == kResultOk {
                out.push(ClassInfo {
                    cid: c1.cid.map(|b| b as u8),
                    category: c_text(&c1.category),
                    name: c_text(&c1.name),
                    sub_categories: String::new(),
                    vendor: String::new(),
                    version: String::new(),
                });
            }
        }
        out
    }

    /// Creates an object of class `cid` through interface `I`. The object
    /// must be released before this module is dropped.
    pub(crate) fn create<I: Interface>(&self, cid: &[u8; 16]) -> Result<ComPtr<I>, HostError> {
        let f = self
            .factory()
            .ok_or_else(|| HostError::LoadFailed("no plug-in factory".into()))?;
        let mut obj: *mut c_void = std::ptr::null_mut();
        // SAFETY: both IDs are 16 bytes that outlive the call; the factory
        // writes an object with a reference for us, or null, into `obj`.
        let r = unsafe {
            f.createInstance(
                cid.as_ptr() as FIDString,
                I::IID.as_ptr() as FIDString,
                &mut obj,
            )
        };
        if r != kResultOk {
            return Err(HostError::LoadFailed(format!(
                "createInstance refused (result {r})"
            )));
        }
        // SAFETY: on success `obj` is an `I` (asked by IID) we now own.
        unsafe { ComPtr::from_raw(obj as *mut I) }
            .ok_or_else(|| HostError::LoadFailed("createInstance gave no object".into()))
    }
}

impl Drop for Module {
    fn drop(&mut self) {
        // The SDK's order: release the factory, then the module exit, then
        // unload (the fields dropping after this).
        drop(self.factory.take());
        if let Some(exit) = self.exit {
            // SAFETY: the entry ran (a `Module` exists only then) and the
            // library is still loaded.
            unsafe { exit() };
        }
    }
}

/// The host's name, as `IHostApplication::getName` gives it.
const HOST_NAME: &str = "VizDAW";

/// Most edits the component handler holds between two main-thread drains;
/// more are counted as dropped.
pub(crate) const EDIT_QUEUE: usize = 1024;

fn copy_wide(src: &str, dst: &mut [TChar]) {
    let mut n = 0;
    let room = dst.len().saturating_sub(1);
    for (d, s) in dst.iter_mut().take(room).zip(src.encode_utf16()) {
        *d = s as TChar;
        n += 1;
    }
    if let Some(end) = dst.get_mut(n) {
        *end = 0;
    }
}

/// 16 bytes at `p` (an ID in the VST3 calling convention), or `None` for
/// null.
fn read_id(p: *const TUID) -> Option<[u8; 16]> {
    // SAFETY: a non-null ID pointer points at 16 bytes.
    (!p.is_null()).then(|| unsafe { *p }.map(|b| b as u8))
}

/// Hands out `obj` as interface `iid` through `out` (`createInstance`'s
/// convention: a reference for the caller, or null and `kNoInterface`).
fn hand_out(obj: Option<ComPtr<FUnknown>>, iid: &[u8; 16], out: *mut *mut c_void) -> tresult {
    if out.is_null() {
        return kInvalidArgument;
    }
    // SAFETY: `obj` is live; a successful query adds the caller's
    // reference.
    let p = obj.and_then(|o| unsafe { FUnknown::query_interface(o.as_ptr(), iid) });
    // SAFETY: `out` is the caller's out pointer.
    unsafe { *out = p.unwrap_or(std::ptr::null_mut()) };
    if p.is_some() { kResultOk } else { kNoInterface }
}

/// The host context (`IHostApplication`): its name, and the messages and
/// attribute lists plug-ins send each other through connection points.
pub(crate) struct HostApp;

impl Class for HostApp {
    type Interfaces = (IHostApplication,);
}

impl IHostApplicationTrait for HostApp {
    unsafe fn getName(&self, name: *mut String128) -> tresult {
        // SAFETY: the plug-in passes a string to fill, or null.
        match unsafe { name.as_mut() } {
            Some(n) => {
                copy_wide(HOST_NAME, n);
                kResultOk
            }
            None => kInvalidArgument,
        }
    }

    unsafe fn createInstance(
        &self,
        cid: *mut TUID,
        iid: *mut TUID,
        obj: *mut *mut c_void,
    ) -> tresult {
        let (Some(cid), Some(iid)) = (read_id(cid), read_id(iid)) else {
            return kInvalidArgument;
        };
        let made = if cid == IMessage::IID {
            ComWrapper::new(HostMessage::default()).to_com_ptr::<FUnknown>()
        } else if cid == IAttributeList::IID {
            ComWrapper::new(HostAttributes::default()).to_com_ptr::<FUnknown>()
        } else {
            None
        };
        hand_out(made, &iid, obj)
    }
}

/// An `IMessage`: an ID and the attribute list it owns.
struct HostMessage {
    id: Mutex<Option<CString>>,
    attributes: ComWrapper<HostAttributes>,
}

impl Default for HostMessage {
    fn default() -> HostMessage {
        HostMessage {
            id: Mutex::new(None),
            attributes: ComWrapper::new(HostAttributes::default()),
        }
    }
}

impl Class for HostMessage {
    type Interfaces = (IMessage,);
}

impl IMessageTrait for HostMessage {
    unsafe fn getMessageID(&self) -> FIDString {
        // The text stays where it is until the next `setMessageID`.
        self.id
            .lock()
            .ok()
            .and_then(|g| g.as_ref().map(|s| s.as_ptr()))
            .unwrap_or(std::ptr::null())
    }

    unsafe fn setMessageID(&self, id: FIDString) {
        // SAFETY: a non-null ID is a NUL-terminated string.
        let id = (!id.is_null()).then(|| unsafe { CStr::from_ptr(id) }.to_owned());
        if let Ok(mut g) = self.id.lock() {
            *g = id;
        }
    }

    unsafe fn getAttributes(&self) -> *mut IAttributeList {
        // Owned by the message: no reference for the caller (the SDK's
        // `HostMessage`).
        self.attributes
            .as_com_ref::<IAttributeList>()
            .map_or(std::ptr::null_mut(), |r| r.as_ptr())
    }
}

/// One value of an attribute list.
enum Attr {
    Int(i64),
    Float(f64),
    Text(Vec<TChar>),
    Binary(Vec<u8>),
}

/// An `IAttributeList`.
#[derive(Default)]
struct HostAttributes {
    values: Mutex<HashMap<Vec<u8>, Attr>>,
}

impl HostAttributes {
    fn key(id: *const c_char) -> Option<Vec<u8>> {
        // SAFETY: a non-null attribute ID is a NUL-terminated string.
        (!id.is_null()).then(|| unsafe { CStr::from_ptr(id) }.to_bytes().to_vec())
    }

    fn set(&self, id: *const c_char, v: Attr) -> tresult {
        let (Some(k), Ok(mut g)) = (Self::key(id), self.values.lock()) else {
            return kInvalidArgument;
        };
        g.insert(k, v);
        kResultOk
    }

    fn with<R>(&self, id: *const c_char, f: impl FnOnce(&Attr) -> Option<R>) -> Option<R> {
        let k = Self::key(id)?;
        let g = self.values.lock().ok()?;
        g.get(&k).and_then(f)
    }
}

impl Class for HostAttributes {
    type Interfaces = (IAttributeList,);
}

impl IAttributeListTrait for HostAttributes {
    unsafe fn setInt(&self, id: *const c_char, value: int64) -> tresult {
        self.set(id, Attr::Int(value))
    }

    unsafe fn getInt(&self, id: *const c_char, value: *mut int64) -> tresult {
        let Some(v) = self.with(id, |a| match a {
            Attr::Int(v) => Some(*v),
            _ => None,
        }) else {
            return kResultFalse;
        };
        // SAFETY: the caller's out value, or null.
        match unsafe { value.as_mut() } {
            Some(out) => {
                *out = v;
                kResultOk
            }
            None => kInvalidArgument,
        }
    }

    unsafe fn setFloat(&self, id: *const c_char, value: f64) -> tresult {
        self.set(id, Attr::Float(value))
    }

    unsafe fn getFloat(&self, id: *const c_char, value: *mut f64) -> tresult {
        let Some(v) = self.with(id, |a| match a {
            Attr::Float(v) => Some(*v),
            _ => None,
        }) else {
            return kResultFalse;
        };
        // SAFETY: the caller's out value, or null.
        match unsafe { value.as_mut() } {
            Some(out) => {
                *out = v;
                kResultOk
            }
            None => kInvalidArgument,
        }
    }

    unsafe fn setString(&self, id: *const c_char, string: *const TChar) -> tresult {
        if string.is_null() {
            return kInvalidArgument;
        }
        let mut text = Vec::new();
        // SAFETY: a NUL-terminated UTF-16 string; read up to the NUL.
        unsafe {
            let mut p = string;
            while *p != 0 {
                text.push(*p);
                p = p.add(1);
            }
        }
        self.set(id, Attr::Text(text))
    }

    unsafe fn getString(&self, id: *const c_char, string: *mut TChar, size: uint32) -> tresult {
        let Some(text) = self.with(id, |a| match a {
            Attr::Text(t) => Some(t.clone()),
            _ => None,
        }) else {
            return kResultFalse;
        };
        let room = usize::try_from(size).unwrap_or(0) / std::mem::size_of::<TChar>();
        if string.is_null() || room == 0 {
            return kInvalidArgument;
        }
        let n = text.len().min(room - 1);
        // SAFETY: the caller's buffer holds `size` bytes (`room` units).
        unsafe {
            std::ptr::copy_nonoverlapping(text.as_ptr(), string, n);
            *string.add(n) = 0;
        }
        kResultOk
    }

    unsafe fn setBinary(&self, id: *const c_char, data: *const c_void, size: uint32) -> tresult {
        let n = usize::try_from(size).unwrap_or(0);
        if data.is_null() && n > 0 {
            return kInvalidArgument;
        }
        let bytes = if n == 0 {
            Vec::new()
        } else {
            // SAFETY: the caller's `size` bytes at `data`.
            unsafe { std::slice::from_raw_parts(data as *const u8, n) }.to_vec()
        };
        self.set(id, Attr::Binary(bytes))
    }

    unsafe fn getBinary(
        &self,
        id: *const c_char,
        data: *mut *const c_void,
        size: *mut uint32,
    ) -> tresult {
        // The bytes stay where they are until the attribute is set again
        // (the SDK's convention: valid while the list is).
        let Some((p, n)) = self.with(id, |a| match a {
            Attr::Binary(b) => Some((b.as_ptr() as *const c_void, b.len())),
            _ => None,
        }) else {
            return kResultFalse;
        };
        // SAFETY: the caller's out values, or null.
        match unsafe { (data.as_mut(), size.as_mut()) } {
            (Some(d), Some(s)) => {
                *d = p;
                *s = uint32::try_from(n).unwrap_or(uint32::MAX);
                kResultOk
            }
            _ => kInvalidArgument,
        }
    }
}

/// The largest stream a plug-in may write (a guard against a plug-in that
/// seeks far past the end and writes there).
pub(crate) const MAX_STREAM: usize = 1 << 30;

/// An `IBStream` (and `ISizeableStream`) over bytes in memory, as the
/// SDK's streams behave: a read past the end reads what is there (0 bytes
/// at the end, still `kResultOk`); a seek before the start stops at the
/// start. A writer ([`MemStream::default`], the SDK's `MemoryStream`)
/// may seek past the end, and a write there fills the gap with zeros, up
/// to [`MAX_STREAM`]; a reader ([`MemStream::reader`], the SDK's
/// `ReadOnlyBStream` over one chunk of a preset) stops a seek at the end
/// and refuses writes.
#[derive(Default)]
pub(crate) struct MemStream {
    inner: Mutex<(Vec<u8>, usize)>,
    read_only: bool,
}

impl MemStream {
    /// A read-only stream over `bytes`, at the start.
    pub(crate) fn reader(bytes: &[u8]) -> MemStream {
        MemStream {
            inner: Mutex::new((bytes.to_vec(), 0)),
            read_only: true,
        }
    }

    /// Moves the read and write position back to the start.
    pub(crate) fn rewind(&self) {
        if let Ok(mut g) = self.inner.lock() {
            g.1 = 0;
        }
    }

    /// Takes the bytes written (the stream is empty afterwards).
    pub(crate) fn take(&self) -> Vec<u8> {
        self.inner
            .lock()
            .map(|mut g| {
                g.1 = 0;
                std::mem::take(&mut g.0)
            })
            .unwrap_or_default()
    }

    /// The farthest a seek may go.
    fn limit(&self, len: usize) -> usize {
        if self.read_only { len } else { MAX_STREAM }
    }
}

impl Class for MemStream {
    type Interfaces = (IBStream, ISizeableStream);
}

impl IBStreamTrait for MemStream {
    unsafe fn read(&self, buffer: *mut c_void, n: int32, read: *mut int32) -> tresult {
        // SAFETY: the caller's out value, or null.
        if let Some(r) = unsafe { read.as_mut() } {
            *r = 0;
        }
        let Ok(mut g) = self.inner.lock() else {
            return kResultFalse;
        };
        let want = usize::try_from(n).unwrap_or(0);
        if buffer.is_null() && want > 0 {
            return kInvalidArgument;
        }
        let (data, pos) = &mut *g;
        let have = data.len().saturating_sub(*pos).min(want);
        if let Some(src) = data.get(*pos..*pos + have) {
            if have > 0 {
                // SAFETY: the caller's buffer holds `n` bytes, `have <= n`.
                unsafe { std::ptr::copy_nonoverlapping(src.as_ptr(), buffer as *mut u8, have) };
            }
        }
        *pos += have;
        // SAFETY: as above.
        if let Some(r) = unsafe { read.as_mut() } {
            *r = int32::try_from(have).unwrap_or(0);
        }
        kResultOk
    }

    unsafe fn write(&self, buffer: *mut c_void, n: int32, written: *mut int32) -> tresult {
        // SAFETY: the caller's out value, or null.
        if let Some(w) = unsafe { written.as_mut() } {
            *w = 0;
        }
        if self.read_only {
            return kNotImplemented;
        }
        let Ok(mut g) = self.inner.lock() else {
            return kResultFalse;
        };
        let n = usize::try_from(n).unwrap_or(0);
        if buffer.is_null() && n > 0 {
            return kInvalidArgument;
        }
        let (data, pos) = &mut *g;
        if n > 0 {
            let Some(end) = pos.checked_add(n).filter(|e| *e <= MAX_STREAM) else {
                return kResultFalse;
            };
            // SAFETY: the caller's `n` bytes at `buffer`.
            let src = unsafe { std::slice::from_raw_parts(buffer as *const u8, n) };
            if data.len() < end {
                // A seek past the end left a gap: zeros, as the SDK's.
                data.resize(end, 0);
            }
            if let Some(dst) = data.get_mut(*pos..end) {
                dst.copy_from_slice(src);
            }
            *pos = end;
        }
        // SAFETY: as above.
        if let Some(w) = unsafe { written.as_mut() } {
            *w = int32::try_from(n).unwrap_or(0);
        }
        kResultOk
    }

    unsafe fn seek(&self, offset: int64, mode: int32, result: *mut int64) -> tresult {
        let Ok(mut g) = self.inner.lock() else {
            return kResultFalse;
        };
        let (data, pos) = &mut *g;
        let base = match mode {
            m if m == IStreamSeekMode_::kIBSeekSet as int32 => 0,
            m if m == IStreamSeekMode_::kIBSeekCur as int32 => *pos as i64,
            m if m == IStreamSeekMode_::kIBSeekEnd as int32 => data.len() as i64,
            _ => return kInvalidArgument,
        };
        let to = base.saturating_add(offset).max(0);
        *pos = usize::try_from(to)
            .unwrap_or(usize::MAX)
            .min(self.limit(data.len()));
        // SAFETY: the caller's out value, or null.
        if let Some(r) = unsafe { result.as_mut() } {
            *r = *pos as i64;
        }
        kResultOk
    }

    unsafe fn tell(&self, pos: *mut int64) -> tresult {
        let Ok(g) = self.inner.lock() else {
            return kResultFalse;
        };
        // SAFETY: the caller's out value, or null.
        match unsafe { pos.as_mut() } {
            Some(p) => {
                *p = g.1 as i64;
                kResultOk
            }
            None => kInvalidArgument,
        }
    }
}

impl ISizeableStreamTrait for MemStream {
    unsafe fn getStreamSize(&self, size: *mut int64) -> tresult {
        let Ok(g) = self.inner.lock() else {
            return kResultFalse;
        };
        // SAFETY: the caller's out value, or null.
        match unsafe { size.as_mut() } {
            Some(s) => {
                *s = g.0.len() as i64;
                kResultOk
            }
            None => kInvalidArgument,
        }
    }

    unsafe fn setStreamSize(&self, size: int64) -> tresult {
        if self.read_only {
            return kNotImplemented;
        }
        let Some(n) = usize::try_from(size).ok().filter(|n| *n <= MAX_STREAM) else {
            return kInvalidArgument;
        };
        let Ok(mut g) = self.inner.lock() else {
            return kResultFalse;
        };
        g.0.resize(n, 0);
        kResultOk
    }
}

/// A parameter edit a plug-in reported through its component handler,
/// with the normalised value it gave.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum Edit {
    /// `beginEdit`.
    Begin(ParamID),
    /// `performEdit` (normalised).
    Perform(ParamID, ParamValue),
    /// `endEdit`.
    End(ParamID),
}

/// The host's `IComponentHandler` (and `IComponentHandler2`). A plug-in
/// may call it from any thread, its processing thread included, so every
/// call only records what it was told (a fixed-capacity lock-free queue
/// and atomic flags, no allocation and no locks); the instance reads them
/// on the main thread ([`ComponentHandler::take_edits`] and friends), where
/// the controller converts values and restarts are handled.
pub(crate) struct ComponentHandler {
    edits: ArrayQueue<Edit>,
    restart: AtomicI32,
    dirty: AtomicBool,
    dropped: AtomicU64,
}

impl ComponentHandler {
    pub(crate) fn new() -> ComponentHandler {
        ComponentHandler {
            edits: ArrayQueue::new(EDIT_QUEUE),
            restart: AtomicI32::new(0),
            dirty: AtomicBool::new(false),
            dropped: AtomicU64::new(0),
        }
    }

    fn push(&self, e: Edit) -> tresult {
        if self.edits.push(e).is_err() {
            self.dropped.fetch_add(1, Ordering::Relaxed);
        }
        kResultOk
    }

    /// The edits since the last call, in order (main thread).
    pub(crate) fn take_edits(&self) -> Vec<Edit> {
        std::iter::from_fn(|| self.edits.pop()).collect()
    }

    /// The `restartComponent` flags since the last call.
    pub(crate) fn take_restart(&self) -> i32 {
        self.restart.swap(0, Ordering::AcqRel)
    }

    /// Whether `setDirty(true)` came since the last call.
    pub(crate) fn take_dirty(&self) -> bool {
        self.dirty.swap(false, Ordering::AcqRel)
    }

    /// Edits lost to a full queue so far.
    pub(crate) fn dropped(&self) -> u64 {
        self.dropped.load(Ordering::Relaxed)
    }
}

impl Class for ComponentHandler {
    type Interfaces = (IComponentHandler, IComponentHandler2);
}

impl IComponentHandlerTrait for ComponentHandler {
    unsafe fn beginEdit(&self, id: ParamID) -> tresult {
        self.push(Edit::Begin(id))
    }

    unsafe fn performEdit(&self, id: ParamID, value: ParamValue) -> tresult {
        self.push(Edit::Perform(id, value))
    }

    unsafe fn endEdit(&self, id: ParamID) -> tresult {
        self.push(Edit::End(id))
    }

    unsafe fn restartComponent(&self, flags: int32) -> tresult {
        self.restart.fetch_or(flags, Ordering::AcqRel);
        kResultOk
    }
}

impl IComponentHandler2Trait for ComponentHandler {
    unsafe fn setDirty(&self, state: TBool) -> tresult {
        if state != 0 {
            self.dirty.store(true, Ordering::Release);
        }
        kResultOk
    }

    unsafe fn requestOpenEditor(&self, _name: FIDString) -> tresult {
        kNotImplemented
    }

    unsafe fn startGroupEdit(&self) -> tresult {
        kResultOk
    }

    unsafe fn finishGroupEdit(&self) -> tresult {
        kResultOk
    }
}

/// One audio or event bus of a component.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct BusRecord {
    pub name: String,
    pub channels: i32,
    /// `kAux` (else `kMain`).
    pub aux: bool,
    pub default_active: bool,
}

/// One parameter as the controller describes it.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ParamRecord {
    pub id: ParamID,
    pub title: String,
    pub steps: i32,
    pub default_normalized: f64,
    pub can_automate: bool,
    pub read_only: bool,
    pub hidden: bool,
}

/// A plug-in's component and controller (one object for a single
/// component, two for a split plug-in), initialised, connected and given
/// the component handler. Dropping it undoes that in order: disconnect,
/// controller terminate, component terminate, then both are released.
/// The module they came from must outlive it.
pub(crate) struct Plugin {
    component: ComPtr<IComponent>,
    component_ready: bool,
    controller: Option<ComPtr<IEditController>>,
    /// A separate controller object that initialised (and so terminates).
    controller_ready: bool,
    connected: Option<(ComPtr<IConnectionPoint>, ComPtr<IConnectionPoint>)>,
    processor: Option<ComPtr<IAudioProcessor>>,
}

impl std::fmt::Debug for Plugin {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Plugin")
            .field("split", &self.controller_ready)
            .finish_non_exhaustive()
    }
}

impl Plugin {
    /// Creates class `cid` of `module` and sets it up as the SDK's
    /// `PlugProvider` does: the component is initialised with the host
    /// context; the controller is the component itself when it is one,
    /// else the class `getControllerClassId` names, created and
    /// initialised; a split pair is connected through `IConnectionPoint`
    /// when both sides offer it; the controller gets the component's state
    /// (`setComponentState`) and `handler`. A failure at any step undoes
    /// the steps before it and leaves nothing alive.
    pub(crate) fn create(
        module: &Module,
        cid: &[u8; 16],
        handler: &ComWrapper<ComponentHandler>,
    ) -> Result<Plugin, HostError> {
        let component = module.create::<IComponent>(cid)?;
        let mut p = Plugin {
            component,
            component_ready: false,
            controller: None,
            controller_ready: false,
            connected: None,
            processor: None,
        };
        let context = module.host_context();
        // SAFETY: the context is the module's, alive while it is.
        if unsafe { p.component.initialize(context) } != kResultOk {
            return Err(HostError::LoadFailed(
                "the plug-in's component did not initialise".into(),
            ));
        }
        p.component_ready = true;
        p.processor = p.component.cast::<IAudioProcessor>();
        if let Some(own) = p.component.cast::<IEditController>() {
            p.controller = Some(own);
        } else {
            let mut tuid: TUID = [0; 16];
            // SAFETY: `tuid` has room for one class ID.
            let named = unsafe { p.component.getControllerClassId(&mut tuid) } == kResultOk;
            let ccid = tuid.map(|b| b as u8);
            if named && ccid != [0; 16] {
                let controller = module
                    .create::<IEditController>(&ccid)
                    .map_err(|e| HostError::LoadFailed(format!("the plug-in's controller: {e}")))?;
                // SAFETY: as for the component.
                let ok = unsafe { controller.initialize(context) } == kResultOk;
                p.controller = Some(controller);
                if !ok {
                    return Err(HostError::LoadFailed(
                        "the plug-in's controller did not initialise".into(),
                    ));
                }
                p.controller_ready = true;
                p.connect();
            }
        }
        if let Some(c) = &p.controller {
            let stream = ComWrapper::new(MemStream::default());
            if let Some(s) = stream.to_com_ptr::<IBStream>() {
                // SAFETY: the stream is live for both calls.
                if unsafe { p.component.getState(s.as_ptr()) } == kResultOk {
                    stream.rewind();
                    // SAFETY: as above.
                    unsafe { c.setComponentState(s.as_ptr()) };
                }
            }
            if let Some(h) = handler.to_com_ptr::<IComponentHandler>() {
                // SAFETY: the controller takes its own reference.
                unsafe { c.setComponentHandler(h.as_ptr()) };
            }
        }
        Ok(p)
    }

    /// Connects a split pair when both sides are connection points.
    fn connect(&mut self) {
        let Some(controller) = &self.controller else {
            return;
        };
        if let (Some(a), Some(b)) = (
            self.component.cast::<IConnectionPoint>(),
            controller.cast::<IConnectionPoint>(),
        ) {
            // SAFETY: both are live; each side takes its own reference.
            unsafe {
                a.connect(b.as_ptr());
                b.connect(a.as_ptr());
            }
            self.connected = Some((a, b));
        }
    }

    /// The buses of one media type and direction.
    fn buses(&self, media: i32, input: bool) -> Vec<BusRecord> {
        let dir = if input { 0 } else { 1 };
        // SAFETY: a plain query.
        let n = unsafe { self.component.getBusCount(media, dir) }.clamp(0, 64);
        (0..n)
            .filter_map(|i| {
                // SAFETY: all-zero is valid for this plain struct.
                let mut b: BusInfo = unsafe { std::mem::zeroed() };
                // SAFETY: the component fills the struct we own.
                (unsafe { self.component.getBusInfo(media, dir, i, &mut b) } == kResultOk).then(
                    || BusRecord {
                        name: w_text(&b.name),
                        channels: b.channelCount,
                        aux: b.busType == BusTypes_::kAux as i32,
                        default_active: b.flags & BusFlags_::kDefaultActive != 0,
                    },
                )
            })
            .collect()
    }

    /// The audio buses, inputs or outputs.
    pub(crate) fn audio_buses(&self, input: bool) -> Vec<BusRecord> {
        self.buses(MediaTypes_::kAudio as i32, input)
    }

    /// The event (note) input buses.
    pub(crate) fn event_inputs(&self) -> Vec<BusRecord> {
        self.buses(MediaTypes_::kEvent as i32, true)
    }

    /// The controller's parameters, in its order.
    pub(crate) fn params(&self) -> Vec<ParamRecord> {
        let Some(c) = &self.controller else {
            return Vec::new();
        };
        // SAFETY: a plain query.
        let n = unsafe { c.getParameterCount() }.clamp(0, 1 << 16);
        (0..n)
            .filter_map(|i| {
                // SAFETY: all-zero is valid for this plain struct.
                let mut p: ParameterInfo = unsafe { std::mem::zeroed() };
                // SAFETY: the controller fills the struct we own.
                (unsafe { c.getParameterInfo(i, &mut p) } == kResultOk).then(|| ParamRecord {
                    id: p.id,
                    title: w_text(&p.title),
                    steps: p.stepCount,
                    default_normalized: p.defaultNormalizedValue,
                    can_automate: p.flags & ParameterFlags_::kCanAutomate != 0,
                    read_only: p.flags & ParameterFlags_::kIsReadOnly != 0,
                    hidden: p.flags & ParameterFlags_::kIsHidden != 0,
                })
            })
            .collect()
    }

    /// A normalised value as a plain one (the controller's mapping).
    pub(crate) fn to_plain(&self, id: ParamID, n: f64) -> f64 {
        match &self.controller {
            // SAFETY: a plain call.
            Some(c) => unsafe { c.normalizedParamToPlain(id, n) },
            None => n,
        }
    }

    /// A plain value as a normalised one.
    pub(crate) fn to_normalized(&self, id: ParamID, plain: f64) -> f64 {
        match &self.controller {
            // SAFETY: a plain call.
            Some(c) => unsafe { c.plainParamToNormalized(id, plain) },
            None => plain,
        }
    }

    /// The controller's current normalised value.
    pub(crate) fn normalized(&self, id: ParamID) -> Option<f64> {
        // SAFETY: a plain call.
        self.controller
            .as_ref()
            .map(|c| unsafe { c.getParamNormalized(id) })
    }

    /// Sets the controller's normalised value.
    pub(crate) fn set_normalized(&self, id: ParamID, n: f64) -> bool {
        // SAFETY: a plain call.
        self.controller
            .as_ref()
            .is_some_and(|c| unsafe { c.setParamNormalized(id, n) } == kResultOk)
    }

    /// `IAudioProcessor::setupProcessing` (32-bit samples) in real-time or
    /// offline mode: whether it was taken, or `None` without a processor.
    /// The component must be inactive.
    pub(crate) fn setup_processing(
        &self,
        offline: bool,
        sample_rate: f64,
        max_frames: u32,
    ) -> Option<bool> {
        let p = self.processor.as_ref()?;
        let mut setup = ProcessSetup {
            processMode: if offline {
                ProcessModes_::kOffline as i32
            } else {
                ProcessModes_::kRealtime as i32
            },
            symbolicSampleSize: SymbolicSampleSizes_::kSample32 as i32,
            maxSamplesPerBlock: i32::try_from(max_frames).unwrap_or(i32::MAX),
            sampleRate: sample_rate,
        };
        // SAFETY: `setup` is ours for the call.
        Some(unsafe { p.setupProcessing(&mut setup) } == kResultOk)
    }
}

/// MIDI controller numbers the host maps through `IMidiMapping`: CC 0–127,
/// then channel aftertouch (`kAfterTouch`) and pitch bend (`kPitchBend`).
pub(crate) const MIDI_CONTROLLERS: usize = ControllerNumbers_::kCountCtrlNumber as usize;

impl Plugin {
    /// Whether the processor takes 32-bit samples.
    pub(crate) fn can_process_f32(&self) -> bool {
        self.processor.as_ref().is_some_and(|p| {
            // SAFETY: a plain query.
            let r = unsafe { p.canProcessSampleSize(SymbolicSampleSizes_::kSample32 as int32) };
            r == kResultOk
        })
    }

    /// Confirms the speaker arrangements the processor reports for its
    /// audio buses (`getBusArrangement`, then `setBusArrangements` with
    /// the same): the host takes the plug-in's layout. A refusal is
    /// harmless; the buses' channel counts are read afterwards.
    pub(crate) fn keep_arrangements(&self) {
        let Some(p) = &self.processor else {
            return;
        };
        let read = |input: bool| -> Vec<SpeakerArrangement> {
            let dir = if input { 0 } else { 1 };
            self.audio_buses(input)
                .iter()
                .enumerate()
                .map(|(i, b)| {
                    let mut a: SpeakerArrangement = 0;
                    let i = i32::try_from(i).unwrap_or(0);
                    // SAFETY: `a` is ours to fill.
                    if unsafe { p.getBusArrangement(dir, i, &mut a) } == kResultOk {
                        a
                    } else {
                        arrangement_of(b.channels)
                    }
                })
                .collect()
        };
        let mut ins = read(true);
        let mut outs = read(false);
        let n_in = i32::try_from(ins.len()).unwrap_or(0);
        let n_out = i32::try_from(outs.len()).unwrap_or(0);
        // SAFETY: both arrays live for the call and hold the counts given.
        unsafe { p.setBusArrangements(ins.as_mut_ptr(), n_in, outs.as_mut_ptr(), n_out) };
    }

    /// Turns every audio bus and the first event input on (or off).
    pub(crate) fn activate_buses(&self, on: bool) {
        let state = TBool::from(on);
        let audio = MediaTypes_::kAudio as i32;
        for (dir, n) in [
            (0, self.audio_buses(true).len()),
            (1, self.audio_buses(false).len()),
        ] {
            for i in 0..i32::try_from(n).unwrap_or(0) {
                // SAFETY: a plain call on a bus the component lists.
                unsafe { self.component.activateBus(audio, dir, i, state) };
            }
        }
        if !self.event_inputs().is_empty() {
            // SAFETY: as above.
            unsafe {
                self.component
                    .activateBus(MediaTypes_::kEvent as i32, 0, 0, state)
            };
        }
    }

    /// `IComponent::setActive`; whether it succeeded.
    pub(crate) fn set_active(&self, on: bool) -> bool {
        // SAFETY: a plain call (main thread).
        unsafe { self.component.setActive(TBool::from(on)) == kResultOk }
    }

    /// `getLatencySamples` (0 without a processor).
    pub(crate) fn latency(&self) -> u32 {
        // SAFETY: a plain query.
        self.processor
            .as_ref()
            .map_or(0, |p| unsafe { p.getLatencySamples() })
    }

    /// `getTailSamples` (`kNoTail` without a processor).
    pub(crate) fn tail(&self) -> u32 {
        // SAFETY: a plain query.
        self.processor
            .as_ref()
            .map_or(0, |p| unsafe { p.getTailSamples() })
    }

    /// The controller's MIDI controller assignments for event bus 0
    /// (`IMidiMapping`): `MIDI_CONTROLLERS` entries per channel, 16
    /// channels, `kNoParamId` where nothing is assigned; empty without
    /// `IMidiMapping`.
    pub(crate) fn midi_mapping(&self) -> Vec<ParamID> {
        let Some(m) = self
            .controller
            .as_ref()
            .and_then(|c| c.cast::<IMidiMapping>())
        else {
            return Vec::new();
        };
        let mut map = vec![kNoParamId; 16 * MIDI_CONTROLLERS];
        for (k, slot) in map.iter_mut().enumerate() {
            let channel = (k / MIDI_CONTROLLERS) as i16;
            let ctrl = (k % MIDI_CONTROLLERS) as i16;
            let mut id: ParamID = kNoParamId;
            // SAFETY: `id` is ours to fill.
            if unsafe { m.getMidiControllerAssignment(0, channel, ctrl, &mut id) } == kResultOk {
                *slot = id;
            }
        }
        map
    }

    /// The processor, for the audio thread's half
    /// ([`super::process_ffi::RtData`]).
    pub(crate) fn audio_processor(&self) -> Option<ComPtr<IAudioProcessor>> {
        self.processor.clone()
    }
}

/// A state call's result that the SDK's preset files take as success
/// (`vstpresetfile.cpp`'s `verify`): done, or nothing to do.
fn state_ok(r: tresult) -> bool {
    r == kResultOk || r == kNotImplemented
}

/// State (host track H1 Task 5): each call gets its own stream, a writer
/// for a `getState` and a reader over exactly one chunk for a `setState`,
/// as the SDK's `PresetFile` gives them. Main thread.
impl Plugin {
    /// Runs `get` with a fresh writer and returns what it wrote.
    fn read_out(&self, get: impl FnOnce(*mut IBStream) -> tresult) -> Result<Vec<u8>, String> {
        let stream = ComWrapper::new(MemStream::default());
        let s = stream
            .to_com_ptr::<IBStream>()
            .ok_or_else(|| "no stream".to_string())?;
        if !state_ok(get(s.as_ptr())) {
            return Err("the plug-in did not save its state".into());
        }
        Ok(stream.take())
    }

    /// Runs `set` with a reader over `bytes`.
    fn feed(&self, bytes: &[u8], set: impl FnOnce(*mut IBStream) -> tresult) -> bool {
        let stream = ComWrapper::new(MemStream::reader(bytes));
        stream
            .to_com_ptr::<IBStream>()
            .is_some_and(|s| state_ok(set(s.as_ptr())))
    }

    /// The component's state (`IComponent::getState`).
    pub(crate) fn component_state(&self) -> Result<Vec<u8>, String> {
        // SAFETY: the stream is live for the call.
        self.read_out(|s| unsafe { self.component.getState(s) })
    }

    /// The controller's own state (`IEditController::getState`), or
    /// `None` when there is no controller.
    pub(crate) fn controller_state(&self) -> Result<Option<Vec<u8>>, String> {
        let Some(c) = &self.controller else {
            return Ok(None);
        };
        // SAFETY: as above.
        self.read_out(|s| unsafe { c.getState(s) }).map(Some)
    }

    /// `IComponent::setState(bytes)`; whether the plug-in took it.
    pub(crate) fn set_component_state(&self, bytes: &[u8]) -> bool {
        // SAFETY: as above.
        self.feed(bytes, |s| unsafe { self.component.setState(s) })
    }

    /// The controller's `setComponentState(bytes)` (the component's
    /// state); true without a controller.
    pub(crate) fn sync_controller(&self, bytes: &[u8]) -> bool {
        let Some(c) = &self.controller else {
            return true;
        };
        // SAFETY: as above.
        self.feed(bytes, |s| unsafe { c.setComponentState(s) })
    }

    /// The controller's `setState(bytes)` (its own state); true without
    /// a controller.
    pub(crate) fn set_controller_state(&self, bytes: &[u8]) -> bool {
        let Some(c) = &self.controller else {
            return true;
        };
        // SAFETY: as above.
        self.feed(bytes, |s| unsafe { c.setState(s) })
    }
}

/// The speaker arrangement of `channels` channels: mono, else the first
/// `channels` speakers (stereo for 2).
fn arrangement_of(channels: i32) -> SpeakerArrangement {
    match channels {
        c if c <= 1 => SpeakerArr::kMono,
        c if c >= 64 => SpeakerArrangement::MAX,
        c => (1u64 << c) - 1,
    }
}

impl Drop for Plugin {
    fn drop(&mut self) {
        if let Some((a, b)) = self.connected.take() {
            // SAFETY: both are live; disconnecting drops each side's
            // reference to the other.
            unsafe {
                a.disconnect(b.as_ptr());
                b.disconnect(a.as_ptr());
            }
        }
        if self.controller_ready {
            if let Some(c) = &self.controller {
                // SAFETY: it initialised; terminated once, before the
                // component.
                unsafe { c.terminate() };
            }
        }
        if self.component_ready {
            // SAFETY: it initialised; terminated once (a single component's
            // controller is this same object).
            unsafe { self.component.terminate() };
        }
        // Released here (the component when the field drops after).
        self.processor = None;
        self.controller = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SET: int32 = IStreamSeekMode_::kIBSeekSet as int32;
    const CUR: int32 = IStreamSeekMode_::kIBSeekCur as int32;
    const END: int32 = IStreamSeekMode_::kIBSeekEnd as int32;

    fn read(s: &MemStream, n: int32) -> (tresult, Vec<u8>) {
        let mut buf = vec![0xEEu8; usize::try_from(n.max(0)).unwrap()];
        let mut got: int32 = -1;
        // SAFETY: `buf` holds `n` bytes.
        let r = unsafe { s.read(buf.as_mut_ptr() as *mut c_void, n, &mut got) };
        buf.truncate(usize::try_from(got).unwrap());
        (r, buf)
    }

    fn write(s: &MemStream, b: &[u8]) -> (tresult, int32) {
        let mut put: int32 = -1;
        // SAFETY: `b` holds its length; the stream only reads it.
        let r = unsafe { s.write(b.as_ptr() as *mut c_void, b.len() as int32, &mut put) };
        (r, put)
    }

    fn seek(s: &MemStream, to: int64, mode: int32) -> (tresult, int64) {
        let mut at: int64 = -1;
        // SAFETY: plain call.
        let r = unsafe { s.seek(to, mode, &mut at) };
        (r, at)
    }

    fn size(s: &MemStream) -> int64 {
        let mut n: int64 = -1;
        // SAFETY: plain call.
        assert_eq!(unsafe { s.getStreamSize(&mut n) }, kResultOk);
        n
    }

    #[test]
    fn a_reader_reads_what_is_there_and_stops_at_its_ends() {
        let s = MemStream::reader(b"abcdef");
        assert_eq!(read(&s, 4), (kResultOk, b"abcd".to_vec()));
        // Past the end: what is left, then 0 bytes, still a success.
        assert_eq!(read(&s, 4), (kResultOk, b"ef".to_vec()));
        assert_eq!(read(&s, 4), (kResultOk, Vec::new()));
        assert_eq!(read(&s, 0), (kResultOk, Vec::new()));
        assert_eq!(read(&s, -5), (kResultOk, Vec::new()));
        // A seek stops at the end and at the start.
        assert_eq!(seek(&s, 100, SET), (kResultOk, 6));
        assert_eq!(seek(&s, 10, END), (kResultOk, 6));
        assert_eq!(seek(&s, -100, CUR), (kResultOk, 0));
        assert_eq!(seek(&s, i64::MIN, CUR), (kResultOk, 0));
        assert_eq!(seek(&s, -2, END), (kResultOk, 4));
        assert_eq!(read(&s, 9), (kResultOk, b"ef".to_vec()));
        assert_eq!(seek(&s, 0, 7).0, kInvalidArgument);
        // Read only.
        assert_eq!(write(&s, b"x"), (kNotImplemented, 0));
        // SAFETY: plain call.
        assert_eq!(unsafe { s.setStreamSize(0) }, kNotImplemented);
        assert_eq!(size(&s), 6);
        let mut at: int64 = -1;
        // SAFETY: plain call.
        assert_eq!(unsafe { s.tell(&mut at) }, kResultOk);
        assert_eq!(at, 6);
        // A null buffer with bytes asked for is refused; with none, not.
        let mut got: int32 = -1;
        // SAFETY: the stream checks the null buffer.
        unsafe {
            assert_eq!(s.read(std::ptr::null_mut(), 1, &mut got), kInvalidArgument);
            assert_eq!(got, 0);
            assert_eq!(
                s.read(std::ptr::null_mut(), 0, std::ptr::null_mut()),
                kResultOk
            );
        }
    }

    #[test]
    fn a_writer_may_seek_past_the_end_and_fills_the_gap_with_zeros() {
        let s = MemStream::default();
        assert_eq!(write(&s, b"ab"), (kResultOk, 2));
        assert_eq!(seek(&s, 3, CUR), (kResultOk, 5));
        assert_eq!(size(&s), 2, "a seek alone writes nothing");
        assert_eq!(read(&s, 4), (kResultOk, Vec::new()), "past the end");
        assert_eq!(write(&s, b"cd"), (kResultOk, 2));
        assert_eq!(seek(&s, 1, SET), (kResultOk, 1));
        assert_eq!(write(&s, b"B"), (kResultOk, 1));
        assert_eq!(s.take(), b"aB\0\0\0cd");
        assert_eq!(size(&s), 0);
        // Not beyond the limit.
        assert_eq!(seek(&s, i64::MAX, SET), (kResultOk, MAX_STREAM as i64));
        assert_eq!(write(&s, b"x"), (kResultFalse, 0));
        assert_eq!(size(&s), 0);
        // SAFETY: plain calls.
        unsafe {
            assert_eq!(s.setStreamSize(3), kResultOk);
            assert_eq!(s.setStreamSize(-1), kInvalidArgument);
            assert_eq!(s.setStreamSize(MAX_STREAM as i64 + 1), kInvalidArgument);
        }
        assert_eq!(size(&s), 3);
    }
}
