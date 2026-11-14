//! Hostile AAF files (plan 6f Task 7, spec §6.5, Ruling 13). Each case is
//! written to a temporary file and goes through `AafFile::open`, `info`,
//! `needed_bytes`, the extraction of its embedded audio, `pairs` and
//! `map`. It passes when that ends in `Refused` or `Cancelled`, or in a
//! mapping with its report; it fails on any panic outside `cfb` (a counting
//! panic hook sees caught ones too), on an `Io` error (the faults are in
//! the bytes, not the disk), or past 2 s and 1 s per 32 MiB of file, or
//! past the AAF read budget (768 MiB) and 64 MiB, on its own thread.
//!
//! - `byte_damage`: flips, inserts, deletes and truncations of the goldens:
//!   Task 4's writer output for four option sets and Task 2's other
//!   shapes.
//! - `structure_soup`: compositions built through `Store` with hostile
//!   values, chains, loops, depths and counts, written by a raw compound
//!   file writer that skips `AafWriter`'s checks, then with their streams
//!   and sectors edited.
//!
//! A panic inside `cfb` is upstream's bug: it must give exactly "This AAF
//! file is damaged." Each run prints the count and the places, for an
//! upstream issue. `VIZDAW_FUZZ_CASES` sets the cases per test (default
//! 256; the task's gate runs 4096 in release).

mod common;

use common::aaf::{
    ClipSpec, FileSpec, Fx, PICTURE, Seg, SlotKind, SlotSpec, Soup, clip, gain, sound, soup,
    soup_bytes, trans, write_export,
};
use common::generators::{audio_track, media};
use common::{CountingAlloc, cases, measure};
use core_engine::fixtures::{Builder, Q};
use core_io::IoError;
use core_io::aaf::dict::class;
use core_io::aaf::export::{AafOptions, ExportEnv, Fps, Stereo, export};
use core_io::aaf::import::{AafEnv, map, pairs};
use core_io::aaf::store::DAMAGED;
use core_io::aaf::timeline::AafFile;
use core_io::dawproject::export::LeftOutAudio;
use core_io::dawproject::import::MediaSource;
use core_io::dawproject::package::{AudioPackage, AudioSource};
use core_io::limits::AafLimits;
use core_media::import::Imported;
use core_model::automation::AutomationTarget;
use core_model::command::{AudioCmd, Command, CrossfadeLength, SequenceCmd};
use core_model::event::{EventBody, EventSpec, Fade, FadeShape};
use core_model::format::BitDepth;
use core_model::tempo::{Bpm, TempoMap};
use core_model::time::Tick;
use proptest::prelude::*;
use proptest::test_runner::{Config, TestRunner};
use std::cell::{Cell, RefCell};
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::sync::atomic::AtomicBool;
use std::sync::{Mutex, Once, OnceLock};
use std::time::{Duration, Instant};

#[global_allocator]
static ALLOC: CountingAlloc = CountingAlloc;

const MIB: usize = 1024 * 1024;
/// A case's time: 2 s, and 1 s more per 32 MiB of file. Reading is linear
/// in the file, and the 768 MiB budget admits files of 600 000 objects,
/// which take about 3 s (release) on the machine this was written on.
fn time_for(bytes: usize) -> Duration {
    Duration::from_secs(2) + Duration::from_secs_f64(bytes as f64 / (32 * MIB) as f64)
}

/// A case's peak allocation: the AAF read budget, and room for the
/// mapping and what the budget does not count.
const PEAK: usize = core_io::limits::AAF_READ_BUDGET + 64 * MIB;
const R48: (i32, i32) = (48_000, 1);

// ---------------------------------------------------------------- panics

thread_local! {
    /// Panics inside `cfb` on this thread since the case began.
    static CFB_PANICS: Cell<usize> = const { Cell::new(0) };
    /// Every other panic on this thread since the case began.
    static OTHER_PANICS: Cell<usize> = const { Cell::new(0) };
    /// Where this thread's `cfb` panics were, with their messages.
    static CFB_SITES: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
}

/// True for a panic raised in `cfb`'s own source.
fn in_cfb(file: &str) -> bool {
    file.contains("/cfb-") || file.contains("\\cfb-")
}

