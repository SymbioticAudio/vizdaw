//! The AAF object store over a compound file (plan 6f Task 1, Ruling 13).
//!
//! **Layout** (AAF Low-Level Container Specification v1.0.1):
//! - each object is a storage whose CLSID is its class AUID;
//! - its properties are the stream `properties`: `byteOrder` (u8, `0x4C`),
//!   `formatVersion` (u8, `0x20` written), `entryCount` (u16), then
//!   `entryCount` entries of `pid`, `storedForm` and `length` (u16 each),
//!   then the values in entry order;
//! - a strong reference's value is the UTF-16LE, NUL-terminated name of a
//!   child storage. A vector's or set's value names the base: its index is
//!   the stream `<base> index` and element `i` is the storage
//!   `<base>{<localKey in lowercase hex>}`;
//! - a weak reference holds `referencedPropertyIndex` (u16),
//!   `identificationPid` (u16), `identificationSize` (u8) and the
//!   identification; a weak vector's or set's value names its index stream
//!   `<name> index`: `entryCount` (u32), the same three header fields once,
//!   then the identifications;
//! - the root's `referenced properties` stream is `byteOrder` (u8),
//!   `pathCount` (u16), `pidCount` (u32), then `pidCount` PIDs, each path
//!   ended by a 0;
//! - a data stream's value is a byte-order byte (`0x55` written) and the
//!   UTF-16LE, NUL-terminated name of a stream in the object's storage.
//!
//! **Reading** walks from the root depth first with an explicit stack.
//! Every name resolves below the storage that holds it (no `..`, no
//! separators), each storage is read once, and every count is checked
//! against its stream's length before anything is allocated. A damaged
//! object or property is left out with [`UNREADABLE_PARTS`]; big-endian
//! data, too many objects, too deep a nesting or too large a store refuse
//! the file.
//!
//! **Writing** names every child after its property: the strong reference
//! `pid` is the storage `<pid as 4 lowercase hex digits>`, a vector's or
//! set's element `i` is `<pid>{<i in hex>}`, and a weak vector's or set's
//! index is `<pid> index`. [`Store::add_child`] gives an object the path
//! the writer will give it, so a store built that way reads back equal.

use crate::IoError;
use crate::limits::AafLimits;
use crate::report::{ItemKind, Report};
use std::collections::{BTreeMap, HashSet};
use std::fmt;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom, Write};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::Path;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};

/// Refusal: the compound file or its objects are broken.
pub const DAMAGED: &str = "This AAF file is damaged.";
/// Refusal: not a compound file, or a compound file that is not AAF.
pub const NOT_AAF: &str = "This isn't an AAF file.";
/// Refusal: a `properties` stream in big-endian order.
pub const BIG_ENDIAN: &str = "This AAF file is big-endian; VizDAW reads little-endian files.";
/// Refusal: more objects than [`AafLimits::max_objects`].
pub const TOO_MANY_OBJECTS: &str = "This AAF file holds too many objects to open.";
/// Refusal: nesting deeper than [`AafLimits::max_depth`].
pub const TOO_DEEP: &str = "This AAF file nests its objects too deeply to open.";
/// Refusal: the store would pass [`AafLimits::max_model_bytes`].
pub const TOO_LARGE: &str = "This AAF file is too large to open.";
/// Refusal: a data stream longer than its caller allows.
pub const STREAM_TOO_LARGE: &str = "Audio in this AAF file is larger than the file says.";
/// Left out: an object or property that could not be read (its path as
/// the detail). The same text as DAWproject's, so the report reads alike.
pub const UNREADABLE_PARTS: &str = crate::dawproject::schema::UNREADABLE_PARTS;

/// Stored forms (the Low-Level Container Specification's table).
pub mod form {
    /// Data.
    pub const DATA: u16 = 0x82;
    /// A data stream.
    pub const DATA_STREAM: u16 = 0x42;
    /// A strong reference.
    pub const STRONG: u16 = 0x22;
    /// A strong reference vector.
    pub const STRONG_VECTOR: u16 = 0x32;
    /// A strong reference set.
    pub const STRONG_SET: u16 = 0x3A;
    /// A weak reference.
    pub const WEAK: u16 = 0x02;
    /// A weak reference vector.
    pub const WEAK_VECTOR: u16 = 0x12;
    /// A weak reference set.
    pub const WEAK_SET: u16 = 0x1A;
    /// A weak reference, stored object ID.
    pub const WEAK_STORED_ID: u16 = 0x03;
    /// A unique object ID.
    pub const UNIQUE_ID: u16 = 0x86;
    /// An opaque stream.
    pub const OPAQUE_STREAM: u16 = 0x40;
}

/// An AUID as stored: the 16-byte GUID layout (`Data1` u32, `Data2` and
/// `Data3` u16, little-endian, then `Data4`'s 8 bytes). It prints as
/// `8-4-4-4-12` hex, as the specification's tables do.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct Auid(pub [u8; 16]);

impl Auid {
    /// The AUID whose printed form is `d1-d2-d3-d4[0..2]-d4[2..8]`.
    pub const fn from_fields(d1: u32, d2: u16, d3: u16, d4: [u8; 8]) -> Auid {
        let [a0, a1, a2, a3] = d1.to_le_bytes();
        let [b0, b1] = d2.to_le_bytes();
        let [c0, c1] = d3.to_le_bytes();
        let [e0, e1, e2, e3, e4, e5, e6, e7] = d4;
        Auid([
            a0, a1, a2, a3, b0, b1, c0, c1, e0, e1, e2, e3, e4, e5, e6, e7,
        ])
    }

    /// The four fields [`from_fields`](Self::from_fields) takes.
    pub fn fields(&self) -> (u32, u16, u16, [u8; 8]) {
        let [
            a0,
            a1,
            a2,
            a3,
            b0,
            b1,
            c0,
            c1,
            e0,
            e1,
            e2,
            e3,
            e4,
            e5,
            e6,
            e7,
        ] = self.0;
        (
            u32::from_le_bytes([a0, a1, a2, a3]),
            u16::from_le_bytes([b0, b1]),
            u16::from_le_bytes([c0, c1]),
            [e0, e1, e2, e3, e4, e5, e6, e7],
        )
    }

