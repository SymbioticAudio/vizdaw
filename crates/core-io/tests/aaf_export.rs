//! The AAF export (plan 6f Task 4): the header, the full MetaDictionary
//! (Ruling 11), Ruling 4's layout, gain and automation (Ruling 6), stereo
//! (Ruling 7), the start timecode (Ruling 3), the MIDI file beside (Ruling
//! 8), what is listed and the names. Each file is read back with Task 1's
//! `AafReader`. No audio is converted, so Linux runs every test.

mod common;

use common::aaf::write_export;
use common::generators::{clap, media, output, route};
use core_engine::fixtures::{Builder, Q, demo_audio_document};
use core_io::aaf::dict::{class, def, pid};
use core_io::aaf::export::{
    AUTOMATION_MODES, AUTOMATION_OFF, AafExport, AafOptions, BUILTIN_SYNTH, CLIP_GAIN_OFF, COLOURS,
    EXPONENTIAL, EXTERNAL_MIDI, EXTRA_OUTPUTS, ExportEnv, FADE_INS_CUT, FADE_OUTS_CUT, Fps, INPUTS,
    INSERTS, KEYBOARD_SLOTS, MIDI_BESIDE, MUTE_SOLO, MUTED_REGIONS, OTHER_AUTOMATION, ROUTING,
    SCURVE, Stereo, TAKES, check, export, folder_name,
};
use core_io::aaf::metadict::{CLASSES, ROOT, TYPES, TypeKind, prop_row};
use core_io::aaf::store::{AafReader, Auid, ObjId, Prop, Store, WeakRef};
use core_io::dawproject::export::{LeftOutAudio, PICKUP};
use core_io::for_daw::midi_tracks;
use core_io::limits::AafLimits;
use core_io::midi_out::{SmfOptions, to_smf_until};
use core_io::report::{ItemKind, Report};
use core_model::automation::AutomationTarget;
use core_model::command::{AudioCmd, Command, CrossfadeLength, EventCmd, MixerCmd, SettingsCmd};
use core_model::document::{Document, SongInfo};
use core_model::event::{EventBody, Fade, FadeShape};
use core_model::format::BitDepth;
use core_model::ids::{LaneId, MediaId, SeqId, StripId};
use core_model::mixer::{AuxSend, StripKind};
use std::collections::BTreeSet;
use std::sync::atomic::AtomicBool;

const MODIFIED: u64 = 1_700_000_000;

fn no() -> AtomicBool {
    AtomicBool::new(false)
}

fn env() -> ExportEnv<'static> {
    ExportEnv {
        app_version: "1.2.3",
        export_id: "0f1e2d3c-4b5a-6978-8796-a5b4c3d2e1f0",
        modified: MODIFIED,
    }
}

fn opts(seq: SeqId) -> AafOptions {
    AafOptions {
        seq,
        embed: true,
        depth: BitDepth::Int24,
        stereo: Stereo::MonoPairs,
        hour_start: false,
        fps: Fps::F25,
        clip_gain: true,
        automation: true,
        left_out: LeftOutAudio::new(),
    }
}

/// A song "Song" with one audio track "Keys" on its own output, and a
/// media item "keys" of `channels` channels (10 s at 48 kHz).
struct Song {
    b: Builder,
    seq: SeqId,
    lane: LaneId,
    strip: StripId,
    media: MediaId,
}

fn song(channels: u16) -> Song {
    let mut b = Builder::new();
    let (seq, lane) = b.sequence("Song", "Keys", None).unwrap();
    let out = output(&mut b, "Keys");
    route(&mut b, seq, lane, out);
    let strip = b.strip_of(StripKind::AudioPath(out)).unwrap();
    let media = media(&mut b, "keys", 480_000, channels);
    Song {
        b,
        seq,
        lane,
        strip,
        media,
    }
}

impl Song {
    fn region(
        &mut self,
        tick: i64,
        offset: u64,
        length: u64,
        gain: f32,
        fin: u64,
    ) -> core_model::ids::EventId {
        let fade = Fade {
            length_frames: fin,
            shape: FadeShape::Linear,
        };
        self.b
            .region(
                self.seq,
                self.lane,
                tick,
                self.media,
                offset,
                length,
                gain,
                fade,
                Fade::default(),
            )
            .unwrap()
    }
}

fn exported(doc: &Document, o: &AafOptions) -> AafExport {
    export(doc, o, &env(), &no()).expect("exports")
}

/// The store of `doc` exported with `o`, written and read back.
fn round(doc: &Document, o: &AafOptions) -> (AafExport, Store) {
    let x = exported(doc, o);
    let dir = tempfile::tempdir().unwrap();
    let bits = if o.depth == BitDepth::Int16 { 16 } else { 24 };
    let p = write_export(doc, &x, dir.path(), bits);
    let r = AafReader::open(&p, &AafLimits::default(), &no()).expect("reads");
    (x, r.store().clone())
}

// ---------------------------------------------------------------- reading helpers

fn prop(s: &Store, id: ObjId, p: u16) -> Option<&Prop> {
    s.prop(id, p)
}

fn strong(s: &Store, id: ObjId, p: u16) -> ObjId {
    match prop(s, id, p) {
        Some(Prop::Strong(c)) => *c,
        other => panic!("{p:#06x} of {id:?} is {other:?}"),
    }
}

fn list(s: &Store, id: ObjId, p: u16) -> Vec<ObjId> {
    match prop(s, id, p) {
        Some(Prop::StrongVec(v) | Prop::StrongSet(v)) => v.clone(),
        _ => Vec::new(),
    }
}

fn data(s: &Store, id: ObjId, p: u16) -> Vec<u8> {
    match prop(s, id, p) {
        Some(Prop::Data(d)) => d.clone(),
        other => panic!("{p:#06x} of {id:?} is {other:?}"),
    }
}

fn text(s: &Store, id: ObjId, p: u16) -> String {
    let d = data(s, id, p);
    let units: Vec<u16> = d
        .chunks_exact(2)
        .map(|c| u16::from_le_bytes([c[0], c[1]]))
        .take_while(|u| *u != 0)
        .collect();
    String::from_utf16(&units).unwrap()
}

fn i64_of(s: &Store, id: ObjId, p: u16) -> i64 {
    i64::from_le_bytes(data(s, id, p).try_into().unwrap())
}

fn class_of(s: &Store, id: ObjId) -> Auid {
    s.get(id).unwrap().class
}

fn weak_id(s: &Store, id: ObjId, p: u16) -> Auid {
    match prop(s, id, p) {
        Some(Prop::Weak(w)) => Auid(w.id.clone().try_into().unwrap()),
        other => panic!("{p:#06x} of {id:?} is {other:?}"),
    }
}

fn header(s: &Store) -> ObjId {
    strong(s, s.root, pid::ROOT_HEADER)
}

fn mobs(s: &Store) -> Vec<ObjId> {
    let content = strong(s, header(s), pid::HEADER_CONTENT);
    list(s, content, pid::CONTENT_STORAGE_MOBS)
}

fn composition(s: &Store) -> ObjId {
    let comps: Vec<ObjId> = mobs(s)
        .into_iter()
        .filter(|m| class_of(s, *m) == class::COMPOSITION_MOB)
        .collect();
    assert_eq!(comps.len(), 1, "one composition");
    comps[0]
}

/// The composition's sound slots: (name, the slot).
fn sound_slots(s: &Store) -> Vec<(String, ObjId)> {
    list(s, composition(s), pid::MOB_SLOTS)
        .into_iter()
        .filter(|sl| class_of(s, *sl) == class::TIMELINE_MOB_SLOT)
        .filter(|sl| {
            let seg = strong(s, *sl, pid::MOB_SLOT_SEGMENT);
            weak_id(s, seg, pid::COMPONENT_DATA_DEFINITION) == def::DATA_DEF_SOUND
        })
        .map(|sl| (text(s, sl, pid::MOB_SLOT_SLOT_NAME), sl))
        .collect()
}

