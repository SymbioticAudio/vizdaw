//! DAWproject export and import from the app (plan 6d Tasks 9 and 10;
//! spec §6, §7.8, §11, §12; Rulings 16–19; L1, L4, L6, L8, S6, S9, S10). Projects are saved packages in temporary
//! folders at 48 kHz 24-bit with generated CAF media: track "Mono" plays
//! `Media/mono.caf` (one channel) and "Stereo" plays `Media/stereo.caf`
//! (two), through one audio path with the test gain CLAP on it. One test at a
//! time: the engine host and the scratch folder are process-wide.
// `im` maps nest deeply; sending a `Session` to a thread needs more depth.
#![recursion_limit = "256"]

mod common;

use core_engine::fixtures::{Builder, Q};
use core_io::dawproject::export::{REGION_GAIN, no_saved_settings, not_loaded, plugin_settings};
use core_io::limits::PackageLimits;
use core_io::zip::{EntryMethod, ZipReader};
use core_media::caf::{CafReader, CafWriter, WriteMode};
use core_model::command::{Command, LaneCmd};
use core_model::document::Document;
use core_model::event::Fade;
use core_model::format::{AudioFormat, BitDepth};
use core_model::ids::{MediaId, PluginInstanceId, SeqId, StripId};
use core_model::lane::LaneInput;
use core_model::mixer::{PluginFormat, PluginRef, StripKind};
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};
use vizdaw_ffi::dawproject::{CAPTURE_FAILS_HOOK, OFFLINE_AUDIO, TOO_LONG_FOR_WAV, not_captured};
use vizdaw_ffi::host::{force_offline, reset_host};
use vizdaw_ffi::{
    BitDepthRow, BlockRule, DawprojectExportRequestRow, DawprojectImportRequestRow,
    DawprojectInfoRow, JobKindRow, JobRow, JobStateRow, ProjectFormatRow, RecordSpecRow,
    RegionGainRow, ReportKindRow, ReportRow, Session, SlotStatus, VizError,
};

static SERIAL: Mutex<()> = Mutex::new(());

fn serial() -> MutexGuard<'static, ()> {
    let g = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    force_offline();
    vizdaw_ffi::host::set_offline_rate(48_000.0);
    reset_host();
    vizdaw_ffi::catalog::add_test_bundle(&test_plugins::bundle()).unwrap();
    g
}

const FRAMES: u64 = 4_800;
const GAIN_ID: &str = "vizdaw.test.gain";

/// Channel `c` of the media at frame `f`: a saw well inside full scale.
fn sample(f: u64, c: u64) -> f32 {
    ((f * 37 + c * 101) % 1000) as f32 / 1000.0 * 0.8 - 0.4
}

fn write_caf(path: &Path, channels: u16) {
    let mut w =
        CafWriter::create(path, 48_000, channels, BitDepth::Int24, WriteMode::Normal).unwrap();
    let samples: Vec<f32> = (0..FRAMES)
        .flat_map(|f| (0..u64::from(channels)).map(move |c| sample(f, c)))
        .collect();
    w.write(&samples).unwrap();
    w.finish().unwrap();
}

fn plugin(id: &str, name: &str) -> PluginRef {
    PluginRef {
        format: PluginFormat::Clap,
        id: id.into(),
        name: name.into(),
        vendor: "VizDAW".into(),
        version: "1.0.0".into(),
    }
}

fn strip_of(d: &Document, want: impl Fn(&StripKind) -> bool) -> Vec<StripId> {
    d.mixer()
        .order()
        .iter()
        .copied()
        .filter(|s| d.mixer().strip(*s).is_ok_and(|x| want(&x.kind)))
        .collect()
}

fn text(p: &Path) -> String {
    p.display().to_string()
}

struct Project {
    dir: tempfile::TempDir,
    pkg: PathBuf,
    s: Arc<Session>,
    seq: u64,
    /// Where exports go (outside the package).
    out: PathBuf,
}

/// `P.vizdaw` at 48 kHz 24-bit: "Song" with "Mono" and "Stereo" each
/// playing their media from tick 0, the gain plug-in on their audio path.
/// `extra` adds to the document before it is saved.
fn project_with(extra: impl FnOnce(&mut Builder, SeqId, MediaId, StripId)) -> Project {
    let dir = tempfile::tempdir().unwrap();
    let pkg = dir.path().join("P.vizdaw");
    let mut b = Builder::new();
    // The default format: 48 kHz 24-bit.
    assert_eq!(
        b.doc().settings().format,
        AudioFormat {
            sample_rate: 48_000,
            depth: BitDepth::Int24,
        }
    );
    let (seq, mono) = b.sequence("Song", "Mono", None).unwrap();
    let stereo = b.lane(seq, "Stereo", None).unwrap();
    let m1 = b.media("Media/mono.caf", FRAMES, 1).unwrap();
    let m2 = b.media("Media/stereo.caf", FRAMES, 2).unwrap();
    for (lane, m) in [(mono, m1), (stereo, m2)] {
        b.region(
            seq,
            lane,
            0,
            m,
            0,
            FRAMES,
            0.0,
            Fade::default(),
            Fade::default(),
        )
        .unwrap();
    }
    let paths = strip_of(b.doc(), |k| matches!(k, StripKind::AudioPath(_)));
    let gain = b.insert(paths[0], plugin(GAIN_ID, "Test Gain")).unwrap();
    extra(&mut b, seq, m1, paths[0]);
    let doc = b.finish();
    core_model::file::save(&doc, &pkg).unwrap();
    write_caf(&pkg.join("Media/mono.caf"), 1);
    write_caf(&pkg.join("Media/stereo.caf"), 2);
    let s = Session::open(text(&pkg)).unwrap();
    s.debug_set_trash_dir(Some(text(&dir.path().join("Trash"))));
    // The hooks that stay set start off in every test.
    s.debug_free_bytes(None).unwrap();
    s.debug_dawproject_capture_fails(false).unwrap();
    let out = dir.path().join("out");
    std::fs::create_dir(&out).unwrap();
    let p = Project {
        dir,
        pkg,
        s,
        seq: seq.0,
        out,
    };
    idle_until(&p.s, |s| {
        s.plugin_slots()
            .unwrap()
            .iter()
            .any(|r| r.id == gain.0 && r.status == SlotStatus::Live)
    });
    p
}

fn project() -> Project {
    project_with(|_, _, _, _| {})
}

fn req(seq: u64, dest: &Path) -> DawprojectExportRequestRow {
    DawprojectExportRequestRow {
        seq,
        path: text(dest),
        replace: false,
        region_gain: RegionGainRow::Apply,
        blocks: BlockRule::WholeLane,
    }
}

fn wait_job(s: &Session, id: u64) -> JobRow {
    let until = Instant::now() + common::wait_limit();
    loop {
        s.idle().unwrap();
        let j = s.job(id).unwrap();
        if j.state != JobStateRow::Running {
            s.idle().unwrap();
            return j;
        }
        assert!(Instant::now() < until, "job {id} still running");
        std::thread::yield_now();
    }
}

fn idle_until(s: &Session, mut done: impl FnMut(&Session) -> bool) {
    let until = Instant::now() + common::wait_limit();
    while !done(s) {
        assert!(Instant::now() < until, "timed out");
        s.idle().unwrap();
        std::thread::sleep(Duration::from_millis(1));
    }
}

/// Exports `r` and waits for it to end Done.
fn export(s: &Session, r: DawprojectExportRequestRow) -> JobRow {
    let j = wait_job(s, s.export_dawproject(r).unwrap());
    assert_eq!(j.state, JobStateRow::Done, "{j:?}");
    assert_eq!(j.kind, JobKindRow::ExportDawproject);
    j
}

/// Every entry under `dir` (hidden ones too), relative, sorted.
fn listing(dir: &Path) -> Vec<String> {
    fn walk(root: &Path, at: &Path, out: &mut Vec<String>) {
        let Ok(rd) = std::fs::read_dir(at) else {
            return;
        };
        for e in rd {
            let p = e.unwrap().path();
            let rel = p.strip_prefix(root).unwrap().display().to_string();
            let meta = std::fs::symlink_metadata(&p).unwrap();
            if meta.is_dir() {
                out.push(format!("{rel}/"));
                walk(root, &p, out);
            } else {
                out.push(rel);
            }
        }
    }
    let mut v = Vec::new();
    walk(dir, dir, &mut v);
    v.sort();
    v
}

fn package(path: &Path) -> ZipReader {
    ZipReader::open(path, &PackageLimits::default()).unwrap()
}

fn entry(z: &ZipReader, name: &str) -> Vec<u8> {
    let e = z.find(name).unwrap_or_else(|| panic!("no {name}"));
    z.read(e, u64::MAX, None, &AtomicBool::new(false)).unwrap()
}

fn record(path: &Path) -> serde_json::Value {
    serde_json::from_slice(&entry(&package(path), "vizdaw/record.json")).unwrap()
}

/// `(channels, rate, bits, samples)` of a WAV's `fmt ` and `data`, the
/// samples as integers.
fn read_wav(b: &[u8]) -> (u16, u32, u16, Vec<i32>) {
    assert_eq!(&b[0..4], b"RIFF");
    assert_eq!(&b[8..12], b"WAVE");
    let mut at = 12;
    let (mut ch, mut rate, mut bits) = (0, 0, 0);
    let mut data = None;
    while at + 8 <= b.len() {
        let id = &b[at..at + 4];
        let n = u32::from_le_bytes(b[at + 4..at + 8].try_into().unwrap()) as usize;
        let body = &b[at + 8..at + 8 + n];
        if id == b"fmt " {
            ch = u16::from_le_bytes(body[2..4].try_into().unwrap());
            rate = u32::from_le_bytes(body[4..8].try_into().unwrap());
            bits = u16::from_le_bytes(body[14..16].try_into().unwrap());
        }
        if id == b"data" {
            data = Some(body.to_vec());
        }
        at += 8 + n + (n & 1);
    }
    let data = data.expect("a data chunk");
    let bytes = usize::from(bits / 8);
    let samples = data
        .chunks(bytes)
        .map(|c| match bytes {
            2 => i32::from(i16::from_le_bytes([c[0], c[1]])),
            3 => i32::from_le_bytes([0, c[0], c[1], c[2]]) >> 8,
            _ => panic!("{bits} bits"),
        })
        .collect::<Vec<_>>();
    assert_eq!(
        samples.len() * bytes,
        data.len(),
        "whole samples in the data chunk"
    );
    (ch, rate, bits, samples)
}

