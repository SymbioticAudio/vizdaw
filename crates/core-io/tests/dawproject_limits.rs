//! The typed DAWproject model under hostile size (plan 6d Task 3, L7): a
//! file whose tree is at its limits reads, or is refused, within the memory
//! the plan promises, counting every allocation (a reallocation as an
//! allocation then a free); and (plan 6d Task 8) one test per limit of
//! Ruling 18, each at the limit (accepted) and one past it (refused, or
//! left out with its line in the report).

mod common;

use common::{CountingAlloc, measure};
use core_io::IoError;
use core_io::dawproject::schema::{check_ids, read_project};
use core_io::limits::{MAX_XML_NODES, PackageLimits};
use core_io::report::Report;
use core_io::xml::Tree;
use std::sync::atomic::AtomicBool;
use std::time::Instant;

#[global_allocator]
static ALLOC: CountingAlloc = CountingAlloc;

const MIB: usize = 1024 * 1024;

/// The most a fuzz case (XML under 64 MiB) may hold at once beyond its text
/// (L7: 256 MiB).
const PEAK: usize = 256 * MIB;

/// A project whose structure or arrangement repeats `item` `n` times.
fn repeated(open: &str, item: &str, n: usize, close: &str) -> String {
    format!(
        "<Project version=\"1.0\"><Application name=\"t\" version=\"1\"/>{open}{}{close}</Project>",
        item.repeat(n)
    )
}

/// Like [`repeated`], with the item's number between `before` and `after`.
fn unique(open: &str, before: &str, after: &str, n: usize, close: &str) -> String {
    use std::fmt::Write as _;
    let mut s = format!("<Project version=\"1.0\"><Application name=\"t\" version=\"1\"/>{open}");
    for i in 0..n {
        let _ = write!(s, "{before}{i}{after}");
    }
    s.push_str(close);
    s.push_str("</Project>");
    s
}

/// Parses, reads and checks `xml`; the tree is dropped before the model.
fn run(xml: &str) -> (Result<(), IoError>, usize, std::time::Duration) {
    let start = Instant::now();
    let (r, seen) = measure(|| {
        let tree = Tree::parse(
            xml.as_bytes(),
            &PackageLimits::default(),
            &AtomicBool::new(false),
        )?;
        let mut report = Report::default();
        let p = read_project(&tree, &mut report)?;
        check_ids(&p, &mut report).map(|_| ())
    });
    (r, seen.peak, start.elapsed())
}

#[test]
fn hostile_sizes_stay_inside_the_promise() {
    // One node over the tree's node cap is not what is tested here (Task 2
    // does that); these sit at the cap, each shape the model costs most for.
    let n = MAX_XML_NODES - 8;
    let shapes = [
        (
            "tracks",
            repeated("<Structure>", "<Track/>", n, "</Structure>"),
        ),
        (
            "clips",
            repeated(
                "<Arrangement><Lanes><Clips>",
                "<Clip time=\"0\"/>",
                n,
                "</Clips></Lanes></Arrangement>",
            ),
        ),
        (
            "notes",
            repeated(
                "<Arrangement><Lanes><Notes>",
                "<Note time=\"0\" duration=\"1\" channel=\"0\" key=\"60\"/>",
                n,
                "</Notes></Lanes></Arrangement>",
            ),
        ),
        (
            "channels with ids",
            repeated("<Structure>", "<Channel id=\"c\"/>", n / 2, "</Structure>"),
        ),
        (
            "unique track ids",
            unique(
                "<Structure>",
                "<Track id=\"t",
                "\"/>",
                1_900_000,
                "</Structure>",
            ),
        ),
        (
            "unique parameter ids",
            unique(
                "<Structure><Channel><Devices><Equalizer deviceName=\"E\" deviceRole=\"audioFX\">",
                "<InputGain unit=\"linear\" id=\"p",
                "\"/>",
                1_900_000,
                "</Equalizer></Devices></Channel></Structure>",
            ),
        ),
        (
            "short markers",
            repeated(
                "<Arrangement><Markers>",
                "<Marker name=\"m\" time=\"1\"/>",
                600_000,
                "</Markers></Arrangement>",
            ),
        ),
    ];
    for (name, xml) in &shapes {
        let (r, peak, took) = run(xml);
        println!(
            "{name}: {} MiB of XML, peak {} MiB, {:?}, {:?}",
            xml.len() / MIB,
            peak / MIB,
            took,
            r.as_ref().err()
        );
        assert!(peak < PEAK, "{name}: peak {} MiB", peak / MIB);
        // Past the budget the file is refused as having too many parts
        // (a duplicate ID is the other refusal here).
        if let Err(e) = r {
            let IoError::Refused(s) = e else {
                panic!("{name}: {e:?}")
            };
            assert!(
                s == "This DAWproject file is damaged: it has too many parts."
                    || s.contains("share the ID"),
                "{name}: {s}"
            );
        }
        assert!(took.as_secs() < 20, "{name}: {took:?}");
    }
}

#[test]
fn a_big_real_project_still_reads() {
    // 100 000 notes and 20 000 clips: well inside the budget.
    let notes =
        "<Note time=\"0\" duration=\"1\" channel=\"0\" key=\"60\" vel=\"0.5\"/>".repeat(100_000);
    let clips = "<Clip name=\"c\" time=\"1\" duration=\"2\"/>".repeat(20_000);
    let xml = repeated(
        "<Arrangement><Lanes><Clips>",
        &clips,
        1,
        &format!("</Clips><Notes>{notes}</Notes></Lanes></Arrangement>"),
    );
    let (r, peak, _) = run(&xml);
    r.expect("reads");
    assert!(peak < 128 * MIB, "peak {} MiB", peak / MIB);
}

#[test]
fn the_id_check_is_bounded_too() {
    use core_io::dawproject::schema::{DpNamed, DpProject, DpStructureItem, DpTrack};
    let track = |i: usize| {
        DpStructureItem::Track(DpTrack {
            named: DpNamed {
                id: Some(format!("t{i}")),
                ..DpNamed::default()
            },
            content: vec![],
            loaded: None,
            channel: None,
            children: vec![],
        })
    };
    let mut p = DpProject {
        version: "1.0".to_owned(),
        structure: (0..1_000_000).map(track).collect(),
        ..DpProject::default()
    };
    let (r, seen) = measure(|| check_ids(&p, &mut Report::default()).map(|ids| ids.kind("t7")));
    println!("1M unique ids: peak {} MiB", seen.peak / MIB);
    assert_eq!(
        r.expect("checks"),
        Some(core_io::dawproject::schema::IdKind::Track)
    );
    assert!(seen.peak < 128 * MIB, "peak {} MiB", seen.peak / MIB);
    // With the model's bytes already near the budget, the same check is
    // refused, early.
    p.model_bytes = core_io::limits::MAX_MODEL_BYTES - 1000;
    let (r, seen) = measure(|| check_ids(&p, &mut Report::default()).map(|_| ()));
    println!("near the budget: peak {} MiB", seen.peak / MIB);
    assert_eq!(
        r,
        Err(IoError::Refused(
            "This DAWproject file is damaged: it has too many parts.".to_owned()
        ))
    );
    assert!(seen.peak < 4 * MIB, "peak {} MiB", seen.peak / MIB);
}

// ================================================================ the limits table (Task 8)