/// The groups around a slot's sequence (outermost first) and the sequence.
fn peel(s: &Store, slot: ObjId) -> (Vec<ObjId>, ObjId) {
    let mut seg = strong(s, slot, pid::MOB_SLOT_SEGMENT);
    let mut groups = Vec::new();
    while class_of(s, seg) == class::OPERATION_GROUP {
        groups.push(seg);
        seg = list(s, seg, pid::OPERATION_GROUP_INPUT_SEGMENTS)[0];
    }
    assert_eq!(class_of(s, seg), class::SEQUENCE);
    (groups, seg)
}

fn class_name(a: Auid) -> &'static str {
    CLASSES.iter().find(|c| c.auid == a).map_or("?", |c| c.name)
}

/// A parameter's value: a rational (direct or indirect).
fn ratio(s: &Store, id: ObjId, p: u16) -> (i32, i32) {
    let d = data(s, id, p);
    let v = &d[d.len() - 8..];
    (
        i32::from_le_bytes(v[..4].try_into().unwrap()),
        i32::from_le_bytes(v[4..].try_into().unwrap()),
    )
}

fn has(r: &Report, kind: ItemKind, what: &str) -> bool {
    r.items.iter().any(|i| i.kind == kind && i.what == what)
}

// ---------------------------------------------------------------- the header

#[test]
fn writes_the_edit_protocol_header() {
    let mut g = song(1);
    g.region(0, 0, 48_000, 0.0, 0);
    let doc = g.b.finish();
    let x = exported(&doc, &opts(g.seq));
    let dir = tempfile::tempdir().unwrap();
    let p = write_export(&doc, &x, dir.path(), 24);
    let s = AafReader::open(&p, &AafLimits::default(), &no())
        .unwrap()
        .store()
        .clone();
    let h = header(&s);
    assert_eq!(
        Auid(
            data(&s, h, pid::HEADER_OPERATIONAL_PATTERN)
                .try_into()
                .unwrap()
        ),
        def::OP_DEF_EDIT_PROTOCOL
    );
    assert_eq!(data(&s, h, pid::HEADER_BYTE_ORDER), 0x4949u16.to_le_bytes());
    assert_eq!(data(&s, h, pid::HEADER_VERSION), [1, 1]);
    let ids = list(&s, h, pid::HEADER_IDENTIFICATION_LIST);
    assert_eq!(ids.len(), 1);
    let i = ids[0];
    assert_eq!(text(&s, i, pid::IDENTIFICATION_PRODUCT_NAME), "VizDAW");
    assert_eq!(text(&s, i, pid::IDENTIFICATION_COMPANY_NAME), "VizDAW");
    assert_eq!(
        text(&s, i, pid::IDENTIFICATION_PRODUCT_VERSION_STRING),
        "1.2.3"
    );
    assert_eq!(text(&s, i, pid::IDENTIFICATION_PLATFORM), "macOS");
    assert_eq!(
        Auid(
            data(&s, i, pid::IDENTIFICATION_PRODUCT_ID)
                .try_into()
                .unwrap()
        ),
        def::VIZDAW_PRODUCT_ID
    );
    // 1 700 000 000 s is 2023-11-14 22:13:20 UTC: year (i16), month, day,
    // hour, minute, second, fraction.
    let stamp = vec![0xE7, 0x07, 11, 14, 22, 13, 20, 0];
    assert_eq!(data(&s, i, pid::IDENTIFICATION_DATE), stamp);
    assert_eq!(data(&s, h, pid::HEADER_LAST_MODIFIED), stamp);
    for m in mobs(&s) {
        assert_eq!(data(&s, m, pid::MOB_CREATION_TIME), stamp);
        assert_eq!(data(&s, m, pid::MOB_LAST_MODIFIED), stamp);
    }
    let comp = composition(&s);
    assert_eq!(
        weak_or_data(&s, comp, pid::MOB_USAGE_CODE),
        def::USAGE_TOP_LEVEL
    );
    assert_eq!(text(&s, comp, pid::MOB_NAME), "Song");
    // 4 KiB sectors.
    let f = cfb::CompoundFile::open(std::fs::File::open(&p).unwrap()).unwrap();
    assert_eq!(f.version(), cfb::Version::V4);
}

fn weak_or_data(s: &Store, id: ObjId, p: u16) -> Auid {
    match prop(s, id, p) {
        Some(Prop::Data(d)) => Auid(d.clone().try_into().unwrap()),
        _ => weak_id(s, id, p),
    }
}

// ---------------------------------------------------------------- the MetaDictionary

/// Every object reachable from the root, with its parent's class (none for
/// the root).
fn objects(s: &Store) -> Vec<ObjId> {
    let mut out = vec![s.root];
    let mut i = 0;
    while i < out.len() {
        let o = s.get(out[i]).unwrap();
        for p in o.props.values() {
            match p {
                Prop::Strong(c) => out.push(*c),
                Prop::StrongVec(v) | Prop::StrongSet(v) => out.extend(v.iter().copied()),
                _ => {}
            }
        }
        i += 1;
    }
    out
}

/// The object a weak reference names, if any.
fn resolve(s: &Store, w: &WeakRef) -> Option<ObjId> {
    let path = s.referenced.get(usize::from(w.table_index))?;
    let (last, head) = path.split_last()?;
    let mut at = s.root;
    for p in head {
        at = match s.prop(at, *p)? {
            Prop::Strong(c) => *c,
            _ => return None,
        };
    }
    let set = match s.prop(at, *last)? {
        Prop::StrongSet(v) | Prop::StrongVec(v) => v.clone(),
        _ => return None,
    };
    set.into_iter()
        .find(|e| matches!(s.prop(*e, w.id_pid), Some(Prop::Data(d)) if *d == w.id))
}

fn class_known(a: Auid) -> bool {
    a == ROOT.auid || CLASSES.iter().any(|c| c.auid == a)
}

#[test]
fn writes_a_full_meta_dictionary() {
    let mut g = song(2);
    g.region(0, 0, 48_000, -6.0, 480);
    let doc = g.b.finish();
    let (_, s) = round(&doc, &opts(g.seq));
    let md = strong(&s, s.root, pid::ROOT_META_DICTIONARY);
    let classes = list(&s, md, pid::META_DICTIONARY_CLASS_DEFINITIONS);
    let types = list(&s, md, pid::META_DICTIONARY_TYPE_DEFINITIONS);
    assert_eq!(classes.len(), CLASSES.len());
    assert_eq!(types.len(), TYPES.len());
    let type_ids: BTreeSet<[u8; 16]> = types
        .iter()
        .map(|t| {
            data(&s, *t, pid::META_DEFINITION_IDENTIFICATION)
                .try_into()
                .unwrap()
        })
        .collect();
    let mut weak = 0;
    for o in objects(&s) {
        let obj = s.get(o).unwrap();
        assert!(
            class_known(obj.class),
            "{} has class {}",
            obj.path,
            obj.class
        );
        for (p, v) in &obj.props {
            assert!(
                prop_row(obj.class, *p).is_some(),
                "{} ({}) holds {p:#06x}, which its class does not define",
                obj.path,
                class_name(obj.class)
            );
            let refs: Vec<&WeakRef> = match v {
                Prop::Weak(w) => vec![w],
                Prop::WeakVec(ws) | Prop::WeakSet(ws) => ws.iter().collect(),
                _ => Vec::new(),
            };
            for w in refs {
                weak += 1;
                assert!(
                    resolve(&s, w).is_some(),
                    "{} {p:#06x} dangles: {w:?}",
                    obj.path
                );
            }
        }
        // A property definition's type (an AUID) names a written type.
        if obj.class == class::PROPERTY_DEFINITION {
            let t: [u8; 16] = data(&s, o, pid::PROPERTY_DEFINITION_TYPE)
                .try_into()
                .unwrap();
            assert!(
                type_ids.contains(&t),
                "{} names an unwritten type",
                obj.path
            );
        }
        if obj.class == class::PARAMETER_DEFINITION {
            assert!(matches!(
                prop(&s, o, pid::PARAMETER_DEFINITION_TYPE),
                Some(Prop::Weak(_))
            ));
        }
        if obj.class == class::CLASS_DEFINITION {
            assert!(matches!(
                prop(&s, o, pid::CLASS_DEFINITION_PARENT_CLASS),
                Some(Prop::Weak(_))
            ));
        }
    }
    // Every class's ParentClass at least: the metadict's references are
    // checked too.
    assert!(weak > CLASSES.len(), "{weak}");
}