    fn uuid(&self) -> uuid::Uuid {
        let (a, b, c, d) = self.fields();
        uuid::Uuid::from_fields(a, b, c, &d)
    }

    fn of_uuid(u: &uuid::Uuid) -> Auid {
        let (a, b, c, d) = u.as_fields();
        Auid::from_fields(a, b, c, *d)
    }
}

impl fmt::Display for Auid {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let (a, b, c, d) = self.fields();
        let [d0, d1, d2, d3, d4, d5, d6, d7] = d;
        write!(
            f,
            "{a:08x}-{b:04x}-{c:04x}-{d0:02x}{d1:02x}-{d2:02x}{d3:02x}{d4:02x}{d5:02x}{d6:02x}{d7:02x}"
        )
    }
}

impl fmt::Debug for Auid {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(self, f)
    }
}

/// A MobID (a SMPTE UMID) as stored, 32 bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct MobId(pub [u8; 32]);

/// An object: its index in [`Store::objects`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ObjId(pub u32);

/// A weak reference: an entry of the referenced-properties table and the
/// identification of the object it names there.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WeakRef {
    /// Index into [`Store::referenced`].
    pub table_index: u16,
    /// The PID of the identifying property of the object it names.
    pub id_pid: u16,
    /// That property's value, usually a 16-byte AUID.
    pub id: Vec<u8>,
}

/// One property's value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Prop {
    /// Data, as stored; also the unique-ID, opaque-stream and
    /// stored-object-ID forms, kept as bytes (written back as data).
    Data(Vec<u8>),
    /// A strong reference.
    Strong(ObjId),
    /// A strong reference vector, in order.
    StrongVec(Vec<ObjId>),
    /// A strong reference set, in index order.
    StrongSet(Vec<ObjId>),
    /// A weak reference.
    Weak(WeakRef),
    /// A weak reference vector.
    WeakVec(Vec<WeakRef>),
    /// A weak reference set.
    WeakSet(Vec<WeakRef>),
    /// A data stream: its stream name in the object's storage.
    Stream(String),
}

/// One object.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Obj {
    /// Its class (the storage's CLSID).
    pub class: Auid,
    /// Its properties by PID.
    pub props: BTreeMap<u16, Prop>,
    /// Its storage path, `/` for the root.
    pub path: String,
}

/// Every object read from (or to be written to) a file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Store {
    /// The objects; [`ObjId`] indexes them.
    pub objects: Vec<Obj>,
    /// The root object.
    pub root: ObjId,
    /// The referenced-properties table: each entry a PID path from the
    /// root, which weak references index.
    pub referenced: Vec<Vec<u16>>,
}

impl Store {
    /// A store holding only a root object of `root_class` at `/`.
    pub fn new(root_class: Auid) -> Store {
        Store {
            objects: vec![Obj {
                class: root_class,
                props: BTreeMap::new(),
                path: "/".to_string(),
            }],
            root: ObjId(0),
            referenced: Vec::new(),
        }
    }

    /// The object `id`.
    pub fn get(&self, id: ObjId) -> Option<&Obj> {
        self.objects.get(id.0 as usize)
    }

    /// The object `id`, to change.
    pub fn get_mut(&mut self, id: ObjId) -> Option<&mut Obj> {
        self.objects.get_mut(id.0 as usize)
    }

    /// Property `pid` of `id`.
    pub fn prop(&self, id: ObjId, pid: u16) -> Option<&Prop> {
        self.get(id)?.props.get(&pid)
    }

    /// Adds an object of `class` that `parent`'s strong reference `pid`
    /// will hold (as element `index` of a vector or set), at the path the
    /// writer gives it. The caller sets `parent`'s property.
    pub fn add_child(&mut self, parent: ObjId, pid: u16, index: Option<u32>, class: Auid) -> ObjId {
        let base = ref_name(pid);
        let name = match index {
            Some(i) => element_name(&base, i),
            None => base,
        };
        let parent_path = self.get(parent).map_or("/", |o| o.path.as_str());
        let path = child_path(parent_path, &name);
        let id = ObjId(u32::try_from(self.objects.len()).unwrap_or(u32::MAX));
        self.objects.push(Obj {
            class,
            props: BTreeMap::new(),
            path,
        });
        id
    }
}

/// The name the writer gives property `pid`'s child, vector base or weak
/// index: four lowercase hex digits.
pub fn ref_name(pid: u16) -> String {
    format!("{pid:04x}")
}

/// Element `key`'s storage name under the vector or set `base`.
pub fn element_name(base: &str, key: u32) -> String {
    format!("{base}{{{key:x}}}")
}

/// The path of `name` in the storage at `parent`.
pub fn child_path(parent: &str, name: &str) -> String {
    if parent.ends_with('/') {
        format!("{parent}{name}")
    } else {
        format!("{parent}/{name}")
    }
}

/// True for a name that resolves below its storage: 1 to 31 UTF-16 units,
/// not `.` or `..`, no separator or other character the compound-file
/// format refuses.
fn valid_name(name: &str) -> bool {
    let units = name.encode_utf16().count();
    (1..=31).contains(&units)
        && name != "."
        && name != ".."
        && !name.contains(['/', '\\', ':', '!', '\0'])
}

/// A UTF-16LE name, NUL-terminated or not; invalid UTF-16 or an odd
/// length is `None`.
fn decode_name(d: &[u8]) -> Option<String> {
    if d.len() % 2 == 1 {
        return None;
    }
    let units: Vec<u16> = d
        .chunks_exact(2)
        .map(|c| {
            u16::from_le_bytes([
                c.first().copied().unwrap_or(0),
                c.get(1).copied().unwrap_or(0),
            ])
        })
        .take_while(|&u| u != 0)
        .collect();
    String::from_utf16(&units).ok()
}

fn utf16z(s: &str) -> Vec<u8> {
    let mut out: Vec<u8> = s.encode_utf16().flat_map(u16::to_le_bytes).collect();
    out.extend_from_slice(&[0, 0]);
    out
}

/// Little-endian readers over a byte slice.
struct Bytes<'a>(&'a [u8]);

