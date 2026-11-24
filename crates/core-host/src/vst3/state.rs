//! VST3 state (host track H1 Task 5). A plug-in's state is two streams:
//! the component's (`IComponent::getState`) and the controller's own
//! (`IEditController::getState`). The host keeps both in one blob in the
//! SDK's `.vstpreset` layout (`public.sdk/source/vst/vstpresetfile.h`), so
//! a DAWproject package carries it as is (Plan Ruling 2):
//!
//! | bytes | what |
//! |---|---|
//! | 4 | `VST3` |
//! | 4 | format version (1), little-endian |
//! | 32 | the class ID, ASCII hex (as the SDK's `FUID::toString`) |
//! | 8 | offset of the chunk list, little-endian |
//! | … | the chunks' data: the component's state, then the controller's |
//! | 4 | `List` |
//! | 4 | number of entries |
//! | 20 each | entry: a chunk ID (`Comp`, `Cont`, …), its offset and size (8 bytes each) |
//!
//! Loading follows the SDK's `PresetFile::loadPreset`: the component's
//! `setState`, then the controller's `setComponentState` with the same
//! stream, then the controller's `setState` with its own. A blob is
//! checked whole before the plug-in sees any of it: a foreign class ID, a
//! truncated blob or a chunk outside it is refused and the plug-in is not
//! called. When the plug-in refuses a stream part way, the state it had
//! is loaded back, so a refused load leaves nothing half applied (as far
//! as the plug-in can take its own state back).
//!
//! Save and load run on the main thread, active or not, as the CLAP
//! host's do; the SDK has a plug-in take state calls on the main thread
//! while it processes.

use super::ffi::Plugin;
use super::{class_id_hex, parse_class_id};
use crate::error::HostError;

const MAGIC: &[u8; 4] = b"VST3";
const VERSION: i32 = 1;
const LIST: &[u8; 4] = b"List";
const COMPONENT: &[u8; 4] = b"Comp";
const CONTROLLER: &[u8; 4] = b"Cont";
/// The header: magic, version, class ID, list offset.
const HEADER: usize = 4 + 4 + 32 + 8;
/// One chunk-list entry: ID, offset, size.
const ENTRY: usize = 4 + 8 + 8;

/// A blob's parts, borrowed from it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Preset<'a> {
    pub class_id: [u8; 16],
    pub component: &'a [u8],
    /// The controller's own state, when the blob has one.
    pub controller: Option<&'a [u8]>,
}

/// A blob in the `.vstpreset` layout.
pub(crate) fn write(class_id: &[u8; 16], component: &[u8], controller: Option<&[u8]>) -> Vec<u8> {
    let mut chunks: Vec<(&[u8; 4], &[u8])> = vec![(COMPONENT, component)];
    chunks.extend(controller.map(|c| (CONTROLLER, c)));
    let data: usize = chunks.iter().map(|(_, b)| b.len()).sum();
    let mut out = Vec::with_capacity(HEADER + data + 8 + ENTRY * chunks.len());
    out.extend_from_slice(MAGIC);
    out.extend_from_slice(&VERSION.to_le_bytes());
    out.extend_from_slice(class_id_hex(class_id).as_bytes());
    out.extend_from_slice(&((HEADER + data) as i64).to_le_bytes());
    let mut entries = Vec::with_capacity(chunks.len());
    for (id, bytes) in &chunks {
        entries.push((*id, out.len() as i64, bytes.len() as i64));
        out.extend_from_slice(bytes);
    }
    out.extend_from_slice(LIST);
    out.extend_from_slice(&(entries.len() as i32).to_le_bytes());
    for (id, offset, size) in entries {
        out.extend_from_slice(id);
        out.extend_from_slice(&offset.to_le_bytes());
        out.extend_from_slice(&size.to_le_bytes());
    }
    out
}

fn damaged(why: &str) -> String {
    format!("damaged VST3 state: {why}")
}

/// `n` bytes of `bytes` at `at`, if all there.
fn take(bytes: &[u8], at: usize, n: usize) -> Option<&[u8]> {
    bytes.get(at..at.checked_add(n)?)
}

fn le_i32(b: &[u8]) -> Option<i32> {
    Some(i32::from_le_bytes(b.try_into().ok()?))
}

fn le_i64(b: &[u8]) -> Option<i64> {
    Some(i64::from_le_bytes(b.try_into().ok()?))
}

