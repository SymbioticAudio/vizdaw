//! The AAF object store on `cfb` (plan 6f Task 1): our writer's files read
//! back object for object, essence streams copy within their bound, and
//! hostile structure is refused or left out and listed within the limits
//! of Ruling 13.

mod common;

use common::aaf::{canonical, props, raw_file, tree, utf16z};
use common::{CountingAlloc, measure};
use core_io::IoError;
use core_io::aaf::dict::{self, class, pid};
use core_io::aaf::store::{
    AafReader, AafWriter, Auid, BIG_ENDIAN, NOT_AAF, ObjId, Prop, Store, TOO_DEEP,
    TOO_MANY_OBJECTS, UNREADABLE_PARTS,
};
use core_io::limits::{AafLimits, MAX_TEXT_BYTES};
use core_io::report::ItemKind;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;

#[global_allocator]
static ALLOC: CountingAlloc = CountingAlloc;

const MIB: usize = 1024 * 1024;

fn no() -> AtomicBool {
    AtomicBool::new(false)
}

/// Writes `s` (and `essence` into the data stream of `stream_obj`) to
/// `dir/name`.
fn write(dir: &Path, name: &str, s: &Store, essence: Option<(ObjId, &[u8])>) -> PathBuf {
    let p = dir.join(name);
    let f = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create_new(true)
        .open(&p)
        .unwrap();
    let mut w = AafWriter::create(f).unwrap();
    w.write_store(s).unwrap();
    if let Some((obj, bytes)) = essence {
        let mut st = w.stream(s, obj, "Data-2702").unwrap();
        st.write_all(bytes).unwrap();
        st.flush().unwrap();
    }
    w.finish().unwrap();
    p
}

fn open(p: &Path) -> Result<AafReader, IoError> {
    AafReader::open(p, &AafLimits::default(), &no())
}

fn refusal<T>(r: Result<T, IoError>) -> String {
    match r {
        Err(IoError::Refused(s)) => s,
        Err(e) => panic!("expected a refusal, got {e:?}"),
        Ok(_) => panic!("expected a refusal, got Ok"),
    }
}

fn unreadable(r: &AafReader) -> u64 {
    r.report()
        .items
        .iter()
        .find(|i| i.kind == ItemKind::LeftOut && i.what == UNREADABLE_PARTS)
        .map_or(0, |i| i.count)
}

fn find(s: &Store, cls: Auid) -> ObjId {
    ObjId(s.objects.iter().position(|o| o.class == cls).unwrap() as u32)
}

#[test]
fn store_round_trips() {
    let dir = tempfile::tempdir().unwrap();
    let s = tree();
    let p = write(dir.path(), "t.aaf", &s, None);
    let r = open(&p).unwrap();
    assert_eq!(canonical(r.store()), canonical(&s));
    assert_eq!(r.store().referenced, s.referenced);
    assert_eq!(r.store().objects.len(), s.objects.len());
    assert!(r.report().is_empty(), "{:?}", r.report());

    let c = cfb::CompoundFile::open(std::fs::File::open(&p).unwrap()).unwrap();
    assert_eq!(c.version(), cfb::Version::V4);
    // The byte-order fact: a class AUID goes in as the CLSID and prints the
    // same through `cfb` (`Uuid::from_fields`) as in the plan's tables.
    assert_eq!(
        c.root_entry().clsid().to_string(),
        "b3b398a5-1c90-11d4-8053-080036210804"
    );
    assert_eq!(
        class::ROOT.to_string(),
        "b3b398a5-1c90-11d4-8053-080036210804"
    );
    assert_eq!(class::ROOT.0[0], 0xa5);
    assert_eq!(
        c.entry("/0002").unwrap().clsid().to_string(),
        class::HEADER.to_string()
    );
    assert_eq!(
        class::HEADER.to_string(),
        "0d010101-0101-2f00-060e-2b3402060101"
    );
    assert_eq!(
        dict::def::DATA_DEF_SOUND.to_string(),
        "01030202-0200-0000-060e-2b3404010101"
    );
}