impl<'a> Bytes<'a> {
    fn take(&mut self, n: usize) -> Option<&'a [u8]> {
        if n > self.0.len() {
            return None;
        }
        let (a, b) = self.0.split_at(n);
        self.0 = b;
        Some(a)
    }
    fn u8(&mut self) -> Option<u8> {
        self.take(1)?.first().copied()
    }
    fn u16(&mut self) -> Option<u16> {
        self.take(2)?.try_into().ok().map(u16::from_le_bytes)
    }
    fn u32(&mut self) -> Option<u32> {
        self.take(4)?.try_into().ok().map(u32::from_le_bytes)
    }
}

/// The compound-file signature.
const MAGIC: [u8; 8] = [0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1];
/// A placeholder for a child not read yet; removed before the store is
/// returned.
const UNSET: ObjId = ObjId(u32::MAX);
/// Bytes counted per object and per property beyond their data, against
/// [`AafLimits::max_model_bytes`].
const OBJ_COST: usize = std::mem::size_of::<Obj>() + 64;
const PROP_COST: usize = std::mem::size_of::<Prop>() + 32;

/// Why one object or property was not read.
enum Bad {
    /// Leave it out, listed with this detail.
    Part(String),
    /// Refuse the whole file.
    File(IoError),
}

fn refused(s: &str) -> IoError {
    IoError::Refused(s.to_string())
}

/// The compound file we read through.
type Cfb = cfb::CompoundFile<File>;

/// Reads the first `n` bytes of stream `path` (all of it when shorter, so
/// at most its real length is allocated); returns them and the stream's
/// length.
fn read_head(c: &mut Cfb, path: &str, n: u64) -> Result<(Vec<u8>, u64), String> {
    let mut s = c
        .open_stream(path)
        .map_err(|_| format!("{path}: missing"))?;
    let len = s.len();
    let want = n.min(len);
    let mut out = Vec::with_capacity(usize::try_from(want).unwrap_or(0));
    (&mut s)
        .take(want)
        .read_to_end(&mut out)
        .map_err(|_| format!("{path}: unreadable"))?;
    if (out.len() as u64) < want {
        return Err(format!("{path}: short"));
    }
    Ok((out, len))
}

/// A pending child: its path, depth, and the slot of its parent's property
/// it fills (parent, pid, position).
struct Pending {
    path: String,
    depth: usize,
    slot: Option<(ObjId, u16, usize)>,
}

/// The walk's state.
struct Walk<'a> {
    c: &'a mut Cfb,
    limits: &'a AafLimits,
    report: &'a mut Report,
    objects: Vec<Obj>,
    stack: Vec<Pending>,
    seen: HashSet<String>,
    /// The index streams the current object's properties have used: each
    /// is read once, so properties sharing one cannot multiply its cost.
    indexes: HashSet<String>,
    queued: usize,
    bytes: usize,
    cancel: &'a AtomicBool,
}

