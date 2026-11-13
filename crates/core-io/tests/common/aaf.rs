//! AAF fixtures (plan 6f): a small object tree built through `Store`, a
//! canonical form for comparing two stores, and raw compound-file builders
//! that can write any `properties` bytes, so the hostile cases bypass
//! `AafWriter`'s checks.

use core_io::aaf::dict::{class, def, pid};
use core_io::aaf::store::{Auid, Obj, ObjId, Prop, Store, WeakRef};
use std::collections::BTreeMap;
use std::io::Write;
use std::path::Path;

/// `s` as UTF-16LE with a NUL, as AAF stores names and strings.
pub fn utf16z(s: &str) -> Vec<u8> {
    let mut out: Vec<u8> = s.encode_utf16().flat_map(u16::to_le_bytes).collect();
    out.extend_from_slice(&[0, 0]);
    out
}

/// A 32-byte MobID whose last byte is `n`.
pub fn mob_id(n: u8) -> Vec<u8> {
    let mut id = vec![0x06, 0x0a, 0x2b, 0x34, 0x01, 0x01, 0x01, 0x05];
    id.resize(31, 0x11);
    id.push(n);
    id
}

fn set(s: &mut Store, id: ObjId, pid: u16, p: Prop) {
    s.objects[id.0 as usize].props.insert(pid, p);
}

/// The small tree of `store_round_trips`: a root, a `Header` whose
/// `ContentStorage` holds a set of two mobs (one with a `Sequence` of three
/// components in a vector), a `Dictionary` with a set of two data
/// definitions that a component's weak reference points into, a weak set on
/// the header, and an `EssenceData` with a data stream.
pub fn tree() -> Store {
    let mut s = Store::new(class::ROOT);
    let root = s.root;
    let header = s.add_child(root, pid::ROOT_HEADER, None, class::HEADER);
    set(&mut s, root, pid::ROOT_HEADER, Prop::Strong(header));
    set(
        &mut s,
        header,
        pid::HEADER_BYTE_ORDER,
        Prop::Data(vec![0x49, 0x49]),
    );
    set(&mut s, header, pid::HEADER_VERSION, Prop::Data(vec![1, 1]));

    let dict = s.add_child(header, pid::HEADER_DICTIONARY, None, class::DICTIONARY);
    set(&mut s, header, pid::HEADER_DICTIONARY, Prop::Strong(dict));
    let mut defs = Vec::new();
    for (i, auid) in [def::DATA_DEF_SOUND, def::DATA_DEF_TIMECODE]
        .iter()
        .enumerate()
    {
        let d = s.add_child(
            dict,
            pid::DICTIONARY_DATA_DEFINITIONS,
            Some(i as u32),
            class::DATA_DEFINITION,
        );
        set(
            &mut s,
            d,
            pid::DEFINITION_OBJECT_IDENTIFICATION,
            Prop::Data(auid.0.to_vec()),
        );
        set(
            &mut s,
            d,
            pid::DEFINITION_OBJECT_NAME,
            Prop::Data(utf16z(&format!("Def {i}"))),
        );
        defs.push(d);
    }
    set(
        &mut s,
        dict,
        pid::DICTIONARY_DATA_DEFINITIONS,
        Prop::StrongSet(defs),
    );

    let content = s.add_child(header, pid::HEADER_CONTENT, None, class::CONTENT_STORAGE);
    set(&mut s, header, pid::HEADER_CONTENT, Prop::Strong(content));
    let sound = WeakRef {
        table_index: 0,
        id_pid: pid::DEFINITION_OBJECT_IDENTIFICATION,
        id: def::DATA_DEF_SOUND.0.to_vec(),
    };
    let mut mobs = Vec::new();
    for i in 0..2u8 {
        let m = s.add_child(
            content,
            pid::CONTENT_STORAGE_MOBS,
            Some(u32::from(i)),
            class::COMPOSITION_MOB,
        );
        set(&mut s, m, pid::MOB_MOB_ID, Prop::Data(mob_id(i)));
        set(
            &mut s,
            m,
            pid::MOB_NAME,
            Prop::Data(utf16z(&format!("Mob {i}"))),
        );
        mobs.push(m);
    }
    set(
        &mut s,
        content,
        pid::CONTENT_STORAGE_MOBS,
        Prop::StrongSet(mobs.clone()),
    );

    let slot = s.add_child(mobs[0], pid::MOB_SLOTS, Some(0), class::TIMELINE_MOB_SLOT);
    set(&mut s, mobs[0], pid::MOB_SLOTS, Prop::StrongVec(vec![slot]));
    set(
        &mut s,
        slot,
        pid::MOB_SLOT_SLOT_ID,
        Prop::Data(1u32.to_le_bytes().to_vec()),
    );
    let seq = s.add_child(slot, pid::MOB_SLOT_SEGMENT, None, class::SEQUENCE);
    set(&mut s, slot, pid::MOB_SLOT_SEGMENT, Prop::Strong(seq));
    set(
        &mut s,
        seq,
        pid::COMPONENT_DATA_DEFINITION,
        Prop::Weak(sound.clone()),
    );
    let mut comps = Vec::new();
    for (i, cls) in [class::FILLER, class::SOURCE_CLIP, class::FILLER]
        .iter()
        .enumerate()
    {
        let c = s.add_child(seq, pid::SEQUENCE_COMPONENTS, Some(i as u32), *cls);
        set(
            &mut s,
            c,
            pid::COMPONENT_LENGTH,
            Prop::Data((100 * i as i64 + 10).to_le_bytes().to_vec()),
        );
        set(
            &mut s,
            c,
            pid::COMPONENT_DATA_DEFINITION,
            Prop::Weak(sound.clone()),
        );
        comps.push(c);
    }
    set(
        &mut s,
        seq,
        pid::SEQUENCE_COMPONENTS,
        Prop::StrongVec(comps),
    );

    let ed = s.add_child(
        content,
        pid::CONTENT_STORAGE_ESSENCE_DATA,
        Some(0),
        class::ESSENCE_DATA,
    );
    set(
        &mut s,
        content,
        pid::CONTENT_STORAGE_ESSENCE_DATA,
        Prop::StrongSet(vec![ed]),
    );
    set(&mut s, ed, pid::ESSENCE_DATA_MOB_ID, Prop::Data(mob_id(9)));
    set(
        &mut s,
        ed,
        pid::ESSENCE_DATA_DATA,
        Prop::Stream("Data-2702".into()),
    );

    set(
        &mut s,
        header,
        pid::HEADER_ESSENCE_CONTAINERS,
        Prop::WeakSet(vec![WeakRef {
            table_index: 1,
            id_pid: pid::DEFINITION_OBJECT_IDENTIFICATION,
            id: def::CONTAINER_DEF_AAF.0.to_vec(),
        }]),
    );
    s.referenced = vec![
        vec![
            pid::ROOT_HEADER,
            pid::HEADER_DICTIONARY,
            pid::DICTIONARY_DATA_DEFINITIONS,
        ],
        vec![
            pid::ROOT_HEADER,
            pid::HEADER_DICTIONARY,
            pid::DICTIONARY_CONTAINER_DEFINITIONS,
        ],
    ];
    s
}