/// The CAF's first `n` frames as 24-bit integers.
fn caf_ints(path: &Path, n: usize) -> Vec<i32> {
    let r = CafReader::open(path).unwrap();
    let ch = usize::from(r.info().channels);
    let mut out = vec![0f32; n * ch];
    let mut scratch = Vec::new();
    assert_eq!(r.read(0, &mut out, &mut scratch).unwrap(), n);
    out.iter()
        .map(|x| (x * 8_388_608.0).round() as i32)
        .collect()
}

fn items(r: &ReportRow, kind: ReportKindRow, what: &str) -> u64 {
    r.items
        .iter()
        .filter(|i| i.kind == kind && i.what == what)
        .map(|i| i.count)
        .sum()
}

fn names(z: &ZipReader) -> Vec<String> {
    z.entries().iter().map(|e| e.name.clone()).collect()
}

/// Every file `project.xml` names (`path="…"`) is an entry of the package.
fn references_resolve(z: &ZipReader) -> Vec<String> {
    let xml = String::from_utf8(entry(z, "project.xml")).unwrap();
    let all = names(z);
    let paths: Vec<String> = xml
        .split("path=\"")
        .skip(1)
        .map(|rest| rest.split('"').next().unwrap().to_owned())
        .collect();
    for p in &paths {
        assert!(all.contains(p), "project.xml names {p}, not in {all:?}");
    }
    paths
}

/// The audio files `project.xml` names, each once, after checking every
/// file it names is in the package.
fn audio_refs(z: &ZipReader) -> Vec<String> {
    let mut v: Vec<String> = references_resolve(z)
        .into_iter()
        .filter(|p| p.starts_with("audio/"))
        .collect();
    v.sort();
    v.dedup();
    v
}

/// The "Huge" track's media item: mono, with more frames than a 24-bit WAV
/// holds (its file is never made: the model's length is what counts).
const HUGE_FRAMES: u64 = 1 << 31;

fn add_huge(b: &mut Builder, seq: SeqId) {
    let lane = b.lane(seq, "Huge", None).unwrap();
    let m = b.media("Media/huge.caf", HUGE_FRAMES, 1).unwrap();
    b.region(
        seq,
        lane,
        0,
        m,
        0,
        FRAMES,
        0.0,
        Fade::default(),
        Fade::default(),
    )
    .unwrap();
}

/// The media item's name in the session.
fn media_name(s: &Session, path: &str) -> String {
    s.media()
        .unwrap()
        .into_iter()
        .find(|m| m.path == path)
        .unwrap_or_else(|| panic!("no media {path}"))
        .name
}

/// Removes `Media/mono.caf` and has the session notice it.
fn take_mono_offline(p: &Project) {
    std::fs::remove_file(p.pkg.join("Media/mono.caf")).unwrap();
    p.s.recheck_media().unwrap();
    assert!(
        p.s.media()
            .unwrap()
            .iter()
            .any(|m| m.path == "Media/mono.caf" && m.offline.is_some()),
        "mono.caf is offline"
    );
}

fn details(r: &ReportRow, kind: ReportKindRow, what: &str) -> Vec<String> {
    r.items
        .iter()
        .filter(|i| i.kind == kind && i.what == what)
        .flat_map(|i| i.details.clone())
        .collect()
}

fn no_partial(dir: &Path) {
    let all = listing(dir);
    assert!(
        all.iter().all(|f| !f.contains(".partial")),
        "a partial is left: {all:?}"
    );
}

fn snapshots(root: &Path) -> Vec<String> {
    let dir = root.join("Interchange");
    let mut v: Vec<String> = std::fs::read_dir(&dir)
        .map(|rd| {
            rd.map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
                .collect()
        })
        .unwrap_or_default();
    v.sort();
    v
}

/// The package holds exactly its six entries, in order, with their
/// methods; the WAVs are the media at the project's depth; the row names
/// the file (spec §6.5; Ruling 8).
#[test]
fn export_writes_the_package() {
    let _g = serial();
    let p = project();
    let dest = p.out.join("Song.dawproject");
    let j = export(&p.s, req(p.seq, &dest));
    assert_eq!(j.written, vec![text(&dest)]);
    let report = j.report.clone().unwrap();
    assert!(report.summary.starts_with("Exported"), "{report:?}");
    let z = package(&dest);
    assert_eq!(
        names(&z),
        [
            "metadata.xml",
            "project.xml",
            "vizdaw/record.json",
            "audio/001.wav",
            "audio/002.wav",
            "plugins/001.clap-preset",
        ]
    );
    let methods: Vec<_> = z.entries().iter().map(|e| e.method).collect();
    use EntryMethod::{Deflated, Stored};
    assert_eq!(
        methods,
        [
            Some(Deflated),
            Some(Deflated),
            Some(Deflated),
            Some(Stored),
            Some(Stored),
            Some(Deflated)
        ]
    );
    for (name, caf, channels) in [
        ("audio/001.wav", "mono.caf", 1u16),
        ("audio/002.wav", "stereo.caf", 2),
    ] {
        let (ch, rate, bits, samples) = read_wav(&entry(&z, name));
        assert_eq!((ch, rate, bits), (channels, 48_000, 24), "{name}");
        assert_eq!(samples.len() as u64, FRAMES * u64::from(channels), "{name}");
        let want = caf_ints(&p.pkg.join("Media").join(caf), 16);
        assert_eq!(&samples[..16 * usize::from(channels)], &want[..], "{name}");
    }
    assert!(!entry(&z, "plugins/001.clap-preset").is_empty());
    assert_eq!(listing(&p.out), vec!["Song.dawproject".to_string()]);
    // The session's last report is this one.
    assert_eq!(p.s.interchange_report().unwrap(), Some(report));
    let _ = &p.dir;
}

/// Ruling 10: Apply writes a −6 dB region's own copy, scaled; LeaveOut
/// shares the media's one copy and lists the gain.
#[test]
fn region_gain_apply_and_leave_out() {
    let _g = serial();
    let p = project_with(|b, seq, mono, _| {
        let quiet = b.lane(seq, "Quiet", None).unwrap();
        b.region(
            seq,
            quiet,
            16 * Q,
            mono,
            0,
            FRAMES,
            -6.0,
            Fade::default(),
            Fade::default(),
        )
        .unwrap();
    });
    let dest = p.out.join("Apply.dawproject");
    let j = export(&p.s, req(p.seq, &dest));
    let z = package(&dest);
    let wavs: Vec<String> = names(&z)
        .into_iter()
        .filter(|n| n.starts_with("audio/"))
        .collect();
    assert_eq!(wavs.len(), 3, "{wavs:?}");
    let full = caf_ints(&p.pkg.join("Media/mono.caf"), FRAMES as usize);
    let gain = 10f64.powf(-6.0 / 20.0);
    assert!((gain - 0.501_187).abs() < 1e-6);
    let quiet = wavs
        .iter()
        .map(|n| read_wav(&entry(&z, n)))
        .find(|(ch, _, _, s)| *ch == 1 && s != &full)
        .expect("the scaled copy");
    for (got, src) in quiet.3.iter().zip(&full) {
        let want = (f64::from(*src) * gain).round() as i32;
        assert!((got - want).abs() <= 1, "{got} vs {want}");
    }
    assert_eq!(
        items(
            j.report.as_ref().unwrap(),
            ReportKindRow::LeftOut,
            REGION_GAIN
        ),
        0
    );

    let dest = p.out.join("Leave.dawproject");
    let j = export(
        &p.s,
        DawprojectExportRequestRow {
            region_gain: RegionGainRow::LeaveOut,
            ..req(p.seq, &dest)
        },
    );
    let z = package(&dest);
    assert_eq!(
        names(&z).iter().filter(|n| n.starts_with("audio/")).count(),
        2,
        "one copy of each media item"
    );
    assert_eq!(
        items(
            j.report.as_ref().unwrap(),
            ReportKindRow::LeftOut,
            REGION_GAIN
        ),
        1
    );
}

/// Ruling 16, L6: each export keeps its snapshot in the media root's
/// `Interchange/` (made when missing); Save As carries the ten newest,
/// never a link; each snapshot is its file's record.
#[test]
fn snapshots_are_kept_and_save_prunes_to_ten() {
    let _g = serial();
    // A titled project with no `Interchange/` gets one.
    let p = project();
    assert!(!p.pkg.join("Interchange").exists());
    let first = p.out.join("First.dawproject");
    export(&p.s, req(p.seq, &first));
    let kept = snapshots(&p.pkg);
    assert_eq!(kept.len(), 1, "{kept:?}");
    let id = record(&first)["export_id"].as_str().unwrap().to_owned();
    assert_eq!(kept, vec![format!("{id}.json")]);
    p.s.close().unwrap();

    // Twelve exports of an untitled project.
    let dir = tempfile::tempdir().unwrap();
    vizdaw_ffi::media::set_scratch_dir(Some(dir.path().join("scratch")));
    let mut b = Builder::new();
    let synth = b.dest("Synth", "builtin:synth", 0).unwrap();
    let (seq, keys) = b.sequence("Song", "Keys", Some(synth)).unwrap();
    b.notes(seq, keys, &[(0, 60, 0x8000, Q)]).unwrap();
    let s = Session::from_document(b.finish()).unwrap();
    s.debug_set_trash_dir(Some(text(&dir.path().join("Trash"))));
    let out = dir.path().join("out");
    std::fs::create_dir(&out).unwrap();
    let mut files = Vec::new();
    for k in 0..12 {
        let f = out.join(format!("{k}.dawproject"));
        export(&s, req(seq.0, &f));
        files.push(f);
    }
    let untitled = dir.path().join("scratch/VizDAW-untitled");
    let roots: Vec<PathBuf> = std::fs::read_dir(&untitled)
        .unwrap()
        .map(|e| e.unwrap().path())
        .collect();
    assert_eq!(roots.len(), 1, "{roots:?}");
    let scratch = roots[0].clone();
    let all = snapshots(&scratch);
    assert_eq!(all.len(), 12);
    for f in &files {
        let rec = record(f);
        let id = rec["export_id"].as_str().unwrap();
        let snap: serde_json::Value = serde_json::from_slice(
            &std::fs::read(scratch.join("Interchange").join(format!("{id}.json"))).unwrap(),
        )
        .unwrap();
        assert_eq!(snap["export_id"], rec["export_id"]);
        assert_eq!(snap["project_id"], rec["project_id"]);
    }
    // A link in `Interchange/` is neither followed nor copied.
    let outside = dir.path().join("elsewhere.json");
    std::fs::write(&outside, b"{}").unwrap();
    let link_name = "0f8fad5b-d9cb-469f-a165-70867728950e.json";
    std::os::unix::fs::symlink(&outside, scratch.join("Interchange").join(link_name)).unwrap();
    let newest: Vec<String> = {
        let mut v: Vec<(std::time::SystemTime, String)> = all
            .iter()
            .map(|n| {
                let m = std::fs::metadata(scratch.join("Interchange").join(n)).unwrap();
                (m.modified().unwrap(), n.clone())
            })
            .collect();
        v.sort();
        let mut names: Vec<String> = v.into_iter().skip(2).map(|(_, n)| n).collect();
        names.sort();
        names
    };
    let saved = dir.path().join("Saved.vizdaw");
    s.save_as(text(&saved)).unwrap();
    assert_eq!(snapshots(&saved), newest);
    assert!(
        std::fs::symlink_metadata(saved.join("Interchange").join(link_name)).is_err(),
        "the link was not copied"
    );
    assert_eq!(std::fs::read(&outside).unwrap(), b"{}");
    // A Save keeps them pruned.
    s.save().unwrap();
    assert_eq!(snapshots(&saved), newest);
    s.close().unwrap();
    vizdaw_ffi::media::set_scratch_dir(None);
}

