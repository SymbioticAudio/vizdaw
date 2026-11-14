//! Ruling 13's limits table (plan 6f Task 7): one test per row, a file at
//! the limit and one past it. At the limit the file reads; past it the
//! file is refused, or the item is left out and listed, as the ruling
//! says. The real limits are used where a test can build them quickly;
//! the object count, the control points and the model budget are lowered
//! (a million objects, or a hundred thousand control points, take many
//! seconds to build and read in an unoptimised test; the fuzz's release
//! run reads 200 000).

mod common;

use common::aaf::{FileSpec, Fx, Seg, Value, clip, indirect, sound, utf16z};
use core_io::IoError;
use core_io::aaf::dict::{class, def, pid};
use core_io::aaf::store::{
    AafReader, ObjId, Prop, TOO_DEEP, TOO_LARGE, TOO_MANY_OBJECTS, UNREADABLE_PARTS, WeakRef,
};
use core_io::aaf::timeline::{AafFile, NO_SOURCE, POINTS_CUT};
use core_io::dawproject::package::{AudioPackage, AudioSource};
use core_io::limits::{
    AafLimits, MAX_CONTROL_POINTS, MAX_IMPORT_TRACKS, MAX_INDEX_ENTRIES, MAX_MOB_CHAIN,
    MAX_PROPERTIES_BYTES, MAX_REF_DEPTH, MAX_TEXT_BYTES,
};
use core_io::report::{ItemKind, Report};
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;

const R48: (i32, i32) = (48_000, 1);

fn no() -> AtomicBool {
    AtomicBool::new(false)
}

/// `fx` written into `dir`.
fn written(dir: &Path, fx: &Fx) -> PathBuf {
    let p = dir.join(format!("f{}.aaf", std::fs::read_dir(dir).unwrap().count()));
    std::fs::write(&p, fx.bytes()).unwrap();
    p
}

fn read(p: &Path, limits: &AafLimits) -> Result<AafReader, IoError> {
    AafReader::open(p, limits, &no())
}

fn refused(r: Result<impl std::fmt::Debug, IoError>, why: &str) {
    match r {
        Err(IoError::Refused(w)) => assert_eq!(w, why),
        other => panic!("{other:?}"),
    }
}

/// The details of `what`'s Left out lines.
fn left_out<'a>(r: &'a Report, what: &str) -> Vec<&'a str> {
    r.items
        .iter()
        .filter(|i| i.kind == ItemKind::LeftOut && i.what == what)
        .flat_map(|i| i.details.iter().map(String::as_str))
        .collect()
}

/// A file with one audio clip.
fn small() -> Fx {
    let mut fx = Fx::new();
    let a = fx.file(&FileSpec::wav("A", 48_000, 1, 480));
    fx.comp(
        "Song",
        true,
        &[sound(1, "T", R48, Seg::Seq(vec![clip(&a, 1, 0, 100)]))],
    );
    fx
}

/// Adds a chain of `n` objects below `from`, each the strong reference
/// `0x7f00` of the one before; returns the last.
fn chain(fx: &mut Fx, from: ObjId, n: usize) -> ObjId {
    let mut at = from;
    for _ in 0..n {
        let next = fx.s.add_child(at, 0x7f00, None, class::FILLER);
        fx.s.objects[at.0 as usize]
            .props
            .insert(0x7f00, Prop::Strong(next));
        at = next;
    }
    at
}

// ---------------------------------------------------------------- structure

#[test]
fn objects() {
    let dir = tempfile::tempdir().unwrap();
    let fx = small();
    let p = written(dir.path(), &fx);
    let n = read(&p, &AafLimits::default())
        .unwrap()
        .store()
        .objects
        .len();
    let at = AafLimits {
        max_objects: n,
        ..AafLimits::default()
    };
    assert_eq!(read(&p, &at).unwrap().store().objects.len(), n);
    let under = AafLimits {
        max_objects: n - 1,
        ..AafLimits::default()
    };
    refused(read(&p, &under), TOO_MANY_OBJECTS);
}

#[test]
fn reference_depth() {
    let dir = tempfile::tempdir().unwrap();
    for (depth, reads) in [(MAX_REF_DEPTH, true), (MAX_REF_DEPTH + 1, false)] {
        let mut fx = small();
        let root = fx.s.root;
        let last = chain(&mut fx, root, depth);
        let p = written(dir.path(), &fx);
        let r = read(&p, &AafLimits::default());
        if reads {
            let s = r.unwrap();
            let path = &s.store().objects.last().unwrap().path;
            assert_eq!(path.matches('/').count(), depth, "{path}");
            assert_eq!(fx.s.objects[last.0 as usize].path, *path);
        } else {
            refused(r, TOO_DEEP);
        }
    }
}