#[test]
fn essence_streams_round_trip() {
    let dir = tempfile::tempdir().unwrap();
    let s = tree();
    let ed = find(&s, class::ESSENCE_DATA);
    let bytes: Vec<u8> = (0..3 * MIB).map(|i| (i * 7 + i / 4096) as u8).collect();
    let p = write(dir.path(), "e.aaf", &s, Some((ed, &bytes)));
    let r = open(&p).unwrap();
    let red = find(r.store(), class::ESSENCE_DATA);
    assert_eq!(
        r.store().objects[red.0 as usize]
            .props
            .get(&pid::ESSENCE_DATA_DATA),
        Some(&Prop::Stream("Data-2702".into()))
    );
    assert_eq!(r.stream_len(red, "Data-2702").unwrap(), bytes.len() as u64);

    let to = dir.path().join("out.wav");
    let n = r
        .copy_stream(red, "Data-2702", &to, bytes.len() as u64, &no())
        .unwrap();
    assert_eq!(n, bytes.len() as u64);
    assert!(std::fs::read(&to).unwrap() == bytes);
    // `to` is created new: an existing file is never replaced.
    assert!(
        r.copy_stream(red, "Data-2702", &to, bytes.len() as u64, &no())
            .is_err()
    );
    assert!(std::fs::read(&to).unwrap() == bytes);

    let short = dir.path().join("short.wav");
    refusal(r.copy_stream(red, "Data-2702", &short, bytes.len() as u64 - 1, &no()));
    assert!(std::fs::symlink_metadata(&short).is_err());
    // A name that leaves the object's storage is refused.
    refusal(r.copy_stream(red, "../0002", &short, u64::MAX, &no()));
    assert!(std::fs::symlink_metadata(&short).is_err());
    // Cancelled mid-way leaves nothing either.
    let cancel = AtomicBool::new(true);
    assert_eq!(
        r.copy_stream(red, "Data-2702", &short, u64::MAX, &cancel),
        Err(IoError::Cancelled)
    );
    assert!(std::fs::symlink_metadata(&short).is_err());
}

#[test]
fn big_endian_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path().join("be.aaf");
    let mut be = props(&[]);
    be[0] = 0x42;
    raw_file(&p, &[("/properties", Some(be))]);
    assert_eq!(refusal(open(&p)), BIG_ENDIAN);
    assert_eq!(
        BIG_ENDIAN,
        "This AAF file is big-endian; VizDAW reads little-endian files."
    );

    // Also below the root.
    let p = dir.path().join("be2.aaf");
    let mut be = props(&[]);
    be[0] = 0x42;
    raw_file(
        &p,
        &[
            ("/properties", Some(props(&[(2, 0x22, utf16z("H"))]))),
            ("/H", None),
            ("/H/properties", Some(be)),
        ],
    );
    assert_eq!(refusal(open(&p)), BIG_ENDIAN);
}

#[test]
fn not_a_compound_file() {
    let dir = tempfile::tempdir().unwrap();
    let zeros = dir.path().join("zeros.aaf");
    std::fs::write(&zeros, vec![0u8; 4096]).unwrap();
    assert_eq!(refusal(open(&zeros)), NOT_AAF);
    assert_eq!(NOT_AAF, "This isn't an AAF file.");

    let empty = dir.path().join("empty.aaf");
    std::fs::write(&empty, b"").unwrap();
    assert_eq!(refusal(open(&empty)), NOT_AAF);

    // A compound file that is not AAF (no root `properties`).
    let other = dir.path().join("other.aaf");
    raw_file(&other, &[("/WordDocument", Some(vec![1, 2, 3]))]);
    assert_eq!(refusal(open(&other)), NOT_AAF);

    // The magic number with junk after it is damaged, not "not AAF".
    let junk = dir.path().join("junk.aaf");
    let mut j = vec![0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1];
    j.resize(4096, 0xEE);
    std::fs::write(&junk, j).unwrap();
    assert_eq!(refusal(open(&junk)), core_io::aaf::store::DAMAGED);

    let folder = dir.path().join("folder.aaf");
    std::fs::create_dir(&folder).unwrap();
    assert!(refusal(open(&folder)).ends_with("is not a regular file"));

    let fifo = dir.path().join("fifo.aaf");
    match std::process::Command::new("mkfifo").arg(&fifo).status() {
        Ok(st) if st.success() => {
            assert!(refusal(open(&fifo)).ends_with("is not a regular file"))
        }
        _ => println!("mkfifo is missing: FIFO case skipped"),
    }
}