#[test]
fn required_properties_are_written() {
    let mut g = song(2);
    g.region(0, 0, 48_000, -6.0, 480);
    g.b.events(
        g.seq,
        g.lane,
        (0..3)
            .map(|k| {
                core_model::event::EventSpec::new(
                    core_model::time::Tick(k * Q),
                    EventBody::Automation {
                        target: AutomationTarget::StripFader(g.strip),
                        value: 0.5,
                    },
                )
            })
            .collect(),
    )
    .unwrap();
    let doc = g.b.finish();
    for embed in [true, false] {
        let mut o = opts(g.seq);
        o.embed = embed;
        o.hour_start = true;
        let (_, s) = round(&doc, &o);
        for id in objects(&s) {
            let obj = s.get(id).unwrap();
            let chain = core_io::aaf::metadict::lineage(obj.class);
            assert!(!chain.is_empty() || obj.class == ROOT.auid);
            for c in chain {
                for p in c.props {
                    // `ObjClass` is the storage's CLSID (Object
                    // Specification), never a property.
                    if p.optional || p.pid == pid::INTERCHANGE_OBJECT_OBJ_CLASS {
                        continue;
                    }
                    assert!(
                        obj.props.contains_key(&p.pid),
                        "{} ({}) lacks {}.{} ({:#06x})",
                        obj.path,
                        class_name(obj.class),
                        c.name,
                        p.name,
                        p.pid
                    );
                }
            }
        }
    }
}

#[test]
fn dict_agrees_with_the_meta_dictionary() {
    let row = |name: &str| {
        if name == "Root" {
            return &ROOT;
        }
        CLASSES
            .iter()
            .find(|c| c.name == name)
            .unwrap_or_else(|| panic!("no class {name}"))
    };
    for (a, name) in CLASS_NAMES {
        assert_eq!(row(name).auid, *a, "class {name}");
    }
    for (p, c, name) in PID_NAMES {
        let r = row(c);
        let found = r
            .props
            .iter()
            .find(|x| x.name == *name)
            .unwrap_or_else(|| panic!("no {c}.{name}"));
        assert_eq!(found.pid, *p, "{c}.{name}");
    }
    // Every class row's parent is a row, and every property's type a type.
    for c in CLASSES {
        assert!(CLASSES.iter().any(|x| x.auid == c.parent), "{}", c.name);
        for p in c.props {
            assert!(
                TYPES.iter().any(|t| t.auid == p.type_auid),
                "{}.{}",
                c.name,
                p.name
            );
        }
    }
    // The strong-reference types name classes in the table.
    for t in TYPES {
        if let TypeKind::StrongRef { class } | TypeKind::WeakRef { class, .. } = t.kind {
            assert!(class_known(class), "{}", t.name);
        }
    }
}

/// The MetaDictionary subtree of a store, in canonical form.
fn meta_subtree(
    s: &Store,
) -> Vec<(
    String,
    Auid,
    std::collections::BTreeMap<u16, common::aaf::Canon>,
)> {
    common::aaf::canonical(s)
        .into_iter()
        .filter(|(p, _, _)| p.starts_with("/0001"))
        .collect()
}

#[test]
fn meta_dictionary_is_fixed() {
    let mut a = song(1);
    a.region(0, 0, 48_000, 0.0, 0);
    let a_doc = a.b.finish();
    let b_doc = demo_audio_document().unwrap();
    let b_seq = b_doc
        .sequences()
        .values()
        .find(|s| s.name == "Song")
        .unwrap()
        .id;
    let (_, sa) = round(&a_doc, &opts(a.seq));
    let (_, sb) = round(&b_doc, &opts(b_seq));
    let (ma, mb) = (meta_subtree(&sa), meta_subtree(&sb));
    let props: usize = CLASSES.iter().map(|c| c.props.len()).sum();
    assert_eq!(ma.len(), 1 + CLASSES.len() + props + TYPES.len());
    assert_eq!(ma, mb);
    // The two weak-reference paths it uses come first in the table.
    assert_eq!(sa.referenced[..2], sb.referenced[..2]);
    assert_eq!(
        sa.referenced[..2],
        [vec![0x0001, 0x0003], vec![0x0001, 0x0004]]
    );
}

// ---------------------------------------------------------------- Ruling 4

#[test]
fn layout_follows_ruling_4() {
    // 120 BPM: a beat (Q ticks) is 24 000 frames.
    let mut g = song(1);
    g.region(0, 0, 48_000, 0.0, 4_800); // fade-in clip at 0
    g.region(2 * Q, 100, 48_000, 0.0, 0); // butted
    let c = g.region(7 * Q / 2, 200, 48_000, 0.0, 0); // crossfades over the one before
    g.b.run(Command::Audio(AudioCmd::Crossfade {
        event: c,
        length: CrossfadeLength::UpTo(4_800),
        shape: FadeShape::Linear,
    }))
    .unwrap();
    g.region(5 * Q, 300, 4_800, 0.0, 0); // over the crossfaded one, no crossfade
    let doc = g.b.finish();
    let (_, s) = round(&doc, &opts(g.seq));
    let slots = sound_slots(&s);
    assert_eq!(slots.len(), 1);
    let (_, seq) = peel(&s, slots[0].1);
    let comps = list(&s, seq, pid::SEQUENCE_COMPONENTS);
    let got: Vec<(&str, i64)> = comps
        .iter()
        .map(|c| {
            (
                class_name(class_of(&s, *c)),
                i64_of(&s, *c, pid::COMPONENT_LENGTH),
            )
        })
        .collect();
    let want = vec![
        ("Filler", 4_800),
        ("Transition", 4_800),
        ("SourceClip", 48_000),
        ("SourceClip", 40_800), // cut to end at the crossfade's start + 4 800
        ("Transition", 4_800),
        ("SourceClip", 36_000), // up to the later start
        ("SourceClip", 4_800),
        ("SourceClip", 7_200), // the rest after it, its StartTime moved on
    ];
    assert_eq!(got, want);
    let start = |i: usize| i64_of(&s, comps[i], pid::SOURCE_CLIP_START_TIME);
    assert_eq!(
        [start(2), start(3), start(5), start(6), start(7)],
        [0, 100, 200, 300, 200 + 40_800]
    );
    assert_eq!(i64_of(&s, comps[1], pid::TRANSITION_CUT_POINT), 0);
    assert_eq!(i64_of(&s, comps[4], pid::TRANSITION_CUT_POINT), 0);
    // The transitions dissolve with the shape as the Level's interpolation.
    for t in [comps[1], comps[4]] {
        let g = strong(&s, t, pid::TRANSITION_OPERATION_GROUP);
        assert_eq!(
            weak_id(&s, g, pid::OPERATION_GROUP_OPERATION),
            def::OPERATION_DEF_MONO_AUDIO_DISSOLVE
        );
        let p = list(&s, g, pid::OPERATION_GROUP_PARAMETERS)[0];
        assert_eq!(class_of(&s, p), class::VARYING_VALUE);
        assert_eq!(
            weak_id(&s, p, pid::VARYING_VALUE_INTERPOLATION),
            def::INTERPOLATION_DEF_LINEAR
        );
        assert_eq!(list(&s, p, pid::VARYING_VALUE_POINT_LIST).len(), 2);
    }
}

// ---------------------------------------------------------------- Ruling 6