/// Ruling 16: an untitled export stamps the project's ID (no undo step);
/// the next export reuses it.
#[test]
fn project_id_is_stamped_once() {
    let _g = serial();
    let dir = tempfile::tempdir().unwrap();
    vizdaw_ffi::media::set_scratch_dir(Some(dir.path().join("scratch")));
    let mut b = Builder::new();
    let synth = b.dest("Synth", "builtin:synth", 0).unwrap();
    let (seq, keys) = b.sequence("Song", "Keys", Some(synth)).unwrap();
    b.notes(seq, keys, &[(0, 60, 0x8000, Q)]).unwrap();
    let s = Session::from_document(b.finish()).unwrap();
    s.debug_set_trash_dir(Some(text(&dir.path().join("Trash"))));
    let label = s.snapshot().unwrap().undo_label;
    let out = dir.path().join("out");
    std::fs::create_dir(&out).unwrap();
    let one = out.join("one.dawproject");
    export(&s, req(seq.0, &one));
    let id = record(&one)["project_id"].as_str().unwrap().to_owned();
    assert_eq!(id.len(), 36, "{id}");
    assert_eq!(s.snapshot().unwrap().undo_label, label, "no undo step");
    let two = out.join("two.dawproject");
    export(&s, req(seq.0, &two));
    assert_eq!(record(&two)["project_id"].as_str(), Some(id.as_str()));
    assert_ne!(record(&two)["export_id"], record(&one)["export_id"]);
    s.close().unwrap();
    vizdaw_ffi::media::set_scratch_dir(None);
}

/// Lessons: a cancel at any stage leaves no file, no partial, no staged
/// audio and no snapshot.
#[test]
fn cancel_at_every_stage_leaves_nothing() {
    let _g = serial();
    let p = project();
    let dest = p.out.join("x.dawproject");
    // Before the run.
    p.s.debug_hold_jobs(true).unwrap();
    let id = p.s.export_dawproject(req(p.seq, &dest)).unwrap();
    p.s.cancel_job(id).unwrap();
    p.s.debug_hold_jobs(false).unwrap();
    assert_eq!(wait_job(&p.s, id).state, JobStateRow::Cancelled);
    assert_eq!(listing(&p.out), Vec::<String>::new());
    for stage in 1..=4 {
        p.s.debug_dawproject_cancel_at(Some(stage)).unwrap();
        let j = wait_job(&p.s, p.s.export_dawproject(req(p.seq, &dest)).unwrap());
        assert_eq!(j.state, JobStateRow::Cancelled, "stage {stage}: {j:?}");
        assert_eq!(listing(&p.out), Vec::<String>::new(), "stage {stage}");
        assert!(snapshots(&p.pkg).is_empty(), "stage {stage}");
    }
    // The hook is used once.
    export(&p.s, req(p.seq, &dest));
    no_partial(&p.out);
}

/// Lessons: an existing file or a dangling link is refused at once
/// without `replace`; with it the old file stays until the commit, and a
/// cancel leaves it as it was.
#[test]
fn replacing_needs_replace() {
    let _g = serial();
    let p = project();
    let dest = p.out.join("x.dawproject");
    std::fs::write(&dest, b"old").unwrap();
    let r = p.s.export_dawproject(req(p.seq, &dest));
    assert!(matches!(r, Err(VizError::Model { .. })), "{r:?}");
    let dangling = p.out.join("gone.dawproject");
    std::os::unix::fs::symlink(p.out.join("nowhere"), &dangling).unwrap();
    let r = p.s.export_dawproject(req(p.seq, &dangling));
    assert!(matches!(r, Err(VizError::Model { .. })), "{r:?}");
    assert!(std::fs::symlink_metadata(&dangling).is_ok());

    let replace = DawprojectExportRequestRow {
        replace: true,
        ..req(p.seq, &dest)
    };
    // A cancel after `project.xml` leaves the old file.
    p.s.debug_dawproject_cancel_at(Some(3)).unwrap();
    let j = wait_job(&p.s, p.s.export_dawproject(replace.clone()).unwrap());
    assert_eq!(j.state, JobStateRow::Cancelled);
    assert_eq!(std::fs::read(&dest).unwrap(), b"old");
    no_partial(&p.out);
    // Held: the old file is there until the job commits.
    p.s.debug_hold_jobs(true).unwrap();
    let id = p.s.export_dawproject(replace).unwrap();
    assert_eq!(std::fs::read(&dest).unwrap(), b"old");
    p.s.debug_hold_jobs(false).unwrap();
    assert_eq!(wait_job(&p.s, id).state, JobStateRow::Done);
    assert!(std::fs::read(&dest).unwrap().starts_with(b"PK"));
    no_partial(&p.out);
}

/// Spec §11: running out of space fails the job and leaves nothing.
#[test]
fn not_enough_space_refuses() {
    let _g = serial();
    let p = project();
    p.s.debug_free_bytes(Some(10)).unwrap();
    let dest = p.out.join("x.dawproject");
    let j = wait_job(&p.s, p.s.export_dawproject(req(p.seq, &dest)).unwrap());
    assert_eq!(j.state, JobStateRow::Failed);
    let m = j.message.unwrap();
    assert!(
        m.starts_with("Not enough free space to export (it needs ") && m.ends_with(" MB)."),
        "{m}"
    );
    assert_eq!(listing(&p.out), Vec::<String>::new());
    assert!(snapshots(&p.pkg).is_empty());
    p.s.debug_free_bytes(None).unwrap();
}

fn busy<T: std::fmt::Debug>(r: Result<T, VizError>, what: &str) {
    assert!(matches!(r, Err(VizError::Busy { .. })), "{what}: {r:?}");
}

/// L1: one interchange job at a time; an open edit and recording refuse.
#[test]
fn busy_states_refuse() {
    let _g = serial();
    let p = project();
    let dest = p.out.join("x.dawproject");
    // Another export.
    p.s.debug_hold_jobs(true).unwrap();
    let id = p.s.export_dawproject(req(p.seq, &dest)).unwrap();
    busy(
        p.s.export_dawproject(req(p.seq, &p.out.join("y.dawproject"))),
        "another export",
    );
    busy(
        p.s.export_midi(vizdaw_ffi::SmfExportRequestRow {
            seq: p.seq,
            lanes: Vec::new(),
            path: text(&p.out.join("a.mid")),
            replace: false,
            type0: false,
            ppq: 960,
            mpe: false,
        }),
        "a MIDI export under it",
    );
    p.s.cancel_job(id).unwrap();
    p.s.debug_hold_jobs(false).unwrap();
    assert_eq!(wait_job(&p.s, id).state, JobStateRow::Cancelled);

    // A running bounce.
    p.s.debug_hold_jobs(true).unwrap();
    let b =
        p.s.bounce(vizdaw_ffi::BounceRequestRow {
            seq: p.seq,
            source: vizdaw_ffi::BounceSourceRow::Mix,
            range: vizdaw_ffi::BounceRangeRow::Ticks {
                start: 0,
                end: 4 * Q,
            },
            container: vizdaw_ffi::ContainerRow::Wav,
            depth: vizdaw_ffi::BitDepthRow::Int24,
            rate: 48_000,
            dither: false,
            path: Some(text(&p.out.join("mix.wav"))),
            add_to_project: false,
            replace: false,
        })
        .unwrap();
    busy(p.s.export_dawproject(req(p.seq, &dest)), "a bounce");
    p.s.cancel_job(b).unwrap();
    p.s.debug_hold_jobs(false).unwrap();
    assert_eq!(wait_job(&p.s, b).state, JobStateRow::Cancelled);

    // A running MIDI import.
    let mid = p.dir.path().join("a.mid");
    std::fs::write(&mid, b"MThd").unwrap();
    p.s.debug_hold_jobs(true).unwrap();
    let i =
        p.s.import_midi(vizdaw_ffi::MidiImportRequestRow {
            path: text(&mid),
            new_project: false,
            seq: p.seq,
            lane: None,
            tick: 0,
            channel_map: Vec::new(),
            split_channels: None,
            mpe: false,
            use_tempo: false,
        })
        .unwrap();
    busy(p.s.export_dawproject(req(p.seq, &dest)), "a MIDI import");
    p.s.cancel_job(i).unwrap();
    p.s.debug_hold_jobs(false).unwrap();
    wait_job(&p.s, i);

    // An open edit.
    p.s.begin_edit("Drag".into()).unwrap();
    busy(p.s.export_dawproject(req(p.seq, &dest)), "an open edit");
    p.s.end_edit().unwrap();
    assert_eq!(listing(&p.out), Vec::<String>::new());

    // Recording (offline, an armed track).
    let mut b = Builder::new();
    let synth = b.dest("Synth", "builtin:synth", 0).unwrap();
    let (rseq, keys) = b.sequence("S", "Keys", Some(synth)).unwrap();
    b.run(Command::Lane(LaneCmd::SetInput {
        seq: rseq,
        lane: keys,
        input: Some(LaneInput::Midi {
            port: Some("Keys".into()),
            channel: None,
        }),
    }))
    .unwrap();
    b.run(Command::Lane(LaneCmd::SetFlags {
        seq: rseq,
        lane: keys,
        mute: None,
        solo: None,
        arm: Some(true),
    }))
    .unwrap();
    let r = Session::from_document(b.finish()).unwrap();
    r.debug_set_trash_dir(Some(text(&p.dir.path().join("Trash"))));
    r.record(
        rseq.0,
        RecordSpecRow {
            replace: true,
            punch: None,
            looped: false,
            count_in_bars: 0,
            pre_roll_bars: 0,
            passes_as_takes: true,
        },
    )
    .unwrap();
    let until = Instant::now() + common::wait_limit();
    while !r.record_state().unwrap().recording {
        assert!(Instant::now() < until, "recording did not start");
        std::thread::sleep(Duration::from_millis(5));
    }
    busy(r.export_dawproject(req(rseq.0, &dest)), "recording");
    r.stop().unwrap();
    r.close().unwrap();
}

