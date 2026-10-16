//! Plugin state blobs (spec §4, Plan Ruling 8) and capture (Plan
//! Ruling 9).

use crate::document::Document;
use crate::error::Result;
use crate::ids::{PluginInstanceId, StateBlobId};
use serde_json::Value;
use std::collections::BTreeSet;
use std::sync::Arc;

/// The package folder holding blobs.
pub const STATE_DIR: &str = "PluginState";

/// The ID of a blob: the 64-bit FNV-1a hash of its bytes.
pub fn blob_id(bytes: &[u8]) -> StateBlobId {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in bytes {
        h ^= u64::from(*b);
        h = h.wrapping_mul(0x0100_0000_01b3);
    }
    StateBlobId(h)
}

/// `<16 lowercase hex digits>.bin`.
pub fn blob_file_name(id: StateBlobId) -> String {
    format!("{:016x}.bin", id.0)
}

/// The blob ID a file name stands for, if it is one.
pub fn parse_blob_file_name(name: &str) -> Option<StateBlobId> {
    let hex = name.strip_suffix(".bin")?;
    if hex.len() != 16 {
        return None;
    }
    u64::from_str_radix(hex, 16).ok().map(StateBlobId)
}

/// Stores `bytes` as plugin instance `slot`'s state and clears its
/// overlay; blobs no slot points at any more are dropped from the copy.
/// The sound is unchanged, so this is not an undo step.
pub fn capture(
    doc: &Document,
    slot: PluginInstanceId,
    bytes: &[u8],
) -> Result<(Document, StateBlobId)> {
    let mut d = doc.clone();
    let id = blob_id(bytes);
    let s = d.slot_mut(slot)?;
    s.state = Some(id);
    s.params.clear();
    d.states_mut().insert(id, Arc::from(bytes));
    let keep = d.referenced_states();
    let stale: Vec<StateBlobId> = d
        .states_mut()
        .keys()
        .filter(|k| !keep.contains(k))
        .copied()
        .collect();
    for k in stale {
        d.states_mut().remove(&k);
    }
    Ok((d, id))
}

/// Blob IDs a `project.json` text references (every `state` field of an
/// object that also has a `plugin` field, i.e. every plugin slot).
pub fn states_in_json(text: &str) -> BTreeSet<StateBlobId> {
    fn walk(v: &Value, out: &mut BTreeSet<StateBlobId>) {
        match v {
            Value::Object(m) => {
                if m.contains_key("plugin") {
                    if let Some(id) = m.get("state").and_then(Value::as_u64) {
                        out.insert(StateBlobId(id));
                    }
                }
                for c in m.values() {
                    walk(c, out);
                }
            }
            Value::Array(a) => {
                for c in a {
                    walk(c, out);
                }
            }
            _ => {}
        }
    }
    let mut out = BTreeSet::new();
    if let Ok(v) = serde_json::from_str::<Value>(text) {
        walk(&v, &mut out);
    }
    out
}