#[test]
fn gain_and_automation() {
    let mut g = song(1);
    g.region(0, 0, 48_000, -6.0, 0);
    g.b.events(
        g.seq,
        g.lane,
        [0.5, 0.6, 0.7]
            .iter()
            .enumerate()
            .map(|(k, v)| {
                core_model::event::EventSpec::new(
                    core_model::time::Tick(k as i64 * Q),
                    EventBody::Automation {
                        target: AutomationTarget::StripFader(g.strip),
                        value: *v,
                    },
                )
            })
            .collect(),
    )
    .unwrap();
    let doc = g.b.finish();
    let (x, s) = round(&doc, &opts(g.seq));
    let (groups, seq) = peel(&s, sound_slots(&s)[0].1);
    // Track volume (a varying value of 3 points) and pan around the sequence.
    let ops: Vec<Auid> = groups
        .iter()
        .map(|g| weak_id(&s, *g, pid::OPERATION_GROUP_OPERATION))
        .collect();
    assert_eq!(
        ops,
        [
            def::OPERATION_DEF_MONO_AUDIO_GAIN,
            def::OPERATION_DEF_MONO_AUDIO_PAN
        ]
    );
    let vol = list(&s, groups[0], pid::OPERATION_GROUP_PARAMETERS)[0];
    assert_eq!(class_of(&s, vol), class::VARYING_VALUE);
    assert_eq!(list(&s, vol, pid::VARYING_VALUE_POINT_LIST).len(), 3);
    let pan = list(&s, groups[1], pid::OPERATION_GROUP_PARAMETERS)[0];
    assert_eq!(class_of(&s, pan), class::CONSTANT_VALUE);
    assert_eq!(ratio(&s, pan, pid::CONSTANT_VALUE_VALUE), (1, 2));
    // The region's −6 dB: a gain group around its clip.
    let clip_group = list(&s, seq, pid::SEQUENCE_COMPONENTS)[0];
    assert_eq!(class_of(&s, clip_group), class::OPERATION_GROUP);
    assert_eq!(
        weak_id(&s, clip_group, pid::OPERATION_GROUP_OPERATION),
        def::OPERATION_DEF_MONO_AUDIO_GAIN
    );
    let amp = list(&s, clip_group, pid::OPERATION_GROUP_PARAMETERS)[0];
    assert_eq!(class_of(&s, amp), class::CONSTANT_VALUE);
    assert_eq!(
        ratio(&s, amp, pid::CONSTANT_VALUE_VALUE),
        (501_187, 1_000_000)
    );
    assert!(!has(&x.report, ItemKind::LeftOut, CLIP_GAIN_OFF));

    // Both off: no groups at all, and both lines.
    let mut o = opts(g.seq);
    o.clip_gain = false;
    o.automation = false;
    let (x, s) = round(&doc, &o);
    let (groups, seq) = peel(&s, sound_slots(&s)[0].1);
    assert!(groups.is_empty());
    assert!(
        list(&s, seq, pid::SEQUENCE_COMPONENTS)
            .iter()
            .all(|c| class_of(&s, *c) != class::OPERATION_GROUP)
    );
    assert!(
        has(&x.report, ItemKind::LeftOut, CLIP_GAIN_OFF),
        "{:?}",
        x.report
    );
    assert!(
        has(&x.report, ItemKind::LeftOut, AUTOMATION_OFF),
        "{:?}",
        x.report
    );
}

// ---------------------------------------------------------------- Ruling 7

#[test]
fn stereo_as_pairs_or_one_slot() {
    let mut g = song(2);
    g.region(0, 0, 48_000, 0.0, 0);
    let doc = g.b.finish();
    let (x, s) = round(&doc, &opts(g.seq));
    let names: Vec<String> = sound_slots(&s).into_iter().map(|(n, _)| n).collect();
    assert_eq!(names, ["Keys L", "Keys R"]);
    assert_eq!(x.audio.len(), 2);
    assert_eq!(
        x.audio.iter().map(|a| a.channel).collect::<Vec<_>>(),
        [Some(0), Some(1)]
    );
    assert!(x.audio.iter().all(|a| a.media == g.media));
    assert_eq!(x.essence.len(), 2);

    let mut o = opts(g.seq);
    o.stereo = Stereo::Stereo;
    let (x, s) = round(&doc, &o);
    let names: Vec<String> = sound_slots(&s).into_iter().map(|(n, _)| n).collect();
    assert_eq!(names, ["Keys"]);
    assert_eq!(x.audio.len(), 1);
    assert_eq!(x.audio[0].channel, None);
    assert_eq!(x.audio[0].file, "Audio/001 keys.wav");
}

// ---------------------------------------------------------------- Ruling 3

/// The composition's timecode: (Start, FPS, Drop).
fn timecode(s: &Store) -> (i64, u16, u8) {
    let slot = list(s, composition(s), pid::MOB_SLOTS)
        .into_iter()
        .find(|sl| {
            let seg = strong(s, *sl, pid::MOB_SLOT_SEGMENT);
            weak_id(s, seg, pid::COMPONENT_DATA_DEFINITION) == def::DATA_DEF_TIMECODE
        })
        .expect("a timecode slot");
    let mut tc = strong(s, slot, pid::MOB_SLOT_SEGMENT);
    if class_of(s, tc) == class::SEQUENCE {
        tc = list(s, tc, pid::SEQUENCE_COMPONENTS)[0];
    }
    assert_eq!(class_of(s, tc), class::TIMECODE);
    (
        i64_of(s, tc, pid::TIMECODE_START),
        u16::from_le_bytes(data(s, tc, pid::TIMECODE_FPS).try_into().unwrap()),
        data(s, tc, pid::TIMECODE_DROP)[0],
    )
}

#[test]
fn timecode_and_fps() {
    let mut g = song(1);
    g.region(0, 0, 48_000, 0.0, 0);
    let doc = g.b.finish();
    let (_, s) = round(&doc, &opts(g.seq));
    assert_eq!(timecode(&s), (0, 25, 0));
    let mut o = opts(g.seq);
    o.hour_start = true;
    let (_, s) = round(&doc, &o);
    assert_eq!(timecode(&s), (90_000, 25, 0));
    o.fps = Fps::F2997Drop;
    let (_, s) = round(&doc, &o);
    // 01:00:00;00 drop-frame: 108 000 frames less 2 × 54 dropped minutes.
    assert_eq!(timecode(&s), (107_892, 30, 1));
    o.fps = Fps::F24;
    let (_, s) = round(&doc, &o);
    assert_eq!(timecode(&s), (86_400, 24, 0));
    o.fps = Fps::F30;
    let (_, s) = round(&doc, &o);
    assert_eq!(timecode(&s), (108_000, 30, 0));
}

// ---------------------------------------------------------------- Ruling 8

#[test]
fn midi_beside_only_with_midi() {
    let mut g = song(1);
    g.region(0, 0, 48_000, 0.0, 0);
    let audio_only = g.b.doc().clone();
    assert_eq!(exported(&audio_only, &opts(g.seq)).midi, None);
    let (dest, _) = g.b.plugin_dest("Synth", clap("org.example.synth")).unwrap();
    let lane = g.b.lane(g.seq, "Lead", Some(dest)).unwrap();
    g.b.notes(g.seq, lane, &[(0, 60, 0x8000, Q)]).unwrap();
    let doc = g.b.finish();
    let x = exported(&doc, &opts(g.seq));
    let (f, _) = to_smf_until(
        &doc,
        &SmfOptions {
            seq: g.seq,
            lanes: midi_tracks(&doc, g.seq).unwrap(),
            type0: false,
            ppq: 960,
            mpe: true,
        },
        &no(),
    )
    .unwrap();
    assert_eq!(x.midi, Some(core_io::smf::write(&f).unwrap()));
    assert!(
        has(&x.report, ItemKind::Note, MIDI_BESIDE),
        "{:?}",
        x.report
    );
}

// ---------------------------------------------------------------- listed