/// Task 4 carry, spec §11: a slot for a plug-in not installed here writes
/// its saved settings with a Note; one with nothing saved is Left out.
#[test]
fn placeholder_and_missing_state_are_listed() {
    let _g = serial();
    let mut slots = (PluginInstanceId(0), PluginInstanceId(0));
    let p = project_with(|b, _, _, mono_path| {
        let stored = b
            .insert(mono_path, plugin("vizdaw.test.nope", "Gone Stored"))
            .unwrap();
        let (doc, _) = core_model::state::capture(b.doc(), stored, b"saved bytes").unwrap();
        *b = Builder::from_doc(doc);
        let bare = b
            .insert(mono_path, plugin("vizdaw.test.nope2", "Gone Bare"))
            .unwrap();
        slots = (stored, bare);
    });
    idle_until(&p.s, |s| {
        s.plugin_slots()
            .unwrap()
            .iter()
            .filter(|r| matches!(r.status, SlotStatus::Placeholder { .. }))
            .count()
            == 2
    });
    let dest = p.out.join("x.dawproject");
    let j = export(&p.s, req(p.seq, &dest));
    let r = j.report.unwrap();
    assert_eq!(
        items(&r, ReportKindRow::Note, &not_loaded("Gone Stored")),
        1,
        "{r:?}"
    );
    assert_eq!(
        items(&r, ReportKindRow::LeftOut, &plugin_settings("Gone Bare")),
        1,
        "{r:?}"
    );
    assert_eq!(
        items(&r, ReportKindRow::Note, &no_saved_settings("Gone Bare")),
        0,
        "{r:?}"
    );
    assert_eq!(
        items(&r, ReportKindRow::Note, &not_loaded("Test Gain")),
        0,
        "the running plug-in was captured"
    );
    let z = package(&dest);
    let states: Vec<Vec<u8>> = names(&z)
        .iter()
        .filter(|n| n.starts_with("plugins/"))
        .map(|n| entry(&z, n))
        .collect();
    assert_eq!(states.len(), 2, "the gain's and the stored one");
    assert!(states.contains(&b"saved bytes".to_vec()));
    let _ = slots;
}

/// The check reports what the job reports.
#[test]
fn check_matches_the_job() {
    let _g = serial();
    let p = project_with(|b, seq, mono, mono_path| {
        let quiet = b.lane(seq, "Quiet", None).unwrap();
        b.region(
            seq,
            quiet,
            16 * Q,
            mono,
            0,
            FRAMES,
            -6.0,
            Fade::default(),
            Fade::default(),
        )
        .unwrap();
        b.insert(mono_path, plugin("vizdaw.test.nope", "Gone"))
            .unwrap();
        add_huge(b, seq);
    });
    idle_until(&p.s, |s| {
        s.plugin_slots()
            .unwrap()
            .iter()
            .any(|r| matches!(r.status, SlotStatus::Placeholder { .. }))
    });
    take_mono_offline(&p);
    let dest = p.out.join("x.dawproject");
    let r = DawprojectExportRequestRow {
        region_gain: RegionGainRow::LeaveOut,
        ..req(p.seq, &dest)
    };
    // From a thread without the plug-in host (as the app's detached
    // check runs), before anything was captured: the same items.
    let away = {
        let (s, r) = (Arc::clone(&p.s), r.clone());
        std::thread::spawn(move || s.export_dawproject_check(r))
            .join()
            .unwrap()
            .unwrap()
    };
    let check = p.s.export_dawproject_check(r.clone()).unwrap();
    assert_eq!(away, check);
    let j = export(&p.s, r);
    let job = j.report.unwrap();
    assert!(!check.items.is_empty());
    // Offline and over-long audio are among the lines.
    assert_eq!(
        items(&job, ReportKindRow::LeftOut, OFFLINE_AUDIO),
        1,
        "{job:?}"
    );
    assert_eq!(
        items(&job, ReportKindRow::LeftOut, TOO_LONG_FOR_WAV),
        1,
        "{job:?}"
    );
    assert_eq!(check.items, job.items);
    assert_eq!(check.summary, job.summary);
    // The check ignores the path.
    let other = p.s.export_dawproject_check(DawprojectExportRequestRow {
        path: String::new(),
        ..req(p.seq, &dest)
    });
    assert!(other.is_ok(), "{other:?}");
    assert_eq!(
        p.s.dawproject_name(p.seq).unwrap(),
        "Song.dawproject".to_string()
    );
    assert!(matches!(
        p.s.dawproject_name(999),
        Err(VizError::Model { .. })
    ));
}

/// Offline media is Left out by name with no entry, `project.xml` names
/// no file the package lacks, and the job is Done.
#[test]
fn offline_audio_is_left_out() {
    let _g = serial();
    let p = project();
    let name = media_name(&p.s, "Media/mono.caf");
    let dest = p.out.join("full.dawproject");
    let full = export(&p.s, req(p.seq, &dest)).report.unwrap();
    assert!(full.summary.contains("2 audio files"), "{full:?}");
    take_mono_offline(&p);
    let dest = p.out.join("x.dawproject");
    let j = export(&p.s, req(p.seq, &dest));
    let r = j.report.unwrap();
    assert_eq!(items(&r, ReportKindRow::LeftOut, OFFLINE_AUDIO), 1, "{r:?}");
    assert_eq!(
        details(&r, ReportKindRow::LeftOut, OFFLINE_AUDIO),
        vec![name]
    );
    assert!(r.summary.contains("1 audio file"), "{r:?}");
    assert!(!r.summary.contains("2 audio files"), "{r:?}");
    let z = package(&dest);
    let audio: Vec<String> = names(&z)
        .into_iter()
        .filter(|n| n.starts_with("audio/"))
        .collect();
    assert_eq!(audio, ["audio/001.wav"], "only the stereo copy");
    let (ch, _, _, _) = read_wav(&entry(&z, "audio/001.wav"));
    assert_eq!(ch, 2, "the entry left is the stereo media's");
    assert_eq!(audio_refs(&z), ["audio/001.wav"]);
    no_partial(&p.out);
}

/// S9: a media item too long for a WAV at the project depth is Left out by
/// name with no entry and no reference (the model's length decides, so no
/// huge file is made).
#[test]
fn audio_too_long_for_a_wav_is_left_out() {
    let _g = serial();
    let p = project_with(|b, seq, _, _| add_huge(b, seq));
    let name = media_name(&p.s, "Media/huge.caf");
    let dest = p.out.join("x.dawproject");
    let j = export(&p.s, req(p.seq, &dest));
    let r = j.report.unwrap();
    assert_eq!(
        items(&r, ReportKindRow::LeftOut, TOO_LONG_FOR_WAV),
        1,
        "{r:?}"
    );
    assert_eq!(
        details(&r, ReportKindRow::LeftOut, TOO_LONG_FOR_WAV),
        vec![name]
    );
    assert_eq!(
        items(&r, ReportKindRow::LeftOut, OFFLINE_AUDIO),
        0,
        "listed once, as too long: {r:?}"
    );
    assert!(r.summary.contains("2 audio files"), "{r:?}");
    let z = package(&dest);
    let audio: Vec<String> = names(&z)
        .into_iter()
        .filter(|n| n.starts_with("audio/"))
        .collect();
    assert_eq!(audio, ["audio/001.wav", "audio/002.wav"]);
    assert_eq!(audio_refs(&z), ["audio/001.wav", "audio/002.wav"]);
    no_partial(&p.out);
}

/// A running plug-in whose capture fails writes its saved settings, with
/// the Note naming the error.
#[test]
fn a_failed_capture_writes_the_saved_settings() {
    let _g = serial();
    let p = project();
    // A first export captures the gain's settings into the project.
    let first = export(&p.s, req(p.seq, &p.out.join("a.dawproject")));
    let first = first.report.unwrap();
    let saved = entry(
        &package(&p.out.join("a.dawproject")),
        "plugins/001.clap-preset",
    );
    p.s.debug_dawproject_capture_fails(true).unwrap();
    let check = p.s.export_dawproject_check(req(p.seq, &p.out)).unwrap();
    let dest = p.out.join("b.dawproject");
    let r = export(&p.s, req(p.seq, &dest)).report.unwrap();
    p.s.debug_dawproject_capture_fails(false).unwrap();
    let line = not_captured("Test Gain");
    assert_eq!(items(&r, ReportKindRow::Note, &line), 1, "{r:?}");
    assert_eq!(
        details(&r, ReportKindRow::Note, &line),
        vec![CAPTURE_FAILS_HOOK.to_string()]
    );
    assert_eq!(items(&first, ReportKindRow::Note, &line), 0, "{first:?}");
    assert_eq!(check.items, r.items, "the check lists it too");
    assert_eq!(
        entry(&package(&dest), "plugins/001.clap-preset"),
        saved,
        "the saved settings were written"
    );
}

