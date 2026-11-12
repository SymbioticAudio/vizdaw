//! Property-list conversion (plan 6d Ruling 8): an AU state between the
//! binary property list VizDAW stores and the XML property list an
//! `.aupreset` holds, through CoreFoundation
//! (`CFPropertyListCreateWithData`, `CFPropertyListCreateData`).

use objc2_core_foundation::{
    CFData, CFDictionary, CFPropertyListCreateData, CFPropertyListCreateWithData,
    CFPropertyListFormat,
};
use std::ptr;

/// `plist` (any property-list format) written in `format`. An AU state is
/// a dictionary (`ClassInfo`): anything else is refused, so text that the
/// old plain-text format would read as a string is not taken.
fn convert(plist: &[u8], format: CFPropertyListFormat) -> Result<Vec<u8>, String> {
    if plist.is_empty() {
        return Err("the settings are empty".to_owned());
    }
    let data = CFData::from_bytes(plist);
    // SAFETY: `data` is a valid CFData; options 0 (immutable); null format
    // and error pointers are allowed.
    let parsed = unsafe {
        CFPropertyListCreateWithData(None, Some(&data), 0, ptr::null_mut(), ptr::null_mut())
    }
    .ok_or("the settings are not a property list")?;
    if parsed.downcast_ref::<CFDictionary>().is_none() {
        return Err("the settings are not a property-list dictionary".to_owned());
    }
    // SAFETY: `parsed` is a valid property list; null error pointer.
    let out = unsafe { CFPropertyListCreateData(None, Some(&parsed), format, 0, ptr::null_mut()) }
        .ok_or("the settings could not be written as a property list")?;
    Ok(out.to_vec())
}

/// Any property list as an XML property list (`XMLFormat_v1_0`).
pub fn to_xml(plist: &[u8]) -> Result<Vec<u8>, String> {
    convert(plist, CFPropertyListFormat::XMLFormat_v1_0)
}

/// Any property list as a binary property list (`BinaryFormat_v1_0`).
pub fn to_binary(plist: &[u8]) -> Result<Vec<u8>, String> {
    convert(plist, CFPropertyListFormat::BinaryFormat_v1_0)
}