#[test]
fn listed() {
    // Every constant's exact text.
    assert_eq!(MUTED_REGIONS, "Muted regions");
    assert_eq!(MUTE_SOLO, "Mute and solo");
    assert_eq!(ROUTING, "Sends, buses and output routing");
    assert_eq!(INSERTS, "Inserts and the EQ slot");
    assert_eq!(COLOURS, "Strip colours");
    assert_eq!(OTHER_AUTOMATION, "Automation other than volume and pan");
    assert_eq!(
        AUTOMATION_MODES,
        "Automation modes, MIDI learn and control surfaces"
    );
    assert_eq!(TAKES, "Inactive takes and other sequences");
    assert_eq!(KEYBOARD_SLOTS, "Keyboard slots");
    assert_eq!(INPUTS, "Input effects and track inputs");
    assert_eq!(EXTRA_OUTPUTS, "Instrument extra outputs");
    assert_eq!(
        MIDI_BESIDE,
        "MIDI tracks are in the MIDI file beside the AAF"
    );
    assert_eq!(
        BUILTIN_SYNTH,
        "Tracks playing the built-in synth (their notes are in the MIDI file)"
    );
    assert_eq!(
        EXTERNAL_MIDI,
        "Tracks playing external MIDI (their notes are in the MIDI file)"
    );
    assert_eq!(CLIP_GAIN_OFF, "Clip gain (turned off)");
    assert_eq!(AUTOMATION_OFF, "Track volume and pan (turned off)");
    assert_eq!(FADE_OUTS_CUT, "Fade-outs under a later region were cut");
    assert_eq!(FADE_INS_CUT, "Fade-ins under a later region were cut");
    assert_eq!(
        EXPONENTIAL,
        "Exponential and S-curve fades are drawn differently in other DAWs"
    );
    assert_eq!(SCURVE, "S-curve fades were written as linear");
    assert_eq!(
        PICKUP,
        "The song starts before bar 1; other DAWs show it from the pickup's start."
    );

    // The demo, with a bus send, an insert and a muted region added.
    let mut b = Builder::from_doc(demo_audio_document().unwrap());
    let seq = b
        .doc()
        .sequences()
        .values()
        .find(|s| s.name == "Song")
        .unwrap()
        .id;
    let s = b.doc().sequence(seq).unwrap().clone();
    let loop_lane = s.lanes.iter().find(|l| l.name == "Loop").unwrap().clone();
    let strip = b
        .strip_of(StripKind::AudioPath(loop_lane.audio_out.unwrap()))
        .unwrap();
    let bus = b.bus("Verb").unwrap();
    b.mixer(MixerCmd::AddSend {
        strip,
        send: AuxSend {
            target: bus,
            level_db: -6.0,
            pre_fader: false,
            enabled: true,
        },
    })
    .unwrap();
    b.insert(strip, clap("org.example.fx")).unwrap();
    let loop_media = *b.doc().media().keys().next().unwrap();
    let muted = b
        .region(
            seq,
            loop_lane.id,
            8 * 4 * Q,
            loop_media,
            0,
            48_000,
            0.0,
            Fade::default(),
            Fade::default(),
        )
        .unwrap();
    b.run(Command::Event(EventCmd::SetMute {
        events: vec![muted],
        mute: true,
    }))
    .unwrap();
    let doc = b.finish();
    let r = check(&doc, &opts(seq)).expect("checks");
    let ours: BTreeSet<&str> = [
        MUTED_REGIONS,
        MUTE_SOLO,
        ROUTING,
        INSERTS,
        COLOURS,
        OTHER_AUTOMATION,
        AUTOMATION_MODES,
        TAKES,
        KEYBOARD_SLOTS,
        INPUTS,
        EXTRA_OUTPUTS,
        MIDI_BESIDE,
        BUILTIN_SYNTH,
        EXTERNAL_MIDI,
        CLIP_GAIN_OFF,
        AUTOMATION_OFF,
        PICKUP,
    ]
    .into_iter()
    .collect();
    let got: BTreeSet<(String, &str)> = r
        .items
        .iter()
        .filter(|i| ours.contains(i.what.as_str()))
        .map(|i| (format!("{:?}", i.kind), i.what.as_str()))
        .collect();
    let want: BTreeSet<(String, &str)> = [
        (ItemKind::LeftOut, MUTED_REGIONS),
        (ItemKind::LeftOut, ROUTING),
        (ItemKind::LeftOut, INSERTS),
        (ItemKind::Note, MIDI_BESIDE),
        (ItemKind::Note, BUILTIN_SYNTH),
    ]
    .into_iter()
    .map(|(k, w)| (format!("{k:?}"), w))
    .collect();
    assert_eq!(got, want, "{r:#?}");
    let detail = |what: &str| {
        r.items
            .iter()
            .find(|i| i.what == what)
            .unwrap()
            .details
            .clone()
    };
    assert_eq!(detail(MUTED_REGIONS), ["Loop"]);
    assert_eq!(detail(MIDI_BESIDE), ["Groove", "Chords"]);
    // The rest are the MIDI file's own lines (6a), its Notes aside.
    let (_, midi) = to_smf_until(
        &doc,
        &SmfOptions {
            seq,
            lanes: midi_tracks(&doc, seq).unwrap(),
            type0: false,
            ppq: 960,
            mpe: true,
        },
        &no(),
    )
    .unwrap();
    for i in r.items.iter().filter(|i| !ours.contains(i.what.as_str())) {
        assert_ne!(i.kind, ItemKind::Note, "{i:?}");
        assert!(
            has(&midi, i.kind, &i.what),
            "{i:?} is neither ours nor the MIDI file's"
        );
    }
    // The export's report is the dry run's.
    let x = exported(&doc, &opts(seq));
    assert_eq!(x.report.items, r.items);
}

// ---------------------------------------------------------------- names

#[test]
fn names_are_safe() {
    let mut g = song(1);
    g.region(0, 0, 48_000, 0.0, 0);
    let set_title = |b: &mut Builder, t: &str| {
        b.run(Command::Settings(SettingsCmd::SetInfo {
            info: SongInfo {
                title: Some(t.into()),
                ..SongInfo::default()
            },
        }))
        .unwrap();
    };
    // No title: the sequence's name.
    assert_eq!(folder_name(g.b.doc(), g.seq).unwrap(), "Song AAF");
    // Song info refuses NUL (and trims), so an inner tab stands for the
    // control characters `safe_part` maps; `/` and a leading `.` are
    // mapped too.
    set_title(&mut g.b, "../a/b\tc");
    assert_eq!(folder_name(g.b.doc(), g.seq).unwrap(), "-.-a-b-c AAF");
    assert_eq!(core_io::names::safe_part("../a/b\0", 0), "-.-a-b-");
    set_title(&mut g.b, &"x".repeat(300));
    let n = folder_name(g.b.doc(), g.seq).unwrap();
    assert!(n.ends_with(" AAF"));
    assert_eq!(format!(".{n} 9999.partial").len(), 255);
    // The staged files' names are safe parts too.
    let x = exported(g.b.doc(), &opts(g.seq));
    assert!(
        x.audio
            .iter()
            .all(|a| a.file.starts_with("Audio/001 ") && a.file.ends_with(".wav"))
    );
}