/// Hostile requests are `Model`, never `Internal`, and write nothing.
#[test]
fn hostile_requests() {
    let _g = serial();
    let p = project();
    let fifo = p.out.join("fifo.dawproject");
    assert!(
        std::process::Command::new("mkfifo")
            .arg(&fifo)
            .status()
            .unwrap()
            .success()
    );
    let lp = p.out.join("loop.dawproject");
    std::os::unix::fs::symlink(&lp, &lp).unwrap();
    let folder = p.out.join("dir.dawproject");
    std::fs::create_dir(&folder).unwrap();
    // A device with the right extension: a link to /dev/zero.
    let device = p.out.join("zero.dawproject");
    std::os::unix::fs::symlink("/dev/zero", &device).unwrap();
    let before = listing(&p.out);
    let long = format!("/{}.dawproject", "a/".repeat(2100));
    let paths: Vec<(String, &str)> = vec![
        (String::new(), "empty"),
        ("relative.dawproject".into(), "relative"),
        (text(&folder), "a folder"),
        (text(&fifo), "a FIFO"),
        ("/dev/zero".into(), "/dev/zero"),
        (text(&lp), "a symlink loop"),
        (long, "over 4 KiB"),
        (format!("{}/a\0b.dawproject", text(&p.out)), "a NUL"),
        (text(&p.pkg.join("in.dawproject")), "inside the package"),
        (text(&p.out.join("x.zip")), "ending .zip"),
        (
            text(&p.out.join(format!("{}.dawproject", "n".repeat(235)))),
            "a name too long for its audio partial",
        ),
    ];
    for (path, what) in &paths {
        for replace in [false, true] {
            let r = p.s.export_dawproject(DawprojectExportRequestRow {
                path: path.clone(),
                replace,
                ..req(p.seq, &p.out)
            });
            assert!(
                matches!(r, Err(VizError::Model { .. })),
                "{what} (replace {replace}): {r:?}"
            );
        }
    }
    let r = p.s.export_dawproject(req(999, &p.out.join("x.dawproject")));
    assert!(matches!(r, Err(VizError::Model { .. })), "{r:?}");
    let r =
        p.s.export_dawproject_check(req(999, &p.out.join("x.dawproject")));
    assert!(matches!(r, Err(VizError::Model { .. })), "{r:?}");
    // The device case itself: with `replace` the name is taken but is not
    // a file, so the device rule (not the extension or the taken name)
    // refuses it, for the device link and for the FIFO.
    for (path, what) in [(&device, "a link to /dev/zero"), (&fifo, "a FIFO")] {
        let r = p.s.export_dawproject(DawprojectExportRequestRow {
            replace: true,
            ..req(p.seq, path)
        });
        match r {
            Err(VizError::Model { message }) => assert!(
                message.ends_with("is not a file; choose another name"),
                "{what}: {message}"
            ),
            other => panic!("{what}: {other:?}"),
        }
    }
    assert_eq!(
        std::fs::read_link(&device).unwrap(),
        Path::new("/dev/zero"),
        "the link is left as it was"
    );
    assert_eq!(listing(&p.out), before, "nothing written");
    assert!(snapshots(&p.pkg).is_empty());
}

// ---------------------------------------------------------------- import (Task 10)

/// The hand-written fixtures of `core-io`.
const FIXTURES: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../core-io/tests/data/dawproject"
);

fn import_req(path: &Path) -> DawprojectImportRequestRow {
    DawprojectImportRequestRow { path: text(path) }
}

/// A new project to import into: 48 kHz 24-bit, no tracks, saved as
/// `I.vizdaw` in its own folder (so its media root is known), with a
/// private Trash.
struct Target {
    dir: tempfile::TempDir,
    pkg: PathBuf,
    s: Arc<Session>,
}

fn target() -> Target {
    let dir = tempfile::tempdir().unwrap();
    let s = Session::new_project(ProjectFormatRow {
        sample_rate: 48_000,
        depth: BitDepthRow::Int24,
    })
    .unwrap();
    s.debug_set_trash_dir(Some(text(&dir.path().join("Trash"))));
    s.debug_free_bytes(None).unwrap();
    let pkg = dir.path().join("I.vizdaw");
    s.save_as(text(&pkg)).unwrap();
    Target { dir, pkg, s }
}

/// Imports `path` into `t` and waits for the job to end.
fn import(t: &Target, path: &Path) -> JobRow {
    let j = wait_job(&t.s, t.s.import_dawproject(import_req(path)).unwrap());
    assert_eq!(j.kind, JobKindRow::ImportProject);
    j
}

/// The session's document.
fn doc_of(s: &Session) -> Document {
    core_model::file::from_json(&s.debug_document_json()).unwrap()
}

/// The first sequence of the session's document.
fn first_seq(s: &Session) -> SeqId {
    SeqId(s.snapshot().unwrap().sequences[0].id)
}

/// The files (not folders) under `<root>/Media`, hidden ones and
/// `.incoming` included, relative and sorted.
fn media_files(root: &Path) -> Vec<String> {
    listing(&root.join("Media"))
        .into_iter()
        .filter(|f| !f.ends_with('/'))
        .collect()
}

/// `R.vizdaw` at 48 kHz 24-bit: "Song" with "Keys" (two notes with
/// per-note expression on the built-in synth, its strip at −3 dB) and
/// "Stereo" (regions A at tick 0 and B a half region later, both
/// `Media/stereo.caf`, B crossfading over A for 1 200 frames; its audio
/// path at −6 dB with a −10 dB send to the bus "Verb"), a tempo ramp over
/// bars 2 to 4, two markers and song info.
fn rich() -> Project {
    use core_model::command::{AudioCmd, CrossfadeLength, MixerCmd, SequenceCmd, SettingsCmd};
    use core_model::document::SongInfo;
    use core_model::event::{EventBody, EventSpec, FadeShape, PerNoteCtl, PerNoteKind};
    use core_model::mixer::AuxSend;
    use core_model::sequence::Marker;
    use core_model::time::Tick;
    let dir = tempfile::tempdir().unwrap();
    let pkg = dir.path().join("R.vizdaw");
    let mut b = Builder::new();
    let synth = b.dest("Synth", "builtin:synth", 0).unwrap();
    let (seq, keys) = b.sequence("Song", "Keys", Some(synth)).unwrap();
    let note = |pitch: u8, bend: u32| EventBody::Note {
        pitch,
        velocity: 0x8000,
        release_velocity: 0,
        duration: Tick(Q),
        attribute: None,
        per_note: [
            PerNoteCtl {
                kind: PerNoteKind::PitchBend,
                offset: Tick(0),
                value: bend,
            },
            PerNoteCtl {
                kind: PerNoteKind::Registered(74),
                offset: Tick(Q / 4),
                value: 0x4000_0000,
            },
        ]
        .into_iter()
        .collect(),
    };
    b.events(
        seq,
        keys,
        vec![
            EventSpec::new(Tick(0), note(60, 0x9000_0000)),
            EventSpec::new(Tick(2 * Q), note(64, 0x7000_0000)),
        ],
    )
    .unwrap();
    let stereo = b.lane(seq, "Stereo", None).unwrap();
    let m = b.media("Media/stereo.caf", FRAMES, 2).unwrap();
    // 4 800 frames are 0.1 s, 3 072 ticks at 120 BPM: B starts 2 400
    // frames into A.
    b.region(
        seq,
        stereo,
        0,
        m,
        0,
        FRAMES,
        0.0,
        Fade::default(),
        Fade::default(),
    )
    .unwrap();
    let second = b
        .region(
            seq,
            stereo,
            1_536,
            m,
            0,
            FRAMES,
            0.0,
            Fade::default(),
            Fade::default(),
        )
        .unwrap();
    b.run(Command::Audio(AudioCmd::Crossfade {
        event: second,
        length: CrossfadeLength::Frames(1_200),
        shape: FadeShape::Linear,
    }))
    .unwrap();
    b.tempo(
        seq,
        &[
            (0, 120, false),
            (core_engine::fixtures::BAR, 120, true),
            (3 * core_engine::fixtures::BAR, 90, false),
        ],
    )
    .unwrap();
    b.run(Command::Sequence(SequenceCmd::SetMarkers {
        seq,
        markers: vec![
            Marker {
                tick: Tick(0),
                name: "Verse".into(),
            },
            Marker {
                tick: Tick(2 * core_engine::fixtures::BAR),
                name: "Chorus".into(),
            },
        ]
        .into_iter()
        .collect(),
    }))
    .unwrap();
    b.run(Command::Settings(SettingsCmd::SetInfo {
        info: SongInfo {
            title: Some("Rich".into()),
            artist: Some("Someone".into()),
            ..SongInfo::default()
        },
    }))
    .unwrap();
    let synth_strip = strip_of(b.doc(), |k| matches!(k, StripKind::BuiltinSynth))[0];
    let path = strip_of(b.doc(), |k| matches!(k, StripKind::AudioPath(_)))[0];
    let verb = b.bus("Verb").unwrap();
    for (strip, gain_db) in [(synth_strip, -3.0), (path, -6.0)] {
        b.mixer(MixerCmd::SetGain { strip, gain_db }).unwrap();
    }
    b.mixer(MixerCmd::AddSend {
        strip: path,
        send: AuxSend {
            level_db: -10.0,
            ..AuxSend::new(verb)
        },
    })
    .unwrap();
    let doc = b.finish();
    core_model::file::save(&doc, &pkg).unwrap();
    write_caf(&pkg.join("Media/stereo.caf"), 2);
    let s = Session::open(text(&pkg)).unwrap();
    s.debug_set_trash_dir(Some(text(&dir.path().join("Trash"))));
    s.debug_free_bytes(None).unwrap();
    let out = dir.path().join("out");
    std::fs::create_dir(&out).unwrap();
    Project {
        dir,
        pkg,
        s,
        seq: seq.0,
        out,
    }
}