/// A property with its object references replaced by the objects' paths.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Canon {
    /// Data, as stored.
    Data(Vec<u8>),
    /// A strong reference, single, vector or set (0, 1, 2), by path.
    Strong(u8, Vec<String>),
    /// A weak reference, single, vector or set (0, 1, 2).
    Weak(u8, Vec<WeakRef>),
    /// A data stream's name.
    Stream(String),
}

/// `s` as (path, class, properties) sorted by path, with references by
/// path, so two stores compare equal whatever their object order.
pub fn canonical(s: &Store) -> Vec<(String, Auid, BTreeMap<u16, Canon>)> {
    let path = |id: &ObjId| s.objects[id.0 as usize].path.clone();
    let mut out: Vec<_> = s
        .objects
        .iter()
        .map(|o: &Obj| {
            let props = o
                .props
                .iter()
                .map(|(k, p)| {
                    let c = match p {
                        Prop::Data(d) => Canon::Data(d.clone()),
                        Prop::Strong(id) => Canon::Strong(0, vec![path(id)]),
                        Prop::StrongVec(v) => Canon::Strong(1, v.iter().map(path).collect()),
                        Prop::StrongSet(v) => Canon::Strong(2, v.iter().map(path).collect()),
                        Prop::Weak(w) => Canon::Weak(0, vec![w.clone()]),
                        Prop::WeakVec(v) => Canon::Weak(1, v.clone()),
                        Prop::WeakSet(v) => Canon::Weak(2, v.clone()),
                        Prop::Stream(n) => Canon::Stream(n.clone()),
                    };
                    (*k, c)
                })
                .collect();
            (o.path.clone(), o.class, props)
        })
        .collect();
    out.sort_by(|a, b| a.0.cmp(&b.0));
    out
}

/// One `properties` stream: little-endian header, then `entries` of
/// (pid, stored form, value).
pub fn props(entries: &[(u16, u16, Vec<u8>)]) -> Vec<u8> {
    let mut out = vec![0x4C, 0x20];
    out.extend_from_slice(&(entries.len() as u16).to_le_bytes());
    for (pid, form, v) in entries {
        out.extend_from_slice(&pid.to_le_bytes());
        out.extend_from_slice(&form.to_le_bytes());
        out.extend_from_slice(&(v.len() as u16).to_le_bytes());
    }
    for (_, _, v) in entries {
        out.extend_from_slice(v);
    }
    out
}

/// A raw compound file at `path`: each `(storage path, None)` makes a
/// storage, each `(stream path, Some(bytes))` a stream. Parents come first.
pub fn raw_file(path: &Path, items: &[(&str, Option<Vec<u8>>)]) {
    let f = std::fs::File::create(path).unwrap();
    let mut c = cfb::CompoundFile::create_with_version(cfb::Version::V4, f).unwrap();
    for (p, bytes) in items {
        match bytes {
            None => c.create_storage(p).unwrap(),
            Some(b) => c.create_stream(p).unwrap().write_all(b).unwrap(),
        }
    }
    c.flush().unwrap();
}

