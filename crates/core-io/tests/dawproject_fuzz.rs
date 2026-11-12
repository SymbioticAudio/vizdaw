//! Hostile DAWproject files (plan 6d Task 8): damaged packages, hostile
//! ZIPs and hostile XML each end in `Ok` or `Refused`, never a panic and
//! never an `Io` error (the faults are in the bytes, not the disk), within
//! 2 s and 256 MiB on the case's own thread.

mod common;

use common::package::{TestPlist, package_bytes};
use common::rawzip_builder::{RawEntry, raw_zip, raw_zip_claiming};
use common::{CountingAlloc, cases, measure};
use core_engine::fixtures::{Builder, Q};
use core_io::IoError;
use core_io::dawproject::export::{ClipRule, ExportEnv, ExportOptions, RegionGain, export};
use core_io::dawproject::import::{ImportEnv, MediaSource, apply, map};
use core_io::dawproject::package::open;
use core_io::limits::PackageLimits;
use core_io::report::Report;
use core_io::zip::{EntryMethod, ZipWriter};
use core_model::command::{Command, MediaCmd};
use core_model::document::MediaOrigin;
use core_model::event::Fade;
use core_model::history::History;
use core_model::ids::AnyId;
use core_model::mixer::PluginFormat;
use core_model::time::Tick;
use proptest::prelude::*;
use std::collections::BTreeMap;
use std::path::Path;
use std::sync::OnceLock;
use std::sync::atomic::AtomicBool;
use std::time::{Duration, Instant};

#[global_allocator]
static ALLOC: CountingAlloc = CountingAlloc;

const DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/data/dawproject");
const MIB: usize = 1024 * 1024;

fn fixture(name: &str) -> Vec<u8> {
    std::fs::read(format!("{DIR}/{name}"))
        .unwrap_or_else(|e| panic!("hand-written fixture {name} is missing: {e}"))
}

/// A package of `entries`, XML and JSON deflated.
fn zip(entries: &[(&str, &[u8])]) -> Vec<u8> {
    let mut z = ZipWriter::new(Vec::new());
    for (name, bytes) in entries {
        let method = if name.ends_with(".xml") || name.ends_with(".json") {
            EntryMethod::Deflated
        } else {
            EntryMethod::Stored
        };
        z.add(name, method, bytes).unwrap();
    }
    z.finish().unwrap()
}

/// The valid packages the damage is done to: the three hand-written
/// fixtures and an export's output.
fn packages() -> &'static [Vec<u8>] {
    static PACKAGES: OnceLock<Vec<Vec<u8>>> = OnceLock::new();
    PACKAGES.get_or_init(|| {
        let minimal = zip(&[
            ("project.xml", &fixture("hand_minimal.project.xml")),
            ("metadata.xml", &fixture("hand_minimal.metadata.xml")),
            ("plugins/001.clap-preset", b"state bytes"),
        ]);
        let other = zip(&[
            ("project.xml", &fixture("hand_other.project.xml")),
            ("metadata.xml", &fixture("hand_other.metadata.xml")),
            ("plugins/d.aupreset", b"<plist>delay</plist>"),
            ("plugins/l.clap-preset", b"lead clap state"),
        ]);
        let timeline = zip(&[("project.xml", &fixture("hand_timeline.project.xml"))]);
        vec![minimal, other, timeline, exported()]
    })
}

