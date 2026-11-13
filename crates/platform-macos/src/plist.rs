//! Property-list conversion (plan 6d Ruling 8): an AU state between the
//! binary property list VizDAW stores and the XML property list an
//! `.aupreset` holds, through CoreFoundation
//! (`CFPropertyListCreateWithData`, `CFPropertyListCreateData`).
//!
//! Both inputs are checked before CoreFoundation sees them. A binary
//! property list may name one object from many places (a DAG), and
//! CoreFoundation's writers write every path through it separately, so a
//! few hundred bytes can write out gigabytes:
//! - [`to_binary`] (the import, a file's `.aupreset`) takes XML only, which
//!   cannot share objects, so its conversion is linear in its size;
//! - [`to_xml`] (the export, a state from a project) takes XML, or a binary
//!   list whose [`written_size`] is bounded first.

use objc2_core_foundation::{
    CFData, CFDictionary, CFPropertyListCreateData, CFPropertyListCreateWithData,
    CFPropertyListFormat,
};
use std::ptr;

/// The largest property list either conversion takes, and the most a
/// binary one may write out to (the import's total for the plug-in states
/// it holds, `core_io::limits::MAX_STATE_TOTAL_BYTES`).
pub const MAX_PLIST_BYTES: usize = 128 * 1024 * 1024;

/// The most objects a binary list may hold (a plug-in state holds tens to
/// thousands), which bounds [`written_size`]'s own memory.
pub const MAX_PLIST_OBJECTS: usize = 1 << 20;

const NOT_XML: &str = "the settings are not an XML property list";
const TOO_LARGE: &str = "the settings are too large";
const WRITES_TOO_LARGE: &str = "the settings would be too large written out";
const MALFORMED: &str = "the settings are not a property list";

/// Whether `plist` is XML: after an optional UTF-8 byte-order mark and
/// white space, it opens with `<?xml`, `<!DOCTYPE` or `<plist`.
fn is_xml(plist: &[u8]) -> bool {
    let rest = plist.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(plist);
    let start = rest
        .iter()
        .position(|b| !b.is_ascii_whitespace())
        .unwrap_or(rest.len());
    let rest = rest.get(start..).unwrap_or_default();
    [&b"<?xml"[..], b"<!DOCTYPE", b"<plist"]
        .iter()
        .any(|p| rest.starts_with(p))
}

/// The size checks both conversions share.
fn sized(plist: &[u8]) -> Result<(), String> {
    if plist.is_empty() {
        return Err("the settings are empty".to_owned());
    }
    if plist.len() > MAX_PLIST_BYTES {
        return Err(TOO_LARGE.to_owned());
    }
    Ok(())
}

/// `plist` (already checked) written in `format`. It must parse as
/// `read_as`. An AU state is a dictionary (`ClassInfo`): anything else is
/// refused, so text that the old plain-text format would read as a string
/// is not taken.
fn convert(
    plist: &[u8],
    format: CFPropertyListFormat,
    read_as: CFPropertyListFormat,
    not_read_as: &str,
) -> Result<Vec<u8>, String> {
    let data = CFData::from_bytes(plist);
    let mut got = CFPropertyListFormat(0);
    // SAFETY: `data` is a valid CFData; options 0 (immutable); `got` is a
    // valid format out-pointer; a null error pointer is allowed.
    let parsed = unsafe {
        CFPropertyListCreateWithData(None, Some(&data), 0, &raw mut got, ptr::null_mut())
    }
    .ok_or(MALFORMED)?;
    if got != read_as {
        return Err(not_read_as.to_owned());
    }
    if parsed.downcast_ref::<CFDictionary>().is_none() {
        return Err("the settings are not a property-list dictionary".to_owned());
    }
    // SAFETY: `parsed` is a valid property list; null error pointer.
    let out = unsafe { CFPropertyListCreateData(None, Some(&parsed), format, 0, ptr::null_mut()) }
        .ok_or("the settings could not be written as a property list")?;
    Ok(out.to_vec())
}