// ---------------------------------------------------------------------
// Composition builders (plan 6f Task 2): typed from Rulings 1 and 4 and the
// Object Specification, so every fixture is made here and nothing
// third-party is committed.

/// `DataDef_Picture`, for a slot that must be left out.
pub const PICTURE: Auid = def::DATA_DEF_PICTURE;

/// An MXF generic-container label (`ContainerDef_MXFGC_…`).
pub const MXF_CONTAINER: Auid = Auid::from_fields(
    0x0d010301,
    0x0206,
    0x0100,
    [0x06, 0x0e, 0x2b, 0x34, 0x04, 0x01, 0x01, 0x01],
);

/// Where a file mob's audio is.
#[derive(Debug, Clone)]
pub enum Media {
    /// Embedded: a whole WAV file as the essence.
    Wav(Vec<u8>),
    /// Embedded: raw interleaved PCM (a `PCMDescriptor`).
    Pcm(Vec<u8>),
    /// Embedded: a whole AIFC file.
    Aifc(Vec<u8>),
    /// External: a `NetworkLocator` with this URL.
    Url(String),
}

/// One audio file: a file `SourceMob` and the `MasterMob` that plays it,
/// one slot per channel.
#[derive(Debug, Clone)]
pub struct FileSpec {
    /// The master mob's name.
    pub name: String,
    /// The descriptor's rate.
    pub rate: u32,
    /// Channels.
    pub channels: u16,
    /// Bits per sample.
    pub bits: u16,
    /// The descriptor's `Length`, in frames.
    pub frames: i64,
    /// Where it is.
    pub media: Media,
    /// The descriptor's `ContainerFormat` (AAF or External when `None`).
    pub container: Option<Auid>,
    /// The file slots' `Origin`.
    pub origin: i64,
}

impl FileSpec {
    /// An embedded 16-bit WAV of `frames` frames of silence.
    pub fn wav(name: &str, rate: u32, channels: u16, frames: i64) -> FileSpec {
        FileSpec {
            name: name.into(),
            rate,
            channels,
            bits: 16,
            frames,
            media: Media::Wav(wav_bytes(rate, channels, frames as u32)),
            container: None,
            origin: 0,
        }
    }

    /// An external file at `url`.
    pub fn url(name: &str, rate: u32, channels: u16, frames: i64, url: &str) -> FileSpec {
        FileSpec {
            name: name.into(),
            rate,
            channels,
            bits: 16,
            frames,
            media: Media::Url(url.into()),
            container: None,
            origin: 0,
        }
    }
}

/// A 16-bit PCM WAV file of `frames` frames of silence.
pub fn wav_bytes(rate: u32, channels: u16, frames: u32) -> Vec<u8> {
    let block = u32::from(channels) * 2;
    let data = frames * block;
    let mut v = Vec::new();
    v.extend_from_slice(b"RIFF");
    v.extend_from_slice(&(36 + data).to_le_bytes());
    v.extend_from_slice(b"WAVEfmt ");
    v.extend_from_slice(&16u32.to_le_bytes());
    v.extend_from_slice(&1u16.to_le_bytes());
    v.extend_from_slice(&channels.to_le_bytes());
    v.extend_from_slice(&rate.to_le_bytes());
    v.extend_from_slice(&(rate * block).to_le_bytes());
    v.extend_from_slice(&(block as u16).to_le_bytes());
    v.extend_from_slice(&16u16.to_le_bytes());
    v.extend_from_slice(b"data");
    v.extend_from_slice(&data.to_le_bytes());
    v.resize(44 + data as usize, 0);
    v
}

/// `p` as a `file://` URL, spaces percent-encoded.
pub fn file_url(p: &Path) -> String {
    format!("file://{}", p.display().to_string().replace(' ', "%20"))
}

/// A parameter's value.
#[derive(Debug, Clone)]
pub enum Value {
    /// A `ConstantValue` holding a `Rational`.
    Const((i32, i32)),
    /// A `VaryingValue`: (time as a fraction of the group, value), and its
    /// interpolation.
    Points(Vec<((i32, i32), (i32, i32))>, Auid),
}

/// One component of a slot, as written.
#[derive(Debug, Clone)]
pub enum Seg {
    /// A `Filler`.
    Fill(i64),
    /// A `SourceClip`.
    Clip(ClipSpec),
    /// A `Transition` of `MonoAudioDissolve` with this interpolation.
    Trans {
        /// Its length.
        len: i64,
        /// Its `CutPoint`, if written.
        cut: Option<i64>,
        /// Its `Level` curve's interpolation.
        interp: Auid,
    },
    /// A `Sequence`.
    Seq(Vec<Seg>),
    /// An `OperationGroup` of `op` with one parameter `param`.
    Op {
        /// The operation.
        op: Auid,
        /// The parameter's definition.
        param: Auid,
        /// Its value.
        value: Value,
        /// The input segment.
        input: Box<Seg>,
    },
    /// A `Selector`: the selected segment and the alternates.
    Select(Box<Seg>, Vec<Seg>),
}