impl Walk<'_> {
    fn left_out(&mut self, detail: String) {
        let (d, _) = crate::text::sanitize(detail.as_bytes(), &crate::limits::Limits::default());
        self.report
            .add(ItemKind::LeftOut, UNREADABLE_PARTS, Some(d));
    }

    /// [`spend`](Self::spend) for the walk's inner steps: a refusal of
    /// the whole file.
    fn charge(&mut self, n: usize) -> Result<(), Bad> {
        self.spend(n).map_err(Bad::File)
    }

    fn cancelled(&self) -> Result<(), Bad> {
        if self.cancel.load(Ordering::Relaxed) {
            return Err(Bad::File(IoError::Cancelled));
        }
        Ok(())
    }

    /// Claims index stream `ip` for the current object; a second claim is
    /// a damaged property.
    fn claim(&mut self, ip: &str) -> Result<(), String> {
        if !self.indexes.insert(ip.to_uppercase()) {
            return Err("shares an index with another property".into());
        }
        Ok(())
    }

    fn spend(&mut self, n: usize) -> Result<(), IoError> {
        self.bytes = self.bytes.saturating_add(n);
        if self.bytes > self.limits.max_model_bytes {
            return Err(refused(TOO_LARGE));
        }
        Ok(())
    }

    /// Queues the child `name` of the object at `parent_path` (depth
    /// `depth`) for slot `slot`; false when the name or storage is bad
    /// (already listed).
    fn queue(
        &mut self,
        parent_path: &str,
        name: &str,
        depth: usize,
        slot: (ObjId, u16, usize),
    ) -> Result<bool, IoError> {
        let path = child_path(parent_path, name);
        if !valid_name(name) {
            self.left_out(format!("{parent_path}: a reference to {name:?}"));
            return Ok(false);
        }
        if !self.c.is_storage(&path) {
            self.left_out(format!("{path}: missing"));
            return Ok(false);
        }
        // Names compare without case in a compound file.
        if !self.seen.insert(path.to_uppercase()) {
            self.left_out(format!("{path}: referenced twice"));
            return Ok(false);
        }
        let depth = depth.saturating_add(1);
        if depth > self.limits.max_depth {
            return Err(refused(TOO_DEEP));
        }
        self.queued = self.queued.saturating_add(1);
        if self.queued > self.limits.max_objects {
            return Err(refused(TOO_MANY_OBJECTS));
        }
        self.spend(path.len().saturating_add(std::mem::size_of::<Pending>()))?;
        self.stack.push(Pending {
            path,
            depth,
            slot: Some(slot),
        });
        Ok(true)
    }

    /// Reads one object's storage into an [`Obj`] (strong references as
    /// [`UNSET`] placeholders) and the children to queue.
    fn read_obj(&mut self, path: &str) -> Result<(Obj, Vec<Child>), Bad> {
        let class = self
            .c
            .entry(path)
            .map(|e| Auid::of_uuid(e.clsid()))
            .map_err(|_| Bad::Part(format!("{path}: missing")))?;
        let pp = child_path(path, "properties");
        let len = self
            .c
            .entry(&pp)
            .ok()
            .filter(cfb::Entry::is_stream)
            .map(|e| e.len())
            .ok_or_else(|| Bad::Part(format!("{path}: no properties")))?;
        if len > self.limits.max_properties_bytes {
            return Err(Bad::Part(format!("{path}: properties too large")));
        }
        let (bytes, _) = read_head(self.c, &pp, len).map_err(Bad::Part)?;
        let mut b = Bytes(&bytes);
        let order = b
            .u8()
            .ok_or_else(|| Bad::Part(format!("{path}: empty properties")))?;
        match order {
            0x4C => {}
            0x42 => return Err(Bad::File(refused(BIG_ENDIAN))),
            _ => return Err(Bad::Part(format!("{path}: unknown byte order"))),
        }
        let _version = b.u8();
        let count = b
            .u16()
            .ok_or_else(|| Bad::Part(format!("{path}: short properties")))?;
        let mut entries = Vec::with_capacity(usize::from(count));
        for _ in 0..count {
            match (b.u16(), b.u16(), b.u16()) {
                (Some(pid), Some(form), Some(len)) => entries.push((pid, form, len)),
                _ => return Err(Bad::Part(format!("{path}: short properties"))),
            }
        }
        let mut obj = Obj {
            class,
            props: BTreeMap::new(),
            path: path.to_string(),
        };
        let mut kids = Vec::new();
        self.indexes.clear();
        for (pid, form, len) in entries {
            self.cancelled()?;
            let Some(v) = b.take(usize::from(len)) else {
                return Err(Bad::Part(format!("{path}: short properties")));
            };
            if obj.props.contains_key(&pid) {
                self.left_out(format!("{path}: property {pid:04x} twice"));
                continue;
            }
            match self.read_prop(path, pid, form, v, &mut kids) {
                Ok(p) => {
                    let _ = obj.props.insert(pid, p);
                }
                Err(Bad::Part(d)) => self.left_out(d),
                Err(e @ Bad::File(_)) => return Err(e),
            }
        }
        Ok((obj, kids))
    }

    fn read_prop(
        &mut self,
        path: &str,
        pid: u16,
        form: u16,
        v: &[u8],
        kids: &mut Vec<Child>,
    ) -> Result<Prop, Bad> {
        let bad = |what: &str| Bad::Part(format!("{path}: property {pid:04x} {what}"));
        let name = || decode_name(v).ok_or_else(|| bad("has a bad name"));
        match form {
            form::DATA | form::UNIQUE_ID | form::OPAQUE_STREAM | form::WEAK_STORED_ID => {
                Ok(Prop::Data(v.to_vec()))
            }
            form::DATA_STREAM => {
                // A byte-order byte, then the name.
                let n = match v.split_first() {
                    Some((_, rest)) if v.len() % 2 == 1 => decode_name(rest),
                    _ => decode_name(v),
                }
                .filter(|n| valid_name(n))
                .ok_or_else(|| bad("names a bad stream"))?;
                Ok(Prop::Stream(n))
            }
            form::STRONG => {
                let n = name()?;
                kids.push(Child {
                    pid,
                    pos: 0,
                    name: n,
                });
                Ok(Prop::Strong(UNSET))
            }
            form::STRONG_VECTOR | form::STRONG_SET => {
                let base = name()?;
                if !valid_name(&base) {
                    return Err(bad("has a bad name"));
                }
                let keys = match self.strong_index(path, &base, form == form::STRONG_SET) {
                    Ok(k) => k,
                    Err(Bad::Part(w)) => return Err(bad(&w)),
                    Err(e) => return Err(e),
                };
                for (pos, k) in keys.iter().enumerate() {
                    if pos % 4096 == 0 {
                        self.cancelled()?;
                    }
                    kids.push(Child {
                        pid,
                        pos,
                        name: element_name(&base, *k),
                    });
                }
                let ids = vec![UNSET; keys.len()];
                Ok(if form == form::STRONG_SET {
                    Prop::StrongSet(ids)
                } else {
                    Prop::StrongVec(ids)
                })
            }
            form::WEAK => {
                let mut b = Bytes(v);
                match (b.u16(), b.u16(), b.u8()) {
                    (Some(table_index), Some(id_pid), Some(size))
                        if b.0.len() == usize::from(size) =>
                    {
                        Ok(Prop::Weak(WeakRef {
                            table_index,
                            id_pid,
                            id: b.0.to_vec(),
                        }))
                    }
                    _ => Err(bad("is a bad weak reference")),
                }
            }
            form::WEAK_VECTOR | form::WEAK_SET => {
                let n = name()?;
                if !valid_name(&n) {
                    return Err(bad("has a bad name"));
                }
                let refs = match self.weak_index(path, &n) {
                    Ok(r) => r,
                    Err(Bad::Part(w)) => return Err(bad(&w)),
                    Err(e) => return Err(e),
                };
                Ok(if form == form::WEAK_SET {
                    Prop::WeakSet(refs)
                } else {
                    Prop::WeakVec(refs)
                })
            }
            _ => Err(bad(&format!("has unknown form {form:#x}"))),
        }
    }

    /// The local keys of the strong vector or set `base` in the storage
    /// at `path`.
    /// Its cost (the index read, and per key a child, its name and its
    /// slot) is charged before anything is allocated.
    fn strong_index(&mut self, path: &str, base: &str, set: bool) -> Result<Vec<u32>, Bad> {
        let ip = child_path(path, &format!("{base} index"));
        self.claim(&ip).map_err(Bad::Part)?;
        let per_key = std::mem::size_of::<Child>()
            .saturating_add(std::mem::size_of::<ObjId>())
            .saturating_add(std::mem::size_of::<u32>())
            .saturating_add(base.len())
            .saturating_add(10);
        let keys = self.strong_keys(&ip, set, per_key).map_err(|e| match e {
            Ok(why) => Bad::Part(why),
            Err(file) => Bad::File(file),
        })?;
        Ok(keys)
    }

    /// The keys of index `ip`; `Err(Ok(why))` leaves the property out,
    /// `Err(Err(e))` refuses the file.
    fn strong_keys(
        &mut self,
        ip: &str,
        set: bool,
        per_key: usize,
    ) -> Result<Vec<u32>, Result<String, IoError>> {
        let part = |s: &str| Ok(s.to_string());
        let head_len: u64 = if set { 15 } else { 12 };
        let (head, len) = read_head(self.c, ip, head_len).map_err(|_| part("has no index"))?;
        let mut b = Bytes(&head);
        let count = b.u32().ok_or_else(|| part("has a short index"))?;
        let _ = (b.u32(), b.u32());
        let size = if set {
            let _key_pid = b.u16().ok_or_else(|| part("has a short index"))?;
            u64::from(b.u8().ok_or_else(|| part("has a short index"))?)
        } else {
            0
        };
        let entry = if set { 8 + size } else { 4 };
        let need = u64::from(count)
            .checked_mul(entry)
            .and_then(|n| n.checked_add(head_len))
            .ok_or_else(|| part("has an index too long for its stream"))?;
        let Some(n) = usize::try_from(count)
            .ok()
            .filter(|c| *c <= self.limits.max_index_entries)
        else {
            return Err(part("has too many entries"));
        };
        if need > len {
            return Err(part("has an index too long for its stream"));
        }
        self.spend(usize::try_from(need).unwrap_or(usize::MAX))
            .and_then(|()| self.spend(n.saturating_mul(per_key)))
            .map_err(Err)?;
        let (all, _) = read_head(self.c, ip, need).map_err(|_| part("has an unreadable index"))?;
        let body = all
            .get(usize::try_from(head_len).unwrap_or(usize::MAX)..)
            .unwrap_or(&[]);
        let step = usize::try_from(entry).unwrap_or(4);
        let keys: Vec<u32> = body
            .chunks_exact(step)
            .filter_map(|e| {
                e.get(..4)
                    .and_then(|k| k.try_into().ok())
                    .map(u32::from_le_bytes)
            })
            .collect();
        Ok(keys)
    }

    /// The entries of the weak vector or set whose index is `<name> index`
    /// in the storage at `path`.
    /// Its cost (the index read, and per entry a reference and its id) is
    /// charged before anything is allocated.
    fn weak_index(&mut self, path: &str, name: &str) -> Result<Vec<WeakRef>, Bad> {
        let part = |s: &str| Bad::Part(s.to_string());
        let ip = child_path(path, &format!("{name} index"));
        self.claim(&ip).map_err(Bad::Part)?;
        let (head, len) = read_head(self.c, &ip, 9).map_err(|_| part("has no index"))?;
        let mut b = Bytes(&head);
        let (Some(count), Some(table_index), Some(id_pid), Some(size)) =
            (b.u32(), b.u16(), b.u16(), b.u8())
        else {
            return Err(part("has a short index"));
        };
        // An empty identification would let the count outgrow the stream.
        if size == 0 && count > 0 {
            return Err(part("has empty identifications"));
        }
        let need = u64::from(count)
            .checked_mul(u64::from(size))
            .and_then(|n| n.checked_add(9))
            .ok_or_else(|| part("has an index too long for its stream"))?;
        let Some(n) = usize::try_from(count)
            .ok()
            .filter(|c| *c <= self.limits.max_index_entries)
        else {
            return Err(part("has too many entries"));
        };
        if need > len {
            return Err(part("has an index too long for its stream"));
        }
        let size = usize::from(size);
        // An id's heap block is at least a pointer's size.
        let per_entry = std::mem::size_of::<WeakRef>().saturating_add(size.max(8));
        self.charge(usize::try_from(need).unwrap_or(usize::MAX))?;
        self.charge(n.saturating_mul(per_entry))?;
        let (all, _) = read_head(self.c, &ip, need).map_err(|_| part("has an unreadable index"))?;
        let body = all.get(9..).unwrap_or(&[]);
        let mut refs = Vec::with_capacity(n);
        for i in 0..n {
            if i % 4096 == 0 {
                self.cancelled()?;
            }
            let id = body
                .get(i.saturating_mul(size)..i.saturating_add(1).saturating_mul(size))
                .unwrap_or(&[]);
            refs.push(WeakRef {
                table_index,
                id_pid,
                id: id.to_vec(),
            });
        }
        Ok(refs)
    }
}