/// A package at `dir/<stem>.dawproject` holding `entries` (XML deflated,
/// the rest stored).
fn zip_of(dir: &Path, stem: &str, entries: &[(&str, &[u8])]) -> PathBuf {
    let path = dir.join(format!("{stem}.dawproject"));
    let mut z = core_io::zip::ZipWriter::new(std::fs::File::create(&path).unwrap());
    for (name, bytes) in entries {
        let method = if name.ends_with(".xml") {
            EntryMethod::Deflated
        } else {
            EntryMethod::Stored
        };
        z.add(name, method, bytes).unwrap();
    }
    z.finish().unwrap();
    path
}

/// `project.xml` with one audio track "A" whose clips, 4 beats apart, play
/// mono files at `paths` (`external` for the ones outside the package).
fn audio_project(paths: &[(&str, bool)]) -> String {
    let clips: String = paths
        .iter()
        .enumerate()
        .map(|(k, (p, ext))| {
            format!(
                r#"<Clip time="{}" duration="1" contentTimeUnit="seconds" playStart="0" playStop="0.25"><Audio sampleRate="48000" channels="1" duration="0.5"><File path="{p}" external="{ext}"/></Audio></Clip>"#,
                k * 4
            )
        })
        .collect();
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<Project version="1.0">
  <Application name="Fixture" version="1"/>
  <Transport><Tempo id="tempo" unit="bpm" value="120"/></Transport>
  <Structure>
    <Track id="m" name="Master" contentType="audio notes"><Channel id="mc" role="master"/></Track>
    <Track id="a" name="A" contentType="audio"><Channel id="ac" role="regular" destination="mc"/></Track>
  </Structure>
  <Arrangement><Lanes timeUnit="beats"><Lanes track="a"><Clips>{clips}</Clips></Lanes></Lanes></Arrangement>
</Project>"#
    )
}

/// L8, Ruling 16: the free function reads a file with no session: our
/// own export says where it came from; another DAW's file does not.
#[test]
fn info_reads_rate_tracks_and_return() {
    let _g = serial();
    let p = project();
    let dest = p.out.join("x.dawproject");
    export(&p.s, req(p.seq, &dest));
    let info = vizdaw_ffi::dawproject_info(text(&dest)).unwrap();
    let snap = snapshots(&p.pkg);
    assert_eq!(snap.len(), 1);
    assert_eq!(
        info,
        DawprojectInfoRow {
            sample_rate: 48_000,
            tracks: 2,
            audio_files: 2,
            from_vizdaw: true,
            export_id: Some(snap[0].trim_end_matches(".json").to_owned()),
        }
    );

    let project = std::fs::read(format!("{FIXTURES}/hand_other.project.xml")).unwrap();
    let metadata = std::fs::read(format!("{FIXTURES}/hand_other.metadata.xml")).unwrap();
    let other = zip_of(
        &p.out,
        "Other",
        &[("project.xml", &project), ("metadata.xml", &metadata)],
    );
    let info = vizdaw_ffi::dawproject_info(text(&other)).unwrap();
    assert!(!info.from_vizdaw);
    assert_eq!(info.export_id, None);
    assert_eq!(info.sample_rate, 48_000);
    assert_eq!(info.tracks, 3);
}

/// Spec §6.1, Ruling 17: an export comes back as a new project with its
/// tracks, notes and expression, online audio, crossfade, tempo, markers,
/// song info, levels and send, and nothing to undo.
#[test]
fn import_as_new_project() {
    let _g = serial();
    let p = rich();
    let dest = p.out.join("Rich.dawproject");
    export(&p.s, req(p.seq, &dest));
    let t = target();
    let j = import(&t, &dest);
    assert_eq!(j.state, JobStateRow::Done, "{j:?}");
    let report = j.report.clone().expect("a report");
    assert!(
        report.summary.starts_with("Imported 2 tracks, "),
        "{}",
        report.summary
    );
    assert_eq!(
        items(
            &report,
            ReportKindRow::Note,
            core_io::dawproject::import::FROM_VIZDAW
        ),
        1
    );
    assert_eq!(t.s.interchange_report().unwrap(), Some(report));
    assert_eq!(t.s.snapshot().unwrap().undo_label, None, "nothing to undo");

    let a = core_io::dawproject::record::projection(&doc_of(&p.s), SeqId(p.seq)).unwrap();
    let i = core_io::dawproject::record::projection(&doc_of(&t.s), first_seq(&t.s)).unwrap();
    let names = |x: &core_io::dawproject::record::Projection| {
        x.tracks.iter().map(|t| t.name.clone()).collect::<Vec<_>>()
    };
    assert_eq!(names(&i), ["Keys", "Stereo"]);
    assert_eq!(names(&i), names(&a));
    // Notes and their expression.
    assert_eq!(i.tracks[0].notes_played, a.tracks[0].notes_played);
    assert!(
        i.tracks[0]
            .notes_played
            .iter()
            .all(|n| n.expression.len() == 2)
    );
    // The regions, online, with the source's audio; B's crossfade.
    let hash = core_media::caf::hash_file(&p.pkg.join("Media/stereo.caf")).unwrap();
    let (ra, ri) = (&a.tracks[1].regions, &i.tracks[1].regions);
    assert_eq!(ri.len(), 2);
    for (x, y) in ra.iter().zip(ri) {
        assert_eq!(
            (x.tick, x.offset, x.length, x.fade_in, x.crossfade),
            (y.tick, y.offset, y.length, y.fade_in, y.crossfade)
        );
        assert_eq!(y.media_hash.as_deref(), Some(hash.as_str()));
    }
    assert!(!ri[0].crossfade && ri[1].crossfade);
    let media = t.s.media().unwrap();
    assert_eq!(media.len(), 1);
    assert!(media.iter().all(|m| m.offline.is_none()), "{media:?}");
    // The maps, markers and song info.
    assert_eq!(i.tempo, a.tempo);
    assert!(i.tempo.iter().any(|p| p.2), "the ramp");
    assert_eq!(i.markers, a.markers);
    assert_eq!(i.info, a.info);
    // The levels and the send.
    let strip = |x: &core_io::dawproject::record::Projection, k: usize| {
        x.strips[x.tracks[k].strip.unwrap()].clone()
    };
    for k in 0..2 {
        assert!((strip(&i, k).gain_db - strip(&a, k).gain_db).abs() < 1e-9);
    }
    assert!((strip(&i, 0).gain_db + 3.0).abs() < 1e-9);
    let sends = |x: &core_io::dawproject::record::Projection| {
        let s = strip(x, 1);
        s.sends
            .iter()
            .map(|(to, db, pre, on)| (x.strips[*to].name.clone(), *db, *pre, *on))
            .collect::<Vec<_>>()
    };
    let (sa, si) = (sends(&a), sends(&i));
    assert_eq!(si.len(), 1);
    assert_eq!(si[0].0, "Verb");
    assert!((si[0].1 - sa[0].1).abs() < 1e-9 && (si[0].1 + 10.0).abs() < 1e-9);
    assert_eq!((si[0].2, si[0].3), (sa[0].2, sa[0].3));
}

/// Spec §12, L4: a CLAP's settings go out in the file and come back into
/// the new project's plug-in.
#[test]
fn clap_state_round_trips() {
    let _g = serial();
    let p = project();
    let slot = p.s.plugin_slots().unwrap()[0].id;
    // The value reaches the plug-in while it processes.
    p.s.play(p.seq).unwrap();
    idle_until(&p.s, |s| s.transport().unwrap().playing);
    p.s.set_param(slot, 0, 0.25).unwrap();
    idle_until(&p.s, |s| {
        s.params(slot)
            .unwrap()
            .iter()
            .any(|r| r.id == 0 && (r.value - 0.25).abs() < 1e-9)
    });
    p.s.stop().unwrap();
    let dest = p.out.join("x.dawproject");
    export(&p.s, req(p.seq, &dest));
    let t = target();
    let j = import(&t, &dest);
    assert_eq!(j.state, JobStateRow::Done, "{j:?}");
    let gain = |s: &Session| {
        s.plugin_slots()
            .unwrap()
            .into_iter()
            .find(|r| r.plugin.id == GAIN_ID && r.status == SlotStatus::Live)
    };
    idle_until(&t.s, |s| gain(s).is_some());
    let new = gain(&t.s).unwrap().id;
    let v = t.s.debug_plugin_param(new, 0).unwrap();
    assert!((v - 0.25).abs() < 1e-9, "{v}");
}

/// Lessons, S10: a cancel before the run and after each of stages 5–8
/// leaves `Media/` and the document as they were.
#[test]
fn cancel_at_every_stage_leaves_media_unchanged() {
    let _g = serial();
    let p = project();
    let dest = p.out.join("x.dawproject");
    export(&p.s, req(p.seq, &dest));
    let t = target();
    let before = media_files(&t.pkg);
    let doc = t.s.debug_document_json();
    // Before the run.
    t.s.debug_free_bytes(None).unwrap();
    t.s.debug_hold_jobs(true).unwrap();
    let id = t.s.import_dawproject(import_req(&dest)).unwrap();
    t.s.cancel_job(id).unwrap();
    t.s.debug_hold_jobs(false).unwrap();
    assert_eq!(wait_job(&t.s, id).state, JobStateRow::Cancelled);
    assert_eq!(media_files(&t.pkg), before);
    assert_eq!(t.s.debug_document_json(), doc);
    for stage in 5..=8 {
        t.s.debug_free_bytes(None).unwrap();
        t.s.debug_dawproject_cancel_at(Some(stage)).unwrap();
        let j = import(&t, &dest);
        assert_eq!(j.state, JobStateRow::Cancelled, "stage {stage}: {j:?}");
        assert_eq!(media_files(&t.pkg), before, "stage {stage}");
        assert_eq!(t.s.debug_document_json(), doc, "stage {stage}");
    }
    // The hook is used once.
    assert_eq!(import(&t, &dest).state, JobStateRow::Done);
}

