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

/// A `Rational` as stored: numerator, then denominator.
pub fn rat(n: i32, d: i32) -> Vec<u8> {
    let mut v = n.to_le_bytes().to_vec();
    v.extend_from_slice(&d.to_le_bytes());
    v
}

/// An indirect value: byte order, a type AUID (zeros here), the value.
pub fn indirect(v: &[u8]) -> Vec<u8> {
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

// ---------------------------------------------------------------- exports (Task 4)

/// A PCM WAV file of `frames` frames of silence at `bits` (16 or 24).
pub fn wav_pcm(rate: u32, channels: u16, frames: u64, bits: u16) -> Vec<u8> {
    let block = u32::from(channels) * u32::from(bits / 8);
    let data = u32::try_from(frames).unwrap() * block;
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
    v.extend_from_slice(&bits.to_le_bytes());
    v.extend_from_slice(b"data");
    v.extend_from_slice(&data.to_le_bytes());
    v.resize(44 + data as usize, 0);
    v
}

/// Writes export `x` of `doc` as `dir/Song.aaf`, each staged file's essence
/// a silent WAV of its media item at `bits`; returns the path.
pub fn write_export(
    doc: &core_model::document::Document,
    x: &core_io::aaf::export::AafExport,
    dir: &Path,
    bits: u16,
) -> std::path::PathBuf {
    let p = dir.join("Song.aaf");
    let staged = x.audio.clone();
    let mut essence = |i: usize, w: &mut dyn Write| -> Result<(), core_io::IoError> {
        let s = &staged[i];
        let m = doc.media().get(&s.media).unwrap();
        let ch = if s.channel.is_some() { 1 } else { m.channels };
        w.write_all(&wav_pcm(m.sample_rate, ch, m.length_frames, bits))
            .map_err(|e| core_io::IoError::Io(e.to_string()))
    };
    core_io::aaf::export::write(
        x,
        &p,
        &mut essence,
        &std::sync::atomic::AtomicBool::new(false),
    )
    .expect("writes");
    p
}

/// One region as the round trip compares it.
#[derive(Debug, Clone, PartialEq)]
pub struct RegionP {
    pub tick: i64,
    pub source_offset: u64,
    /// Its audible length, in frames: up to where a later region takes
    /// over (plus that region's crossfade).
    pub length: u64,
    pub fade_in: (u64, core_model::event::FadeShape),
    pub fade_out: (u64, core_model::event::FadeShape),
    pub gain_db: f32,
    pub name: Option<String>,
    pub crossfade: bool,
}

/// One audio track as the round trip compares it.
#[derive(Debug, Clone, PartialEq)]
pub struct TrackP {
    pub name: String,
    pub channels: u16,
    pub regions: Vec<RegionP>,
    /// The strip's fader in dB, when it has no fader automation.
    pub fader: Option<f64>,
    /// The strip's pan, when it has no pan automation.
    pub pan: Option<f64>,
    pub fader_points: Vec<(i64, f64)>,
    pub pan_points: Vec<(i64, f64)>,
}

/// What the AAF round trip keeps (spec §12): per audio track, its
/// channels, regions, fader, pan and their points; the markers.
#[derive(Debug, Clone, PartialEq)]
pub struct Projection {
    pub tracks: Vec<TrackP>,
    pub markers: Vec<(i64, String)>,
}

/// A fade as Ruling 5 brings it back: S-curve becomes linear; no length,
/// no shape.
fn shape_back(f: &core_model::event::Fade) -> (u64, core_model::event::FadeShape) {
    use core_model::event::FadeShape;
    let s = match f.shape {
        _ if f.length_frames == 0 => FadeShape::Linear,
        FadeShape::SCurve => FadeShape::Linear,
        s => s,
    };
    (f.length_frames, s)
}

/// The projection of sequence `seq` of `doc`: every track with an audio
/// output, in order; its unmuted regions with their audible lengths (a
/// region a later one starts inside plays up to that start, plus the later
/// one's crossfade, and loses its fade-out); its strip's fader and pan.
pub fn projection(doc: &core_model::document::Document, seq: core_model::ids::SeqId) -> Projection {
    let end = core_model::query::seq_length(doc, seq).unwrap().0;
    projection_until(doc, seq, end)
}

/// [`projection`] with the automation points before `end` (events at or
/// past a sequence's end do not play).
pub fn projection_until(
    doc: &core_model::document::Document,
    seq: core_model::ids::SeqId,
    end: i64,
) -> Projection {
    use core_model::automation::AutomationTarget;
    use core_model::event::{EventBody, Fade};
    use core_model::mixer::StripKind;
    let s = doc.sequence(seq).unwrap();
    let clock = s.tempo.clock();
    let mut tracks = Vec::new();
    for lane in s.lanes.iter().filter(|l| l.audio_out.is_some()) {
        let strip = doc
            .mixer()
            .find_kind(StripKind::AudioPath(lane.audio_out.unwrap()))
            .and_then(|id| doc.mixer().strip(id).ok())
            .unwrap();
        let events: Vec<_> = lane
            .active()
            .map(|t| t.events.values().cloned().collect())
            .unwrap_or_default();
        let mut channels = 1;
        let mut regions: Vec<(RegionP, u32, Fade)> = Vec::new();
        let (mut fader_points, mut pan_points) = (Vec::new(), Vec::new());
        for e in &events {
            match &e.body {
                EventBody::AudioRegion {
                    media,
                    source_offset,
                    length,
                    gain_db,
                    fade_in,
                    fade_out,
                    name,
                    crossfade,
                } if !e.mute => {
                    let m = doc.media().get(media).unwrap();
                    channels = channels.max(m.channels);
                    regions.push((
                        RegionP {
                            tick: e.tick.0,
                            source_offset: *source_offset,
                            length: *length,
                            fade_in: shape_back(fade_in),
                            fade_out: shape_back(fade_out),
                            gain_db: *gain_db,
                            name: name.clone(),
                            crossfade: *crossfade,
                        },
                        m.sample_rate,
                        *fade_out,
                    ));
                }
                EventBody::Automation { target, value } if e.tick.0 < end => match target {
                    AutomationTarget::StripFader(st) if *st == strip.id => {
                        fader_points.push((e.tick.0, *value))
                    }
                    AutomationTarget::StripPan(st) if *st == strip.id => {
                        pan_points.push((e.tick.0, *value))
                    }
                    _ => {}
                },
                _ => {}
            }
        }
        // Audible lengths: the later start takes over.
        for i in 0..regions.len() {
            let Some(next) = regions.get(i + 1).map(|r| r.0.clone()) else {
                continue;
            };
            let (r, rate, _) = &mut regions[i];
            let secs = clock.tick_to_seconds(core_model::time::Tick(next.tick))
                - clock.tick_to_seconds(core_model::time::Tick(r.tick));
            let mut until = (secs * f64::from(*rate)).round() as u64;
            if next.crossfade {
                until += next.fade_in.0;
            }
            if until < r.length {
                r.length = until;
                r.fade_out = (0, core_model::event::FadeShape::Linear);
            }
        }
        tracks.push(TrackP {
            name: lane.name.clone(),
            channels,
            regions: regions.into_iter().map(|r| r.0).collect(),
            fader: fader_points.is_empty().then_some(strip.gain_db),
            pan: pan_points.is_empty().then_some(strip.pan),
            fader_points,
            pan_points,
        });
    }
    Projection {
        tracks,
        markers: s
            .markers
            .iter()
            .map(|m| (m.tick.0, m.name.clone()))
            .collect(),
    }
}

/// Whether two projections agree: gains to 0.001 dB, fader and pan values
/// and points to 1e-9, the rest exactly. `Err` names the first difference.
pub fn agree(a: &Projection, b: &Projection) -> Result<(), String> {
    let close = |x: f64, y: f64, tol: f64| (x - y).abs() <= tol || (x.is_infinite() && x == y);
    if a.markers != b.markers {
        return Err(format!("markers {:?} != {:?}", a.markers, b.markers));
    }
    if a.tracks.len() != b.tracks.len() {
        return Err(format!(
            "tracks {:?} != {:?}",
            a.tracks.iter().map(|t| &t.name).collect::<Vec<_>>(),
            b.tracks.iter().map(|t| &t.name).collect::<Vec<_>>()
        ));
    }
    for (x, y) in a.tracks.iter().zip(&b.tracks) {
        let t = &x.name;
        if (&x.name, x.channels) != (&y.name, y.channels) {
            return Err(format!(
                "track {t}: {:?} != {:?}",
                (&x.name, x.channels),
                (&y.name, y.channels)
            ));
        }
        if x.regions.len() != y.regions.len() {
            return Err(format!(
                "track {t}: regions {:#?} != {:#?}",
                x.regions, y.regions
            ));
        }
        for (r, q) in x.regions.iter().zip(&y.regions) {
            let mut r2 = r.clone();
            r2.gain_db = q.gain_db;
            if r2 != *q || !close(f64::from(r.gain_db), f64::from(q.gain_db), 0.001) {
                return Err(format!("track {t}: region {r:#?} != {q:#?}"));
            }
        }
        for (u, v, what) in [(x.fader, y.fader, "fader"), (x.pan, y.pan, "pan")] {
            match (u, v) {
                (Some(u), Some(v)) if close(u, v, 1e-9) => {}
                (None, None) => {}
                _ => return Err(format!("track {t}: {what} {u:?} != {v:?}")),
            }
        }
        for (u, v, what) in [
            (&x.fader_points, &y.fader_points, "fader points"),
            (&x.pan_points, &y.pan_points, "pan points"),
        ] {
            let same = u.len() == v.len()
                && u.iter()
                    .zip(v)
                    .all(|(p, q)| p.0 == q.0 && close(p.1, q.1, 1e-9));
            if !same {
                return Err(format!("track {t}: {what} {u:?} != {v:?}"));
            }
        }
    }
    Ok(())
}

// ---------------------------------------------------------------- hostile files (Task 7)

impl Fx {
    /// The file's bytes, written in memory through `AafWriter`.
    pub fn bytes(&self) -> Vec<u8> {
        let cur = std::io::Cursor::new(Vec::new());
        let mut w = core_io::aaf::store::AafWriter::create(cur).unwrap();
        w.write_store(&self.s).unwrap();
        for (obj, bytes) in &self.streams {
            let mut st = w.stream(&self.s, *obj, "Data-2702").unwrap();
            st.write_all(bytes).unwrap();
            st.flush().unwrap();
        }
        w.finish().unwrap().into_inner()
    }

    /// The file's bytes through [`raw_write`], with none of `AafWriter`'s
    /// checks.
    pub fn raw_bytes(&self) -> Vec<u8> {
        raw_write(&self.s, &self.streams)
    }
}

fn uuid_of(a: Auid) -> uuid::Uuid {
    let (d1, d2, d3, d4) = a.fields();
    uuid::Uuid::from_fields(d1, d2, d3, &d4)
}

/// The last name of a storage path.
fn last_name(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or("")
}

/// The keys a set's index uses, as `AafWriter` looks for them.
const SET_KEYS: [u16; 4] = [0x4401, 0x2701, 0x1B01, 0x0005];

/// A compound file of `s` written without `AafWriter`'s checks: every
/// object at its own `path` (a path written twice keeps its first
/// storage), each strong reference named by its child's last path name,
/// indexes taking each element's local key from its name, weak indexes
/// sized by their first entry, and value lengths stored as `u16` whatever
/// they are. `streams` are each object's `Data-2702`.
pub fn raw_write(s: &Store, streams: &[(ObjId, Vec<u8>)]) -> Vec<u8> {
    let cur = std::io::Cursor::new(Vec::new());
    let mut c = cfb::CompoundFile::create_with_version(cfb::Version::V4, cur).unwrap();
    let path_of = |id: &ObjId| {
        s.objects
            .get(id.0 as usize)
            .map_or(String::new(), |o| o.path.clone())
    };
    for o in &s.objects {
        if o.path != "/" && c.create_storage(&o.path).is_err() {
            continue;
        }
        let _ = c.set_storage_clsid(&o.path, uuid_of(o.class));
        let mut entries: Vec<(u16, u16, Vec<u8>)> = Vec::new();
        for (&p, prop) in &o.props {
            let base = core_io::aaf::store::ref_name(p);
            let index_path = core_io::aaf::store::child_path(&o.path, &format!("{base} index"));
            let entry = match prop {
                Prop::Data(d) => (0x82, d.clone()),
                Prop::Strong(id) => (0x22, utf16z(last_name(&path_of(id)))),
                Prop::StrongVec(v) | Prop::StrongSet(v) => {
                    let set = matches!(prop, Prop::StrongSet(_));
                    let n = v.len() as u32;
                    let mut idx = Vec::new();
                    idx.extend_from_slice(&n.to_le_bytes());
                    idx.extend_from_slice(&n.to_le_bytes());
                    idx.extend_from_slice(&u32::MAX.to_le_bytes());
                    let key = set
                        .then(|| {
                            let first = v.first()?;
                            SET_KEYS.iter().find_map(|k| match s.prop(*first, *k) {
                                Some(Prop::Data(d)) => Some((*k, d.len())),
                                _ => None,
                            })
                        })
                        .flatten();
                    if set {
                        let (kp, size) = key.unwrap_or((0, 0));
                        idx.extend_from_slice(&kp.to_le_bytes());
                        idx.push(size as u8);
                    }
                    for (i, id) in v.iter().enumerate() {
                        let name = path_of(id);
                        let local = last_name(&name)
                            .strip_prefix(&format!("{base}{{"))
                            .and_then(|r| r.strip_suffix('}'))
                            .and_then(|k| u32::from_str_radix(k, 16).ok())
                            .unwrap_or(i as u32);
                        idx.extend_from_slice(&local.to_le_bytes());
                        if let Some((kp, size)) = key {
                            idx.extend_from_slice(&1u32.to_le_bytes());
                            let mut k = match s.prop(*id, kp) {
                                Some(Prop::Data(d)) => d.clone(),
                                _ => Vec::new(),
                            };
                            k.resize(size, 0);
                            idx.extend_from_slice(&k);
                        }
                    }
                    put(&mut c, &index_path, &idx);
                    (if set { 0x3A } else { 0x32 }, utf16z(&base))
                }
                Prop::Weak(w) => {
                    let mut b = w.table_index.to_le_bytes().to_vec();
                    b.extend_from_slice(&w.id_pid.to_le_bytes());
                    b.push(w.id.len() as u8);
                    b.extend_from_slice(&w.id);
                    (0x02, b)
                }
                Prop::WeakVec(v) | Prop::WeakSet(v) => {
                    let (ti, kp, size) = v
                        .first()
                        .map_or((0, 0, 0), |w| (w.table_index, w.id_pid, w.id.len()));
                    let mut idx = (v.len() as u32).to_le_bytes().to_vec();
                    idx.extend_from_slice(&ti.to_le_bytes());
                    idx.extend_from_slice(&kp.to_le_bytes());
                    idx.push(size as u8);
                    for w in v {
                        idx.extend_from_slice(&w.id);
                    }
                    put(&mut c, &index_path, &idx);
                    let f = if matches!(prop, Prop::WeakSet(_)) {
                        0x1A
                    } else {
                        0x12
                    };
                    (f, utf16z(&base))
                }
                Prop::Stream(n) => {
                    let sp = core_io::aaf::store::child_path(&o.path, n);
                    if !c.is_stream(&sp) {
                        put(&mut c, &sp, &[]);
                    }
                    let mut b = vec![0x55];
                    b.extend_from_slice(&utf16z(n));
                    (0x42, b)
                }
            };
            entries.push((p, entry.0, entry.1));
        }
        let mut bytes = vec![0x4C, 0x20];
        bytes.extend_from_slice(&(entries.len() as u16).to_le_bytes());
        for (p, f, v) in &entries {
            bytes.extend_from_slice(&p.to_le_bytes());
            bytes.extend_from_slice(&f.to_le_bytes());
            bytes.extend_from_slice(&(v.len() as u16).to_le_bytes());
        }
        for (_, _, v) in &entries {
            bytes.extend_from_slice(v);
        }
        put(
            &mut c,
            &core_io::aaf::store::child_path(&o.path, "properties"),
            &bytes,
        );
    }
    for (obj, bytes) in streams {
        let sp = core_io::aaf::store::child_path(&path_of(obj), "Data-2702");
        put(&mut c, &sp, bytes);
    }
    let mut t = vec![0x4C];
    t.extend_from_slice(&(s.referenced.len() as u16).to_le_bytes());
    let pids: usize = s.referenced.iter().map(|p| p.len() + 1).sum();
    t.extend_from_slice(&(pids as u32).to_le_bytes());
    for p in &s.referenced {
        for pid in p {
            t.extend_from_slice(&pid.to_le_bytes());
        }
        t.extend_from_slice(&[0, 0]);
    }
    put(&mut c, "/referenced properties", &t);
    c.flush().unwrap();
    c.into_inner().into_inner()
}

/// Writes `bytes` as stream `path` of `c`, replacing what was there.
fn put<F: std::io::Read + Write + std::io::Seek>(
    c: &mut cfb::CompoundFile<F>,
    path: &str,
    bytes: &[u8],
) {
    if let Ok(mut st) = c.create_stream(path) {
        st.write_all(bytes).unwrap();
        st.flush().unwrap();
    }
}

fn open_mem(bytes: Vec<u8>) -> cfb::CompoundFile<std::io::Cursor<Vec<u8>>> {
    cfb::CompoundFile::open(std::io::Cursor::new(bytes)).unwrap()
}

/// Every stream of the compound file `bytes`, by path, with its bytes.
pub fn streams_of(bytes: &[u8]) -> Vec<(String, Vec<u8>)> {
    use std::io::Read;
    let mut c = open_mem(bytes.to_vec());
    let paths: Vec<String> = c
        .walk()
        .filter(|e| e.is_stream())
        .map(|e| e.path().to_string_lossy().into_owned())
        .collect();
    paths
        .into_iter()
        .map(|p| {
            let mut b = Vec::new();
            c.open_stream(&p).unwrap().read_to_end(&mut b).unwrap();
            (p, b)
        })
        .collect()
}

/// `bytes` with each `(path, new bytes)` stream replaced (or made).
pub fn with_streams(bytes: Vec<u8>, edits: &[(String, Vec<u8>)]) -> Vec<u8> {
    let mut c = open_mem(bytes);
    for (p, b) in edits {
        put(&mut c, p, b);
    }
    c.flush().unwrap();
    c.into_inner().into_inner()
}

/// A `properties` stream's entries: (pid, form, value); `None` when it
/// does not parse.
pub fn parse_props(b: &[u8]) -> Option<Vec<(u16, u16, Vec<u8>)>> {
    let u16_at = |i: usize| Some(u16::from_le_bytes(b.get(i..i + 2)?.try_into().ok()?));
    let n = usize::from(u16_at(2)?);
    let mut at = 4 + 6 * n;
    let mut out = Vec::new();
    for k in 0..n {
        let (p, f, l) = (
            u16_at(4 + 6 * k)?,
            u16_at(6 + 6 * k)?,
            usize::from(u16_at(8 + 6 * k)?),
        );
        out.push((p, f, b.get(at..at + l)?.to_vec()));
        at += l;
    }
    Some(out)
}

/// The PIDs a soup case may set hostile values on: lengths, positions,
/// rates, counts, IDs and values.
pub const POKE_PIDS: [u16; 22] = [
    pid::COMPONENT_LENGTH,
    pid::SOURCE_CLIP_START_TIME,
    pid::SOURCE_CLIP_FADE_IN_LENGTH,
    pid::SOURCE_CLIP_FADE_OUT_LENGTH,
    pid::EVENT_POSITION,
    pid::TIMELINE_MOB_SLOT_ORIGIN,
    pid::TIMELINE_MOB_SLOT_EDIT_RATE,
    pid::EVENT_MOB_SLOT_EDIT_RATE,
    pid::FILE_DESCRIPTOR_SAMPLE_RATE,
    pid::FILE_DESCRIPTOR_LENGTH,
    pid::TIMECODE_START,
    pid::TIMECODE_FPS,
    pid::TRANSITION_CUT_POINT,
    pid::CONSTANT_VALUE_VALUE,
    pid::CONTROL_POINT_TIME,
    pid::CONTROL_POINT_VALUE,
    pid::SOUND_DESCRIPTOR_CHANNELS,
    pid::SOUND_DESCRIPTOR_QUANTIZATION_BITS,
    pid::MOB_SLOT_SLOT_ID,
    pid::SOURCE_REFERENCE_SOURCE_MOB_SLOT_ID,
    pid::SOURCE_REFERENCE_SOURCE_ID,
    pid::MOB_MOB_ID,
];

/// A strong reference's hostile name.
#[derive(Debug, Clone, Copy)]
pub enum Name {
    /// `..`
    Up,
    /// `.`
    Dot,
    /// Empty.
    Empty,
    /// `/`
    Slash,
    /// `a/b`
    Nested,
    /// The object's own `properties` stream.
    Properties,
    /// The name another strong reference of the same object uses.
    Sibling,
    /// The object's own storage name.
    Own,
    /// 40 characters.
    Long,
    /// The property's own index stream.
    Index,
}

/// An edit to the streams of a written file.
#[derive(Debug, Clone)]
pub enum Edit {
    /// The first strong reference of a `properties` stream renamed.
    StrongName(proptest::sample::Index, Name),
    /// An index stream's count set.
    IndexCount(proptest::sample::Index, u32),
    /// One 4-byte field after an index stream's header set.
    IndexKey(proptest::sample::Index, proptest::sample::Index, u32),
    /// One entry's length in a `properties` stream set.
    PropLen(proptest::sample::Index, proptest::sample::Index, u16),
    /// A `properties` stream replaced by 6 bytes claiming 65 535 entries.
    EntryCount(proptest::sample::Index),
    /// A `properties` stream marked big-endian.
    BigEndian(proptest::sample::Index),
    /// A `properties` stream cut short.
    Cut(proptest::sample::Index, proptest::sample::Index),
    /// The referenced-properties table replaced.
    Referenced(Vec<u8>),
}

/// An edit to the sectors of a written file, after every [`Edit`].
#[derive(Debug, Clone)]
pub enum SectorEdit {
    /// A used FAT entry pointed at itself or at another used sector.
    FatLoop(proptest::sample::Index, proptest::sample::Index),
    /// A directory entry's left, right or child link (0, 1, 2) pointed at
    /// another entry or itself.
    DirLink(proptest::sample::Index, u8, proptest::sample::Index),
    /// A directory entry's stream size.
    StreamSize(proptest::sample::Index, u64),
    /// A directory entry's start sector.
    Start(proptest::sample::Index, u32),
}

/// One structure-soup case: a small composition built through `Store`
/// with hostile knobs, written by [`raw_write`], then edited.
#[derive(Debug, Clone)]
pub struct Soup {
    /// Control points on the gain curve around a clip.
    pub points: usize,
    /// Sequences nested around that clip (60–70 deep at the top).
    pub nest: usize,
    /// Master mobs chained in front of the file's master mob.
    pub chain: usize,
    /// Two master mobs that play each other, played by a clip.
    pub mob_loop: bool,
    /// The embedded WAV's data chunk claims `u32::MAX` bytes.
    pub wav_lie: bool,
    /// Hostile values: (which of [`POKE_PIDS`], which object, value).
    pub pokes: Vec<(proptest::sample::Index, proptest::sample::Index, Vec<u8>)>,
    /// A weak reference's table index (past the table).
    pub weak: Option<(proptest::sample::Index, u16)>,
    /// Edits to the streams.
    pub edits: Vec<Edit>,
    /// Edits to the sectors.
    pub sectors: Vec<SectorEdit>,
}

const R48_: (i32, i32) = (48_000, 1);

/// The soup's composition, before pokes.
pub fn soup_fx(k: &Soup) -> Fx {
    let mut fx = Fx::new();
    let mut spec = FileSpec::wav("A", 48_000, 1, 256);
    if k.wav_lie
        && let Media::Wav(b) = &mut spec.media
    {
        b[40..44].copy_from_slice(&u32::MAX.to_le_bytes());
    }
    let a = fx.file(&spec);
    let mut target = a.clone();
    for _ in 0..k.chain {
        let id = fx.new_id();
        fx.mob(
            class::MASTER_MOB,
            &id,
            "Hop",
            false,
            &[sound(1, "", R48_, clip(&target, 1, 0, 100))],
        );
        target = id;
    }
    let mut extra = Vec::new();
    if k.mob_loop {
        let (ma, mb) = (fx.new_id(), fx.new_id());
        fx.mob(
            class::MASTER_MOB,
            &ma,
            "LoopA",
            false,
            &[sound(1, "", R48_, clip(&mb, 1, 0, 100))],
        );
        fx.mob(
            class::MASTER_MOB,
            &mb,
            "LoopB",
            false,
            &[sound(1, "", R48_, clip(&ma, 1, 0, 100))],
        );
        extra.push(clip(&ma, 1, 0, 30));
    }
    let n = k.points.max(1) as i32;
    let pts = (0..k.points)
        .map(|i| ((i as i32, n), (1 + (i % 3) as i32, 2)))
        .collect();
    let mut seg = Seg::Op {
        op: def::OPERATION_DEF_MONO_AUDIO_GAIN,
        param: def::PARAMETER_DEF_AMPLITUDE,
        value: Value::Points(pts, def::INTERPOLATION_DEF_LINEAR),
        input: Box::new(clip(&target, 1, 0, 100)),
    };
    for _ in 0..k.nest {
        seg = Seg::Seq(vec![seg]);
    }
    let mut segs = vec![
        Seg::Fill(10),
        Seg::Clip(ClipSpec {
            mob: a.clone(),
            slot: 1,
            start: 5,
            len: 50,
            fade_in: Some((5, 1)),
            fade_out: Some((5, 2)),
        }),
        trans(10, Some(5)),
        seg,
        Seg::Fill(5),
    ];
    segs.extend(extra);
    fx.comp(
        "Soup",
        true,
        &[
            sound(1, "T", R48_, Seg::Seq(segs)),
            SlotSpec {
                id: 2,
                name: "M".into(),
                rate: R48_,
                kind: SlotKind::Markers(vec![(0, "m".into()), (40, "n".into())]),
            },
            SlotSpec {
                id: 3,
                name: "TC".into(),
                rate: (25, 1),
                kind: SlotKind::Timecode(90_000, 25, false),
            },
        ],
    );
    fx
}

/// The soup case's file bytes.
pub fn soup_bytes(k: &Soup) -> Vec<u8> {
    let mut fx = soup_fx(k);
    for (p, o, v) in &k.pokes {
        let p = POKE_PIDS[p.index(POKE_PIDS.len())];
        let having: Vec<usize> = (0..fx.s.objects.len())
            .filter(|i| fx.s.objects[*i].props.contains_key(&p))
            .collect();
        let at = if having.is_empty() {
            o.index(fx.s.objects.len())
        } else {
            having[o.index(having.len())]
        };
        fx.s.objects[at].props.insert(p, Prop::Data(v.clone()));
    }
    if let Some((i, table)) = &k.weak {
        let weak: Vec<(usize, u16)> = (0..fx.s.objects.len())
            .flat_map(|o| {
                let s = &fx.s;
                s.objects[o]
                    .props
                    .iter()
                    .filter(|(_, p)| matches!(p, Prop::Weak(_)))
                    .map(move |(k, _)| (o, *k))
            })
            .collect();
        if !weak.is_empty() {
            let (o, p) = weak[i.index(weak.len())];
            if let Some(Prop::Weak(w)) = fx.s.objects[o].props.get_mut(&p) {
                w.table_index = *table;
            }
        }
    }
    let mut bytes = fx.raw_bytes();
    if !k.edits.is_empty() {
        bytes = edited(bytes, &k.edits);
    }
    for e in &k.sectors {
        sector_edit(&mut bytes, e);
    }
    bytes
}

/// `bytes` with `edits` made to its streams.
pub fn edited(bytes: Vec<u8>, edits: &[Edit]) -> Vec<u8> {
    let mut all = streams_of(&bytes);
    let props: Vec<usize> = (0..all.len())
        .filter(|i| all[*i].0.ends_with("/properties"))
        .collect();
    let indexes: Vec<usize> = (0..all.len())
        .filter(|i| all[*i].0.ends_with(" index"))
        .collect();
    let mut changed = std::collections::BTreeSet::new();
    for e in edits {
        match e {
            Edit::StrongName(i, name) => {
                let strong: Vec<usize> = props
                    .iter()
                    .copied()
                    .filter(|k| {
                        parse_props(&all[*k].1).is_some_and(|es| {
                            es.iter().any(|(_, f, _)| matches!(f, 0x22 | 0x32 | 0x3A))
                        })
                    })
                    .collect();
                if strong.is_empty() {
                    continue;
                }
                let k = strong[i.index(strong.len())];
                let mut es = parse_props(&all[k].1).unwrap();
                let names: Vec<String> = es
                    .iter()
                    .filter(|(_, f, _)| matches!(f, 0x22 | 0x32 | 0x3A))
                    .map(|(p, _, _)| core_io::aaf::store::ref_name(*p))
                    .collect();
                let storage = all[k].0.trim_end_matches("/properties").to_string();
                let first = es
                    .iter()
                    .position(|(_, f, _)| matches!(f, 0x22 | 0x32 | 0x3A))
                    .unwrap();
                let new = match name {
                    Name::Up => "..".to_string(),
                    Name::Dot => ".".to_string(),
                    Name::Empty => String::new(),
                    Name::Slash => "/".to_string(),
                    Name::Nested => "a/b".to_string(),
                    Name::Properties => "properties".to_string(),
                    Name::Sibling => names.get(1).cloned().unwrap_or_else(|| "..".into()),
                    Name::Own => last_name(&storage).to_string(),
                    Name::Long => "x".repeat(40),
                    Name::Index => format!("{} index", names[0]),
                };
                es[first].2 = utf16z(&new);
                all[k].1 = props_raw(&es);
                changed.insert(k);
            }
            Edit::IndexCount(i, n) => {
                if let Some(&k) = (!indexes.is_empty()).then(|| &indexes[i.index(indexes.len())])
                    && all[k].1.len() >= 4
                {
                    all[k].1[..4].copy_from_slice(&n.to_le_bytes());
                    changed.insert(k);
                }
            }
            Edit::IndexKey(i, j, v) => {
                if let Some(&k) = (!indexes.is_empty()).then(|| &indexes[i.index(indexes.len())])
                    && all[k].1.len() >= 16
                {
                    let n = (all[k].1.len() - 12) / 4;
                    let at = 12 + 4 * j.index(n);
                    all[k].1[at..at + 4].copy_from_slice(&v.to_le_bytes());
                    changed.insert(k);
                }
            }
            Edit::PropLen(i, j, len) => {
                let k = props[i.index(props.len())];
                let b = &mut all[k].1;
                let n = b.get(2..4).map_or(0, |c| u16::from_le_bytes([c[0], c[1]]));
                if n > 0 {
                    let at = 8 + 6 * j.index(usize::from(n));
                    if at + 2 <= b.len() {
                        b[at..at + 2].copy_from_slice(&len.to_le_bytes());
                        changed.insert(k);
                    }
                }
            }
            Edit::EntryCount(i) => {
                let k = props[i.index(props.len())];
                all[k].1 = vec![0x4C, 0x20, 0xFF, 0xFF, 0, 0];
                changed.insert(k);
            }
            Edit::BigEndian(i) => {
                let k = props[i.index(props.len())];
                if let Some(b) = all[k].1.first_mut() {
                    *b = 0x42;
                    changed.insert(k);
                }
            }
            Edit::Cut(i, at) => {
                let k = props[i.index(props.len())];
                let n = all[k].1.len();
                all[k].1.truncate(at.index(n + 1));
                changed.insert(k);
            }
            Edit::Referenced(b) => {
                if let Some(k) = all.iter().position(|(p, _)| p == "/referenced properties") {
                    all[k].1 = b.clone();
                    changed.insert(k);
                }
            }
        }
    }
    let edits: Vec<(String, Vec<u8>)> = changed.into_iter().map(|k| all[k].clone()).collect();
    with_streams(bytes, &edits)
}

/// A `properties` stream of `entries` (little-endian).
fn props_raw(entries: &[(u16, u16, Vec<u8>)]) -> Vec<u8> {
    props(entries)
}

/// Where a compound file keeps its FAT entries and directory entries, by
/// file offset.
#[derive(Debug, Default)]
pub struct Layout {
    /// Each FAT entry's offset, by sector ID.
    pub fat: Vec<usize>,
    /// Each directory entry's offset, by stream ID.
    pub dir: Vec<usize>,
}

fn u32_at(b: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(b.get(at..at + 4)?.try_into().ok()?))
}