/// A child named by a strong reference: its property, position and name.
struct Child {
    pid: u16,
    pos: usize,
    name: String,
}

fn prop_cost(p: &Prop) -> usize {
    PROP_COST.saturating_add(match p {
        Prop::Data(d) => d.len(),
        Prop::Stream(s) => s.len(),
        Prop::Weak(w) => w.id.len(),
        // Vectors and sets were charged as their index was read.
        Prop::Strong(_)
        | Prop::StrongVec(_)
        | Prop::StrongSet(_)
        | Prop::WeakVec(_)
        | Prop::WeakSet(_) => 0,
    })
}

/// Reads the whole store of `c`.
fn read_store(
    c: &mut Cfb,
    limits: &AafLimits,
    cancel: &AtomicBool,
    report: &mut Report,
) -> Result<Store, IoError> {
    if !c.is_stream("/properties") {
        return Err(refused(NOT_AAF));
    }
    let mut w = Walk {
        c,
        limits,
        report,
        objects: Vec::new(),
        stack: vec![Pending {
            path: "/".to_string(),
            depth: 0,
            slot: None,
        }],
        seen: HashSet::new(),
        indexes: HashSet::new(),
        queued: 1,
        bytes: 0,
        cancel,
    };
    if w.queued > limits.max_objects {
        return Err(refused(TOO_MANY_OBJECTS));
    }
    while let Some(p) = w.stack.pop() {
        if cancel.load(Ordering::Relaxed) {
            return Err(IoError::Cancelled);
        }
        let (obj, kids) = match w.read_obj(&p.path) {
            Ok(r) => r,
            Err(Bad::File(e)) => return Err(e),
            // The root itself unreadable: nothing to read below it.
            Err(Bad::Part(_)) if p.slot.is_none() => return Err(refused(DAMAGED)),
            Err(Bad::Part(d)) => {
                w.left_out(d);
                continue;
            }
        };
        let cost = obj
            .props
            .values()
            .fold(OBJ_COST.saturating_add(obj.path.len()), |n, p| {
                n.saturating_add(prop_cost(p))
            });
        w.spend(cost)?;
        let id = ObjId(u32::try_from(w.objects.len()).map_err(|_| refused(TOO_MANY_OBJECTS))?);
        let path = obj.path.clone();
        w.objects.push(obj);
        if let Some((parent, pid, pos)) = p.slot {
            fill(&mut w.objects, parent, pid, pos, id);
        }
        // Queued in reverse, so the stack pops them in file order.
        for (i, k) in kids.iter().rev().enumerate() {
            if i % 4096 == 0 && cancel.load(Ordering::Relaxed) {
                return Err(IoError::Cancelled);
            }
            let _ = w.queue(&path, &k.name, p.depth, (id, k.pid, k.pos))?;
        }
    }
    let mut objects = w.objects;
    for o in &mut objects {
        o.props.retain(|_, p| match p {
            Prop::Strong(id) => *id != UNSET,
            Prop::StrongVec(v) | Prop::StrongSet(v) => {
                v.retain(|id| *id != UNSET);
                true
            }
            _ => true,
        });
    }
    let referenced = read_referenced(w.c, limits, w.report);
    Ok(Store {
        objects,
        root: ObjId(0),
        referenced,
    })
}