/// Ruling 19: an entry that is absent or not audio, an external file that
/// is not a regular file and a URL each become an offline item with its
/// region, listed.
#[test]
fn missing_and_undecodable_audio_become_offline() {
    let _g = serial();
    let t = target();
    let dir = t.dir.path().join("src");
    std::fs::create_dir(&dir).unwrap();
    let fifo = dir.join("pipe.wav");
    assert!(
        std::process::Command::new("mkfifo")
            .arg(&fifo)
            .status()
            .unwrap()
            .success()
    );
    let fifo_text = text(&fifo);
    let xml = audio_project(&[
        ("audio/absent.wav", false),
        ("audio/junk.wav", false),
        (&fifo_text, true),
        ("http://x/a.wav", true),
    ]);
    let junk: Vec<u8> = (0..1024u32).map(|i| (i * 7 % 251) as u8).collect();
    let path = zip_of(
        &dir,
        "Bad",
        &[("project.xml", xml.as_bytes()), ("audio/junk.wav", &junk)],
    );
    let j = import(&t, &path);
    assert_eq!(j.state, JobStateRow::Done, "{j:?}");
    let report = j.report.unwrap();
    // core-io lists them as a Note (Task 7): the regions are kept.
    let d = details(
        &report,
        ReportKindRow::Note,
        core_io::dawproject::import::OFFLINE,
    );
    assert_eq!(d.len(), 4, "{report:?}");
    assert_eq!(
        d[0],
        "absent: 'audio/absent.wav' isn't in the DAWproject file"
    );
    // The decoder's own words, naming the entry, not the extracted copy.
    assert!(
        d[1].starts_with("junk: ") && d[1].contains("junk.wav") && !d[1].contains(".incoming"),
        "{}",
        d[1]
    );
    assert_eq!(d[2], "pipe: it is not a regular file");
    assert_eq!(d[3], "a: it is a URL, not a file");
    let media = t.s.media().unwrap();
    assert_eq!(media.len(), 4);
    assert!(media.iter().all(|m| m.offline.is_some()), "{media:?}");
    let doc = doc_of(&t.s);
    let regions = doc
        .sequence(first_seq(&t.s))
        .unwrap()
        .lanes
        .iter()
        .flat_map(|l| l.takes.iter())
        .flat_map(|k| k.events.values())
        .filter(|e| matches!(e.body, core_model::event::EventBody::AudioRegion { .. }))
        .count();
    assert_eq!(regions, 4);
    assert!(media_files(&t.pkg).is_empty(), "no audio was written");
}

/// Ruling 18: an import that would not fit fails before writing anything.
#[test]
fn import_not_enough_space_refuses() {
    let _g = serial();
    let p = project();
    let dest = p.out.join("x.dawproject");
    export(&p.s, req(p.seq, &dest));
    let t = target();
    t.s.debug_free_bytes(Some(10)).unwrap();
    let j = import(&t, &dest);
    assert_eq!(j.state, JobStateRow::Failed);
    let m = j.message.unwrap();
    assert!(
        m.starts_with("Not enough free space to import this file (it needs ")
            && m.ends_with(" MB)."),
        "{m}"
    );
    assert!(media_files(&t.pkg).is_empty());
    assert!(t.s.lanes(first_seq(&t.s).0).unwrap().is_empty());
    t.s.debug_free_bytes(None).unwrap();
}

/// An 8-bit mono WAV at 8 kHz holding `frames` frames of silence.
fn wav8(frames: u32) -> Vec<u8> {
    let mut w = b"RIFF".to_vec();
    w.extend_from_slice(&(36 + frames).to_le_bytes());
    w.extend_from_slice(b"WAVEfmt ");
    w.extend_from_slice(&16u32.to_le_bytes());
    w.extend_from_slice(&1u16.to_le_bytes()); // PCM
    w.extend_from_slice(&1u16.to_le_bytes()); // mono
    w.extend_from_slice(&8000u32.to_le_bytes());
    w.extend_from_slice(&8000u32.to_le_bytes());
    w.extend_from_slice(&1u16.to_le_bytes());
    w.extend_from_slice(&8u16.to_le_bytes());
    w.extend_from_slice(b"data");
    w.extend_from_slice(&frames.to_le_bytes());
    w.extend(std::iter::repeat_n(0x80u8, frames as usize));
    w
}

/// A mono 16-bit 48 kHz FLAC of `blocks` silent blocks of 4096 frames
/// (verbatim subframes) whose header gives no length (total samples 0).
fn flac_of_unknown_length(blocks: u8) -> Vec<u8> {
    fn crc8(b: &[u8]) -> u8 {
        b.iter().fold(0u8, |mut c, x| {
            c ^= x;
            for _ in 0..8 {
                c = if c & 0x80 != 0 {
                    (c << 1) ^ 0x07
                } else {
                    c << 1
                };
            }
            c
        })
    }
    fn crc16(b: &[u8]) -> u16 {
        b.iter().fold(0u16, |mut c, x| {
            c ^= u16::from(*x) << 8;
            for _ in 0..8 {
                c = if c & 0x8000 != 0 {
                    (c << 1) ^ 0x8005
                } else {
                    c << 1
                };
            }
            c
        })
    }
    assert!(blocks < 128);
    let mut out = b"fLaC".to_vec();
    // STREAMINFO, the last metadata block: blocks of 4096, frame sizes
    // unknown, then 48000 Hz (20 bits), mono (3), 16 bits (5) and total
    // samples 0 (36), and no MD5.
    out.extend_from_slice(&[0x80, 0, 0, 34]);
    out.extend_from_slice(&[0x10, 0x00, 0x10, 0x00, 0, 0, 0, 0, 0, 0]);
    let packed: u64 = (48_000u64 << 44) | (15u64 << 36);
    out.extend_from_slice(&packed.to_be_bytes());
    out.extend_from_slice(&[0; 16]);
    for k in 0..blocks {
        // Sync and fixed blocks; 4096 frames (12), 48 kHz (10); mono,
        // 16 bits; the frame number.
        let mut frame = vec![0xFF, 0xF8, 0xCA, 0x08, k];
        frame.push(crc8(&frame));
        frame.push(0x02); // a verbatim subframe
        frame.extend(std::iter::repeat_n(0u8, 4096 * 2));
        let c = crc16(&frame);
        frame.extend_from_slice(&c.to_be_bytes());
        out.extend_from_slice(&frame);
    }
    out
}

/// Final review I1: the free space is checked again once each file is
/// probed, so a file whose stated duration understates it (here an 8 kHz
/// 8-bit WAV said to last half a second, which converts to 18 times its
/// size) cannot fill the disk; nothing is left.
#[cfg(target_os = "macos")]
#[test]
fn import_rechecks_the_space_from_the_probe() {
    let _g = serial();
    let t = target();
    let dir = t.dir.path().join("src");
    std::fs::create_dir(&dir).unwrap();
    let path = zip_of(
        &dir,
        "Short",
        &[
            (
                "project.xml",
                audio_project(&[("audio/a.wav", false)]).as_bytes(),
            ),
            ("audio/a.wav", &wav8(100_000)),
        ],
    );
    // The file's own numbers need about 0.5 MB; the conversion about 2.
    t.s.debug_free_bytes(Some(1_000_000)).unwrap();
    let j = import(&t, &path);
    t.s.debug_free_bytes(None).unwrap();
    assert_eq!(j.state, JobStateRow::Failed, "{j:?}");
    assert_eq!(
        j.message.as_deref(),
        Some("Not enough free space to import this file (it needs 3 MB).")
    );
    assert!(media_files(&t.pkg).is_empty(), "{:?}", media_files(&t.pkg));
    assert!(t.s.lanes(first_seq(&t.s).0).unwrap().is_empty());
    // With room, the same file imports.
    let j = import(&t, &path);
    assert_eq!(j.state, JobStateRow::Done, "{j:?}");
}

/// Final review I1: a conversion stops at its frame budget (the probe's
/// frames at the project rate plus a second): a FLAC whose header gives
/// no length decodes far past it, so the import stops and leaves nothing.
#[cfg(target_os = "macos")]
#[test]
fn import_stops_a_file_longer_than_it_says() {
    let _g = serial();
    let t = target();
    let dir = t.dir.path().join("src");
    std::fs::create_dir(&dir).unwrap();
    let path = zip_of(
        &dir,
        "Long",
        &[
            (
                "project.xml",
                audio_project(&[("audio/a.flac", false)]).as_bytes(),
            ),
            ("audio/a.flac", &flac_of_unknown_length(30)),
        ],
    );
    let j = import(&t, &path);
    assert_eq!(j.state, JobStateRow::Failed, "{j:?}");
    assert_eq!(
        j.message.as_deref(),
        Some(vizdaw_ffi::dawproject::audio_longer_than_it_says("a.flac").as_str())
    );
    assert!(media_files(&t.pkg).is_empty(), "{:?}", media_files(&t.pkg));
    assert!(t.s.lanes(first_seq(&t.s).0).unwrap().is_empty());
}