#[test]
fn properties_bytes() {
    let dir = tempfile::tempdir().unwrap();
    for (size, reads) in [
        (MAX_PROPERTIES_BYTES, true),
        (MAX_PROPERTIES_BYTES + 1, false),
    ] {
        let mut fx = small();
        let root = fx.s.root;
        let big = chain(&mut fx, root, 1);
        // A stream of 17 values: its 4-byte header, 6 bytes per entry, then
        // the values, 65 535 bytes at most each.
        let n = 17u16;
        let mut left = size as usize - 4 - 6 * usize::from(n);
        for k in 0..n {
            let len = left.min(65_535);
            left -= len;
            fx.s.objects[big.0 as usize]
                .props
                .insert(0x7000 + k, Prop::Data(vec![0x5A; len]));
        }
        assert_eq!(left, 0);
        let p = written(dir.path(), &fx);
        let r = read(&p, &AafLimits::default()).unwrap();
        let path = fx.s.objects[big.0 as usize].path.clone();
        let found = r.store().objects.iter().any(|o| o.path == path);
        let listed = left_out(r.report(), UNREADABLE_PARTS);
        if reads {
            assert!(found);
            assert!(listed.is_empty(), "{listed:?}");
        } else {
            assert!(!found);
            assert_eq!(listed, [format!("{path}: properties too large")]);
        }
    }
}

#[test]
fn index_entries() {
    let dir = tempfile::tempdir().unwrap();
    for (count, reads) in [(MAX_INDEX_ENTRIES, true), (MAX_INDEX_ENTRIES + 1, false)] {
        let mut fx = small();
        let root = fx.s.root;
        let holder = chain(&mut fx, root, 1);
        // A weak vector of one-byte identifications: its index is 9 bytes
        // and one per entry.
        let refs = vec![
            WeakRef {
                table_index: 0,
                id_pid: pid::DEFINITION_OBJECT_IDENTIFICATION,
                id: vec![7],
            };
            count
        ];
        fx.s.objects[holder.0 as usize]
            .props
            .insert(0x7001, Prop::WeakVec(refs));
        let p = written(dir.path(), &fx);
        let r = read(&p, &AafLimits::default()).unwrap();
        let path = fx.s.objects[holder.0 as usize].path.clone();
        let o = r.store().objects.iter().find(|o| o.path == path).unwrap();
        let listed = left_out(r.report(), UNREADABLE_PARTS);
        if reads {
            assert!(matches!(o.props.get(&0x7001), Some(Prop::WeakVec(v)) if v.len() == count));
            assert!(listed.is_empty(), "{listed:?}");
        } else {
            assert!(!o.props.contains_key(&0x7001));
            assert_eq!(
                listed,
                [format!("{path}: property 7001 has too many entries")]
            );
        }
    }
}

#[test]
fn mob_chain() {
    let dir = tempfile::tempdir().unwrap();
    // The clip's mob, the masters in front of the file's master, that
    // master and the file's source mob: `mobs` in all.
    for (mobs, reads) in [(MAX_MOB_CHAIN, true), (MAX_MOB_CHAIN + 1, false)] {
        let mut fx = Fx::new();
        let mut target = fx.file(&FileSpec::wav("A", 48_000, 1, 480));
        for _ in 0..mobs - 2 {
            let id = fx.new_id();
            fx.mob(
                class::MASTER_MOB,
                &id,
                "Hop",
                false,
                &[sound(1, "", R48, clip(&target, 1, 0, 100))],
            );
            target = id;
        }
        fx.comp(
            "Song",
            true,
            &[sound(1, "T", R48, Seg::Seq(vec![clip(&target, 1, 0, 100)]))],
        );
        let p = written(dir.path(), &fx);
        let f = AafFile::open(&p, None, &AafLimits::default(), &no()).unwrap();
        let c = &f.composition().slots[0].clips[0];
        let source = &f.audio().iter().find(|a| a.key == c.media).unwrap().source;
        let listed = left_out(&f.composition().report, NO_SOURCE);
        if reads {
            assert!(!matches!(source, AudioSource::Offline(_)), "{source:?}");
            assert!(listed.is_empty(), "{listed:?}");
        } else {
            assert!(matches!(source, AudioSource::Offline(_)), "{source:?}");
            assert_eq!(listed, ["its chain of sources loops or runs too deep"]);
        }
    }
}