#[test]
fn strong_names_stay_below() {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path().join("names.aaf");
    raw_file(
        &p,
        &[
            (
                "/properties",
                Some(props(&[
                    (2, 0x22, utf16z("H")),
                    (3, 0x22, utf16z("..")),
                    (4, 0x22, utf16z("/x")),
                    (5, 0x32, utf16z("..")),
                ])),
            ),
            ("/H", None),
            (
                "/H/properties",
                Some(props(&[
                    (0x3b03, 0x22, utf16z("H")),
                    (0x3b04, 0x22, utf16z("C")),
                    (0x3b05, 0x22, utf16z(".")),
                    (0x3b06, 0x22, utf16z("a\\b")),
                ])),
            ),
            ("/H/C", None),
            ("/H/C/properties", Some(props(&[(0x0101, 0x82, vec![7])]))),
            ("/x", None),
            ("/x/properties", Some(props(&[]))),
        ],
    );
    let r = open(&p).unwrap();
    let s = r.store();
    let paths: Vec<&str> = s.objects.iter().map(|o| o.path.as_str()).collect();
    assert_eq!(paths, ["/", "/H", "/H/C"]);
    let root = &s.objects[s.root.0 as usize];
    assert!(matches!(root.props.get(&2), Some(Prop::Strong(_))));
    assert!(!root.props.contains_key(&3) && !root.props.contains_key(&4));
    assert!(!root.props.contains_key(&5));
    let hp = &s.objects[1].props;
    assert!(matches!(hp.get(&0x3b04), Some(Prop::Strong(ObjId(2)))));
    assert!(!hp.contains_key(&0x3b03) && !hp.contains_key(&0x3b05) && !hp.contains_key(&0x3b06));
    assert_eq!(s.objects[2].props.get(&0x0101), Some(&Prop::Data(vec![7])));
    assert_eq!(unreadable(&r), 6, "{:?}", r.report());
    assert_eq!(UNREADABLE_PARTS, "Parts of the file that could not be read");
}

#[test]
fn index_counts_fit_their_streams() {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path().join("index.aaf");
    let mut claim = 0xFFFF_FFFFu32.to_le_bytes().to_vec();
    claim.extend_from_slice(&[0; 8]);
    let mut set_claim = claim.clone();
    set_claim.extend_from_slice(&[0x01, 0x44, 32]);
    let mut weak_claim = 0xFFFF_FFFFu32.to_le_bytes().to_vec();
    weak_claim.extend_from_slice(&[0, 0, 0x01, 0x1b, 16]);
    // A count the limit allows, of empty identifications: no room is
    // taken in the stream, so the count alone must not allocate.
    let mut empty_ids = 1_000_000u32.to_le_bytes().to_vec();
    empty_ids.extend_from_slice(&[0, 0, 0x01, 0x1b, 0]);
    raw_file(
        &p,
        &[
            (
                "/properties",
                Some(props(&[
                    (2, 0x32, utf16z("V")),
                    (3, 0x3A, utf16z("S")),
                    (4, 0x12, utf16z("W")),
                    (5, 0x82, vec![1, 2]),
                    (6, 0x1A, utf16z("Z")),
                ])),
            ),
            ("/V index", Some(claim)),
            ("/S index", Some(set_claim)),
            ("/W index", Some(weak_claim)),
            ("/Z index", Some(empty_ids)),
        ],
    );
    let (r, seen) = measure(|| open(&p));
    let r = r.unwrap();
    assert!(seen.peak < MIB, "peak {} bytes", seen.peak);
    let root = &r.store().objects[0];
    assert_eq!(root.props.keys().copied().collect::<Vec<_>>(), [5]);
    assert_eq!(unreadable(&r), 4, "{:?}", r.report());
}

/// A root with a chain of `n` objects below it, each holding the next.
fn chain(n: usize) -> Store {
    let mut s = Store::new(class::ROOT);
    let mut at = s.root;
    for _ in 0..n {
        let c = s.add_child(at, pid::ROOT_HEADER, None, class::HEADER);
        s.objects[at.0 as usize]
            .props
            .insert(pid::ROOT_HEADER, Prop::Strong(c));
        at = c;
    }
    s
}

/// A root holding `n` objects in one vector.
fn wide(n: usize) -> Store {
    let mut s = Store::new(class::ROOT);
    let root = s.root;
    let kids: Vec<ObjId> = (0..n)
        .map(|i| {
            s.add_child(
                root,
                pid::MOB_SLOTS,
                Some(i as u32),
                class::TIMELINE_MOB_SLOT,
            )
        })
        .collect();
    s.objects[0]
        .props
        .insert(pid::MOB_SLOTS, Prop::StrongVec(kids));
    s
}