/// One `export`'s package: notes, a region, a crossfade, state, a send.
fn exported() -> Vec<u8> {
    use core_model::command::{AudioCmd, CrossfadeLength};
    use core_model::event::FadeShape;
    use core_model::mixer::PluginRef;
    let mut b = Builder::new();
    let (dest, slot) = b
        .plugin_dest(
            "Keys",
            PluginRef {
                format: PluginFormat::Clap,
                id: "org.example.keys".into(),
                name: "Keys".into(),
                vendor: "Example".into(),
                version: "1".into(),
            },
        )
        .unwrap();
    let (seq, lane) = b.sequence("Song", "Keys", Some(dest)).unwrap();
    b.notes(seq, lane, &[(0, 60, 0x8000, Q), (Q, 64, 0x9000, Q)])
        .unwrap();
    let audio = b.lane(seq, "Audio", None).unwrap();
    let r = b
        .run(Command::Media(MediaCmd::Add {
            path: "Media/a.caf".into(),
            sample_rate: 48_000,
            channels: 2,
            length_frames: 96_000,
            content_hash: Some("h".into()),
            name: Some("a".into()),
            origin: Some(MediaOrigin::Imported {
                original_name: "a.wav".into(),
            }),
        }))
        .unwrap();
    let media = r
        .created
        .iter()
        .find_map(|i| match i {
            AnyId::Media(m) => Some(*m),
            _ => None,
        })
        .unwrap();
    b.region(
        seq,
        audio,
        0,
        media,
        0,
        96_000,
        0.0,
        Fade::default(),
        Fade::default(),
    )
    .unwrap();
    let second = b
        .region(
            seq,
            audio,
            Q,
            media,
            0,
            96_000,
            -3.0,
            Fade::default(),
            Fade::default(),
        )
        .unwrap();
    b.run(Command::Audio(AudioCmd::Crossfade {
        event: second,
        length: CrossfadeLength::Frames(480),
        shape: FadeShape::Linear,
    }))
    .unwrap();
    let (doc, _) = core_model::state::capture(b.doc(), slot, b"keys state").unwrap();
    let x = export(
        &doc,
        &ExportOptions {
            seq,
            region_gain: RegionGain::Apply,
            clips: ClipRule::SilenceGap(Tick(4 * Q)),
            left_out: Default::default(),
        },
        &ExportEnv {
            plist: &TestPlist,
            app_version: "1",
            project_id: "00000000-0000-4000-8000-000000000001",
            export_id: "E1",
            exported_at: 1,
        },
        &AtomicBool::new(false),
    )
    .unwrap();
    package_bytes(&x).unwrap()
}

// ---------------------------------------------------------------- one case

/// Opens, maps and applies the package at `path`: every error is a
/// refusal, and a file that opens must also map and apply.
fn attempt(path: &Path) -> Result<(), IoError> {
    let never = AtomicBool::new(false);
    let pkg = open(path, &PackageLimits::default(), &never)?;
    let media: BTreeMap<usize, MediaSource> = pkg
        .audio()
        .iter()
        .map(|a| {
            (
                a.key,
                MediaSource::Offline {
                    why: "not read in this test".into(),
                },
            )
        })
        .collect();
    let installed = |_: PluginFormat, _: &str| true;
    let imp = map(
        &pkg,
        &media,
        &ImportEnv {
            installed: &installed,
            plist: &TestPlist,
            rate: 48_000,
        },
        &never,
    )?;
    let mut h = History::new(core_model::document::Document::new());
    let r = h
        .execute(
            &Command::Sequence(core_model::command::SequenceCmd::New { name: "S".into() }),
            &core_model::selection::ViewFilter::default(),
        )
        .unwrap();
    let seq = r
        .created
        .iter()
        .find_map(|id| match id {
            AnyId::Seq(s) => Some(*s),
            _ => None,
        })
        .unwrap();
    let lane = h.doc().sequence(seq).unwrap().lanes.front().unwrap().id;
    h.execute(
        &Command::Lane(core_model::command::LaneCmd::Remove { seq, lane }),
        &core_model::selection::ViewFilter::default(),
    )
    .unwrap();
    h.begin("Import DAWproject").unwrap();
    let applied = apply(&mut h, &imp, seq, &mut Report::default());
    match applied {
        Ok(_) => h
            .commit()
            .map_err(|e| IoError::Io(format!("commit: {e}")))?,
        Err(e) => return Err(IoError::Io(format!("a mapped file did not apply: {e}"))),
    }
    Ok(())
}

/// Runs one case on `bytes`: a refusal or success, in bounded time and
/// memory on this thread.
#[track_caller]
fn run_case(bytes: &[u8]) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("Hostile.dawproject");
    std::fs::write(&path, bytes).unwrap();
    let start = Instant::now();
    let (result, seen) = measure(|| attempt(&path));
    let took = start.elapsed();
    match result {
        Ok(()) | Err(IoError::Refused(_)) => {}
        Err(e) => panic!("{e:?} ({} bytes)", bytes.len()),
    }
    assert!(
        seen.peak < 256 * MIB,
        "peak {} MiB for {} bytes",
        seen.peak / MIB,
        bytes.len()
    );
    assert!(took < Duration::from_secs(2), "{took:?}");
}

// ---------------------------------------------------------------- damaged packages