// Every class and PID `dict.rs` holds, by its name in the tables.
const CLASS_NAMES: &[(Auid, &str)] = &[
    (class::ROOT, "Root"),
    (class::INTERCHANGE_OBJECT, "InterchangeObject"),
    (class::COMPONENT, "Component"),
    (class::SEGMENT, "Segment"),
    (class::EVENT, "Event"),
    (class::COMMENT_MARKER, "CommentMarker"),
    (class::FILLER, "Filler"),
    (class::OPERATION_GROUP, "OperationGroup"),
    (class::SEQUENCE, "Sequence"),
    (class::SOURCE_REFERENCE, "SourceReference"),
    (class::SOURCE_CLIP, "SourceClip"),
    (class::TIMECODE, "Timecode"),
    (class::TRANSITION, "Transition"),
    (class::CONTENT_STORAGE, "ContentStorage"),
    (class::CONTROL_POINT, "ControlPoint"),
    (class::DEFINITION_OBJECT, "DefinitionObject"),
    (class::DATA_DEFINITION, "DataDefinition"),
    (class::OPERATION_DEFINITION, "OperationDefinition"),
    (class::PARAMETER_DEFINITION, "ParameterDefinition"),
    (class::CODEC_DEFINITION, "CodecDefinition"),
    (class::CONTAINER_DEFINITION, "ContainerDefinition"),
    (class::INTERPOLATION_DEFINITION, "InterpolationDefinition"),
    (class::DICTIONARY, "Dictionary"),
    (class::ESSENCE_DATA, "EssenceData"),
    (class::ESSENCE_DESCRIPTOR, "EssenceDescriptor"),
    (class::FILE_DESCRIPTOR, "FileDescriptor"),
    (class::AIFC_DESCRIPTOR, "AIFCDescriptor"),
    (class::WAVE_DESCRIPTOR, "WAVEDescriptor"),
    (class::HEADER, "Header"),
    (class::IDENTIFICATION, "Identification"),
    (class::LOCATOR, "Locator"),
    (class::NETWORK_LOCATOR, "NetworkLocator"),
    (class::MOB, "Mob"),
    (class::COMPOSITION_MOB, "CompositionMob"),
    (class::MASTER_MOB, "MasterMob"),
    (class::SOURCE_MOB, "SourceMob"),
    (class::MOB_SLOT, "MobSlot"),
    (class::EVENT_MOB_SLOT, "EventMobSlot"),
    (class::TIMELINE_MOB_SLOT, "TimelineMobSlot"),
    (class::PARAMETER, "Parameter"),
    (class::CONSTANT_VALUE, "ConstantValue"),
    (class::VARYING_VALUE, "VaryingValue"),
    (class::TAGGED_VALUE, "TaggedValue"),
    (class::DESCRIPTIVE_MARKER, "DescriptiveMarker"),
    (class::SOUND_DESCRIPTOR, "SoundDescriptor"),
    (class::PCM_DESCRIPTOR, "PCMDescriptor"),
    (class::CLASS_DEFINITION, "ClassDefinition"),
    (class::PROPERTY_DEFINITION, "PropertyDefinition"),
    (class::TYPE_DEFINITION, "TypeDefinition"),
    (class::META_DEFINITION, "MetaDefinition"),
    (class::META_DICTIONARY, "MetaDictionary"),
    (class::ESSENCE_GROUP, "EssenceGroup"),
    (class::NESTED_SCOPE, "NestedScope"),
    (class::SCOPE_REFERENCE, "ScopeReference"),
    (class::SELECTOR, "Selector"),
];
const PID_NAMES: &[(u16, &str, &str)] = &[
    (
        pid::SOURCE_REFERENCE_CHANNEL_IDS,
        "SourceReference",
        "ChannelIDs",
    ),
    (
        pid::SOURCE_REFERENCE_MONO_SOURCE_SLOT_IDS,
        "SourceReference",
        "MonoSourceSlotIDs",
    ),
    (pid::ROOT_META_DICTIONARY, "Root", "MetaDictionary"),
    (pid::ROOT_HEADER, "Root", "Header"),
    (
        pid::INTERCHANGE_OBJECT_OBJ_CLASS,
        "InterchangeObject",
        "ObjClass",
    ),
    (
        pid::INTERCHANGE_OBJECT_GENERATION,
        "InterchangeObject",
        "Generation",
    ),
    (
        pid::COMPONENT_DATA_DEFINITION,
        "Component",
        "DataDefinition",
    ),
    (pid::COMPONENT_LENGTH, "Component", "Length"),
    (pid::COMPONENT_KLV_DATA, "Component", "KLVData"),
    (pid::COMPONENT_USER_COMMENTS, "Component", "UserComments"),
    (pid::COMPONENT_ATTRIBUTES, "Component", "Attributes"),
    (pid::EVENT_POSITION, "Event", "Position"),
    (pid::EVENT_COMMENT, "Event", "Comment"),
    (
        pid::COMMENT_MARKER_ANNOTATION,
        "CommentMarker",
        "Annotation",
    ),
    (
        pid::OPERATION_GROUP_OPERATION,
        "OperationGroup",
        "Operation",
    ),
    (
        pid::OPERATION_GROUP_INPUT_SEGMENTS,
        "OperationGroup",
        "InputSegments",
    ),
    (
        pid::OPERATION_GROUP_PARAMETERS,
        "OperationGroup",
        "Parameters",
    ),
    (
        pid::OPERATION_GROUP_BYPASS_OVERRIDE,
        "OperationGroup",
        "BypassOverride",
    ),
    (
        pid::OPERATION_GROUP_RENDERING,
        "OperationGroup",
        "Rendering",
    ),
    (pid::SEQUENCE_COMPONENTS, "Sequence", "Components"),
    (
        pid::SOURCE_REFERENCE_SOURCE_ID,
        "SourceReference",
        "SourceID",
    ),
    (
        pid::SOURCE_REFERENCE_SOURCE_MOB_SLOT_ID,
        "SourceReference",
        "SourceMobSlotID",
    ),
    (pid::SOURCE_CLIP_START_TIME, "SourceClip", "StartTime"),
    (
        pid::SOURCE_CLIP_FADE_IN_LENGTH,
        "SourceClip",
        "FadeInLength",
    ),
    (pid::SOURCE_CLIP_FADE_IN_TYPE, "SourceClip", "FadeInType"),
    (
        pid::SOURCE_CLIP_FADE_OUT_LENGTH,
        "SourceClip",
        "FadeOutLength",
    ),
    (pid::SOURCE_CLIP_FADE_OUT_TYPE, "SourceClip", "FadeOutType"),
    (pid::TIMECODE_START, "Timecode", "Start"),
    (pid::TIMECODE_FPS, "Timecode", "FPS"),
    (pid::TIMECODE_DROP, "Timecode", "Drop"),
    (
        pid::TRANSITION_OPERATION_GROUP,
        "Transition",
        "OperationGroup",
    ),
    (pid::TRANSITION_CUT_POINT, "Transition", "CutPoint"),
    (pid::CONTENT_STORAGE_MOBS, "ContentStorage", "Mobs"),
    (
        pid::CONTENT_STORAGE_ESSENCE_DATA,
        "ContentStorage",
        "EssenceData",
    ),
    (pid::CONTROL_POINT_VALUE, "ControlPoint", "Value"),
    (pid::CONTROL_POINT_TIME, "ControlPoint", "Time"),
    (pid::CONTROL_POINT_EDIT_HINT, "ControlPoint", "EditHint"),
    (
        pid::DEFINITION_OBJECT_IDENTIFICATION,
        "DefinitionObject",
        "Identification",
    ),
    (pid::DEFINITION_OBJECT_NAME, "DefinitionObject", "Name"),
    (
        pid::DEFINITION_OBJECT_DESCRIPTION,
        "DefinitionObject",
        "Description",
    ),
    (
        pid::OPERATION_DEFINITION_DATA_DEFINITION,
        "OperationDefinition",
        "DataDefinition",
    ),
    (
        pid::OPERATION_DEFINITION_IS_TIME_WARP,
        "OperationDefinition",
        "IsTimeWarp",
    ),
    (
        pid::OPERATION_DEFINITION_DEGRADE_TO,
        "OperationDefinition",
        "DegradeTo",
    ),
    (
        pid::OPERATION_DEFINITION_OPERATION_CATEGORY,
        "OperationDefinition",
        "OperationCategory",
    ),
    (
        pid::OPERATION_DEFINITION_NUMBER_INPUTS,
        "OperationDefinition",
        "NumberInputs",
    ),
    (
        pid::OPERATION_DEFINITION_BYPASS,
        "OperationDefinition",
        "Bypass",
    ),
    (
        pid::OPERATION_DEFINITION_PARAMETERS_DEFINED,
        "OperationDefinition",
        "ParametersDefined",
    ),
    (
        pid::PARAMETER_DEFINITION_TYPE,
        "ParameterDefinition",
        "Type",
    ),
    (
        pid::PARAMETER_DEFINITION_DISPLAY_UNITS,
        "ParameterDefinition",
        "DisplayUnits",
    ),
    (
        pid::CODEC_DEFINITION_FILE_DESCRIPTOR_CLASS,
        "CodecDefinition",
        "FileDescriptorClass",
    ),
    (
        pid::CODEC_DEFINITION_DATA_DEFINITIONS,
        "CodecDefinition",
        "DataDefinitions",
    ),
    (
        pid::CONTAINER_DEFINITION_ESSENCE_IS_IDENTIFIED,
        "ContainerDefinition",
        "EssenceIsIdentified",
    ),
    (
        pid::DICTIONARY_OPERATION_DEFINITIONS,
        "Dictionary",
        "OperationDefinitions",
    ),
    (
        pid::DICTIONARY_PARAMETER_DEFINITIONS,
        "Dictionary",
        "ParameterDefinitions",
    ),
    (
        pid::DICTIONARY_DATA_DEFINITIONS,
        "Dictionary",
        "DataDefinitions",
    ),
    (
        pid::DICTIONARY_PLUGIN_DEFINITIONS,
        "Dictionary",
        "PluginDefinitions",
    ),
    (
        pid::DICTIONARY_CODEC_DEFINITIONS,
        "Dictionary",
        "CodecDefinitions",
    ),
    (
        pid::DICTIONARY_CONTAINER_DEFINITIONS,
        "Dictionary",
        "ContainerDefinitions",
    ),
    (
        pid::DICTIONARY_INTERPOLATION_DEFINITIONS,
        "Dictionary",
        "InterpolationDefinitions",
    ),
    (
        pid::DICTIONARY_KLV_DATA_DEFINITIONS,
        "Dictionary",
        "KLVDataDefinitions",
    ),
    (
        pid::DICTIONARY_TAGGED_VALUE_DEFINITIONS,
        "Dictionary",
        "TaggedValueDefinitions",
    ),
    (pid::ESSENCE_DATA_MOB_ID, "EssenceData", "MobID"),
    (pid::ESSENCE_DATA_DATA, "EssenceData", "Data"),
    (pid::ESSENCE_DATA_SAMPLE_INDEX, "EssenceData", "SampleIndex"),
    (
        pid::ESSENCE_DESCRIPTOR_LOCATOR,
        "EssenceDescriptor",
        "Locator",
    ),
    (
        pid::FILE_DESCRIPTOR_SAMPLE_RATE,
        "FileDescriptor",
        "SampleRate",
    ),
    (pid::FILE_DESCRIPTOR_LENGTH, "FileDescriptor", "Length"),
    (
        pid::FILE_DESCRIPTOR_CONTAINER_FORMAT,
        "FileDescriptor",
        "ContainerFormat",
    ),
    (
        pid::FILE_DESCRIPTOR_CODEC_DEFINITION,
        "FileDescriptor",
        "CodecDefinition",
    ),
    (
        pid::FILE_DESCRIPTOR_LINKED_SLOT_ID,
        "FileDescriptor",
        "LinkedSlotID",
    ),
    (pid::AIFC_DESCRIPTOR_SUMMARY, "AIFCDescriptor", "Summary"),
    (pid::WAVE_DESCRIPTOR_SUMMARY, "WAVEDescriptor", "Summary"),
    (pid::HEADER_BYTE_ORDER, "Header", "ByteOrder"),
    (pid::HEADER_LAST_MODIFIED, "Header", "LastModified"),
    (pid::HEADER_CONTENT, "Header", "Content"),
    (pid::HEADER_DICTIONARY, "Header", "Dictionary"),
    (pid::HEADER_VERSION, "Header", "Version"),
    (
        pid::HEADER_IDENTIFICATION_LIST,
        "Header",
        "IdentificationList",
    ),
    (
        pid::HEADER_OBJECT_MODEL_VERSION,
        "Header",
        "ObjectModelVersion",
    ),
    (pid::HEADER_PRIMARY_MOB, "Header", "PrimaryMob"),
    (
        pid::HEADER_OPERATIONAL_PATTERN,
        "Header",
        "OperationalPattern",
    ),
    (
        pid::HEADER_ESSENCE_CONTAINERS,
        "Header",
        "EssenceContainers",
    ),
    (
        pid::HEADER_DESCRIPTIVE_SCHEMES,
        "Header",
        "DescriptiveSchemes",
    ),
    (
        pid::IDENTIFICATION_COMPANY_NAME,
        "Identification",
        "CompanyName",
    ),
    (
        pid::IDENTIFICATION_PRODUCT_NAME,
        "Identification",
        "ProductName",
    ),
    (
        pid::IDENTIFICATION_PRODUCT_VERSION,
        "Identification",
        "ProductVersion",
    ),
    (
        pid::IDENTIFICATION_PRODUCT_VERSION_STRING,
        "Identification",
        "ProductVersionString",
    ),
    (
        pid::IDENTIFICATION_PRODUCT_ID,
        "Identification",
        "ProductID",
    ),
    (pid::IDENTIFICATION_DATE, "Identification", "Date"),
    (
        pid::IDENTIFICATION_TOOLKIT_VERSION,
        "Identification",
        "ToolkitVersion",
    ),
    (pid::IDENTIFICATION_PLATFORM, "Identification", "Platform"),
    (
        pid::IDENTIFICATION_GENERATION_AUID,
        "Identification",
        "GenerationAUID",
    ),
    (
        pid::NETWORK_LOCATOR_URL_STRING,
        "NetworkLocator",
        "URLString",
    ),
    (pid::MOB_MOB_ID, "Mob", "MobID"),
    (pid::MOB_NAME, "Mob", "Name"),
    (pid::MOB_SLOTS, "Mob", "Slots"),
    (pid::MOB_LAST_MODIFIED, "Mob", "LastModified"),
    (pid::MOB_CREATION_TIME, "Mob", "CreationTime"),
    (pid::MOB_USER_COMMENTS, "Mob", "UserComments"),
    (pid::MOB_KLV_DATA, "Mob", "KLVData"),
    (pid::MOB_ATTRIBUTES, "Mob", "Attributes"),
    (pid::MOB_USAGE_CODE, "Mob", "UsageCode"),
    (
        pid::COMPOSITION_MOB_DEFAULT_FADE_LENGTH,
        "CompositionMob",
        "DefaultFadeLength",
    ),
    (
        pid::COMPOSITION_MOB_DEF_FADE_TYPE,
        "CompositionMob",
        "DefFadeType",
    ),
    (
        pid::COMPOSITION_MOB_DEF_FADE_EDIT_UNIT,
        "CompositionMob",
        "DefFadeEditUnit",
    ),
    (
        pid::COMPOSITION_MOB_RENDERING,
        "CompositionMob",
        "Rendering",
    ),
    (
        pid::SOURCE_MOB_ESSENCE_DESCRIPTION,
        "SourceMob",
        "EssenceDescription",
    ),
    (pid::MOB_SLOT_SLOT_ID, "MobSlot", "SlotID"),
    (pid::MOB_SLOT_SLOT_NAME, "MobSlot", "SlotName"),
    (pid::MOB_SLOT_SEGMENT, "MobSlot", "Segment"),
    (
        pid::MOB_SLOT_PHYSICAL_TRACK_NUMBER,
        "MobSlot",
        "PhysicalTrackNumber",
    ),
    (pid::EVENT_MOB_SLOT_EDIT_RATE, "EventMobSlot", "EditRate"),
    (
        pid::EVENT_MOB_SLOT_EVENT_SLOT_ORIGIN,
        "EventMobSlot",
        "EventSlotOrigin",
    ),
    (
        pid::TIMELINE_MOB_SLOT_EDIT_RATE,
        "TimelineMobSlot",
        "EditRate",
    ),
    (pid::TIMELINE_MOB_SLOT_ORIGIN, "TimelineMobSlot", "Origin"),
    (pid::TIMELINE_MOB_SLOT_MARK_IN, "TimelineMobSlot", "MarkIn"),
    (
        pid::TIMELINE_MOB_SLOT_MARK_OUT,
        "TimelineMobSlot",
        "MarkOut",
    ),
    (
        pid::TIMELINE_MOB_SLOT_USER_POS,
        "TimelineMobSlot",
        "UserPos",
    ),
    (pid::PARAMETER_DEFINITION, "Parameter", "Definition"),
    (pid::CONSTANT_VALUE_VALUE, "ConstantValue", "Value"),
    (
        pid::VARYING_VALUE_INTERPOLATION,
        "VaryingValue",
        "Interpolation",
    ),
    (pid::VARYING_VALUE_POINT_LIST, "VaryingValue", "PointList"),
    (pid::TAGGED_VALUE_NAME, "TaggedValue", "Name"),
    (pid::TAGGED_VALUE_VALUE, "TaggedValue", "Value"),
    (
        pid::DESCRIPTIVE_MARKER_DESCRIBED_SLOTS,
        "DescriptiveMarker",
        "DescribedSlots",
    ),
    (
        pid::DESCRIPTIVE_MARKER_DESCRIPTION,
        "DescriptiveMarker",
        "Description",
    ),
    (
        pid::SOUND_DESCRIPTOR_AUDIO_SAMPLING_RATE,
        "SoundDescriptor",
        "AudioSamplingRate",
    ),
    (pid::SOUND_DESCRIPTOR_LOCKED, "SoundDescriptor", "Locked"),
    (
        pid::SOUND_DESCRIPTOR_AUDIO_REF_LEVEL,
        "SoundDescriptor",
        "AudioRefLevel",
    ),
    (
        pid::SOUND_DESCRIPTOR_ELECTRO_SPATIAL,
        "SoundDescriptor",
        "ElectroSpatial",
    ),
    (
        pid::SOUND_DESCRIPTOR_CHANNELS,
        "SoundDescriptor",
        "Channels",
    ),
    (
        pid::SOUND_DESCRIPTOR_QUANTIZATION_BITS,
        "SoundDescriptor",
        "QuantizationBits",
    ),
    (
        pid::SOUND_DESCRIPTOR_DIAL_NORM,
        "SoundDescriptor",
        "DialNorm",
    ),
    (
        pid::SOUND_DESCRIPTOR_COMPRESSION,
        "SoundDescriptor",
        "Compression",
    ),
    (
        pid::PCM_DESCRIPTOR_BLOCK_ALIGN,
        "PCMDescriptor",
        "BlockAlign",
    ),
    (
        pid::PCM_DESCRIPTOR_SEQUENCE_OFFSET,
        "PCMDescriptor",
        "SequenceOffset",
    ),
    (
        pid::PCM_DESCRIPTOR_AVERAGE_BPS,
        "PCMDescriptor",
        "AverageBPS",
    ),
    (
        pid::PCM_DESCRIPTOR_CHANNEL_ASSIGNMENT,
        "PCMDescriptor",
        "ChannelAssignment",
    ),
    (
        pid::PCM_DESCRIPTOR_PEAK_ENVELOPE_VERSION,
        "PCMDescriptor",
        "PeakEnvelopeVersion",
    ),
    (
        pid::PCM_DESCRIPTOR_PEAK_ENVELOPE_FORMAT,
        "PCMDescriptor",
        "PeakEnvelopeFormat",
    ),
    (
        pid::PCM_DESCRIPTOR_POINTS_PER_PEAK_VALUE,
        "PCMDescriptor",
        "PointsPerPeakValue",
    ),
    (
        pid::PCM_DESCRIPTOR_PEAK_ENVELOPE_BLOCK_SIZE,
        "PCMDescriptor",
        "PeakEnvelopeBlockSize",
    ),
    (
        pid::PCM_DESCRIPTOR_PEAK_CHANNELS,
        "PCMDescriptor",
        "PeakChannels",
    ),
    (
        pid::PCM_DESCRIPTOR_PEAK_FRAMES,
        "PCMDescriptor",
        "PeakFrames",
    ),
    (
        pid::PCM_DESCRIPTOR_PEAK_OF_PEAKS_POSITION,
        "PCMDescriptor",
        "PeakOfPeaksPosition",
    ),
    (
        pid::PCM_DESCRIPTOR_PEAK_ENVELOPE_TIMESTAMP,
        "PCMDescriptor",
        "PeakEnvelopeTimestamp",
    ),
    (
        pid::PCM_DESCRIPTOR_PEAK_ENVELOPE_DATA,
        "PCMDescriptor",
        "PeakEnvelopeData",
    ),
    (
        pid::CLASS_DEFINITION_PARENT_CLASS,
        "ClassDefinition",
        "ParentClass",
    ),
    (
        pid::CLASS_DEFINITION_PROPERTIES,
        "ClassDefinition",
        "Properties",
    ),
    (
        pid::CLASS_DEFINITION_IS_CONCRETE,
        "ClassDefinition",
        "IsConcrete",
    ),
    (pid::PROPERTY_DEFINITION_TYPE, "PropertyDefinition", "Type"),
    (
        pid::PROPERTY_DEFINITION_IS_OPTIONAL,
        "PropertyDefinition",
        "IsOptional",
    ),
    (
        pid::PROPERTY_DEFINITION_LOCAL_IDENTIFICATION,
        "PropertyDefinition",
        "LocalIdentification",
    ),
    (
        pid::PROPERTY_DEFINITION_IS_UNIQUE_IDENTIFIER,
        "PropertyDefinition",
        "IsUniqueIdentifier",
    ),
    (
        pid::META_DEFINITION_IDENTIFICATION,
        "MetaDefinition",
        "Identification",
    ),
    (pid::META_DEFINITION_NAME, "MetaDefinition", "Name"),
    (
        pid::META_DEFINITION_DESCRIPTION,
        "MetaDefinition",
        "Description",
    ),
    (
        pid::META_DICTIONARY_CLASS_DEFINITIONS,
        "MetaDictionary",
        "ClassDefinitions",
    ),
    (
        pid::META_DICTIONARY_TYPE_DEFINITIONS,
        "MetaDictionary",
        "TypeDefinitions",
    ),
    (pid::ESSENCE_GROUP_CHOICES, "EssenceGroup", "Choices"),
    (pid::NESTED_SCOPE_SLOTS, "NestedScope", "Slots"),
    (pid::SELECTOR_SELECTED, "Selector", "Selected"),
    (pid::SELECTOR_ALTERNATES, "Selector", "Alternates"),
];

