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