#[derive(Debug, Clone)]
enum Damage {
    Flip(prop::sample::Index, u8),
    Insert(prop::sample::Index, Vec<u8>),
    Delete(prop::sample::Index, usize),
    Truncate(prop::sample::Index),
}

fn damage() -> impl Strategy<Value = Damage> {
    prop_oneof![
        (any::<prop::sample::Index>(), 1u8..=255).prop_map(|(i, x)| Damage::Flip(i, x)),
        (
            any::<prop::sample::Index>(),
            proptest::collection::vec(any::<u8>(), 1..16)
        )
            .prop_map(|(i, b)| Damage::Insert(i, b)),
        (any::<prop::sample::Index>(), 1usize..64).prop_map(|(i, n)| Damage::Delete(i, n)),
        any::<prop::sample::Index>().prop_map(Damage::Truncate),
    ]
}

fn damaged(mut bytes: Vec<u8>, d: &Damage) -> Vec<u8> {
    // An earlier truncation can leave nothing to flip or delete.
    if bytes.is_empty() && matches!(d, Damage::Flip(..) | Damage::Delete(..)) {
        return bytes;
    }
    match d {
        Damage::Flip(i, x) => {
            let at = i.index(bytes.len());
            bytes[at] ^= x;
        }
        Damage::Insert(i, more) => {
            let at = i.index(bytes.len() + 1);
            bytes.splice(at..at, more.iter().copied());
        }
        Damage::Delete(i, n) => {
            let at = i.index(bytes.len());
            let end = (at + n).min(bytes.len());
            bytes.drain(at..end);
        }
        Damage::Truncate(i) => bytes.truncate(i.index(bytes.len() + 1)),
    }
    bytes
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(cases(256)))]

    #[test]
    fn damaged_packages_never_panic(
        which in 0usize..4,
        damages in proptest::collection::vec(damage(), 1..4),
    ) {
        let mut bytes = packages()[which].clone();
        for d in &damages {
            bytes = damaged(bytes, d);
        }
        run_case(&bytes);
    }
}

#[test]
fn the_undamaged_packages_import() {
    for (i, p) in packages().iter().enumerate() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("Fine.dawproject");
        std::fs::write(&path, p).unwrap();
        attempt(&path).unwrap_or_else(|e| panic!("package {i}: {e:?}"));
    }
}

// ---------------------------------------------------------------- hostile zips

const SMALL: &str = r#"<Project version="1.0"><Application name="t" version="1"/><Structure><Track id="m" name="Master" contentType="audio notes"><Channel id="mc" role="master"/></Track></Structure></Project>"#;

/// 50 MiB of zeros as a deflated `project.xml`, made once.
fn bomb() -> &'static RawEntry {
    static BOMB: OnceLock<RawEntry> = OnceLock::new();
    BOMB.get_or_init(|| RawEntry::deflated("project.xml", &vec![0u8; 50 * MIB]))
}

fn hostile_entry() -> impl Strategy<Value = RawEntry> {
    let names = prop_oneof![
        Just(b"project.xml".to_vec()),
        Just(b"metadata.xml".to_vec()),
        Just(b"../x".to_vec()),
        Just(b"/abs".to_vec()),
        Just(b"a/../b".to_vec()),
        Just(b"C:x".to_vec()),
        Just(b"a\\b".to_vec()),
        Just(b"x\0y".to_vec()),
        Just(b"dir/".to_vec()),
        Just(b"audio/001.wav".to_vec()),
        Just(b"vizdaw/record.json".to_vec()),
        Just(b"plugins/001.clap-preset".to_vec()),
        proptest::collection::vec(any::<u8>(), 0..40),
        Just(vec![b'n'; 1025]),
    ];
    (
        names,
        proptest::collection::vec(any::<u8>(), 0..64),
        prop_oneof![Just(0u16), Just(8), Just(12), Just(99)],
        prop_oneof![Just(0u16), Just(1), Just(8), Just(0x800), Just(0xFFFF)],
        prop_oneof![Just(0u32), Just(u32::MAX), Just(64), Just(1 << 20)],
        proptest::option::of(any::<u32>()),
        proptest::option::of(prop_oneof![
            Just(0u64),
            Just(1 << 63),
            Just(u64::MAX),
            Just(1 << 40)
        ]),
        any::<bool>(),
    )
        .prop_map(
            |(name, data, method, flags, declared_size, crc, zip64_size, link)| RawEntry {
                name,
                data,
                method,
                flags,
                declared_size,
                crc,
                unix_mode: link.then_some(0o120_777),
                zip64_size,
            },
        )
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(cases(256)))]

    #[test]
    fn hostile_zips_never_panic(
        honest in any::<bool>(),
        bomb_first in 0u8..8,
        entries in proptest::collection::vec(hostile_entry(), 0..5),
        duplicate in any::<bool>(),
        claimed in proptest::option::of(prop_oneof![Just(0u16), Just(65_535), Just(10_001)]),
    ) {
        let mut all: Vec<RawEntry> = Vec::new();
        if bomb_first == 0 {
            all.push(bomb().clone());
        } else if honest {
            all.push(RawEntry::deflated("project.xml", SMALL.as_bytes()));
        }
        all.extend(entries);
        if duplicate && let Some(first) = all.first().cloned() {
            all.push(first);
        }
        let bytes = match claimed {
            Some(n) => raw_zip_claiming(&all, n),
            None => raw_zip(&all),
        };
        run_case(&bytes);
    }
}