/// A `SourceClip`.
#[derive(Debug, Clone)]
pub struct ClipSpec {
    /// The mob it plays.
    pub mob: Vec<u8>,
    /// That mob's slot.
    pub slot: u32,
    /// `StartTime`, in this slot's edit units.
    pub start: i64,
    /// `Length`.
    pub len: i64,
    /// `FadeInLength` and `FadeInType`.
    pub fade_in: Option<(i64, u8)>,
    /// `FadeOutLength` and `FadeOutType`.
    pub fade_out: Option<(i64, u8)>,
}

/// A `SourceClip` of `mob`'s `slot`.
pub fn clip(mob: &[u8], slot: u32, start: i64, len: i64) -> Seg {
    Seg::Clip(ClipSpec {
        mob: mob.to_vec(),
        slot,
        start,
        len,
        fade_in: None,
        fade_out: None,
    })
}

/// A transition of `len` with a linear `Level` and no `CutPoint` unless
/// `cut`.
pub fn trans(len: i64, cut: Option<i64>) -> Seg {
    Seg::Trans {
        len,
        cut,
        interp: def::INTERPOLATION_DEF_LINEAR,
    }
}

/// `input` inside a constant `MonoAudioGain` of `num/den`.
pub fn gain(num: i32, den: i32, input: Seg) -> Seg {
    Seg::Op {
        op: def::OPERATION_DEF_MONO_AUDIO_GAIN,
        param: def::PARAMETER_DEF_AMPLITUDE,
        value: Value::Const((num, den)),
        input: Box::new(input),
    }
}

impl Seg {
    /// Its length, as a `Sequence` places it (Ruling 4).
    pub fn len(&self) -> i64 {
        match self {
            Seg::Fill(l) | Seg::Trans { len: l, .. } => *l,
            Seg::Clip(c) => c.len,
            Seg::Seq(v) => v.iter().fold(0, |acc, s| match s {
                Seg::Trans { len, .. } => acc - len,
                s => acc + s.len(),
            }),
            Seg::Op { input, .. } => input.len(),
            Seg::Select(s, _) => s.len(),
        }
    }
}

/// One mob slot.
#[derive(Debug, Clone)]
pub enum SlotKind {
    /// A `TimelineMobSlot` of sound.
    Sound(Seg),
    /// A `TimelineMobSlot` of this data kind.
    Data(Auid, Seg),
    /// A timecode slot: `Start`, `FPS`, `Drop`.
    Timecode(i64, u16, bool),
    /// An `EventMobSlot` of `DescriptiveMarker`s: (position, comment).
    Markers(Vec<(i64, String)>),
}

/// A mob slot: its ID, name, edit rate and contents.
#[derive(Debug, Clone)]
pub struct SlotSpec {
    /// `SlotID`.
    pub id: u32,
    /// `SlotName`.
    pub name: String,
    /// `EditRate`.
    pub rate: (i32, i32),
    /// Contents.
    pub kind: SlotKind,
}

/// A sound slot.
pub fn sound(id: u32, name: &str, rate: (i32, i32), seg: Seg) -> SlotSpec {
    SlotSpec {
        id,
        name: name.into(),
        rate,
        kind: SlotKind::Sound(seg),
    }
}

/// A file being built: a header with its content storage, and the
/// essence streams to write.
pub struct Fx {
    /// The store.
    pub s: Store,
    header: ObjId,
    content: ObjId,
    mobs: Vec<ObjId>,
    essence: Vec<ObjId>,
    idents: Vec<ObjId>,
    streams: Vec<(ObjId, Vec<u8>)>,
    next: u8,
}

fn weak(id: Auid) -> Prop {
    Prop::Weak(WeakRef {
        table_index: 0,
        id_pid: pid::DEFINITION_OBJECT_IDENTIFICATION,
        id: id.0.to_vec(),
    })
}

fn rat(n: i32, d: i32) -> Vec<u8> {
    let mut v = n.to_le_bytes().to_vec();
    v.extend_from_slice(&d.to_le_bytes());
    v
}

/// An indirect value: byte order, a type AUID (zeros here), the value.
fn indirect(v: &[u8]) -> Vec<u8> {
    let mut out = vec![0x4C];
    out.extend_from_slice(&[0u8; 16]);
    out.extend_from_slice(v);
    out
}

impl Default for Fx {
    fn default() -> Self {
        Fx::new()
    }
}