#[test]
fn control_points() {
    let dir = tempfile::tempdir().unwrap();
    let limits = AafLimits {
        max_control_points: 100,
        ..AafLimits::default()
    };
    assert_eq!(AafLimits::default().max_control_points, MAX_CONTROL_POINTS);
    for (n, cut) in [(100, false), (101, true)] {
        let mut fx = Fx::new();
        let a = fx.file(&FileSpec::wav("A", 48_000, 1, 48_000));
        let pts = (0..n).map(|i| ((i, n), (1, 2))).collect::<Vec<_>>();
        fx.comp(
            "Song",
            true,
            &[sound(
                1,
                "T",
                R48,
                Seg::Op {
                    op: def::OPERATION_DEF_MONO_AUDIO_GAIN,
                    param: def::PARAMETER_DEF_AMPLITUDE,
                    value: Value::Points(pts, def::INTERPOLATION_DEF_LINEAR),
                    input: Box::new(Seg::Seq(vec![clip(&a, 1, 0, 1_000)])),
                },
            )],
        );
        let p = written(dir.path(), &fx);
        let f = AafFile::open(&p, None, &limits, &no()).unwrap();
        let c = f.composition();
        assert_eq!(c.slots[0].gain.points.len(), 100);
        let listed = c
            .report
            .items
            .iter()
            .any(|i| i.kind == ItemKind::LeftOut && i.what == POINTS_CUT);
        assert_eq!(listed, cut);
    }
}

// ---------------------------------------------------------------- text, tracks, memory

#[test]
fn text_bytes() {
    let dir = tempfile::tempdir().unwrap();
    // 21 845 three-byte characters, then one-byte ones: UTF-16 fits a
    // property, and the text is exactly at the limit, or one byte past it.
    let base = "\u{3042}".repeat(21_845);
    // The composition's tagged comment holds it.
    for (text, whole) in [(format!("{base}a"), true), (format!("{base}ab"), false)] {
        let mut fx = Fx::new();
        let id = fx.new_id();
        let mob = fx.mob(class::COMPOSITION_MOB, &id, "Tagged", true, &[]);
        let tag =
            fx.s.add_child(mob, pid::MOB_USER_COMMENTS, Some(0), class::TAGGED_VALUE);
        fx.s.objects[mob.0 as usize]
            .props
            .insert(pid::MOB_USER_COMMENTS, Prop::StrongVec(vec![tag]));
        let o = &mut fx.s.objects[tag.0 as usize].props;
        o.insert(pid::TAGGED_VALUE_NAME, Prop::Data(utf16z("Note")));
        o.insert(
            pid::TAGGED_VALUE_VALUE,
            Prop::Data(indirect(&utf16z(&text))),
        );
        let p = written(dir.path(), &fx);
        let f = AafFile::open(&p, None, &AafLimits::default(), &no()).unwrap();
        let comments = &f.composition().comments;
        assert_eq!(comments.len(), 1);
        assert_eq!(comments[0].1.len(), MAX_TEXT_BYTES);
        assert_eq!(comments[0].1 == text, whole);
        assert!(text.starts_with(&comments[0].1));
    }
}

#[test]
fn tracks() {
    let dir = tempfile::tempdir().unwrap();
    for (n, reads) in [(MAX_IMPORT_TRACKS, true), (MAX_IMPORT_TRACKS + 1, false)] {
        let mut fx = Fx::new();
        let slots: Vec<_> = (1..=n as u32)
            .map(|k| sound(k, &format!("T{k}"), R48, Seg::Seq(vec![Seg::Fill(10)])))
            .collect();
        fx.comp("Song", true, &slots);
        let p = written(dir.path(), &fx);
        let r = AafFile::open(&p, None, &AafLimits::default(), &no());
        if reads {
            assert_eq!(r.unwrap().composition().slots.len(), n);
        } else {
            refused(
                r,
                "This AAF file has more than 1000 tracks, more than VizDAW can open.",
            );
        }
    }
}

#[test]
fn model_budget() {
    let dir = tempfile::tempdir().unwrap();
    let p = written(dir.path(), &small());
    let with = |bytes: usize| {
        read(
            &p,
            &AafLimits {
                max_model_bytes: bytes,
                ..AafLimits::default()
            },
        )
    };
    // The least budget the file reads in, found by bisection.
    let (mut lo, mut hi) = (0usize, 64 << 20);
    assert!(with(hi).is_ok());
    while hi - lo > 1 {
        let mid = lo + (hi - lo) / 2;
        if with(mid).is_ok() {
            hi = mid;
        } else {
            lo = mid;
        }
    }
    assert!(with(hi).is_ok());
    refused(with(hi - 1), TOO_LARGE);
}