/// Reads a blob in the `.vstpreset` layout (ours or another host's: extra
/// chunks such as `Info` are skipped; the first chunk of an ID counts, as
/// in the SDK). Every offset and size is checked against the blob.
pub(crate) fn read(bytes: &[u8]) -> Result<Preset<'_>, String> {
    if take(bytes, 0, 4) != Some(MAGIC.as_slice()) {
        return Err("not a VST3 state".into());
    }
    if bytes.len() < HEADER {
        return Err(damaged("the header is cut short"));
    }
    let class_id = take(bytes, 8, 32)
        .and_then(|t| std::str::from_utf8(t).ok())
        .and_then(parse_class_id)
        .ok_or_else(|| damaged("no class ID"))?;
    let list = take(bytes, 40, 8)
        .and_then(le_i64)
        .and_then(|o| usize::try_from(o).ok())
        .filter(|o| *o >= HEADER)
        .ok_or_else(|| damaged("no chunk list"))?;
    if take(bytes, list, 4) != Some(LIST.as_slice()) {
        return Err(damaged("no chunk list"));
    }
    let count = take(bytes, list + 4, 4)
        .and_then(le_i32)
        .and_then(|n| usize::try_from(n).ok())
        .ok_or_else(|| damaged("a bad chunk count"))?;
    let first = list + 8;
    let mut component = None;
    let mut controller = None;
    for k in 0..count {
        // An entry that is not all there ends the read (so a bogus count
        // stops at the end of the blob).
        let entry = k
            .checked_mul(ENTRY)
            .and_then(|o| first.checked_add(o))
            .and_then(|at| take(bytes, at, ENTRY))
            .ok_or_else(|| damaged("the chunk list is cut short"))?;
        let id = entry.get(..4);
        let offset = entry.get(4..12).and_then(le_i64);
        let size = entry.get(12..20).and_then(le_i64);
        let chunk = offset
            .zip(size)
            .and_then(|(o, n)| Some((usize::try_from(o).ok()?, usize::try_from(n).ok()?)))
            .filter(|(o, _)| *o >= HEADER)
            .and_then(|(o, n)| take(bytes, o, n))
            .ok_or_else(|| damaged("a chunk outside the state"))?;
        if id == Some(COMPONENT.as_slice()) {
            component.get_or_insert(chunk);
        } else if id == Some(CONTROLLER.as_slice()) {
            controller.get_or_insert(chunk);
        }
    }
    let component = component.ok_or_else(|| damaged("no component state"))?;
    Ok(Preset {
        class_id,
        component,
        controller,
    })
}

/// Both of `plugin`'s states as a blob for class `class_id`.
pub(crate) fn save(plugin: &Plugin, class_id: &[u8; 16]) -> Result<Vec<u8>, HostError> {
    let component = plugin.component_state().map_err(HostError::StateRejected)?;
    let controller = plugin
        .controller_state()
        .map_err(HostError::StateRejected)?;
    Ok(write(class_id, &component, controller.as_deref()))
}