impl Fx {
    /// An empty file whose application is "Fixture".
    pub fn new() -> Fx {
        let mut s = Store::new(class::ROOT);
        let root = s.root;
        let header = s.add_child(root, pid::ROOT_HEADER, None, class::HEADER);
        set(&mut s, root, pid::ROOT_HEADER, Prop::Strong(header));
        set(
            &mut s,
            header,
            pid::HEADER_BYTE_ORDER,
            Prop::Data(vec![0x49, 0x49]),
        );
        let content = s.add_child(header, pid::HEADER_CONTENT, None, class::CONTENT_STORAGE);
        set(&mut s, header, pid::HEADER_CONTENT, Prop::Strong(content));
        s.referenced = vec![vec![
            pid::ROOT_HEADER,
            pid::HEADER_DICTIONARY,
            pid::DICTIONARY_DATA_DEFINITIONS,
        ]];
        let mut fx = Fx {
            s,
            header,
            content,
            mobs: Vec::new(),
            essence: Vec::new(),
            idents: Vec::new(),
            streams: Vec::new(),
            next: 1,
        };
        fx.product("Fixture");
        fx
    }

    /// Adds an `Identification` naming `product`.
    pub fn product(&mut self, product: &str) {
        let i = self.idents.len() as u32;
        let id = self.s.add_child(
            self.header,
            pid::HEADER_IDENTIFICATION_LIST,
            Some(i),
            class::IDENTIFICATION,
        );
        set(
            &mut self.s,
            id,
            pid::IDENTIFICATION_PRODUCT_NAME,
            Prop::Data(utf16z(product)),
        );
        set(
            &mut self.s,
            id,
            pid::IDENTIFICATION_COMPANY_NAME,
            Prop::Data(utf16z(product)),
        );
        self.idents.push(id);
        let h = self.header;
        set(
            &mut self.s,
            h,
            pid::HEADER_IDENTIFICATION_LIST,
            Prop::StrongVec(self.idents.clone()),
        );
    }

    /// A fresh MobID.
    pub fn new_id(&mut self) -> Vec<u8> {
        self.next += 1;
        mob_id(self.next)
    }

    fn add_mob(&mut self, cls: Auid) -> ObjId {
        let i = self.mobs.len() as u32;
        let m = self
            .s
            .add_child(self.content, pid::CONTENT_STORAGE_MOBS, Some(i), cls);
        self.mobs.push(m);
        let c = self.content;
        set(
            &mut self.s,
            c,
            pid::CONTENT_STORAGE_MOBS,
            Prop::StrongSet(self.mobs.clone()),
        );
        m
    }

    /// A mob of `cls` with `id`, `name` and `slots`; `top` sets
    /// `Usage_TopLevel`.
    pub fn mob(
        &mut self,
        cls: Auid,
        id: &[u8],
        name: &str,
        top: bool,
        slots: &[SlotSpec],
    ) -> ObjId {
        let m = self.add_mob(cls);
        set(&mut self.s, m, pid::MOB_MOB_ID, Prop::Data(id.to_vec()));
        set(&mut self.s, m, pid::MOB_NAME, Prop::Data(utf16z(name)));
        if top {
            set(
                &mut self.s,
                m,
                pid::MOB_USAGE_CODE,
                Prop::Data(def::USAGE_TOP_LEVEL.0.to_vec()),
            );
        }
        let mut ids = Vec::new();
        for (i, sl) in slots.iter().enumerate() {
            let (cls, rate_pid) = match sl.kind {
                SlotKind::Markers(_) => (class::EVENT_MOB_SLOT, pid::EVENT_MOB_SLOT_EDIT_RATE),
                _ => (class::TIMELINE_MOB_SLOT, pid::TIMELINE_MOB_SLOT_EDIT_RATE),
            };
            let o = self.s.add_child(m, pid::MOB_SLOTS, Some(i as u32), cls);
            set(
                &mut self.s,
                o,
                pid::MOB_SLOT_SLOT_ID,
                Prop::Data(sl.id.to_le_bytes().to_vec()),
            );
            set(
                &mut self.s,
                o,
                pid::MOB_SLOT_SLOT_NAME,
                Prop::Data(utf16z(&sl.name)),
            );
            set(
                &mut self.s,
                o,
                rate_pid,
                Prop::Data(rat(sl.rate.0, sl.rate.1)),
            );
            if cls == class::TIMELINE_MOB_SLOT {
                set(
                    &mut self.s,
                    o,
                    pid::TIMELINE_MOB_SLOT_ORIGIN,
                    Prop::Data(0i64.to_le_bytes().to_vec()),
                );
            }
            match &sl.kind {
                SlotKind::Sound(seg) => {
                    self.seg(o, pid::MOB_SLOT_SEGMENT, None, seg, def::DATA_DEF_SOUND);
                }
                SlotKind::Data(dd, seg) => {
                    self.seg(o, pid::MOB_SLOT_SEGMENT, None, seg, *dd);
                }
                SlotKind::Timecode(start, fps, drop) => {
                    let t = self
                        .s
                        .add_child(o, pid::MOB_SLOT_SEGMENT, None, class::TIMECODE);
                    set(&mut self.s, o, pid::MOB_SLOT_SEGMENT, Prop::Strong(t));
                    self.component(t, 1_000_000, def::DATA_DEF_TIMECODE);
                    set(
                        &mut self.s,
                        t,
                        pid::TIMECODE_START,
                        Prop::Data(start.to_le_bytes().to_vec()),
                    );
                    set(
                        &mut self.s,
                        t,
                        pid::TIMECODE_FPS,
                        Prop::Data(fps.to_le_bytes().to_vec()),
                    );
                    set(
                        &mut self.s,
                        t,
                        pid::TIMECODE_DROP,
                        Prop::Data(vec![u8::from(*drop)]),
                    );
                }
                SlotKind::Markers(ms) => {
                    let q = self
                        .s
                        .add_child(o, pid::MOB_SLOT_SEGMENT, None, class::SEQUENCE);
                    set(&mut self.s, o, pid::MOB_SLOT_SEGMENT, Prop::Strong(q));
                    self.component(q, 0, def::DATA_DEF_DESCRIPTIVE_METADATA);
                    let mut evs = Vec::new();
                    for (k, (pos, text)) in ms.iter().enumerate() {
                        let e = self.s.add_child(
                            q,
                            pid::SEQUENCE_COMPONENTS,
                            Some(k as u32),
                            class::DESCRIPTIVE_MARKER,
                        );
                        self.component(e, 0, def::DATA_DEF_DESCRIPTIVE_METADATA);
                        set(
                            &mut self.s,
                            e,
                            pid::EVENT_POSITION,
                            Prop::Data(pos.to_le_bytes().to_vec()),
                        );
                        set(&mut self.s, e, pid::EVENT_COMMENT, Prop::Data(utf16z(text)));
                        evs.push(e);
                    }
                    set(
                        &mut self.s,
                        q,
                        pid::SEQUENCE_COMPONENTS,
                        Prop::StrongVec(evs),
                    );
                }
            }
            ids.push(o);
        }
        set(&mut self.s, m, pid::MOB_SLOTS, Prop::StrongVec(ids));
        m
    }