/// Installs, once, a hook that counts every panic on its thread (caught or
/// not): `cfb`'s and the read's test hook's silently, the rest also
/// through the default hook.
fn count_panics() {
    static HOOK: Once = Once::new();
    HOOK.call_once(|| {
        let default = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            let file = info.location().map_or("", |l| l.file());
            if in_cfb(file) {
                let _ = CFB_PANICS.try_with(|c| c.set(c.get() + 1));
                let at = info
                    .location()
                    .map_or_else(String::new, |l| format!("{}:{}", l.file(), l.line()));
                let msg = info
                    .payload()
                    .downcast_ref::<&str>()
                    .map(|s| (*s).to_string())
                    .or_else(|| info.payload().downcast_ref::<String>().cloned())
                    .unwrap_or_default();
                let _ = CFB_SITES.try_with(|s| s.borrow_mut().push(format!("{at}: {msg}")));
            } else {
                let _ = OTHER_PANICS.try_with(|c| c.set(c.get() + 1));
                let hook = info
                    .payload()
                    .downcast_ref::<&str>()
                    .is_some_and(|m| m.ends_with("(test hook)"));
                if !hook {
                    default(info);
                }
            }
        }));
    });
}

// ---------------------------------------------------------------- the run's numbers

/// What a test's cases came to.
#[derive(Debug, Default)]
struct Stats {
    cases: usize,
    slowest: Duration,
    slowest_case: String,
    peak: usize,
    peak_case: String,
    refused: usize,
    cfb_panics: usize,
    cfb_sites: BTreeSet<String>,
    cfb_inputs: Vec<String>,
}

impl Stats {
    fn print(&self, name: &str, took: Duration) {
        println!(
            "{name}: {} cases in {took:.1?}; {} refused; slowest {:.1?} ({}); peak {:.1} MiB ({}); cfb panics {}",
            self.cases,
            self.refused,
            self.slowest,
            self.slowest_case,
            self.peak as f64 / MIB as f64,
            self.peak_case,
            self.cfb_panics,
        );
        for s in &self.cfb_sites {
            println!("  cfb panic at {s}");
        }
        for s in self.cfb_inputs.iter().take(5) {
            println!("  cfb panic input: {s}");
        }
    }
}

// ---------------------------------------------------------------- one case

/// A converted item standing for `a`, as the job would make it.
fn item_of(a: &core_io::dawproject::package::AudioRef) -> Imported {
    Imported {
        path: format!("Media/{} {}.caf", a.name, a.key),
        name: a.name.clone(),
        original_name: format!("{}.wav", a.name),
        channels: a.channels,
        sample_rate: a.sample_rate,
        length_frames: a.frames,
        content_hash: format!("hash {}", a.key),
        clipped: 0,
    }
}

/// Opens the AAF at `path` and does what the import job does with it,
/// short of converting audio: the info, the bytes needed, each embedded
/// file extracted beside it, the pairs and the mapping (every file that
/// is not offline standing as converted).
fn attempt(path: &Path) -> Result<(), IoError> {
    let never = AtomicBool::new(false);
    let f = AafFile::open(path, None, &AafLimits::default(), &never)?;
    let _ = f.info();
    let _ = f.needed_bytes(3, &never)?;
    let dir = path.parent().unwrap_or(Path::new("."));
    let mut sources = BTreeMap::new();
    for a in f.audio() {
        let s = match &a.source {
            AudioSource::Offline(why) => MediaSource::Offline { why: why.clone() },
            AudioSource::External(_) => MediaSource::Online(vec![item_of(a)]),
            _ => {
                let to = dir.join(format!("extract {}", a.key));
                match f.extract(a, &to, &never) {
                    Ok(_) => MediaSource::Online(vec![item_of(a)]),
                    Err(IoError::Refused(why)) => MediaSource::Offline { why },
                    Err(e) => return Err(e),
                }
            }
        };
        sources.insert(a.key, s);
    }
    let joined = pairs(f.composition(), &sources, true);
    let tempo = TempoMap::constant(Bpm::whole(120)).unwrap();
    let env = AafEnv {
        rate: 48_000,
        tempo: &tempo,
        start: Tick(0),
        join_pairs: true,
        audio: f.audio(),
    };
    let imp = map(f.composition(), &sources, &joined, &env, &never)?;
    std::hint::black_box(&imp.report);
    Ok(())
}