#[test]
fn depth_and_object_limits() {
    let dir = tempfile::tempdir().unwrap();
    let ok = write(dir.path(), "64.aaf", &chain(64), None);
    assert_eq!(open(&ok).unwrap().store().objects.len(), 65);
    let deep = write(dir.path(), "65.aaf", &chain(65), None);
    assert_eq!(refusal(open(&deep)), TOO_DEEP);

    let limits = AafLimits {
        max_objects: 100,
        ..AafLimits::default()
    };
    let hundred = write(dir.path(), "100.aaf", &wide(99), None);
    let r = AafReader::open(&hundred, &limits, &no()).unwrap();
    assert_eq!(r.store().objects.len(), 100);
    let more = write(dir.path(), "101.aaf", &wide(100), None);
    assert_eq!(
        refusal(AafReader::open(&more, &limits, &no())),
        TOO_MANY_OBJECTS
    );

    // A storage named twice is read once; the second reference is listed.
    let p = dir.path().join("twice.aaf");
    let mut idx = 2u32.to_le_bytes().to_vec();
    idx.extend_from_slice(&[2, 0, 0, 0, 0xff, 0xff, 0xff, 0xff, 0, 0, 0, 0, 0, 0, 0, 0]);
    raw_file(
        &p,
        &[
            ("/properties", Some(props(&[(2, 0x32, utf16z("V"))]))),
            ("/V index", Some(idx)),
            ("/V{0}", None),
            ("/V{0}/properties", Some(props(&[]))),
        ],
    );
    let r = open(&p).unwrap();
    assert_eq!(r.store().objects.len(), 2);
    assert_eq!(
        r.store().objects[0].props.get(&2),
        Some(&Prop::StrongVec(vec![ObjId(1)]))
    );
    assert_eq!(unreadable(&r), 1);
}

#[test]
fn strings_are_sanitised() {
    let l = AafLimits::default();
    let u16s = |v: &[u16]| -> Vec<u8> { v.iter().flat_map(|c| c.to_le_bytes()).collect() };

    // Plain, with and without the NUL.
    assert_eq!(
        dict::string(&Prop::Data(utf16z("Kick")), &l),
        ("Kick".to_string(), false)
    );
    let mut no_nul = utf16z("Kick");
    no_nul.truncate(8);
    assert_eq!(
        dict::string(&Prop::Data(no_nul.clone()), &l),
        ("Kick".to_string(), false)
    );
    // An odd length drops the last byte.
    no_nul.push(0x41);
    assert_eq!(
        dict::string(&Prop::Data(no_nul), &l),
        ("Kick".to_string(), true)
    );
    // An unpaired surrogate becomes U+FFFD.
    let s = u16s(&[0x41, 0xD800, 0x42, 0]);
    assert_eq!(
        dict::string(&Prop::Data(s), &l),
        ("A\u{FFFD}B".to_string(), true)
    );
    // Control characters and bidirectional overrides are dropped.
    let s = u16s(&[0x41, 0x07, 0x202E, 0x42]);
    assert_eq!(dict::string(&Prop::Data(s), &l), ("AB".to_string(), true));
    // 70 000 characters are cut to MAX_TEXT_BYTES.
    let long: Vec<u16> = std::iter::repeat_n(0x61, 70_000).collect();
    let (t, cut) = dict::string(&Prop::Data(u16s(&long)), &l);
    assert!(cut);
    assert_eq!(t.len(), MAX_TEXT_BYTES);
    // Not data: empty, flagged.
    assert_eq!(
        dict::string(&Prop::Stream("x".into()), &l),
        (String::new(), true)
    );
}

#[test]
fn decoders_check_their_values() {
    assert_eq!(
        dict::i64_at(&Prop::Data((-5i64).to_le_bytes().to_vec())),
        Some(-5)
    );
    assert_eq!(dict::i64_at(&Prop::Data(vec![1, 2, 3])), None);
    let rat = |n: i32, d: i32| {
        let mut v = n.to_le_bytes().to_vec();
        v.extend_from_slice(&d.to_le_bytes());
        Prop::Data(v)
    };
    assert_eq!(dict::rational(&rat(48_000, 1)), Some((48_000, 1)));
    assert_eq!(dict::rational(&rat(1, 0)), None);
    assert_eq!(dict::rational(&rat(1, -1)), None);
    assert_eq!(
        dict::auid(&Prop::Data(class::ROOT.0.to_vec())),
        Some(class::ROOT)
    );
    assert_eq!(dict::auid(&Prop::Data(vec![0; 15])), None);
}

#[test]
fn cancel_stops_the_read() {
    let dir = tempfile::tempdir().unwrap();
    let p = write(dir.path(), "c.aaf", &tree(), None);
    let cancel = AtomicBool::new(true);
    assert!(matches!(
        AafReader::open(&p, &AafLimits::default(), &cancel),
        Err(IoError::Cancelled)
    ));
}