    /// A `CompositionMob`; its MobID.
    pub fn comp(&mut self, name: &str, top: bool, slots: &[SlotSpec]) -> Vec<u8> {
        let id = self.new_id();
        self.mob(class::COMPOSITION_MOB, &id, name, top, slots);
        id
    }

    fn component(&mut self, o: ObjId, len: i64, dd: Auid) {
        set(
            &mut self.s,
            o,
            pid::COMPONENT_LENGTH,
            Prop::Data(len.to_le_bytes().to_vec()),
        );
        set(&mut self.s, o, pid::COMPONENT_DATA_DEFINITION, weak(dd));
    }

    /// Writes `seg` as `parent`'s strong reference `pid` (element `index`).
    fn seg(&mut self, parent: ObjId, p: u16, index: Option<u32>, seg: &Seg, dd: Auid) -> ObjId {
        let cls = match seg {
            Seg::Fill(_) => class::FILLER,
            Seg::Clip(_) => class::SOURCE_CLIP,
            Seg::Trans { .. } => class::TRANSITION,
            Seg::Seq(_) => class::SEQUENCE,
            Seg::Op { .. } => class::OPERATION_GROUP,
            Seg::Select(..) => class::SELECTOR,
        };
        let o = self.s.add_child(parent, p, index, cls);
        if index.is_none() {
            set(&mut self.s, parent, p, Prop::Strong(o));
        }
        self.component(o, seg.len(), dd);
        match seg {
            Seg::Fill(_) => {}
            Seg::Clip(c) => {
                set(
                    &mut self.s,
                    o,
                    pid::SOURCE_REFERENCE_SOURCE_ID,
                    Prop::Data(c.mob.clone()),
                );
                set(
                    &mut self.s,
                    o,
                    pid::SOURCE_REFERENCE_SOURCE_MOB_SLOT_ID,
                    Prop::Data(c.slot.to_le_bytes().to_vec()),
                );
                set(
                    &mut self.s,
                    o,
                    pid::SOURCE_CLIP_START_TIME,
                    Prop::Data(c.start.to_le_bytes().to_vec()),
                );
                if let Some((l, t)) = c.fade_in {
                    set(
                        &mut self.s,
                        o,
                        pid::SOURCE_CLIP_FADE_IN_LENGTH,
                        Prop::Data(l.to_le_bytes().to_vec()),
                    );
                    set(
                        &mut self.s,
                        o,
                        pid::SOURCE_CLIP_FADE_IN_TYPE,
                        Prop::Data(vec![t]),
                    );
                }
                if let Some((l, t)) = c.fade_out {
                    set(
                        &mut self.s,
                        o,
                        pid::SOURCE_CLIP_FADE_OUT_LENGTH,
                        Prop::Data(l.to_le_bytes().to_vec()),
                    );
                    set(
                        &mut self.s,
                        o,
                        pid::SOURCE_CLIP_FADE_OUT_TYPE,
                        Prop::Data(vec![t]),
                    );
                }
            }
            Seg::Trans { len, cut, interp } => {
                if let Some(c) = cut {
                    set(
                        &mut self.s,
                        o,
                        pid::TRANSITION_CUT_POINT,
                        Prop::Data(c.to_le_bytes().to_vec()),
                    );
                }
                let g = self.s.add_child(
                    o,
                    pid::TRANSITION_OPERATION_GROUP,
                    None,
                    class::OPERATION_GROUP,
                );
                set(
                    &mut self.s,
                    o,
                    pid::TRANSITION_OPERATION_GROUP,
                    Prop::Strong(g),
                );
                self.component(g, *len, dd);
                self.op(
                    g,
                    def::OPERATION_DEF_MONO_AUDIO_DISSOLVE,
                    def::PARAMETER_DEF_LEVEL,
                    &Value::Points(vec![((0, 1), (0, 1)), ((1, 1), (1, 1))], *interp),
                );
            }
            Seg::Seq(v) => {
                let kids: Vec<ObjId> = v
                    .iter()
                    .enumerate()
                    .map(|(i, s)| self.seg(o, pid::SEQUENCE_COMPONENTS, Some(i as u32), s, dd))
                    .collect();
                set(
                    &mut self.s,
                    o,
                    pid::SEQUENCE_COMPONENTS,
                    Prop::StrongVec(kids),
                );
            }
            Seg::Op {
                op,
                param,
                value,
                input,
            } => {
                self.op(o, *op, *param, value);
                let i = self.seg(o, pid::OPERATION_GROUP_INPUT_SEGMENTS, Some(0), input, dd);
                set(
                    &mut self.s,
                    o,
                    pid::OPERATION_GROUP_INPUT_SEGMENTS,
                    Prop::StrongVec(vec![i]),
                );
            }
            Seg::Select(sel, alts) => {
                self.seg(o, pid::SELECTOR_SELECTED, None, sel, dd);
                let kids: Vec<ObjId> = alts
                    .iter()
                    .enumerate()
                    .map(|(i, s)| self.seg(o, pid::SELECTOR_ALTERNATES, Some(i as u32), s, dd))
                    .collect();
                set(
                    &mut self.s,
                    o,
                    pid::SELECTOR_ALTERNATES,
                    Prop::StrongVec(kids),
                );
            }
        }
        o
    }