use common::rawzip_builder::{RawEntry, raw_zip};
use core_io::dawproject::import::{
    ImportEnv, LOOP_OUTPUTS, MediaSource, NESTED_TOO_DEEP, OFFLINE, PAST_END, PLUGIN_SETTINGS,
    apply, map,
};
use core_io::dawproject::package::{
    AudioSource, DpPackage, RECORD_UNREADABLE, SONG_INFO_UNREADABLE, debug_take_external_stats,
    open,
};
use core_io::dawproject::plugins::NoPlist;
use core_io::dawproject::schema::{MISSING_REFERENCES, UNREADABLE_PARTS, UNREADABLE_VALUES};
use core_io::limits;
use core_io::report::ItemKind;
use core_io::zip::{EntryMethod, TOO_LARGE, ZipReader, ZipWriter, check_name};

const TOO_LARGE_TO_OPEN: &str = "is too large to open.";
use core_model::history::History;
use core_model::mixer::PluginFormat;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// A project of `structure` (after a master `m`/`mc`) and `arrangement`.
fn project(structure: &str, arrangement: &str) -> String {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<Project version="1.0"><Application name="T" version="1"/><Structure><Track id="m" name="Master" contentType="audio notes"><Channel id="mc" role="master"/></Track>{structure}</Structure><Arrangement>{arrangement}</Arrangement></Project>"#
    )
}

/// A package at `dir/<stem>.dawproject` of `entries`, every one stored (a
/// stored entry's ratio is 1, so no ratio limit is in the way).
fn stored(dir: &Path, stem: &str, entries: &[(&str, &[u8])]) -> PathBuf {
    let path = dir.join(format!("{stem}.dawproject"));
    let mut z = ZipWriter::new(std::fs::File::create(&path).unwrap());
    for (name, bytes) in entries {
        z.add(name, EntryMethod::Stored, bytes).unwrap();
    }
    z.finish().unwrap();
    path
}

fn open_with(path: &Path, limits: &PackageLimits) -> Result<DpPackage, IoError> {
    open(path, limits, &AtomicBool::new(false))
}

fn refused(r: Result<DpPackage, IoError>) -> String {
    match r {
        Err(IoError::Refused(s)) => s,
        Err(e) => panic!("{e:?}"),
        Ok(_) => panic!("opened, but should have been refused"),
    }
}

/// `xml` as a package, opened with `limits`.
fn open_xml(xml: &str, limits: &PackageLimits) -> Result<DpPackage, IoError> {
    let dir = tempfile::tempdir().unwrap();
    let path = stored(dir.path(), "P", &[("project.xml", xml.as_bytes())]);
    open_with(&path, limits)
}

fn yes(_: PluginFormat, _: &str) -> bool {
    true
}

fn mapped(
    pkg: &DpPackage,
    media: &BTreeMap<usize, MediaSource>,
) -> core_io::dawproject::import::DpImport {
    map(
        pkg,
        media,
        &ImportEnv {
            installed: &yes,
            plist: &NoPlist,
            rate: 48_000,
        },
        &AtomicBool::new(false),
    )
    .expect("maps")
}

/// The report of mapping `pkg` and applying it to a new project (which must
/// apply).
fn imported_report(pkg: &DpPackage) -> (Report, core_model::document::Document) {
    let imp = mapped(pkg, &BTreeMap::new());
    let mut h = History::new(core_model::document::Document::new());
    let r = h
        .execute(
            &core_model::command::Command::Sequence(core_model::command::SequenceCmd::New {
                name: "S".into(),
            }),
            &core_model::selection::ViewFilter::default(),
        )
        .unwrap();
    let seq = r
        .created
        .iter()
        .find_map(|id| match id {
            core_model::ids::AnyId::Seq(s) => Some(*s),
            _ => None,
        })
        .unwrap();
    let lane = h.doc().sequence(seq).unwrap().lanes.front().unwrap().id;
    h.execute(
        &core_model::command::Command::Lane(core_model::command::LaneCmd::Remove { seq, lane }),
        &core_model::selection::ViewFilter::default(),
    )
    .unwrap();
    h.begin("Import").unwrap();
    let mut report = Report::default();
    apply(&mut h, &imp, seq, &mut report).expect("applies");
    h.commit().unwrap();
    (report, h.doc().clone())
}

#[track_caller]
fn has_line(r: &Report, kind: ItemKind, what: &str) -> core_io::report::Item {
    r.items
        .iter()
        .find(|i| i.kind == kind && i.what == what)
        .unwrap_or_else(|| panic!("no {kind:?} '{what}' in {:#?}", r.items))
        .clone()
}

#[track_caller]
fn lacks_line(r: &Report, what: &str) {
    assert!(
        r.items.iter().all(|i| i.what != what),
        "unexpected '{what}' in {:#?}",
        r.items
    );
}

fn a_project() -> String {
    project("", "")
}

#[test]
fn zip_entries() {
    // 10 000 entries open; 10 001 are refused, whatever the end record
    // claims.
    let make = |n: usize| {
        let mut entries = vec![RawEntry::stored("project.xml", a_project().as_bytes())];
        entries.extend((1..n).map(|i| RawEntry::stored(&format!("f/{i}"), b"")));
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("Many.dawproject");
        std::fs::write(&path, raw_zip(&entries)).unwrap();
        (dir, path)
    };
    let (_dir, path) = make(limits::MAX_ZIP_ENTRIES);
    open_with(&path, &PackageLimits::default()).expect("10 000 entries open");
    let (_dir, path) = make(limits::MAX_ZIP_ENTRIES + 1);
    assert!(
        refused(open_with(&path, &PackageLimits::default())).contains("more than 10 000 files")
    );
}

#[test]
fn zip_names() {
    // Each forbidden form is refused; the longest legal name is not.
    let try_name = |name: &[u8]| {
        let entries = [
            RawEntry::stored("project.xml", a_project().as_bytes()),
            RawEntry {
                name: name.to_vec(),
                ..RawEntry::stored("x", b"")
            },
        ];
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("Names.dawproject");
        std::fs::write(&path, raw_zip(&entries)).unwrap();
        open_with(&path, &PackageLimits::default())
    };
    let long = vec![b'n'; limits::MAX_ENTRY_NAME_BYTES];
    let too_long = vec![b'n'; limits::MAX_ENTRY_NAME_BYTES + 1];
    try_name(&long).expect("a 1 024-byte name is allowed");
    try_name(b"audio/001.wav").expect("an ordinary name");
    try_name(b"dir/").expect("a directory entry");
    let forbidden: [(&str, &[u8]); 11] = [
        ("empty", b""),
        ("dot", b"."),
        ("dot dot", b"../x"),
        ("inside", b"a/../../b"),
        ("absolute", b"/abs"),
        ("drive", b"C:x"),
        ("backslash", b"a\\b"),
        ("NUL", b"x\0y"),
        ("empty part", b"a//b"),
        ("dot part", b"./x"),
        ("too long", &too_long),
    ];
    for (what, name) in forbidden {
        let why = refused(try_name(name));
        assert!(
            why.contains("an entry name leaves the package"),
            "{what}: {why}"
        );
    }
    // A duplicate is refused too (an exact repeat of a name).
    let entries = [
        RawEntry::stored("project.xml", a_project().as_bytes()),
        RawEntry::stored("project.xml", a_project().as_bytes()),
    ];
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("Dup.dawproject");
    std::fs::write(&path, raw_zip(&entries)).unwrap();
    assert!(refused(open_with(&path, &PackageLimits::default())).contains("two entries are named"));
    // The rule itself.
    assert_eq!(check_name("plugins/001.aupreset"), Ok(()));
    assert!(check_name("..").is_err());
}

#[test]
fn zip_links() {
    let with_mode = |mode: Option<u32>| {
        let entries = [
            RawEntry::stored("project.xml", a_project().as_bytes()),
            RawEntry {
                unix_mode: mode,
                ..RawEntry::stored("audio/a.wav", b"RIFF")
            },
        ];
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("Links.dawproject");
        std::fs::write(&path, raw_zip(&entries)).unwrap();
        open_with(&path, &PackageLimits::default())
    };
    with_mode(Some(0o100_644)).expect("a regular file");
    with_mode(None).expect("no mode");
    assert!(refused(with_mode(Some(0o120_777))).contains("it holds a link"));
}