#[test]
fn write_never_replaces_and_cleans_up() {
    let mut g = song(1);
    g.region(0, 0, 48_000, 0.0, 0);
    let doc = g.b.finish();
    let x = exported(&doc, &opts(g.seq));
    let dir = tempfile::tempdir().unwrap();
    let dest = dir.path().join("Song.aaf");
    // An existing file is refused and kept.
    std::fs::write(&dest, b"mine").unwrap();
    let mut essence = |_: usize, w: &mut dyn std::io::Write| -> Result<(), core_io::IoError> {
        w.write_all(b"x")
            .map_err(|e| core_io::IoError::Io(e.to_string()))
    };
    assert!(core_io::aaf::export::write(&x, &dest, &mut essence, &no()).is_err());
    assert_eq!(std::fs::read(&dest).unwrap(), b"mine");
    std::fs::remove_file(&dest).unwrap();
    // A failing essence, or a cancel, leaves no file.
    let mut failing = |_: usize, _: &mut dyn std::io::Write| -> Result<(), core_io::IoError> {
        Err(core_io::IoError::Io(
            "the staged file could not be read".into(),
        ))
    };
    assert!(core_io::aaf::export::write(&x, &dest, &mut failing, &no()).is_err());
    assert!(std::fs::symlink_metadata(&dest).is_err());
    let cancelled = AtomicBool::new(true);
    assert!(matches!(
        core_io::aaf::export::write(&x, &dest, &mut essence, &cancelled),
        Err(core_io::IoError::Cancelled)
    ));
    assert!(std::fs::symlink_metadata(&dest).is_err());
}

