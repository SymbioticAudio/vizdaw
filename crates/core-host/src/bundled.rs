//! The plug-ins VizDAW ships inside its app bundle (plan 6g): the Multi-Q
//! EQ, and the folder they live in. That folder is searched before the
//! standard CLAP folders, and its copy of a plug-in wins over another
//! copy with the same ID (see [`crate::catalog::Catalog::set_preferred_dir`]).

use std::ffi::OsString;
use std::path::{Path, PathBuf};

/// The CLAP ID of the EQ VizDAW bundles (Ruling 1, Branch A).
pub const MULTIQ_CLAP_ID: &str = "com.dusk-audio.multi-q";
/// Its AU ID as VizDAW spells AU IDs (`type/subtype/manufacturer`).
pub const MULTIQ_AU_ID: &str = "aufx/MulQ/Dusk";
/// The CLAP bundle's name inside the folder.
pub const MULTIQ_CLAP_BUNDLE: &str = "Multi-Q.clap";
/// The AU bundle's name inside the folder.
pub const MULTIQ_AU_BUNDLE: &str = "Multi-Q.component";

/// The environment variable that names the folder (dev runs and tests).
pub const ENV: &str = "VIZDAW_BUNDLED_PLUGINS";

/// The app's own plug-in folder (Ruling 3): `VIZDAW_BUNDLED_PLUGINS` when
/// it names an existing folder, else `Contents/PlugIns` of the app the
/// running executable is in, else `None`.
pub fn dir() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok();
    dir_from(std::env::var_os(ENV), exe.as_deref())
}

/// [`dir`] computed from explicit inputs: `env` is the variable's value,
/// `exe` the running executable. An empty `env` or one that is not an
/// existing folder counts as unset. `exe` counts only when it sits in
/// `<X>.app/Contents/MacOS/` and `<X>.app/Contents/PlugIns` exists.
pub fn dir_from(env: Option<OsString>, exe: Option<&Path>) -> Option<PathBuf> {
    if let Some(v) = env.filter(|v| !v.is_empty()) {
        let p = PathBuf::from(v);
        if p.is_dir() {
            return Some(p);
        }
    }
    let macos = exe?.parent()?;
    if macos.file_name()? != "MacOS" {
        return None;
    }
    let contents = macos.parent()?;
    if contents.file_name()? != "Contents" {
        return None;
    }
    let app = contents.parent()?;
    if app.extension()? != "app" {
        return None;
    }
    let plugins = contents.join("PlugIns");
    plugins.is_dir().then_some(plugins)
}