#[test]
fn a_bomb_is_refused_for_its_ratio() {
    let bytes = raw_zip(&[bomb().clone()]);
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("Bomb.dawproject");
    std::fs::write(&path, bytes).unwrap();
    let (r, seen) = measure(|| attempt(&path));
    let Err(IoError::Refused(why)) = r else {
        panic!("{r:?}")
    };
    assert!(why.contains("200 times"), "{why}");
    assert!(seen.peak < 64 * MIB, "peak {} MiB", seen.peak / MIB);
}

// ---------------------------------------------------------------- hostile XML

#[derive(Debug, Clone)]
struct Hostile {
    doctype: bool,
    nesting: usize,
    repeats: usize,
    number: &'static str,
    dangling: bool,
    cycle: bool,
    unknown: bool,
    words: usize,
    prolog: bool,
    /// Extra `role="master"` tracks after the first (each a bus to the
    /// import).
    masters: usize,
}

const NUMBERS: [&str; 7] = [
    "1e400",
    "NaN",
    "-0",
    "inf",
    "-inf",
    "123456789012345678901234567890123456789012345678901234567890123456789",
    "0.5",
];

fn hostile() -> impl Strategy<Value = Hostile> {
    (
        any::<bool>(),
        prop_oneof![5 => Just(0usize), 2 => 1usize..300, 1 => Just(256usize), 1 => Just(257usize)],
        prop_oneof![8 => Just(0usize), 2 => 1usize..200, 1 => Just(100_000usize)],
        proptest::sample::select(NUMBERS.to_vec()),
        any::<bool>(),
        any::<bool>(),
        any::<bool>(),
        prop_oneof![3 => Just(0usize), 1 => Just(1000usize)],
        any::<bool>(),
        prop_oneof![6 => Just(0usize), 1 => 1usize..300, 1 => Just(1_000usize)],
    )
        .prop_map(
            |(
                doctype,
                nesting,
                repeats,
                number,
                dangling,
                cycle,
                unknown,
                words,
                prolog,
                masters,
            )| {
                Hostile {
                    doctype,
                    nesting,
                    repeats,
                    number,
                    dangling,
                    cycle,
                    unknown,
                    words,
                    prolog,
                    masters,
                }
            },
        )
}

