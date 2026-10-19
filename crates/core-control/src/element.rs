//! Element IDs (spec §5.4): `"<index path>:<check>"`, e.g. `"0.3.1:9f2c01ab"`.
//! The path walks the window's accessibility children; the check is the
//! FNV-1a hash of the element's role and label, so an ID that now points
//! at a different element is refused (`no_element`) instead of clicking
//! the wrong thing.

/// The deepest path an ID may hold.
pub const MAX_PATH_LEN: usize = 64;
/// The largest child index an ID may hold.
pub const MAX_INDEX: u32 = 100_000;

/// An element a request addresses.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ElementRef {
    /// The ID as given.
    pub id: String,
    /// Child indices from the window down.
    pub path: Vec<u32>,
    /// [`check`] of the element's role and label.
    pub check: u32,
}

/// 32-bit FNV-1a of `role`, U+001F and `label`.
pub fn check(role: &str, label: &str) -> u32 {
    let mut h: u32 = 0x811c_9dc5;
    for b in role.bytes().chain([0x1f]).chain(label.bytes()) {
        h ^= u32::from(b);
        h = h.wrapping_mul(0x0100_0193);
    }
    h
}

/// The ID of the element at `path` with `role` and `label`.
pub fn element_id(path: &[u32], role: &str, label: &str) -> String {
    let p: Vec<String> = path.iter().map(u32::to_string).collect();
    format!("{}:{:08x}", p.join("."), check(role, label))
}

/// Parses an ID; Err says why it is not one.
pub fn parse(id: &str) -> Result<ElementRef, String> {
    let bad = || {
        format!(
            "{:?} is not an element ID (\"0.3.1:9f2c01ab\" from `ui.tree`)",
            crate::protocol::truncate(id, 80)
        )
    };
    let (path, hash) = id.split_once(':').ok_or_else(bad)?;
    if hash.len() != 8 || !hash.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(bad());
    }
    let check = u32::from_str_radix(hash, 16).map_err(|_| bad())?;
    let mut indices = Vec::new();
    for part in path.split('.') {
        if part.is_empty() || part.len() > 6 || !part.bytes().all(|b| b.is_ascii_digit()) {
            return Err(bad());
        }
        let i: u32 = part.parse().map_err(|_| bad())?;
        if i > MAX_INDEX {
            return Err(bad());
        }
        indices.push(i);
    }
    if indices.len() > MAX_PATH_LEN {
        return Err(bad());
    }
    Ok(ElementRef {
        id: id.to_string(),
        path: indices,
        check,
    })
}