/// Sets slot `pos` of `parent`'s strong property `pid` to `id`.
fn fill(objects: &mut [Obj], parent: ObjId, pid: u16, pos: usize, id: ObjId) {
    let Some(p) = objects
        .get_mut(parent.0 as usize)
        .and_then(|o| o.props.get_mut(&pid))
    else {
        return;
    };
    match p {
        Prop::Strong(slot) => *slot = id,
        Prop::StrongVec(v) | Prop::StrongSet(v) => {
            if let Some(slot) = v.get_mut(pos) {
                *slot = id;
            }
        }
        _ => {}
    }
}

/// The root's `referenced properties` table; a damaged one is listed and
/// what was read is kept.
fn read_referenced(c: &mut Cfb, limits: &AafLimits, report: &mut Report) -> Vec<Vec<u16>> {
    const PATH: &str = "/referenced properties";
    let mut damaged = || report.add(ItemKind::LeftOut, UNREADABLE_PARTS, Some(PATH.to_string()));
    let len = match c.entry(PATH) {
        Ok(e) if e.is_stream() => e.len(),
        _ => return Vec::new(),
    };
    if len > limits.max_properties_bytes {
        damaged();
        return Vec::new();
    }
    let Ok((bytes, _)) = read_head(c, PATH, len) else {
        damaged();
        return Vec::new();
    };
    let mut b = Bytes(&bytes);
    let (Some(0x4C), Some(paths), Some(pids)) = (b.u8(), b.u16(), b.u32()) else {
        damaged();
        return Vec::new();
    };
    let mut out = Vec::new();
    let mut cur = Vec::new();
    for _ in 0..pids {
        match b.u16() {
            Some(0) => out.push(std::mem::take(&mut cur)),
            Some(p) => cur.push(p),
            None => break,
        }
    }
    if out.len() != usize::from(paths) || !cur.is_empty() {
        damaged();
    }
    out
}

/// An open AAF file: its object store, what was left out reading it, and
/// its data streams on demand.
pub struct AafReader {
    cfb: Mutex<Cfb>,
    store: Store,
    report: Report,
    limits: AafLimits,
}

impl fmt::Debug for AafReader {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AafReader")
            .field("objects", &self.store.objects.len())
            .field("limits", &self.limits)
            .finish_non_exhaustive()
    }
}

impl AafReader {
    /// Opens the regular file at `path` ([`crate::read::open_regular`]) and
    /// reads its whole object store within `limits`.
    pub fn open(
        path: &Path,
        limits: &AafLimits,
        cancel: &AtomicBool,
    ) -> Result<AafReader, IoError> {
        if cancel.load(Ordering::Relaxed) {
            return Err(IoError::Cancelled);
        }
        let mut file = crate::read::open_regular(path)?;
        let mut magic = [0u8; 8];
        let mut got = 0;
        while got < magic.len() {
            match file.read(magic.get_mut(got..).unwrap_or(&mut [])) {
                Ok(0) => break,
                Ok(n) => got += n,
                Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
                Err(e) => return Err(crate::io_err(path, "read", &e)),
            }
        }
        if got < magic.len() || magic != MAGIC {
            return Err(refused(NOT_AAF));
        }
        file.seek(SeekFrom::Start(0))
            .map_err(|e| crate::io_err(path, "read", &e))?;
        let opened = catch_unwind(AssertUnwindSafe(|| cfb::CompoundFile::open(file)));
        let mut c = match opened {
            Ok(Ok(c)) => c,
            _ => return Err(refused(DAMAGED)),
        };
        let mut report = Report::default();
        let read = catch_unwind(AssertUnwindSafe(|| {
            read_store(&mut c, limits, cancel, &mut report)
        }));
        let store = match read {
            Ok(r) => r?,
            Err(_) => return Err(refused(DAMAGED)),
        };
        Ok(AafReader {
            cfb: Mutex::new(c),
            store,
            report,
            limits: *limits,
        })
    }

    /// The objects read.
    pub fn store(&self) -> &Store {
        &self.store
    }

    /// What reading left out.
    pub fn report(&self) -> &Report {
        &self.report
    }

    /// The path of data stream `name` of `obj`; a name that would leave the
    /// object's storage, or a missing object, is refused.
    fn stream_path(&self, obj: ObjId, name: &str) -> Result<String, IoError> {
        let o = self.store.get(obj).ok_or_else(|| refused(DAMAGED))?;
        if !valid_name(name) {
            return Err(refused(DAMAGED));
        }
        Ok(child_path(&o.path, name))
    }

    /// The length of data stream `name` of `obj`, in bytes.
    pub fn stream_len(&self, obj: ObjId, name: &str) -> Result<u64, IoError> {
        let path = self.stream_path(obj, name)?;
        let c = self.cfb.lock().map_err(|_| refused(DAMAGED))?;
        match c.entry(&path) {
            Ok(e) if e.is_stream() => Ok(e.len()),
            _ => Err(refused(DAMAGED)),
        }
    }

    /// Streams data stream `name` of `obj` into the new file `to`
    /// (`create_new`), at most `max` bytes: a longer stream is refused
    /// before `to` is made. On any failure or cancel, `to` is removed.
    pub fn copy_stream(
        &self,
        obj: ObjId,
        name: &str,
        to: &Path,
        max: u64,
        cancel: &AtomicBool,
    ) -> Result<u64, IoError> {
        self.copy_stream_after(obj, name, &[], to, max, cancel)
    }