/// A root whose `n` properties of `form` all name the index `name`, which
/// is `index`.
fn sharing(p: &Path, n: u16, form: u16, name: &str, index: Vec<u8>) {
    let entries: Vec<(u16, u16, Vec<u8>)> = (1..=n).map(|pid| (pid, form, utf16z(name))).collect();
    let index_path = format!("/{name} index");
    raw_file(
        p,
        &[
            ("/properties", Some(props(&entries))),
            (&index_path, Some(index)),
        ],
    );
}

#[test]
fn shared_indexes_are_read_once() {
    let dir = tempfile::tempdir().unwrap();
    const PROPS: u16 = 60_000;
    const KEYS: u32 = 100_000;

    // 60 000 strong vectors naming one index of 100 000 keys: read once,
    // the rest listed (before the fix: 6·10⁹ children).
    let p = dir.path().join("strong.aaf");
    let mut idx = KEYS.to_le_bytes().to_vec();
    idx.extend_from_slice(&KEYS.to_le_bytes());
    idx.extend_from_slice(&u32::MAX.to_le_bytes());
    for k in 0..KEYS {
        idx.extend_from_slice(&k.to_le_bytes());
    }
    sharing(&p, PROPS, 0x32, "a", idx);
    let start = std::time::Instant::now();
    let (r, seen) = measure(|| open(&p));
    let r = r.unwrap();
    assert!(start.elapsed().as_secs() < 20, "{:?}", start.elapsed());
    assert!(seen.peak < 64 * MIB, "peak {} bytes", seen.peak);
    let root = &r.store().objects[0];
    assert_eq!(root.props.len(), 1);
    // 59 999 shared, and the one read names 100 000 missing storages.
    assert_eq!(unreadable(&r), u64::from(PROPS) - 1 + u64::from(KEYS));

    // The same with weak vectors: one index of 100 000 ids.
    let p = dir.path().join("weak.aaf");
    let mut idx = KEYS.to_le_bytes().to_vec();
    idx.extend_from_slice(&[0, 0, 0x01, 0x1b, 16]);
    for k in 0..KEYS {
        idx.extend_from_slice(&[k as u8; 16]);
    }
    sharing(&p, PROPS, 0x12, "w", idx);
    let (r, seen) = measure(|| open(&p));
    let r = r.unwrap();
    assert!(seen.peak < 64 * MIB, "peak {} bytes", seen.peak);
    let root = &r.store().objects[0];
    assert_eq!(root.props.len(), 1);
    assert!(matches!(root.props.get(&1), Some(Prop::WeakVec(v)) if v.len() == KEYS as usize));
    assert_eq!(unreadable(&r), u64::from(PROPS) - 1);
}

#[test]
fn huge_index_counts_are_charged_first() {
    let dir = tempfile::tempdir().unwrap();
    const N: u32 = 1_000_000;
    let limits = AafLimits {
        max_model_bytes: 8 * MIB,
        ..AafLimits::default()
    };

    // A weak index whose count fits its stream and the entry limit, but
    // whose references would pass the budget: refused before they exist.
    let p = dir.path().join("weak.aaf");
    let mut idx = N.to_le_bytes().to_vec();
    idx.extend_from_slice(&[0, 0, 0x01, 0x1b, 1]);
    idx.resize(idx.len() + N as usize, 7);
    sharing(&p, 1, 0x12, "w", idx);
    let start = std::time::Instant::now();
    let (r, seen) = measure(|| AafReader::open(&p, &limits, &no()));
    assert_eq!(refusal(r), core_io::aaf::store::TOO_LARGE);
    assert!(start.elapsed().as_secs() < 5, "{:?}", start.elapsed());
    assert!(seen.peak < 2 * MIB, "peak {} bytes", seen.peak);

    // The same for a strong vector's keys.
    let p = dir.path().join("strong.aaf");
    let mut idx = N.to_le_bytes().to_vec();
    idx.extend_from_slice(&[0; 8]);
    idx.resize(idx.len() + 4 * N as usize, 0);
    sharing(&p, 1, 0x32, "a", idx);
    let (r, seen) = measure(|| AafReader::open(&p, &limits, &no()));
    assert_eq!(refusal(r), core_io::aaf::store::TOO_LARGE);
    assert!(seen.peak < 2 * MIB, "peak {} bytes", seen.peak);
}