    /// `g`'s operation and its one parameter.
    fn op(&mut self, g: ObjId, op: Auid, param: Auid, value: &Value) {
        set(&mut self.s, g, pid::OPERATION_GROUP_OPERATION, weak(op));
        let cls = match value {
            Value::Const(_) => class::CONSTANT_VALUE,
            Value::Points(..) => class::VARYING_VALUE,
        };
        let p = self
            .s
            .add_child(g, pid::OPERATION_GROUP_PARAMETERS, Some(0), cls);
        set(
            &mut self.s,
            g,
            pid::OPERATION_GROUP_PARAMETERS,
            Prop::StrongVec(vec![p]),
        );
        set(
            &mut self.s,
            p,
            pid::PARAMETER_DEFINITION,
            Prop::Data(param.0.to_vec()),
        );
        match value {
            Value::Const((n, d)) => set(
                &mut self.s,
                p,
                pid::CONSTANT_VALUE_VALUE,
                Prop::Data(indirect(&rat(*n, *d))),
            ),
            Value::Points(pts, interp) => {
                set(
                    &mut self.s,
                    p,
                    pid::VARYING_VALUE_INTERPOLATION,
                    weak(*interp),
                );
                let mut cps = Vec::new();
                for (i, (t, v)) in pts.iter().enumerate() {
                    let c = self.s.add_child(
                        p,
                        pid::VARYING_VALUE_POINT_LIST,
                        Some(i as u32),
                        class::CONTROL_POINT,
                    );
                    set(
                        &mut self.s,
                        c,
                        pid::CONTROL_POINT_TIME,
                        Prop::Data(rat(t.0, t.1)),
                    );
                    set(
                        &mut self.s,
                        c,
                        pid::CONTROL_POINT_VALUE,
                        Prop::Data(indirect(&rat(v.0, v.1))),
                    );
                    cps.push(c);
                }
                set(
                    &mut self.s,
                    p,
                    pid::VARYING_VALUE_POINT_LIST,
                    Prop::StrongVec(cps),
                );
            }
        }
    }

    /// An audio file: its file `SourceMob` (with its descriptor and, when
    /// embedded, its `EssenceData`) and its `MasterMob`, both with one
    /// slot per channel. Returns the master mob's MobID.
    pub fn file(&mut self, f: &FileSpec) -> Vec<u8> {
        self.file_ids(f).0
    }