#[test]
fn zip_encrypted() {
    let build = |entries: Vec<RawEntry>| {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("Enc.dawproject");
        std::fs::write(&path, raw_zip(&entries)).unwrap();
        (dir, path)
    };
    // An encrypted project.xml refuses the file (at read).
    let encrypted = |name: &str, data: &[u8]| RawEntry {
        flags: 1,
        ..RawEntry::stored(name, data)
    };
    let (_d, path) = build(vec![encrypted("project.xml", a_project().as_bytes())]);
    assert!(refused(open_with(&path, &PackageLimits::default())).contains("is encrypted"));
    // An encrypted state entry is left out and listed; the file opens.
    let xml = project(
        r#"<Track id="t" name="T" contentType="notes"><Channel id="c" role="regular"><Devices><ClapPlugin id="d" deviceID="org.example.s" deviceName="S" deviceRole="instrument"><State path="plugins/s.clap-preset"/></ClapPlugin></Devices></Channel></Track>"#,
        "",
    );
    let (_d, path) = build(vec![
        RawEntry::stored("project.xml", xml.as_bytes()),
        encrypted("plugins/s.clap-preset", b"state"),
    ]);
    let pkg = open_with(&path, &PackageLimits::default()).expect("opens");
    let imp = mapped(&pkg, &BTreeMap::new());
    has_line(&imp.report, ItemKind::LeftOut, PLUGIN_SETTINGS);
    // The same state, not encrypted, is kept.
    let (_d, path) = build(vec![
        RawEntry::stored("project.xml", xml.as_bytes()),
        RawEntry::stored("plugins/s.clap-preset", b"state"),
    ]);
    let pkg = open_with(&path, &PackageLimits::default()).unwrap();
    lacks_line(&mapped(&pkg, &BTreeMap::new()).report, PLUGIN_SETTINGS);
}

/// A project entry whose size is `200 × packed + extra`: valid XML, a
/// comment of pseudo-random text that does not pack well, and trailing
/// spaces that do. Returns the deflated entry and its ratio's numerator
/// and denominator.
fn ratio_entry(extra: i64) -> Option<(RawEntry, u64, u64)> {
    use std::io::Write;
    let mut x = 0x2545_F491_4F6C_DD1Du64;
    let noise: String = (0..200)
        .map(|_| {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            char::from(b'a' + (x % 26) as u8)
        })
        .collect();
    let head = format!("{}<!--{noise}-->", a_project());
    let pack = |pad: usize| {
        let mut plain = head.clone().into_bytes();
        plain.extend(std::iter::repeat_n(b' ', pad));
        let mut enc =
            flate2::write::DeflateEncoder::new(Vec::new(), flate2::Compression::default());
        enc.write_all(&plain).unwrap();
        (plain, enc.finish().unwrap())
    };
    // Search for the padding whose size is exactly 200 × its packed size
    // plus `extra`.
    let mut pad = 0usize;
    loop {
        let (plain, packed) = pack(pad);
        let gap = plain.len() as i64 - 200 * packed.len() as i64 - extra;
        if gap == 0 {
            let entry = RawEntry {
                name: b"project.xml".to_vec(),
                crc: Some(rawzip::crc32(&plain)),
                declared_size: u32::try_from(plain.len()).unwrap(),
                data: packed.clone(),
                method: 8,
                flags: 0,
                unix_mode: None,
                zip64_size: None,
            };
            return Some((entry, plain.len() as u64, packed.len() as u64));
        }
        if pad > 400_000 {
            return None;
        }
        // Far below: jump; near: step by one.
        pad += if gap < -2_000 { 997 } else { 1 };
    }
}

#[test]
fn zip_ratio() {
    let dir = tempfile::tempdir().unwrap();
    let try_extra = |extra: i64| {
        let (entry, size, packed) = ratio_entry(extra).expect("an entry of that ratio");
        let path = dir.path().join(format!("R{extra}.dawproject"));
        std::fs::write(&path, raw_zip(&[entry])).unwrap();
        (open_with(&path, &PackageLimits::default()), size, packed)
    };
    // Exactly 200 times its packed size: accepted.
    let (r, size, packed) = try_extra(0);
    assert_eq!(size, 200 * packed);
    r.expect("200:1 is accepted");
    // One byte more: refused for its ratio.
    let (r, size, packed) = try_extra(1);
    assert_eq!(size, 200 * packed + 1);
    assert!(refused(r).contains("unpacks to more than 200 times its size"));
}