    /// [`AafReader::copy_stream`], with `prefix` written first (a header
    /// the stream lacks). `max` bounds the stream, not the prefix; the
    /// count returned includes both.
    pub fn copy_stream_after(
        &self,
        obj: ObjId,
        name: &str,
        prefix: &[u8],
        to: &Path,
        max: u64,
        cancel: &AtomicBool,
    ) -> Result<u64, IoError> {
        let path = self.stream_path(obj, name)?;
        let mut c = self.cfb.lock().map_err(|_| refused(DAMAGED))?;
        let len = match c.entry(&path) {
            Ok(e) if e.is_stream() => e.len(),
            _ => return Err(refused(DAMAGED)),
        };
        if len > max {
            return Err(refused(STREAM_TOO_LARGE));
        }
        if cancel.load(Ordering::Relaxed) {
            return Err(IoError::Cancelled);
        }
        let mut out = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(to)
            .map_err(|e| crate::io_err(to, "created", &e))?;
        let copied = catch_unwind(AssertUnwindSafe(|| {
            out.write_all(prefix)
                .map_err(|e| crate::io_err(to, "written", &e))?;
            copy(&mut c, &path, &mut out, to, max, cancel)
                .map(|n| n.saturating_add(prefix.len() as u64))
        }))
        .unwrap_or_else(|_| Err(refused(DAMAGED)));
        drop(out);
        if copied.is_err() {
            let _ = std::fs::remove_file(to);
        }
        copied
    }
}

/// Copies stream `path` into `out`, at most `max` bytes.
fn copy(
    c: &mut Cfb,
    path: &str,
    out: &mut File,
    to: &Path,
    max: u64,
    cancel: &AtomicBool,
) -> Result<u64, IoError> {
    let mut s = c.open_stream(path).map_err(|_| refused(DAMAGED))?;
    let mut buf = vec![0u8; 64 * 1024];
    let mut total: u64 = 0;
    loop {
        if cancel.load(Ordering::Relaxed) {
            return Err(IoError::Cancelled);
        }
        let n = match s.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => n,
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(_) => return Err(refused(DAMAGED)),
        };
        total = total.saturating_add(n as u64);
        if total > max {
            return Err(refused(STREAM_TOO_LARGE));
        }
        out.write_all(buf.get(..n).unwrap_or(&[]))
            .map_err(|e| crate::io_err(to, "written", &e))?;
    }
    out.flush().map_err(|e| crate::io_err(to, "written", &e))?;
    Ok(total)
}

/// A writer of AAF files: a `cfb` compound file of 4 KiB sectors
/// (`Version::V4`).
pub struct AafWriter<W: Read + Write + Seek> {
    cfb: cfb::CompoundFile<W>,
    /// The storage path each object was written at.
    paths: Vec<Option<String>>,
}

impl<W: Read + Write + Seek> fmt::Debug for AafWriter<W> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AafWriter").finish_non_exhaustive()
    }
}

fn internal(what: &str) -> IoError {
    IoError::Refused(format!("internal error: {what}"))
}

fn cfb_err(e: std::io::Error) -> IoError {
    IoError::Io(e.to_string())
}

/// The identifying properties of the classes held in sets (each class's
/// unique-identifier property): a set's key is the first of these all its elements
/// hold. `MobID`, `EssenceData::MobID`, `DefinitionObject::Identification`
/// and `MetaDefinition::Identification`.
const SET_KEYS: [u16; 4] = [0x4401, 0x2701, 0x1B01, 0x0005];

impl<W: Read + Write + Seek> AafWriter<W> {
    /// A new, empty compound file over `inner`.
    pub fn create(inner: W) -> Result<Self, IoError> {
        let cfb =
            cfb::CompoundFile::create_with_version(cfb::Version::V4, inner).map_err(cfb_err)?;
        Ok(AafWriter {
            cfb,
            paths: Vec::new(),
        })
    }

    /// Writes every object reachable from `s.root` by strong references,
    /// and the referenced-properties table. An object reached twice, a
    /// value over 65 535 bytes or a weak vector whose entries differ in
    /// table or key is refused as an internal error.
    pub fn write_store(&mut self, s: &Store) -> Result<(), IoError> {
        self.paths = vec![None; s.objects.len()];
        let mut stack = vec![(s.root, "/".to_string())];
        while let Some((id, path)) = stack.pop() {
            let slot = self
                .paths
                .get_mut(id.0 as usize)
                .ok_or_else(|| internal("a reference to no object"))?;
            if slot.is_some() {
                return Err(internal("an object referenced twice"));
            }
            *slot = Some(path.clone());
            let o = s
                .get(id)
                .ok_or_else(|| internal("a reference to no object"))?;
            if path != "/" {
                self.cfb.create_storage(&path).map_err(cfb_err)?;
            }
            self.cfb
                .set_storage_clsid(&path, o.class.uuid())
                .map_err(cfb_err)?;
            let mut entries: Vec<(u16, u16, Vec<u8>)> = Vec::with_capacity(o.props.len());
            let mut kids = Vec::new();
            for (&pid, p) in &o.props {
                let (form, value) = self.write_prop(s, &path, pid, p, &mut kids)?;
                entries.push((pid, form, value));
            }
            let mut bytes = vec![0x4C, 0x20];
            let n = u16::try_from(entries.len()).map_err(|_| internal("too many properties"))?;
            bytes.extend_from_slice(&n.to_le_bytes());
            for (pid, form, v) in &entries {
                let len =
                    u16::try_from(v.len()).map_err(|_| internal("a property over 65 535 bytes"))?;
                bytes.extend_from_slice(&pid.to_le_bytes());
                bytes.extend_from_slice(&form.to_le_bytes());
                bytes.extend_from_slice(&len.to_le_bytes());
            }
            for (_, _, v) in &entries {
                bytes.extend_from_slice(v);
            }
            self.put(&child_path(&path, "properties"), &bytes)?;
            stack.extend(kids.into_iter().rev());
        }
        let mut t = vec![0x4C];
        let paths = u16::try_from(s.referenced.len())
            .map_err(|_| internal("too many referenced properties"))?;
        let pids: usize = s.referenced.iter().map(|p| p.len().saturating_add(1)).sum();
        let pids = u32::try_from(pids).map_err(|_| internal("too many referenced properties"))?;
        t.extend_from_slice(&paths.to_le_bytes());
        t.extend_from_slice(&pids.to_le_bytes());
        for p in &s.referenced {
            for pid in p {
                t.extend_from_slice(&pid.to_le_bytes());
            }
            t.extend_from_slice(&[0, 0]);
        }
        self.put("/referenced properties", &t)
    }