/// The FAT and directory of the compound file `b` (header DIFAT only).
pub fn layout(b: &[u8]) -> Layout {
    let mut out = Layout::default();
    let Some(shift) = b.get(0x1E..0x20).map(|c| u16::from_le_bytes([c[0], c[1]])) else {
        return out;
    };
    if !(9..=12).contains(&shift) {
        return out;
    }
    let ss = 1usize << shift;
    let fat_sectors = u32_at(b, 0x2C).unwrap_or(0) as usize;
    for k in 0..fat_sectors.min(109) {
        let Some(sid) = u32_at(b, 0x4C + 4 * k).filter(|s| *s < 0xFFFF_FFFA) else {
            break;
        };
        let start = (sid as usize + 1) * ss;
        for e in 0..ss / 4 {
            let at = start + 4 * e;
            if at + 4 > b.len() {
                break;
            }
            out.fat.push(at);
        }
    }
    let mut sid = u32_at(b, 0x30).unwrap_or(u32::MAX);
    let mut seen = std::collections::HashSet::new();
    while sid < 0xFFFF_FFFA && seen.insert(sid) {
        let start = (sid as usize + 1) * ss;
        for e in 0..ss / 128 {
            if start + 128 * (e + 1) <= b.len() {
                out.dir.push(start + 128 * e);
            }
        }
        sid = out
            .fat
            .get(sid as usize)
            .and_then(|at| u32_at(b, *at))
            .unwrap_or(u32::MAX);
    }
    out
}