/// A `project.xml` that puts each hostile thing in a place the reader
/// looks at.
fn xml_of(h: &Hostile) -> String {
    use std::fmt::Write as _;
    let n = h.number;
    let mut s = String::new();
    if h.prolog {
        s.push_str("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
    }
    if h.doctype {
        s.push_str(
            "<!DOCTYPE Project [<!ENTITY a \"aaaaaaaaaa\"><!ENTITY b \"&a;&a;&a;&a;&a;&a;&a;&a;\">]>\n",
        );
    }
    let entity = if h.doctype { "&b;" } else { "&amp;" };
    let content_type = if h.words > 0 {
        "notes audio ".repeat(h.words / 2)
    } else {
        "notes".to_owned()
    };
    let _ = write!(
        s,
        "<Project version=\"1.0\"><Application name=\"t\" version=\"1\"/>\
         <Transport><Tempo id=\"tempo\" unit=\"bpm\" value=\"{n}\"/>\
         <TimeSignature id=\"timesig\" numerator=\"4\" denominator=\"4\"/></Transport><Structure>\
         <Track id=\"m\" name=\"Master\" contentType=\"audio notes\"><Channel id=\"mc\" role=\"master\">\
         <Volume id=\"mcv\" unit=\"linear\" value=\"{n}\"/></Channel></Track>\
         <Track id=\"t1\" name=\"One {entity}\" contentType=\"{content_type}\">\
         <Channel id=\"c1\" role=\"regular\" destination=\"{}\"><Pan id=\"c1p\" value=\"{n}\"/>\
         <Sends><Send id=\"s1\" destination=\"{}\"><Volume id=\"s1v\" value=\"{n}\"/></Send></Sends></Channel></Track>\
         <Track id=\"t2\" name=\"Two\" contentType=\"notes\">\
         <Channel id=\"c2\" role=\"effect\" destination=\"{}\"/></Track>",
        if h.dangling { "nowhere" } else { "mc" },
        if h.cycle { "c2" } else { "mc" },
        if h.cycle { "c1" } else { "mc" },
    );
    if h.unknown {
        s.push_str("<Frobnicate a=\"b\"><Quux/></Frobnicate>");
    }
    for i in 0..h.repeats {
        let _ = write!(s, "<Track id=\"r{i}\" name=\"r\" contentType=\"notes\"/>");
    }
    for i in 0..h.masters {
        let _ = write!(
            s,
            "<Track id=\"x{i}\" contentType=\"audio\"><Channel id=\"xc{i}\" role=\"master\"/></Track>"
        );
    }
    s.push_str("</Structure><Arrangement><Lanes timeUnit=\"beats\"><Lanes track=\"t1\"><Clips>");
    for _ in 0..h.nesting {
        let _ = write!(s, "<Clip time=\"{n}\" duration=\"4\"><Clips>");
    }
    let _ = write!(
        s,
        "<Clip time=\"{n}\" duration=\"{n}\" playStart=\"{n}\"><Notes>\
         <Note time=\"{n}\" duration=\"{n}\" channel=\"0\" key=\"60\" vel=\"{n}\"/>\
         <Note time=\"1\" duration=\"1\" channel=\"99\" key=\"200\"/></Notes></Clip>"
    );
    for _ in 0..h.nesting {
        s.push_str("</Clips></Clip>");
    }
    let _ = write!(
        s,
        "</Clips></Lanes></Lanes><Markers><Marker name=\"m\" time=\"{n}\"/></Markers></Arrangement></Project>"
    );
    s
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(cases(256)))]

    #[test]
    fn hostile_xml_never_panics(h in hostile()) {
        let xml = xml_of(&h);
        let bytes = zip(&[("project.xml", xml.as_bytes())]);
        run_case(&bytes);
    }
}

#[test]
fn hostile_xml_has_the_shapes_it_names() {
    // A DOCTYPE is refused; a depth of 257 is; the same file without them
    // opens and has its bad items left out.
    let base = Hostile {
        doctype: false,
        nesting: 0,
        repeats: 0,
        number: "0.5",
        dangling: false,
        cycle: false,
        unknown: true,
        words: 0,
        prolog: true,
        masters: 0,
    };
    let try_it = |h: &Hostile| {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("X.dawproject");
        std::fs::write(&path, zip(&[("project.xml", xml_of(h).as_bytes())])).unwrap();
        attempt(&path)
    };
    try_it(&base).expect("the plain file imports");
    assert!(matches!(
        try_it(&Hostile {
            doctype: true,
            ..base.clone()
        }),
        Err(IoError::Refused(_))
    ));
    assert!(matches!(
        try_it(&Hostile {
            nesting: 257,
            ..base.clone()
        }),
        Err(IoError::Refused(_))
    ));
    // Repeated master tracks are buses: past the bus cap they are refused.
    match try_it(&Hostile {
        masters: core_io::limits::MAX_IMPORT_BUSES + 1,
        ..base.clone()
    }) {
        Err(IoError::Refused(s)) => assert!(s.contains("more than 256 buses"), "{s}"),
        other => panic!("{other:?}"),
    }
    try_it(&Hostile {
        masters: core_io::limits::MAX_IMPORT_BUSES - 1,
        ..base.clone()
    })
    .expect("255 extra masters and one effect channel are 256 buses");
    assert!(matches!(
        try_it(&Hostile {
            dangling: true,
            ..base.clone()
        }),
        Ok(()) | Err(IoError::Refused(_))
    ));
}
