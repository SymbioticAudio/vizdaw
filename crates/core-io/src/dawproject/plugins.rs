//! Plug-in state files (plan 6d Ruling 8): CLAP state is written as the
//! plug-in's raw stream, AU state as an `.aupreset` property list in XML.
//! The property-list conversion is the platform's ([`PlistCodec`]).

use core_model::mixer::PluginFormat;

/// Converts an AU state between the binary property list VizDAW stores and
/// the XML property list an `.aupreset` holds.
pub trait PlistCodec: Send + Sync {
    /// A binary (or any) property list as an XML property list.
    fn to_xml(&self, binary: &[u8]) -> Result<Vec<u8>, String>;
    /// An XML (or any) property list as a binary property list.
    fn to_binary(&self, plist: &[u8]) -> Result<Vec<u8>, String>;
}

/// Why [`NoPlist`] refuses.
const NEEDS_MACOS: &str = "converting Audio Unit settings needs macOS";

/// The codec where there is none (Linux, dry runs, tests): it refuses both
/// ways.
#[derive(Debug, Clone, Copy, Default)]
pub struct NoPlist;

impl PlistCodec for NoPlist {
    fn to_xml(&self, _binary: &[u8]) -> Result<Vec<u8>, String> {
        Err(NEEDS_MACOS.to_owned())
    }

    fn to_binary(&self, _plist: &[u8]) -> Result<Vec<u8>, String> {
        Err(NEEDS_MACOS.to_owned())
    }
}

/// The package entry of the `n`th state file written (from 1):
/// `plugins/001.aupreset` or `plugins/001.clap-preset`.
pub fn state_entry(n: usize, format: PluginFormat) -> String {
    let ext = match format {
        PluginFormat::Au => "aupreset",
        PluginFormat::Clap => "clap-preset",
    };
    format!("plugins/{n:03}.{ext}")
}