/// Runs one case on `bytes` (described by `what` for the report).
fn run_case(bytes: &[u8], what: &dyn Fn() -> String, stats: &Mutex<Stats>) {
    count_panics();
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("Hostile.aaf");
    std::fs::write(&path, bytes).unwrap();
    CFB_PANICS.with(|c| c.set(0));
    OTHER_PANICS.with(|c| c.set(0));
    CFB_SITES.with(|s| s.borrow_mut().clear());
    let start = Instant::now();
    let (result, seen) = measure(|| attempt(&path));
    let took = start.elapsed();
    let (cfb, other) = (CFB_PANICS.with(Cell::get), OTHER_PANICS.with(Cell::get));
    let sites = CFB_SITES.with(|s| std::mem::take(&mut *s.borrow_mut()));
    assert_eq!(other, 0, "a panic outside cfb ({}): {result:?}", what());
    match &result {
        Ok(()) | Err(IoError::Refused(_)) | Err(IoError::Cancelled) => {}
        Err(e) => panic!("{e:?} ({})", what()),
    }
    if cfb > 0 {
        assert!(
            matches!(&result, Err(IoError::Refused(w)) if w == DAMAGED),
            "a cfb panic gave {result:?} ({})",
            what()
        );
    }
    assert!(
        seen.peak < PEAK,
        "peak {} MiB ({})",
        seen.peak / MIB,
        what()
    );
    assert!(took < time_for(bytes.len()), "{took:?} ({})", what());
    let mut s = stats.lock().unwrap_or_else(|e| e.into_inner());
    s.cases += 1;
    if matches!(result, Err(IoError::Refused(_))) {
        s.refused += 1;
    }
    if took > s.slowest {
        s.slowest = took;
        s.slowest_case = what();
    }
    if seen.peak > s.peak {
        s.peak = seen.peak;
        s.peak_case = what();
    }
    if cfb > 0 {
        s.cfb_panics += cfb;
        s.cfb_sites.extend(sites);
        if s.cfb_inputs.len() < 5 {
            s.cfb_inputs.push(what());
        }
    }
}

// ---------------------------------------------------------------- goldens

fn no() -> AtomicBool {
    AtomicBool::new(false)
}

/// A small song: "Keys" (stereo) with a faded region, a crossfaded one and
/// fader and pan automation; "Vox" (mono) with gain; two markers.
fn song() -> (core_model::document::Document, core_model::ids::SeqId) {
    let mut b = Builder::new();
    let (seq, _) = b.sequence("Song", "Unused", None).unwrap();
    let (keys, keys_strip) = audio_track(&mut b, seq, "Keys");
    let (vox, _) = audio_track(&mut b, seq, "Vox");
    let st = media(&mut b, "keys", 4_800, 2);
    let mo = media(&mut b, "vox", 2_400, 1);
    let fade = |n| Fade {
        length_frames: n,
        shape: FadeShape::Linear,
    };
    b.region(seq, keys, 0, st, 0, 4_800, 0.0, fade(480), fade(240))
        .unwrap();
    let second = b
        .region(
            seq,
            keys,
            Q / 8,
            st,
            100,
            2_000,
            -3.0,
            Fade::default(),
            fade(100),
        )
        .unwrap();
    b.run(Command::Audio(AudioCmd::Crossfade {
        event: second,
        length: CrossfadeLength::UpTo(480),
        shape: FadeShape::Linear,
    }))
    .unwrap();
    b.region(
        seq,
        vox,
        Q,
        mo,
        0,
        2_400,
        6.0,
        Fade::default(),
        Fade::default(),
    )
    .unwrap();
    b.events(
        seq,
        keys,
        [
            (0, 0.5, true),
            (Q, 0.75, true),
            (0, 0.25, false),
            (Q, 0.8, false),
        ]
        .into_iter()
        .map(|(t, v, fader)| {
            EventSpec::new(
                Tick(t),
                EventBody::Automation {
                    target: if fader {
                        AutomationTarget::StripFader(keys_strip)
                    } else {
                        AutomationTarget::StripPan(keys_strip)
                    },
                    value: v,
                },
            )
        })
        .collect(),
    )
    .unwrap();
    b.run(Command::Sequence(SequenceCmd::SetMarkers {
        seq,
        markers: [(0, "Start"), (Q, "Verse")]
            .into_iter()
            .map(|(t, n)| core_model::sequence::Marker {
                tick: Tick(t),
                name: n.into(),
            })
            .collect(),
    }))
    .unwrap();
    (b.finish(), seq)
}