#[test]
fn external_audio_is_located_in_the_export_folder() {
    let mut g = song(1);
    g.region(0, 0, 48_000, 0.0, 0);
    let doc = g.b.finish();
    let mut o = opts(g.seq);
    o.embed = false;
    let x = exported(&doc, &o);
    assert!(x.essence.is_empty());
    // Written into a staging folder, the absolute locator names the folder
    // it is committed as; the relative one follows.
    let dir = tempfile::tempdir().unwrap();
    let stage = dir.path().join(".Song AAF.partial");
    std::fs::create_dir(&stage).unwrap();
    let p = write_export(&doc, &x, &stage, 24);
    let r = AafReader::open(&p, &AafLimits::default(), &no()).unwrap();
    let s = r.store();
    let urls: Vec<String> = s
        .objects
        .iter()
        .enumerate()
        .filter(|(_, o)| o.class == class::NETWORK_LOCATOR)
        .map(|(i, _)| text(s, ObjId(i as u32), pid::NETWORK_LOCATOR_URL_STRING))
        .collect();
    let folder = std::path::absolute(dir.path().join("Song AAF")).unwrap();
    let abs = format!("file://{}/Audio/001%20keys.wav", folder.display()).replace(' ', "%20");
    assert_eq!(urls, [abs, "Audio/001%20keys.wav".to_string()]);
}
