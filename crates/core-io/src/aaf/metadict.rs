// This file holds data generated from pyaaf2's model tables
// (<https://github.com/markreidvfx/pyaaf2>, src/aaf2/model/classdefs.py,
// src/aaf2/model/typedefs.py and the root tables of src/aaf2/metadict.py,
// at commit 08dcc3dfe823ea5781db1cb657d9c2606557fcdc). pyaaf2's licence:
//
// The MIT License (MIT)
//
// Copyright (c) 2017 Mark Reid
//
// Permission is hereby granted, free of charge, to any person obtaining a copy of
// this software and associated documentation files (the "Software"), to deal in
// the Software without restriction, including without limitation the rights to
// use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of
// the Software, and to permit persons to whom the Software is furnished to do so,
// subject to the following conditions:
//
// The above copyright notice and this permission notice shall be included in all
// copies or substantial portions of the Software.
//
// THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
// IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS
// FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR
// COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER
// IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN
// CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.

//! The fixed MetaDictionary (plan 6f Ruling 11): every built-in class with
//! its properties, and every built-in type, as the model tables at
//! <https://github.com/markreidvfx/pyaaf2> write them into a new file, and
//! [`write_into`], which adds the whole subtree to a store the same way
//! every time.
//!
//! The tables are generated once (`crates/core-io/tests/data/aaf/README.md`
//! gives the steps); do not edit them by hand. Properties whose PID the
//! model leaves to the file get dynamic PIDs as those tables' writer gives
//! them: counted down from 0xFFFF in table order.

use super::dict::class;
use super::store::Auid;
use super::store::{Obj, ObjId, Prop, Store, WeakRef};
use std::collections::BTreeMap;

/// One built-in class.
#[derive(Debug)]
pub struct ClassRow {
    /// Its AUID.
    pub auid: Auid,
    /// Its name in the Object Specification.
    pub name: &'static str,
    /// Its parent class (itself for a class with none).
    pub parent: Auid,
    /// Whether objects of exactly this class may exist.
    pub concrete: bool,
    /// The properties it defines (its ancestors' are theirs).
    pub props: &'static [PropRow],
}

/// One property a class defines.
#[derive(Debug)]
pub struct PropRow {
    /// Its local identification (PID).
    pub pid: u16,
    /// Its AUID.
    pub auid: Auid,
    /// Its name.
    pub name: &'static str,
    /// The AUID of its type (a [`TypeRow`]).
    pub type_auid: Auid,
    /// Whether an object may leave it out.
    pub optional: bool,
    /// Whether it identifies its object uniquely (a set's key).
    pub unique: bool,
}

/// One built-in type.
#[derive(Debug)]
pub struct TypeRow {
    /// Its AUID.
    pub auid: Auid,
    /// Its name.
    pub name: &'static str,
    /// Its `TypeDefinition` subclass and that subclass's fields.
    pub kind: TypeKind,
}

/// A type's `TypeDefinition` subclass and its fields.
#[derive(Debug)]
pub enum TypeKind {
    /// `TypeDefinitionInteger`.
    Integer {
        /// Bytes.
        size: u8,
        /// Signed.
        signed: bool,
    },
    /// `TypeDefinitionEnumeration`.
    Enumeration {
        /// The element type.
        element: Auid,
        /// The element names.
        names: &'static [&'static str],
        /// The element values, in `names` order.
        values: &'static [i64],
    },
    /// `TypeDefinitionRecord`.
    Record {
        /// Each member's name and type.
        members: &'static [(&'static str, Auid)],
    },
    /// `TypeDefinitionFixedArray`.
    FixedArray {
        /// The element type.
        element: Auid,
        /// The element count.
        count: u32,
    },
    /// `TypeDefinitionVariableArray`.
    VariableArray {
        /// The element type.
        element: Auid,
    },
    /// `TypeDefinitionRename`.
    Rename {
        /// The type renamed.
        renamed: Auid,
    },
    /// `TypeDefinitionString`.
    String {
        /// The character type.
        element: Auid,
    },
    /// `TypeDefinitionStream`.
    Stream,
    /// `TypeDefinitionOpaque`.
    Opaque,
    /// `TypeDefinitionExtendibleEnumeration`.
    ExtEnum {
        /// Each element's name and value.
        elements: &'static [(&'static str, Auid)],
    },
    /// `TypeDefinitionCharacter`.
    Character,
    /// `TypeDefinitionGenericCharacter`.
    GenericCharacter {
        /// Bytes per character.
        size: u8,
    },
    /// `TypeDefinitionIndirect`.
    Indirect,
    /// `TypeDefinitionSet`.
    Set {
        /// The element type.
        element: Auid,
    },
    /// `TypeDefinitionStrongObjectReference`.
    StrongRef {
        /// The class referenced.
        class: Auid,
    },
    /// `TypeDefinitionWeakObjectReference`.
    WeakRef {
        /// The class referenced.
        class: Auid,
        /// The property path (AUIDs from `Root`) of the set it names.
        target: &'static [Auid],
    },
}

// The meta classes' PIDs (Object Specification, the meta-class tables; the
// same in the tables below).
const META_DICTIONARY: u16 = 0x0001;
const CLASS_DEFINITIONS: u16 = 0x0003;
const TYPE_DEFINITIONS: u16 = 0x0004;
const IDENTIFICATION: u16 = 0x0005;
const NAME: u16 = 0x0006;
const PARENT_CLASS: u16 = 0x0008;
const PROPERTIES: u16 = 0x0009;
const IS_CONCRETE: u16 = 0x000a;
const TYPE: u16 = 0x000b;
const IS_OPTIONAL: u16 = 0x000c;
const LOCAL_IDENTIFICATION: u16 = 0x000d;
const IS_UNIQUE_IDENTIFIER: u16 = 0x000e;
const INT_SIZE: u16 = 0x000f;
const INT_SIGNED: u16 = 0x0010;
const STRONG_TYPE: u16 = 0x0011;
const WEAK_TYPE: u16 = 0x0012;
const WEAK_TARGET_SET: u16 = 0x0013;
const ENUM_TYPE: u16 = 0x0014;
const ENUM_NAMES: u16 = 0x0015;
const ENUM_VALUES: u16 = 0x0016;
const FIXED_TYPE: u16 = 0x0017;
const FIXED_COUNT: u16 = 0x0018;
const VAR_TYPE: u16 = 0x0019;
const SET_TYPE: u16 = 0x001a;
const STRING_TYPE: u16 = 0x001b;
const RECORD_TYPES: u16 = 0x001c;
const RECORD_NAMES: u16 = 0x001d;
const RENAME_TYPE: u16 = 0x001e;
const EXTENUM_NAMES: u16 = 0x001f;
const EXTENUM_VALUES: u16 = 0x0020;

/// How deep a type or class chain is followed: past it the tables loop.
const MAX_CHAIN: usize = 32;

/// The class `auid` ([`ROOT`] included).
pub fn class_row(auid: Auid) -> Option<&'static ClassRow> {
    if auid == ROOT.auid {
        return Some(&ROOT);
    }
    CLASSES.iter().find(|c| c.auid == auid)
}

/// The type `auid`.
pub fn type_row(auid: Auid) -> Option<&'static TypeRow> {
    TYPES.iter().find(|t| t.auid == auid)
}

/// Class `auid` and its ancestors, nearest first (empty for an unknown
/// class).
pub fn lineage(auid: Auid) -> Vec<&'static ClassRow> {
    let mut out: Vec<&'static ClassRow> = Vec::new();
    let mut at = class_row(auid);
    while let Some(c) = at {
        if out.len() >= MAX_CHAIN || out.iter().any(|x| x.auid == c.auid) {
            break;
        }
        out.push(c);
        at = (c.parent != c.auid).then(|| class_row(c.parent)).flatten();
    }
    out
}

/// Property `pid` as class `auid` or an ancestor defines it.
pub fn prop_row(auid: Auid, pid: u16) -> Option<&'static PropRow> {
    lineage(auid)
        .into_iter()
        .flat_map(|c| c.props.iter())
        .find(|p| p.pid == pid)
}

/// The byte size of a value of type `auid`, when it is fixed.
fn fixed_size(auid: Auid, depth: usize) -> Option<usize> {
    if depth > MAX_CHAIN {
        return None;
    }
    match &type_row(auid)?.kind {
        TypeKind::Integer { size, .. } => Some(usize::from(*size)),
        TypeKind::Record { members } => {
            members.iter().map(|(_, t)| fixed_size(*t, depth + 1)).sum()
        }
        TypeKind::FixedArray { element, count } => {
            fixed_size(*element, depth + 1)?.checked_mul(usize::try_from(*count).ok()?)
        }
        TypeKind::Rename { renamed } => fixed_size(*renamed, depth + 1),
        TypeKind::Enumeration { element, .. } => fixed_size(*element, depth + 1),
        _ => None,
    }
}

/// The key of strong set `pid` of class `auid`: the PID of the unique
/// property of the class its elements are, and that key's size in bytes.
pub fn set_key(auid: Auid, pid: u16) -> Option<(u16, usize)> {
    let set = type_row(prop_row(auid, pid)?.type_auid)?;
    let TypeKind::Set { element } = set.kind else {
        return None;
    };
    let TypeKind::StrongRef { class } = type_row(element)?.kind else {
        return None;
    };
    let key = lineage(class)
        .into_iter()
        .flat_map(|c| c.props.iter())
        .find(|p| p.unique)?;
    Some((key.pid, fixed_size(key.type_auid, 0)?))
}

/// The index of `path` in the store's referenced-properties table, added
/// when absent.
pub fn table_index(s: &mut Store, path: &[u16]) -> u16 {
    let at = match s.referenced.iter().position(|p| p == path) {
        Some(i) => i,
        None => {
            s.referenced.push(path.to_vec());
            s.referenced.len() - 1
        }
    };
    u16::try_from(at).unwrap_or(u16::MAX)
}

/// A UTF-16LE string with its terminating NUL.
pub fn utf16z(text: &str) -> Vec<u8> {
    let mut out: Vec<u8> = text.encode_utf16().flat_map(u16::to_le_bytes).collect();
    out.extend_from_slice(&[0, 0]);
    out
}

/// A boolean as stored: one byte.
fn boolean(b: bool) -> Prop {
    Prop::Data(vec![u8::from(b)])
}

/// The class of a type's definition object.
fn kind_class(kind: &TypeKind) -> Auid {
    match kind {
        TypeKind::Integer { .. } => id(0x0d010101, 0x0204, 0x0000, 0x060e2b3402060101),
        TypeKind::StrongRef { .. } => id(0x0d010101, 0x0205, 0x0000, 0x060e2b3402060101),
        TypeKind::WeakRef { .. } => id(0x0d010101, 0x0206, 0x0000, 0x060e2b3402060101),
        TypeKind::Enumeration { .. } => id(0x0d010101, 0x0207, 0x0000, 0x060e2b3402060101),
        TypeKind::FixedArray { .. } => id(0x0d010101, 0x0208, 0x0000, 0x060e2b3402060101),
        TypeKind::VariableArray { .. } => id(0x0d010101, 0x0209, 0x0000, 0x060e2b3402060101),
        TypeKind::Set { .. } => id(0x0d010101, 0x020a, 0x0000, 0x060e2b3402060101),
        TypeKind::String { .. } => id(0x0d010101, 0x020b, 0x0000, 0x060e2b3402060101),
        TypeKind::Stream => id(0x0d010101, 0x020c, 0x0000, 0x060e2b3402060101),
        TypeKind::Record { .. } => id(0x0d010101, 0x020d, 0x0000, 0x060e2b3402060101),
        TypeKind::Rename { .. } => id(0x0d010101, 0x020e, 0x0000, 0x060e2b3402060101),
        TypeKind::ExtEnum { .. } => id(0x0d010101, 0x0220, 0x0000, 0x060e2b3402060101),
        TypeKind::Indirect => id(0x0d010101, 0x0221, 0x0000, 0x060e2b3402060101),
        TypeKind::Opaque => id(0x0d010101, 0x0222, 0x0000, 0x060e2b3402060101),
        TypeKind::Character => id(0x0d010101, 0x0223, 0x0000, 0x060e2b3402060101),
        TypeKind::GenericCharacter { .. } => id(0x0e040101, 0x0000, 0x0000, 0x060e2b3402060101),
    }
}

/// Adds the whole MetaDictionary to `s` as `root`'s strong reference
/// `0x0001`: one `ClassDefinition` per [`CLASSES`] row with a
/// `PropertyDefinition` per property, and one `TypeDefinition` per
/// [`TYPES`] row, in table order, so the subtree is the same every time.
/// Call it before anything else registers weak-reference paths, so its
/// table entries are the same too (its two paths come first).
pub fn write_into(s: &mut Store, root: ObjId) {
    let classes_at = table_index(s, &[META_DICTIONARY, CLASS_DEFINITIONS]);
    let types_at = table_index(s, &[META_DICTIONARY, TYPE_DEFINITIONS]);
    let class_ref = |a: Auid| {
        Prop::Weak(WeakRef {
            table_index: classes_at,
            id_pid: IDENTIFICATION,
            id: a.0.to_vec(),
        })
    };
    let type_entry = |a: Auid| WeakRef {
        table_index: types_at,
        id_pid: IDENTIFICATION,
        id: a.0.to_vec(),
    };
    let type_ref = |a: Auid| Prop::Weak(type_entry(a));
    let md = s.add_child(root, META_DICTIONARY, None, class::META_DICTIONARY);
    set(s, root, META_DICTIONARY, Prop::Strong(md));

    let mut classes = Vec::with_capacity(CLASSES.len());
    for (i, c) in (0u32..).zip(CLASSES.iter()) {
        let o = s.add_child(md, CLASS_DEFINITIONS, Some(i), class::CLASS_DEFINITION);
        let mut props = Vec::with_capacity(c.props.len());
        for (j, p) in (0u32..).zip(c.props.iter()) {
            let po = s.add_child(o, PROPERTIES, Some(j), class::PROPERTY_DEFINITION);
            let mut m = BTreeMap::new();
            m.insert(IDENTIFICATION, Prop::Data(p.auid.0.to_vec()));
            m.insert(NAME, Prop::Data(utf16z(p.name)));
            m.insert(TYPE, Prop::Data(p.type_auid.0.to_vec()));
            m.insert(IS_OPTIONAL, boolean(p.optional));
            m.insert(
                LOCAL_IDENTIFICATION,
                Prop::Data(p.pid.to_le_bytes().to_vec()),
            );
            m.insert(IS_UNIQUE_IDENTIFIER, boolean(p.unique));
            fill(s, po, m);
            props.push(po);
        }
        let mut m = BTreeMap::new();
        m.insert(IDENTIFICATION, Prop::Data(c.auid.0.to_vec()));
        m.insert(NAME, Prop::Data(utf16z(c.name)));
        m.insert(PARENT_CLASS, class_ref(c.parent));
        m.insert(PROPERTIES, Prop::StrongSet(props));
        m.insert(IS_CONCRETE, boolean(c.concrete));
        fill(s, o, m);
        classes.push(o);
    }
    set(s, md, CLASS_DEFINITIONS, Prop::StrongSet(classes));

    let char_size = CLASSES
        .iter()
        .find(|c| c.auid == kind_class(&TypeKind::GenericCharacter { size: 0 }))
        .and_then(|c| c.props.iter().find(|p| p.name == "CharacterSize"))
        .map(|p| p.pid);
    let mut types = Vec::with_capacity(TYPES.len());
    for (i, t) in (0u32..).zip(TYPES.iter()) {
        let o = s.add_child(md, TYPE_DEFINITIONS, Some(i), kind_class(&t.kind));
        let mut m = BTreeMap::new();
        m.insert(IDENTIFICATION, Prop::Data(t.auid.0.to_vec()));
        m.insert(NAME, Prop::Data(utf16z(t.name)));
        match &t.kind {
            TypeKind::Integer { size, signed } => {
                m.insert(INT_SIZE, Prop::Data(vec![*size]));
                m.insert(INT_SIGNED, boolean(*signed));
            }
            TypeKind::StrongRef { class } => {
                m.insert(STRONG_TYPE, class_ref(*class));
            }
            TypeKind::WeakRef { class, target } => {
                m.insert(WEAK_TYPE, class_ref(*class));
                let path = target.iter().flat_map(|a| a.0).collect();
                m.insert(WEAK_TARGET_SET, Prop::Data(path));
            }
            TypeKind::Enumeration {
                element,
                names,
                values,
            } => {
                m.insert(ENUM_TYPE, type_ref(*element));
                let n = names.iter().flat_map(|x| utf16z(x)).collect();
                m.insert(ENUM_NAMES, Prop::Data(n));
                let v = values.iter().flat_map(|x| x.to_le_bytes()).collect();
                m.insert(ENUM_VALUES, Prop::Data(v));
            }
            TypeKind::FixedArray { element, count } => {
                m.insert(FIXED_TYPE, type_ref(*element));
                m.insert(FIXED_COUNT, Prop::Data(count.to_le_bytes().to_vec()));
            }
            TypeKind::VariableArray { element } => {
                m.insert(VAR_TYPE, type_ref(*element));
            }
            TypeKind::Set { element } => {
                m.insert(SET_TYPE, type_ref(*element));
            }
            TypeKind::String { element } => {
                m.insert(STRING_TYPE, type_ref(*element));
            }
            TypeKind::Record { members } => {
                let refs = members.iter().map(|(_, t)| type_entry(*t)).collect();
                m.insert(RECORD_TYPES, Prop::WeakVec(refs));
                let n = members.iter().flat_map(|(x, _)| utf16z(x)).collect();
                m.insert(RECORD_NAMES, Prop::Data(n));
            }
            TypeKind::Rename { renamed } => {
                m.insert(RENAME_TYPE, type_ref(*renamed));
            }
            TypeKind::ExtEnum { elements } => {
                let n = elements.iter().flat_map(|(x, _)| utf16z(x)).collect();
                m.insert(EXTENUM_NAMES, Prop::Data(n));
                let v = elements.iter().flat_map(|(_, a)| a.0).collect();
                m.insert(EXTENUM_VALUES, Prop::Data(v));
            }
            TypeKind::GenericCharacter { size } => {
                if let Some(pid) = char_size {
                    m.insert(pid, Prop::Data(vec![*size]));
                }
            }
            TypeKind::Stream | TypeKind::Opaque | TypeKind::Character | TypeKind::Indirect => {}
        }
        fill(s, o, m);
        types.push(o);
    }
    set(s, md, TYPE_DEFINITIONS, Prop::StrongSet(types));
}

/// Sets property `pid` of `o`.
fn set(s: &mut Store, o: ObjId, pid: u16, p: Prop) {
    if let Some(obj) = s.get_mut(o) {
        obj.props.insert(pid, p);
    }
}

/// Gives `o` the properties `m`.
fn fill(s: &mut Store, o: ObjId, m: BTreeMap<u16, Prop>) {
    if let Some(Obj { props, .. }) = s.get_mut(o) {
        *props = m;
    }
}

/// An AUID from its four fields, `d4` as eight big-endian bytes.
const fn id(d1: u32, d2: u16, d3: u16, d4: u64) -> Auid {
    Auid::from_fields(d1, d2, d3, d4.to_be_bytes())
}

/// The `Root` class (the root storage's class; not in the file's
/// `ClassDefinitions`, as the source tables write it).
pub static ROOT: ClassRow = ClassRow {
    auid: id(0xb3b398a5, 0x1c90, 0x11d4, 0x8053080036210804),
    name: "Root",
    parent: id(0xb3b398a5, 0x1c90, 0x11d4, 0x8053080036210804),
    concrete: true,
    props: &[
        PropRow {
            pid: 0x0002,
            auid: id(0x0d010301, 0x0102, 0x0100, 0x060e2b3401010102),
            name: "Header",
            type_auid: id(0x05022800, 0x0000, 0x0000, 0x060e2b3401040101),
            optional: false,
            unique: false,
        },
        PropRow {
            pid: 0x0001,
            auid: id(0x0d010301, 0x0101, 0x0100, 0x060e2b3401010102),
            name: "MetaDictionary",
            type_auid: id(0x05022700, 0x0000, 0x0000, 0x060e2b3401040101),
            optional: false,
            unique: false,
        },
    ],
};