/// Makes `e` on the compound file `b`.
pub fn sector_edit(b: &mut [u8], e: &SectorEdit) {
    let l = layout(b);
    let set32 = |b: &mut [u8], at: usize, v: u32| {
        if let Some(s) = b.get_mut(at..at + 4) {
            s.copy_from_slice(&v.to_le_bytes());
        }
    };
    let used_dir: Vec<usize> = (0..l.dir.len())
        .filter(|i| b.get(l.dir[*i] + 0x42).is_some_and(|t| *t != 0))
        .collect();
    match e {
        SectorEdit::FatLoop(i, j) => {
            let used: Vec<usize> = (0..l.fat.len())
                .filter(|k| u32_at(b, l.fat[*k]).is_some_and(|v| v < 0xFFFF_FFFA))
                .collect();
            if used.is_empty() {
                return;
            }
            let (from, to) = (used[i.index(used.len())], used[j.index(used.len())]);
            set32(b, l.fat[from], to as u32);
        }
        SectorEdit::DirLink(i, field, j) => {
            if used_dir.is_empty() {
                return;
            }
            let (from, to) = (
                used_dir[i.index(used_dir.len())],
                used_dir[j.index(used_dir.len())],
            );
            set32(
                b,
                l.dir[from] + 0x44 + 4 * usize::from(field % 3),
                to as u32,
            );
        }
        SectorEdit::StreamSize(i, size) => {
            if used_dir.is_empty() {
                return;
            }
            let at = l.dir[used_dir[i.index(used_dir.len())]] + 0x78;
            if let Some(s) = b.get_mut(at..at + 8) {
                s.copy_from_slice(&size.to_le_bytes());
            }
        }
        SectorEdit::Start(i, sid) => {
            if used_dir.is_empty() {
                return;
            }
            let at = l.dir[used_dir[i.index(used_dir.len())]] + 0x74;
            set32(b, at, *sid);
        }
    }
}

