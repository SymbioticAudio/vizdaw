//! The VST3 host's `unsafe` calls: opening a module's library, its module
//! entry and exit, and the factory's COM calls. This mirrors the SDK's
//! `public.sdk/source/vst/hosting/module_mac.mm` and `module_linux.cpp`:
//! on macOS `bundleEntry(CFBundleRef)` (or the older `BundleEntry`) runs
//! before `GetPluginFactory` and `bundleExit` after the factory is
//! released; on Linux `ModuleEntry(handle)` and `ModuleExit`.

use super::module::{ClassInfo, binary_path, c_text, w_text};
use crate::error::HostError;
use std::ffi::c_void;
use std::path::Path;
use vst3::Steinberg::{
    FIDString, IPluginFactory, IPluginFactory2, IPluginFactory2Trait, IPluginFactory3,
    IPluginFactory3Trait, IPluginFactoryTrait, PClassInfo, PClassInfo2, PClassInfoW, PFactoryInfo,
    kResultOk,
};
use vst3::{ComPtr, Interface};

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
/// it releases the factory, runs the module exit and unloads the library,
/// in that order; every COM object made from the factory must be released
/// first.
pub(crate) struct Module {
    factory: Option<ComPtr<IPluginFactory>>,
    exit: Option<ModuleExit>,
    // Dropped after `drop` ran the exit: the library, then (macOS) the
    // bundle reference the entry got.
    _library: libloading::Library,
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
            let library =
                unsafe { libloading::Library::new(&binary) }.map_err(|e| load_failed(bundle, e))?;
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
            let library = unsafe { Library::open(Some(&binary), RTLD_LAZY | RTLD_LOCAL) }
                .map_err(|e| load_failed(bundle, e))?;
            type Entry = unsafe extern "system" fn(*mut c_void) -> bool;
            // SAFETY: the SDK declares `bool ModuleEntry(void* sharedLibraryHandle)`.
            let entry = unsafe { library.get::<Entry>(b"ModuleEntry\0") }
                .map(|s| *s)
                .map_err(|_| load_failed(bundle, "no ModuleEntry function"))?;
            // The entry gets the `dlopen` handle, as in the SDK.
            let handle = library.into_raw();
            // SAFETY: `handle` came from `into_raw` just now.
            let library = unsafe { Library::from_raw(handle) };
            // SAFETY: the handle is the open library's.
            let entered = unsafe { entry(handle) };
            (libloading::Library::from(library), entered)
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
        module.factory = Some(factory);
        Ok(module)
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
