//! The safe half of loading a VST3 module: where a bundle's binary is,
//! class IDs, and turning the factory's class records into plug-ins. The
//! `unsafe` half (opening the library, the module entry and exit, the
//! factory's COM calls) is in `ffi.rs`.

use crate::error::HostError;
use crate::info::PluginKind;
use core_model::mixer::{PluginFormat as Format, PluginRef};
use std::ffi::c_char;
use std::path::{Path, PathBuf};

/// The category of a factory class that is a plug-in (the SDK's
/// `kVstAudioEffectClass`); controllers and other helper classes have
/// other categories.
pub(crate) const AUDIO_MODULE_CLASS: &str = "Audio Module Class";

/// One class a module's factory lists, from the richest record the factory
/// has (`PClassInfoW`, then `PClassInfo2`, then `PClassInfo`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ClassInfo {
    /// The 16-byte class ID.
    pub cid: [u8; 16],
    /// `Audio Module Class` for plug-ins.
    pub category: String,
    /// Display name.
    pub name: String,
    /// `|`-separated, such as `Fx|Delay` or `Instrument|Synth` (empty
    /// without `PClassInfo2`).
    pub sub_categories: String,
    /// Vendor (empty: the factory's).
    pub vendor: String,
    /// Version (empty when not given).
    pub version: String,
}

/// The class ID as 32 upper-case hex digits (the SDK's `FUID::toString`
/// spelling on macOS and Linux; what `core_model::mixer::vst3_class_id`
/// keeps).
pub fn class_id_hex(cid: &[u8; 16]) -> String {
    cid.iter().map(|b| format!("{b:02X}")).collect()
}

/// The class ID spelled by `text` (32 hex digits, either case).
pub fn parse_class_id(text: &str) -> Option<[u8; 16]> {
    let text = core_model::mixer::vst3_class_id(text)?;
    let mut cid = [0u8; 16];
    for (i, b) in cid.iter_mut().enumerate() {
        let pair = text.get(2 * i..2 * i + 2)?;
        *b = u8::from_str_radix(pair, 16).ok()?;
    }
    Some(cid)
}

/// The text of a fixed-size `char8` field, up to its first NUL.
pub(crate) fn c_text(chars: &[c_char]) -> String {
    let bytes: Vec<u8> = chars
        .iter()
        .map(|&c| c as u8)
        .take_while(|&b| b != 0)
        .collect();
    String::from_utf8_lossy(&bytes).trim().to_string()
}

/// The text of a fixed-size `char16` field, up to its first NUL.
pub(crate) fn w_text(chars: &[u16]) -> String {
    let units: Vec<u16> = chars.iter().copied().take_while(|&u| u != 0).collect();
    String::from_utf16_lossy(&units).trim().to_string()
}

/// The string value of `key` in the bundle's `Contents/Info.plist`.
pub(crate) fn plist_value(bundle: &Path, key: &str) -> Option<String> {
    let text = std::fs::read_to_string(bundle.join("Contents").join("Info.plist")).ok()?;
    let key = format!("<key>{key}</key>");
    let (_, rest) = text.split_once(&key)?;
    let (_, rest) = rest.split_once("<string>")?;
    let (v, _) = rest.split_once("</string>")?;
    Some(v.trim().to_string()).filter(|v| !v.is_empty())
}

/// The `Version` in the bundle's `Contents/Resources/moduleinfo.json`
/// (SDK 3.7.5 and later). The file is JSON5, so it is searched rather than
/// parsed.
pub(crate) fn moduleinfo_version(bundle: &Path) -> Option<String> {
    let path = bundle
        .join("Contents")
        .join("Resources")
        .join("moduleinfo.json");
    let text = std::fs::read_to_string(path).ok()?;
    let (_, rest) = text.split_once("\"Version\"")?;
    let (_, rest) = rest.trim_start().strip_prefix(':')?.split_once('"')?;
    let (v, _) = rest.split_once('"')?;
    Some(v.trim().to_string()).filter(|v| !v.is_empty())
}

/// The bundle's version: `CFBundleShortVersionString` from its plist, else
/// the `Version` of its `moduleinfo.json`, else "".
pub(crate) fn bundle_version(bundle: &Path) -> String {
    plist_value(bundle, "CFBundleShortVersionString")
        .or_else(|| moduleinfo_version(bundle))
        .unwrap_or_default()
}