/// A hostile value: an `i64` at the edges, a rational with a zero,
/// negative or `i32::MIN` denominator (bare or indirect), a `u32` at the
/// edges, or a few random bytes.
pub fn hostile_value() -> impl proptest::strategy::Strategy<Value = Vec<u8>> {
    use proptest::prelude::*;
    use proptest::sample::select;
    let nums = vec![i32::MIN, i32::MAX, -1, 0, 1, 48_000];
    let dens = vec![0, -1, i32::MIN, 1, i32::MAX];
    prop_oneof![
        select(vec![
            i64::MIN,
            i64::MAX,
            i64::MIN + 1,
            -1,
            0,
            1 << 40,
            -(1 << 40),
            1 << 62
        ])
        .prop_map(|v| v.to_le_bytes().to_vec()),
        (select(nums.clone()), select(dens.clone())).prop_map(|(n, d)| rat(n, d)),
        (select(nums), select(dens)).prop_map(|(n, d)| indirect(&rat(n, d))),
        select(vec![0u32, 1, 2, 0xFFFF, 0x8000_0000, u32::MAX])
            .prop_map(|v| v.to_le_bytes().to_vec()),
        proptest::collection::vec(any::<u8>(), 0..12),
    ]
}

fn edit() -> impl proptest::strategy::Strategy<Value = Edit> {
    use proptest::prelude::*;
    use proptest::sample::{Index, select};
    let names = select(vec![
        Name::Up,
        Name::Dot,
        Name::Empty,
        Name::Slash,
        Name::Nested,
        Name::Properties,
        Name::Sibling,
        Name::Own,
        Name::Long,
        Name::Index,
    ]);
    let counts = prop_oneof![
        select(vec![0u32, 1, 2, 3, 65_535, 1_000_000, 1_000_001, u32::MAX]),
        any::<u32>()
    ];
    prop_oneof![
        (any::<Index>(), names).prop_map(|(i, n)| Edit::StrongName(i, n)),
        (any::<Index>(), counts).prop_map(|(i, n)| Edit::IndexCount(i, n)),
        (
            any::<Index>(),
            any::<Index>(),
            select(vec![0u32, 1, 7, u32::MAX])
        )
            .prop_map(|(i, j, v)| Edit::IndexKey(i, j, v)),
        (
            any::<Index>(),
            any::<Index>(),
            select(vec![0u16, 1, 0x7FFF, 0xFFFF])
        )
            .prop_map(|(i, j, l)| Edit::PropLen(i, j, l)),
        any::<Index>().prop_map(Edit::EntryCount),
        any::<Index>().prop_map(Edit::BigEndian),
        (any::<Index>(), any::<Index>()).prop_map(|(i, j)| Edit::Cut(i, j)),
        proptest::collection::vec(any::<u8>(), 0..24).prop_map(Edit::Referenced),
    ]
}