    fn put(&mut self, path: &str, bytes: &[u8]) -> Result<(), IoError> {
        let mut st = self.cfb.create_stream(path).map_err(cfb_err)?;
        st.write_all(bytes).map_err(cfb_err)?;
        st.flush().map_err(cfb_err)
    }

    /// One property's stored form and value; writes its index stream and
    /// queues its children.
    fn write_prop(
        &mut self,
        s: &Store,
        path: &str,
        pid: u16,
        p: &Prop,
        kids: &mut Vec<(ObjId, String)>,
    ) -> Result<(u16, Vec<u8>), IoError> {
        let base = ref_name(pid);
        Ok(match p {
            Prop::Data(d) => (form::DATA, d.clone()),
            Prop::Strong(id) => {
                kids.push((*id, child_path(path, &base)));
                (form::STRONG, utf16z(&base))
            }
            Prop::StrongVec(v) | Prop::StrongSet(v) => {
                let set = matches!(p, Prop::StrongSet(_));
                let n = u32::try_from(v.len()).map_err(|_| internal("too many elements"))?;
                let mut idx = Vec::new();
                idx.extend_from_slice(&n.to_le_bytes());
                idx.extend_from_slice(&n.to_le_bytes());
                idx.extend_from_slice(&u32::MAX.to_le_bytes());
                let key = if set { set_key(s, v)? } else { None };
                if set {
                    let (kp, size) = key.as_ref().map_or((0, 0), |(kp, keys)| {
                        (*kp, keys.first().map_or(0, |k: &Vec<u8>| k.len()))
                    });
                    idx.extend_from_slice(&kp.to_le_bytes());
                    idx.push(u8::try_from(size).map_err(|_| internal("a set key over 255 bytes"))?);
                }
                for (i, id) in (0u32..).zip(v) {
                    idx.extend_from_slice(&i.to_le_bytes());
                    if let Some((_, keys)) = &key {
                        idx.extend_from_slice(&1u32.to_le_bytes());
                        idx.extend_from_slice(keys.get(i as usize).map_or(&[][..], Vec::as_slice));
                    }
                    kids.push((*id, child_path(path, &element_name(&base, i))));
                }
                self.put(&child_path(path, &format!("{base} index")), &idx)?;
                (
                    if set {
                        form::STRONG_SET
                    } else {
                        form::STRONG_VECTOR
                    },
                    utf16z(&base),
                )
            }
            Prop::Weak(w) => {
                let size =
                    u8::try_from(w.id.len()).map_err(|_| internal("a weak key over 255 bytes"))?;
                let mut v = Vec::with_capacity(5 + w.id.len());
                v.extend_from_slice(&w.table_index.to_le_bytes());
                v.extend_from_slice(&w.id_pid.to_le_bytes());
                v.push(size);
                v.extend_from_slice(&w.id);
                (form::WEAK, v)
            }
            Prop::WeakVec(v) | Prop::WeakSet(v) => {
                let first = v.first();
                let (ti, kp, size) =
                    first.map_or((0, 0, 0), |w| (w.table_index, w.id_pid, w.id.len()));
                if v.iter()
                    .any(|w| w.table_index != ti || w.id_pid != kp || w.id.len() != size)
                {
                    return Err(internal("a weak vector whose entries differ"));
                }
                let n = u32::try_from(v.len()).map_err(|_| internal("too many elements"))?;
                let mut idx = Vec::new();
                idx.extend_from_slice(&n.to_le_bytes());
                idx.extend_from_slice(&ti.to_le_bytes());
                idx.extend_from_slice(&kp.to_le_bytes());
                idx.push(u8::try_from(size).map_err(|_| internal("a weak key over 255 bytes"))?);
                for w in v {
                    idx.extend_from_slice(&w.id);
                }
                self.put(&child_path(path, &format!("{base} index")), &idx)?;
                let f = if matches!(p, Prop::WeakSet(_)) {
                    form::WEAK_SET
                } else {
                    form::WEAK_VECTOR
                };
                (f, utf16z(&base))
            }
            Prop::Stream(name) => {
                if !valid_name(name) || name == "properties" || name.ends_with(" index") {
                    return Err(internal("a bad data stream name"));
                }
                self.put(&child_path(path, name), &[])?;
                let mut v = vec![0x55];
                v.extend_from_slice(&utf16z(name));
                (form::DATA_STREAM, v)
            }
        })
    }

    /// Opens data stream `name` of `obj` (written by
    /// [`write_store`](Self::write_store)) for the caller to fill, as
    /// essence. The caller flushes it.
    pub fn stream(
        &mut self,
        s: &Store,
        obj: ObjId,
        name: &str,
    ) -> Result<impl Write + '_, IoError> {
        let has = s.get(obj).is_some_and(|o| {
            o.props
                .values()
                .any(|p| matches!(p, Prop::Stream(n) if n == name))
        });
        let path = self.paths.get(obj.0 as usize).cloned().flatten();
        let (true, Some(path)) = (has, path) else {
            return Err(internal("no such data stream"));
        };
        self.cfb
            .create_stream(child_path(&path, name))
            .map_err(cfb_err)
    }

    /// Flushes the compound file and returns what it was written into.
    pub fn finish(mut self) -> Result<W, IoError> {
        self.cfb.flush().map_err(cfb_err)?;
        Ok(self.cfb.into_inner())
    }
}

/// A set's key PID and each element's key, in order.
type SetKey = (u16, Vec<Vec<u8>>);

/// A set's key: the first of [`SET_KEYS`] that every element holds as
/// data of one length, and each element's key. An empty set has none.
fn set_key(s: &Store, v: &[ObjId]) -> Result<Option<SetKey>, IoError> {
    if v.is_empty() {
        return Ok(None);
    }
    for kp in SET_KEYS {
        let keys: Option<Vec<Vec<u8>>> = v
            .iter()
            .map(|id| match s.prop(*id, kp) {
                Some(Prop::Data(d)) => Some(d.clone()),
                _ => None,
            })
            .collect();
        if let Some(keys) = keys
            && keys
                .iter()
                .all(|k| Some(k.len()) == keys.first().map(Vec::len))
        {
            return Ok(Some((kp, keys)));
        }
    }
    Err(internal("a set whose elements have no common key"))
}