#[test]
fn free_space_estimate() {
    // `needed_bytes` = every entry's declared size (its extracted copy)
    // plus one CAF per audio source, entries and regular external files
    // alike, at 2, 3 and 4 bytes per sample (L10); what will not be read
    // counts nothing.
    let dir = tempfile::tempdir().unwrap();
    let external = dir.path().join("outside.wav");
    std::fs::write(&external, vec![0u8; 100]).unwrap();
    let clip = |path: &str, ext: bool, channels: u32, secs: u32| {
        format!(
            r#"<Clip time="0" duration="1"><Audio sampleRate="48000" channels="{channels}" duration="{secs}"><File path="{path}"{}/></Audio></Clip>"#,
            if ext { r#" external="true""# } else { "" }
        )
    };
    let arrangement = format!(
        r#"<Lanes timeUnit="beats"><Clips track="a">{}{}{}</Clips></Lanes>"#,
        clip("audio/in.wav", false, 2, 10),
        clip(external.to_str().unwrap(), true, 1, 4),
        clip("audio/missing.wav", false, 2, 10),
    );
    let xml = project(
        r#"<Track id="a" name="A" contentType="audio"><Channel id="ac" role="regular" destination="mc"/></Track>"#,
        &arrangement,
    );
    let entry = vec![7u8; 1_000];
    let path = stored(
        dir.path(),
        "Space",
        &[("project.xml", xml.as_bytes()), ("audio/in.wav", &entry)],
    );
    let pkg = open_with(&path, &PackageLimits::default()).unwrap();
    assert_eq!(pkg.audio().len(), 3);
    for depth in [2u32, 3, 4] {
        let d = u64::from(depth);
        // The entry: 1 000 bytes extracted, then 480 000 frames × 2 × depth
        // (never less than four times its size); the external file: 192 000
        // frames × 1 × depth (at least 400); the missing entry: nothing.
        let caf_in = (480_000 * 2 * d).max(4_000);
        let caf_out = (192_000 * d).max(400);
        let want = 1_000 + caf_in + caf_out;
        assert_eq!(
            pkg.needed_bytes(depth, &AtomicBool::new(false)).unwrap(),
            want,
            "{depth} bytes per sample"
        );
    }
    // When the Audio element lies low, the file's size × 4 is the floor.
    let xml = project(
        r#"<Track id="a" name="A" contentType="audio"><Channel id="ac" role="regular" destination="mc"/></Track>"#,
        &format!(
            r#"<Lanes timeUnit="beats"><Clips track="a">{}</Clips></Lanes>"#,
            clip("audio/in.wav", false, 1, 0)
        ),
    );
    let path = stored(
        dir.path(),
        "Low",
        &[("project.xml", xml.as_bytes()), ("audio/in.wav", &entry)],
    );
    let pkg = open_with(&path, &PackageLimits::default()).unwrap();
    assert_eq!(
        pkg.needed_bytes(3, &AtomicBool::new(false)).unwrap(),
        1_000 + 4_000
    );
}

/// `a_project()` padded with trailing whitespace to exactly `n` bytes.
fn padded(n: usize) -> String {
    let mut s = a_project();
    assert!(s.len() <= n);
    s.push_str(&" ".repeat(n - s.len()));
    s
}

#[test]
fn xml_size() {
    // `max_project_xml` lowered: a project.xml of exactly that many bytes
    // opens; one more is refused as too large.
    let lowered = PackageLimits {
        max_project_xml: 1_024,
        ..PackageLimits::default()
    };
    open_xml(&padded(1_024), &lowered).expect("1 024 bytes");
    assert_eq!(refused(open_xml(&padded(1_025), &lowered)), TOO_LARGE);
    // The constants.
    assert_eq!(limits::MAX_PROJECT_XML_BYTES, 256 << 20);
}

#[test]
fn xml_metadata_size() {
    let dir = tempfile::tempdir().unwrap();
    let meta = |n: usize| {
        let mut m = String::from("<MetaData><Title>T</Title></MetaData>");
        m.push_str(&" ".repeat(n - m.len()));
        m
    };
    let lowered = PackageLimits {
        max_metadata_xml: 256,
        ..PackageLimits::default()
    };
    let path = stored(
        dir.path(),
        "M1",
        &[
            ("project.xml", a_project().as_bytes()),
            ("metadata.xml", meta(256).as_bytes()),
        ],
    );
    let pkg = open_with(&path, &lowered).unwrap();
    lacks_line(&pkg.report, SONG_INFO_UNREADABLE);
    assert_eq!(pkg.metadata.fields[0].as_deref(), Some("T"));
    let path = stored(
        dir.path(),
        "M2",
        &[
            ("project.xml", a_project().as_bytes()),
            ("metadata.xml", meta(257).as_bytes()),
        ],
    );
    let pkg = open_with(&path, &lowered).unwrap();
    has_line(&pkg.report, ItemKind::LeftOut, SONG_INFO_UNREADABLE);
    assert!(pkg.metadata.fields.iter().all(Option::is_none));
    assert_eq!(limits::MAX_METADATA_XML_BYTES, 1 << 20);
}

#[test]
fn record_size() {
    // The record's head is read from an entry no larger than `max_record`;
    // a larger one is noted and the file treated as having none.
    let dir = tempfile::tempdir().unwrap();
    let record = |n: usize| {
        let head = r#"{"format":1,"project_id":"p","export_id":"e","pad":""#;
        let mut s = head.to_owned();
        s.push_str(&"x".repeat(n - head.len() - 2));
        s.push_str("\"}");
        assert_eq!(s.len(), n);
        s
    };
    let lowered = PackageLimits {
        max_record: 512,
        ..PackageLimits::default()
    };
    let path = stored(
        dir.path(),
        "R1",
        &[
            ("project.xml", a_project().as_bytes()),
            ("vizdaw/record.json", record(512).as_bytes()),
        ],
    );
    let pkg = open_with(&path, &lowered).unwrap();
    lacks_line(&pkg.report, RECORD_UNREADABLE);
    assert!(pkg.ret.record.is_some());
    let path = stored(
        dir.path(),
        "R2",
        &[
            ("project.xml", a_project().as_bytes()),
            ("vizdaw/record.json", record(513).as_bytes()),
        ],
    );
    let pkg = open_with(&path, &lowered).unwrap();
    has_line(&pkg.report, ItemKind::Note, RECORD_UNREADABLE);
    assert!(pkg.ret.record.is_none());
    assert_eq!(limits::MAX_RECORD_BYTES, 256 << 20);
}

/// `n` note tracks, each with a CLAP instrument whose state is `plugins/<i>`.
fn states_project(n: usize) -> String {
    let tracks: String = (0..n)
        .map(|i| {
            format!(
                r#"<Track id="t{i}" name="T{i}" contentType="notes"><Channel id="c{i}" role="regular"><Devices><ClapPlugin id="d{i}" deviceID="org.example.s" deviceName="S{i}" deviceRole="instrument"><State path="plugins/{i}.clap-preset"/></ClapPlugin></Devices></Channel></Track>"#
            )
        })
        .collect();
    project(&tracks, "")
}

#[test]
fn state_size() {
    let dir = tempfile::tempdir().unwrap();
    let lowered = PackageLimits {
        max_state: 100,
        ..PackageLimits::default()
    };
    let xml = states_project(1);
    for (len, kept) in [(100usize, true), (101, false)] {
        let state = vec![9u8; len];
        let path = stored(
            dir.path(),
            &format!("S{len}"),
            &[
                ("project.xml", xml.as_bytes()),
                ("plugins/0.clap-preset", &state),
            ],
        );
        let pkg = open_with(&path, &lowered).unwrap();
        let imp = mapped(&pkg, &BTreeMap::new());
        if kept {
            lacks_line(&imp.report, PLUGIN_SETTINGS);
        } else {
            let line = has_line(&imp.report, ItemKind::LeftOut, PLUGIN_SETTINGS);
            // One state past `max_state` is "too large to open".
            assert!(
                line.details.iter().all(|d| d.ends_with(TOO_LARGE_TO_OPEN)),
                "{:?}",
                line.details
            );
        }
    }
    assert_eq!(limits::MAX_STATE_BYTES, 64 << 20);
}

#[test]
fn state_total() {
    // Three distinct 100-byte states against a total of 300: all kept; the
    // same against 299: the third is left out.
    let dir = tempfile::tempdir().unwrap();
    let xml = states_project(3);
    let states: Vec<Vec<u8>> = (0..3u8).map(|i| vec![i + 1; 100]).collect();
    let path = stored(
        dir.path(),
        "Total",
        &[
            ("project.xml", xml.as_bytes()),
            ("plugins/0.clap-preset", &states[0]),
            ("plugins/1.clap-preset", &states[1]),
            ("plugins/2.clap-preset", &states[2]),
        ],
    );
    for (total, left_out) in [(300u64, 0u64), (299, 1)] {
        let lowered = PackageLimits {
            max_state_total: total,
            ..PackageLimits::default()
        };
        let pkg = open_with(&path, &lowered).unwrap();
        let imp = mapped(&pkg, &BTreeMap::new());
        match left_out {
            0 => lacks_line(&imp.report, PLUGIN_SETTINGS),
            n => assert_eq!(
                has_line(&imp.report, ItemKind::LeftOut, PLUGIN_SETTINGS).count,
                n
            ),
        }
    }
    assert_eq!(limits::MAX_STATE_TOTAL_BYTES, 128 << 20);
}

#[test]
fn xml_doctype() {
    let with = |doctype: &str| format!("{doctype}{}", a_project());
    open_xml(&with(""), &PackageLimits::default()).expect("no DOCTYPE");
    for doctype in [
        "<!DOCTYPE Project>",
        "<!DOCTYPE Project [<!ENTITY a \"b\">]>",
        "<!DOCTYPE Project SYSTEM \"http://x/y.dtd\">",
    ] {
        let why = refused(open_xml(&with(doctype), &PackageLimits::default()));
        assert!(why.contains("declares a document type"), "{doctype}: {why}");
    }
    // An unknown entity is refused too (nothing is ever expanded).
    let xml = a_project().replace("name=\"T\"", "name=\"&b;\"");
    assert!(refused(open_xml(&xml, &PackageLimits::default())).contains("unknown entity"));
}

#[test]
fn xml_depth() {
    let nested = |n: usize| format!("{}{}", "<a>".repeat(n), "</a>".repeat(n));
    let parse = |xml: &str| {
        Tree::parse(
            xml.as_bytes(),
            &PackageLimits::default(),
            &AtomicBool::new(false),
        )
    };
    parse(&nested(limits::MAX_XML_DEPTH)).expect("256 deep");
    let Err(IoError::Refused(why)) = parse(&nested(limits::MAX_XML_DEPTH + 1)) else {
        panic!("257 deep is refused")
    };
    assert!(why.contains("nested too deeply"), "{why}");
    // Through a whole package too.
    // (Project and Arrangement are two of the levels.)
    let xml = project("", &nested(limits::MAX_XML_DEPTH - 2));
    open_xml(&xml, &PackageLimits::default()).expect("256 deep in a project");
    let xml = project("", &nested(limits::MAX_XML_DEPTH - 1));
    assert!(refused(open_xml(&xml, &PackageLimits::default())).contains("nested too deeply"));
    assert_eq!(limits::MAX_XML_DEPTH, 256);
}

#[test]
fn xml_nodes() {
    let lowered = PackageLimits {
        max_nodes: 50,
        ..PackageLimits::default()
    };
    let flat = |n: usize| format!("<r>{}</r>", "<a/>".repeat(n - 1));
    let parse = |xml: &str| Tree::parse(xml.as_bytes(), &lowered, &AtomicBool::new(false));
    parse(&flat(50)).expect("50 nodes");
    let Err(IoError::Refused(why)) = parse(&flat(51)) else {
        panic!("51 nodes are refused")
    };
    assert!(why.contains("too many parts"), "{why}");
    assert_eq!(limits::MAX_XML_NODES, 4_000_000);
}

#[test]
fn xml_tree_bytes() {
    // A ratio-legal (stored) project.xml of 4 000 000 nodes parses with the
    // text, the tree and one working copy in hand: under the text plus
    // 128 MiB plus 16 MiB (L7), whatever the node count. One node more is
    // refused as having too many parts.
    let dir = tempfile::tempdir().unwrap();
    let flat = |n: usize| {
        format!(
            "<Project version=\"1.0\">{}</Project>",
            "<a/>".repeat(n - 1)
        )
    };
    let text = flat(limits::MAX_XML_NODES);
    let path = stored(dir.path(), "Wide", &[("project.xml", text.as_bytes())]);
    let (r, seen) = measure(|| {
        let zip = ZipReader::open(&path, &PackageLimits::default())?;
        let entry = zip.find("project.xml").expect("entry");
        let bytes = zip.read(
            entry,
            limits::MAX_PROJECT_XML_BYTES,
            Some(limits::MAX_XML_RATIO),
            &AtomicBool::new(false),
        )?;
        Tree::parse(&bytes, &PackageLimits::default(), &AtomicBool::new(false)).map(|_| ())
    });
    r.expect("4 000 000 nodes parse");
    let bound = text.len() + 128 * MIB + 16 * MIB;
    println!(
        "4M nodes: peak {} MiB (bound {} MiB)",
        seen.peak / MIB,
        bound / MIB
    );
    assert!(
        seen.peak < text.len() + 144 * MIB,
        "peak {} MiB",
        seen.peak / MIB
    );
    drop(text);
    let text = flat(limits::MAX_XML_NODES + 1);
    let path = stored(dir.path(), "Wider", &[("project.xml", text.as_bytes())]);
    let (r, seen) = measure(|| open_with(&path, &PackageLimits::default()));
    assert!(refused(r).contains("too many parts"));
    assert!(
        seen.peak < text.len() + 144 * MIB,
        "peak {} MiB",
        seen.peak / MIB
    );
    assert_eq!(limits::MAX_XML_TREE_BYTES, 128 * MIB);
}

#[test]
fn the_text_counts_against_the_parse_budget() {
    // Final review: the text and the tree share one 256 MiB parse budget,
    // and the text's buffer is reserved once, so a read at the full text
    // size peaks at about 256 MiB. Comments fill the text: they carry
    // nothing and the tree does not hold them.
    let dir = tempfile::tempdir().unwrap();
    let full = limits::MAX_PROJECT_XML_BYTES as usize - MIB;
    let padded = |body: &str| {
        let head =
            format!("<Project version=\"1.0\"><Application name=\"t\" version=\"1\"/>{body}<!--");
        let tail = "--></Project>";
        let fill = full - head.len() - tail.len();
        let mut s = String::with_capacity(full);
        s.push_str(&head);
        s.extend(std::iter::repeat_n(' ', fill));
        s.push_str(tail);
        assert_eq!(s.len(), full);
        s
    };
    let bound = limits::MAX_PARSE_BYTES + 16 * MIB;
    // Mostly comment, a small tree: it reads.
    let text = padded("");
    let path = stored(dir.path(), "Quiet", &[("project.xml", text.as_bytes())]);
    drop(text);
    let (r, seen) = measure(|| open_with(&path, &PackageLimits::default()).map(|_| ()));
    println!(
        "{} MiB of comment: peak {} MiB",
        full / MIB,
        seen.peak / MIB
    );
    r.expect("a small tree in a large text reads");
    assert!(seen.peak < bound, "peak {} MiB", seen.peak / MIB);
    std::fs::remove_file(&path).unwrap();
    // A tree that fits on its own (4 000 000 nodes, as in `xml_tree_bytes`)
    // but not beside the text: refused, within the budget.
    let nodes = "<a/>".repeat(limits::MAX_XML_NODES - 8);
    let text = padded(&nodes);
    drop(nodes);
    let path = stored(dir.path(), "Loud", &[("project.xml", text.as_bytes())]);
    drop(text);
    let (r, seen) = measure(|| open_with(&path, &PackageLimits::default()));
    println!(
        "{} MiB with 4M nodes: peak {} MiB",
        full / MIB,
        seen.peak / MIB
    );
    assert!(refused(r).contains("too many parts"));
    assert!(seen.peak < bound, "peak {} MiB", seen.peak / MIB);
    assert_eq!(limits::MAX_PARSE_BYTES, 256 * MIB);
}

#[test]
fn xml_tree_bytes_while_growing() {
    // CDATA text is out of sight of the pass that sizes the tree, so its
    // arena and text table grow as the parse goes. A growing table holds
    // its old and new buffers at once, and both count: the tree never
    // passes its budget, accepted or refused.
    let budget = 8 * MIB;
    let lowered = PackageLimits {
        max_tree_bytes: budget,
        ..PackageLimits::default()
    };
    let cdata = |n: usize| {
        format!(
            "<Project>{}</Project>",
            format!("<t><![CDATA[{}]]></t>", "x".repeat(100)).repeat(n)
        )
    };
    let parse = |text: &str| {
        measure(|| Tree::parse(text.as_bytes(), &lowered, &AtomicBool::new(false)).map(|_| ()))
    };
    // About 2.5 MiB of tree, grown step by step: it fits.
    let text = cdata(20_000);
    let (r, seen) = parse(&text);
    r.expect("about 2.5 MiB of tree fits in 8 MiB");
    assert!(seen.peak < budget + MIB, "peak {} KiB", seen.peak / 1024);
    // Too much for the budget: refused, still within it.
    let text = cdata(200_000);
    let (r, seen) = parse(&text);
    assert!(
        matches!(&r, Err(IoError::Refused(s)) if s.contains("too many parts")),
        "{r:?}"
    );
    assert!(seen.peak < budget + MIB, "peak {} KiB", seen.peak / 1024);
}

#[test]
fn numbers() {
    // 64 characters are read; 65 are not, and neither are NaN, infinity or
    // what overflows to it.
    let digits = |n: usize| format!("0.{}", "1".repeat(n - 2));
    assert_eq!(digits(64).len(), 64);
    assert!(core_io::xml::double(&digits(64)).is_some());
    assert!(core_io::xml::double(&digits(65)).is_none());
    for bad in [
        "NaN", "nan", "inf", "-inf", "INF", "1e400", "-1e400", "0x10", "1,5", "",
    ] {
        assert!(core_io::xml::double(bad).is_none(), "{bad}");
    }
    assert!(core_io::xml::double("-0").is_some());
    assert!(core_io::xml::int(&format!("{:0>64}", 7)).is_some());
    assert!(core_io::xml::int(&format!("{:0>65}", 7)).is_none());
    // In a file: the marker's bad time is left out with its line.
    let marker = |time: &str| {
        project(
            "",
            &format!(r#"<Markers><Marker name="m" time="{time}"/></Markers>"#),
        )
    };
    let pkg = open_xml(&marker(&digits(64)), &PackageLimits::default()).unwrap();
    lacks_line(&pkg.report, UNREADABLE_VALUES);
    lacks_line(&pkg.report, UNREADABLE_PARTS);
    for bad in [digits(65), "NaN".to_owned(), "1e400".to_owned()] {
        let pkg = open_xml(&marker(&bad), &PackageLimits::default()).unwrap();
        // A marker with no readable time is a part that could not be read.
        let line = has_line(&pkg.report, ItemKind::LeftOut, UNREADABLE_PARTS);
        assert_eq!(line.details, vec!["Marker".to_owned()], "{bad}");
    }
    // An optional value is left out alone, and listed as a value.
    let volume = |v: &str| {
        project("", "").replace(
            r#"<Channel id="mc" role="master"/>"#,
            &format!(r#"<Channel id="mc" role="master"><Volume id="mcv" unit="linear" value="{v}"/></Channel>"#),
        )
    };
    let pkg = open_xml(&volume(&digits(64)), &PackageLimits::default()).unwrap();
    lacks_line(&pkg.report, UNREADABLE_VALUES);
    lacks_line(&pkg.report, UNREADABLE_PARTS);
    for bad in [digits(65), "NaN".to_owned(), "1e400".to_owned()] {
        let pkg = open_xml(&volume(&bad), &PackageLimits::default()).unwrap();
        assert!(
            pkg.report
                .items
                .iter()
                .any(|i| i.what == UNREADABLE_VALUES || i.what == UNREADABLE_PARTS),
            "{bad}: {:#?}",
            pkg.report.items
        );
    }
    assert_eq!(limits::MAX_NUMBER_CHARS, 64);
}

#[test]
fn idrefs_resolve() {
    let with_destination = |to: &str| {
        project(
            &format!(
                r#"<Track id="t" name="T" contentType="notes"><Channel id="c" role="regular" destination="{to}"><Sends><Send id="s" destination="{to}" type="post"><Volume id="sv" unit="linear" value="0.5"/></Send></Sends></Channel></Track>"#
            ),
            "",
        )
    };
    let fx = r#"<Track id="fx" name="FX" contentType="audio"><Channel id="fxc" role="effect" destination="mc"/></Track>"#;
    let with_fx = |xml: String| xml.replace("</Structure>", &format!("{fx}</Structure>"));
    // An ID that resolves to a channel is read as it is.
    let pkg = open_xml(&with_fx(with_destination("fxc")), &PackageLimits::default()).unwrap();
    lacks_line(&pkg.report, MISSING_REFERENCES);
    lacks_line(&pkg.report, UNREADABLE_PARTS);
    // One that points at nothing, or at a part of another kind (a track, a
    // parameter), is left out and listed; the file still opens and maps.
    for dangling in ["nowhere", "t", "sv"] {
        let pkg = open_xml(
            &with_fx(with_destination(dangling)),
            &PackageLimits::default(),
        )
        .unwrap_or_else(|e| panic!("{dangling}: {e:?}"));
        assert!(
            pkg.report.items.iter().any(|i| i.kind == ItemKind::LeftOut
                && (i.what == MISSING_REFERENCES || i.what == UNREADABLE_PARTS)),
            "{dangling}: {:#?}",
            pkg.report.items
        );
        let (report, _) = imported_report(&pkg);
        assert!(!report.items.is_empty(), "{dangling}");
    }
}

#[test]
fn reference_cycles() {
    // A loop of channel outputs, and one of sends, are cut (listed); a clip
    // that references a timeline inside itself stops at the nesting limit.
    let looped = project(
        r#"<Track id="a" name="A" contentType="audio"><Channel id="ac" role="effect" destination="bc"><Sends><Send id="as" destination="bc"/></Sends></Channel></Track>
           <Track id="b" name="B" contentType="audio"><Channel id="bc" role="effect" destination="ac"><Sends><Send id="bs" destination="ac"/></Sends></Channel></Track>"#,
        "",
    );
    let pkg = open_xml(&looped, &PackageLimits::default()).expect("opens");
    let (report, _) = imported_report(&pkg);
    has_line(&report, ItemKind::Changed, LOOP_OUTPUTS);
    // Clips that refer to each other in a loop are refused, each shape with
    // the same sentence, at once.
    const LOOP: &str = "This DAWproject file is damaged: clips refer to each other in a loop.";
    let track = r#"<Track id="n" name="N" contentType="notes"><Channel id="nc" role="regular" destination="mc"/></Track>"#;
    let on_n = |clips: &str, defs: &str| {
        project(
            track,
            &format!(
                r#"<Lanes timeUnit="beats"><Lanes track="n"><Clips>{clips}</Clips></Lanes><Lanes>{defs}</Lanes></Lanes>"#
            ),
        )
    };
    let use_of = |id: &str| format!(r#"<Clip time="0" duration="2" reference="{id}"/>"#);
    let loops = [
        // A clip inside c1 that uses c1.
        project(
            track,
            r#"<Lanes timeUnit="beats"><Lanes track="n"><Clips id="c1"><Clip time="0" duration="4"><Clips id="c2"><Clip time="0" duration="2" reference="c1"/></Clips></Clip></Clips></Lanes></Lanes>"#,
        ),
        // A timeline that uses itself.
        on_n(
            &use_of("s"),
            &format!(r#"<Clips id="s">{}</Clips>"#, use_of("s")),
        ),
        // Two timelines that use each other.
        on_n(
            &use_of("x"),
            &format!(
                r#"<Clips id="x">{}</Clips><Clips id="y">{}</Clips>"#,
                use_of("y"),
                use_of("x")
            ),
        ),
    ];
    for (i, xml) in loops.iter().enumerate() {
        let start = std::time::Instant::now();
        assert_eq!(
            refused(open_xml(xml, &PackageLimits::default())),
            LOOP,
            "loop {i}"
        );
        assert!(start.elapsed().as_secs() < 5, "loop {i}");
    }
    // A use of a timeline that is not in the file is left out with its
    // line; the rest imports.
    let pkg = open_xml(
        &on_n(
            &format!(
                r#"{}<Clip time="4" duration="1"><Notes><Note time="0" duration="1" channel="0" key="60" vel="0.5"/></Notes></Clip>"#,
                use_of("nowhere")
            ),
            "",
        ),
        &PackageLimits::default(),
    )
    .expect("opens");
    let line = has_line(&pkg.report, ItemKind::LeftOut, MISSING_REFERENCES);
    assert!(
        line.details.iter().any(|d| d.ends_with("'nowhere'")),
        "{:?}",
        line.details
    );
    let (report, doc) = imported_report(&pkg);
    lacks_line(&report, NESTED_TOO_DEEP);
    assert_eq!(
        doc.all_events()
            .filter(|(_, e)| matches!(e.body, core_model::event::EventBody::Note { .. }))
            .count(),
        1,
        "the note beside the missing use is imported"
    );
    // A chain without a loop, longer than the nesting limit: it opens, and
    // the import stops at the limit with its line.
    let hops = 40;
    let defs: String = (1..hops)
        .map(|k| format!(r#"<Clips id="h{k}">{}</Clips>"#, use_of(&format!("h{}", k + 1))))
        .chain(std::iter::once(format!(
            r#"<Notes id="h{hops}"><Note time="0" duration="1" channel="0" key="60" vel="0.5"/></Notes>"#
        )))
        .collect();
    let start = std::time::Instant::now();
    let pkg = open_xml(&on_n(&use_of("h1"), &defs), &PackageLimits::default()).expect("opens");
    lacks_line(&pkg.report, MISSING_REFERENCES);
    let (report, _) = imported_report(&pkg);
    has_line(&report, ItemKind::LeftOut, NESTED_TOO_DEEP);
    assert!(start.elapsed().as_secs() < 5);
}

/// A note track whose clip nests `depth` clips, the innermost holding a note.
fn nested_clips(depth: usize) -> String {
    let mut arrangement = String::from(r#"<Lanes timeUnit="beats"><Lanes track="n"><Clips>"#);
    for _ in 0..depth {
        arrangement.push_str(r#"<Clip time="0" duration="8"><Clips>"#);
    }
    arrangement.push_str(
        r#"<Clip time="0" duration="8"><Notes><Note time="0" duration="1" channel="0" key="60" vel="0.5"/></Notes></Clip>"#,
    );
    for _ in 0..depth {
        arrangement.push_str("</Clips></Clip>");
    }
    arrangement.push_str("</Clips></Lanes></Lanes>");
    project(
        r#"<Track id="n" name="N" contentType="notes"><Channel id="nc" role="regular" destination="mc"/></Track>"#,
        &arrangement,
    )
}

#[test]
fn nesting() {
    // 16 levels of timelines are read; the 17th is left out with its line.
    // The levels here are Lanes, Lanes, Clips, then one Clips for each
    // clip inside a clip, and the innermost clip's Notes: 12 nested clips
    // make 3 + 12 + 1 = 16.
    let notes = |doc: &core_model::document::Document| {
        doc.all_events()
            .filter(|(_, e)| matches!(e.body, core_model::event::EventBody::Note { .. }))
            .count()
    };
    assert_eq!(limits::MAX_TIMELINE_NESTING, 16);
    let pkg = open_xml(&nested_clips(12), &PackageLimits::default()).unwrap();
    lacks_line(&pkg.report, UNREADABLE_PARTS);
    let (report, doc) = imported_report(&pkg);
    lacks_line(&report, NESTED_TOO_DEEP);
    assert_eq!(notes(&doc), 1, "the note 16 levels down is imported");
    let pkg = open_xml(&nested_clips(13), &PackageLimits::default()).unwrap();
    let line = has_line(&pkg.report, ItemKind::LeftOut, UNREADABLE_PARTS);
    assert_eq!(line.details, vec!["Notes".to_owned()]);
    let (_, doc) = imported_report(&pkg);
    assert_eq!(notes(&doc), 0, "the note 17 levels down is left out");
    // Clips and references nested past the limit in the import's own walk
    // (the chain test in dawproject_timeline_in.rs) list NESTED_TOO_DEEP.
    assert!(NESTED_TOO_DEEP.contains("16"));
}

#[test]
fn positions() {
    // A marker at the last whole beat that fits a tick is placed; one beat
    // past it is left out and listed.
    let last = rules_max_beats();
    let marker_at = |beats: i64| {
        project(
            "",
            &format!(r#"<Markers><Marker name="m" time="{beats}"/></Markers>"#),
        )
    };
    let pkg = open_xml(&marker_at(last), &PackageLimits::default()).unwrap();
    let (report, doc) = imported_report(&pkg);
    lacks_line(&report, PAST_END);
    let markers: Vec<i64> = doc
        .sequences()
        .values()
        .flat_map(|s| s.markers.iter().map(|m| m.tick.0))
        .collect();
    assert_eq!(markers, vec![last * 15_360]);
    assert!(last * 15_360 <= core_model::rules::MAX_TICKS);
    let pkg = open_xml(&marker_at(last + 1), &PackageLimits::default()).unwrap();
    let (report, doc) = imported_report(&pkg);
    has_line(&report, ItemKind::LeftOut, PAST_END);
    assert_eq!(
        doc.sequences()
            .values()
            .map(|s| s.markers.len())
            .sum::<usize>(),
        0
    );
    // A clip placed there: its notes follow the same rule.
    let clip_at = |beats: i64| {
        project(
            r#"<Track id="n" name="N" contentType="notes"><Channel id="nc" role="regular" destination="mc"/></Track>"#,
            &format!(
                r#"<Lanes timeUnit="beats"><Lanes track="n"><Clips><Clip time="{beats}" duration="4"><Notes><Note time="0" duration="1" channel="0" key="60" vel="0.5"/></Notes></Clip></Clips></Lanes></Lanes>"#
            ),
        )
    };
    let notes_in = |doc: &core_model::document::Document| {
        doc.all_events()
            .filter(|(_, e)| matches!(e.body, core_model::event::EventBody::Note { .. }))
            .map(|(_, e)| e.tick.0)
            .collect::<Vec<_>>()
    };
    let pkg = open_xml(&clip_at(last), &PackageLimits::default()).unwrap();
    let (report, doc) = imported_report(&pkg);
    lacks_line(&report, PAST_END);
    assert_eq!(notes_in(&doc), vec![last * 15_360]);
    let pkg = open_xml(&clip_at(last + 1), &PackageLimits::default()).unwrap();
    let (report, doc) = imported_report(&pkg);
    has_line(&report, ItemKind::LeftOut, PAST_END);
    assert!(notes_in(&doc).is_empty());
}

/// The last whole beat whose tick is within `MAX_TICKS`.
fn rules_max_beats() -> i64 {
    core_model::rules::MAX_TICKS / 15_360
}

#[test]
fn external_media() {
    // A FIFO, a directory, a symlink loop and a URL are never read: the
    // package opens without looking at them, each stays an `AudioRef` for
    // the job, `needed_bytes` counts nothing for them, and the job makes them
    // offline.
    use std::os::unix::fs::symlink;
    let dir = tempfile::tempdir().unwrap();
    let fifo = dir.path().join("pipe.wav");
    assert!(
        std::process::Command::new("mkfifo")
            .arg(&fifo)
            .status()
            .expect("mkfifo")
            .success()
    );
    let folder = dir.path().join("folder.wav");
    std::fs::create_dir(&folder).unwrap();
    let (a, b) = (dir.path().join("loop-a.wav"), dir.path().join("loop-b.wav"));
    symlink(&b, &a).unwrap();
    symlink(&a, &b).unwrap();
    let url = "http://x/a.wav";
    let paths = [
        fifo.to_str().unwrap().to_owned(),
        folder.to_str().unwrap().to_owned(),
        a.to_str().unwrap().to_owned(),
        url.to_owned(),
    ];
    let clips: String = paths
        .iter()
        .map(|p| {
            format!(
                r#"<Clip time="0" duration="1"><Audio sampleRate="48000" channels="2" duration="1"><File path="{p}" external="true"/></Audio></Clip>"#
            )
        })
        .collect();
    let xml = project(
        r#"<Track id="a" name="A" contentType="audio"><Channel id="ac" role="regular" destination="mc"/></Track>"#,
        &format!(r#"<Lanes timeUnit="beats"><Clips track="a">{clips}</Clips></Lanes>"#),
    );
    let path = stored(dir.path(), "External", &[("project.xml", xml.as_bytes())]);
    let _ = debug_take_external_stats();
    let pkg = open_with(&path, &PackageLimits::default()).unwrap();
    assert_eq!(
        debug_take_external_stats(),
        0,
        "open looked at no external file"
    );
    assert_eq!(pkg.audio().len(), 4);
    assert!(
        pkg.audio()
            .iter()
            .all(|a| matches!(a.source, AudioSource::External(_)))
    );
    assert_eq!(pkg.info().audio_files, 4);
    // `needed_bytes` runs on a detached thread: a FIFO opened for reading
    // would block for ever, and this must fail instead of hanging (a scoped
    // thread would be joined, and the test would hang on it).
    let shared = std::sync::Arc::new(pkg);
    let (tx, rx) = std::sync::mpsc::channel();
    {
        let shared = std::sync::Arc::clone(&shared);
        std::thread::spawn(move || {
            let _ = tx.send(shared.needed_bytes(3, &AtomicBool::new(false)));
        });
    }
    let got = rx
        .recv_timeout(std::time::Duration::from_secs(10))
        .expect("needed_bytes returned");
    assert_eq!(got.unwrap(), 0, "none of them will be read");
    let pkg: &DpPackage = &shared;
    // The job makes them offline, and the import lists them.
    let media: BTreeMap<usize, MediaSource> = pkg
        .audio()
        .iter()
        .map(|a| {
            (
                a.key,
                MediaSource::Offline {
                    why: "it is not a regular audio file".into(),
                },
            )
        })
        .collect();
    let imp = mapped(pkg, &media);
    assert_eq!(has_line(&imp.report, ItemKind::Note, OFFLINE).count, 4);
}

#[test]
fn import_tracks_and_buses() {
    // Applying n tracks takes time quadratic in n, so a project holds at
    // most `MAX_IMPORT_TRACKS` note and audio tracks and `MAX_IMPORT_BUSES`
    // buses; more is refused when it is opened.
    let tracks = |n: usize, role: &str, content: &str| -> String {
        (0..n)
            .map(|i| {
                format!(
                    r#"<Track id="t{i}" contentType="{content}"><Channel id="c{i}" role="{role}" destination="mc"/></Track>"#
                )
            })
            .collect()
    };
    let lowered = PackageLimits {
        max_import_tracks: 20,
        max_import_buses: 5,
        ..PackageLimits::default()
    };
    open_xml(&project(&tracks(20, "regular", "notes"), ""), &lowered).expect("20 tracks");
    assert!(
        refused(open_xml(
            &project(&tracks(21, "regular", "notes"), ""),
            &lowered
        ))
        .contains("more than 20 tracks")
    );
    open_xml(&project(&tracks(5, "effect", "audio"), ""), &lowered).expect("5 buses");
    assert!(
        refused(open_xml(
            &project(&tracks(6, "effect", "audio"), ""),
            &lowered
        ))
        .contains("more than 5 buses")
    );
    // Folders are buses too.
    let folder = |n: usize| -> String {
        (0..n)
            .map(|i| format!(r#"<Track id="f{i}" contentType="tracks"/>"#))
            .collect()
    };
    open_xml(&project(&folder(5), ""), &lowered).expect("5 folders");
    assert!(refused(open_xml(&project(&folder(6), ""), &lowered)).contains("more than 5 buses"));
    // Every master after the first becomes a bus, so it counts as one: the
    // project's own master plus 5 more opens, plus 6 more is refused.
    let masters = |n: usize| -> String {
        (0..n)
            .map(|i| {
                format!(
                    r#"<Track id="x{i}" contentType="audio"><Channel id="xc{i}" role="master"/></Track>"#
                )
            })
            .collect()
    };
    let pkg = open_xml(&project(&masters(5), ""), &lowered).expect("5 extra masters");
    let (_, doc) = imported_report(&pkg);
    assert_eq!(bus_count(&doc), 5, "the count at open is what apply builds");
    assert!(refused(open_xml(&project(&masters(6), ""), &lowered)).contains("more than 5 buses"));
    // At the real cap: `MAX_IMPORT_BUSES + 1` extra master tracks are
    // refused.
    assert!(
        refused(open_xml(
            &project(&masters(limits::MAX_IMPORT_BUSES + 1), ""),
            &PackageLimits::default()
        ))
        .contains("more than 256 buses")
    );
    // A top-level master channel and a VCA channel are no buses: a legal
    // file with exactly `MAX_IMPORT_BUSES` buses besides them opens, and
    // builds exactly that many.
    let legal = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<Project version="1.0"><Application name="T" version="1"/><Structure><Channel id="mc" role="master"/><Channel id="vc" role="vca"/>{}</Structure><Arrangement/></Project>"#,
        tracks(limits::MAX_IMPORT_BUSES, "effect", "audio")
    );
    let pkg = open_xml(&legal, &PackageLimits::default()).expect("256 buses and a master channel");
    let (_, doc) = imported_report(&pkg);
    assert_eq!(bus_count(&doc), limits::MAX_IMPORT_BUSES);
    // A top-level channel that is neither is a bus, counted with the rest.
    let one_more = legal.replace(
        r#"<Channel id="vc" role="vca"/>"#,
        r#"<Channel id="vc" role="effect"/>"#,
    );
    assert!(
        refused(open_xml(&one_more, &PackageLimits::default())).contains("more than 256 buses")
    );
    assert_eq!(limits::MAX_IMPORT_TRACKS, 1_000);
    assert_eq!(limits::MAX_IMPORT_BUSES, 256);
}

/// How many buses `doc`'s mixer holds.
fn bus_count(doc: &core_model::document::Document) -> usize {
    doc.mixer()
        .strips()
        .values()
        .filter(|s| s.kind == core_model::mixer::StripKind::Bus)
        .count()
}

#[test]
fn defaults_are_the_spec_values() {
    // Ruling 18, constant by constant.
    assert_eq!(limits::MAX_ZIP_ENTRIES, 10_000);
    assert_eq!(limits::MAX_ENTRY_NAME_BYTES, 1_024);
    assert_eq!(limits::MAX_XML_RATIO, 200);
    assert_eq!(limits::MAX_PROJECT_XML_BYTES, 256 * 1024 * 1024);
    assert_eq!(limits::MAX_METADATA_XML_BYTES, 1024 * 1024);
    assert_eq!(limits::MAX_RECORD_BYTES, 256 * 1024 * 1024);
    assert_eq!(limits::MAX_STATE_BYTES, 64 * 1024 * 1024);
    assert_eq!(limits::MAX_STATE_TOTAL_BYTES, 128 * 1024 * 1024);
    assert_eq!(limits::MAX_XML_DEPTH, 256);
    assert_eq!(limits::MAX_XML_NODES, 4_000_000);
    assert_eq!(limits::MAX_XML_TREE_BYTES, 128 * 1024 * 1024);
    assert_eq!(limits::MAX_MODEL_BYTES, 192 * 1024 * 1024);
    assert_eq!(limits::MAX_XML_ATTRS, 64);
    assert_eq!(limits::MAX_TEXT_BYTES, 64 * 1024);
    assert_eq!(limits::MAX_NUMBER_CHARS, 64);
    assert_eq!(limits::MAX_TIMELINE_NESTING, 16);
    let d = PackageLimits::default();
    assert_eq!(
        (
            d.max_entries,
            d.max_project_xml,
            d.max_metadata_xml,
            d.max_record,
            d.max_state,
            d.max_state_total,
            d.max_ratio,
            d.max_depth,
            d.max_nodes,
            d.max_tree_bytes,
            d.max_import_tracks,
            d.max_import_buses,
        ),
        (
            10_000,
            256 << 20,
            1 << 20,
            256 << 20,
            64 << 20,
            128 << 20,
            200,
            256,
            4_000_000,
            128 << 20,
            1_000,
            256,
        )
    );
}
