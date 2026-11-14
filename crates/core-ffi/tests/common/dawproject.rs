//! The DAWproject tests' shared fixtures (plan 6d Tasks 9 and 10, moved
//! here by plan 6e Task 8 so the import, export and merge tests use one
//! copy): the serial guard, the saved project with two audio tracks and
//! the test gain CLAP, the job waits, listings and the request row.
#![allow(dead_code)]

use core_engine::fixtures::Builder;
use core_media::caf::{CafWriter, WriteMode};
use core_model::document::Document;
use core_model::event::Fade;
use core_model::format::{AudioFormat, BitDepth};
use core_model::ids::{MediaId, SeqId, StripId};
use core_model::mixer::{PluginFormat, PluginRef, StripKind};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};
use vizdaw_ffi::host::{force_offline, reset_host};
use vizdaw_ffi::{
    BlockRule, DawprojectExportRequestRow, JobKindRow, JobRow, JobStateRow, RegionGainRow, Session,
    SlotStatus,
};

pub static SERIAL: Mutex<()> = Mutex::new(());

pub fn serial() -> MutexGuard<'static, ()> {
    let g = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    force_offline();
    vizdaw_ffi::host::set_offline_rate(48_000.0);
    reset_host();
    vizdaw_ffi::catalog::add_test_bundle(&test_plugins::bundle()).unwrap();
    g
}

pub const FRAMES: u64 = 4_800;
pub const GAIN_ID: &str = "vizdaw.test.gain";

/// Channel `c` of the media at frame `f`: a saw well inside full scale.
pub fn sample(f: u64, c: u64) -> f32 {
    ((f * 37 + c * 101) % 1000) as f32 / 1000.0 * 0.8 - 0.4
}

pub fn write_caf(path: &Path, channels: u16) {
    let mut w =
        CafWriter::create(path, 48_000, channels, BitDepth::Int24, WriteMode::Normal).unwrap();
    let samples: Vec<f32> = (0..FRAMES)
        .flat_map(|f| (0..u64::from(channels)).map(move |c| sample(f, c)))
        .collect();
    w.write(&samples).unwrap();
    w.finish().unwrap();
}

pub fn plugin(id: &str, name: &str) -> PluginRef {
    PluginRef {
        format: PluginFormat::Clap,
        id: id.into(),
        name: name.into(),
        vendor: "VizDAW".into(),
        version: "1.0.0".into(),
    }
}

pub fn strip_of(d: &Document, want: impl Fn(&StripKind) -> bool) -> Vec<StripId> {
    d.mixer()
        .order()
        .iter()
        .copied()
        .filter(|s| d.mixer().strip(*s).is_ok_and(|x| want(&x.kind)))
        .collect()
}

pub fn text(p: &Path) -> String {
    p.display().to_string()
}

pub struct Project {
    pub dir: tempfile::TempDir,
    pub pkg: PathBuf,
    pub s: Arc<Session>,
    pub seq: u64,
    /// Where exports go (outside the package).
    pub out: PathBuf,
}

/// `P.vizdaw` at 48 kHz 24-bit: "Song" with "Mono" and "Stereo" each
/// playing their media from tick 0, the gain plug-in on their audio path.
/// `extra` adds to the document before it is saved.
pub fn project_with(extra: impl FnOnce(&mut Builder, SeqId, MediaId, StripId)) -> Project {
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

pub fn project() -> Project {
    project_with(|_, _, _, _| {})
}

pub fn req(seq: u64, dest: &Path) -> DawprojectExportRequestRow {
    DawprojectExportRequestRow {
        seq,
        path: text(dest),
        replace: false,
        region_gain: RegionGainRow::Apply,
        blocks: BlockRule::WholeLane,
    }
}

pub fn wait_job(s: &Session, id: u64) -> JobRow {
    let until = Instant::now() + super::wait_limit();
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

pub fn idle_until(s: &Session, mut done: impl FnMut(&Session) -> bool) {
    let until = Instant::now() + super::wait_limit();
    while !done(s) {
        assert!(Instant::now() < until, "timed out");
        s.idle().unwrap();
        std::thread::sleep(Duration::from_millis(1));
    }
}

/// Exports `r` and waits for it to end Done.
pub fn export(s: &Session, r: DawprojectExportRequestRow) -> JobRow {
    let j = wait_job(s, s.export_dawproject(r).unwrap());
    assert_eq!(j.state, JobStateRow::Done, "{j:?}");
    assert_eq!(j.kind, JobKindRow::ExportDawproject);
    j
}

/// Every entry under `dir` (hidden ones too), relative, sorted.
pub fn listing(dir: &Path) -> Vec<String> {
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
/// Sets the running test gain's parameter 0 on `slot` to `value` while the
/// project plays (it reaches the plug-in only while it processes), then
/// stops (6d's `clap_state_round_trips` setup).
pub fn set_live_gain(p: &Project, slot: u64, value: f64) {
    p.s.play(p.seq).unwrap();
    idle_until(&p.s, |s| s.transport().unwrap().playing);
    p.s.set_param(slot, 0, value).unwrap();
    idle_until(&p.s, |s| {
        s.params(slot)
            .unwrap()
            .iter()
            .any(|r| r.id == 0 && (r.value - value).abs() < 1e-9)
    });
    p.s.stop().unwrap();
}