/// Task 4's writer output for four option sets.
fn exports() -> Vec<Vec<u8>> {
    let (doc, seq) = song();
    let base = AafOptions {
        seq,
        embed: true,
        depth: BitDepth::Int24,
        stereo: Stereo::MonoPairs,
        hour_start: false,
        fps: Fps::F25,
        clip_gain: true,
        automation: true,
        left_out: LeftOutAudio::new(),
    };
    let sets = [
        base.clone(),
        AafOptions {
            embed: false,
            depth: BitDepth::Int16,
            hour_start: true,
            fps: Fps::F2997Drop,
            ..base.clone()
        },
        AafOptions {
            stereo: Stereo::Stereo,
            depth: BitDepth::Int16,
            fps: Fps::F24,
            ..base.clone()
        },
        AafOptions {
            clip_gain: false,
            automation: false,
            fps: Fps::F30,
            ..base
        },
    ];
    let env = ExportEnv {
        app_version: "1.2.3",
        export_id: "0f1e2d3c-4b5a-6978-8796-a5b4c3d2e1f0",
        modified: 1_700_000_000,
    };
    sets.iter()
        .map(|o| {
            let x = export(&doc, o, &env, &no()).expect("exports");
            let dir = tempfile::tempdir().unwrap();
            let bits = if o.depth == BitDepth::Int16 { 16 } else { 24 };
            std::fs::read(write_export(&doc, &x, dir.path(), bits)).unwrap()
        })
        .collect()
}