fn sector() -> impl proptest::strategy::Strategy<Value = SectorEdit> {
    use proptest::prelude::*;
    use proptest::sample::{Index, select};
    prop_oneof![
        (any::<Index>(), any::<Index>()).prop_map(|(i, j)| SectorEdit::FatLoop(i, j)),
        (any::<Index>(), 0u8..3, any::<Index>()).prop_map(|(i, f, j)| SectorEdit::DirLink(i, f, j)),
        (
            any::<Index>(),
            prop_oneof![
                select(vec![0u64, 4095, 4096, 4097, 1 << 32, 1 << 40, u64::MAX]),
                any::<u64>()
            ]
        )
            .prop_map(|(i, n)| SectorEdit::StreamSize(i, n)),
        (
            any::<Index>(),
            prop_oneof![
                select(vec![0u32, 1, 0xFFFF_FFFE, 0xFFFF_FFFF]),
                any::<u32>()
            ]
        )
            .prop_map(|(i, n)| SectorEdit::Start(i, n)),
    ]
}

/// The soup's cases. Large control-point counts are rare (each builds
/// 100 000 objects or more) and drawn only in an optimised build: an
/// unoptimised one reads 200 000 objects in several seconds, past the
/// cases' 2 s.
pub fn soup() -> impl proptest::strategy::Strategy<Value = Soup> {
    use proptest::collection::vec;
    use proptest::prelude::*;
    use proptest::sample::{Index, select};
    let big = if cfg!(debug_assertions) {
        vec![1_000usize, 2_000]
    } else {
        vec![99_999usize, 100_000, 100_001, 200_000]
    };
    (
        prop_oneof![
            60 => 0usize..=40,
            1 => select(big),
        ],
        prop_oneof![6 => Just(0usize), 2 => 1usize..8, 2 => 53usize..=63],
        prop_oneof![4 => Just(0usize), 2 => 1usize..=20],
        any::<bool>(),
        prop_oneof![4 => Just(false), 1 => Just(true)],
        vec((any::<Index>(), any::<Index>(), hostile_value()), 0..4),
        proptest::option::of((any::<Index>(), select(vec![1u16, 2, 7, 0x7FFF, u16::MAX]))),
        prop_oneof![2 => Just(Vec::new()), 3 => vec(edit(), 1..=3)],
        prop_oneof![3 => Just(Vec::new()), 1 => vec(sector(), 1..=2)],
    )
        .prop_map(
            |(points, nest, chain, mob_loop, wav_lie, pokes, weak, edits, sectors)| Soup {
                points,
                nest,
                chain,
                mob_loop,
                wav_lie,
                pokes,
                weak,
                edits,
                sectors,
            },
        )
}