/// Every built-in class (115), in the source tables' order. A class with no
/// parent names itself, as the file stores it.
pub static CLASSES: &[ClassRow] = &[
    ClassRow {
        auid: id(0x0d010101, 0x0101, 0x0100, 0x060e2b3402060101),
        name: "InterchangeObject",
        parent: id(0x0d010101, 0x0101, 0x0100, 0x060e2b3402060101),
        concrete: false,
        props: &[
            PropRow {
                pid: 0x0101,
                auid: id(0x06010104, 0x0101, 0x0000, 0x060e2b3401010102),
                name: "ObjClass",
                type_auid: id(0x05010100, 0x0000, 0x0000, 0x060e2b3401040101),
                optional: false,
                unique: false,
            },
            PropRow {
                pid: 0x0102,
                auid: id(0x05200701, 0x0800, 0x0000, 0x060e2b3401010102),
                name: "Generation",
                type_auid: id(0x01030100, 0x0000, 0x0000, 0x060e2b3401040101),
                optional: true,
                unique: false,
            },
        ],
    },
    ClassRow {
        auid: id(0x0d010101, 0x0101, 0x0200, 0x060e2b3402060101),
        name: "Component",
        parent: id(0x0d010101, 0x0101, 0x0100, 0x060e2b3402060101),
        concrete: false,
        props: &[
            PropRow {
                pid: 0x0201,
                auid: id(0x04070100, 0x0000, 0x0000, 0x060e2b3401010102),
                name: "DataDefinition",
                type_auid: id(0x05010300, 0x0000, 0x0000, 0x060e2b3401040101),
                optional: false,
                unique: false,
            },
            PropRow {
                pid: 0x0202,
                auid: id(0x07020201, 0x0103, 0x0000, 0x060e2b3401010102),
                name: "Length",
                type_auid: id(0x01012002, 0x0000, 0x0000, 0x060e2b3401040101),
                optional: true,
                unique: false,
            },
            PropRow {
                pid: 0x0203,
                auid: id(0x03010210, 0x0400, 0x0000, 0x060e2b3401010102),
                name: "KLVData",
                type_auid: id(0x05060900, 0x0000, 0x0000, 0x060e2b3401040101),
                optional: true,
                unique: false,
            },
            PropRow {
                pid: 0x0204,
                auid: id(0x03020102, 0x1600, 0x0000, 0x060e2b3401010107),
                name: "UserComments",
                type_auid: id(0x05060800, 0x0000, 0x0000, 0x060e2b3401040101),
                optional: true,
                unique: false,
            },
            PropRow {
                pid: 0x0205,
                auid: id(0x03010210, 0x0800, 0x0000, 0x060e2b3401010107),
                name: "Attributes",
                type_auid: id(0x05060800, 0x0000, 0x0000, 0x060e2b3401040101),
                optional: true,
                unique: false,
            },
        ],
    },
    ClassRow {
        auid: id(0x0d010101, 0x0101, 0x0300, 0x060e2b3402060101),
        name: "Segment",
        parent: id(0x0d010101, 0x0101, 0x0200, 0x060e2b3402060101),
        concrete: false,
        props: &[],
    },
    ClassRow {
        auid: id(0x0d010101, 0x0101, 0x0400, 0x060e2b3402060101),
        name: "EdgeCode",
        parent: id(0x0d010101, 0x0101, 0x0300, 0x060e2b3402060101),
        concrete: true,
        props: &[
            PropRow {
                pid: 0x0401,
                auid: id(0x01040901, 0x0000, 0x0000, 0x060e2b3401010102),
                name: "Start",
                type_auid: id(0x01012001, 0x0000, 0x0000, 0x060e2b3401040101),
                optional: false,
                unique: false,
            },
            PropRow {
                pid: 0x0402,
                auid: id(0x04100103, 0x0109, 0x0000, 0x060e2b3401010102),
                name: "FilmKind",
                type_auid: id(0x0201010d, 0x0000, 0x0000, 0x060e2b3401040101),
                optional: false,
                unique: false,
            },
            PropRow {
                pid: 0x0403,
                auid: id(0x04100103, 0x0102, 0x0000, 0x060e2b3401010101),
                name: "CodeFormat",
                type_auid: id(0x0201010c, 0x0000, 0x0000, 0x060e2b3401040101),
                optional: false,
                unique: false,
            },
            PropRow {
                pid: 0x0404,
                auid: id(0x01030201, 0x0200, 0x0000, 0x060e2b3401010102),
                name: "Header",
                type_auid: id(0x04100100, 0x0000, 0x0000, 0x060e2b3401040101),
                optional: true,
                unique: false,
            },
        ],
    },
    ClassRow {
        auid: id(0x0d010101, 0x0101, 0x0500, 0x060e2b3402060101),
        name: "EssenceGroup",
        parent: id(0x0d010101, 0x0101, 0x0300, 0x060e2b3402060101),
        concrete: true,
        props: &[
            PropRow {
                pid: 0x0501,
                auid: id(0x06010104, 0x0601, 0x0000, 0x060e2b3401010102),
                name: "Choices",
                type_auid: id(0x05060700, 0x0000, 0x0000, 0x060e2b3401040101),
                optional: false,
                unique: false,
            },
            PropRow {
                pid: 0x0502,
                auid: id(0x06010104, 0x0208, 0x0000, 0x060e2b3401010102),
                name: "StillFrame",
                type_auid: id(0x05020800, 0x0000, 0x0000, 0x060e2b3401040101),
                optional: true,
                unique: false,
            },
        ],
    },
    ClassRow {
        auid: id(0x0d010101, 0x0101, 0x0600, 0x060e2b3402060101),
        name: "Event",
        parent: id(0x0d010101, 0x0101, 0x0300, 0x060e2b3402060101),
        concrete: false,
        props: &[
            PropRow {
                pid: 0x0601,
                auid: id(0x07020103, 0x0303, 0x0000, 0x060e2b3401010102),
                name: "Position",
                type_auid: id(0x01012001, 0x0000, 0x0000, 0x060e2b3401040101),
                optional: false,
                unique: false,
            },
            PropRow {
                pid: 0x0602,
                auid: id(0x05300404, 0x0100, 0x0000, 0x060e2b3401010102),
                name: "Comment",
                type_auid: id(0x01100200, 0x0000, 0x0000, 0x060e2b3401040101),
                optional: true,
                unique: false,
            },
        ],
    },
    ClassRow {
        auid: id(0x0d010101, 0x0101, 0x0700, 0x060e2b3402060101),
        name: "GPITrigger",
        parent: id(0x0d010101, 0x0101, 0x0600, 0x060e2b3402060101),
        concrete: true,
        props: &[PropRow {
            pid: 0x0801,
            auid: id(0x05300401, 0x0000, 0x0000, 0x060e2b3401010101),
            name: "ActiveState",
            type_auid: id(0x01040100, 0x0000, 0x0000, 0x060e2b3401040101),
            optional: false,
            unique: false,
        }],
    },
    ClassRow {
        auid: id(0x0d010101, 0x0101, 0x0800, 0x060e2b3402060101),
        name: "CommentMarker",
        parent: id(0x0d010101, 0x0101, 0x0600, 0x060e2b3402060101),
        concrete: true,
        props: &[PropRow {
            pid: 0x0901,
            auid: id(0x06010104, 0x020a, 0x0000, 0x060e2b3401010102),
            name: "Annotation",
            type_auid: id(0x05020800, 0x0000, 0x0000, 0x060e2b3401040101),
            optional: true,
            unique: false,
        }],
    },
    ClassRow {
        auid: id(0x0d010101, 0x0101, 0x0900, 0x060e2b3402060101),
        name: "Filler",
        parent: id(0x0d010101, 0x0101, 0x0300, 0x060e2b3402060101),
        concrete: true,
        props: &[],
    },
    ClassRow {
        auid: id(0x0d010101, 0x0101, 0x0a00, 0x060e2b3402060101),
        name: "OperationGroup",
        parent: id(0x0d010101, 0x0101, 0x0300, 0x060e2b3402060101),
        concrete: true,
        props: &[
            PropRow {
                pid: 0x0b01,
                auid: id(0x05300506, 0x0000, 0x0000, 0x060e2b3401010102),
                name: "Operation",
                type_auid: id(0x05010700, 0x0000, 0x0000, 0x060e2b3401040101),
                optional: false,
                unique: false,
            },
            PropRow {
                pid: 0x0b02,
                auid: id(0x06010104, 0x0602, 0x0000, 0x060e2b3401010102),
                name: "InputSegments",
                type_auid: id(0x05060600, 0x0000, 0x0000, 0x060e2b3401040101),
                optional: true,
                unique: false,
            },
            PropRow {
                pid: 0x0b03,
                auid: id(0x06010104, 0x060a, 0x0000, 0x060e2b3401010102),
                name: "Parameters",
                type_auid: id(0x05060a00, 0x0000, 0x0000, 0x060e2b3401040101),
                optional: true,
                unique: false,
            },
            PropRow {
                pid: 0x0b04,
                auid: id(0x0530050c, 0x0000, 0x0000, 0x060e2b3401010102),
                name: "BypassOverride",
                type_auid: id(0x01010300, 0x0000, 0x0000, 0x060e2b3401040101),
                optional: true,
                unique: false,
            },
            PropRow {
                pid: 0x0b05,
                auid: id(0x06010104, 0x0206, 0x0000, 0x060e2b3401010102),
                name: "Rendering",
                type_auid: id(0x05020800, 0x0000, 0x0000, 0x060e2b3401040101),
                optional: true,
                unique: false,
            },
        ],
    },
    ClassRow {
        auid: id(0x0d010101, 0x0101, 0x0b00, 0x060e2b3402060101),
        name: "NestedScope",
        parent: id(0x0d010101, 0x0101, 0x0300, 0x060e2b3402060101),
        concrete: true,
        props: &[PropRow {
            pid: 0x0c01,
            auid: id(0x06010104, 0x0607, 0x0000, 0x060e2b3401010102),
            name: "Slots",
            type_auid: id(0x05060600, 0x0000, 0x0000, 0x060e2b3401040101),
            optional: false,
            unique: false,
        }],
    },
    ClassRow {
        auid: id(0x0d010101, 0x0101, 0x0c00, 0x060e2b3402060101),
        name: "Pulldown",
        parent: id(0x0d010101, 0x0101, 0x0300, 0x060e2b3402060101),
        concrete: true,
        props: &[
            PropRow {
                pid: 0x0d01,
                auid: id(0x06010104, 0x0207, 0x0000, 0x060e2b3401010102),
                name: "InputSegment",
                type_auid: id(0x05020600, 0x0000, 0x0000, 0x060e2b3401040101),
                optional: false,
                unique: false,
            },
            PropRow {
                pid: 0x0d02,
                auid: id(0x05401001, 0x0200, 0x0000, 0x060e2b3401010102),
                name: "PulldownKind",
                type_auid: id(0x0201010b, 0x0000, 0x0000, 0x060e2b3401040101),
                optional: false,
                unique: false,
            },
            PropRow {
                pid: 0x0d03,
                auid: id(0x05401001, 0x0100, 0x0000, 0x060e2b3401010102),
                name: "PulldownDirection",
                type_auid: id(0x0201010a, 0x0000, 0x0000, 0x060e2b3401040101),
                optional: false,
                unique: false,
            },
            PropRow {
                pid: 0x0d04,
                auid: id(0x05401001, 0x0300, 0x0000, 0x060e2b3401010102),
                name: "PhaseFrame",
                type_auid: id(0x01012300, 0x0000, 0x0000, 0x060e2b3401040101),
                optional: false,
                unique: false,
            },
        ],
    },
    ClassRow {
        auid: id(0x0d010101, 0x0101, 0x0d00, 0x060e2b3402060101),
        name: "ScopeReference",
        parent: id(0x0d010101, 0x0101, 0x0300, 0x060e2b3402060101),
        concrete: true,
        props: &[
            PropRow {
                pid: 0x0e01,
                auid: id(0x06010103, 0x0300, 0x0000, 0x060e2b3401010102),
                name: "RelativeScope",
                type_auid: id(0x01010300, 0x0000, 0x0000, 0x060e2b3401040101),
                optional: false,
                unique: false,
            },
            PropRow {
                pid: 0x0e02,
                auid: id(0x06010103, 0x0400, 0x0000, 0x060e2b3401010102),
                name: "RelativeSlot",
                type_auid: id(0x01010300, 0x0000, 0x0000, 0x060e2b3401040101),
                optional: false,
                unique: false,
            },
        ],
    },
    ClassRow {
        auid: id(0x0d010101, 0x0101, 0x0e00, 0x060e2b3402060101),
        name: "Selector",
        parent: id(0x0d010101, 0x0101, 0x0300, 0x060e2b3402060101),
        concrete: true,
        props: &[
            PropRow {
                pid: 0x0f01,
                auid: id(0x06010104, 0x0209, 0x0000, 0x060e2b3401010102),
                name: "Selected",
                type_auid: id(0x05020600, 0x0000, 0x0000, 0x060e2b3401040101),
                optional: false,
                unique: false,
            },
            PropRow {
                pid: 0x0f02,
                auid: id(0x06010104, 0x0608, 0x0000, 0x060e2b3401010102),
                name: "Alternates",
                type_auid: id(0x05060600, 0x0000, 0x0000, 0x060e2b3401040101),
                optional: true,
                unique: false,
            },
        ],
    },
    ClassRow {
        auid: id(0x0d010101, 0x0101, 0x0f00, 0x060e2b3402060101),
        name: "Sequence",
        parent: id(0x0d010101, 0x0101, 0x0300, 0x060e2b3402060101),
        concrete: true,
        props: &[PropRow {
            pid: 0x1001,
            auid: id(0x06010104, 0x0609, 0x0000, 0x060e2b3401010102),
            name: "Components",
            type_auid: id(0x05060100, 0x0000, 0x0000, 0x060e2b3401040101),
            optional: false,
            unique: false,
        }],
    },
    ClassRow {
        auid: id(0x0d010101, 0x0101, 0x1000, 0x060e2b3402060101),
        name: "SourceReference",
        parent: id(0x0d010101, 0x0101, 0x0300, 0x060e2b3402060101),
        concrete: false,
        props: &[
            PropRow {
                pid: 0x1101,
                auid: id(0x06010103, 0x0100, 0x0000, 0x060e2b3401010102),
                name: "SourceID",
                type_auid: id(0x01030200, 0x0000, 0x0000, 0x060e2b3401040101),
                optional: true,
                unique: false,
            },
            PropRow {
                pid: 0x1102,
                auid: id(0x06010103, 0x0200, 0x0000, 0x060e2b3401010102),
                name: "SourceMobSlotID",
                type_auid: id(0x01010300, 0x0000, 0x0000, 0x060e2b3401040101),
                optional: false,
                unique: false,
            },
            PropRow {
                pid: 0x1103,
                auid: id(0x06010103, 0x0700, 0x0000, 0x060e2b3401010107),
                name: "ChannelIDs",
                type_auid: id(0x04010900, 0x0000, 0x0000, 0x060e2b3401040101),
                optional: true,
                unique: false,
            },
            PropRow {
                pid: 0x1104,
                auid: id(0x06010103, 0x0800, 0x0000, 0x060e2b3401010108),
                name: "MonoSourceSlotIDs",
                type_auid: id(0x04010900, 0x0000, 0x0000, 0x060e2b3401040101),
                optional: true,
                unique: false,
            },
        ],
    },
    ClassRow {
        auid: id(0x0d010101, 0x0101, 0x1100, 0x060e2b3402060101),
        name: "SourceClip",
        parent: id(0x0d010101, 0x0101, 0x1000, 0x060e2b3402060101),
        concrete: true,
        props: &[
            PropRow {
                pid: 0x1201,
                auid: id(0x07020103, 0x0104, 0x0000, 0x060e2b3401010102),
                name: "StartTime",
                type_auid: id(0x01012001, 0x0000, 0x0000, 0x060e2b3401040101),
                optional: true,
                unique: false,
            },
            PropRow {
                pid: 0x1202,
                auid: id(0x07020201, 0x0105, 0x0200, 0x060e2b3401010102),
                name: "FadeInLength",
                type_auid: id(0x01012002, 0x0000, 0x0000, 0x060e2b3401040101),
                optional: true,
                unique: false,
            },
            PropRow {
                pid: 0x1203,
                auid: id(0x05300501, 0x0000, 0x0000, 0x060e2b3401010101),
                name: "FadeInType",
                type_auid: id(0x02010107, 0x0000, 0x0000, 0x060e2b3401040101),
                optional: true,
                unique: false,
            },
            PropRow {
                pid: 0x1204,
                auid: id(0x07020201, 0x0105, 0x0300, 0x060e2b3401010102),
                name: "FadeOutLength",
                type_auid: id(0x01012002, 0x0000, 0x0000, 0x060e2b3401040101),
                optional: true,
                unique: false,
            },
            PropRow {
                pid: 0x1205,
                auid: id(0x05300502, 0x0000, 0x0000, 0x060e2b3401010101),
                name: "FadeOutType",
                type_auid: id(0x02010107, 0x0000, 0x0000, 0x060e2b3401040101),
                optional: true,
                unique: false,
            },
        ],
    },
    ClassRow {
        auid: id(0x0d010101, 0x0101, 0x1200, 0x060e2b3402060101),
        name: "TextClip",
        parent: id(0x0d010101, 0x0101, 0x1000, 0x060e2b3402060101),
        concrete: false,
        props: &[],
    },
    ClassRow {
        auid: id(0x0d010101, 0x0101, 0x1300, 0x060e2b3402060101),
        name: "HTMLClip",
        parent: id(0x0d010101, 0x0101, 0x1200, 0x060e2b3402060101),
        concrete: true,
        props: &[
            PropRow {
                pid: 0x1401,
                auid: id(0x05300601, 0x0100, 0x0000, 0x060e2b3401010102),
                name: "BeginAnchor",
                type_auid: id(0x01100200, 0x0000, 0x0000, 0x060e2b3401040101),
                optional: true,
                unique: false,
            },
            PropRow {
                pid: 0x1402,
                auid: id(0x05300602, 0x0100, 0x0000, 0x060e2b3401010102),
                name: "EndAnchor",
                type_auid: id(0x01100200, 0x0000, 0x0000, 0x060e2b3401040101),
                optional: true,
                unique: false,
            },
        ],
    },
    ClassRow {
        auid: id(0x0d010101, 0x0101, 0x1400, 0x060e2b3402060101),
        name: "Timecode",
        parent: id(0x0d010101, 0x0101, 0x0300, 0x060e2b3402060101),
        concrete: true,
        props: &[
            PropRow {
                pid: 0x1501,
                auid: id(0x07020103, 0x0105, 0x0000, 0x060e2b3401010102),
                name: "Start",
                type_auid: id(0x01012001, 0x0000, 0x0000, 0x060e2b3401040101),
                optional: false,
                unique: false,
            },
            PropRow {
                pid: 0x1502,
                auid: id(0x04040101, 0x0206, 0x0000, 0x060e2b3401010102),
                name: "FPS",
                type_auid: id(0x01010200, 0x0000, 0x0000, 0x060e2b3401040101),
                optional: false,
                unique: false,
            },
            PropRow {
                pid: 0x1503,
                auid: id(0x04040101, 0x0500, 0x0000, 0x060e2b3401010101),
                name: "Drop",
                type_auid: id(0x01040100, 0x0000, 0x0000, 0x060e2b3401040101),
                optional: false,
                unique: false,
            },
        ],
    },
    ClassRow {
        auid: id(0x0d010101, 0x0101, 0x1500, 0x060e2b3402060101),
        name: "TimecodeStream",
        parent: id(0x0d010101, 0x0101, 0x0300, 0x060e2b3402060101),
        concrete: false,
        props: &[
            PropRow {
                pid: 0x1601,
                auid: id(0x04040101, 0x0201, 0x0000, 0x060e2b3401010102),
                name: "SampleRate",
                type_auid: id(0x03010100, 0x0000, 0x0000, 0x060e2b3401040101),
                optional: false,
                unique: false,
            },
            PropRow {
                pid: 0x1602,
                auid: id(0x04070300, 0x0000, 0x0000, 0x060e2b3401010102),
                name: "Source",
                type_auid: id(0x04100200, 0x0000, 0x0000, 0x060e2b3401040101),
                optional: false,
                unique: false,
            },
            PropRow {
                pid: 0x1603,
                auid: id(0x04040201, 0x0000, 0x0000, 0x060e2b3401010101),
                name: "SourceType",
                type_auid: id(0x02010109, 0x0000, 0x0000, 0x060e2b3401040101),
                optional: false,
                unique: false,
            },
        ],
    },
    ClassRow {
        auid: id(0x0d010101, 0x0101, 0x1600, 0x060e2b3402060101),
        name: "TimecodeStream12M",
        parent: id(0x0d010101, 0x0101, 0x1500, 0x060e2b3402060101),
        concrete: true,
        props: &[PropRow {
            pid: 0x1701,
            auid: id(0x04040101, 0x0400, 0x0000, 0x060e2b3401010101),
            name: "IncludeSync",
            type_auid: id(0x01040100, 0x0000, 0x0000, 0x060e2b3401040101),
            optional: false,
            unique: false,
        }],
    },
    ClassRow {
        auid: id(0x0d010101, 0x0101, 0x1700, 0x060e2b3402060101),
        name: "Transition",
        parent: id(0x0d010101, 0x0101, 0x0200, 0x060e2b3402060101),
        concrete: true,
        props: &[
            PropRow {
                pid: 0x1801,
                auid: id(0x06010104, 0x0205, 0x0000, 0x060e2b3401010102),
                name: "OperationGroup",
                type_auid: id(0x05020500, 0x0000, 0x0000, 0x060e2b3401040101),
                optional: false,
                unique: false,
            },
            PropRow {
                pid: 0x1802,
                auid: id(0x07020103, 0x0106, 0x0000, 0x060e2b3401010102),
                name: "CutPoint",
                type_auid: id(0x01012001, 0x0000, 0x0000, 0x060e2b3401040101),
                optional: false,
                unique: false,
            },
        ],
    },
    ClassRow {
        auid: id(0x0d010101, 0x0101, 0x1800, 0x060e2b3402060101),
        name: "ContentStorage",
        parent: id(0x0d010101, 0x0101, 0x0100, 0x060e2b3402060101),
        concrete: true,
        props: &[
            PropRow {
                pid: 0x1901,
                auid: id(0x06010104, 0x0501, 0x0000, 0x060e2b3401010102),
                name: "Mobs",
                type_auid: id(0x05050700, 0x0000, 0x0000, 0x060e2b3401040101),
                optional: false,
                unique: false,
            },
            PropRow {
                pid: 0x1902,
                auid: id(0x06010104, 0x0502, 0x0000, 0x060e2b3401010102),
                name: "EssenceData",
                type_auid: id(0x05050500, 0x0000, 0x0000, 0x060e2b3401040101),
                optional: true,
                unique: false,
            },
        ],
    },
    ClassRow {
        auid: id(0x0d010101, 0x0101, 0x1900, 0x060e2b3402060101),
        name: "ControlPoint",
        parent: id(0x0d010101, 0x0101, 0x0100, 0x060e2b3402060101),
        concrete: true,
        props: &[
            PropRow {
                pid: 0x1a02,
                auid: id(0x0530050d, 0x0000, 0x0000, 0x060e2b3401010102),
                name: "Value",
                type_auid: id(0x04100300, 0x0000, 0x0000, 0x060e2b3401040101),
                optional: false,
                unique: false,
            },
            PropRow {
                pid: 0x1a03,
                auid: id(0x07020103, 0x1002, 0x0100, 0x060e2b3401010102),
                name: "Time",
                type_auid: id(0x03010100, 0x0000, 0x0000, 0x060e2b3401040101),
                optional: false,
                unique: false,
            },
            PropRow {
                pid: 0x1a04,
                auid: id(0x05300508, 0x0000, 0x0000, 0x060e2b3401010102),
                name: "EditHint",
                type_auid: id(0x02010106, 0x0000, 0x0000, 0x060e2b3401040101),
                optional: true,
                unique: false,
            },
        ],
    },
    ClassRow {
        auid: id(0x0d010101, 0x0101, 0x1a00, 0x060e2b3402060101),
        name: "DefinitionObject",
        parent: id(0x0d010101, 0x0101, 0x0100, 0x060e2b3402060101),
        concrete: false,
        props: &[
            PropRow {
                pid: 0x1b01,
                auid: id(0x01011503, 0x0000, 0x0000, 0x060e2b3401010102),
                name: "Identification",
                type_auid: id(0x01030100, 0x0000, 0x0000, 0x060e2b3401040101),
                optional: false,
                unique: true,
            },
            PropRow {
                pid: 0x1b02,
                auid: id(0x01070102, 0x0301, 0x0000, 0x060e2b3401010102),
                name: "Name",
                type_auid: id(0x01100200, 0x0000, 0x0000, 0x060e2b3401040101),
                optional: false,
                unique: false,
            },
            PropRow {
                pid: 0x1b03,
                auid: id(0x03020301, 0x0201, 0x0000, 0x060e2b3401010102),
                name: "Description",
                type_auid: id(0x01100200, 0x0000, 0x0000, 0x060e2b3401040101),
                optional: true,
                unique: false,
            },
        ],
    },
    ClassRow {
        auid: id(0x0d010101, 0x0101, 0x1b00, 0x060e2b3402060101),
        name: "DataDefinition",
        parent: id(0x0d010101, 0x0101, 0x1a00, 0x060e2b3402060101),
        concrete: true,
        props: &[],
    },
    ClassRow {
        auid: id(0x0d010101, 0x0101, 0x1c00, 0x060e2b3402060101),
        name: "OperationDefinition",
        parent: id(0x0d010101, 0x0101, 0x1a00, 0x060e2b3402060101),
        concrete: true,
        props: &[
            PropRow {
                pid: 0x1e01,
                auid: id(0x05300509, 0x0000, 0x0000, 0x060e2b3401010102),
                name: "DataDefinition",
                type_auid: id(0x05010300, 0x0000, 0x0000, 0x060e2b3401040101),
                optional: false,
                unique: false,
            },
            PropRow {
                pid: 0x1e02,
                auid: id(0x05300503, 0x0000, 0x0000, 0x060e2b3401010101),
                name: "IsTimeWarp",
                type_auid: id(0x01040100, 0x0000, 0x0000, 0x060e2b3401040101),
                optional: true,
                unique: false,
            },
            PropRow {
                pid: 0x1e03,
                auid: id(0x06010104, 0x0401, 0x0000, 0x060e2b3401010102),
                name: "DegradeTo",
                type_auid: id(0x05040100, 0x0000, 0x0000, 0x060e2b3401040101),
                optional: true,
                unique: false,
            },
            PropRow {
                pid: 0x1e06,
                auid: id(0x0530050a, 0x0000, 0x0000, 0x060e2b3401010102),
                name: "OperationCategory",
                type_auid: id(0x02020101, 0x0000, 0x0000, 0x060e2b3401040101),
                optional: true,
                unique: false,
            },
            PropRow {
                pid: 0x1e07,
                auid: id(0x05300504, 0x0000, 0x0000, 0x060e2b3401010101),
                name: "NumberInputs",
                type_auid: id(0x01010700, 0x0000, 0x0000, 0x060e2b3401040101),
                optional: false,
                unique: false,
            },
            PropRow {
                pid: 0x1e08,
                auid: id(0x05300505, 0x0000, 0x0000, 0x060e2b3401010101),
                name: "Bypass",
                type_auid: id(0x01010300, 0x0000, 0x0000, 0x060e2b3401040101),
                optional: true,
                unique: false,
            },
            PropRow {
                pid: 0x1e09,
                auid: id(0x06010104, 0x0302, 0x0000, 0x060e2b3401010102),
                name: "ParametersDefined",
                type_auid: id(0x05030e00, 0x0000, 0x0000, 0x060e2b3401040101),
                optional: true,
                unique: false,
            },
        ],
    },
    ClassRow {
        auid: id(0x0d010101, 0x0101, 0x1d00, 0x060e2b3402060101),
        name: "ParameterDefinition",
        parent: id(0x0d010101, 0x0101, 0x1a00, 0x060e2b3402060101),
        concrete: true,
        props: &[
            PropRow {
                pid: 0x1f01,
                auid: id(0x06010104, 0x0106, 0x0000, 0x060e2b3401010102),
                name: "Type",
                type_auid: id(0x05010900, 0x0000, 0x0000, 0x060e2b3401040101),
                optional: false,
                unique: false,
            },
            PropRow {
                pid: 0x1f03,
                auid: id(0x0530050b, 0x0100, 0x0000, 0x060e2b3401010102),
                name: "DisplayUnits",
                type_auid: id(0x01100200, 0x0000, 0x0000, 0x060e2b3401040101),
                optional: true,
                unique: false,
            },
        ],
    },
    ClassRow {
        auid: id(0x0d010101, 0x0101, 0x1e00, 0x060e2b3402060101),
        name: "PluginDefinition",
        parent: id(0x0d010101, 0x0101, 0x1a00, 0x060e2b3402060101),
        concrete: true,
        props: &[
            PropRow {
                pid: 0x2203,
                auid: id(0x05200901, 0x0000, 0x0000, 0x060e2b3401010102),
                name: "PluginCategory",
                type_auid: id(0x02020103, 0x0000, 0x0000, 0x060e2b3401040101),
                optional: false,
                unique: false,
            },
            PropRow {
                pid: 0x2204,
                auid: id(0x03030301, 0x0300, 0x0000, 0x060e2b3401010102),
                name: "VersionNumber",
                type_auid: id(0x03010300, 0x0000, 0x0000, 0x060e2b3401040101),
                optional: false,
                unique: false,
            },
            PropRow {
                pid: 0x2205,
                auid: id(0x03030301, 0x0201, 0x0000, 0x060e2b3401010102),
                name: "VersionString",
                type_auid: id(0x01100200, 0x0000, 0x0000, 0x060e2b3401040101),
                optional: true,
                unique: false,
            },
            PropRow {
                pid: 0x2206,
                auid: id(0x010a0101, 0x0101, 0x0000, 0x060e2b3401010102),
                name: "Manufacturer",
                type_auid: id(0x01100200, 0x0000, 0x0000, 0x060e2b3401040101),
                optional: true,
                unique: false,
            },
            PropRow {
                pid: 0x2207,
                auid: id(0x06010104, 0x020b, 0x0000, 0x060e2b3401010102),
                name: "ManufacturerInfo",
                type_auid: id(0x05020400, 0x0000, 0x0000, 0x060e2b3401040101),
                optional: true,
                unique: false,
            },
            PropRow {
                pid: 0x2208,
                auid: id(0x010a0101, 0x0300, 0x0000, 0x060e2b3401010102),
                name: "ManufacturerID",
                type_auid: id(0x01030100, 0x0000, 0x0000, 0x060e2b3401040101),
                optional: true,
                unique: false,
            },
            PropRow {
                pid: 0x2209,
                auid: id(0x05200902, 0x0000, 0x0000, 0x060e2b3401010102),
                name: "Platform",
                type_auid: id(0x01030100, 0x0000, 0x0000, 0x060e2b3401040101),
                optional: true,
                unique: false,
            },
            PropRow {
                pid: 0x220a,
                auid: id(0x05200903, 0x0000, 0x0000, 0x060e2b3401010102),
                name: "MinPlatformVersion",
                type_auid: id(0x03010300, 0x0000, 0x0000, 0x060e2b3401040101),
                optional: true,
                unique: false,
            },
            PropRow {
                pid: 0x220b,
                auid: id(0x05200904, 0x0000, 0x0000, 0x060e2b3401010102),
                name: "MaxPlatformVersion",
                type_auid: id(0x03010300, 0x0000, 0x0000, 0x060e2b3401040101),
                optional: true,
                unique: false,
            },
            PropRow {
                pid: 0x220c,
                auid: id(0x05200905, 0x0000, 0x0000, 0x060e2b3401010102),
                name: "Engine",
                type_auid: id(0x01030100, 0x0000, 0x0000, 0x060e2b3401040101),
                optional: true,
                unique: false,
            },
            PropRow {
                pid: 0x220d,
                auid: id(0x05200906, 0x0000, 0x0000, 0x060e2b3401010102),
                name: "MinEngineVersion",
                type_auid: id(0x03010300, 0x0000, 0x0000, 0x060e2b3401040101),
                optional: true,
                unique: false,
            },
            PropRow {
                pid: 0x220e,
                auid: id(0x05200907, 0x0000, 0x0000, 0x060e2b3401010102),
                name: "MaxEngineVersion",
                type_auid: id(0x03010300, 0x0000, 0x0000, 0x060e2b3401040101),
                optional: true,
                unique: false,
            },
            PropRow {
                pid: 0x220f,
                auid: id(0x05200908, 0x0000, 0x0000, 0x060e2b3401010102),
                name: "PluginAPI",
                type_auid: id(0x01030100, 0x0000, 0x0000, 0x060e2b3401040101),
                optional: true,
                unique: false,
            },
            PropRow {
                pid: 0x2210,
                auid: id(0x05200909, 0x0000, 0x0000, 0x060e2b3401010102),
                name: "MinPluginAPI",
                type_auid: id(0x03010300, 0x0000, 0x0000, 0x060e2b3401040101),
                optional: true,
                unique: false,
            },
            PropRow {
                pid: 0x2211,
                auid: id(0x0520090a, 0x0000, 0x0000, 0x060e2b3401010102),
                name: "MaxPluginAPI",
                type_auid: id(0x03010300, 0x0000, 0x0000, 0x060e2b3401040101),
                optional: true,
                unique: false,
            },
            PropRow {
                pid: 0x2212,
                auid: id(0x0520090b, 0x0000, 0x0000, 0x060e2b3401010102),
                name: "SoftwareOnly",
                type_auid: id(0x01040100, 0x0000, 0x0000, 0x060e2b3401040101),
                optional: true,
                unique: false,
            },
            PropRow {
                pid: 0x2213,
                auid: id(0x0520090c, 0x0000, 0x0000, 0x060e2b3401010102),
                name: "Accelerator",
                type_auid: id(0x01040100, 0x0000, 0x0000, 0x060e2b3401040101),
                optional: true,
                unique: false,
            },
            PropRow {
                pid: 0x2214,
                auid: id(0x0520090d, 0x0000, 0x0000, 0x060e2b3401010102),
                name: "Locators",
                type_auid: id(0x05060400, 0x0000, 0x0000, 0x060e2b3401040101),
                optional: true,
                unique: false,
            },
            PropRow {
                pid: 0x2215,
                auid: id(0x0520090e, 0x0000, 0x0000, 0x060e2b3401010102),
                name: "Authentication",
                type_auid: id(0x01040100, 0x0000, 0x0000, 0x060e2b3401040101),
                optional: true,
                unique: false,
            },
            PropRow {
                pid: 0x2216,
                auid: id(0x0520090f, 0x0000, 0x0000, 0x060e2b3401010102),
                name: "DefinitionObject",
                type_auid: id(0x01030100, 0x0000, 0x0000, 0x060e2b3401040101),
                optional: true,
                unique: false,
            },
        ],
    },
    ClassRow {
        auid: id(0x0d010101, 0x0101, 0x1f00, 0x060e2b3402060101),
        name: "CodecDefinition",
        parent: id(0x0d010101, 0x0101, 0x1a00, 0x060e2b3402060101),
        concrete: true,
        props: &[
            PropRow {
                pid: 0x2301,
                auid: id(0x06010104, 0x0107, 0x0000, 0x060e2b3401010102),
                name: "FileDescriptorClass",
                type_auid: id(0x05010100, 0x0000, 0x0000, 0x060e2b3401040101),
                optional: false,
                unique: false,
            },
            PropRow {
                pid: 0x2302,
                auid: id(0x06010104, 0x0301, 0x0000, 0x060e2b3401010102),
                name: "DataDefinitions",
                type_auid: id(0x05040300, 0x0000, 0x0000, 0x060e2b3401040101),
                optional: false,
                unique: false,
            },
        ],
    },
    ClassRow {
        auid: id(0x0d010101, 0x0101, 0x2000, 0x060e2b3402060101),
        name: "ContainerDefinition",
        parent: id(0x0d010101, 0x0101, 0x1a00, 0x060e2b3402060101),
        concrete: true,
        props: &[PropRow {
            pid: 0x2401,
            auid: id(0x03010201, 0x0300, 0x0000, 0x060e2b3401010101),
            name: "EssenceIsIdentified",
            type_auid: id(0x01040100, 0x0000, 0x0000, 0x060e2b3401040101),
            optional: true,
            unique: false,
        }],
    },
    ClassRow {
        auid: id(0x0d010101, 0x0101, 0x2100, 0x060e2b3402060101),
        name: "InterpolationDefinition",
        parent: id(0x0d010101, 0x0101, 0x1a00, 0x060e2b3402060101),
        concrete: true,
        props: &[],
    },
    ClassRow {
        auid: id(0x0d010101, 0x0101, 0x2200, 0x060e2b3402060101),
        name: "Dictionary",
        parent: id(0x0d010101, 0x0101, 0x0100, 0x060e2b3402060101),
        concrete: true,
        props: &[
            PropRow {
                pid: 0x2603,
                auid: id(0x06010104, 0x0503, 0x0000, 0x060e2b3401010102),
                name: "OperationDefinitions",
                type_auid: id(0x05050800, 0x0000, 0x0000, 0x060e2b3401040101),
                optional: true,
                unique: false,
            },
            PropRow {
                pid: 0x2604,
                auid: id(0x06010104, 0x0504, 0x0000, 0x060e2b3401010102),
                name: "ParameterDefinitions",
                type_auid: id(0x05050900, 0x0000, 0x0000, 0x060e2b3401040101),
                optional: true,
                unique: false,
            },
            PropRow {
                pid: 0x2605,
                auid: id(0x06010104, 0x0505, 0x0000, 0x060e2b3401010102),
                name: "DataDefinitions",
                type_auid: id(0x05050400, 0x0000, 0x0000, 0x060e2b3401040101),
                optional: true,
                unique: false,
            },
            PropRow {
                pid: 0x2606,
                auid: id(0x06010104, 0x0506, 0x0000, 0x060e2b3401010102),
                name: "PluginDefinitions",
                type_auid: id(0x05050a00, 0x0000, 0x0000, 0x060e2b3401040101),
                optional: true,
                unique: false,
            },
            PropRow {
                pid: 0x2607,
                auid: id(0x06010104, 0x0507, 0x0000, 0x060e2b3401010102),
                name: "CodecDefinitions",
                type_auid: id(0x05050200, 0x0000, 0x0000, 0x060e2b3401040101),
                optional: true,
                unique: false,
            },
            PropRow {
                pid: 0x2608,
                auid: id(0x06010104, 0x0508, 0x0000, 0x060e2b3401010102),
                name: "ContainerDefinitions",
                type_auid: id(0x05050300, 0x0000, 0x0000, 0x060e2b3401040101),
                optional: true,
                unique: false,
            },
            PropRow {
                pid: 0x2609,
                auid: id(0x06010104, 0x0509, 0x0000, 0x060e2b3401010102),
                name: "InterpolationDefinitions",
                type_auid: id(0x05050600, 0x0000, 0x0000, 0x060e2b3401040101),
                optional: true,
                unique: false,
            },
            PropRow {
                pid: 0x260a,
                auid: id(0x06010104, 0x050a, 0x0000, 0x060e2b3401010107),
                name: "KLVDataDefinitions",
                type_auid: id(0x05050d00, 0x0000, 0x0000, 0x060e2b3401040101),
                optional: true,
                unique: false,
            },
            PropRow {
                pid: 0x260b,
                auid: id(0x06010104, 0x050b, 0x0000, 0x060e2b3401010107),
                name: "TaggedValueDefinitions",
                type_auid: id(0x05050e00, 0x0000, 0x0000, 0x060e2b3401040101),
                optional: true,
                unique: false,
            },
        ],
    },
    ClassRow {
        auid: id(0x0d010101, 0x0101, 0x2300, 0x060e2b3402060101),
        name: "EssenceData",
        parent: id(0x0d010101, 0x0101, 0x0100, 0x060e2b3402060101),
        concrete: true,
        props: &[
            PropRow {
                pid: 0x2701,
                auid: id(0x06010106, 0x0100, 0x0000, 0x060e2b3401010102),
                name: "MobID",
                type_auid: id(0x01030200, 0x0000, 0x0000, 0x060e2b3401040101),
                optional: false,
                unique: true,
            },
            PropRow {
                pid: 0x2702,
                auid: id(0x04070200, 0x0000, 0x0000, 0x060e2b3401010102),
                name: "Data",
                type_auid: id(0x04100200, 0x0000, 0x0000, 0x060e2b3401040101),
                optional: false,
                unique: false,
            },
            PropRow {
                pid: 0x2b01,
                auid: id(0x06010102, 0x0100, 0x0000, 0x060e2b3401010102),
                name: "SampleIndex",
                type_auid: id(0x04100200, 0x0000, 0x0000, 0x060e2b3401040101),
                optional: true,
                unique: false,
            },
        ],
    },
    ClassRow {
        auid: id(0x0d010101, 0x0101, 0x2400, 0x060e2b3402060101),
        name: "EssenceDescriptor",
        parent: id(0x0d010101, 0x0101, 0x0100, 0x060e2b3402060101),
        concrete: false,
        props: &[
            PropRow {
                pid: 0x2f01,
                auid: id(0x06010104, 0x0603, 0x0000, 0x060e2b3401010102),
                name: "Locator",
                type_auid: id(0x05060400, 0x0000, 0x0000, 0x060e2b3401040101),
                optional: true,
                unique: false,
            },
            PropRow {
                pid: 0xffff,
                auid: id(0x06010104, 0x0610, 0x0000, 0x060e2b3401010109),
                name: "SubDescriptors",
                type_auid: id(0x05060e00, 0x0000, 0x0000, 0x060e2b3401040101),
                optional: true,
                unique: false,
            },
        ],
    },
    ClassRow {
        auid: id(0x0d010101, 0x0101, 0x2500, 0x060e2b3402060101),
        name: "FileDescriptor",
        parent: id(0x0d010101, 0x0101, 0x2400, 0x060e2b3402060101),
        concrete: false,
        props: &[
            PropRow {
                pid: 0x3001,
                auid: id(0x04060101, 0x0000, 0x0000, 0x060e2b3401010101),
                name: "SampleRate",
                type_auid: id(0x03010100, 0x0000, 0x0000, 0x060e2b3401040101),
                optional: false,
                unique: false,
            },
            PropRow {
                pid: 0x3002,
                auid: id(0x04060102, 0x0000, 0x0000, 0x060e2b3401010101),
                name: "Length",
                type_auid: id(0x01012002, 0x0000, 0x0000, 0x060e2b3401040101),
                optional: false,
                unique: false,
            },
            PropRow {
                pid: 0x3004,
                auid: id(0x06010104, 0x0102, 0x0000, 0x060e2b3401010102),
                name: "ContainerFormat",
                type_auid: id(0x05010200, 0x0000, 0x0000, 0x060e2b3401040101),
                optional: true,
                unique: false,
            },
            PropRow {
                pid: 0x3005,
                auid: id(0x06010104, 0x0103, 0x0000, 0x060e2b3401010102),
                name: "CodecDefinition",
                type_auid: id(0x05010b00, 0x0000, 0x0000, 0x060e2b3401040101),
                optional: true,
                unique: false,
            },
            PropRow {
                pid: 0x3006,
                auid: id(0x06010103, 0x0500, 0x0000, 0x060e2b3401010105),
                name: "LinkedSlotID",
                type_auid: id(0x01010300, 0x0000, 0x0000, 0x060e2b3401040101),
                optional: true,
                unique: false,
            },
        ],
    },
    ClassRow {
        auid: id(0x0d010101, 0x0101, 0x2600, 0x060e2b3402060101),
        name: "AIFCDescriptor",
        parent: id(0x0d010101, 0x0101, 0x2500, 0x060e2b3402060101),
        concrete: true,
        props: &[PropRow {
            pid: 0x3101,
            auid: id(0x03030302, 0x0200, 0x0000, 0x060e2b3401010102),
            name: "Summary",
            type_auid: id(0x04100100, 0x0000, 0x0000, 0x060e2b3401040101),
            optional: false,
            unique: false,
        }],
    },
    ClassRow {
        auid: id(0x0d010101, 0x0101, 0x2700, 0x060e2b3402060101),
        name: "DigitalImageDescriptor",
        parent: id(0x0d010101, 0x0101, 0x2500, 0x060e2b3402060101),
        concrete: false,
        props: &[
            PropRow {
                pid: 0x3201,
                auid: id(0x04010601, 0x0000, 0x0000, 0x060e2b3401010102),
                name: "Compression",
                type_auid: id(0x01030100, 0x0000, 0x0000, 0x060e2b3401040101),
                optional: true,
                unique: false,
            },
            PropRow {
                pid: 0x3202,
                auid: id(0x04010502, 0x0100, 0x0000, 0x060e2b3401010101),
                name: "StoredHeight",
                type_auid: id(0x01010300, 0x0000, 0x0000, 0x060e2b3401040101),
                optional: false,
                unique: false,
            },
            PropRow {
                pid: 0x3203,
                auid: id(0x04010502, 0x0200, 0x0000, 0x060e2b3401010101),
                name: "StoredWidth",
                type_auid: id(0x01010300, 0x0000, 0x0000, 0x060e2b3401040101),
                optional: false,
                unique: false,
            },
            PropRow {
                pid: 0x3204,
                auid: id(0x04010501, 0x0700, 0x0000, 0x060e2b3401010101),
                name: "SampledHeight",
                type_auid: id(0x01010300, 0x0000, 0x0000, 0x060e2b3401040101),
                optional: true,
                unique: false,
            },
            PropRow {
                pid: 0x3205,
                auid: id(0x04010501, 0x0800, 0x0000, 0x060e2b3401010101),
                name: "SampledWidth",
                type_auid: id(0x01010300, 0x0000, 0x0000, 0x060e2b3401040101),
                optional: true,
                unique: false,
            },
            PropRow {
                pid: 0x3206,
                auid: id(0x04010501, 0x0900, 0x0000, 0x060e2b3401010101),
                name: "SampledXOffset",
                type_auid: id(0x01010700, 0x0000, 0x0000, 0x060e2b3401040101),
                optional: true,
                unique: false,
            },
            PropRow {
                pid: 0x3207,
                auid: id(0x04010501, 0x0a00, 0x0000, 0x060e2b3401010101),
                name: "SampledYOffset",
                type_auid: id(0x01010700, 0x0000, 0x0000, 0x060e2b3401040101),
                optional: true,
                unique: false,
            },
            PropRow {
                pid: 0x3208,
                auid: id(0x04010501, 0x0b00, 0x0000, 0x060e2b3401010101),
                name: "DisplayHeight",
                type_auid: id(0x01010300, 0x0000, 0x0000, 0x060e2b3401040101),
                optional: true,
                unique: false,
            },
            PropRow {
                pid: 0x3209,
                auid: id(0x04010501, 0x0c00, 0x0000, 0x060e2b3401010101),
                name: "DisplayWidth",
                type_auid: id(0x01010300, 0x0000, 0x0000, 0x060e2b3401040101),
                optional: true,
                unique: false,
            },
            PropRow {
                pid: 0x320a,
                auid: id(0x04010501, 0x0d00, 0x0000, 0x060e2b3401010101),
                name: "DisplayXOffset",
                type_auid: id(0x01010700, 0x0000, 0x0000, 0x060e2b3401040101),
                optional: true,
                unique: false,
            },
            PropRow {
                pid: 0x320b,
                auid: id(0x04010501, 0x0e00, 0x0000, 0x060e2b3401010101),
                name: "DisplayYOffset",
                type_auid: id(0x01010700, 0x0000, 0x0000, 0x060e2b3401040101),
                optional: true,
                unique: false,
            },
            PropRow {
                pid: 0x320c,
                auid: id(0x04010301, 0x0400, 0x0000, 0x060e2b3401010101),
                name: "FrameLayout",
                type_auid: id(0x02010108, 0x0000, 0x0000, 0x060e2b3401040101),
                optional: false,
                unique: false,
            },
            PropRow {
                pid: 0x320d,
                auid: id(0x04010302, 0x0500, 0x0000, 0x060e2b3401010102),
                name: "VideoLineMap",
                type_auid: id(0x04010300, 0x0000, 0x0000, 0x060e2b3401040101),
                optional: false,
                unique: false,
            },
            PropRow {
                pid: 0x320e,
                auid: id(0x04010101, 0x0100, 0x0000, 0x060e2b3401010101),
                name: "ImageAspectRatio",
                type_auid: id(0x03010100, 0x0000, 0x0000, 0x060e2b3401040101),
                optional: false,
                unique: false,
            },
            PropRow {
                pid: 0x320f,
                auid: id(0x05200102, 0x0000, 0x0000, 0x060e2b3401010102),
                name: "AlphaTransparency",
                type_auid: id(0x02010120, 0x0000, 0x0000, 0x060e2b3401040101),
                optional: true,
                unique: false,
            },
            PropRow {
                pid: 0x3210,
                auid: id(0x04010201, 0x0101, 0x0200, 0x060e2b3401010102),
                name: "TransferCharacteristic",
                type_auid: id(0x02020102, 0x0000, 0x0000, 0x060e2b3401040101),
                optional: true,
                unique: false,
            },
            PropRow {
                pid: 0x3219,
                auid: id(0x04010201, 0x0106, 0x0100, 0x060e2b3401010109),
                name: "ColorPrimaries",
                type_auid: id(0x02020105, 0x0000, 0x0000, 0x060e2b3401040101),
                optional: true,
                unique: false,
            },
            PropRow {
                pid: 0x321a,
                auid: id(0x04010201, 0x0103, 0x0100, 0x060e2b3401010102),
                name: "CodingEquations",
                type_auid: id(0x02020106, 0x0000, 0x0000, 0x060e2b3401040101),
                optional: true,
                unique: false,
            },
            PropRow {
                pid: 0x3211,
                auid: id(0x04180101, 0x0000, 0x0000, 0x060e2b3401010102),
                name: "ImageAlignmentFactor",
                type_auid: id(0x01010300, 0x0000, 0x0000, 0x060e2b3401040101),
                optional: true,
                unique: false,
            },
            PropRow {
                pid: 0x3212,
                auid: id(0x04010301, 0x0600, 0x0000, 0x060e2b3401010102),
                name: "FieldDominance",
                type_auid: id(0x02010121, 0x0000, 0x0000, 0x060e2b3401040101),
                optional: true,
                unique: false,
            },
            PropRow {
                pid: 0x3213,
                auid: id(0x04180102, 0x0000, 0x0000, 0x060e2b3401010102),
                name: "FieldStartOffset",
                type_auid: id(0x01010300, 0x0000, 0x0000, 0x060e2b3401040101),
                optional: true,
                unique: false,
            },
            PropRow {
                pid: 0x3214,
                auid: id(0x04180103, 0x0000, 0x0000, 0x060e2b3401010102),
                name: "FieldEndOffset",
                type_auid: id(0x01010300, 0x0000, 0x0000, 0x060e2b3401040101),
                optional: true,
                unique: false,
            },
            PropRow {
                pid: 0x3215,
                auid: id(0x04050113, 0x0000, 0x0000, 0x060e2b3401010105),
                name: "SignalStandard",
                type_auid: id(0x02010127, 0x0000, 0x0000, 0x060e2b3401040101),
                optional: true,
                unique: false,
            },
            PropRow {
                pid: 0x3216,
                auid: id(0x04010302, 0x0800, 0x0000, 0x060e2b3401010105),
                name: "StoredF2Offset",
                type_auid: id(0x01010700, 0x0000, 0x0000, 0x060e2b3401040101),
                optional: true,
                unique: false,
            },
            PropRow {
                pid: 0x3217,
                auid: id(0x04010302, 0x0700, 0x0000, 0x060e2b3401010105),
                name: "DisplayF2Offset",
                type_auid: id(0x01010700, 0x0000, 0x0000, 0x060e2b3401040101),
                optional: true,
                unique: false,
            },
            PropRow {
                pid: 0x3218,
                auid: id(0x04010302, 0x0900, 0x0000, 0x060e2b3401010105),
                name: "ActiveFormatDescriptor",
                type_auid: id(0x01010100, 0x0000, 0x0000, 0x060e2b3401040101),
                optional: true,
                unique: false,
            },
        ],
    },
    ClassRow {
        auid: id(0x0d010101, 0x0101, 0x2800, 0x060e2b3402060101),
        name: "CDCIDescriptor",
        parent: id(0x0d010101, 0x0101, 0x2700, 0x060e2b3402060101),
        concrete: true,
        props: &[
            PropRow {
                pid: 0x3301,
                auid: id(0x04010503, 0x0a00, 0x0000, 0x060e2b3401010102),
                name: "ComponentWidth",
                type_auid: id(0x01010300, 0x0000, 0x0000, 0x060e2b3401040101),
                optional: false,
                unique: false,
            },
            PropRow {
                pid: 0x3302,
                auid: id(0x04010501, 0x0500, 0x0000, 0x060e2b3401010101),
                name: "HorizontalSubsampling",
                type_auid: id(0x01010300, 0x0000, 0x0000, 0x060e2b3401040101),
                optional: false,
                unique: false,
            },
            PropRow {
                pid: 0x3303,
                auid: id(0x04010501, 0x0600, 0x0000, 0x060e2b3401010101),
                name: "ColorSiting",
                type_auid: id(0x02010105, 0x0000, 0x0000, 0x060e2b3401040101),
                optional: true,
                unique: false,
            },
            PropRow {
                pid: 0x3304,
                auid: id(0x04010503, 0x0300, 0x0000, 0x060e2b3401010101),
                name: "BlackReferenceLevel",
                type_auid: id(0x01010300, 0x0000, 0x0000, 0x060e2b3401040101),
                optional: true,
                unique: false,
            },
            PropRow {
                pid: 0x3305,
                auid: id(0x04010503, 0x0400, 0x0000, 0x060e2b3401010101),
                name: "WhiteReferenceLevel",
                type_auid: id(0x01010300, 0x0000, 0x0000, 0x060e2b3401040101),
                optional: true,
                unique: false,
            },
            PropRow {
                pid: 0x3306,
                auid: id(0x04010503, 0x0500, 0x0000, 0x060e2b3401010102),
                name: "ColorRange",
                type_auid: id(0x01010300, 0x0000, 0x0000, 0x060e2b3401040101),
                optional: true,
                unique: false,
            },
            PropRow {
                pid: 0x3307,
                auid: id(0x04180104, 0x0000, 0x0000, 0x060e2b3401010102),
                name: "PaddingBits",
                type_auid: id(0x01010600, 0x0000, 0x0000, 0x060e2b3401040101),
                optional: true,
                unique: false,
            },
            PropRow {
                pid: 0x3308,
                auid: id(0x04010501, 0x1000, 0x0000, 0x060e2b3401010102),
                name: "VerticalSubsampling",
                type_auid: id(0x01010300, 0x0000, 0x0000, 0x060e2b3401040101),
                optional: true,
                unique: false,
            },
            PropRow {
                pid: 0x3309,
                auid: id(0x04010503, 0x0700, 0x0000, 0x060e2b3401010102),
                name: "AlphaSamplingWidth",
                type_auid: id(0x01010300, 0x0000, 0x0000, 0x060e2b3401040101),
                optional: true,
                unique: false,
            },
            PropRow {
                pid: 0x330b,
                auid: id(0x03010201, 0x0a00, 0x0000, 0x060e2b3401010105),
                name: "ReversedByteOrder",
                type_auid: id(0x01040100, 0x0000, 0x0000, 0x060e2b3401040101),
                optional: true,
                unique: false,
            },
        ],
    },
    ClassRow {
        auid: id(0x0d010101, 0x0101, 0x2900, 0x060e2b3402060101),
        name: "RGBADescriptor",
        parent: id(0x0d010101, 0x0101, 0x2700, 0x060e2b3402060101),
        concrete: true,
        props: &[
            PropRow {
                pid: 0x3401,
                auid: id(0x04010503, 0x0600, 0x0000, 0x060e2b3401010102),
                name: "PixelLayout",
                type_auid: id(0x04020100, 0x0000, 0x0000, 0x060e2b3401040101),
                optional: false,
                unique: false,
            },
            PropRow {
                pid: 0x3403,
                auid: id(0x04010503, 0x0800, 0x0000, 0x060e2b3401010102),
                name: "Palette",
                type_auid: id(0x04100100, 0x0000, 0x0000, 0x060e2b3401040101),
                optional: true,
                unique: false,
            },
            PropRow {
                pid: 0x3404,
                auid: id(0x04010503, 0x0900, 0x0000, 0x060e2b3401010102),
                name: "PaletteLayout",
                type_auid: id(0x04020100, 0x0000, 0x0000, 0x060e2b3401040101),
                optional: true,
                unique: false,
            },
            PropRow {
                pid: 0x3405,
                auid: id(0x04010404, 0x0100, 0x0000, 0x060e2b3401010105),
                name: "ScanningDirection",
                type_auid: id(0x02010128, 0x0000, 0x0000, 0x060e2b3401040101),
                optional: true,
                unique: false,
            },
            PropRow {
                pid: 0x3406,
                auid: id(0x04010503, 0x0b00, 0x0000, 0x060e2b3401010105),
                name: "ComponentMaxRef",
                type_auid: id(0x01010300, 0x0000, 0x0000, 0x060e2b3401040101),
                optional: true,
                unique: false,
            },
            PropRow {
                pid: 0x3407,
                auid: id(0x04010503, 0x0c00, 0x0000, 0x060e2b3401010105),
                name: "ComponentMinRef",
                type_auid: id(0x01010300, 0x0000, 0x0000, 0x060e2b3401040101),
                optional: true,
                unique: false,
            },
            PropRow {
                pid: 0x3408,
                auid: id(0x04010503, 0x0d00, 0x0000, 0x060e2b3401010105),
                name: "AlphaMaxRef",
                type_auid: id(0x01010300, 0x0000, 0x0000, 0x060e2b3401040101),
                optional: true,
                unique: false,
            },
            PropRow {
                pid: 0x3409,
                auid: id(0x04010503, 0x0e00, 0x0000, 0x060e2b3401010105),
                name: "AlphaMinRef",
                type_auid: id(0x01010300, 0x0000, 0x0000, 0x060e2b3401040101),
                optional: true,
                unique: false,
            },
        ],
    },
    ClassRow {
        auid: id(0x0d010101, 0x0101, 0x2a00, 0x060e2b3402060101),
        name: "HTMLDescriptor",
        parent: id(0x0d010101, 0x0101, 0x2500, 0x060e2b3402060101),
        concrete: true,
        props: &[],
    },
    ClassRow {
        auid: id(0x0d010101, 0x0101, 0x2b00, 0x060e2b3402060101),
        name: "TIFFDescriptor",
        parent: id(0x0d010101, 0x0101, 0x2500, 0x060e2b3402060101),
        concrete: true,
        props: &[
            PropRow {
                pid: 0x3701,
                auid: id(0x05020103, 0x0101, 0x0000, 0x060e2b3401010102),
                name: "IsUniform",
                type_auid: id(0x01040100, 0x0000, 0x0000, 0x060e2b3401040101),
                optional: false,
                unique: false,
            },
            PropRow {
                pid: 0x3702,
                auid: id(0x06080201, 0x0000, 0x0000, 0x060e2b3401010101),
                name: "IsContiguous",
                type_auid: id(0x01040100, 0x0000, 0x0000, 0x060e2b3401040101),
                optional: false,
                unique: false,
            },
            PropRow {
                pid: 0x3703,
                auid: id(0x04010302, 0x0300, 0x0000, 0x060e2b3401010101),
                name: "LeadingLines",
                type_auid: id(0x01010700, 0x0000, 0x0000, 0x060e2b3401040101),
                optional: true,
                unique: false,
            },
            PropRow {
                pid: 0x3704,
                auid: id(0x04010302, 0x0400, 0x0000, 0x060e2b3401010101),
                name: "TrailingLines",
                type_auid: id(0x01010700, 0x0000, 0x0000, 0x060e2b3401040101),
                optional: true,
                unique: false,
            },
            PropRow {
                pid: 0x3705,
                auid: id(0x05020103, 0x0102, 0x0000, 0x060e2b3401010102),
                name: "JPEGTableID",
                type_auid: id(0x01012003, 0x0000, 0x0000, 0x060e2b3401040101),
                optional: true,
                unique: false,
            },
            PropRow {
                pid: 0x3706,
                auid: id(0x03030302, 0x0300, 0x0000, 0x060e2b3401010102),
                name: "Summary",
                type_auid: id(0x04100100, 0x0000, 0x0000, 0x060e2b3401040101),
                optional: false,
                unique: false,
            },
        ],
    },
    ClassRow {
        auid: id(0x0d010101, 0x0101, 0x2c00, 0x060e2b3402060101),
        name: "WAVEDescriptor",
        parent: id(0x0d010101, 0x0101, 0x2500, 0x060e2b3402060101),
        concrete: true,
        props: &[PropRow {
            pid: 0x3801,
            auid: id(0x03030302, 0x0100, 0x0000, 0x060e2b3401010102),
            name: "Summary",
            type_auid: id(0x04100100, 0x0000, 0x0000, 0x060e2b3401040101),
            optional: false,
            unique: false,
        }],
    },
    ClassRow {
        auid: id(0x0d010101, 0x0101, 0x2d00, 0x060e2b3402060101),
        name: "FilmDescriptor",
        parent: id(0x0d010101, 0x0101, 0x2400, 0x060e2b3402060101),
        concrete: true,
        props: &[
            PropRow {
                pid: 0x3901,
                auid: id(0x04100103, 0x0108, 0x0000, 0x060e2b3401010102),
                name: "FilmFormat",
                type_auid: id(0x0201010d, 0x0000, 0x0000, 0x060e2b3401040101),
                optional: true,
                unique: false,
            },
            PropRow {
                pid: 0x3902,
                auid: id(0x04010802, 0x0300, 0x0000, 0x060e2b3401010102),
                name: "FrameRate",
                type_auid: id(0x01010300, 0x0000, 0x0000, 0x060e2b3401040101),
                optional: true,
                unique: false,
            },
            PropRow {
                pid: 0x3903,
                auid: id(0x04100103, 0x0103, 0x0000, 0x060e2b3401010102),
                name: "PerforationsPerFrame",
                type_auid: id(0x01010100, 0x0000, 0x0000, 0x060e2b3401040101),
                optional: true,
                unique: false,
            },
            PropRow {
                pid: 0x3904,
                auid: id(0x04100103, 0x0203, 0x0000, 0x060e2b3401010102),
                name: "FilmAspectRatio",
                type_auid: id(0x03010100, 0x0000, 0x0000, 0x060e2b3401040101),
                optional: true,
                unique: false,
            },
            PropRow {
                pid: 0x3905,
                auid: id(0x04100103, 0x0106, 0x0100, 0x060e2b3401010102),
                name: "Manufacturer",
                type_auid: id(0x01100200, 0x0000, 0x0000, 0x060e2b3401040101),
                optional: true,
                unique: false,
            },
            PropRow {
                pid: 0x3906,
                auid: id(0x04100103, 0x0105, 0x0100, 0x060e2b3401010102),
                name: "Model",
                type_auid: id(0x01100200, 0x0000, 0x0000, 0x060e2b3401040101),
                optional: true,
                unique: false,
            },
            PropRow {
                pid: 0x3907,
                auid: id(0x04100103, 0x0104, 0x0100, 0x060e2b3401010102),
                name: "FilmGaugeFormat",
                type_auid: id(0x01100200, 0x0000, 0x0000, 0x060e2b3401040101),
                optional: true,
                unique: false,
            },
            PropRow {
                pid: 0x3908,
                auid: id(0x04100103, 0x0107, 0x0100, 0x060e2b3401010102),
                name: "FilmBatchNumber",
                type_auid: id(0x01100200, 0x0000, 0x0000, 0x060e2b3401040101),
                optional: true,
                unique: false,
            },
        ],
    },
    ClassRow {
        auid: id(0x0d010101, 0x0101, 0x2e00, 0x060e2b3402060101),
        name: "TapeDescriptor",
        parent: id(0x0d010101, 0x0101, 0x2400, 0x060e2b3402060101),
        concrete: true,
        props: &[
            PropRow {
                pid: 0x3a01,
                auid: id(0x04100101, 0x0101, 0x0000, 0x060e2b3401010102),
                name: "FormFactor",
                type_auid: id(0x02010104, 0x0000, 0x0000, 0x060e2b3401040101),
                optional: true,
                unique: false,
            },
            PropRow {
                pid: 0x3a02,
                auid: id(0x04010401, 0x0100, 0x0000, 0x060e2b3401010102),
                name: "VideoSignal",
                type_auid: id(0x02010103, 0x0000, 0x0000, 0x060e2b3401040101),
                optional: true,
                unique: false,
            },
            PropRow {
                pid: 0x3a03,
                auid: id(0x0d010101, 0x0101, 0x0100, 0x060e2b3401010102),
                name: "TapeFormat",
                type_auid: id(0x02010102, 0x0000, 0x0000, 0x060e2b3401040101),
                optional: true,
                unique: false,
            },
            PropRow {
                pid: 0x3a04,
                auid: id(0x04100101, 0x0300, 0x0000, 0x060e2b3401010102),
                name: "Length",
                type_auid: id(0x01010300, 0x0000, 0x0000, 0x060e2b3401040101),
                optional: true,
                unique: false,
            },
            PropRow {
                pid: 0x3a05,
                auid: id(0x04100101, 0x0401, 0x0000, 0x060e2b3401010102),
                name: "ManufacturerID",
                type_auid: id(0x01100200, 0x0000, 0x0000, 0x060e2b3401040101),
                optional: true,
                unique: false,
            },
            PropRow {
                pid: 0x3a06,
                auid: id(0x04100101, 0x0201, 0x0000, 0x060e2b3401010102),
                name: "Model",
                type_auid: id(0x01100200, 0x0000, 0x0000, 0x060e2b3401040101),
                optional: true,
                unique: false,
            },
            PropRow {
                pid: 0x3a07,
                auid: id(0x04100101, 0x0601, 0x0000, 0x060e2b3401010102),
                name: "TapeBatchNumber",
                type_auid: id(0x01100200, 0x0000, 0x0000, 0x060e2b3401040101),
                optional: true,
                unique: false,
            },
            PropRow {
                pid: 0x3a08,
                auid: id(0x04100101, 0x0501, 0x0000, 0x060e2b3401010102),
                name: "TapeStock",
                type_auid: id(0x01100200, 0x0000, 0x0000, 0x060e2b3401040101),
                optional: true,
                unique: false,
            },
        ],
    },
    ClassRow {
        auid: id(0x0d010101, 0x0101, 0x2f00, 0x060e2b3402060101),
        name: "Header",
        parent: id(0x0d010101, 0x0101, 0x0100, 0x060e2b3402060101),
        concrete: true,
        props: &[
            PropRow {
                pid: 0x3b01,
                auid: id(0x03010201, 0x0200, 0x0000, 0x060e2b3401010101),
                name: "ByteOrder",
                type_auid: id(0x01010600, 0x0000, 0x0000, 0x060e2b3401040101),
                optional: false,
                unique: false,
            },
            PropRow {
                pid: 0x3b02,
                auid: id(0x07020110, 0x0204, 0x0000, 0x060e2b3401010102),
                name: "LastModified",
                type_auid: id(0x03010700, 0x0000, 0x0000, 0x060e2b3401040101),
                optional: false,
                unique: false,
            },
            PropRow {
                pid: 0x3b03,
                auid: id(0x06010104, 0x0201, 0x0000, 0x060e2b3401010102),
                name: "Content",
                type_auid: id(0x05020100, 0x0000, 0x0000, 0x060e2b3401040101),
                optional: false,
                unique: false,
            },
            PropRow {
                pid: 0x3b04,
                auid: id(0x06010104, 0x0202, 0x0000, 0x060e2b3401010102),
                name: "Dictionary",
                type_auid: id(0x05020200, 0x0000, 0x0000, 0x060e2b3401040101),
                optional: false,
                unique: false,
            },
            PropRow {
                pid: 0x3b05,
                auid: id(0x03010201, 0x0500, 0x0000, 0x060e2b3401010102),
                name: "Version",
                type_auid: id(0x03010300, 0x0000, 0x0000, 0x060e2b3401040101),
                optional: false,
                unique: false,
            },
            PropRow {
                pid: 0x3b06,
                auid: id(0x06010104, 0x0604, 0x0000, 0x060e2b3401010102),
                name: "IdentificationList",
                type_auid: id(0x05060300, 0x0000, 0x0000, 0x060e2b3401040101),
                optional: false,
                unique: false,
            },
            PropRow {
                pid: 0x3b07,
                auid: id(0x03010201, 0x0400, 0x0000, 0x060e2b3401010102),
                name: "ObjectModelVersion",
                type_auid: id(0x01010300, 0x0000, 0x0000, 0x060e2b3401040101),
                optional: true,
                unique: false,
            },
            PropRow {
                pid: 0x3b08,
                auid: id(0x06010104, 0x0108, 0x0000, 0x060e2b3401010104),
                name: "PrimaryMob",
                type_auid: id(0x05010600, 0x0000, 0x0000, 0x060e2b3401040101),
                optional: true,
                unique: false,
            },
            PropRow {
                pid: 0x3b09,
                auid: id(0x01020203, 0x0000, 0x0000, 0x060e2b3401010105),
                name: "OperationalPattern",
                type_auid: id(0x01030100, 0x0000, 0x0000, 0x060e2b3401040101),
                optional: true,
                unique: false,
            },
            PropRow {
                pid: 0x3b0a,
                auid: id(0x01020210, 0x0201, 0x0000, 0x060e2b3401010105),
                name: "EssenceContainers",
                type_auid: id(0x04030100, 0x0000, 0x0000, 0x060e2b3401040101),
                optional: true,
                unique: false,
            },
            PropRow {
                pid: 0x3b0b,
                auid: id(0x01020210, 0x0202, 0x0000, 0x060e2b3401010105),
                name: "DescriptiveSchemes",
                type_auid: id(0x04030100, 0x0000, 0x0000, 0x060e2b3401040101),
                optional: true,
                unique: false,
            },
        ],
    },
    ClassRow {
        auid: id(0x0d010101, 0x0101, 0x3000, 0x060e2b3402060101),
        name: "Identification",
        parent: id(0x0d010101, 0x0101, 0x0100, 0x060e2b3402060101),
        concrete: true,
        props: &[
            PropRow {
                pid: 0x3c01,
                auid: id(0x05200701, 0x0201, 0x0000, 0x060e2b3401010102),
                name: "CompanyName",
                type_auid: id(0x01100200, 0x0000, 0x0000, 0x060e2b3401040101),
                optional: false,
                unique: false,
            },
            PropRow {
                pid: 0x3c02,
                auid: id(0x05200701, 0x0301, 0x0000, 0x060e2b3401010102),
                name: "ProductName",
                type_auid: id(0x01100200, 0x0000, 0x0000, 0x060e2b3401040101),
                optional: false,
                unique: false,
            },
            PropRow {
                pid: 0x3c03,
                auid: id(0x05200701, 0x0400, 0x0000, 0x060e2b3401010102),
                name: "ProductVersion",
                type_auid: id(0x03010200, 0x0000, 0x0000, 0x060e2b3401040101),
                optional: true,
                unique: false,
            },
            PropRow {
                pid: 0x3c04,
                auid: id(0x05200701, 0x0501, 0x0000, 0x060e2b3401010102),
                name: "ProductVersionString",
                type_auid: id(0x01100200, 0x0000, 0x0000, 0x060e2b3401040101),
                optional: false,
                unique: false,
            },
            PropRow {
                pid: 0x3c05,
                auid: id(0x05200701, 0x0700, 0x0000, 0x060e2b3401010102),
                name: "ProductID",
                type_auid: id(0x01030100, 0x0000, 0x0000, 0x060e2b3401040101),
                optional: false,
                unique: false,
            },
            PropRow {
                pid: 0x3c06,
                auid: id(0x07020110, 0x0203, 0x0000, 0x060e2b3401010102),
                name: "Date",
                type_auid: id(0x03010700, 0x0000, 0x0000, 0x060e2b3401040101),
                optional: false,
                unique: false,
            },
            PropRow {
                pid: 0x3c07,
                auid: id(0x05200701, 0x0a00, 0x0000, 0x060e2b3401010102),
                name: "ToolkitVersion",
                type_auid: id(0x03010200, 0x0000, 0x0000, 0x060e2b3401040101),
                optional: true,
                unique: false,
            },
            PropRow {
                pid: 0x3c08,
                auid: id(0x05200701, 0x0601, 0x0000, 0x060e2b3401010102),
                name: "Platform",
                type_auid: id(0x01100200, 0x0000, 0x0000, 0x060e2b3401040101),
                optional: true,
                unique: false,
            },
            PropRow {
                pid: 0x3c09,
                auid: id(0x05200701, 0x0100, 0x0000, 0x060e2b3401010102),
                name: "GenerationAUID",
                type_auid: id(0x01030100, 0x0000, 0x0000, 0x060e2b3401040101),
                optional: false,
                unique: false,
            },
        ],
    },
    ClassRow {
        auid: id(0x0d010101, 0x0101, 0x3100, 0x060e2b3402060101),
        name: "Locator",
        parent: id(0x0d010101, 0x0101, 0x0100, 0x060e2b3402060101),
        concrete: false,
        props: &[],
    },
    ClassRow {
        auid: id(0x0d010101, 0x0101, 0x3200, 0x060e2b3402060101),
        name: "NetworkLocator",
        parent: id(0x0d010101, 0x0101, 0x3100, 0x060e2b3402060101),
        concrete: true,
        props: &[PropRow {
            pid: 0x4001,
            auid: id(0x01020101, 0x0100, 0x0000, 0x060e2b3401010101),
            name: "URLString",
            type_auid: id(0x01100200, 0x0000, 0x0000, 0x060e2b3401040101),
            optional: false,
            unique: false,
        }],
    },
    ClassRow {
        auid: id(0x0d010101, 0x0101, 0x3300, 0x060e2b3402060101),
        name: "TextLocator",
        parent: id(0x0d010101, 0x0101, 0x3100, 0x060e2b3402060101),
        concrete: true,
        props: &[PropRow {
            pid: 0x4101,
            auid: id(0x01040102, 0x0100, 0x0000, 0x060e2b3401010102),
            name: "Name",
            type_auid: id(0x01100200, 0x0000, 0x0000, 0x060e2b3401040101),
            optional: false,
            unique: false,
        }],
    },
    ClassRow {
        auid: id(0x0d010101, 0x0101, 0x3400, 0x060e2b3402060101),
        name: "Mob",
        parent: id(0x0d010101, 0x0101, 0x0100, 0x060e2b3402060101),
        concrete: false,
        props: &[
            PropRow {
                pid: 0x4401,
                auid: id(0x01011510, 0x0000, 0x0000, 0x060e2b3401010101),
                name: "MobID",
                type_auid: id(0x01030200, 0x0000, 0x0000, 0x060e2b3401040101),
                optional: false,
                unique: true,
            },
            PropRow {
                pid: 0x4402,
                auid: id(0x01030302, 0x0100, 0x0000, 0x060e2b3401010101),
                name: "Name",
                type_auid: id(0x01100200, 0x0000, 0x0000, 0x060e2b3401040101),
                optional: true,
                unique: false,
            },
            PropRow {
                pid: 0x4403,
                auid: id(0x06010104, 0x0605, 0x0000, 0x060e2b3401010102),
                name: "Slots",
                type_auid: id(0x05060500, 0x0000, 0x0000, 0x060e2b3401040101),
                optional: false,
                unique: false,
            },
            PropRow {
                pid: 0x4404,
                auid: id(0x07020110, 0x0205, 0x0000, 0x060e2b3401010102),
                name: "LastModified",
                type_auid: id(0x03010700, 0x0000, 0x0000, 0x060e2b3401040101),
                optional: false,
                unique: false,
            },
            PropRow {
                pid: 0x4405,
                auid: id(0x07020110, 0x0103, 0x0000, 0x060e2b3401010102),
                name: "CreationTime",
                type_auid: id(0x03010700, 0x0000, 0x0000, 0x060e2b3401040101),
                optional: false,
                unique: false,
            },
            PropRow {
                pid: 0x4406,
                auid: id(0x03020102, 0x0c00, 0x0000, 0x060e2b3401010102),
                name: "UserComments",
                type_auid: id(0x05060800, 0x0000, 0x0000, 0x060e2b3401040101),
                optional: true,
                unique: false,
            },
            PropRow {
                pid: 0x4407,
                auid: id(0x03010210, 0x0300, 0x0000, 0x060e2b3401010102),
                name: "KLVData",
                type_auid: id(0x05060900, 0x0000, 0x0000, 0x060e2b3401040101),
                optional: true,
                unique: false,
            },
            PropRow {
                pid: 0x4409,
                auid: id(0x03010210, 0x0700, 0x0000, 0x060e2b3401010107),
                name: "Attributes",
                type_auid: id(0x05060800, 0x0000, 0x0000, 0x060e2b3401040101),
                optional: true,
                unique: false,
            },
            PropRow {
                pid: 0x4408,
                auid: id(0x05010108, 0x0000, 0x0000, 0x060e2b3401010107),
                name: "UsageCode",
                type_auid: id(0x02020104, 0x0000, 0x0000, 0x060e2b3401040101),
                optional: true,
                unique: false,
            },
        ],
    },
    ClassRow {
        auid: id(0x0d010101, 0x0101, 0x3500, 0x060e2b3402060101),
        name: "CompositionMob",
        parent: id(0x0d010101, 0x0101, 0x3400, 0x060e2b3402060101),
        concrete: true,
        props: &[
            PropRow {
                pid: 0x4501,
                auid: id(0x07020201, 0x0105, 0x0100, 0x060e2b3401010102),
                name: "DefaultFadeLength",
                type_auid: id(0x01012002, 0x0000, 0x0000, 0x060e2b3401040101),
                optional: true,
                unique: false,
            },
            PropRow {
                pid: 0x4502,
                auid: id(0x05300201, 0x0000, 0x0000, 0x060e2b3401010101),
                name: "DefFadeType",
                type_auid: id(0x02010107, 0x0000, 0x0000, 0x060e2b3401040101),
                optional: true,
                unique: false,
            },
            PropRow {
                pid: 0x4503,
                auid: id(0x05300403, 0x0000, 0x0000, 0x060e2b3401010102),
                name: "DefFadeEditUnit",
                type_auid: id(0x03010100, 0x0000, 0x0000, 0x060e2b3401040101),
                optional: true,
                unique: false,
            },
            PropRow {
                pid: 0x4504,
                auid: id(0x06010104, 0x010a, 0x0000, 0x060e2b3401010108),
                name: "Rendering",
                type_auid: id(0x01030200, 0x0000, 0x0000, 0x060e2b3401040101),
                optional: true,
                unique: false,
            },
        ],
    },
    ClassRow {
        auid: id(0x0d010101, 0x0101, 0x3600, 0x060e2b3402060101),
        name: "MasterMob",
        parent: id(0x0d010101, 0x0101, 0x3400, 0x060e2b3402060101),
        concrete: true,
        props: &[],
    },
    ClassRow {
        auid: id(0x0d010101, 0x0101, 0x3700, 0x060e2b3402060101),
        name: "SourceMob",
        parent: id(0x0d010101, 0x0101, 0x3400, 0x060e2b3402060101),
        concrete: true,
        props: &[PropRow {
            pid: 0x4701,
            auid: id(0x06010104, 0x0203, 0x0000, 0x060e2b3401010102),
            name: "EssenceDescription",
            type_auid: id(0x05020300, 0x0000, 0x0000, 0x060e2b3401040101),
            optional: false,
            unique: false,
        }],
    },
    ClassRow {
        auid: id(0x0d010101, 0x0101, 0x3800, 0x060e2b3402060101),
        name: "MobSlot",
        parent: id(0x0d010101, 0x0101, 0x0100, 0x060e2b3402060101),
        concrete: false,
        props: &[
            PropRow {
                pid: 0x4801,
                auid: id(0x01070101, 0x0000, 0x0000, 0x060e2b3401010102),
                name: "SlotID",
                type_auid: id(0x01010300, 0x0000, 0x0000, 0x060e2b3401040101),
                optional: false,
                unique: false,
            },
            PropRow {
                pid: 0x4802,
                auid: id(0x01070102, 0x0100, 0x0000, 0x060e2b3401010102),
                name: "SlotName",
                type_auid: id(0x01100200, 0x0000, 0x0000, 0x060e2b3401040101),
                optional: true,
                unique: false,
            },
            PropRow {
                pid: 0x4803,
                auid: id(0x06010104, 0x0204, 0x0000, 0x060e2b3401010102),
                name: "Segment",
                type_auid: id(0x05020600, 0x0000, 0x0000, 0x060e2b3401040101),
                optional: false,
                unique: false,
            },
            PropRow {
                pid: 0x4804,
                auid: id(0x01040103, 0x0000, 0x0000, 0x060e2b3401010102),
                name: "PhysicalTrackNumber",
                type_auid: id(0x01010300, 0x0000, 0x0000, 0x060e2b3401040101),
                optional: true,
                unique: false,
            },
        ],
    },
    ClassRow {
        auid: id(0x0d010101, 0x0101, 0x3900, 0x060e2b3402060101),
        name: "EventMobSlot",
        parent: id(0x0d010101, 0x0101, 0x3800, 0x060e2b3402060101),
        concrete: true,
        props: &[
            PropRow {
                pid: 0x4901,
                auid: id(0x05300402, 0x0000, 0x0000, 0x060e2b3401010102),
                name: "EditRate",
                type_auid: id(0x03010100, 0x0000, 0x0000, 0x060e2b3401040101),
                optional: false,
                unique: false,
            },
            PropRow {
                pid: 0x4902,
                auid: id(0x07020103, 0x010b, 0x0000, 0x060e2b3401010105),
                name: "EventSlotOrigin",
                type_auid: id(0x01012001, 0x0000, 0x0000, 0x060e2b3401040101),
                optional: true,
                unique: false,
            },
        ],
    },
    ClassRow {
        auid: id(0x0d010101, 0x0101, 0x3a00, 0x060e2b3402060101),
        name: "StaticMobSlot",
        parent: id(0x0d010101, 0x0101, 0x3800, 0x060e2b3402060101),
        concrete: true,
        props: &[],
    },
    ClassRow {
        auid: id(0x0d010101, 0x0101, 0x3b00, 0x060e2b3402060101),
        name: "TimelineMobSlot",
        parent: id(0x0d010101, 0x0101, 0x3800, 0x060e2b3402060101),
        concrete: true,
        props: &[
            PropRow {
                pid: 0x4b01,
                auid: id(0x05300405, 0x0000, 0x0000, 0x060e2b3401010102),
                name: "EditRate",
                type_auid: id(0x03010100, 0x0000, 0x0000, 0x060e2b3401040101),
                optional: false,
                unique: false,
            },
            PropRow {
                pid: 0x4b02,
                auid: id(0x07020103, 0x0103, 0x0000, 0x060e2b3401010102),
                name: "Origin",
                type_auid: id(0x01012001, 0x0000, 0x0000, 0x060e2b3401040101),
                optional: false,
                unique: false,
            },
            PropRow {
                pid: 0x4b03,
                auid: id(0x07020103, 0x010c, 0x0000, 0x060e2b3401010107),
                name: "MarkIn",
                type_auid: id(0x01012001, 0x0000, 0x0000, 0x060e2b3401040101),
                optional: true,
                unique: false,
            },
            PropRow {
                pid: 0x4b04,
                auid: id(0x07020103, 0x0203, 0x0000, 0x060e2b3401010107),
                name: "MarkOut",
                type_auid: id(0x01012001, 0x0000, 0x0000, 0x060e2b3401040101),
                optional: true,
                unique: false,
            },
            PropRow {
                pid: 0x4b05,
                auid: id(0x07020103, 0x010d, 0x0000, 0x060e2b3401010107),
                name: "UserPos",
                type_auid: id(0x01012001, 0x0000, 0x0000, 0x060e2b3401040101),
                optional: true,
                unique: false,
            },
        ],
    },
    ClassRow {
        auid: id(0x0d010101, 0x0101, 0x3c00, 0x060e2b3402060101),
        name: "Parameter",
        parent: id(0x0d010101, 0x0101, 0x0100, 0x060e2b3402060101),
        concrete: false,
        props: &[PropRow {
            pid: 0x4c01,
            auid: id(0x06010104, 0x0104, 0x0000, 0x060e2b3401010102),
            name: "Definition",
            type_auid: id(0x01030100, 0x0000, 0x0000, 0x060e2b3401040101),
            optional: false,
            unique: false,
        }],
    },
    ClassRow {
        auid: id(0x0d010101, 0x0101, 0x3d00, 0x060e2b3402060101),
        name: "ConstantValue",
        parent: id(0x0d010101, 0x0101, 0x3c00, 0x060e2b3402060101),
        concrete: true,
        props: &[PropRow {
            pid: 0x4d01,
            auid: id(0x05300507, 0x0000, 0x0000, 0x060e2b3401010102),
            name: "Value",
            type_auid: id(0x04100300, 0x0000, 0x0000, 0x060e2b3401040101),
            optional: false,
            unique: false,
        }],
    },
    ClassRow {
        auid: id(0x0d010101, 0x0101, 0x3e00, 0x060e2b3402060101),
        name: "VaryingValue",
        parent: id(0x0d010101, 0x0101, 0x3c00, 0x060e2b3402060101),
        concrete: true,
        props: &[
            PropRow {
                pid: 0x4e01,
                auid: id(0x06010104, 0x0105, 0x0000, 0x060e2b3401010102),
                name: "Interpolation",
                type_auid: id(0x05010500, 0x0000, 0x0000, 0x060e2b3401040101),
                optional: false,
                unique: false,
            },
            PropRow {
                pid: 0x4e02,
                auid: id(0x06010104, 0x0606, 0x0000, 0x060e2b3401010102),
                name: "PointList",
                type_auid: id(0x05060200, 0x0000, 0x0000, 0x060e2b3401040101),
                optional: false,
                unique: false,
            },
        ],
    },
    ClassRow {
        auid: id(0x0d010101, 0x0101, 0x3f00, 0x060e2b3402060101),
        name: "TaggedValue",
        parent: id(0x0d010101, 0x0101, 0x0100, 0x060e2b3402060101),
        concrete: true,
        props: &[
            PropRow {
                pid: 0x5001,
                auid: id(0x03020102, 0x0901, 0x0000, 0x060e2b3401010102),
                name: "Name",
                type_auid: id(0x01100200, 0x0000, 0x0000, 0x060e2b3401040101),
                optional: false,
                unique: false,
            },
            PropRow {
                pid: 0x5003,
                auid: id(0x03020102, 0x0a01, 0x0000, 0x060e2b3401010102),
                name: "Value",
                type_auid: id(0x04100300, 0x0000, 0x0000, 0x060e2b3401040101),
                optional: false,
                unique: false,
            },
        ],
    },
    ClassRow {
        auid: id(0x0d010101, 0x0101, 0x4000, 0x060e2b3402060101),
        name: "KLVData",
        parent: id(0x0d010101, 0x0101, 0x0100, 0x060e2b3402060101),
        concrete: true,
        props: &[PropRow {
            pid: 0x5101,
            auid: id(0x03010210, 0x0200, 0x0000, 0x060e2b3401010102),
            name: "Value",
            type_auid: id(0x04100400, 0x0000, 0x0000, 0x060e2b3401040101),
            optional: false,
            unique: false,
        }],
    },
    ClassRow {
        auid: id(0x0d010101, 0x0101, 0x4100, 0x060e2b3402060101),
        name: "DescriptiveMarker",
        parent: id(0x0d010101, 0x0101, 0x0800, 0x060e2b3402060101),
        concrete: true,
        props: &[
            PropRow {
                pid: 0x6102,
                auid: id(0x01070105, 0x0000, 0x0000, 0x060e2b3401010104),
                name: "DescribedSlots",
                type_auid: id(0x04030200, 0x0000, 0x0000, 0x060e2b3401040101),
                optional: true,
                unique: false,
            },
            PropRow {
                pid: 0x6101,
                auid: id(0x06010104, 0x020c, 0x0000, 0x060e2b3401010105),
                name: "Description",
                type_auid: id(0x05021f00, 0x0000, 0x0000, 0x060e2b3401040101),
                optional: true,
                unique: false,
            },
        ],
    },
    ClassRow {
        auid: id(0x0d010101, 0x0101, 0x4200, 0x060e2b3402060101),
        name: "SoundDescriptor",
        parent: id(0x0d010101, 0x0101, 0x2500, 0x060e2b3402060101),
        concrete: true,
        props: &[
            PropRow {
                pid: 0x3d03,
                auid: id(0x04020301, 0x0101, 0x0000, 0x060e2b3401010105),
                name: "AudioSamplingRate",
                type_auid: id(0x03010100, 0x0000, 0x0000, 0x060e2b3401040101),
                optional: false,
                unique: false,
            },
            PropRow {
                pid: 0x3d02,
                auid: id(0x04020301, 0x0400, 0x0000, 0x060e2b3401010104),
                name: "Locked",
                type_auid: id(0x01040100, 0x0000, 0x0000, 0x060e2b3401040101),
                optional: true,
                unique: false,
            },
            PropRow {
                pid: 0x3d04,
                auid: id(0x04020101, 0x0300, 0x0000, 0x060e2b3401010101),
                name: "AudioRefLevel",
                type_auid: id(0x01010500, 0x0000, 0x0000, 0x060e2b3401040101),
                optional: true,
                unique: false,
            },
            PropRow {
                pid: 0x3d05,
                auid: id(0x04020101, 0x0100, 0x0000, 0x060e2b3401010101),
                name: "ElectroSpatial",
                type_auid: id(0x02010122, 0x0000, 0x0000, 0x060e2b3401040101),
                optional: true,
                unique: false,
            },
            PropRow {
                pid: 0x3d07,
                auid: id(0x04020101, 0x0400, 0x0000, 0x060e2b3401010105),
                name: "Channels",
                type_auid: id(0x01010300, 0x0000, 0x0000, 0x060e2b3401040101),
                optional: false,
                unique: false,
            },
            PropRow {
                pid: 0x3d01,
                auid: id(0x04020303, 0x0400, 0x0000, 0x060e2b3401010104),
                name: "QuantizationBits",
                type_auid: id(0x01010300, 0x0000, 0x0000, 0x060e2b3401040101),
                optional: false,
                unique: false,
            },
            PropRow {
                pid: 0x3d0c,
                auid: id(0x04020701, 0x0000, 0x0000, 0x060e2b3401010105),
                name: "DialNorm",
                type_auid: id(0x01010500, 0x0000, 0x0000, 0x060e2b3401040101),
                optional: true,
                unique: false,
            },
            PropRow {
                pid: 0x3d06,
                auid: id(0x04020402, 0x0000, 0x0000, 0x060e2b3401010102),
                name: "Compression",
                type_auid: id(0x01030100, 0x0000, 0x0000, 0x060e2b3401040101),
                optional: true,
                unique: false,
            },
        ],
    },
    ClassRow {
        auid: id(0x0d010101, 0x0101, 0x4300, 0x060e2b3402060101),
        name: "DataEssenceDescriptor",
        parent: id(0x0d010101, 0x0101, 0x2500, 0x060e2b3402060101),
        concrete: true,
        props: &[PropRow {
            pid: 0x3e01,
            auid: id(0x04030302, 0x0000, 0x0000, 0x060e2b3401010103),
            name: "DataEssenceCoding",
            type_auid: id(0x01030100, 0x0000, 0x0000, 0x060e2b3401040101),
            optional: true,
            unique: false,
        }],
    },
    ClassRow {
        auid: id(0x0d010101, 0x0101, 0x4400, 0x060e2b3402060101),
        name: "MultipleDescriptor",
        parent: id(0x0d010101, 0x0101, 0x2500, 0x060e2b3402060101),
        concrete: true,
        props: &[PropRow {
            pid: 0x3f01,
            auid: id(0x06010104, 0x060b, 0x0000, 0x060e2b3401010104),
            name: "FileDescriptors",
            type_auid: id(0x05060b00, 0x0000, 0x0000, 0x060e2b3401040101),
            optional: false,
            unique: false,
        }],
    },
    ClassRow {
        auid: id(0x0d010101, 0x0101, 0x4500, 0x060e2b3402060101),
        name: "DescriptiveClip",
        parent: id(0x0d010101, 0x0101, 0x1100, 0x060e2b3402060101),
        concrete: true,
        props: &[PropRow {
            pid: 0x6103,
            auid: id(0x01070106, 0x0000, 0x0000, 0x060e2b3401010105),
            name: "DescribedSlotIDs",
            type_auid: id(0x04030200, 0x0000, 0x0000, 0x060e2b3401040101),
            optional: true,
            unique: false,
        }],
    },
    ClassRow {
        auid: id(0x0d010101, 0x0101, 0x4700, 0x060e2b3402060101),
        name: "AES3PCMDescriptor",
        parent: id(0x0d010101, 0x0101, 0x4800, 0x060e2b3402060101),
        concrete: true,
        props: &[
            PropRow {
                pid: 0x3d0d,
                auid: id(0x04020501, 0x0600, 0x0000, 0x060e2b3401010105),
                name: "Emphasis",
                type_auid: id(0x02010123, 0x0000, 0x0000, 0x060e2b3401040101),
                optional: true,
                unique: false,
            },
            PropRow {
                pid: 0x3d0f,
                auid: id(0x04020302, 0x0300, 0x0000, 0x060e2b3401010105),
                name: "BlockStartOffset",
                type_auid: id(0x01010200, 0x0000, 0x0000, 0x060e2b3401040101),
                optional: true,
                unique: false,
            },
            PropRow {
                pid: 0x3d08,
                auid: id(0x04020501, 0x0100, 0x0000, 0x060e2b3401010105),
                name: "AuxBitsMode",
                type_auid: id(0x02010124, 0x0000, 0x0000, 0x060e2b3401040101),
                optional: true,
                unique: false,
            },
            PropRow {
                pid: 0x3d10,
                auid: id(0x04020501, 0x0200, 0x0000, 0x060e2b3401010105),
                name: "ChannelStatusMode",
                type_auid: id(0x04010a00, 0x0000, 0x0000, 0x060e2b3401040101),
                optional: true,
                unique: false,
            },
            PropRow {
                pid: 0x3d11,
                auid: id(0x04020501, 0x0300, 0x0000, 0x060e2b3401010105),
                name: "FixedChannelStatusData",
                type_auid: id(0x04010100, 0x0000, 0x0000, 0x060e2b3401040101),
                optional: true,
                unique: false,
            },
            PropRow {
                pid: 0x3d12,
                auid: id(0x04020501, 0x0400, 0x0000, 0x060e2b3401010105),
                name: "UserDataMode",
                type_auid: id(0x04010b00, 0x0000, 0x0000, 0x060e2b3401040101),
                optional: true,
                unique: false,
            },
            PropRow {
                pid: 0x3d13,
                auid: id(0x04020501, 0x0500, 0x0000, 0x060e2b3401010105),
                name: "FixedUserData",
                type_auid: id(0x04010100, 0x0000, 0x0000, 0x060e2b3401040101),
                optional: true,
                unique: false,
            },
        ],
    },
    ClassRow {
        auid: id(0x0d010101, 0x0101, 0x4800, 0x060e2b3402060101),
        name: "PCMDescriptor",
        parent: id(0x0d010101, 0x0101, 0x4200, 0x060e2b3402060101),
        concrete: true,
        props: &[
            PropRow {
                pid: 0x3d0a,
                auid: id(0x04020302, 0x0100, 0x0000, 0x060e2b3401010105),
                name: "BlockAlign",
                type_auid: id(0x01010200, 0x0000, 0x0000, 0x060e2b3401040101),
                optional: false,
                unique: false,
            },
            PropRow {
                pid: 0x3d0b,
                auid: id(0x04020302, 0x0200, 0x0000, 0x060e2b3401010105),
                name: "SequenceOffset",
                type_auid: id(0x01010100, 0x0000, 0x0000, 0x060e2b3401040101),
                optional: true,
                unique: false,
            },
            PropRow {
                pid: 0x3d09,
                auid: id(0x04020303, 0x0500, 0x0000, 0x060e2b3401010105),
                name: "AverageBPS",
                type_auid: id(0x01010300, 0x0000, 0x0000, 0x060e2b3401040101),
                optional: false,
                unique: false,
            },
            PropRow {
                pid: 0x3d32,
                auid: id(0x04020101, 0x0500, 0x0000, 0x060e2b3401010107),
                name: "ChannelAssignment",
                type_auid: id(0x01030100, 0x0000, 0x0000, 0x060e2b3401040101),
                optional: true,
                unique: false,
            },
            PropRow {
                pid: 0x3d29,
                auid: id(0x04020301, 0x0600, 0x0000, 0x060e2b3401010108),
                name: "PeakEnvelopeVersion",
                type_auid: id(0x01010300, 0x0000, 0x0000, 0x060e2b3401040101),
                optional: true,
                unique: false,
            },
            PropRow {
                pid: 0x3d2a,
                auid: id(0x04020301, 0x0700, 0x0000, 0x060e2b3401010108),
                name: "PeakEnvelopeFormat",
                type_auid: id(0x01010300, 0x0000, 0x0000, 0x060e2b3401040101),
                optional: true,
                unique: false,
            },
            PropRow {
                pid: 0x3d2b,
                auid: id(0x04020301, 0x0800, 0x0000, 0x060e2b3401010108),
                name: "PointsPerPeakValue",
                type_auid: id(0x01010300, 0x0000, 0x0000, 0x060e2b3401040101),
                optional: true,
                unique: false,
            },
            PropRow {
                pid: 0x3d2c,
                auid: id(0x04020301, 0x0900, 0x0000, 0x060e2b3401010108),
                name: "PeakEnvelopeBlockSize",
                type_auid: id(0x01010300, 0x0000, 0x0000, 0x060e2b3401040101),
                optional: true,
                unique: false,
            },
            PropRow {
                pid: 0x3d2d,
                auid: id(0x04020301, 0x0a00, 0x0000, 0x060e2b3401010108),
                name: "PeakChannels",
                type_auid: id(0x01010300, 0x0000, 0x0000, 0x060e2b3401040101),
                optional: true,
                unique: false,
            },
            PropRow {
                pid: 0x3d2e,
                auid: id(0x04020301, 0x0b00, 0x0000, 0x060e2b3401010108),
                name: "PeakFrames",
                type_auid: id(0x01010300, 0x0000, 0x0000, 0x060e2b3401040101),
                optional: true,
                unique: false,
            },
            PropRow {
                pid: 0x3d2f,
                auid: id(0x04020301, 0x0c00, 0x0000, 0x060e2b3401010108),
                name: "PeakOfPeaksPosition",
                type_auid: id(0x01012001, 0x0000, 0x0000, 0x060e2b3401040101),
                optional: true,
                unique: false,
            },
            PropRow {
                pid: 0x3d30,
                auid: id(0x04020301, 0x0d00, 0x0000, 0x060e2b3401010108),
                name: "PeakEnvelopeTimestamp",
                type_auid: id(0x03010700, 0x0000, 0x0000, 0x060e2b3401040101),
                optional: true,
                unique: false,
            },
            PropRow {
                pid: 0x3d31,
                auid: id(0x04020301, 0x0e00, 0x0000, 0x060e2b3401010108),
                name: "PeakEnvelopeData",
                type_auid: id(0x04100200, 0x0000, 0x0000, 0x060e2b3401040101),
                optional: true,
                unique: false,
            },
        ],
    },
    ClassRow {
        auid: id(0x0d010101, 0x0101, 0x4900, 0x060e2b3402060101),
        name: "PhysicalDescriptor",
        parent: id(0x0d010101, 0x0101, 0x2400, 0x060e2b3402060101),
        concrete: false,
        props: &[],
    },
    ClassRow {
        auid: id(0x0d010101, 0x0101, 0x4a00, 0x060e2b3402060101),
        name: "ImportDescriptor",
        parent: id(0x0d010101, 0x0101, 0x4900, 0x060e2b3402060101),
        concrete: true,
        props: &[],
    },
    ClassRow {
        auid: id(0x0d010101, 0x0101, 0x4b00, 0x060e2b3402060101),
        name: "RecordingDescriptor",
        parent: id(0x0d010101, 0x0101, 0x4900, 0x060e2b3402060101),
        concrete: true,
        props: &[],
    },
    ClassRow {
        auid: id(0x0d010101, 0x0101, 0x4c00, 0x060e2b3402060101),
        name: "TaggedValueDefinition",
        parent: id(0x0d010101, 0x0101, 0x1a00, 0x060e2b3402060101),
        concrete: true,
        props: &[],
    },
    ClassRow {
        auid: id(0x0d010101, 0x0101, 0x4d00, 0x060e2b3402060101),
        name: "KLVDataDefinition",
        parent: id(0x0d010101, 0x0101, 0x1a00, 0x060e2b3402060101),
        concrete: true,
        props: &[PropRow {
            pid: 0x4d12,
            auid: id(0x06010104, 0x0109, 0x0000, 0x060e2b3401010107),
            name: "KLVDataType",
            type_auid: id(0x05010900, 0x0000, 0x0000, 0x060e2b3401040101),
            optional: true,
            unique: false,
        }],
    },
    ClassRow {
        auid: id(0x0d010101, 0x0101, 0x4e00, 0x060e2b3402060101),
        name: "AuxiliaryDescriptor",
        parent: id(0x0d010101, 0x0101, 0x4900, 0x060e2b3402060101),
        concrete: true,
        props: &[
            PropRow {
                pid: 0x4e11,
                auid: id(0x04090201, 0x0000, 0x0000, 0x060e2b3401010107),
                name: "MimeType",
                type_auid: id(0x01100200, 0x0000, 0x0000, 0x060e2b3401040101),
                optional: false,
                unique: false,
            },
            PropRow {
                pid: 0x4e12,
                auid: id(0x04090300, 0x0000, 0x0000, 0x060e2b3401010108),
                name: "CharSet",
                type_auid: id(0x01100200, 0x0000, 0x0000, 0x060e2b3401040101),
                optional: true,
                unique: false,
            },
        ],
    },
    ClassRow {
        auid: id(0x0d010101, 0x0101, 0x4f00, 0x060e2b3402060101),
        name: "RIFFChunk",
        parent: id(0x0d010101, 0x0101, 0x0100, 0x060e2b3402060101),
        concrete: true,
        props: &[
            PropRow {
                pid: 0x4f01,
                auid: id(0x04060802, 0x0000, 0x0000, 0x060e2b3401010108),
                name: "ChunkID",
                type_auid: id(0x01010300, 0x0000, 0x0000, 0x060e2b3401040101),
                optional: false,
                unique: false,
            },
            PropRow {
                pid: 0x4f02,
                auid: id(0x04060903, 0x0000, 0x0000, 0x060e2b3401010108),
                name: "ChunkLength",
                type_auid: id(0x01010300, 0x0000, 0x0000, 0x060e2b3401040101),
                optional: false,
                unique: false,
            },
            PropRow {
                pid: 0x4f03,
                auid: id(0x04070400, 0x0000, 0x0000, 0x060e2b3401010108),
                name: "ChunkData",
                type_auid: id(0x04100200, 0x0000, 0x0000, 0x060e2b3401040101),
                optional: false,
                unique: false,
            },
        ],
    },
    ClassRow {
        auid: id(0x0d010101, 0x0101, 0x5000, 0x060e2b3402060101),
        name: "BWFImportDescriptor",
        parent: id(0x0d010101, 0x0101, 0x4a00, 0x060e2b3402060101),
        concrete: true,
        props: &[
            PropRow {
                pid: 0x3d15,
                auid: id(0x04020302, 0x0500, 0x0000, 0x060e2b3401010105),
                name: "QltyFileSecurityReport",
                type_auid: id(0x01010300, 0x0000, 0x0000, 0x060e2b3401040101),
                optional: true,
                unique: false,
            },
            PropRow {
                pid: 0x3d16,
                auid: id(0x04020302, 0x0600, 0x0000, 0x060e2b3401010105),
                name: "QltyFileSecurityWave",
                type_auid: id(0x01010300, 0x0000, 0x0000, 0x060e2b3401040101),
                optional: true,
                unique: false,
            },
            PropRow {
                pid: 0x3d21,
                auid: id(0x04020502, 0x0101, 0x0000, 0x060e2b3401010105),
                name: "BextCodingHistory",
                type_auid: id(0x01100200, 0x0000, 0x0000, 0x060e2b3401040101),
                optional: true,
                unique: false,
            },
            PropRow {
                pid: 0x3d22,
                auid: id(0x04020502, 0x0201, 0x0000, 0x060e2b3401010105),
                name: "QltyBasicData",
                type_auid: id(0x01100200, 0x0000, 0x0000, 0x060e2b3401040101),
                optional: true,
                unique: false,
            },
            PropRow {
                pid: 0x3d23,
                auid: id(0x04020502, 0x0301, 0x0000, 0x060e2b3401010105),
                name: "QltyStartOfModulation",
                type_auid: id(0x01100200, 0x0000, 0x0000, 0x060e2b3401040101),
                optional: true,
                unique: false,
            },
            PropRow {
                pid: 0x3d24,
                auid: id(0x04020502, 0x0401, 0x0000, 0x060e2b3401010105),
                name: "QltyQualityEvent",
                type_auid: id(0x01100200, 0x0000, 0x0000, 0x060e2b3401040101),
                optional: true,
                unique: false,
            },
            PropRow {
                pid: 0x3d25,
                auid: id(0x04020502, 0x0501, 0x0000, 0x060e2b3401010105),
                name: "QltyEndOfModulation",
                type_auid: id(0x01100200, 0x0000, 0x0000, 0x060e2b3401040101),
                optional: true,
                unique: false,
            },
            PropRow {
                pid: 0x3d26,
                auid: id(0x04020502, 0x0601, 0x0000, 0x060e2b3401010105),
                name: "QltyQualityParameter",
                type_auid: id(0x01100200, 0x0000, 0x0000, 0x060e2b3401040101),
                optional: true,
                unique: false,
            },
            PropRow {
                pid: 0x3d27,
                auid: id(0x04020502, 0x0701, 0x0000, 0x060e2b3401010105),
                name: "QltyOperatorComment",
                type_auid: id(0x01100200, 0x0000, 0x0000, 0x060e2b3401040101),
                optional: true,
                unique: false,
            },
            PropRow {
                pid: 0x3d28,
                auid: id(0x04020502, 0x0801, 0x0000, 0x060e2b3401010105),
                name: "QltyCueSheet",
                type_auid: id(0x01100200, 0x0000, 0x0000, 0x060e2b3401040101),
                optional: true,
                unique: false,
            },
            PropRow {
                pid: 0x3d33,
                auid: id(0x06010104, 0x060f, 0x0000, 0x060e2b3401010108),
                name: "UnknownBWFChunks",
                type_auid: id(0x05060c00, 0x0000, 0x0000, 0x060e2b3401040101),
                optional: true,
                unique: false,
            },
        ],
    },
    ClassRow {
        auid: id(0x0d010101, 0x0101, 0x5100, 0x060e2b3402060101),
        name: "MPEGVideoDescriptor",
        parent: id(0x0d010101, 0x0101, 0x2800, 0x060e2b3402060101),
        concrete: true,
        props: &[
            PropRow {
                pid: 0xfffe,
                auid: id(0x04010602, 0x0102, 0x0000, 0x060e2b3401010105),
                name: "SingleSequence",
                type_auid: id(0x01040100, 0x0000, 0x0000, 0x060e2b3401040101),
                optional: true,
                unique: false,
            },
            PropRow {
                pid: 0xfffd,
                auid: id(0x04010602, 0x0103, 0x0000, 0x060e2b3401010105),
                name: "ConstantBPictureCount",
                type_auid: id(0x01040100, 0x0000, 0x0000, 0x060e2b3401040101),
                optional: true,
                unique: false,
            },
            PropRow {
                pid: 0xfffc,
                auid: id(0x04010602, 0x0104, 0x0000, 0x060e2b3401010105),
                name: "CodedContentScanning",
                type_auid: id(0x0201012a, 0x0000, 0x0000, 0x060e2b3401040101),
                optional: true,
                unique: false,
            },
            PropRow {
                pid: 0xfffb,
                auid: id(0x04010602, 0x0105, 0x0000, 0x060e2b3401010105),
                name: "LowDelay",
                type_auid: id(0x01040100, 0x0000, 0x0000, 0x060e2b3401040101),
                optional: true,
                unique: false,
            },
            PropRow {
                pid: 0xfffa,
                auid: id(0x04010602, 0x0106, 0x0000, 0x060e2b3401010105),
                name: "ClosedGOP",
                type_auid: id(0x01040100, 0x0000, 0x0000, 0x060e2b3401040101),
                optional: true,
                unique: false,
            },
            PropRow {
                pid: 0xfff9,
                auid: id(0x04010602, 0x0107, 0x0000, 0x060e2b3401010105),
                name: "IdenticalGOP",
                type_auid: id(0x01040100, 0x0000, 0x0000, 0x060e2b3401040101),
                optional: true,
                unique: false,
            },
            PropRow {
                pid: 0xfff8,
                auid: id(0x04010602, 0x0108, 0x0000, 0x060e2b3401010105),
                name: "MaxGOP",
                type_auid: id(0x01010200, 0x0000, 0x0000, 0x060e2b3401040101),
                optional: true,
                unique: false,
            },
            PropRow {
                pid: 0xfff7,
                auid: id(0x04010602, 0x0109, 0x0000, 0x060e2b3401010105),
                name: "MaxBPictureCount",
                type_auid: id(0x01010200, 0x0000, 0x0000, 0x060e2b3401040101),
                optional: true,
                unique: false,
            },
            PropRow {
                pid: 0xfff6,
                auid: id(0x04010602, 0x010b, 0x0000, 0x060e2b3401010105),
                name: "BitRate",
                type_auid: id(0x01010300, 0x0000, 0x0000, 0x060e2b3401040101),
                optional: true,
                unique: false,
            },
            PropRow {
                pid: 0xfff5,
                auid: id(0x04010602, 0x010a, 0x0000, 0x060e2b3401010105),
                name: "ProfileAndLevel",
                type_auid: id(0x01010100, 0x0000, 0x0000, 0x060e2b3401040101),
                optional: true,
                unique: false,
            },
        ],
    },
    ClassRow {
        auid: id(0x0d010101, 0x0201, 0x0000, 0x060e2b3402060101),
        name: "ClassDefinition",
        parent: id(0x0d010101, 0x0224, 0x0000, 0x060e2b3402060101),
        concrete: true,
        props: &[
            PropRow {
                pid: 0x0008,
                auid: id(0x06010107, 0x0100, 0x0000, 0x060e2b3401010102),
                name: "ParentClass",
                type_auid: id(0x05010100, 0x0000, 0x0000, 0x060e2b3401040101),
                optional: false,
                unique: false,
            },
            PropRow {
                pid: 0x0009,
                auid: id(0x06010107, 0x0200, 0x0000, 0x060e2b3401010102),
                name: "Properties",
                type_auid: id(0x05050b00, 0x0000, 0x0000, 0x060e2b3401040101),
                optional: true,
                unique: false,
            },
            PropRow {
                pid: 0x000a,
                auid: id(0x06010107, 0x0300, 0x0000, 0x060e2b3401010102),
                name: "IsConcrete",
                type_auid: id(0x01040100, 0x0000, 0x0000, 0x060e2b3401040101),
                optional: false,
                unique: false,
            },
        ],
    },
    ClassRow {
        auid: id(0x0d010101, 0x0202, 0x0000, 0x060e2b3402060101),
        name: "PropertyDefinition",
        parent: id(0x0d010101, 0x0224, 0x0000, 0x060e2b3402060101),
        concrete: true,
        props: &[
            PropRow {
                pid: 0x000b,
                auid: id(0x06010107, 0x0400, 0x0000, 0x060e2b3401010102),
                name: "Type",
                type_auid: id(0x01030100, 0x0000, 0x0000, 0x060e2b3401040101),
                optional: false,
                unique: false,
            },
            PropRow {
                pid: 0x000c,
                auid: id(0x03010202, 0x0100, 0x0000, 0x060e2b3401010102),
                name: "IsOptional",
                type_auid: id(0x01040100, 0x0000, 0x0000, 0x060e2b3401040101),
                optional: false,
                unique: false,
            },
            PropRow {
                pid: 0x000d,
                auid: id(0x06010107, 0x0500, 0x0000, 0x060e2b3401010102),
                name: "LocalIdentification",
                type_auid: id(0x01010200, 0x0000, 0x0000, 0x060e2b3401040101),
                optional: false,
                unique: false,
            },
            PropRow {
                pid: 0x000e,
                auid: id(0x06010107, 0x0600, 0x0000, 0x060e2b3401010102),
                name: "IsUniqueIdentifier",
                type_auid: id(0x01040100, 0x0000, 0x0000, 0x060e2b3401040101),
                optional: true,
                unique: false,
            },
        ],
    },
    ClassRow {
        auid: id(0x0d010101, 0x0203, 0x0000, 0x060e2b3402060101),
        name: "TypeDefinition",
        parent: id(0x0d010101, 0x0224, 0x0000, 0x060e2b3402060101),
        concrete: false,
        props: &[],
    },
    ClassRow {
        auid: id(0x0d010101, 0x0204, 0x0000, 0x060e2b3402060101),
        name: "TypeDefinitionInteger",
        parent: id(0x0d010101, 0x0203, 0x0000, 0x060e2b3402060101),
        concrete: true,
        props: &[
            PropRow {
                pid: 0x000f,
                auid: id(0x03010203, 0x0100, 0x0000, 0x060e2b3401010102),
                name: "Size",
                type_auid: id(0x01010100, 0x0000, 0x0000, 0x060e2b3401040101),
                optional: false,
                unique: false,
            },
            PropRow {
                pid: 0x0010,
                auid: id(0x03010203, 0x0200, 0x0000, 0x060e2b3401010102),
                name: "IsSigned",
                type_auid: id(0x01040100, 0x0000, 0x0000, 0x060e2b3401040101),
                optional: false,
                unique: false,
            },
        ],
    },
    ClassRow {
        auid: id(0x0d010101, 0x0205, 0x0000, 0x060e2b3402060101),
        name: "TypeDefinitionStrongObjectReference",
        parent: id(0x0d010101, 0x0203, 0x0000, 0x060e2b3402060101),
        concrete: true,
        props: &[PropRow {
            pid: 0x0011,
            auid: id(0x06010107, 0x0900, 0x0000, 0x060e2b3401010102),
            name: "ReferencedType",
            type_auid: id(0x05010100, 0x0000, 0x0000, 0x060e2b3401040101),
            optional: false,
            unique: false,
        }],
    },
    ClassRow {
        auid: id(0x0d010101, 0x0206, 0x0000, 0x060e2b3402060101),
        name: "TypeDefinitionWeakObjectReference",
        parent: id(0x0d010101, 0x0203, 0x0000, 0x060e2b3402060101),
        concrete: true,
        props: &[
            PropRow {
                pid: 0x0012,
                auid: id(0x06010107, 0x0a00, 0x0000, 0x060e2b3401010102),
                name: "ReferencedType",
                type_auid: id(0x05010100, 0x0000, 0x0000, 0x060e2b3401040101),
                optional: false,
                unique: false,
            },
            PropRow {
                pid: 0x0013,
                auid: id(0x03010203, 0x0b00, 0x0000, 0x060e2b3401010102),
                name: "TargetSet",
                type_auid: id(0x04010600, 0x0000, 0x0000, 0x060e2b3401040101),
                optional: false,
                unique: false,
            },
        ],
    },
    ClassRow {
        auid: id(0x0d010101, 0x0207, 0x0000, 0x060e2b3402060101),
        name: "TypeDefinitionEnumeration",
        parent: id(0x0d010101, 0x0203, 0x0000, 0x060e2b3402060101),
        concrete: true,
        props: &[
            PropRow {
                pid: 0x0014,
                auid: id(0x06010107, 0x0b00, 0x0000, 0x060e2b3401010102),
                name: "ElementType",
                type_auid: id(0x05010900, 0x0000, 0x0000, 0x060e2b3401040101),
                optional: false,
                unique: false,
            },
            PropRow {
                pid: 0x0015,
                auid: id(0x03010203, 0x0400, 0x0000, 0x060e2b3401010102),
                name: "ElementNames",
                type_auid: id(0x04010500, 0x0000, 0x0000, 0x060e2b3401040101),
                optional: false,
                unique: false,
            },
            PropRow {
                pid: 0x0016,
                auid: id(0x03010203, 0x0500, 0x0000, 0x060e2b3401010102),
                name: "ElementValues",
                type_auid: id(0x04010400, 0x0000, 0x0000, 0x060e2b3401040101),
                optional: false,
                unique: false,
            },
        ],
    },
    ClassRow {
        auid: id(0x0d010101, 0x0101, 0x5900, 0x060e2b3402060101),
        name: "SubDescriptor",
        parent: id(0x0d010101, 0x0101, 0x0100, 0x060e2b3402060101),
        concrete: false,
        props: &[],
    },
    ClassRow {
        auid: id(0x0d010101, 0x0101, 0x5a00, 0x060e2b3402060101),
        name: "JPEG2000SubDescriptor",
        parent: id(0x0d010101, 0x0101, 0x5900, 0x060e2b3402060101),
        concrete: true,
        props: &[
            PropRow {
                pid: 0xfff4,
                auid: id(0x04010603, 0x0100, 0x0000, 0x060e2b340101010a),
                name: "Rsiz",
                type_auid: id(0x01010200, 0x0000, 0x0000, 0x060e2b3401040101),
                optional: false,
                unique: false,
            },
            PropRow {
                pid: 0xfff3,
                auid: id(0x04010603, 0x0200, 0x0000, 0x060e2b340101010a),
                name: "Xsiz",
                type_auid: id(0x01010300, 0x0000, 0x0000, 0x060e2b3401040101),
                optional: false,
                unique: false,
            },
            PropRow {
                pid: 0xfff2,
                auid: id(0x04010603, 0x0300, 0x0000, 0x060e2b340101010a),
                name: "Ysiz",
                type_auid: id(0x01010300, 0x0000, 0x0000, 0x060e2b3401040101),
                optional: false,
                unique: false,
            },
            PropRow {
                pid: 0xfff1,
                auid: id(0x04010603, 0x0400, 0x0000, 0x060e2b340101010a),
                name: "XOsiz",
                type_auid: id(0x01010300, 0x0000, 0x0000, 0x060e2b3401040101),
                optional: false,
                unique: false,
            },
            PropRow {
                pid: 0xfff0,
                auid: id(0x04010603, 0x0500, 0x0000, 0x060e2b340101010a),
                name: "YOsiz",
                type_auid: id(0x01010300, 0x0000, 0x0000, 0x060e2b3401040101),
                optional: false,
                unique: false,
            },
            PropRow {
                pid: 0xffef,
                auid: id(0x04010603, 0x0600, 0x0000, 0x060e2b340101010a),
                name: "XTsiz",
                type_auid: id(0x01010300, 0x0000, 0x0000, 0x060e2b3401040101),
                optional: false,
                unique: false,
            },
            PropRow {
                pid: 0xffee,
                auid: id(0x04010603, 0x0700, 0x0000, 0x060e2b340101010a),
                name: "YTsiz",
                type_auid: id(0x01010300, 0x0000, 0x0000, 0x060e2b3401040101),
                optional: false,
                unique: false,
            },
            PropRow {
                pid: 0xffed,
                auid: id(0x04010603, 0x0800, 0x0000, 0x060e2b340101010a),
                name: "XTOsiz",
                type_auid: id(0x01010300, 0x0000, 0x0000, 0x060e2b3401040101),
                optional: false,
                unique: false,
            },
            PropRow {
                pid: 0xffec,
                auid: id(0x04010603, 0x0900, 0x0000, 0x060e2b340101010a),
                name: "YTOsiz",
                type_auid: id(0x01010300, 0x0000, 0x0000, 0x060e2b3401040101),
                optional: false,
                unique: false,
            },
            PropRow {
                pid: 0xffeb,
                auid: id(0x04010603, 0x0a00, 0x0000, 0x060e2b340101010a),
                name: "Csiz",
                type_auid: id(0x01010200, 0x0000, 0x0000, 0x060e2b3401040101),
                optional: false,
                unique: false,
            },
            PropRow {
                pid: 0xffea,
                auid: id(0x04010603, 0x0b00, 0x0000, 0x060e2b340101010a),
                name: "PictureComponentSizing",
                type_auid: id(0x04010100, 0x0000, 0x0000, 0x060e2b3401040101),
                optional: false,
                unique: false,
            },
            PropRow {
                pid: 0xffe9,
                auid: id(0x04010603, 0x0c00, 0x0000, 0x060e2b340101010a),
                name: "CodingStyleDefault",
                type_auid: id(0x04100100, 0x0000, 0x0000, 0x060e2b3401040101),
                optional: true,
                unique: false,
            },
            PropRow {
                pid: 0xffe8,
                auid: id(0x04010603, 0x0d00, 0x0000, 0x060e2b340101010a),
                name: "QuantizationDefault",
                type_auid: id(0x04100100, 0x0000, 0x0000, 0x060e2b3401040101),
                optional: true,
                unique: false,
            },
            PropRow {
                pid: 0xffe7,
                auid: id(0x04010603, 0x0e00, 0x0000, 0x060e2b340101010e),
                name: "J2CLayout",
                type_auid: id(0x04020100, 0x0000, 0x0000, 0x060e2b3401040101),
                optional: true,
                unique: false,
            },
        ],
    },
    ClassRow {
        auid: id(0x0d010101, 0x0101, 0x5b00, 0x060e2b3402060101),
        name: "VBIDataDescriptor",
        parent: id(0x0d010101, 0x0101, 0x4300, 0x060e2b3402060101),
        concrete: true,
        props: &[],
    },
    ClassRow {
        auid: id(0x0d010101, 0x0101, 0x5c00, 0x060e2b3402060101),
        name: "ANCDataDescriptor",
        parent: id(0x0d010101, 0x0101, 0x4300, 0x060e2b3402060101),
        concrete: true,
        props: &[],
    },
    ClassRow {
        auid: id(0x0d010101, 0x0101, 0x6700, 0x060e2b3402060101),
        name: "ContainerConstraintsSubDescriptor",
        parent: id(0x0d010101, 0x0101, 0x5900, 0x060e2b3402060101),
        concrete: true,
        props: &[],
    },
    ClassRow {
        auid: id(0x0d010101, 0x0101, 0x6800, 0x060e2b3402060101),
        name: "MPEG4VisualSubDescriptor",
        parent: id(0x0d010101, 0x0101, 0x5900, 0x060e2b3402060101),
        concrete: true,
        props: &[
            PropRow {
                pid: 0xffe6,
                auid: id(0x04010602, 0x0202, 0x0000, 0x060e2b340101010d),
                name: "MPEG4VisualSingleSequence",
                type_auid: id(0x01040100, 0x0000, 0x0000, 0x060e2b3401040101),
                optional: true,
                unique: false,
            },
            PropRow {
                pid: 0xffe5,
                auid: id(0x04010602, 0x0203, 0x0000, 0x060e2b340101010d),
                name: "MPEG4VisualConstantBVOPs",
                type_auid: id(0x01040100, 0x0000, 0x0000, 0x060e2b3401040101),
                optional: true,
                unique: false,
            },
            PropRow {
                pid: 0xffe4,
                auid: id(0x04010602, 0x0204, 0x0000, 0x060e2b340101010d),
                name: "MPEG4VisualCodedContentType",
                type_auid: id(0x0201012d, 0x0000, 0x0000, 0x060e2b3401040101),
                optional: true,
                unique: false,
            },
            PropRow {
                pid: 0xffe3,
                auid: id(0x04010602, 0x0205, 0x0000, 0x060e2b340101010d),
                name: "MPEG4VisualLowDelay",
                type_auid: id(0x01040100, 0x0000, 0x0000, 0x060e2b3401040101),
                optional: true,
                unique: false,
            },
            PropRow {
                pid: 0xffe2,
                auid: id(0x04010602, 0x0206, 0x0000, 0x060e2b340101010d),
                name: "MPEG4VisualClosedGOV",
                type_auid: id(0x01040100, 0x0000, 0x0000, 0x060e2b3401040101),
                optional: true,
                unique: false,
            },
            PropRow {
                pid: 0xffe1,
                auid: id(0x04010602, 0x0207, 0x0000, 0x060e2b340101010d),
                name: "MPEG4VisualIdenticalGOV",
                type_auid: id(0x01040100, 0x0000, 0x0000, 0x060e2b3401040101),
                optional: true,
                unique: false,
            },
            PropRow {
                pid: 0xffe0,
                auid: id(0x04010602, 0x0208, 0x0000, 0x060e2b340101010d),
                name: "MPEG4VisualMaxGOV",
                type_auid: id(0x01010200, 0x0000, 0x0000, 0x060e2b3401040101),
                optional: true,
                unique: false,
            },
            PropRow {
                pid: 0xffdf,
                auid: id(0x04010602, 0x0209, 0x0000, 0x060e2b340101010d),
                name: "MPEG4VisualBVOPCount",
                type_auid: id(0x01010200, 0x0000, 0x0000, 0x060e2b3401040101),
                optional: true,
                unique: false,
            },
            PropRow {
                pid: 0xffde,
                auid: id(0x04010602, 0x020a, 0x0000, 0x060e2b340101010d),
                name: "MPEG4VisualProfileAndLevel",
                type_auid: id(0x01010100, 0x0000, 0x0000, 0x060e2b3401040101),
                optional: true,
                unique: false,
            },
            PropRow {
                pid: 0xffdd,
                auid: id(0x04010602, 0x020b, 0x0000, 0x060e2b340101010d),
                name: "MPEG4VisualBitRate",
                type_auid: id(0x01010300, 0x0000, 0x0000, 0x060e2b3401040101),
                optional: true,
                unique: false,
            },
        ],
    },
    ClassRow {
        auid: id(0x0d010101, 0x0101, 0x6a00, 0x060e2b3402060101),
        name: "MCALabelSubDescriptor",
        parent: id(0x0d010101, 0x0101, 0x5900, 0x060e2b3402060101),
        concrete: false,
        props: &[
            PropRow {
                pid: 0xffdc,
                auid: id(0x01030701, 0x0100, 0x0000, 0x060e2b340101010e),
                name: "MCALabelDictionaryID",
                type_auid: id(0x01030100, 0x0000, 0x0000, 0x060e2b3401040101),
                optional: false,
                unique: false,
            },
            PropRow {
                pid: 0xffdb,
                auid: id(0x01030701, 0x0500, 0x0000, 0x060e2b340101010e),
                name: "MCALinkID",
                type_auid: id(0x01030300, 0x0000, 0x0000, 0x060e2b3401040101),
                optional: false,
                unique: false,
            },
            PropRow {
                pid: 0xffda,
                auid: id(0x01030701, 0x0200, 0x0000, 0x060e2b340101010e),
                name: "MCATagSymbol",
                type_auid: id(0x01100200, 0x0000, 0x0000, 0x060e2b3401040101),
                optional: false,
                unique: false,
            },
            PropRow {
                pid: 0xffd9,
                auid: id(0x01030701, 0x0300, 0x0000, 0x060e2b340101010e),
                name: "MCATagName",
                type_auid: id(0x01100200, 0x0000, 0x0000, 0x060e2b3401040101),
                optional: true,
                unique: false,
            },
            PropRow {
                pid: 0xffd8,
                auid: id(0x0103040a, 0x0000, 0x0000, 0x060e2b340101010e),
                name: "MCAChannelID",
                type_auid: id(0x01010300, 0x0000, 0x0000, 0x060e2b3401040101),
                optional: true,
                unique: false,
            },
            PropRow {
                pid: 0xffd7,
                auid: id(0x03010102, 0x0315, 0x0000, 0x060e2b340101010d),
                name: "RFC5646SpokenLanguage",
                type_auid: id(0x01100400, 0x0000, 0x0000, 0x060e2b3401040101),
                optional: true,
                unique: false,
            },
            PropRow {
                pid: 0xffd6,
                auid: id(0x01051000, 0x0000, 0x0000, 0x060e2b340101010e),
                name: "MCATitle",
                type_auid: id(0x01100200, 0x0000, 0x0000, 0x060e2b3401040101),
                optional: true,
                unique: false,
            },
            PropRow {
                pid: 0xffd5,
                auid: id(0x01051100, 0x0000, 0x0000, 0x060e2b340101010e),
                name: "MCATitleVersion",
                type_auid: id(0x01100200, 0x0000, 0x0000, 0x060e2b3401040101),
                optional: true,
                unique: false,
            },
            PropRow {
                pid: 0xffd4,
                auid: id(0x01051200, 0x0000, 0x0000, 0x060e2b340101010e),
                name: "MCATitleSubVersion",
                type_auid: id(0x01100200, 0x0000, 0x0000, 0x060e2b3401040101),
                optional: true,
                unique: false,
            },
            PropRow {
                pid: 0xffd3,
                auid: id(0x01051300, 0x0000, 0x0000, 0x060e2b340101010e),
                name: "MCAEpisode",
                type_auid: id(0x01100200, 0x0000, 0x0000, 0x060e2b3401040101),
                optional: true,
                unique: false,
            },
            PropRow {
                pid: 0xffd2,
                auid: id(0x01040105, 0x0000, 0x0000, 0x060e2b340101010e),
                name: "MCAPartitionKind",
                type_auid: id(0x01100200, 0x0000, 0x0000, 0x060e2b3401040101),
                optional: true,
                unique: false,
            },
            PropRow {
                pid: 0xffd1,
                auid: id(0x01040106, 0x0000, 0x0000, 0x060e2b340101010e),
                name: "MCAPartitionNumber",
                type_auid: id(0x01100200, 0x0000, 0x0000, 0x060e2b3401040101),
                optional: true,
                unique: false,
            },
            PropRow {
                pid: 0xffd0,
                auid: id(0x03020102, 0x2000, 0x0000, 0x060e2b340101010e),
                name: "MCAAudioContentKind",
                type_auid: id(0x01100200, 0x0000, 0x0000, 0x060e2b3401040101),
                optional: true,
                unique: false,
            },
            PropRow {
                pid: 0xffcf,
                auid: id(0x03020102, 0x2100, 0x0000, 0x060e2b340101010e),
                name: "MCAAudioElementKind",
                type_auid: id(0x01100200, 0x0000, 0x0000, 0x060e2b3401040101),
                optional: true,
                unique: false,
            },
        ],
    },
    ClassRow {
        auid: id(0x0d010101, 0x0101, 0x6b00, 0x060e2b3402060101),
        name: "AudioChannelLabelSubDescriptor",
        parent: id(0x0d010101, 0x0101, 0x6a00, 0x060e2b3402060101),
        concrete: true,
        props: &[PropRow {
            pid: 0xffce,
            auid: id(0x01030701, 0x0600, 0x0000, 0x060e2b340101010e),
            name: "SoundfieldGroupLinkID",
            type_auid: id(0x01030300, 0x0000, 0x0000, 0x060e2b3401040101),
            optional: true,
            unique: false,
        }],
    },
    ClassRow {
        auid: id(0x0d010101, 0x0101, 0x6c00, 0x060e2b3402060101),
        name: "SoundfieldGroupLabelSubDescriptor",
        parent: id(0x0d010101, 0x0101, 0x6a00, 0x060e2b3402060101),
        concrete: true,
        props: &[PropRow {
            pid: 0xffcd,
            auid: id(0x01030701, 0x0400, 0x0000, 0x060e2b340101010e),
            name: "GroupOfSoundfieldGroupsLinkID",
            type_auid: id(0x04011500, 0x0000, 0x0000, 0x060e2b3401040101),
            optional: true,
            unique: false,
        }],
    },
    ClassRow {
        auid: id(0x0d010101, 0x0101, 0x6d00, 0x060e2b3402060101),
        name: "GroupOfSoundfieldGroupsLabelSubDescriptor",
        parent: id(0x0d010101, 0x0101, 0x6a00, 0x060e2b3402060101),
        concrete: true,
        props: &[],
    },
    ClassRow {
        auid: id(0x0d010101, 0x0101, 0x6e00, 0x060e2b3402060101),
        name: "AVCSubDescriptor",
        parent: id(0x0d010101, 0x0101, 0x5900, 0x060e2b3402060101),
        concrete: true,
        props: &[
            PropRow {
                pid: 0xffcc,
                auid: id(0x04010606, 0x0103, 0x0000, 0x060e2b340101010e),
                name: "AVCConstantBPictureFlag",
                type_auid: id(0x01040100, 0x0000, 0x0000, 0x060e2b3401040101),
                optional: true,
                unique: false,
            },
            PropRow {
                pid: 0xffcb,
                auid: id(0x04010606, 0x0104, 0x0000, 0x060e2b340101010e),
                name: "AVCCodedContentKind",
                type_auid: id(0x0201012c, 0x0000, 0x0000, 0x060e2b3401040101),
                optional: true,
                unique: false,
            },
            PropRow {
                pid: 0xffca,
                auid: id(0x04010606, 0x0106, 0x0000, 0x060e2b340101010e),
                name: "AVCClosedGOPIndicator",
                type_auid: id(0x01040100, 0x0000, 0x0000, 0x060e2b3401040101),
                optional: true,
                unique: false,
            },
            PropRow {
                pid: 0xffc9,
                auid: id(0x04010606, 0x0107, 0x0000, 0x060e2b340101010e),
                name: "AVCIdenticalGOPIndicator",
                type_auid: id(0x01040100, 0x0000, 0x0000, 0x060e2b3401040101),
                optional: true,
                unique: false,
            },
            PropRow {
                pid: 0xffc8,
                auid: id(0x04010606, 0x0108, 0x0000, 0x060e2b340101010e),
                name: "AVCMaximumGOPSize",
                type_auid: id(0x01010200, 0x0000, 0x0000, 0x060e2b3401040101),
                optional: true,
                unique: false,
            },
            PropRow {
                pid: 0xffc7,
                auid: id(0x04010606, 0x0109, 0x0000, 0x060e2b340101010e),
                name: "AVCMaximumBPictureCount",
                type_auid: id(0x01010200, 0x0000, 0x0000, 0x060e2b3401040101),
                optional: true,
                unique: false,
            },
            PropRow {
                pid: 0xffc6,
                auid: id(0x04010606, 0x010a, 0x0000, 0x060e2b340101010e),
                name: "AVCProfile",
                type_auid: id(0x01010100, 0x0000, 0x0000, 0x060e2b3401040101),
                optional: true,
                unique: false,
            },
            PropRow {
                pid: 0xffc5,
                auid: id(0x04010606, 0x010b, 0x0000, 0x060e2b340101010e),
                name: "AVCMaximumBitRate",
                type_auid: id(0x01010300, 0x0000, 0x0000, 0x060e2b3401040101),
                optional: true,
                unique: false,
            },
            PropRow {
                pid: 0xffc4,
                auid: id(0x04010606, 0x010c, 0x0000, 0x060e2b340101010e),
                name: "AVCProfileConstraint",
                type_auid: id(0x01010100, 0x0000, 0x0000, 0x060e2b3401040101),
                optional: true,
                unique: false,
            },
            PropRow {
                pid: 0xffc3,
                auid: id(0x04010606, 0x010d, 0x0000, 0x060e2b340101010e),
                name: "AVCLevel",
                type_auid: id(0x01010100, 0x0000, 0x0000, 0x060e2b3401040101),
                optional: true,
                unique: false,
            },
            PropRow {
                pid: 0xffc2,
                auid: id(0x04010606, 0x010e, 0x0000, 0x060e2b340101010e),
                name: "AVCDecodingDelay",
                type_auid: id(0x01010100, 0x0000, 0x0000, 0x060e2b3401040101),
                optional: false,
                unique: false,
            },
            PropRow {
                pid: 0xffc1,
                auid: id(0x04010606, 0x010f, 0x0000, 0x060e2b340101010e),
                name: "AVCMaximumRefFrames",
                type_auid: id(0x01010100, 0x0000, 0x0000, 0x060e2b3401040101),
                optional: true,
                unique: false,
            },
            PropRow {
                pid: 0xffc0,
                auid: id(0x04010606, 0x0110, 0x0000, 0x060e2b340101010e),
                name: "AVCSequenceParameterSetFlag",
                type_auid: id(0x01010100, 0x0000, 0x0000, 0x060e2b3401040101),
                optional: true,
                unique: false,
            },
            PropRow {
                pid: 0xffbf,
                auid: id(0x04010606, 0x0111, 0x0000, 0x060e2b340101010e),
                name: "AVCPictureParameterSetFlag",
                type_auid: id(0x01010100, 0x0000, 0x0000, 0x060e2b3401040101),
                optional: true,
                unique: false,
            },
            PropRow {
                pid: 0xffbe,
                auid: id(0x04010606, 0x0114, 0x0000, 0x060e2b340101010e),
                name: "AVCAverageBitRate",
                type_auid: id(0x01010300, 0x0000, 0x0000, 0x060e2b3401040101),
                optional: true,
                unique: false,
            },
        ],
    },
    ClassRow {
        auid: id(0x0d010101, 0x0101, 0x5e00, 0x060e2b3402060101),
        name: "MPEGAudioDescriptor",
        parent: id(0x0d010101, 0x0101, 0x4200, 0x060e2b3402060101),
        concrete: true,
        props: &[PropRow {
            pid: 0xffbd,
            auid: id(0x04020403, 0x0102, 0x0000, 0x060e2b340101010a),
            name: "MPEGAudioBitRate",
            type_auid: id(0x01010300, 0x0000, 0x0000, 0x060e2b3401040101),
            optional: true,
            unique: false,
        }],
    },
    ClassRow {
        auid: id(0x0d010101, 0x0208, 0x0000, 0x060e2b3402060101),
        name: "TypeDefinitionFixedArray",
        parent: id(0x0d010101, 0x0203, 0x0000, 0x060e2b3402060101),
        concrete: true,
        props: &[
            PropRow {
                pid: 0x0017,
                auid: id(0x06010107, 0x0c00, 0x0000, 0x060e2b3401010102),
                name: "ElementType",
                type_auid: id(0x05010900, 0x0000, 0x0000, 0x060e2b3401040101),
                optional: false,
                unique: false,
            },
            PropRow {
                pid: 0x0018,
                auid: id(0x03010203, 0x0300, 0x0000, 0x060e2b3401010102),
                name: "ElementCount",
                type_auid: id(0x01010300, 0x0000, 0x0000, 0x060e2b3401040101),
                optional: false,
                unique: false,
            },
        ],
    },
    ClassRow {
        auid: id(0x0d010101, 0x0209, 0x0000, 0x060e2b3402060101),
        name: "TypeDefinitionVariableArray",
        parent: id(0x0d010101, 0x0203, 0x0000, 0x060e2b3402060101),
        concrete: true,
        props: &[PropRow {
            pid: 0x0019,
            auid: id(0x06010107, 0x0d00, 0x0000, 0x060e2b3401010102),
            name: "ElementType",
            type_auid: id(0x05010900, 0x0000, 0x0000, 0x060e2b3401040101),
            optional: false,
            unique: false,
        }],
    },
    ClassRow {
        auid: id(0x0d010101, 0x020a, 0x0000, 0x060e2b3402060101),
        name: "TypeDefinitionSet",
        parent: id(0x0d010101, 0x0203, 0x0000, 0x060e2b3402060101),
        concrete: true,
        props: &[PropRow {
            pid: 0x001a,
            auid: id(0x06010107, 0x0e00, 0x0000, 0x060e2b3401010102),
            name: "ElementType",
            type_auid: id(0x05010900, 0x0000, 0x0000, 0x060e2b3401040101),
            optional: false,
            unique: false,
        }],
    },
    ClassRow {
        auid: id(0x0d010101, 0x020b, 0x0000, 0x060e2b3402060101),
        name: "TypeDefinitionString",
        parent: id(0x0d010101, 0x0203, 0x0000, 0x060e2b3402060101),
        concrete: true,
        props: &[PropRow {
            pid: 0x001b,
            auid: id(0x06010107, 0x0f00, 0x0000, 0x060e2b3401010102),
            name: "ElementType",
            type_auid: id(0x05010900, 0x0000, 0x0000, 0x060e2b3401040101),
            optional: false,
            unique: false,
        }],
    },
    ClassRow {
        auid: id(0x0d010101, 0x020c, 0x0000, 0x060e2b3402060101),
        name: "TypeDefinitionStream",
        parent: id(0x0d010101, 0x0203, 0x0000, 0x060e2b3402060101),
        concrete: true,
        props: &[],
    },
    ClassRow {
        auid: id(0x0d010101, 0x020d, 0x0000, 0x060e2b3402060101),
        name: "TypeDefinitionRecord",
        parent: id(0x0d010101, 0x0203, 0x0000, 0x060e2b3402060101),
        concrete: true,
        props: &[
            PropRow {
                pid: 0x001c,
                auid: id(0x06010107, 0x1100, 0x0000, 0x060e2b3401010102),
                name: "MemberTypes",
                type_auid: id(0x05040200, 0x0000, 0x0000, 0x060e2b3401040101),
                optional: false,
                unique: false,
            },
            PropRow {
                pid: 0x001d,
                auid: id(0x03010203, 0x0600, 0x0000, 0x060e2b3401010102),
                name: "MemberNames",
                type_auid: id(0x04010500, 0x0000, 0x0000, 0x060e2b3401040101),
                optional: false,
                unique: false,
            },
        ],
    },
    ClassRow {
        auid: id(0x0d010101, 0x020e, 0x0000, 0x060e2b3402060101),
        name: "TypeDefinitionRename",
        parent: id(0x0d010101, 0x0203, 0x0000, 0x060e2b3402060101),
        concrete: true,
        props: &[PropRow {
            pid: 0x001e,
            auid: id(0x06010107, 0x1200, 0x0000, 0x060e2b3401010102),
            name: "RenamedType",
            type_auid: id(0x05010900, 0x0000, 0x0000, 0x060e2b3401040101),
            optional: false,
            unique: false,
        }],
    },
    ClassRow {
        auid: id(0x0d010101, 0x0220, 0x0000, 0x060e2b3402060101),
        name: "TypeDefinitionExtendibleEnumeration",
        parent: id(0x0d010101, 0x0203, 0x0000, 0x060e2b3402060101),
        concrete: true,
        props: &[
            PropRow {
                pid: 0x001f,
                auid: id(0x03010203, 0x0700, 0x0000, 0x060e2b3401010102),
                name: "ElementNames",
                type_auid: id(0x04010500, 0x0000, 0x0000, 0x060e2b3401040101),
                optional: false,
                unique: false,
            },
            PropRow {
                pid: 0x0020,
                auid: id(0x03010203, 0x0800, 0x0000, 0x060e2b3401010102),
                name: "ElementValues",
                type_auid: id(0x04010600, 0x0000, 0x0000, 0x060e2b3401040101),
                optional: false,
                unique: false,
            },
        ],
    },
    ClassRow {
        auid: id(0x0d010101, 0x0221, 0x0000, 0x060e2b3402060101),
        name: "TypeDefinitionIndirect",
        parent: id(0x0d010101, 0x0203, 0x0000, 0x060e2b3402060101),
        concrete: true,
        props: &[],
    },
    ClassRow {
        auid: id(0x0d010101, 0x0222, 0x0000, 0x060e2b3402060101),
        name: "TypeDefinitionOpaque",
        parent: id(0x0d010101, 0x0221, 0x0000, 0x060e2b3402060101),
        concrete: true,
        props: &[],
    },
    ClassRow {
        auid: id(0x0d010101, 0x0223, 0x0000, 0x060e2b3402060101),
        name: "TypeDefinitionCharacter",
        parent: id(0x0d010101, 0x0203, 0x0000, 0x060e2b3402060101),
        concrete: true,
        props: &[],
    },
    ClassRow {
        auid: id(0x0e040101, 0x0000, 0x0000, 0x060e2b3402060101),
        name: "TypeDefinitionGenericCharacter",
        parent: id(0x0d010101, 0x0203, 0x0000, 0x060e2b3402060101),
        concrete: true,
        props: &[PropRow {
            pid: 0xffbc,
            auid: id(0x0e040101, 0x0101, 0x0111, 0x060e2b3401010101),
            name: "CharacterSize",
            type_auid: id(0x01010100, 0x0000, 0x0000, 0x060e2b3401040101),
            optional: false,
            unique: false,
        }],
    },
    ClassRow {
        auid: id(0x0d010101, 0x0224, 0x0000, 0x060e2b3402060101),
        name: "MetaDefinition",
        parent: id(0x0d010101, 0x0224, 0x0000, 0x060e2b3402060101),
        concrete: false,
        props: &[
            PropRow {
                pid: 0x0005,
                auid: id(0x06010107, 0x1300, 0x0000, 0x060e2b3401010102),
                name: "Identification",
                type_auid: id(0x01030100, 0x0000, 0x0000, 0x060e2b3401040101),
                optional: false,
                unique: true,
            },
            PropRow {
                pid: 0x0006,
                auid: id(0x03020401, 0x0201, 0x0000, 0x060e2b3401010102),
                name: "Name",
                type_auid: id(0x01100200, 0x0000, 0x0000, 0x060e2b3401040101),
                optional: false,
                unique: false,
            },
            PropRow {
                pid: 0x0007,
                auid: id(0x06010107, 0x1401, 0x0000, 0x060e2b3401010102),
                name: "Description",
                type_auid: id(0x01100200, 0x0000, 0x0000, 0x060e2b3401040101),
                optional: true,
                unique: false,
            },
        ],
    },
    ClassRow {
        auid: id(0x0d010101, 0x0225, 0x0000, 0x060e2b3402060101),
        name: "MetaDictionary",
        parent: id(0x0d010101, 0x0225, 0x0000, 0x060e2b3402060101),
        concrete: true,
        props: &[
            PropRow {
                pid: 0x0003,
                auid: id(0x06010107, 0x0700, 0x0000, 0x060e2b3401010102),
                name: "ClassDefinitions",
                type_auid: id(0x05050100, 0x0000, 0x0000, 0x060e2b3401040101),
                optional: true,
                unique: false,
            },
            PropRow {
                pid: 0x0004,
                auid: id(0x06010107, 0x0800, 0x0000, 0x060e2b3401010102),
                name: "TypeDefinitions",
                type_auid: id(0x05050c00, 0x0000, 0x0000, 0x060e2b3401040101),
                optional: true,
                unique: false,
            },
        ],
    },
    ClassRow {
        auid: id(0x0d010400, 0x0000, 0x0000, 0x060e2b3402060101),
        name: "DescriptiveObject",
        parent: id(0x0d010101, 0x0101, 0x0100, 0x060e2b3402060101),
        concrete: false,
        props: &[],
    },
    ClassRow {
        auid: id(0x0d010401, 0x0000, 0x0000, 0x060e2b3402060101),
        name: "DescriptiveFramework",
        parent: id(0x0d010101, 0x0101, 0x0100, 0x060e2b3402060101),
        concrete: false,
        props: &[],
    },
];