/// Gives `plugin` a preset's streams in the SDK's order; the first one it
/// refuses ends the load.
fn apply(plugin: &Plugin, p: &Preset<'_>) -> Result<(), &'static str> {
    if !plugin.set_component_state(p.component) {
        return Err("the plug-in refused its state");
    }
    if !plugin.sync_controller(p.component) {
        return Err("the plug-in's controller refused the component's state");
    }
    // An empty controller chunk is a controller with nothing to keep
    // (its `getState` wrote nothing).
    if let Some(c) = p.controller.filter(|c| !c.is_empty()) {
        if !plugin.set_controller_state(c) {
            return Err("the plug-in's controller refused its state");
        }
    }
    Ok(())
}

/// Loads a blob from [`save`] (or a `.vstpreset` for class `class_id`)
/// into `plugin`. A damaged or foreign blob is refused before the plug-in
/// is called; a stream the plug-in refuses puts back the state it had.
pub(crate) fn load(plugin: &Plugin, class_id: &[u8; 16], bytes: &[u8]) -> Result<(), HostError> {
    let p = read(bytes).map_err(HostError::StateRejected)?;
    if p.class_id != *class_id {
        return Err(HostError::StateRejected(format!(
            "the VST3 state is for another plug-in ({})",
            class_id_hex(&p.class_id)
        )));
    }
    // What to put back if the plug-in refuses part way (none when it
    // cannot say what it has: then there is nothing to put back).
    let before = save(plugin, class_id).ok();
    apply(plugin, &p).map_err(|why| {
        if let Some(old) = before.as_deref().and_then(|b| read(b).ok()) {
            let _ = apply(plugin, &old);
        }
        HostError::StateRejected(why.into())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const CID: [u8; 16] = [
        0x56, 0x49, 0x5A, 0x44, 0x56, 0x53, 0x54, 0x33, 0x7E, 0x57, 0, 0, 0xAB, 0xCD, 0, 1,
    ];

    fn put_i64(b: &mut [u8], at: usize, v: i64) {
        b[at..at + 8].copy_from_slice(&v.to_le_bytes());
    }

    #[test]
    fn a_blob_has_the_sdk_preset_layout() {
        let b = write(&CID, b"comp", Some(b"ctl!!"));
        assert_eq!(&b[..4], b"VST3");
        assert_eq!(le_i32(&b[4..8]), Some(1));
        assert_eq!(&b[8..40], b"56495A44565354337E570000ABCD0001");
        assert_eq!(le_i64(&b[40..48]), Some(48 + 9));
        assert_eq!(&b[48..52], b"comp");
        assert_eq!(&b[52..57], b"ctl!!");
        assert_eq!(&b[57..61], b"List");
        assert_eq!(le_i32(&b[61..65]), Some(2));
        assert_eq!(&b[65..69], b"Comp");
        assert_eq!(
            (le_i64(&b[69..77]), le_i64(&b[77..85])),
            (Some(48), Some(4))
        );
        assert_eq!(&b[85..89], b"Cont");
        assert_eq!(
            (le_i64(&b[89..97]), le_i64(&b[97..105])),
            (Some(52), Some(5))
        );
        assert_eq!(b.len(), 105);
        let p = read(&b).unwrap();
        assert_eq!(p.class_id, CID);
        assert_eq!(p.component, b"comp");
        assert_eq!(p.controller, Some(b"ctl!!".as_slice()));
    }

    #[test]
    fn empty_and_missing_controller_states_read_back() {
        let p = write(&CID, b"", None);
        let p = read(&p).unwrap();
        assert_eq!((p.component, p.controller), (b"".as_slice(), None));
        let p = write(&CID, b"x", Some(b""));
        assert_eq!(read(&p).unwrap().controller, Some(b"".as_slice()));
    }

    /// Another host's preset: lower-case class ID, an `Info` chunk, the
    /// controller's chunk listed first and the list before the data.
    #[test]
    fn another_hosts_preset_reads() {
        let mut b = Vec::new();
        b.extend_from_slice(b"VST3");
        b.extend_from_slice(&1i32.to_le_bytes());
        b.extend_from_slice(b"56495a44565354337e570000abcd0001");
        b.extend_from_slice(&48i64.to_le_bytes());
        b.extend_from_slice(b"List");
        b.extend_from_slice(&3i32.to_le_bytes());
        let data = 48 + 8 + 3 * 20;
        for (id, off, n) in [
            (b"Cont", data + 2, 3),
            (b"Info", data + 5, 4),
            (b"Comp", data, 2),
        ] {
            b.extend_from_slice(id);
            b.extend_from_slice(&(off as i64).to_le_bytes());
            b.extend_from_slice(&(n as i64).to_le_bytes());
        }
        b.extend_from_slice(b"CCtttMETA");
        let p = read(&b).unwrap();
        assert_eq!(p.class_id, CID);
        assert_eq!(
            (p.component, p.controller),
            (b"CC".as_slice(), Some(b"ttt".as_slice()))
        );
    }

    #[test]
    fn damaged_and_foreign_blobs_are_refused() {
        let good = write(&CID, b"comp", Some(b"ctl"));
        let list = 48 + 7;
        let mut cases: Vec<(&str, Vec<u8>)> = vec![
            ("empty", Vec::new()),
            ("not a preset", b"plain bytes from somewhere".to_vec()),
            ("header only", good[..47].to_vec()),
            ("no list", good[..list].to_vec()),
            ("half an entry", good[..good.len() - 1].to_vec()),
        ];
        let mut bad_id = good.clone();
        bad_id[8] = b'Z';
        cases.push(("bad class ID", bad_id));
        let mut list_far = good.clone();
        put_i64(&mut list_far, 40, i64::MAX);
        cases.push(("list offset past the end", list_far));
        let mut list_neg = good.clone();
        put_i64(&mut list_neg, 40, -1);
        cases.push(("negative list offset", list_neg));
        let mut list_in_header = good.clone();
        put_i64(&mut list_in_header, 40, 4);
        cases.push(("list in the header", list_in_header));
        let mut count = good.clone();
        count[list + 4..list + 8].copy_from_slice(&i32::MAX.to_le_bytes());
        cases.push(("huge count", count));
        let mut neg_count = good.clone();
        neg_count[list + 4..list + 8].copy_from_slice(&(-1i32).to_le_bytes());
        cases.push(("negative count", neg_count));
        let entry = list + 8;
        let mut size = good.clone();
        put_i64(&mut size, entry + 12, 1 << 40);
        cases.push(("chunk past the end", size));
        let mut wrap = good.clone();
        put_i64(&mut wrap, entry + 4, i64::MAX);
        put_i64(&mut wrap, entry + 12, i64::MAX);
        cases.push(("chunk offset overflows", wrap));
        let mut neg = good.clone();
        put_i64(&mut neg, entry + 12, -4);
        cases.push(("negative size", neg));
        let mut in_header = good.clone();
        put_i64(&mut in_header, entry + 4, 0);
        cases.push(("chunk in the header", in_header));
        let mut no_comp = good.clone();
        no_comp[entry..entry + 4].copy_from_slice(b"Prog");
        cases.push(("no component chunk", no_comp));
        for (what, b) in cases {
            assert!(read(&b).is_err(), "{what}");
        }
        // Every truncation of a good blob is refused.
        for n in 0..good.len() {
            assert!(read(&good[..n]).is_err(), "cut at {n}");
        }
    }
}