/// A compound file at `path` whose header and FAT declare a directory
/// chain of `dir` sectors and a MiniFAT chain of `mini` (4 KiB sectors),
/// the directory's last sector pointing back at its first when `looping`.
/// Only the header, the FAT and a root entry are written; the rest is a
/// sparse hole of zeros.
pub fn cfb_skeleton(path: &Path, dir: u32, mini: u32, looping: bool) {
    const END: u32 = 0xFFFF_FFFE;
    let mut fat_n = 1u32;
    while fat_n * 1024 < fat_n + dir + mini {
        fat_n += 1;
    }
    assert!(fat_n <= 109, "the header's DIFAT holds 109 FAT sectors");
    let total = fat_n + dir + mini;
    let mut fat = vec![0xFFFF_FFFFu32; (fat_n * 1024) as usize];
    for k in 0..fat_n {
        fat[k as usize] = 0xFFFF_FFFD;
    }
    let first_dir = fat_n;
    for k in 0..dir {
        let s = first_dir + k;
        fat[s as usize] = if k + 1 < dir {
            s + 1
        } else if looping {
            first_dir
        } else {
            END
        };
    }
    let first_mini = first_dir + dir;
    for k in 0..mini {
        let s = first_mini + k;
        fat[s as usize] = if k + 1 < mini { s + 1 } else { END };
    }
    let mut h = vec![0u8; 4096];
    h[..8].copy_from_slice(&[0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1]);
    let put16 =
        |h: &mut Vec<u8>, at: usize, v: u16| h[at..at + 2].copy_from_slice(&v.to_le_bytes());
    let put32 =
        |h: &mut Vec<u8>, at: usize, v: u32| h[at..at + 4].copy_from_slice(&v.to_le_bytes());
    put16(&mut h, 0x18, 0x3E);
    put16(&mut h, 0x1A, 4);
    put16(&mut h, 0x1C, 0xFFFE);
    put16(&mut h, 0x1E, 12);
    put16(&mut h, 0x20, 6);
    put32(&mut h, 0x28, dir);
    put32(&mut h, 0x2C, fat_n);
    put32(&mut h, 0x30, first_dir);
    put32(&mut h, 0x38, 4096);
    put32(&mut h, 0x3C, if mini > 0 { first_mini } else { END });
    put32(&mut h, 0x40, mini);
    put32(&mut h, 0x44, END);
    for k in 0..109 {
        put32(
            &mut h,
            0x4C + 4 * k,
            if (k as u32) < fat_n {
                k as u32
            } else {
                u32::MAX
            },
        );
    }
    let mut f = std::fs::File::create(path).unwrap();
    f.write_all(&h).unwrap();
    let bytes: Vec<u8> = fat.iter().flat_map(|v| v.to_le_bytes()).collect();
    f.write_all(&bytes).unwrap();
    // The root entry, first in the directory.
    let mut root = vec![0u8; 128];
    let name: Vec<u8> = "Root Entry"
        .encode_utf16()
        .flat_map(u16::to_le_bytes)
        .collect();
    root[..name.len()].copy_from_slice(&name);
    put16(&mut root, 0x40, 22);
    root[0x42] = 5;
    root[0x43] = 1;
    for at in [0x44, 0x48, 0x4C] {
        put32(&mut root, at, u32::MAX);
    }
    put32(&mut root, 0x74, END);
    f.write_all(&root).unwrap();
    f.set_len(4096 * (u64::from(total) + 1)).unwrap();
}