/// The folder name of this machine's Linux binaries (`x86_64-linux`,
/// `aarch64-linux`, …; the SDK uses the `uname` machine name).
#[cfg(not(target_os = "macos"))]
fn linux_arch_dir() -> String {
    let arch = match std::env::consts::ARCH {
        "x86" => "i386",
        "arm" => "armv7l",
        a => a,
    };
    format!("{arch}-linux")
}

/// The binary of the VST3 bundle at `bundle`: `Contents/MacOS/<name>` on
/// macOS (`<name>` from `CFBundleExecutable`, else the bundle's name), and
/// `Contents/<arch>-linux/<name>.so` on Linux. A `.vst3` that is a file is
/// its own binary.
pub fn binary_path(bundle: &Path) -> Result<PathBuf, HostError> {
    if bundle.is_file() {
        return Ok(bundle.to_path_buf());
    }
    let stem = bundle
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    let contents = bundle.join("Contents");
    #[cfg(target_os = "macos")]
    let path = {
        let name = plist_value(bundle, "CFBundleExecutable").unwrap_or(stem);
        contents.join("MacOS").join(name)
    };
    #[cfg(not(target_os = "macos"))]
    let path = contents.join(linux_arch_dir()).join(format!("{stem}.so"));
    if path.is_file() {
        Ok(path)
    } else {
        Err(HostError::LoadFailed(format!(
            "{}: no binary at {}",
            bundle.display(),
            path.display()
        )))
    }
}

/// Instrument when the sub-categories name one, else effect.
pub(crate) fn kind_of(sub_categories: &str) -> PluginKind {
    if sub_categories
        .split('|')
        .any(|c| c.trim().eq_ignore_ascii_case("Instrument"))
    {
        PluginKind::Instrument
    } else {
        PluginKind::Effect
    }
}

/// The plug-ins among a factory's `classes`, in factory order; a class
/// without a vendor gets the factory's.
pub(crate) fn plugins(classes: &[ClassInfo], factory_vendor: &str) -> Vec<(PluginRef, PluginKind)> {
    classes
        .iter()
        .filter(|c| c.category == AUDIO_MODULE_CLASS)
        .map(|c| {
            let id = class_id_hex(&c.cid);
            (
                PluginRef {
                    format: Format::Vst3,
                    name: if c.name.is_empty() {
                        id.clone()
                    } else {
                        c.name.clone()
                    },
                    vendor: if c.vendor.is_empty() {
                        factory_vendor.to_string()
                    } else {
                        c.vendor.clone()
                    },
                    version: c.version.clone(),
                    id,
                },
                kind_of(&c.sub_categories),
            )
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn class(category: &str, sub: &str, vendor: &str) -> ClassInfo {
        ClassInfo {
            cid: [0xAB; 16],
            category: category.into(),
            name: String::new(),
            sub_categories: sub.into(),
            vendor: vendor.into(),
            version: "1".into(),
        }
    }

    #[test]
    fn instruments_are_named_by_their_sub_categories() {
        assert_eq!(kind_of("Instrument|Synth"), PluginKind::Instrument);
        assert_eq!(kind_of("Fx|Instrument"), PluginKind::Instrument);
        assert_eq!(kind_of("Fx|Delay"), PluginKind::Effect);
        assert_eq!(kind_of(""), PluginKind::Effect);
    }

    #[test]
    fn only_audio_modules_are_plugins_and_names_and_vendors_fall_back() {
        let got = plugins(
            &[
                class(AUDIO_MODULE_CLASS, "Fx", ""),
                class("Component Controller Class", "", "X"),
                class(AUDIO_MODULE_CLASS, "Instrument", "Own"),
            ],
            "Factory Vendor",
        );
        assert_eq!(got.len(), 2);
        assert_eq!(got[0].0.id, "AB".repeat(16));
        assert_eq!(got[0].0.name, "AB".repeat(16));
        assert_eq!(got[0].0.vendor, "Factory Vendor");
        assert_eq!(got[1].0.vendor, "Own");
        assert_eq!(got[1].1, PluginKind::Instrument);
    }

    #[test]
    fn fixed_size_text_stops_at_the_first_nul_or_the_end() {
        let c: Vec<c_char> = b"Gain\0junk".iter().map(|&b| b as c_char).collect();
        assert_eq!(c_text(&c), "Gain");
        let full: Vec<c_char> = b"abcd".iter().map(|&b| b as c_char).collect();
        assert_eq!(c_text(&full), "abcd");
        let w: Vec<u16> = "Synth ♪\0x".encode_utf16().collect();
        assert_eq!(w_text(&w), "Synth ♪");
    }
}