/// Every built-in type (164), in the source tables' order.
pub static TYPES: &[TypeRow] = &[
    TypeRow {
        auid: id(0x05022800, 0x0000, 0x0000, 0x060e2b3401040101),
        name: "HeaderStrongRefence",
        kind: TypeKind::StrongRef {
            class: id(0x0d010101, 0x0101, 0x2f00, 0x060e2b3402060101),
        },
    },
    TypeRow {
        auid: id(0x05022700, 0x0000, 0x0000, 0x060e2b3401040101),
        name: "MetaDictionaryStrongReference",
        kind: TypeKind::StrongRef {
            class: id(0x0d010101, 0x0225, 0x0000, 0x060e2b3402060101),
        },
    },
    TypeRow {
        auid: id(0x01010100, 0x0000, 0x0000, 0x060e2b3401040101),
        name: "aafUInt8",
        kind: TypeKind::Integer {
            size: 1,
            signed: false,
        },
    },
    TypeRow {
        auid: id(0x01010200, 0x0000, 0x0000, 0x060e2b3401040101),
        name: "aafUInt16",
        kind: TypeKind::Integer {
            size: 2,
            signed: false,
        },
    },
    TypeRow {
        auid: id(0x01010300, 0x0000, 0x0000, 0x060e2b3401040101),
        name: "aafUInt32",
        kind: TypeKind::Integer {
            size: 4,
            signed: false,
        },
    },
    TypeRow {
        auid: id(0x01010400, 0x0000, 0x0000, 0x060e2b3401040101),
        name: "aafUInt64",
        kind: TypeKind::Integer {
            size: 8,
            signed: false,
        },
    },
    TypeRow {
        auid: id(0x01010500, 0x0000, 0x0000, 0x060e2b3401040101),
        name: "aafInt8",
        kind: TypeKind::Integer {
            size: 1,
            signed: true,
        },
    },
    TypeRow {
        auid: id(0x01010600, 0x0000, 0x0000, 0x060e2b3401040101),
        name: "aafInt16",
        kind: TypeKind::Integer {
            size: 2,
            signed: true,
        },
    },
    TypeRow {
        auid: id(0x01010700, 0x0000, 0x0000, 0x060e2b3401040101),
        name: "aafInt32",
        kind: TypeKind::Integer {
            size: 4,
            signed: true,
        },
    },
    TypeRow {
        auid: id(0x01010800, 0x0000, 0x0000, 0x060e2b3401040101),
        name: "aafInt64",
        kind: TypeKind::Integer {
            size: 8,
            signed: true,
        },
    },
    TypeRow {
        auid: id(0x01040100, 0x0000, 0x0000, 0x060e2b3401040101),
        name: "Boolean",
        kind: TypeKind::Enumeration {
            element: id(0x01010100, 0x0000, 0x0000, 0x060e2b3401040101),
            names: &["False", "True"],
            values: &[0, 1],
        },
    },
    TypeRow {
        auid: id(0x02010101, 0x0000, 0x0000, 0x060e2b3401040101),
        name: "ProductReleaseType",
        kind: TypeKind::Enumeration {
            element: id(0x01010100, 0x0000, 0x0000, 0x060e2b3401040101),
            names: &[
                "VersionUnknown",
                "VersionReleased",
                "VersionDebug",
                "VersionPatched",
                "VersionBeta",
                "VersionPrivateBuild",
            ],
            values: &[0, 1, 2, 3, 4, 5],
        },
    },
    TypeRow {
        auid: id(0x02010102, 0x0000, 0x0000, 0x060e2b3401040101),
        name: "TapeFormatType",
        kind: TypeKind::Enumeration {
            element: id(0x01010100, 0x0000, 0x0000, 0x060e2b3401040101),
            names: &[
                "TapeFormatNull",
                "BetacamFormat",
                "BetacamSPFormat",
                "VHSFormat",
                "SVHSFormat",
                "8mmFormat",
                "Hi8Format",
            ],
            values: &[0, 1, 2, 3, 4, 5, 6],
        },
    },
    TypeRow {
        auid: id(0x02010103, 0x0000, 0x0000, 0x060e2b3401040101),
        name: "VideoSignalType",
        kind: TypeKind::Enumeration {
            element: id(0x01010100, 0x0000, 0x0000, 0x060e2b3401040101),
            names: &["VideoSignalNull", "NTSCSignal", "PALSignal"],
            values: &[0, 1, 2],
        },
    },
    TypeRow {
        auid: id(0x02010104, 0x0000, 0x0000, 0x060e2b3401040101),
        name: "TapeCaseType",
        kind: TypeKind::Enumeration {
            element: id(0x01010100, 0x0000, 0x0000, 0x060e2b3401040101),
            names: &[
                "TapeCaseNull",
                "ThreeFourthInchVideoTape",
                "VHSVideoTape",
                "8mmVideoTape",
                "BetacamVideoTape",
                "CompactCassette",
                "DATCartridge",
                "NagraAudioTape",
            ],
            values: &[0, 1, 2, 3, 4, 5, 6, 7],
        },
    },
    TypeRow {
        auid: id(0x02010105, 0x0000, 0x0000, 0x060e2b3401040101),
        name: "ColorSitingType",
        kind: TypeKind::Enumeration {
            element: id(0x01010100, 0x0000, 0x0000, 0x060e2b3401040101),
            names: &[
                "CoSiting",
                "Averaging",
                "ThreeTap",
                "Quincunx",
                "Rec601",
                "LineAlternating",
                "VerticalMidpoint",
                "UnknownSiting",
            ],
            values: &[0, 1, 2, 3, 4, 5, 6, 255],
        },
    },
    TypeRow {
        auid: id(0x02010106, 0x0000, 0x0000, 0x060e2b3401040101),
        name: "EditHintType",
        kind: TypeKind::Enumeration {
            element: id(0x01010100, 0x0000, 0x0000, 0x060e2b3401040101),
            names: &[
                "NoEditHint",
                "Proportional",
                "RelativeLeft",
                "RelativeRight",
                "RelativeFixed",
            ],
            values: &[0, 1, 2, 3, 4],
        },
    },
    TypeRow {
        auid: id(0x02010107, 0x0000, 0x0000, 0x060e2b3401040101),
        name: "FadeType",
        kind: TypeKind::Enumeration {
            element: id(0x01010100, 0x0000, 0x0000, 0x060e2b3401040101),
            names: &["FadeNone", "FadeLinearAmp", "FadeLinearPower"],
            values: &[0, 1, 2],
        },
    },
    TypeRow {
        auid: id(0x02010108, 0x0000, 0x0000, 0x060e2b3401040101),
        name: "LayoutType",
        kind: TypeKind::Enumeration {
            element: id(0x01010100, 0x0000, 0x0000, 0x060e2b3401040101),
            names: &[
                "FullFrame",
                "SeparateFields",
                "OneField",
                "MixedFields",
                "SegmentedFrame",
            ],
            values: &[0, 1, 2, 3, 4],
        },
    },
    TypeRow {
        auid: id(0x02010109, 0x0000, 0x0000, 0x060e2b3401040101),
        name: "TCSource",
        kind: TypeKind::Enumeration {
            element: id(0x01010100, 0x0000, 0x0000, 0x060e2b3401040101),
            names: &["TimecodeLTC", "TimecodeVITC"],
            values: &[0, 1],
        },
    },
    TypeRow {
        auid: id(0x0201010a, 0x0000, 0x0000, 0x060e2b3401040101),
        name: "PulldownDirectionType",
        kind: TypeKind::Enumeration {
            element: id(0x01010100, 0x0000, 0x0000, 0x060e2b3401040101),
            names: &["TapeToFilmSpeed", "FilmToTapeSpeed"],
            values: &[0, 1],
        },
    },
    TypeRow {
        auid: id(0x0201010b, 0x0000, 0x0000, 0x060e2b3401040101),
        name: "PulldownKindType",
        kind: TypeKind::Enumeration {
            element: id(0x01010100, 0x0000, 0x0000, 0x060e2b3401040101),
            names: &[
                "TwoThreePD",
                "PALPD",
                "OneToOneNTSC",
                "OneToOnePAL",
                "VideoTapNTSC",
                "OneToOneHDSixty",
                "TwentyFourToSixtyPD",
                "TwoToOnePD",
            ],
            values: &[0, 1, 2, 3, 4, 5, 6, 7],
        },
    },
    TypeRow {
        auid: id(0x0201010c, 0x0000, 0x0000, 0x060e2b3401040101),
        name: "EdgeType",
        kind: TypeKind::Enumeration {
            element: id(0x01010100, 0x0000, 0x0000, 0x060e2b3401040101),
            names: &[
                "EtNull",
                "EtKeycode",
                "EtEdgenum4",
                "EtEdgenum5",
                "EtHeaderSize",
            ],
            values: &[0, 1, 2, 3, 8],
        },
    },
    TypeRow {
        auid: id(0x0201010d, 0x0000, 0x0000, 0x060e2b3401040101),
        name: "FilmType",
        kind: TypeKind::Enumeration {
            element: id(0x01010100, 0x0000, 0x0000, 0x060e2b3401040101),
            names: &["FtNull", "Ft35MM", "Ft16MM", "Ft8MM", "Ft65MM"],
            values: &[0, 1, 2, 3, 4],
        },
    },
    TypeRow {
        auid: id(0x0201010e, 0x0000, 0x0000, 0x060e2b3401040101),
        name: "RGBAComponentKind",
        kind: TypeKind::Enumeration {
            element: id(0x01010100, 0x0000, 0x0000, 0x060e2b3401040101),
            names: &[
                "CompNone",
                "CompAlpha",
                "CompBlue",
                "CompFill",
                "CompGreen",
                "CompPalette",
                "CompRed",
                "CompNull",
                "CompRedLSBs",
                "CompGreenLSBs",
                "CompBlueLSBs",
                "CompAlphaLSBs",
                "CompColorDifferenceU",
                "CompColorDifferenceV",
                "CompComposite",
                "CompNonCoSitedLuma",
                "CompLuma",
                "CompDepth",
                "CompColorDifferenceULSBs",
                "CompColorDifferenceVLSBs",
                "CompCompositeLSBs",
                "CompNonCoSitedLumaLSBs",
                "CompLumaLSBs",
                "CompDepthLSBs",
                "CompColorX",
                "CompColorY",
                "CompColorZ",
            ],
            values: &[
                48, 65, 66, 70, 71, 80, 82, 0, 114, 103, 98, 97, 85, 86, 87, 88, 89, 90, 117, 118,
                119, 120, 121, 122, 216, 217, 218,
            ],
        },
    },
    TypeRow {
        auid: id(0x0201010f, 0x0000, 0x0000, 0x060e2b3401040101),
        name: "ReferenceType",
        kind: TypeKind::Enumeration {
            element: id(0x01010100, 0x0000, 0x0000, 0x060e2b3401040101),
            names: &[
                "RefLimitMinimum",
                "RefLimitMaximum",
                "RefMinimum",
                "RefMaximum",
                "RefEnumvalue",
            ],
            values: &[0, 1, 2, 3, 4],
        },
    },
    TypeRow {
        auid: id(0x02010120, 0x0000, 0x0000, 0x060e2b3401040101),
        name: "AlphaTransparencyType",
        kind: TypeKind::Enumeration {
            element: id(0x01010100, 0x0000, 0x0000, 0x060e2b3401040101),
            names: &["MinValueTransparent", "MaxValueTransparent"],
            values: &[0, 1],
        },
    },
    TypeRow {
        auid: id(0x02010121, 0x0000, 0x0000, 0x060e2b3401040101),
        name: "FieldNumber",
        kind: TypeKind::Enumeration {
            element: id(0x01010100, 0x0000, 0x0000, 0x060e2b3401040101),
            names: &["UnspecifiedField", "FieldOne", "FieldTwo"],
            values: &[0, 1, 2],
        },
    },
    TypeRow {
        auid: id(0x02010122, 0x0000, 0x0000, 0x060e2b3401040101),
        name: "ElectroSpatialFormulation",
        kind: TypeKind::Enumeration {
            element: id(0x01010100, 0x0000, 0x0000, 0x060e2b3401040101),
            names: &[
                "ElectroSpatialFormulation_Default",
                "ElectroSpatialFormulation_TwoChannelMode",
                "ElectroSpatialFormulation_SingleChannelMode",
                "ElectroSpatialFormulation_PrimarySecondaryMode",
                "ElectroSpatialFormulation_StereophonicMode",
                "ElectroSpatialFormulation_SingleChannelDoubleSamplingFrequencyMode",
                "ElectroSpatialFormulation_StereoLeftChannelDoubleSamplingFrequencyMode",
                "ElectroSpatialFormulation_StereoRightChannelDoubleSamplingFrequencyMode",
                "ElectroSpatialFormulation_MultiChannelMode",
            ],
            values: &[0, 1, 2, 3, 4, 7, 8, 9, 15],
        },
    },
    TypeRow {
        auid: id(0x02010123, 0x0000, 0x0000, 0x060e2b3401040101),
        name: "EmphasisType",
        kind: TypeKind::Enumeration {
            element: id(0x01010100, 0x0000, 0x0000, 0x060e2b3401040101),
            names: &[
                "Emphasis_Unknown",
                "Emphasis_Reserved0",
                "Emphasis_Reserved1",
                "Emphasis_Reserved2",
                "Emphasis_None",
                "Emphasis_Reserved3",
                "Emphasis_15and50",
                "Emphasis_ITU",
            ],
            values: &[0, 1, 2, 3, 4, 5, 6, 7],
        },
    },
    TypeRow {
        auid: id(0x02010124, 0x0000, 0x0000, 0x060e2b3401040101),
        name: "AuxBitsModeType",
        kind: TypeKind::Enumeration {
            element: id(0x01010100, 0x0000, 0x0000, 0x060e2b3401040101),
            names: &[
                "AuxBitsMode_NotDefined",
                "AuxBitsMode_MainAudioSampleData",
                "AuxBitsMode_SingleCoordinationSignal",
                "AuxBitsMode_UserDefined",
                "AuxBitsMode_Reserved0",
                "AuxBitsMode_Reserved1",
                "AuxBitsMode_Reserved2",
                "AuxBitsMode_Reserved3",
            ],
            values: &[0, 1, 2, 3, 4, 5, 6, 7],
        },
    },
    TypeRow {
        auid: id(0x02010125, 0x0000, 0x0000, 0x060e2b3401040101),
        name: "ChannelStatusModeType",
        kind: TypeKind::Enumeration {
            element: id(0x01010100, 0x0000, 0x0000, 0x060e2b3401040101),
            names: &[
                "ChannelStatusMode_None",
                "ChannelStatusMode_Minimum",
                "ChannelStatusMode_Standard",
                "ChannelStatusMode_Fixed",
                "ChannelStatusMode_Stream",
                "ChannelStatusMode_Essence",
            ],
            values: &[0, 1, 2, 3, 4, 5],
        },
    },
    TypeRow {
        auid: id(0x02010126, 0x0000, 0x0000, 0x060e2b3401040101),
        name: "UserDataModeType",
        kind: TypeKind::Enumeration {
            element: id(0x01010100, 0x0000, 0x0000, 0x060e2b3401040101),
            names: &[
                "UserDataMode_NotDefined",
                "UserDataMode_192BitBlockStructure",
                "UserDataMode_AES18",
                "UserDataMode_UserDefined",
                "UserDataMode_IEC",
                "UserDataMode_Metadata",
                "UserDataMode_Reserved0",
                "UserDataMode_Reserved1",
                "UserDataMode_Reserved2",
                "UserDataMode_Reserved3",
                "UserDataMode_Reserved4",
                "UserDataMode_Reserved5",
                "UserDataMode_Reserved6",
                "UserDataMode_Reserved7",
                "UserDataMode_Reserved8",
                "UserDataMode_Reserved9",
            ],
            values: &[0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15],
        },
    },
    TypeRow {
        auid: id(0x02010127, 0x0000, 0x0000, 0x060e2b3401040101),
        name: "SignalStandardType",
        kind: TypeKind::Enumeration {
            element: id(0x01010100, 0x0000, 0x0000, 0x060e2b3401040101),
            names: &[
                "SignalStandard_None",
                "SignalStandard_ITU601",
                "SignalStandard_ITU1358",
                "SignalStandard_SMPTE347M",
                "SignalStandard_SMPTE274M",
                "SignalStandard_SMPTE296M",
                "SignalStandard_SMPTE349M",
                "SignalStandard_SMPTE428_1",
            ],
            values: &[0, 1, 2, 3, 4, 5, 6, 7],
        },
    },
    TypeRow {
        auid: id(0x02010128, 0x0000, 0x0000, 0x060e2b3401040101),
        name: "ScanningDirectionType",
        kind: TypeKind::Enumeration {
            element: id(0x01010100, 0x0000, 0x0000, 0x060e2b3401040101),
            names: &[
                "ScanningDirection_LeftToRightTopToBottom",
                "ScanningDirection_RightToLeftTopToBottom",
                "ScanningDirection_LeftToRightBottomToTop",
                "ScanningDirection_RightToLeftBottomToTop",
                "ScanningDirection_TopToBottomLeftToRight",
                "ScanningDirection_TopToBottomRightToLeft",
                "ScanningDirection_BottomToTopLeftToRight",
                "ScanningDirection_BottomToTopRightToLeft",
            ],
            values: &[0, 1, 2, 3, 4, 5, 6, 7],
        },
    },
    TypeRow {
        auid: id(0x0201012a, 0x0000, 0x0000, 0x060e2b3401040101),
        name: "ContentScanningType",
        kind: TypeKind::Enumeration {
            element: id(0x01010100, 0x0000, 0x0000, 0x060e2b3401040101),
            names: &[
                "ContentScanning_NotKnown",
                "ContentScanning_Progressive",
                "ContentScanning_Interlace",
                "ContentScanning_Mixed",
            ],
            values: &[0, 1, 2, 3],
        },
    },
    TypeRow {
        auid: id(0x0201012b, 0x0000, 0x0000, 0x060e2b3401040101),
        name: "TitleAlignmentType",
        kind: TypeKind::Enumeration {
            element: id(0x01010100, 0x0000, 0x0000, 0x060e2b3401040101),
            names: &[
                "TitleAlignment_Left",
                "TitleAlignment_Center",
                "TitleAlignment_Right",
            ],
            values: &[0, 1, 2],
        },
    },
    TypeRow {
        auid: id(0x0201012c, 0x0000, 0x0000, 0x060e2b3401040101),
        name: "AVCContentScanningType",
        kind: TypeKind::Enumeration {
            element: id(0x01010100, 0x0000, 0x0000, 0x060e2b3401040101),
            names: &[
                "AVCContentScanning_NotKnown",
                "AVCContentScanning_ProgressiveFramePicture",
                "AVCContentScanning_InterlaceFieldPicture",
                "AVCContentScanning_InterlaceFramePicture",
                "AVCContentScanning_Interlace_FrameFieldPicture",
            ],
            values: &[0, 1, 2, 3, 4],
        },
    },
    TypeRow {
        auid: id(0x0201012d, 0x0000, 0x0000, 0x060e2b3401040101),
        name: "MPEG4VisualCodedContentType",
        kind: TypeKind::Enumeration {
            element: id(0x01010100, 0x0000, 0x0000, 0x060e2b3401040101),
            names: &[
                "MPEG4VisualCodedContent_Unknown",
                "MPEG4VisualCodedContent_Progressive",
                "MPEG4VisualCodedContent_Interlaced",
                "MPEG4VisualCodedContent_Mixed",
            ],
            values: &[0, 1, 2, 3],
        },
    },
    TypeRow {
        auid: id(0x01030100, 0x0000, 0x0000, 0x060e2b3401040101),
        name: "AUID",
        kind: TypeKind::Record {
            members: &[
                ("Data1", id(0x01010300, 0x0000, 0x0000, 0x060e2b3401040101)),
                ("Data2", id(0x01010200, 0x0000, 0x0000, 0x060e2b3401040101)),
                ("Data3", id(0x01010200, 0x0000, 0x0000, 0x060e2b3401040101)),
                ("Data4", id(0x04010800, 0x0000, 0x0000, 0x060e2b3401040101)),
            ],
        },
    },
    TypeRow {
        auid: id(0x01030200, 0x0000, 0x0000, 0x060e2b3401040101),
        name: "MobIDType",
        kind: TypeKind::Record {
            members: &[
                (
                    "SMPTELabel",
                    id(0x04010200, 0x0000, 0x0000, 0x060e2b3401040101),
                ),
                ("length", id(0x01010100, 0x0000, 0x0000, 0x060e2b3401040101)),
                (
                    "instanceHigh",
                    id(0x01010100, 0x0000, 0x0000, 0x060e2b3401040101),
                ),
                (
                    "instanceMid",
                    id(0x01010100, 0x0000, 0x0000, 0x060e2b3401040101),
                ),
                (
                    "instanceLow",
                    id(0x01010100, 0x0000, 0x0000, 0x060e2b3401040101),
                ),
                (
                    "material",
                    id(0x01030100, 0x0000, 0x0000, 0x060e2b3401040101),
                ),
            ],
        },
    },
    TypeRow {
        auid: id(0x03010100, 0x0000, 0x0000, 0x060e2b3401040101),
        name: "Rational",
        kind: TypeKind::Record {
            members: &[
                (
                    "Numerator",
                    id(0x01010700, 0x0000, 0x0000, 0x060e2b3401040101),
                ),
                (
                    "Denominator",
                    id(0x01010700, 0x0000, 0x0000, 0x060e2b3401040101),
                ),
            ],
        },
    },
    TypeRow {
        auid: id(0x03010200, 0x0000, 0x0000, 0x060e2b3401040101),
        name: "ProductVersion",
        kind: TypeKind::Record {
            members: &[
                ("major", id(0x01010200, 0x0000, 0x0000, 0x060e2b3401040101)),
                ("minor", id(0x01010200, 0x0000, 0x0000, 0x060e2b3401040101)),
                (
                    "tertiary",
                    id(0x01010200, 0x0000, 0x0000, 0x060e2b3401040101),
                ),
                (
                    "patchLevel",
                    id(0x01010200, 0x0000, 0x0000, 0x060e2b3401040101),
                ),
                ("type", id(0x02010101, 0x0000, 0x0000, 0x060e2b3401040101)),
            ],
        },
    },
    TypeRow {
        auid: id(0x03010300, 0x0000, 0x0000, 0x060e2b3401040101),
        name: "VersionType",
        kind: TypeKind::Record {
            members: &[
                ("major", id(0x01010500, 0x0000, 0x0000, 0x060e2b3401040101)),
                ("minor", id(0x01010500, 0x0000, 0x0000, 0x060e2b3401040101)),
            ],
        },
    },
    TypeRow {
        auid: id(0x03010400, 0x0000, 0x0000, 0x060e2b3401040101),
        name: "RGBAComponent",
        kind: TypeKind::Record {
            members: &[
                ("Code", id(0x0201010e, 0x0000, 0x0000, 0x060e2b3401040101)),
                ("Size", id(0x01010100, 0x0000, 0x0000, 0x060e2b3401040101)),
            ],
        },
    },
    TypeRow {
        auid: id(0x03010500, 0x0000, 0x0000, 0x060e2b3401040101),
        name: "DateStruct",
        kind: TypeKind::Record {
            members: &[
                ("year", id(0x01010600, 0x0000, 0x0000, 0x060e2b3401040101)),
                ("month", id(0x01010100, 0x0000, 0x0000, 0x060e2b3401040101)),
                ("day", id(0x01010100, 0x0000, 0x0000, 0x060e2b3401040101)),
            ],
        },
    },
    TypeRow {
        auid: id(0x03010600, 0x0000, 0x0000, 0x060e2b3401040101),
        name: "TimeStruct",
        kind: TypeKind::Record {
            members: &[
                ("hour", id(0x01010100, 0x0000, 0x0000, 0x060e2b3401040101)),
                ("minute", id(0x01010100, 0x0000, 0x0000, 0x060e2b3401040101)),
                ("second", id(0x01010100, 0x0000, 0x0000, 0x060e2b3401040101)),
                (
                    "fraction",
                    id(0x01010100, 0x0000, 0x0000, 0x060e2b3401040101),
                ),
            ],
        },
    },
    TypeRow {
        auid: id(0x03010700, 0x0000, 0x0000, 0x060e2b3401040101),
        name: "TimeStamp",
        kind: TypeKind::Record {
            members: &[
                ("date", id(0x03010500, 0x0000, 0x0000, 0x060e2b3401040101)),
                ("time", id(0x03010600, 0x0000, 0x0000, 0x060e2b3401040101)),
            ],
        },
    },
    TypeRow {
        auid: id(0x01030300, 0x0000, 0x0000, 0x060e2b3401040101),
        name: "aafUUID",
        kind: TypeKind::FixedArray {
            element: id(0x01010100, 0x0000, 0x0000, 0x060e2b3401040101),
            count: 16,
        },
    },
    TypeRow {
        auid: id(0x04010200, 0x0000, 0x0000, 0x060e2b3401040101),
        name: "aafUInt8Array12",
        kind: TypeKind::FixedArray {
            element: id(0x01010100, 0x0000, 0x0000, 0x060e2b3401040101),
            count: 12,
        },
    },
    TypeRow {
        auid: id(0x04010800, 0x0000, 0x0000, 0x060e2b3401040101),
        name: "aafUInt8Array8",
        kind: TypeKind::FixedArray {
            element: id(0x01010100, 0x0000, 0x0000, 0x060e2b3401040101),
            count: 8,
        },
    },
    TypeRow {
        auid: id(0x04020100, 0x0000, 0x0000, 0x060e2b3401040101),
        name: "aafRGBALayout",
        kind: TypeKind::FixedArray {
            element: id(0x03010400, 0x0000, 0x0000, 0x060e2b3401040101),
            count: 8,
        },
    },
    TypeRow {
        auid: id(0x04011500, 0x0000, 0x0000, 0x060e2b3401040101),
        name: "aafUUIDArray",
        kind: TypeKind::VariableArray {
            element: id(0x01030300, 0x0000, 0x0000, 0x060e2b3401040101),
        },
    },
    TypeRow {
        auid: id(0x04010100, 0x0000, 0x0000, 0x060e2b3401040101),
        name: "aafUInt8Array",
        kind: TypeKind::VariableArray {
            element: id(0x01010100, 0x0000, 0x0000, 0x060e2b3401040101),
        },
    },
    TypeRow {
        auid: id(0x04010300, 0x0000, 0x0000, 0x060e2b3401040101),
        name: "aafInt32Array",
        kind: TypeKind::VariableArray {
            element: id(0x01010700, 0x0000, 0x0000, 0x060e2b3401040101),
        },
    },
    TypeRow {
        auid: id(0x04010400, 0x0000, 0x0000, 0x060e2b3401040101),
        name: "aafInt64Array",
        kind: TypeKind::VariableArray {
            element: id(0x01010800, 0x0000, 0x0000, 0x060e2b3401040101),
        },
    },
    TypeRow {
        auid: id(0x04010500, 0x0000, 0x0000, 0x060e2b3401040101),
        name: "aafStringArray",
        kind: TypeKind::VariableArray {
            element: id(0x01100100, 0x0000, 0x0000, 0x060e2b3401040101),
        },
    },
    TypeRow {
        auid: id(0x04010600, 0x0000, 0x0000, 0x060e2b3401040101),
        name: "aafAUIDArray",
        kind: TypeKind::VariableArray {
            element: id(0x01030100, 0x0000, 0x0000, 0x060e2b3401040101),
        },
    },
    TypeRow {
        auid: id(0x04010700, 0x0000, 0x0000, 0x060e2b3401040101),
        name: "aafPositionArray",
        kind: TypeKind::VariableArray {
            element: id(0x01010100, 0x0000, 0x0000, 0x060e2b3401040101),
        },
    },
    TypeRow {
        auid: id(0x04010900, 0x0000, 0x0000, 0x060e2b3401040101),
        name: "aafUInt32Array",
        kind: TypeKind::VariableArray {
            element: id(0x01010300, 0x0000, 0x0000, 0x060e2b3401040101),
        },
    },
    TypeRow {
        auid: id(0x04010a00, 0x0000, 0x0000, 0x060e2b3401040101),
        name: "aafChannelStatusModeArray",
        kind: TypeKind::VariableArray {
            element: id(0x02010125, 0x0000, 0x0000, 0x060e2b3401040101),
        },
    },
    TypeRow {
        auid: id(0x04010b00, 0x0000, 0x0000, 0x060e2b3401040101),
        name: "aafUserDataModeArray",
        kind: TypeKind::VariableArray {
            element: id(0x02010126, 0x0000, 0x0000, 0x060e2b3401040101),
        },
    },
    TypeRow {
        auid: id(0x04100100, 0x0000, 0x0000, 0x060e2b3401040101),
        name: "aafDataValue",
        kind: TypeKind::VariableArray {
            element: id(0x01010100, 0x0000, 0x0000, 0x060e2b3401040101),
        },
    },
    TypeRow {
        auid: id(0x05060100, 0x0000, 0x0000, 0x060e2b3401040101),
        name: "kAAFTypeID_ComponentStrongReferenceVector",
        kind: TypeKind::VariableArray {
            element: id(0x05020b00, 0x0000, 0x0000, 0x060e2b3401040101),
        },
    },
    TypeRow {
        auid: id(0x05060200, 0x0000, 0x0000, 0x060e2b3401040101),
        name: "kAAFTypeID_ControlPointStrongReferenceVector",
        kind: TypeKind::VariableArray {
            element: id(0x05020d00, 0x0000, 0x0000, 0x060e2b3401040101),
        },
    },
    TypeRow {
        auid: id(0x05060300, 0x0000, 0x0000, 0x060e2b3401040101),
        name: "kAAFTypeID_IdentificationStrongReferenceVector",
        kind: TypeKind::VariableArray {
            element: id(0x05021000, 0x0000, 0x0000, 0x060e2b3401040101),
        },
    },
    TypeRow {
        auid: id(0x05060400, 0x0000, 0x0000, 0x060e2b3401040101),
        name: "kAAFTypeID_LocatorStrongReferenceVector",
        kind: TypeKind::VariableArray {
            element: id(0x05021200, 0x0000, 0x0000, 0x060e2b3401040101),
        },
    },
    TypeRow {
        auid: id(0x05060500, 0x0000, 0x0000, 0x060e2b3401040101),
        name: "kAAFTypeID_MobSlotStrongReferenceVector",
        kind: TypeKind::VariableArray {
            element: id(0x05021400, 0x0000, 0x0000, 0x060e2b3401040101),
        },
    },
    TypeRow {
        auid: id(0x05060600, 0x0000, 0x0000, 0x060e2b3401040101),
        name: "kAAFTypeID_SegmentStrongReferenceVector",
        kind: TypeKind::VariableArray {
            element: id(0x05020600, 0x0000, 0x0000, 0x060e2b3401040101),
        },
    },
    TypeRow {
        auid: id(0x05060700, 0x0000, 0x0000, 0x060e2b3401040101),
        name: "kAAFTypeID_SourceReferenceStrongReferenceVector",
        kind: TypeKind::VariableArray {
            element: id(0x05020800, 0x0000, 0x0000, 0x060e2b3401040101),
        },
    },
    TypeRow {
        auid: id(0x05060800, 0x0000, 0x0000, 0x060e2b3401040101),
        name: "kAAFTypeID_TaggedValueStrongReferenceVector",
        kind: TypeKind::VariableArray {
            element: id(0x05021a00, 0x0000, 0x0000, 0x060e2b3401040101),
        },
    },
    TypeRow {
        auid: id(0x05060900, 0x0000, 0x0000, 0x060e2b3401040101),
        name: "kAAFTypeID_KLVDataStrongReferenceVector",
        kind: TypeKind::VariableArray {
            element: id(0x05021c00, 0x0000, 0x0000, 0x060e2b3401040101),
        },
    },
    TypeRow {
        auid: id(0x05060a00, 0x0000, 0x0000, 0x060e2b3401040101),
        name: "kAAFTypeID_ParameterStrongReferenceVector",
        kind: TypeKind::VariableArray {
            element: id(0x05021600, 0x0000, 0x0000, 0x060e2b3401040101),
        },
    },
    TypeRow {
        auid: id(0x05060b00, 0x0000, 0x0000, 0x060e2b3401040101),
        name: "kAAFTypeID_FileDescriptorStrongReferenceVector",
        kind: TypeKind::VariableArray {
            element: id(0x05021d00, 0x0000, 0x0000, 0x060e2b3401040101),
        },
    },
    TypeRow {
        auid: id(0x05060c00, 0x0000, 0x0000, 0x060e2b3401040101),
        name: "kAAFTypeID_RIFFChunkStrongReferenceVector",
        kind: TypeKind::VariableArray {
            element: id(0x05021e00, 0x0000, 0x0000, 0x060e2b3401040101),
        },
    },
    TypeRow {
        auid: id(0x05060d00, 0x0000, 0x0000, 0x060e2b3401040101),
        name: "kAAFTypeID_DescriptiveObjectStrongReferenceVector",
        kind: TypeKind::VariableArray {
            element: id(0x05022200, 0x0000, 0x0000, 0x060e2b3401040101),
        },
    },
    TypeRow {
        auid: id(0x05060e00, 0x0000, 0x0000, 0x060e2b3401040101),
        name: "kAAFTypeID_SubDescriptorStrongReferenceVector",
        kind: TypeKind::VariableArray {
            element: id(0x05022600, 0x0000, 0x0000, 0x060e2b3401040101),
        },
    },
    TypeRow {
        auid: id(0x05040100, 0x0000, 0x0000, 0x060e2b3401040101),
        name: "kAAFTypeID_OperationDefinitionWeakReferenceVector",
        kind: TypeKind::VariableArray {
            element: id(0x05010700, 0x0000, 0x0000, 0x060e2b3401040101),
        },
    },
    TypeRow {
        auid: id(0x05040200, 0x0000, 0x0000, 0x060e2b3401040101),
        name: "kAAFTypeID_TypeDefinitionWeakReferenceVector",
        kind: TypeKind::VariableArray {
            element: id(0x05010900, 0x0000, 0x0000, 0x060e2b3401040101),
        },
    },
    TypeRow {
        auid: id(0x05040300, 0x0000, 0x0000, 0x060e2b3401040101),
        name: "kAAFTypeID_DataDefinitionWeakReferenceVector",
        kind: TypeKind::VariableArray {
            element: id(0x05010300, 0x0000, 0x0000, 0x060e2b3401040101),
        },
    },
    TypeRow {
        auid: id(0x01012001, 0x0000, 0x0000, 0x060e2b3401040101),
        name: "aafPositionType",
        kind: TypeKind::Rename {
            renamed: id(0x01010800, 0x0000, 0x0000, 0x060e2b3401040101),
        },
    },
    TypeRow {
        auid: id(0x01012002, 0x0000, 0x0000, 0x060e2b3401040101),
        name: "aafLengthType",
        kind: TypeKind::Rename {
            renamed: id(0x01010800, 0x0000, 0x0000, 0x060e2b3401040101),
        },
    },
    TypeRow {
        auid: id(0x01012003, 0x0000, 0x0000, 0x060e2b3401040101),
        name: "aafJPEGTableIDType",
        kind: TypeKind::Rename {
            renamed: id(0x01010700, 0x0000, 0x0000, 0x060e2b3401040101),
        },
    },
    TypeRow {
        auid: id(0x01012300, 0x0000, 0x0000, 0x060e2b3401040101),
        name: "aafPhaseFrameType",
        kind: TypeKind::Rename {
            renamed: id(0x01010700, 0x0000, 0x0000, 0x060e2b3401040101),
        },
    },
    TypeRow {
        auid: id(0x01100200, 0x0000, 0x0000, 0x060e2b3401040101),
        name: "aafString",
        kind: TypeKind::String {
            element: id(0x01100100, 0x0000, 0x0000, 0x060e2b3401040101),
        },
    },
    TypeRow {
        auid: id(0x01100400, 0x0000, 0x0000, 0x060e2b3401040101),
        name: "aafISO7",
        kind: TypeKind::String {
            element: id(0x01100300, 0x0000, 0x0000, 0x060e2b3401040101),
        },
    },
    TypeRow {
        auid: id(0x04100200, 0x0000, 0x0000, 0x060e2b3401040101),
        name: "Stream",
        kind: TypeKind::Stream,
    },
    TypeRow {
        auid: id(0x04100400, 0x0000, 0x0000, 0x060e2b3401040101),
        name: "aafOpaque",
        kind: TypeKind::Opaque,
    },
    TypeRow {
        auid: id(0x02020101, 0x0000, 0x0000, 0x060e2b3401040101),
        name: "OperationCategoryType",
        kind: TypeKind::ExtEnum {
            elements: &[(
                "OperationCategory_Effect",
                id(0x0d010102, 0x0101, 0x0100, 0x060e2b3404010101),
            )],
        },
    },
    TypeRow {
        auid: id(0x02020102, 0x0000, 0x0000, 0x060e2b3401040101),
        name: "TransferCharacteristicType",
        kind: TypeKind::ExtEnum {
            elements: &[
                (
                    "TransferCharacteristic_ITU470_PAL",
                    id(0x04010101, 0x0101, 0x0000, 0x060e2b3404010101),
                ),
                (
                    "TransferCharacteristic_ITU709",
                    id(0x04010101, 0x0102, 0x0000, 0x060e2b3404010101),
                ),
                (
                    "TransferCharacteristic_SMPTE240M",
                    id(0x04010101, 0x0103, 0x0000, 0x060e2b3404010101),
                ),
                (
                    "TransferCharacteristic_274M_296M",
                    id(0x04010101, 0x0104, 0x0000, 0x060e2b3404010101),
                ),
                (
                    "TransferCharacteristic_ITU1361",
                    id(0x04010101, 0x0105, 0x0000, 0x060e2b3404010101),
                ),
                (
                    "TransferCharacteristic_linear",
                    id(0x04010101, 0x0106, 0x0000, 0x060e2b3404010101),
                ),
                (
                    "TransferCharacteristic_SMPTE_DCDM",
                    id(0x04010101, 0x0107, 0x0000, 0x060e2b3404010108),
                ),
                (
                    "TransferCharacteristic_IEC6196624_xvYCC",
                    id(0x04010101, 0x0108, 0x0000, 0x060e2b340401010d),
                ),
                (
                    "TransferCharacteristic_ITU2020",
                    id(0x04010101, 0x0109, 0x0000, 0x060e2b340401010e),
                ),
                (
                    "TransferCharacteristic_SMPTEST2084",
                    id(0x04010101, 0x010a, 0x0000, 0x060e2b340401010d),
                ),
                (
                    "TransferCharacteristic_HLG_OETF",
                    id(0x04010101, 0x010b, 0x0000, 0x060e2b340401010d),
                ),
            ],
        },
    },
    TypeRow {
        auid: id(0x02020103, 0x0000, 0x0000, 0x060e2b3401040101),
        name: "PluginCategoryType",
        kind: TypeKind::ExtEnum {
            elements: &[
                (
                    "PluginCategory_Effect",
                    id(0x0d010102, 0x0101, 0x0200, 0x060e2b3404010101),
                ),
                (
                    "PluginCategory_Codec",
                    id(0x0d010102, 0x0101, 0x0300, 0x060e2b3404010101),
                ),
                (
                    "PluginCategory_Interpolation",
                    id(0x0d010102, 0x0101, 0x0400, 0x060e2b3404010101),
                ),
            ],
        },
    },
    TypeRow {
        auid: id(0x02020104, 0x0000, 0x0000, 0x060e2b3401040101),
        name: "UsageType",
        kind: TypeKind::ExtEnum {
            elements: &[
                (
                    "Usage_SubClip",
                    id(0x0d010102, 0x0101, 0x0500, 0x060e2b3404010101),
                ),
                (
                    "Usage_AdjustedClip",
                    id(0x0d010102, 0x0101, 0x0600, 0x060e2b3404010101),
                ),
                (
                    "Usage_TopLevel",
                    id(0x0d010102, 0x0101, 0x0700, 0x060e2b3404010101),
                ),
                (
                    "Usage_LowerLevel",
                    id(0x0d010102, 0x0101, 0x0800, 0x060e2b3404010101),
                ),
                (
                    "Usage_Template",
                    id(0x0d010102, 0x0101, 0x0900, 0x060e2b3404010101),
                ),
            ],
        },
    },
    TypeRow {
        auid: id(0x02020105, 0x0000, 0x0000, 0x060e2b3401040101),
        name: "ColorPrimariesType",
        kind: TypeKind::ExtEnum {
            elements: &[
                (
                    "ColorPrimaries_SMPTE170M",
                    id(0x04010101, 0x0301, 0x0000, 0x060e2b3404010106),
                ),
                (
                    "ColorPrimaries_ITU470_PAL",
                    id(0x04010101, 0x0302, 0x0000, 0x060e2b3404010106),
                ),
                (
                    "ColorPrimaries_ITU709",
                    id(0x04010101, 0x0303, 0x0000, 0x060e2b3404010106),
                ),
                (
                    "ColorPrimaries_ITU2020",
                    id(0x04010101, 0x0304, 0x0000, 0x060e2b340401010d),
                ),
                (
                    "ColorPrimaries_SMPTE_DCDM",
                    id(0x04010101, 0x0305, 0x0000, 0x060e2b340401010d),
                ),
                (
                    "ColorPrimaries_P3D65",
                    id(0x04010101, 0x0306, 0x0000, 0x060e2b340401010d),
                ),
            ],
        },
    },
    TypeRow {
        auid: id(0x02020106, 0x0000, 0x0000, 0x060e2b3401040101),
        name: "CodingEquationsType",
        kind: TypeKind::ExtEnum {
            elements: &[
                (
                    "CodingEquations_ITU601",
                    id(0x04010101, 0x0201, 0x0000, 0x060e2b3404010101),
                ),
                (
                    "CodingEquations_ITU709",
                    id(0x04010101, 0x0202, 0x0000, 0x060e2b3404010101),
                ),
                (
                    "CodingEquations_SMPTE240M",
                    id(0x04010101, 0x0203, 0x0000, 0x060e2b3404010101),
                ),
                (
                    "CodingEquations_YCgCo",
                    id(0x04010101, 0x0204, 0x0000, 0x060e2b340401010d),
                ),
                (
                    "CodingEquations_GBR",
                    id(0x04010101, 0x0205, 0x0000, 0x060e2b340401010d),
                ),
                (
                    "CodingEquations_ITU2020_NCL",
                    id(0x04010101, 0x0206, 0x0000, 0x060e2b340401010d),
                ),
            ],
        },
    },
    TypeRow {
        auid: id(0x01100100, 0x0000, 0x0000, 0x060e2b3401040101),
        name: "aafCharacter",
        kind: TypeKind::Character,
    },
    TypeRow {
        auid: id(0x01100300, 0x0000, 0x0000, 0x060e2b3401040101),
        name: "aafChar",
        kind: TypeKind::GenericCharacter { size: 1 },
    },
    TypeRow {
        auid: id(0x04100300, 0x0000, 0x0000, 0x060e2b3401040101),
        name: "aafIndirect",
        kind: TypeKind::Indirect,
    },
    TypeRow {
        auid: id(0x04030100, 0x0000, 0x0000, 0x060e2b3401040101),
        name: "AUIDSet",
        kind: TypeKind::Set {
            element: id(0x01030100, 0x0000, 0x0000, 0x060e2b3401040101),
        },
    },
    TypeRow {
        auid: id(0x04030200, 0x0000, 0x0000, 0x060e2b3401040101),
        name: "UInt32Set",
        kind: TypeKind::Set {
            element: id(0x01010300, 0x0000, 0x0000, 0x060e2b3401040101),
        },
    },
    TypeRow {
        auid: id(0x05050100, 0x0000, 0x0000, 0x060e2b3401040101),
        name: "kAAFTypeID_ClassDefinitionStrongReferenceSet",
        kind: TypeKind::Set {
            element: id(0x05020900, 0x0000, 0x0000, 0x060e2b3401040101),
        },
    },
    TypeRow {
        auid: id(0x05050200, 0x0000, 0x0000, 0x060e2b3401040101),
        name: "kAAFTypeID_CodecDefinitionStrongReferenceSet",
        kind: TypeKind::Set {
            element: id(0x05020a00, 0x0000, 0x0000, 0x060e2b3401040101),
        },
    },
    TypeRow {
        auid: id(0x05050300, 0x0000, 0x0000, 0x060e2b3401040101),
        name: "kAAFTypeID_ContainerDefinitionStrongReferenceSet",
        kind: TypeKind::Set {
            element: id(0x05020c00, 0x0000, 0x0000, 0x060e2b3401040101),
        },
    },
    TypeRow {
        auid: id(0x05050400, 0x0000, 0x0000, 0x060e2b3401040101),
        name: "kAAFTypeID_DataDefinitionStrongReferenceSet",
        kind: TypeKind::Set {
            element: id(0x05020e00, 0x0000, 0x0000, 0x060e2b3401040101),
        },
    },
    TypeRow {
        auid: id(0x05050500, 0x0000, 0x0000, 0x060e2b3401040101),
        name: "kAAFTypeID_EssenceDataStrongReferenceSet",
        kind: TypeKind::Set {
            element: id(0x05020f00, 0x0000, 0x0000, 0x060e2b3401040101),
        },
    },
    TypeRow {
        auid: id(0x05050600, 0x0000, 0x0000, 0x060e2b3401040101),
        name: "kAAFTypeID_InterpolationDefinitionStrongReferenceSet",
        kind: TypeKind::Set {
            element: id(0x05021100, 0x0000, 0x0000, 0x060e2b3401040101),
        },
    },
    TypeRow {
        auid: id(0x05050700, 0x0000, 0x0000, 0x060e2b3401040101),
        name: "kAAFTypeID_MobStrongReferenceSet",
        kind: TypeKind::Set {
            element: id(0x05021300, 0x0000, 0x0000, 0x060e2b3401040101),
        },
    },
    TypeRow {
        auid: id(0x05050800, 0x0000, 0x0000, 0x060e2b3401040101),
        name: "kAAFTypeID_OperationDefinitionStrongReferenceSet",
        kind: TypeKind::Set {
            element: id(0x05021500, 0x0000, 0x0000, 0x060e2b3401040101),
        },
    },
    TypeRow {
        auid: id(0x05050900, 0x0000, 0x0000, 0x060e2b3401040101),
        name: "kAAFTypeID_ParameterDefinitionStrongReferenceSet",
        kind: TypeKind::Set {
            element: id(0x05021700, 0x0000, 0x0000, 0x060e2b3401040101),
        },
    },
    TypeRow {
        auid: id(0x05050a00, 0x0000, 0x0000, 0x060e2b3401040101),
        name: "kAAFTypeID_PluginDefinitionStrongReferenceSet",
        kind: TypeKind::Set {
            element: id(0x05021800, 0x0000, 0x0000, 0x060e2b3401040101),
        },
    },
    TypeRow {
        auid: id(0x05050b00, 0x0000, 0x0000, 0x060e2b3401040101),
        name: "kAAFTypeID_PropertyDefinitionStrongReferenceSet",
        kind: TypeKind::Set {
            element: id(0x05021900, 0x0000, 0x0000, 0x060e2b3401040101),
        },
    },
    TypeRow {
        auid: id(0x05050c00, 0x0000, 0x0000, 0x060e2b3401040101),
        name: "kAAFTypeID_TypeDefinitionStrongReferenceSet",
        kind: TypeKind::Set {
            element: id(0x05021b00, 0x0000, 0x0000, 0x060e2b3401040101),
        },
    },
    TypeRow {
        auid: id(0x05050d00, 0x0000, 0x0000, 0x060e2b3401040101),
        name: "kAAFTypeID_KLVDataDefinitionStrongReferenceSet",
        kind: TypeKind::Set {
            element: id(0x05022000, 0x0000, 0x0000, 0x060e2b3401040101),
        },
    },
    TypeRow {
        auid: id(0x05050e00, 0x0000, 0x0000, 0x060e2b3401040101),
        name: "kAAFTypeID_TaggedValueDefinitionStrongReferenceSet",
        kind: TypeKind::Set {
            element: id(0x05022100, 0x0000, 0x0000, 0x060e2b3401040101),
        },
    },
    TypeRow {
        auid: id(0x05050f00, 0x0000, 0x0000, 0x060e2b3401040101),
        name: "kAAFTypeID_DescriptiveObjectStrongReferenceSet",
        kind: TypeKind::Set {
            element: id(0x05022200, 0x0000, 0x0000, 0x060e2b3401040101),
        },
    },
    TypeRow {
        auid: id(0x05030d00, 0x0000, 0x0000, 0x060e2b3401040101),
        name: "kAAFTypeID_DataDefinitionWeakReferenceSet",
        kind: TypeKind::Set {
            element: id(0x05010300, 0x0000, 0x0000, 0x060e2b3401040101),
        },
    },
    TypeRow {
        auid: id(0x05030e00, 0x0000, 0x0000, 0x060e2b3401040101),
        name: "kAAFTypeID_ParameterDefinitionWeakReferenceSet",
        kind: TypeKind::Set {
            element: id(0x05010800, 0x0000, 0x0000, 0x060e2b3401040101),
        },
    },
    TypeRow {
        auid: id(0x05030f00, 0x0000, 0x0000, 0x060e2b3401040101),
        name: "kAAFTypeID_PluginDefinitionWeakReferenceSet",
        kind: TypeKind::Set {
            element: id(0x05010a00, 0x0000, 0x0000, 0x060e2b3401040101),
        },
    },
    TypeRow {
        auid: id(0x05031000, 0x0000, 0x0000, 0x060e2b3401040101),
        name: "kAAFTypeID_PropertyDefinitionWeakReferenceSet",
        kind: TypeKind::Set {
            element: id(0x05010c00, 0x0000, 0x0000, 0x060e2b3401040101),
        },
    },
    TypeRow {
        auid: id(0x05020100, 0x0000, 0x0000, 0x060e2b3401040101),
        name: "kAAFTypeID_ContentStorageStrongReference",
        kind: TypeKind::StrongRef {
            class: id(0x0d010101, 0x0101, 0x1800, 0x060e2b3402060101),
        },
    },
    TypeRow {
        auid: id(0x05020200, 0x0000, 0x0000, 0x060e2b3401040101),
        name: "kAAFTypeID_DictionaryStrongReference",
        kind: TypeKind::StrongRef {
            class: id(0x0d010101, 0x0101, 0x2200, 0x060e2b3402060101),
        },
    },
    TypeRow {
        auid: id(0x05020300, 0x0000, 0x0000, 0x060e2b3401040101),
        name: "kAAFTypeID_EssenceDescriptorStrongReference",
        kind: TypeKind::StrongRef {
            class: id(0x0d010101, 0x0101, 0x2400, 0x060e2b3402060101),
        },
    },
    TypeRow {
        auid: id(0x05020400, 0x0000, 0x0000, 0x060e2b3401040101),
        name: "kAAFTypeID_NetworkLocatorStrongReference",
        kind: TypeKind::StrongRef {
            class: id(0x0d010101, 0x0101, 0x3200, 0x060e2b3402060101),
        },
    },
    TypeRow {
        auid: id(0x05020500, 0x0000, 0x0000, 0x060e2b3401040101),
        name: "kAAFTypeID_OperationGroupStrongReference",
        kind: TypeKind::StrongRef {
            class: id(0x0d010101, 0x0101, 0x0a00, 0x060e2b3402060101),
        },
    },
    TypeRow {
        auid: id(0x05020600, 0x0000, 0x0000, 0x060e2b3401040101),
        name: "kAAFTypeID_SegmentStrongReference",
        kind: TypeKind::StrongRef {
            class: id(0x0d010101, 0x0101, 0x0300, 0x060e2b3402060101),
        },
    },
    TypeRow {
        auid: id(0x05020700, 0x0000, 0x0000, 0x060e2b3401040101),
        name: "kAAFTypeID_SourceClipStrongReference",
        kind: TypeKind::StrongRef {
            class: id(0x0d010101, 0x0101, 0x1100, 0x060e2b3402060101),
        },
    },
    TypeRow {
        auid: id(0x05020800, 0x0000, 0x0000, 0x060e2b3401040101),
        name: "kAAFTypeID_SourceReferenceStrongReference",
        kind: TypeKind::StrongRef {
            class: id(0x0d010101, 0x0101, 0x1000, 0x060e2b3402060101),
        },
    },
    TypeRow {
        auid: id(0x05020900, 0x0000, 0x0000, 0x060e2b3401040101),
        name: "kAAFTypeID_ClassDefinitionStrongReference",
        kind: TypeKind::StrongRef {
            class: id(0x0d010101, 0x0201, 0x0000, 0x060e2b3402060101),
        },
    },
    TypeRow {
        auid: id(0x05020a00, 0x0000, 0x0000, 0x060e2b3401040101),
        name: "kAAFTypeID_CodecDefinitionStrongReference",
        kind: TypeKind::StrongRef {
            class: id(0x0d010101, 0x0101, 0x1f00, 0x060e2b3402060101),
        },
    },
    TypeRow {
        auid: id(0x05020b00, 0x0000, 0x0000, 0x060e2b3401040101),
        name: "kAAFTypeID_ComponentStrongReference",
        kind: TypeKind::StrongRef {
            class: id(0x0d010101, 0x0101, 0x0200, 0x060e2b3402060101),
        },
    },
    TypeRow {
        auid: id(0x05020c00, 0x0000, 0x0000, 0x060e2b3401040101),
        name: "kAAFTypeID_ContainerDefinitionStrongReference",
        kind: TypeKind::StrongRef {
            class: id(0x0d010101, 0x0101, 0x2000, 0x060e2b3402060101),
        },
    },
    TypeRow {
        auid: id(0x05020d00, 0x0000, 0x0000, 0x060e2b3401040101),
        name: "kAAFTypeID_ControlPointStrongReference",
        kind: TypeKind::StrongRef {
            class: id(0x0d010101, 0x0101, 0x1900, 0x060e2b3402060101),
        },
    },
    TypeRow {
        auid: id(0x05020e00, 0x0000, 0x0000, 0x060e2b3401040101),
        name: "kAAFTypeID_DataDefinitionStrongReference",
        kind: TypeKind::StrongRef {
            class: id(0x0d010101, 0x0101, 0x1b00, 0x060e2b3402060101),
        },
    },
    TypeRow {
        auid: id(0x05020f00, 0x0000, 0x0000, 0x060e2b3401040101),
        name: "kAAFTypeID_EssenceDataStrongReference",
        kind: TypeKind::StrongRef {
            class: id(0x0d010101, 0x0101, 0x2300, 0x060e2b3402060101),
        },
    },
    TypeRow {
        auid: id(0x05021000, 0x0000, 0x0000, 0x060e2b3401040101),
        name: "kAAFTypeID_IdentificationStrongReference",
        kind: TypeKind::StrongRef {
            class: id(0x0d010101, 0x0101, 0x3000, 0x060e2b3402060101),
        },
    },
    TypeRow {
        auid: id(0x05021100, 0x0000, 0x0000, 0x060e2b3401040101),
        name: "kAAFTypeID_InterpolationDefinitionStrongReference",
        kind: TypeKind::StrongRef {
            class: id(0x0d010101, 0x0101, 0x2100, 0x060e2b3402060101),
        },
    },
    TypeRow {
        auid: id(0x05021200, 0x0000, 0x0000, 0x060e2b3401040101),
        name: "kAAFTypeID_LocatorStrongReference",
        kind: TypeKind::StrongRef {
            class: id(0x0d010101, 0x0101, 0x3100, 0x060e2b3402060101),
        },
    },
    TypeRow {
        auid: id(0x05021300, 0x0000, 0x0000, 0x060e2b3401040101),
        name: "kAAFTypeID_MobStrongReference",
        kind: TypeKind::StrongRef {
            class: id(0x0d010101, 0x0101, 0x3400, 0x060e2b3402060101),
        },
    },
    TypeRow {
        auid: id(0x05021400, 0x0000, 0x0000, 0x060e2b3401040101),
        name: "kAAFTypeID_MobSlotStrongReference",
        kind: TypeKind::StrongRef {
            class: id(0x0d010101, 0x0101, 0x3800, 0x060e2b3402060101),
        },
    },
    TypeRow {
        auid: id(0x05021500, 0x0000, 0x0000, 0x060e2b3401040101),
        name: "kAAFTypeID_OperationDefinitionStrongReference",
        kind: TypeKind::StrongRef {
            class: id(0x0d010101, 0x0101, 0x1c00, 0x060e2b3402060101),
        },
    },
    TypeRow {
        auid: id(0x05021600, 0x0000, 0x0000, 0x060e2b3401040101),
        name: "kAAFTypeID_ParameterStrongReference",
        kind: TypeKind::StrongRef {
            class: id(0x0d010101, 0x0101, 0x3c00, 0x060e2b3402060101),
        },
    },
    TypeRow {
        auid: id(0x05021700, 0x0000, 0x0000, 0x060e2b3401040101),
        name: "kAAFTypeID_ParameterDefinitionStrongReference",
        kind: TypeKind::StrongRef {
            class: id(0x0d010101, 0x0101, 0x1d00, 0x060e2b3402060101),
        },
    },
    TypeRow {
        auid: id(0x05021800, 0x0000, 0x0000, 0x060e2b3401040101),
        name: "kAAFTypeID_PluginDefinitionStrongReference",
        kind: TypeKind::StrongRef {
            class: id(0x0d010101, 0x0101, 0x1e00, 0x060e2b3402060101),
        },
    },
    TypeRow {
        auid: id(0x05021900, 0x0000, 0x0000, 0x060e2b3401040101),
        name: "kAAFTypeID_PropertyDefinitionStrongReference",
        kind: TypeKind::StrongRef {
            class: id(0x0d010101, 0x0202, 0x0000, 0x060e2b3402060101),
        },
    },
    TypeRow {
        auid: id(0x05021a00, 0x0000, 0x0000, 0x060e2b3401040101),
        name: "kAAFTypeID_TaggedValueStrongReference",
        kind: TypeKind::StrongRef {
            class: id(0x0d010101, 0x0101, 0x3f00, 0x060e2b3402060101),
        },
    },
    TypeRow {
        auid: id(0x05021b00, 0x0000, 0x0000, 0x060e2b3401040101),
        name: "kAAFTypeID_TypeDefinitionStrongReference",
        kind: TypeKind::StrongRef {
            class: id(0x0d010101, 0x0203, 0x0000, 0x060e2b3402060101),
        },
    },
    TypeRow {
        auid: id(0x05021c00, 0x0000, 0x0000, 0x060e2b3401040101),
        name: "kAAFTypeID_KLVDataStrongReference",
        kind: TypeKind::StrongRef {
            class: id(0x0d010101, 0x0101, 0x4000, 0x060e2b3402060101),
        },
    },
    TypeRow {
        auid: id(0x05021d00, 0x0000, 0x0000, 0x060e2b3401040101),
        name: "kAAFTypeID_FileDescriptorStrongReference",
        kind: TypeKind::StrongRef {
            class: id(0x0d010101, 0x0101, 0x2500, 0x060e2b3402060101),
        },
    },
    TypeRow {
        auid: id(0x05021e00, 0x0000, 0x0000, 0x060e2b3401040101),
        name: "kAAFTypeID_RIFFChunkStrongReference",
        kind: TypeKind::StrongRef {
            class: id(0x0d010101, 0x0101, 0x4f00, 0x060e2b3402060101),
        },
    },
    TypeRow {
        auid: id(0x05021f00, 0x0000, 0x0000, 0x060e2b3401040101),
        name: "kAAFTypeID_DescriptiveFrameworkStrongReference",
        kind: TypeKind::StrongRef {
            class: id(0x0d010401, 0x0000, 0x0000, 0x060e2b3402060101),
        },
    },
    TypeRow {
        auid: id(0x05022000, 0x0000, 0x0000, 0x060e2b3401040101),
        name: "kAAFTypeID_KLVDataDefinitionStrongReference",
        kind: TypeKind::StrongRef {
            class: id(0x0d010101, 0x0101, 0x4d00, 0x060e2b3402060101),
        },
    },
    TypeRow {
        auid: id(0x05022100, 0x0000, 0x0000, 0x060e2b3401040101),
        name: "kAAFTypeID_TaggedValueDefinitionStrongReference",
        kind: TypeKind::StrongRef {
            class: id(0x0d010101, 0x0101, 0x4c00, 0x060e2b3402060101),
        },
    },
    TypeRow {
        auid: id(0x05022200, 0x0000, 0x0000, 0x060e2b3401040101),
        name: "kAAFTypeID_DescriptiveObjectStrongReference",
        kind: TypeKind::StrongRef {
            class: id(0x0d010400, 0x0000, 0x0000, 0x060e2b3402060101),
        },
    },
    TypeRow {
        auid: id(0x05022600, 0x0000, 0x0000, 0x060e2b3401040101),
        name: "kAAFTypeID_SubDescriptorStrongReference",
        kind: TypeKind::StrongRef {
            class: id(0x0d010101, 0x0101, 0x5900, 0x060e2b3402060101),
        },
    },
    TypeRow {
        auid: id(0x05010100, 0x0000, 0x0000, 0x060e2b3401040101),
        name: "ClassDefinitionWeakReference",
        kind: TypeKind::WeakRef {
            class: id(0x0d010101, 0x0201, 0x0000, 0x060e2b3402060101),
            target: &[
                id(0x0d010301, 0x0101, 0x0100, 0x060e2b3401010102),
                id(0x06010107, 0x0700, 0x0000, 0x060e2b3401010102),
            ],
        },
    },
    TypeRow {
        auid: id(0x05010200, 0x0000, 0x0000, 0x060e2b3401040101),
        name: "ContainerDefinitionWeakReference",
        kind: TypeKind::WeakRef {
            class: id(0x0d010101, 0x0101, 0x2000, 0x060e2b3402060101),
            target: &[
                id(0x0d010301, 0x0102, 0x0100, 0x060e2b3401010102),
                id(0x06010104, 0x0202, 0x0000, 0x060e2b3401010102),
                id(0x06010104, 0x0508, 0x0000, 0x060e2b3401010102),
            ],
        },
    },
    TypeRow {
        auid: id(0x05010300, 0x0000, 0x0000, 0x060e2b3401040101),
        name: "DataDefinitionWeakReference",
        kind: TypeKind::WeakRef {
            class: id(0x0d010101, 0x0101, 0x1b00, 0x060e2b3402060101),
            target: &[
                id(0x0d010301, 0x0102, 0x0100, 0x060e2b3401010102),
                id(0x06010104, 0x0202, 0x0000, 0x060e2b3401010102),
                id(0x06010104, 0x0505, 0x0000, 0x060e2b3401010102),
            ],
        },
    },
    TypeRow {
        auid: id(0x05010500, 0x0000, 0x0000, 0x060e2b3401040101),
        name: "InterpolationDefinitionWeakReference",
        kind: TypeKind::WeakRef {
            class: id(0x0d010101, 0x0101, 0x2100, 0x060e2b3402060101),
            target: &[
                id(0x0d010301, 0x0102, 0x0100, 0x060e2b3401010102),
                id(0x06010104, 0x0202, 0x0000, 0x060e2b3401010102),
                id(0x06010104, 0x0509, 0x0000, 0x060e2b3401010102),
            ],
        },
    },
    TypeRow {
        auid: id(0x05010600, 0x0000, 0x0000, 0x060e2b3401040101),
        name: "MobWeakReference",
        kind: TypeKind::WeakRef {
            class: id(0x0d010101, 0x0101, 0x3400, 0x060e2b3402060101),
            target: &[
                id(0x0d010301, 0x0102, 0x0100, 0x060e2b3401010102),
                id(0x06010104, 0x0201, 0x0000, 0x060e2b3401010102),
                id(0x06010104, 0x0501, 0x0000, 0x060e2b3401010102),
            ],
        },
    },
    TypeRow {
        auid: id(0x05010700, 0x0000, 0x0000, 0x060e2b3401040101),
        name: "OperationDefinitionWeakReference",
        kind: TypeKind::WeakRef {
            class: id(0x0d010101, 0x0101, 0x1c00, 0x060e2b3402060101),
            target: &[
                id(0x0d010301, 0x0102, 0x0100, 0x060e2b3401010102),
                id(0x06010104, 0x0202, 0x0000, 0x060e2b3401010102),
                id(0x06010104, 0x0503, 0x0000, 0x060e2b3401010102),
            ],
        },
    },
    TypeRow {
        auid: id(0x05010800, 0x0000, 0x0000, 0x060e2b3401040101),
        name: "ParameterDefinitionWeakReference",
        kind: TypeKind::WeakRef {
            class: id(0x0d010101, 0x0101, 0x1d00, 0x060e2b3402060101),
            target: &[
                id(0x0d010301, 0x0102, 0x0100, 0x060e2b3401010102),
                id(0x06010104, 0x0202, 0x0000, 0x060e2b3401010102),
                id(0x06010104, 0x0504, 0x0000, 0x060e2b3401010102),
            ],
        },
    },
    TypeRow {
        auid: id(0x05010900, 0x0000, 0x0000, 0x060e2b3401040101),
        name: "TypeDefinitionWeakReference",
        kind: TypeKind::WeakRef {
            class: id(0x0d010101, 0x0203, 0x0000, 0x060e2b3402060101),
            target: &[
                id(0x0d010301, 0x0101, 0x0100, 0x060e2b3401010102),
                id(0x06010107, 0x0800, 0x0000, 0x060e2b3401010102),
            ],
        },
    },
    TypeRow {
        auid: id(0x05010a00, 0x0000, 0x0000, 0x060e2b3401040101),
        name: "PluginDefinitionWeakReference",
        kind: TypeKind::WeakRef {
            class: id(0x0d010101, 0x0101, 0x1e00, 0x060e2b3402060101),
            target: &[
                id(0x0d010301, 0x0102, 0x0100, 0x060e2b3401010102),
                id(0x06010104, 0x0202, 0x0000, 0x060e2b3401010102),
                id(0x06010104, 0x0506, 0x0000, 0x060e2b3401010102),
            ],
        },
    },
    TypeRow {
        auid: id(0x05010b00, 0x0000, 0x0000, 0x060e2b3401040101),
        name: "CodecDefinitionWeakReference",
        kind: TypeKind::WeakRef {
            class: id(0x0d010101, 0x0101, 0x1f00, 0x060e2b3402060101),
            target: &[
                id(0x0d010301, 0x0102, 0x0100, 0x060e2b3401010102),
                id(0x06010104, 0x0202, 0x0000, 0x060e2b3401010102),
                id(0x06010104, 0x0507, 0x0000, 0x060e2b3401010102),
            ],
        },
    },
    TypeRow {
        auid: id(0x05010c00, 0x0000, 0x0000, 0x060e2b3401040101),
        name: "PropertyDefinitionWeakReference",
        kind: TypeKind::WeakRef {
            class: id(0x0d010101, 0x0202, 0x0000, 0x060e2b3402060101),
            target: &[
                id(0x0d010301, 0x0101, 0x0100, 0x060e2b3401010102),
                id(0x06010107, 0x0700, 0x0000, 0x060e2b3401010102),
                id(0x06010107, 0x0200, 0x0000, 0x060e2b3401010102),
            ],
        },
    },
];