/// A property list as an XML property list (`XMLFormat_v1_0`): XML, or a
/// binary list (`bplist00`) whose [`written_size`] is within
/// [`MAX_PLIST_BYTES`]. Anything else is refused.
pub fn to_xml(plist: &[u8]) -> Result<Vec<u8>, String> {
    sized(plist)?;
    let xml = CFPropertyListFormat::XMLFormat_v1_0;
    if plist.starts_with(b"bplist00") {
        let cap = MAX_PLIST_BYTES as u64;
        if written_size(plist, cap)? > cap {
            return Err(WRITES_TOO_LARGE.to_owned());
        }
        return convert(
            plist,
            xml,
            CFPropertyListFormat::BinaryFormat_v1_0,
            MALFORMED,
        );
    }
    if !is_xml(plist) {
        return Err(MALFORMED.to_owned());
    }
    convert(plist, xml, xml, MALFORMED)
}

/// An XML property list as a binary property list (`BinaryFormat_v1_0`).
/// A binary list (`bplist`, any version) or any other text is refused
/// before CoreFoundation reads it.
pub fn to_binary(plist: &[u8]) -> Result<Vec<u8>, String> {
    sized(plist)?;
    if plist.starts_with(b"bplist") || !is_xml(plist) {
        return Err(NOT_XML.to_owned());
    }
    let xml = CFPropertyListFormat::XMLFormat_v1_0;
    convert(plist, CFPropertyListFormat::BinaryFormat_v1_0, xml, NOT_XML)
}

// ---- a binary list's written size ----

/// Each object's share of the written size beyond its payload (an XML
/// element's tags and indent).
const PER_OBJECT: u64 = 32;
/// A size slot's mark for an object on the walk's path.
const ON_PATH: u64 = u64::MAX;

/// `n` (≤ 8) big-endian bytes at `at`.
fn be(bytes: &[u8], at: usize, n: usize) -> Option<u64> {
    if n > 8 {
        return None;
    }
    let s = bytes.get(at..at.checked_add(n)?)?;
    Some(s.iter().fold(0u64, |a, b| (a << 8) | u64::from(*b)))
}

/// A binary list's trailer fields.
struct Bplist<'a> {
    bytes: &'a [u8],
    offset_size: usize,
    ref_size: usize,
    count: usize,
    table: usize,
}

/// An object: its own written bytes, and where its `refs` references
/// start.
struct Object {
    own: u64,
    start: usize,
    refs: usize,
}

impl<'a> Bplist<'a> {
    /// The trailer, checked, and the top object.
    fn read(bytes: &'a [u8]) -> Option<(Self, usize)> {
        let t = bytes.len().checked_sub(32)?;
        let offset_size = usize::from(*bytes.get(t.checked_add(6)?)?);
        let ref_size = usize::from(*bytes.get(t.checked_add(7)?)?);
        let count = usize::try_from(be(bytes, t.checked_add(8)?, 8)?).ok()?;
        let top = usize::try_from(be(bytes, t.checked_add(16)?, 8)?).ok()?;
        let table = usize::try_from(be(bytes, t.checked_add(24)?, 8)?).ok()?;
        if !(1..=8).contains(&offset_size) || !(1..=8).contains(&ref_size) {
            return None;
        }
        // Every object takes a byte or more between the header and the
        // offset table, which ends before the trailer.
        if count == 0 || top >= count || table < 8 || count > table - 8 {
            return None;
        }
        if count.checked_mul(offset_size)?.checked_add(table)? > t {
            return None;
        }
        let list = Self {
            bytes,
            offset_size,
            ref_size,
            count,
            table,
        };
        Some((list, top))
    }

    /// The count in a marker's low nibble, or in the integer after it for
    /// 0xF, and where the contents start.
    fn count_at(&self, at: usize, nibble: u8) -> Option<(u64, usize)> {
        let next = at.checked_add(1)?;
        if nibble != 0x0F {
            return Some((u64::from(nibble), next));
        }
        let m = *self.bytes.get(next)?;
        if m >> 4 != 0x1 || m & 0x0F > 3 {
            return None;
        }
        let n = 1usize << (m & 0x0F);
        let start = next.checked_add(1)?;
        Some((be(self.bytes, start, n)?, start.checked_add(n)?))
    }