/// Ruling 18: what is not a DAWproject file version 1, or leaves the
/// package, fails with the reader's sentence and writes nothing.
#[test]
fn import_refused_files() {
    let _g = serial();
    let t = target();
    let dir = t.dir.path().join("src");
    std::fs::create_dir(&dir).unwrap();
    let not_zip = dir.join("plain.dawproject");
    std::fs::write(&not_zip, b"this is not a ZIP file").unwrap();
    let v2 = zip_of(
        &dir,
        "V2",
        &[(
            "project.xml",
            audio_project(&[])
                .replace(r#"version="1.0""#, r#"version="2.0""#)
                .as_bytes(),
        )],
    );
    // A name that leaves the package: written as "aa/evil.wav", then
    // patched in the local header and the directory (the CRC covers the
    // data only).
    let evil = zip_of(
        &dir,
        "Evil",
        &[
            (
                "project.xml",
                audio_project(&[("audio/a.wav", false)]).as_bytes(),
            ),
            ("aa/evil.wav", b"RIFF"),
        ],
    );
    let mut bytes = std::fs::read(&evil).unwrap();
    let mut patched = 0;
    for k in 0..bytes.len().saturating_sub(11) {
        if &bytes[k..k + 11] == b"aa/evil.wav" {
            bytes[k..k + 3].copy_from_slice(b"../");
            patched += 1;
        }
    }
    assert_eq!(patched, 2);
    std::fs::write(&evil, &bytes).unwrap();
    for (path, what) in [
        (&not_zip, "not a ZIP"),
        (&v2, "version 2"),
        (&evil, "a name with .."),
    ] {
        let refusal = match vizdaw_ffi::dawproject_info(text(path)) {
            Err(VizError::Model { message }) => message,
            other => panic!("{what}: {other:?}"),
        };
        let j = import(&t, path);
        assert_eq!(j.state, JobStateRow::Failed, "{what}: {j:?}");
        assert_eq!(j.message.as_deref(), Some(refusal.as_str()), "{what}");
        assert!(media_files(&t.pkg).is_empty(), "{what}");
        assert!(t.s.lanes(first_seq(&t.s).0).unwrap().is_empty(), "{what}");
    }
    let r = vizdaw_ffi::dawproject_info(text(&v2));
    assert!(
        matches!(&r, Err(VizError::Model { message }) if message.contains("version 2.0")),
        "{r:?}"
    );
}

/// Final review C1: an AU state that is a binary property list (here a
/// few hundred bytes whose shared arrays would write out terabytes) is
/// refused before it is converted: the import ends at once with the
/// plug-in in place, no settings, and a Left out line saying why.
#[cfg(target_os = "macos")]
#[test]
fn binary_au_state_is_refused_unconverted() {
    let _g = serial();
    let t = target();
    let dir = t.dir.path().join("src");
    std::fs::create_dir(&dir).unwrap();
    // `{k: A0}`, each `Ai = [Ai+1, Ai+1]`, 40 deep (bplist00, one-byte
    // offsets and references).
    let mut bomb = b"bplist00".to_vec();
    let mut offsets = Vec::new();
    let mut objects = vec![vec![0xD1u8, 1, 2], vec![0x51, b'k']];
    for i in 0..40u8 {
        objects.push(vec![0xA2, 3 + i, 3 + i]);
    }
    objects.push(vec![0xA0]);
    for o in &objects {
        offsets.push(u8::try_from(bomb.len()).unwrap());
        bomb.extend_from_slice(o);
    }
    let table = bomb.len() as u64;
    bomb.extend_from_slice(&offsets);
    bomb.extend_from_slice(&[0, 0, 0, 0, 0, 0, 1, 1]);
    bomb.extend_from_slice(&(objects.len() as u64).to_be_bytes());
    bomb.extend_from_slice(&0u64.to_be_bytes());
    bomb.extend_from_slice(&table.to_be_bytes());
    assert!(bomb.len() < 1024);
    let project = audio_project(&[]).replace(
        r#"<Channel id="ac" role="regular" destination="mc"/>"#,
        r#"<Channel id="ac" role="regular" destination="mc"><Devices><AuPlugin id="d1" name="Delay" deviceID="aufx/dely/xmpl" deviceName="Delay" deviceRole="audioFX"><State path="plugins/d.aupreset"/></AuPlugin></Devices></Channel>"#,
    );
    assert!(project.contains("AuPlugin"));
    let path = zip_of(
        &dir,
        "Bomb",
        &[
            ("project.xml", project.as_bytes()),
            ("plugins/d.aupreset", &bomb),
        ],
    );
    let start = std::time::Instant::now();
    let j = import(&t, &path);
    assert!(start.elapsed() < std::time::Duration::from_secs(10));
    assert_eq!(j.state, JobStateRow::Done, "{j:?}");
    let report = j.report.expect("a report");
    assert_eq!(
        details(
            &report,
            ReportKindRow::LeftOut,
            core_io::dawproject::import::PLUGIN_SETTINGS
        ),
        ["Delay: the settings are not an XML property list"]
    );
    let doc = doc_of(&t.s);
    let slots: Vec<_> = doc.mixer().inserts().map(|(_, _, p)| p.clone()).collect();
    assert_eq!(slots.len(), 1, "{slots:?}");
    assert_eq!(slots[0].plugin.id, "aufx/dely/xmpl");
    assert_eq!(slots[0].state, None);
}

/// Ruling 17: a project with tracks is refused at once.
#[test]
fn into_a_project_with_tracks_is_refused() {
    let _g = serial();
    let p = project();
    let dest = p.out.join("x.dawproject");
    export(&p.s, req(p.seq, &dest));
    match p.s.import_dawproject(import_req(&dest)) {
        Err(VizError::Model { message }) => {
            assert_eq!(message, core_io::dawproject::import::NOT_NEW)
        }
        other => panic!("{other:?}"),
    }
}

/// L1: one interchange job at a time; a media job, an open edit and
/// recording refuse.
#[test]
fn import_busy_states_refuse() {
    let _g = serial();
    let p = project();
    let dest = p.out.join("x.dawproject");
    export(&p.s, req(p.seq, &dest));
    let t = target();
    let seq = first_seq(&t.s).0;
    // A running export.
    t.s.debug_hold_jobs(true).unwrap();
    let e =
        t.s.export_dawproject(req(seq, &p.out.join("y.dawproject")))
            .unwrap();
    busy(t.s.import_dawproject(import_req(&dest)), "an export");
    t.s.cancel_job(e).unwrap();
    t.s.debug_hold_jobs(false).unwrap();
    wait_job(&t.s, e);
    // A Convert job.
    t.s.debug_hold_jobs(true).unwrap();
    let c = t.s.convert_media().unwrap();
    busy(t.s.import_dawproject(import_req(&dest)), "a Convert job");
    t.s.cancel_job(c).unwrap();
    t.s.debug_hold_jobs(false).unwrap();
    wait_job(&t.s, c);
    // A running import holds exports, MIDI imports and media jobs.
    t.s.debug_hold_jobs(true).unwrap();
    let i = t.s.import_dawproject(import_req(&dest)).unwrap();
    busy(t.s.import_dawproject(import_req(&dest)), "another import");
    busy(
        t.s.export_dawproject(req(seq, &p.out.join("z.dawproject"))),
        "an export under an import",
    );
    busy(t.s.convert_media(), "a conversion under an import");
    t.s.cancel_job(i).unwrap();
    t.s.debug_hold_jobs(false).unwrap();
    assert_eq!(wait_job(&t.s, i).state, JobStateRow::Cancelled);
    // An open edit.
    t.s.begin_edit("Drag".into()).unwrap();
    busy(t.s.import_dawproject(import_req(&dest)), "an open edit");
    t.s.end_edit().unwrap();
    // Recording (offline, an armed track): Busy comes before the track
    // refusal.
    let mut b = Builder::new();
    let synth = b.dest("Synth", "builtin:synth", 0).unwrap();
    let (rseq, keys) = b.sequence("S", "Keys", Some(synth)).unwrap();
    b.run(Command::Lane(LaneCmd::SetInput {
        seq: rseq,
        lane: keys,
        input: Some(LaneInput::Midi {
            port: Some("Keys".into()),
            channel: None,
        }),
    }))
    .unwrap();
    b.run(Command::Lane(LaneCmd::SetFlags {
        seq: rseq,
        lane: keys,
        mute: None,
        solo: None,
        arm: Some(true),
    }))
    .unwrap();
    let r = Session::from_document(b.finish()).unwrap();
    r.debug_set_trash_dir(Some(text(&p.dir.path().join("Trash"))));
    r.record(
        rseq.0,
        RecordSpecRow {
            replace: true,
            punch: None,
            looped: false,
            count_in_bars: 0,
            pre_roll_bars: 0,
            passes_as_takes: true,
        },
    )
    .unwrap();
    let until = Instant::now() + common::wait_limit();
    while !r.record_state().unwrap().recording {
        assert!(Instant::now() < until, "recording did not start");
        std::thread::sleep(Duration::from_millis(5));
    }
    busy(r.import_dawproject(import_req(&dest)), "recording");
    r.stop().unwrap();
    r.close().unwrap();
    assert!(media_files(&t.pkg).is_empty());
}

/// Hostile paths: `Model` or `Io` (never `Internal`) from the info read
/// and from the import, which may also fail its job with the reader's
/// sentence; nothing is written.
#[test]
fn import_hostile_requests() {
    let _g = serial();
    let t = target();
    let out = t.dir.path().join("out");
    std::fs::create_dir(&out).unwrap();
    let fifo = out.join("fifo.dawproject");
    assert!(
        std::process::Command::new("mkfifo")
            .arg(&fifo)
            .status()
            .unwrap()
            .success()
    );
    let lp = out.join("loop.dawproject");
    std::os::unix::fs::symlink(&lp, &lp).unwrap();
    let folder = out.join("dir.dawproject");
    std::fs::create_dir(&folder).unwrap();
    let device = out.join("zero.dawproject");
    std::os::unix::fs::symlink("/dev/zero", &device).unwrap();
    let long = format!("/{}.dawproject", "a/".repeat(2100));
    let paths: Vec<(String, &str)> = vec![
        (String::new(), "empty"),
        ("relative.dawproject".into(), "relative"),
        (text(&folder), "a folder"),
        (text(&fifo), "a FIFO"),
        ("/dev/zero".into(), "/dev/zero"),
        (text(&device), "a link to /dev/zero"),
        (text(&lp), "a symlink loop"),
        (long, "over 4 KiB"),
        (format!("{}/a\0b.dawproject", text(&out)), "a NUL"),
        (format!("{}/../x.dawproject", text(&out)), "a .. component"),
        (text(&t.pkg.join("in.dawproject")), "inside the package"),
        (text(&out.join("missing.zip")), "missing, ending .zip"),
    ];
    let fine = |r: &VizError| matches!(r, VizError::Model { .. } | VizError::Io { .. });
    for (path, what) in &paths {
        match vizdaw_ffi::dawproject_info(path.clone()) {
            Err(e) => assert!(fine(&e), "info, {what}: {e:?}"),
            Ok(i) => panic!("info, {what}: {i:?}"),
        }
        match t
            .s
            .import_dawproject(DawprojectImportRequestRow { path: path.clone() })
        {
            Err(e) => assert!(fine(&e), "import, {what}: {e:?}"),
            Ok(id) => {
                let j = wait_job(&t.s, id);
                assert_eq!(j.state, JobStateRow::Failed, "import, {what}: {j:?}");
                let m = j.message.unwrap_or_default();
                assert!(!m.starts_with("internal error"), "import, {what}: {m}");
            }
        }
    }
    assert!(media_files(&t.pkg).is_empty());
    assert!(t.s.lanes(first_seq(&t.s).0).unwrap().is_empty());
}