    /// [`Fx::file`], returning the master mob's and the file mob's IDs.
    pub fn file_ids(&mut self, f: &FileSpec) -> (Vec<u8>, Vec<u8>) {
        let src_id = self.new_id();
        let rate = (f.rate as i32, 1);
        let src_slots: Vec<SlotSpec> = (1..=u32::from(f.channels))
            .map(|k| sound(k, "", rate, Seg::Fill(f.frames)))
            .collect();
        let src = self.mob(class::SOURCE_MOB, &src_id, &f.name, false, &src_slots);
        if f.origin != 0 {
            for sl in self.s.objects[src.0 as usize]
                .props
                .get(&pid::MOB_SLOTS)
                .map(|p| match p {
                    Prop::StrongVec(v) => v.clone(),
                    _ => Vec::new(),
                })
                .unwrap_or_default()
            {
                set(
                    &mut self.s,
                    sl,
                    pid::TIMELINE_MOB_SLOT_ORIGIN,
                    Prop::Data(f.origin.to_le_bytes().to_vec()),
                );
            }
        }
        let (cls, embedded) = match &f.media {
            Media::Wav(b) => (class::WAVE_DESCRIPTOR, Some(b.clone())),
            Media::Aifc(b) => (class::AIFC_DESCRIPTOR, Some(b.clone())),
            Media::Pcm(b) => (class::PCM_DESCRIPTOR, Some(b.clone())),
            Media::Url(_) => (class::WAVE_DESCRIPTOR, None),
        };
        let d = self
            .s
            .add_child(src, pid::SOURCE_MOB_ESSENCE_DESCRIPTION, None, cls);
        set(
            &mut self.s,
            src,
            pid::SOURCE_MOB_ESSENCE_DESCRIPTION,
            Prop::Strong(d),
        );
        set(
            &mut self.s,
            d,
            pid::FILE_DESCRIPTOR_SAMPLE_RATE,
            Prop::Data(rat(rate.0, 1)),
        );
        set(
            &mut self.s,
            d,
            pid::FILE_DESCRIPTOR_LENGTH,
            Prop::Data(f.frames.to_le_bytes().to_vec()),
        );
        let container = f.container.unwrap_or(if embedded.is_some() {
            def::CONTAINER_DEF_AAF
        } else {
            def::CONTAINER_DEF_EXTERNAL
        });
        set(
            &mut self.s,
            d,
            pid::FILE_DESCRIPTOR_CONTAINER_FORMAT,
            weak(container),
        );
        match &f.media {
            Media::Pcm(_) => {
                set(
                    &mut self.s,
                    d,
                    pid::SOUND_DESCRIPTOR_CHANNELS,
                    Prop::Data(u32::from(f.channels).to_le_bytes().to_vec()),
                );
                set(
                    &mut self.s,
                    d,
                    pid::SOUND_DESCRIPTOR_QUANTIZATION_BITS,
                    Prop::Data(u32::from(f.bits).to_le_bytes().to_vec()),
                );
                set(
                    &mut self.s,
                    d,
                    pid::SOUND_DESCRIPTOR_AUDIO_SAMPLING_RATE,
                    Prop::Data(rat(rate.0, 1)),
                );
            }
            Media::Aifc(_) => {}
            _ => {
                let summary = wav_bytes(f.rate, f.channels, 0);
                set(
                    &mut self.s,
                    d,
                    pid::WAVE_DESCRIPTOR_SUMMARY,
                    Prop::Data(summary),
                );
            }
        }
        if let Media::Url(u) = &f.media {
            let l = self.s.add_child(
                d,
                pid::ESSENCE_DESCRIPTOR_LOCATOR,
                Some(0),
                class::NETWORK_LOCATOR,
            );
            set(
                &mut self.s,
                l,
                pid::NETWORK_LOCATOR_URL_STRING,
                Prop::Data(utf16z(u)),
            );
            set(
                &mut self.s,
                d,
                pid::ESSENCE_DESCRIPTOR_LOCATOR,
                Prop::StrongVec(vec![l]),
            );
        }
        if let Some(bytes) = embedded {
            let i = self.essence.len() as u32;
            let e = self.s.add_child(
                self.content,
                pid::CONTENT_STORAGE_ESSENCE_DATA,
                Some(i),
                class::ESSENCE_DATA,
            );
            set(
                &mut self.s,
                e,
                pid::ESSENCE_DATA_MOB_ID,
                Prop::Data(src_id.clone()),
            );
            set(
                &mut self.s,
                e,
                pid::ESSENCE_DATA_DATA,
                Prop::Stream("Data-2702".into()),
            );
            self.essence.push(e);
            let c = self.content;
            set(
                &mut self.s,
                c,
                pid::CONTENT_STORAGE_ESSENCE_DATA,
                Prop::StrongSet(self.essence.clone()),
            );
            self.streams.push((e, bytes));
        }
        let master_id = self.new_id();
        let slots: Vec<SlotSpec> = (1..=u32::from(f.channels))
            .map(|k| sound(k, "", rate, clip(&src_id, k, 0, f.frames)))
            .collect();
        self.mob(class::MASTER_MOB, &master_id, &f.name, false, &slots);
        (master_id, src_id)
    }

    /// Writes the file to `path`, the essence streamed in.
    pub fn write(&self, path: &Path) {
        let f = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create_new(true)
            .open(path)
            .unwrap();
        let mut w = core_io::aaf::store::AafWriter::create(f).unwrap();
        w.write_store(&self.s).unwrap();
        for (obj, bytes) in &self.streams {
            let mut st = w.stream(&self.s, *obj, "Data-2702").unwrap();
            st.write_all(bytes).unwrap();
            st.flush().unwrap();
        }
        w.finish().unwrap();
    }
}