/// Task 2's other shapes: another edit rate, nested sequences, nested
/// gain, a transition without a cut point, clip fades, a sub-composition,
/// a selector, one channel of a stereo file, a picture slot, a mob chain
/// that loops, PCM essence and an external file.
fn shapes() -> Vec<Vec<u8>> {
    let mut out = Vec::new();

    let mut fx = Fx::new();
    let a = fx.file(&FileSpec::wav("A", 48_000, 1, 4_800));
    fx.comp(
        "Film",
        true,
        &[sound(
            1,
            "Dialog",
            (25, 1),
            Seg::Seq(vec![Seg::Fill(25), clip(&a, 1, 10, 50)]),
        )],
    );
    out.push(fx.bytes());

    let mut fx = Fx::new();
    let a = fx.file(&FileSpec::wav("A", 48_000, 1, 4_800));
    fx.comp(
        "Nest",
        true,
        &[sound(
            1,
            "T",
            R48,
            Seg::Seq(vec![
                Seg::Fill(100),
                Seg::Seq(vec![
                    gain(1, 2, gain(1, 2, clip(&a, 1, 0, 200))),
                    Seg::Fill(50),
                    Seg::Seq(vec![clip(&a, 1, 0, 100)]),
                ]),
                trans(20, None),
                Seg::Clip(ClipSpec {
                    mob: a.clone(),
                    slot: 1,
                    start: 0,
                    len: 1_000,
                    fade_in: Some((240, 1)),
                    fade_out: Some((120, 2)),
                }),
            ]),
        )],
    );
    out.push(fx.bytes());

    let mut fx = Fx::new();
    let a = fx.file(&FileSpec::wav("A", 48_000, 1, 4_800));
    let b = fx.file(&FileSpec::wav("B", 48_000, 1, 4_800));
    let st = fx.file(&FileSpec::wav("Stereo", 48_000, 2, 4_800));
    let sub = fx.comp(
        "Sub",
        false,
        &[sound(
            1,
            "S",
            R48,
            Seg::Seq(vec![Seg::Fill(100), clip(&a, 1, 0, 500)]),
        )],
    );
    fx.comp(
        "Main",
        true,
        &[
            sound(
                1,
                "M",
                R48,
                Seg::Seq(vec![Seg::Fill(1_000), clip(&sub, 1, 50, 400)]),
            ),
            sound(
                2,
                "Sel",
                R48,
                Seg::Seq(vec![Seg::Select(
                    Box::new(clip(&a, 1, 0, 300)),
                    vec![clip(&b, 1, 0, 300)],
                )]),
            ),
            sound(3, "R", R48, Seg::Seq(vec![clip(&st, 2, 0, 300)])),
            SlotSpec {
                id: 4,
                name: "V1".into(),
                rate: (25, 1),
                kind: SlotKind::Data(PICTURE, Seg::Seq(vec![Seg::Fill(100)])),
            },
        ],
    );
    out.push(fx.bytes());

    let mut fx = Fx::new();
    let (ma, mb) = (fx.new_id(), fx.new_id());
    fx.mob(
        class::MASTER_MOB,
        &ma,
        "A",
        false,
        &[sound(1, "", R48, clip(&mb, 1, 0, 100))],
    );
    fx.mob(
        class::MASTER_MOB,
        &mb,
        "B",
        false,
        &[sound(1, "", R48, clip(&ma, 1, 0, 100))],
    );
    let pcm = fx.file(&FileSpec {
        media: common::aaf::Media::Pcm(vec![0; 2 * 4_800]),
        ..FileSpec::wav("Pcm", 48_000, 1, 4_800)
    });
    let ext = fx.file(&FileSpec::url(
        "Ext",
        48_000,
        1,
        4_800,
        "file:///nowhere/ext.wav",
    ));
    fx.comp(
        "Loop",
        true,
        &[
            sound(1, "L", R48, Seg::Seq(vec![clip(&ma, 1, 0, 100)])),
            sound(2, "P", R48, Seg::Seq(vec![clip(&pcm, 1, 0, 100)])),
            sound(3, "E", R48, Seg::Seq(vec![clip(&ext, 1, 0, 100)])),
        ],
    );
    out.push(fx.bytes());
    out
}

/// Every golden: the exports, then the shapes.
fn goldens() -> &'static [Vec<u8>] {
    static GOLDENS: OnceLock<Vec<Vec<u8>>> = OnceLock::new();
    GOLDENS.get_or_init(|| {
        let mut all = exports();
        all.extend(shapes());
        all
    })
}

#[test]
fn the_goldens_import() {
    for (i, g) in goldens().iter().enumerate() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("Fine.aaf");
        std::fs::write(&path, g).unwrap();
        attempt(&path).unwrap_or_else(|e| panic!("golden {i}: {e:?}"));
    }
}

/// `VIZDAW_FUZZ_CASES` cases (default 256), failures not saved to a file.
fn config() -> Config {
    Config {
        failure_persistence: None,
        ..Config::with_cases(cases(256))
    }
}

// ---------------------------------------------------------------- byte damage

#[derive(Debug, Clone)]
enum Damage {
    Flip(prop::sample::Index, u8),
    Insert(prop::sample::Index, Vec<u8>),
    Delete(prop::sample::Index, usize),
    Truncate(prop::sample::Index),
}

fn damage() -> impl Strategy<Value = Damage> {
    prop_oneof![
        4 => (any::<prop::sample::Index>(), 1u8..=255).prop_map(|(i, x)| Damage::Flip(i, x)),
        1 => (
            any::<prop::sample::Index>(),
            proptest::collection::vec(any::<u8>(), 1..16)
        )
            .prop_map(|(i, b)| Damage::Insert(i, b)),
        1 => (any::<prop::sample::Index>(), 1usize..64).prop_map(|(i, n)| Damage::Delete(i, n)),
        1 => any::<prop::sample::Index>().prop_map(Damage::Truncate),
    ]
}