    /// Object `i`, checked to lie before the offset table.
    fn object(&self, i: usize) -> Option<Object> {
        let at = i.checked_mul(self.offset_size)?.checked_add(self.table)?;
        let at = usize::try_from(be(self.bytes, at, self.offset_size)?).ok()?;
        if !(8..self.table).contains(&at) {
            return None;
        }
        let marker = *self.bytes.get(at)?;
        let (kind, nibble) = (marker >> 4, marker & 0x0F);
        let room = u64::try_from(self.table - at).ok()?;
        let leaf = |own| Object {
            own,
            start: 0,
            refs: 0,
        };
        match kind {
            // Data and strings (UTF-16 in two-byte units), written as up to
            // twice their bytes (base64, escapes).
            0x4..=0x7 => {
                let (n, _) = self.count_at(at, nibble)?;
                let n = if kind == 0x6 { n.checked_mul(2)? } else { n };
                (n <= room).then(|| leaf(n.saturating_mul(2)))
            }
            // Arrays and sets: n references; dictionaries: n keys and n
            // values.
            0xA..=0xD => {
                let (n, start) = self.count_at(at, nibble)?;
                let n = if kind == 0xD { n.checked_mul(2)? } else { n };
                if n.checked_mul(self.ref_size as u64)? > room {
                    return None;
                }
                Some(Object {
                    own: 0,
                    start,
                    refs: usize::try_from(n).ok()?,
                })
            }
            // Numbers, dates, booleans and UIDs: a few bytes each.
            _ => Some(leaf(16)),
        }
    }

    /// Reference `k` of `o`, checked against the object count.
    fn reference(&self, o: &Object, k: usize) -> Option<usize> {
        let at = k.checked_mul(self.ref_size)?.checked_add(o.start)?;
        let r = usize::try_from(be(self.bytes, at, self.ref_size)?).ok()?;
        (r < self.count).then_some(r)
    }
}

/// An estimate of the bytes a binary property list (`bplist00`) writes out
/// to, counting every path through shared objects: each object
/// [`PER_OBJECT`] bytes plus twice its payload. It reads the list itself,
/// in time and memory linear in its size, and stops once the count passes
/// `cap`. Malformed lists, lists of more than [`MAX_PLIST_OBJECTS`],
/// references past the object count and cycles are refused.
pub fn written_size(plist: &[u8], cap: u64) -> Result<u64, String> {
    let (list, top) = Bplist::read(plist).ok_or(MALFORMED)?;
    if list.count > MAX_PLIST_OBJECTS {
        return Err(TOO_LARGE.to_owned());
    }
    // Each object's size once known; 0 not yet seen; ON_PATH on the path.
    let mut size = vec![0u64; list.count];
    // The path: an object and the next of its references to visit.
    let mut path: Vec<(usize, Object, usize)> = Vec::new();
    let first = list.object(top).ok_or(MALFORMED)?;
    *size.get_mut(top).ok_or(MALFORMED)? = ON_PATH;
    path.push((top, first, 0));
    while let Some((i, o, k)) = path.last_mut() {
        if *k < o.refs {
            let r = list.reference(o, *k).ok_or(MALFORMED)?;
            *k += 1;
            match size.get(r).copied() {
                Some(0) => {
                    let child = list.object(r).ok_or(MALFORMED)?;
                    *size.get_mut(r).ok_or(MALFORMED)? = ON_PATH;
                    path.push((r, child, 0));
                }
                Some(ON_PATH) | None => return Err(MALFORMED.to_owned()),
                Some(_) => {}
            }
            continue;
        }
        let mut total = o.own.saturating_add(PER_OBJECT);
        for j in 0..o.refs {
            let r = list.reference(o, j).ok_or(MALFORMED)?;
            total = total.saturating_add(size.get(r).copied().ok_or(MALFORMED)?);
        }
        if total > cap {
            return Ok(total);
        }
        let i = *i;
        *size.get_mut(i).ok_or(MALFORMED)? = total;
        path.pop();
    }
    size.get(top).copied().ok_or_else(|| MALFORMED.to_owned())
}