fn damaged(mut bytes: Vec<u8>, d: &Damage) -> Vec<u8> {
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

#[test]
fn byte_damage() {
    let n = goldens().len();
    let stats = Mutex::new(Stats::default());
    let mut runner = TestRunner::new(config());
    let start = Instant::now();
    let strategy = (0..n, proptest::collection::vec(damage(), 1..6));
    let r = runner.run(&strategy, |(which, damages)| {
        let mut bytes = goldens()[which].clone();
        for d in &damages {
            bytes = damaged(bytes, d);
        }
        run_case(&bytes, &|| format!("golden {which}, {damages:?}"), &stats);
        Ok(())
    });
    let s = stats.lock().unwrap_or_else(|e| e.into_inner());
    s.print("byte_damage", start.elapsed());
    r.unwrap();
}

// ---------------------------------------------------------------- structure soup

#[test]
fn structure_soup() {
    let stats = Mutex::new(Stats::default());
    let mut runner = TestRunner::new(config());
    let start = Instant::now();
    let r = runner.run(&soup(), |k: Soup| {
        let bytes = soup_bytes(&k);
        run_case(&bytes, &|| format!("{k:?}"), &stats);
        Ok(())
    });
    let s = stats.lock().unwrap_or_else(|e| e.into_inner());
    s.print("structure_soup", start.elapsed());
    r.unwrap();
}

/// The soup with nothing hostile in it opens and maps, so the soup's
/// damage is what the cases test.
#[test]
fn the_plain_soup_imports() {
    for points in [0, 3] {
        let k = Soup {
            points,
            nest: 0,
            chain: 2,
            mob_loop: false,
            wav_lie: false,
            pokes: Vec::new(),
            weak: None,
            edits: Vec::new(),
            sectors: Vec::new(),
        };
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("Plain.aaf");
        std::fs::write(&path, soup_bytes(&k)).unwrap();
        let f = AafFile::open(&path, None, &AafLimits::default(), &no()).unwrap();
        let c = f.composition();
        assert_eq!(c.slots.len(), 1, "{:?}", c.report);
        assert_eq!(c.slots[0].clips.len(), 2, "{:?}", c.report);
        assert_eq!(c.markers.len(), 2);
        assert!(c.start_tc.is_some());
        attempt(&path).unwrap();
    }
}

/// Regression (Task 7's fuzz): a file of 100 000 control points nested 60
/// deep took 2.2 s to read (in release), past the cases' 2 s, as the walk
/// looked up each object's storage, class and `properties` stream from the
/// root four times. Now an object's children are listed once and each
/// object costs one lookup (1.4 s). An unoptimised build is too slow to
/// time it.
#[test]
#[cfg_attr(debug_assertions, ignore = "times an optimised build")]
fn deep_objects_read_in_time() {
    let k = Soup {
        points: 100_000,
        nest: 53,
        chain: 0,
        mob_loop: false,
        wav_lie: false,
        pokes: Vec::new(),
        weak: None,
        edits: Vec::new(),
        sectors: Vec::new(),
    };
    let stats = Mutex::new(Stats::default());
    run_case(
        &soup_bytes(&k),
        &|| "100 000 points, 53 deep".into(),
        &stats,
    );
}

/// Regression (Task 7's fuzz): a file of many deeply nested objects held
/// about twice its model budget before the budget refused it (a 200 000
/// control-point file nested 60 deep peaked at 415 MiB against 192 MiB),
/// because the walk did not count the directory `cfb` holds, the map node
/// each object's properties take, or the copy of each path it checks for
/// repeats. Now the read holds no more than its budget and a little.
#[test]
fn a_read_holds_no_more_than_its_budget() {
    let budget = 16 * MIB;
    let limits = AafLimits {
        max_model_bytes: budget,
        ..AafLimits::default()
    };
    for (points, nest) in [(20_000, 56), (20_000, 0)] {
        let k = Soup {
            points,
            nest,
            chain: 0,
            mob_loop: false,
            wav_lie: false,
            pokes: Vec::new(),
            weak: None,
            edits: Vec::new(),
            sectors: Vec::new(),
        };
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("Many.aaf");
        std::fs::write(&path, soup_bytes(&k)).unwrap();
        let (r, seen) = measure(|| AafFile::open(&path, None, &limits, &no()).map(|_| ()));
        assert!(
            matches!(&r, Err(IoError::Refused(w)) if w == core_io::aaf::store::TOO_LARGE),
            "{r:?}"
        );
        assert!(
            seen.peak <= budget + budget / 4,
            "{points} points {nest} deep: peak {:.1} MiB against a budget of {} MiB",
            seen.peak as f64 / MIB as f64,
            budget / MIB
        );
    }
}

/// A panic inside `cfb` is caught in `store::read` and the file is refused
/// as damaged (Ruling 13). No input that makes `cfb` 0.16 panic was found
/// (Task 1's search, then this fuzz), so the read's test hook panics where
/// `cfb` would.
#[test]
fn cfb_panics_are_refusals() {
    count_panics();
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("Fine.aaf");
    std::fs::write(&path, &goldens()[0]).unwrap();
    core_io::aaf::store::debug_panic_in_next_read();
    let r = AafFile::open(&path, None, &AafLimits::default(), &no());
    assert!(
        matches!(&r, Err(IoError::Refused(w)) if w == DAMAGED),
        "{r:?}"
    );
    // The hook fires once: the same file then reads.
    AafFile::open(&path, None, &AafLimits::default(), &no()).unwrap();
}

/// Regression (Task 7's review): `cfb` loads a file's whole directory and
/// MiniFAT as it opens it, before any limit, so a large file whose header
/// declares long chains made it allocate in proportion to the file (and
/// an allocation that fails aborts the app). The chains are now counted
/// from the header and FAT first: too long is refused, a loop is refused,
/// and little is allocated either way.
#[test]
fn long_directory_and_minifat_chains_are_refused_before_cfb() {
    let limits = AafLimits {
        max_model_bytes: 16 * MIB,
        ..AafLimits::default()
    };
    let dir = tempfile::tempdir().unwrap();
    for (name, d, m, looping, why) in [
        ("dir", 4_096, 0, false, core_io::aaf::store::TOO_LARGE),
        ("minifat", 1, 4_096, false, core_io::aaf::store::TOO_LARGE),
        ("loop", 64, 0, true, DAMAGED),
    ] {
        let path = dir.path().join(format!("{name}.aaf"));
        common::aaf::cfb_skeleton(&path, d, m, looping);
        let t = Instant::now();
        let (r, seen) = measure(|| AafFile::open(&path, None, &limits, &no()).map(|_| ()));
        assert!(
            matches!(&r, Err(IoError::Refused(w)) if w == why),
            "{name}: {r:?}"
        );
        assert!(seen.peak < 2 * MIB, "{name}: peak {} KiB", seen.peak / 1024);
        assert!(t.elapsed() < Duration::from_secs(1), "{name}");
    }
}

/// The owner's ruling after Task 7's review: a large post-production
/// session (300 000 objects and more) reads, within the AAF read budget and
/// the cases' time. Too slow to build and read in an unoptimised build.
#[test]
#[cfg_attr(debug_assertions, ignore = "times an optimised build")]
fn three_hundred_thousand_objects_read() {
    let k = Soup {
        points: 300_000,
        nest: 0,
        chain: 0,
        mob_loop: false,
        wav_lie: false,
        pokes: Vec::new(),
        weak: None,
        edits: Vec::new(),
        sectors: Vec::new(),
    };
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("Large.aaf");
    std::fs::write(&path, soup_bytes(&k)).unwrap();
    let t = Instant::now();
    let (f, seen) = measure(|| AafFile::open(&path, None, &AafLimits::default(), &no()));
    let took = t.elapsed();
    let f = f.unwrap();
    assert_eq!(f.composition().slots.len(), 1);
    assert!(
        seen.peak < core_io::limits::AAF_READ_BUDGET,
        "peak {} MiB",
        seen.peak / MIB
    );
    let size = std::fs::metadata(&path).unwrap().len() as usize;
    assert!(took < time_for(size), "{took:?} for {} MiB", size / MIB);
    println!("300 000 objects: {took:?}, peak {} MiB", seen.peak / MIB);
}
