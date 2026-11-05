//! The Bounce job (plan 5f-2 Task 4; spec §9, §12.1, §12.3; Rulings 3–6,
//! 8–10, 12, 13; owner answers 2026-11-03). Projects are saved packages
//! in temporary folders, built with the fixtures' `Builder`, at 48 kHz
//! float, with a region of a generated CAF from tick 0 (pre-flight C2: the
//! demo's media does not exist and the offline synth is silent). One test
//! at a time: the engine host and the bounce settings are process-wide.

mod common;

use core_engine::fixtures::{Builder, Q};
use core_media::caf::{CafReader, CafWriter, WriteMode};
use core_model::automation::AutomationTarget;
use core_model::command::{Command, LaneCmd, MixerCmd, SettingsCmd};
use core_model::document::Document;
use core_model::event::{EventBody, Fade};
use core_model::format::{AudioFormat, BitDepth};
use core_model::ids::{SeqId, StripId};
use core_model::mixer::{AuxSend, Output, ParamId, PluginFormat, PluginRef, StripKind};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Instant;
use vizdaw_ffi::edit::{EditCommand, SequenceEdit};
use vizdaw_ffi::host::{force_offline, reset_host};
use vizdaw_ffi::{
    BitDepthRow, BounceRangeRow, BounceRequestRow, BounceSettingsRow, BounceSourceRow,
    ContainerRow, JobRow, JobStateRow, MediaOriginRow, ProjectFormatRow, Session, SlotStatus,
    VizError, bounce_settings, set_bounce_settings,
};

static SERIAL: Mutex<()> = Mutex::new(());

fn serial() -> MutexGuard<'static, ()> {
    let g = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    force_offline();
    reset_host();
    vizdaw_ffi::catalog::add_test_bundle(&test_plugins::bundle()).unwrap();
    set_bounce_settings(BounceSettingsRow {
        tail_cap_ms: 10_000,
    });
    g
}

const FRAMES: u64 = 480_000;
/// Four bars at 120 BPM (the Builder's tempo): 16 quarters of 15 360 ticks.
const BARS4: i64 = 16 * Q;
/// ... are 16 × 24 000 frames.
const BARS4_FRAMES: u64 = 384_000;
const STEP24: f32 = 1.0 / 8_388_608.0;

fn value(f: u64) -> f32 {
    ((f % 997) as f32 + 1.0) / 2000.0
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

fn strip_of(d: &Document, want: fn(&StripKind) -> bool) -> StripId {
    d.mixer()
        .order()
        .iter()
        .copied()
        .find(|s| d.mixer().strip(*s).is_ok_and(|x| want(&x.kind)))
        .unwrap()
}

struct Project {
    dir: tempfile::TempDir,
    pkg: PathBuf,
    s: Arc<Session>,
    seq: u64,
    /// The region's AudioPath strip.
    path: u64,
}

/// `P.vizdaw` in a temporary folder, at 48 kHz float: Song (120 BPM) with
/// lane "Audio" playing `Media/a.caf` (value(f) left, −value(f) right,
/// FRAMES frames) from tick 0 on its AudioPath strip; `extra` adds to the
/// document before it is saved. Opened, with a private Trash.
fn project(extra: impl FnOnce(&mut Builder, SeqId, StripId)) -> Project {
    let dir = tempfile::tempdir().unwrap();
    let pkg = dir.path().join("P.vizdaw");
    let mut b = Builder::new();
    b.run(Command::Settings(SettingsCmd::SetFormat {
        format: AudioFormat {
            sample_rate: 48_000,
            depth: BitDepth::Float32,
        },
        swaps: Vec::new(),
    }))
    .unwrap();
    let (seq, lane) = b.sequence("Song", "Audio", None).unwrap();
    let m = b.media("Media/a.caf", FRAMES, 2).unwrap();
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
    let path = strip_of(b.doc(), |k| matches!(k, StripKind::AudioPath(_)));
    extra(&mut b, seq, path);
    core_model::file::save(&b.finish(), &pkg).unwrap();
    let mut w = CafWriter::create(
        &pkg.join("Media/a.caf"),
        48_000,
        2,
        BitDepth::Float32,
        WriteMode::Normal,
    )
    .unwrap();
    w.write(
        &(0..FRAMES)
            .flat_map(|f| [value(f), -value(f)])
            .collect::<Vec<f32>>(),
    )
    .unwrap();
    w.finish().unwrap();
    let s = Session::open(pkg.display().to_string()).unwrap();
    s.debug_set_trash_dir(Some(dir.path().join("Trash").display().to_string()));
    Project {
        dir,
        pkg,
        s,
        seq: seq.0,
        path: path.0,
    }
}

/// A Mix of the first four bars to `dest`, 48 kHz 24-bit WAV, no dither.
fn mix_to(p: &Project, dest: &Path) -> BounceRequestRow {
    BounceRequestRow {
        seq: p.seq,
        source: BounceSourceRow::Mix,
        range: BounceRangeRow::Ticks {
            start: 0,
            end: BARS4,
        },
        container: ContainerRow::Wav,
        depth: BitDepthRow::Int24,
        rate: 48_000,
        dither: false,
        path: Some(dest.display().to_string()),
        add_to_project: false,
        replace: false,
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
        std::thread::yield_now();
    }
}

/// `(rate, channels, bits, float, frames, samples)` of a WAV (Task 1's reader).
fn read_wav(p: &Path) -> (u32, u16, u16, bool, u64, Vec<i64>) {
    let b = std::fs::read(p).unwrap();
    assert_eq!(&b[0..4], b"RIFF");
    assert_eq!(&b[8..12], b"WAVE");
    let mut at = 12;
    let (mut rate, mut ch, mut bits, mut tag) = (0, 0, 0, 0);
    let mut data = Vec::new();
    while at + 8 <= b.len() {
        let id = &b[at..at + 4];
        let n = u32::from_le_bytes(b[at + 4..at + 8].try_into().unwrap()) as usize;
        let body = &b[at + 8..at + 8 + n];
        if id == b"fmt " {
            tag = u16::from_le_bytes(body[0..2].try_into().unwrap());
            ch = u16::from_le_bytes(body[2..4].try_into().unwrap());
            rate = u32::from_le_bytes(body[4..8].try_into().unwrap());
            bits = u16::from_le_bytes(body[14..16].try_into().unwrap());
        }
        if id == b"data" {
            data = body.to_vec();
        }
        at += 8 + n + (n & 1);
    }
    let bytes = usize::from(bits / 8);
    let samples: Vec<i64> = data
        .chunks(bytes)
        .map(|c| match bytes {
            2 => i64::from(i16::from_le_bytes([c[0], c[1]])),
            3 => i64::from(i32::from_le_bytes([0, c[0], c[1], c[2]]) >> 8),
            _ => i64::from(u32::from_le_bytes([c[0], c[1], c[2], c[3]])),
        })
        .collect();
    (
        rate,
        ch,
        bits,
        tag == 3,
        (samples.len() / usize::from(ch)) as u64,
        samples,
    )
}

/// A WAV's channel `c` as floats.
fn wav_channel(p: &Path, c: usize) -> Vec<f32> {
    let (_, ch, bits, float, _, s) = read_wav(p);
    s.iter()
        .skip(c)
        .step_by(usize::from(ch))
        .map(|x| match (bits, float) {
            (16, _) => *x as f32 / 32_768.0,
            (24, _) => *x as f32 / 8_388_608.0,
            _ => f32::from_bits(*x as u32),
        })
        .collect()
}

/// A media item's left channel, from its file in the package.
fn media_left(p: &Project, id: u64) -> Vec<f32> {
    let row =
        p.s.media()
            .unwrap()
            .into_iter()
            .find(|r| r.id == id)
            .unwrap();
    let r = CafReader::open(&p.pkg.join(&row.path)).unwrap();
    let mut buf = vec![0f32; r.info().frames as usize * usize::from(r.info().channels)];
    r.read(0, &mut buf, &mut Vec::new()).unwrap();
    buf.iter()
        .step_by(usize::from(r.info().channels))
        .copied()
        .collect()
}

fn sounds(x: &[f32]) -> bool {
    x.iter().any(|v| v.abs() > 1e-3)
}

/// Every name in `dir` (sorted).
fn names(dir: &Path) -> Vec<String> {
    let mut v: Vec<String> = std::fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    v.sort();
    v
}

/// Files left in `Media/.incoming`.
fn incoming(p: &Project) -> Vec<String> {
    let d = p.pkg.join("Media/.incoming");
    if d.is_dir() { names(&d) } else { Vec::new() }
}

#[test]
fn a_bounce_equals_an_offline_render_of_the_range() {
    let _g = serial();
    let p = project(|_, _, _| {});
    let dest = p.dir.path().join("mix.wav");
    let req = mix_to(&p, &dest);
    let check = p.s.bounce_check(req.clone()).unwrap();
    assert_eq!(check.frames, BARS4_FRAMES, "four bars at 120 BPM");
    assert_eq!(
        (
            check.files.clone(),
            check.external_lanes,
            check.warning.clone()
        ),
        (vec![dest.display().to_string()], 0, None)
    );
    let j = wait_job(&p.s, p.s.bounce(req).unwrap());
    assert_eq!(j.state, JobStateRow::Done, "{j:?}");
    assert!(j.problems.is_empty(), "{j:?}");
    let (rate, ch, bits, float, frames, _) = read_wav(&dest);
    assert_eq!((rate, ch, bits, float), (48_000, 2, 24, false));
    assert!(
        check.frames <= frames && frames <= check.frames + 480_000,
        "{frames} (pre-flight M11)"
    );
    let (left, right) = (wav_channel(&dest, 0), wav_channel(&dest, 1));
    let k = left[996] / value(996);
    assert!(k > 0.1, "the strip's pan law is a constant");
    for f in 0..BARS4_FRAMES as usize {
        let want = k * value(f as u64);
        assert!(
            (left[f] - want).abs() <= 2.0 * STEP24,
            "left frame {f}: {} vs {want}",
            left[f]
        );
        assert!((right[f] + want).abs() <= 2.0 * STEP24, "right frame {f}");
    }
    let res = p.s.bounce_result(j.id).unwrap().unwrap();
    assert_eq!(res.media, Vec::<u64>::new());
    let file = &res.files[0];
    assert_eq!(
        (
            file.path.clone(),
            file.frames,
            file.rate,
            file.bits.clone(),
            file.channels,
            file.clipped
        ),
        (
            dest.display().to_string(),
            frames,
            48_000,
            "24-bit".to_string(),
            2,
            0
        )
    );
    assert!(
        !p.dir.path().join(".mix.wav.partial").exists(),
        "no partial"
    );
    assert!(incoming(&p).is_empty(), "no temporary left");
}

/// Ruling 3 and owner answer 2: names are made safe, and a name taken in
/// the bounce or on disk is numbered; nothing in the folder is replaced.
#[test]
fn stem_files_get_safe_unique_names() {
    let _g = serial();
    let mut extra = Vec::new();
    let p = project(|b, _, path| {
        b.mixer(MixerCmd::RenameStrip {
            strip: path,
            name: "A/B".into(),
        })
        .unwrap();
        extra.push(b.bus("A:B").unwrap());
        extra.push(b.bus(".hidden").unwrap());
        let empty = b.bus("Unnamed").unwrap();
        b.mixer(MixerCmd::RenameStrip {
            strip: empty,
            name: String::new(),
        })
        .unwrap();
        extra.push(empty);
        extra.push(b.bus(&"é".repeat(200)).unwrap());
    });
    let folder = p.dir.path().join("stems");
    std::fs::create_dir(&folder).unwrap();
    std::fs::write(folder.join("A-B.wav"), b"keep").unwrap();
    let strips = vec![p.path, extra[0].0, extra[1].0, extra[2].0, extra[3].0];
    let req = BounceRequestRow {
        source: BounceSourceRow::Stems { strips },
        path: Some(folder.display().to_string()),
        replace: true,
        ..mix_to(&p, &folder)
    };
    let mut want: Vec<String> = ["A-B 2.wav", "A-B 3.wav", "-hidden.wav"]
        .iter()
        .map(|n| folder.join(n).display().to_string())
        .collect();
    want.push(
        folder
            .join(format!("Strip {}.wav", extra[2].0))
            .display()
            .to_string(),
    );
    let files = p.s.bounce_check(req.clone()).unwrap().files;
    assert_eq!(files[..4], want[..], "an empty name becomes Strip <id>");
    // The long name is cut at a character boundary so that `.<name> NN.wav.partial` fits 255 bytes.
    let long = Path::new(&files[4])
        .file_name()
        .unwrap()
        .to_str()
        .unwrap()
        .to_string();
    assert!(
        long.ends_with(".wav") && long.len() < 255 - ".partial".len(),
        "{} bytes",
        long.len()
    );
    assert!(long.starts_with('é'));
    want.push(files[4].clone());
    let j = wait_job(&p.s, p.s.bounce(req).unwrap());
    assert_eq!(j.state, JobStateRow::Done, "{j:?}");
    assert_eq!(
        std::fs::read(folder.join("A-B.wav")).unwrap(),
        b"keep",
        "never replaced, even with replace"
    );
    let mut on_disk = vec![
        "-hidden.wav".to_string(),
        "A-B 2.wav".into(),
        "A-B 3.wav".into(),
        "A-B.wav".into(),
        format!("Strip {}.wav", extra[2].0),
        long.clone(),
    ];
    on_disk.sort();
    assert_eq!(names(&folder), on_disk);
    assert!(
        sounds(&wav_channel(&folder.join("A-B 2.wav"), 0)),
        "the track's stem"
    );
    let res = p.s.bounce_result(j.id).unwrap().unwrap();
    assert_eq!(
        res.files.iter().map(|f| f.path.clone()).collect::<Vec<_>>(),
        want
    );
    let lens: Vec<u64> = res.files.iter().map(|f| f.frames).collect();
    assert!(
        lens.iter().all(|n| *n == lens[0]),
        "one length for every file: {lens:?}"
    );
}

/// Ruling 9 and pre-flight I8: one undo step "Bounce", a new lane at the
/// range start, a `Bounced` item exactly as long as the file.
#[test]
fn add_to_project_places_the_bounce_on_a_new_lane() {
    let _g = serial();
    let p = project(|_, _, _| {});
    let items = p.s.media().unwrap().len();
    let add_only = BounceRequestRow {
        path: None,
        add_to_project: true,
        ..mix_to(&p, p.dir.path())
    };
    let j = wait_job(&p.s, p.s.bounce(add_only).unwrap());
    assert_eq!(j.state, JobStateRow::Done, "{j:?}");
    assert_eq!(
        p.s.snapshot().unwrap().undo_label.as_deref(),
        Some("Bounce")
    );
    let lanes = p.s.lanes(p.seq).unwrap();
    let last = lanes.last().unwrap();
    assert_eq!((last.name.as_str(), last.region_count), ("Mix", 1));
    let ev = p.s.lane_events(p.seq, last.id).unwrap();
    assert_eq!(ev.events[0].tick, 0, "at the range start");
    let rows = p.s.media().unwrap();
    assert_eq!(rows.len(), items + 1);
    let item = rows.iter().find(|r| j.created.contains(&r.id)).unwrap();
    assert_eq!(
        (item.origin, item.sample_rate, item.channels),
        (MediaOriginRow::Bounced, 48_000, 2)
    );
    assert_eq!(p.s.bounce_result(j.id).unwrap().unwrap().media, j.created);
    p.s.undo().unwrap();
    assert_eq!(
        p.s.lanes(p.seq).unwrap().len(),
        lanes.len() - 1,
        "one undo removes the lane"
    );
    assert_eq!(p.s.media().unwrap().len(), items, "and the item");
    // With a file too: the item is as long as the file, and named after it.
    let dest = p.dir.path().join("take one.wav");
    let both = BounceRequestRow {
        add_to_project: true,
        ..mix_to(&p, &dest)
    };
    let j = wait_job(&p.s, p.s.bounce(both).unwrap());
    assert_eq!(j.state, JobStateRow::Done, "{j:?}");
    assert_eq!(p.s.lanes(p.seq).unwrap().last().unwrap().name, "take one");
    let item =
        p.s.media()
            .unwrap()
            .into_iter()
            .find(|r| j.created.contains(&r.id))
            .unwrap();
    assert_eq!(
        item.frames,
        read_wav(&dest).4,
        "Add to project holds exactly the file's frames"
    );
    assert!(incoming(&p).is_empty());
}

/// Ruling 5, owner answer 3: files ignore mute and solo; the imports are
/// as heard, through the routing (a track into a muted bus is silent).
#[test]
fn add_to_project_respects_mute_and_solo_through_the_routing_and_files_do_not() {
    let _g = serial();
    let mut ids = Vec::new();
    let p = project(|b, _, path| {
        let bus = b.bus("Bus").unwrap();
        b.mixer(MixerCmd::SetOutput {
            strip: path,
            output: Output::Bus(bus),
        })
        .unwrap();
        ids.push(bus);
        // "Other" gets a send from the track; "Third" is fed by nothing.
        let other = b.bus("Other").unwrap();
        b.mixer(MixerCmd::AddSend {
            strip: path,
            send: AuxSend::new(other),
        })
        .unwrap();
        ids.push(other);
        ids.push(b.bus("Third").unwrap());
    });
    let (track, bus, other, third) = (p.path, ids[0].0, ids[1].0, ids[2].0);
    // (case, file track, file bus, heard track, heard bus)
    let cases: [(&str, bool, bool, bool, bool); 5] = [
        ("nothing", true, true, true, true),
        ("mute the track", true, false, false, false),
        ("mute the bus", true, true, false, false),
        // The track's solo-silenced edge carries nothing into the bus.
        ("solo another strip", true, false, false, false),
        // The track feeds the soloed bus (by its send), so it is kept and
        // still feeds Bus; Bus is solo-silenced, and the heard track (its
        // main path, Ruling 5: sends are not in the chain) is silent.
        ("solo the send's bus", true, true, false, false),
    ];
    for (k, (case, ft, fb, ht, hb)) in cases.into_iter().enumerate() {
        p.s.set_strip_mute(track, case == "mute the track").unwrap();
        p.s.set_strip_mute(bus, case == "mute the bus").unwrap();
        p.s.set_strip_solo(third, case == "solo another strip")
            .unwrap();
        p.s.set_strip_solo(other, case == "solo the send's bus")
            .unwrap();
        let folder = p.dir.path().join(format!("case {k}"));
        std::fs::create_dir(&folder).unwrap();
        let req = BounceRequestRow {
            source: BounceSourceRow::Stems {
                strips: vec![track, bus],
            },
            path: Some(folder.display().to_string()),
            add_to_project: true,
            ..mix_to(&p, &folder)
        };
        let j = wait_job(&p.s, p.s.bounce(req).unwrap());
        assert_eq!(j.state, JobStateRow::Done, "{case}: {j:?}");
        let res = p.s.bounce_result(j.id).unwrap().unwrap();
        let file = |i: usize| wav_channel(Path::new(&res.files[i].path), 0);
        assert_eq!(sounds(&file(0)), ft, "{case}: the track's file");
        assert_eq!(sounds(&file(1)), fb, "{case}: the bus's file");
        assert_eq!(res.media.len(), 2, "{case}");
        assert_eq!(
            sounds(&media_left(&p, res.media[0])),
            ht,
            "{case}: the track in place"
        );
        assert_eq!(
            sounds(&media_left(&p, res.media[1])),
            hb,
            "{case}: the bus in place"
        );
    }
}

/// Ruling 12: a cancel during the render (0.4), while the imports are
/// written (0.85) or before the start removes the imports, the temporaries
/// and every partial; the copies end.
#[test]
fn cancel_removes_everything_the_bounce_wrote() {
    let _g = serial();
    let p = project(|b, _, path| {
        b.insert(path, plugin("vizdaw.test.gain", "Test Gain"))
            .unwrap();
    });
    idle_until(&p.s, |s| {
        s.plugin_slots().unwrap()[0].status == SlotStatus::Live
    });
    let items = p.s.media().unwrap().len();
    let folder = p.dir.path().join("stems");
    std::fs::create_dir(&folder).unwrap();
    let req = BounceRequestRow {
        source: BounceSourceRow::Stems {
            strips: vec![p.path],
        },
        path: Some(folder.display().to_string()),
        add_to_project: true,
        ..mix_to(&p, &folder)
    };
    // During the render (pre-check M-6): the temporary CAFs go.
    p.s.debug_bounce_cancel_at(Some(0.4)).unwrap();
    let j = wait_job(&p.s, p.s.bounce(req.clone()).unwrap());
    assert_eq!(j.state, JobStateRow::Cancelled, "{j:?}");
    assert!(
        names(&folder).is_empty() && incoming(&p).is_empty(),
        "no file, no temporary"
    );
    assert_eq!(p.s.media().unwrap().len(), items);
    assert_eq!(vizdaw_ffi::plugins::bounce_hosts(), 0);
    // While the imports are written.
    p.s.debug_bounce_cancel_at(Some(0.85)).unwrap();
    let j = wait_job(&p.s, p.s.bounce(req.clone()).unwrap());
    assert_eq!(j.state, JobStateRow::Cancelled, "{j:?}");
    assert!(
        names(&folder).is_empty(),
        "no file and no partial: {:?}",
        names(&folder)
    );
    assert!(incoming(&p).is_empty(), "no temporary");
    assert_eq!(p.s.media().unwrap().len(), items, "no media item");
    assert_eq!(
        vizdaw_ffi::plugins::bounce_hosts(),
        0,
        "the copies ended (pre-flight I2)"
    );
    // Cancelled before it starts: the copies end too.
    p.s.debug_bounce_cancel_at(None).unwrap();
    p.s.debug_hold_jobs(true).unwrap();
    let id = p.s.bounce(req).unwrap();
    assert_eq!(vizdaw_ffi::plugins::bounce_hosts(), 1);
    p.s.cancel_job(id).unwrap();
    p.s.debug_hold_jobs(false).unwrap();
    assert_eq!(wait_job(&p.s, id).state, JobStateRow::Cancelled);
    assert_eq!(vizdaw_ffi::plugins::bounce_hosts(), 0);
    assert!(names(&folder).is_empty() && incoming(&p).is_empty());
}

/// Review Focus 3: cancelled at 95 % (the export's partial being written)
/// over a file it was told to replace: the old file is byte-identical.
#[test]
fn a_late_cancel_keeps_the_replaced_file() {
    let _g = serial();
    let p = project(|_, _, _| {});
    let dest = p.dir.path().join("mix.wav");
    std::fs::write(&dest, b"the old mix").unwrap();
    p.s.debug_bounce_cancel_at(Some(0.95)).unwrap();
    let j = wait_job(
        &p.s,
        p.s.bounce(BounceRequestRow {
            replace: true,
            ..mix_to(&p, &dest)
        })
        .unwrap(),
    );
    assert_eq!(j.state, JobStateRow::Cancelled, "{j:?}");
    assert_eq!(std::fs::read(&dest).unwrap(), b"the old mix");
    assert!(!p.dir.path().join(".mix.wav.partial").exists());
    assert!(incoming(&p).is_empty());
    p.s.debug_bounce_cancel_at(None).unwrap();
}

/// Ruling 6: the copies take the live (unsaved) state; pre-flight C2: the
/// check can fail, since the region sounds.
#[test]
fn copies_render_offline_with_the_live_state() {
    let _g = serial();
    let p = project(|b, _, path| {
        b.insert(path, plugin("vizdaw.test.gain", "Test Gain"))
            .unwrap();
    });
    idle_until(&p.s, |s| {
        s.plugin_slots().unwrap()[0].status == SlotStatus::Live
    });
    let slot = p.s.plugin_slots().unwrap()[0].id;
    let bounce = |name: &str| {
        let dest = p.dir.path().join(name);
        let j = wait_job(&p.s, p.s.bounce(mix_to(&p, &dest)).unwrap());
        assert_eq!(j.state, JobStateRow::Done, "{j:?}");
        assert!(
            j.problems.is_empty(),
            "the gain plugin renders offline: {j:?}"
        );
        wav_channel(&dest, 0)
    };
    let full = bounce("full.wav");
    assert!(sounds(&full));
    p.s.set_param(slot, 0, 0.5).unwrap();
    let half = bounce("half.wav");
    for f in 0..BARS4_FRAMES as usize {
        assert!(
            (half[f] - full[f] / 2.0).abs() <= 2.0 * STEP24,
            "frame {f}: the copy has the live 0.5"
        );
    }
    p.s.set_param(slot, 0, 0.0).unwrap();
    assert!(!sounds(&bounce("silent.wav")), "and the live 0");
    assert_eq!(
        vizdaw_ffi::plugins::bounce_hosts(),
        0,
        "the copies went to closing"
    );
}

/// Plan 7b B2 review M1: a project whose only plug-in is an EQ still makes
/// its copies, so the bounce renders the EQ with its live state (the case
/// where an empty `slot_ids` would have skipped them and bounced dry).
#[test]
fn an_eq_only_project_bounces_through_its_eq() {
    let _g = serial();
    let p = project(|b, _, path| {
        b.mixer(MixerCmd::SetEq {
            strip: path,
            plugin: Some(plugin("vizdaw.test.gain", "Test Gain")),
        })
        .unwrap();
    });
    idle_until(&p.s, |s| {
        s.plugin_slots()
            .unwrap()
            .first()
            .is_some_and(|r| r.status == SlotStatus::Live)
    });
    let slots = p.s.plugin_slots().unwrap();
    assert_eq!(slots.len(), 1, "the EQ is the only plug-in: {slots:?}");
    let slot = slots[0].id;
    let bounce = |name: &str| {
        let dest = p.dir.path().join(name);
        let j = wait_job(&p.s, p.s.bounce(mix_to(&p, &dest)).unwrap());
        assert_eq!(j.state, JobStateRow::Done, "{j:?}");
        assert!(j.problems.is_empty(), "the EQ renders offline: {j:?}");
        wav_channel(&dest, 0)
    };
    let full = bounce("full.wav");
    assert!(sounds(&full));
    p.s.set_param(slot, 0, 0.5).unwrap();
    let half = bounce("half.wav");
    for f in 0..BARS4_FRAMES as usize {
        assert!(
            (half[f] - full[f] / 2.0).abs() <= 2.0 * STEP24,
            "frame {f}: the EQ's copy has the live 0.5"
        );
    }
    p.s.set_param(slot, 0, 0.0).unwrap();
    assert!(
        !sounds(&bounce("silent.wav")),
        "the EQ applied: not the dry signal"
    );
}

/// Ruling 6: a copy that cannot load is a pass-through, named in the problems.
#[test]
fn a_copy_that_fails_is_a_pass_through_and_named() {
    let _g = serial();
    let dry = project(|_, _, _| {});
    let dest = dry.dir.path().join("dry.wav");
    assert_eq!(
        wait_job(&dry.s, dry.s.bounce(mix_to(&dry, &dest)).unwrap()).state,
        JobStateRow::Done
    );
    let p = project(|b, _, path| {
        b.insert(path, plugin("vizdaw.test.nothere", "Not Here"))
            .unwrap();
    });
    let wet = p.dir.path().join("wet.wav");
    let j = wait_job(&p.s, p.s.bounce(mix_to(&p, &wet)).unwrap());
    assert_eq!(j.state, JobStateRow::Done, "{j:?}");
    assert!(j.problems.iter().any(|m| m.contains("Not Here")), "{j:?}");
    assert_eq!(
        wav_channel(&wet, 0),
        wav_channel(&dest, 0),
        "the dry signal"
    );
}

/// Ruling 6: live playback goes on, and its engine is not reloaded.
#[test]
fn a_bounce_while_playing_keeps_playback_running() {
    let _g = serial();
    // A second region at bar 100 keeps the song playing for minutes, so a
    // slow build never reaches its end while the bounce runs (pre-check M-13).
    let p = project(|b, seq, _| {
        let m = *b.doc().media().keys().next().unwrap();
        let l = b.lane(seq, "Late", None).unwrap();
        b.region(
            seq,
            l,
            400 * Q,
            m,
            0,
            FRAMES,
            0.0,
            Fade::default(),
            Fade::default(),
        )
        .unwrap();
    });
    p.s.play(p.seq).unwrap();
    idle_until(&p.s, |s| s.transport().unwrap().playing);
    let loads = vizdaw_ffi::host::loads();
    let j = wait_job(
        &p.s,
        p.s.bounce(mix_to(&p, &p.dir.path().join("live.wav")))
            .unwrap(),
    );
    assert_eq!(j.state, JobStateRow::Done, "{j:?}");
    assert!(p.s.transport().unwrap().playing, "playback went on");
    assert_eq!(
        vizdaw_ffi::host::loads(),
        loads,
        "the live engine was not reloaded"
    );
    p.s.stop().unwrap();
}

/// Rulings 6 and 12; pre-flight M13, M15: one bounce per session; not inside
/// an open edit; no Save As while it runs; the copies always end, also
/// at close.
#[test]
fn one_bounce_at_a_time() {
    let _g = serial();
    let p = project(|b, _, path| {
        b.insert(path, plugin("vizdaw.test.gain", "Test Gain"))
            .unwrap();
    });
    idle_until(&p.s, |s| {
        s.plugin_slots().unwrap()[0].status == SlotStatus::Live
    });
    let req = mix_to(&p, &p.dir.path().join("one.wav"));
    p.s.begin_edit("Fader".into()).unwrap();
    let e = p.s.bounce(req.clone()).unwrap_err();
    assert!(
        matches!(&e, VizError::Busy { message } if message.contains("Finish the open edit")),
        "{e:?}"
    );
    p.s.abort_edit().unwrap();
    p.s.debug_hold_jobs(true).unwrap();
    let id = p.s.bounce(req.clone()).unwrap();
    let e = p.s.bounce(req.clone()).unwrap_err();
    assert!(
        matches!(&e, VizError::Busy { message } if message == "A bounce is running."),
        "a second bounce waits: {e:?}"
    );
    assert!(matches!(
        p.s.save_as(p.dir.path().join("Q.vizdaw").display().to_string()),
        Err(VizError::Busy { .. })
    ));
    p.s.cancel_job(id).unwrap();
    p.s.debug_hold_jobs(false).unwrap();
    assert_eq!(wait_job(&p.s, id).state, JobStateRow::Cancelled);
    assert_eq!(vizdaw_ffi::plugins::bounce_hosts(), 0);
    assert_eq!(
        wait_job(&p.s, p.s.bounce(req.clone()).unwrap()).state,
        JobStateRow::Done,
        "then the next one runs"
    );
    // That one wrote one.wav: the next replaces it (Ruling 3 refuses a
    // taken name without `replace`).
    let req = BounceRequestRow {
        replace: true,
        ..req
    };
    // Closing the session ends a held bounce's copies (pre-flight I2).
    p.s.debug_hold_jobs(true).unwrap();
    p.s.bounce(req).unwrap();
    assert_eq!(vizdaw_ffi::plugins::bounce_hosts(), 1);
    p.s.close().unwrap();
    assert_eq!(
        vizdaw_ffi::plugins::bounce_hosts(),
        0,
        "close ends the copies"
    );
}

/// Ruling 9, pre-flight I9.
#[test]
fn a_bounce_and_a_format_change_wait_for_each_other() {
    let _g = serial();
    let p = project(|_, _, _| {});
    let add = BounceRequestRow {
        path: None,
        add_to_project: true,
        ..mix_to(&p, p.dir.path())
    };
    let fmt = ProjectFormatRow {
        sample_rate: 48_000,
        depth: BitDepthRow::Int24,
    };
    p.s.debug_hold_jobs(true).unwrap();
    let b = p.s.bounce(add.clone()).unwrap();
    assert!(
        matches!(p.s.set_project_format(fmt), Err(VizError::Busy { .. })),
        "no format change under a bounce"
    );
    p.s.cancel_job(b).unwrap();
    p.s.debug_hold_jobs(false).unwrap();
    wait_job(&p.s, b);
    p.s.debug_hold_jobs(true).unwrap();
    let f = p.s.set_project_format(fmt).unwrap();
    assert!(
        matches!(p.s.bounce(add), Err(VizError::Busy { .. })),
        "no bounce in place under a format change"
    );
    p.s.cancel_job(f).unwrap();
    p.s.debug_hold_jobs(false).unwrap();
    wait_job(&p.s, f);
}

/// Ruling 12 (pre-flight I1): the apply fails (the sequence went), but the
/// exported file, which replaced an old one, stays whole.
#[test]
fn a_failed_import_keeps_the_exported_file() {
    let _g = serial();
    let p = project(|b, _, _| {
        b.sequence("Other", "Audio", None).unwrap();
    });
    let dest = p.dir.path().join("mix.wav");
    std::fs::write(&dest, b"old").unwrap();
    let items = p.s.media().unwrap().len();
    p.s.debug_hold_jobs(true).unwrap();
    let id =
        p.s.bounce(BounceRequestRow {
            add_to_project: true,
            replace: true,
            ..mix_to(&p, &dest)
        })
        .unwrap();
    p.s.edit(
        EditCommand::Sequence {
            edit: SequenceEdit::Delete {
                seq: p.seq,
                delete_references: true,
            },
        },
        None,
    )
    .unwrap();
    p.s.debug_hold_jobs(false).unwrap();
    let j = wait_job(&p.s, id);
    assert_eq!(j.state, JobStateRow::Failed, "{j:?}");
    let (rate, ch, bits, _, frames, _) = read_wav(&dest);
    assert_eq!((rate, ch, bits), (48_000, 2, 24), "the new file, whole");
    assert!(frames >= BARS4_FRAMES);
    assert_eq!(p.s.media().unwrap().len(), items, "its import was removed");
    assert_eq!(
        p.s.bounce_result(id).unwrap().unwrap().files[0].path,
        dest.display().to_string()
    );
    assert!(incoming(&p).is_empty());
}

/// Ruling 8: the check counts lanes on external MIDI and says the spec's
/// sentence, singular and plural (silence itself: Task 2's
/// `external_midi_is_silent_in_a_bounce`).
#[test]
fn external_midi_is_counted_in_the_check() {
    let _g = serial();
    for lanes in [1, 2] {
        let p = project(|b, seq, _| {
            let ext = b.dest("Hardware", "Ext Port", 0).unwrap();
            for n in 0..lanes {
                let l = b.lane(seq, &format!("Keys {n}"), Some(ext)).unwrap();
                b.notes(seq, l, &[(0, 60, 0x8000, Q)]).unwrap();
            }
        });
        let req = mix_to(&p, &p.dir.path().join("ext.wav"));
        let c = p.s.bounce_check(req.clone()).unwrap();
        assert_eq!(c.external_lanes, lanes);
        let want = if lanes == 1 {
            "1 track plays external MIDI and will be silent. Track it as audio first: Track Synth…"
        } else {
            "2 tracks play external MIDI and will be silent. Track them as audio first: Track Synth…"
        };
        assert_eq!(c.warning.as_deref(), Some(want));
        assert_eq!(
            wait_job(&p.s, p.s.bounce(req).unwrap()).state,
            JobStateRow::Done
        );
    }
}

/// Ruling 13, owner answer 1: a hot mix clips in a 16-bit file (both
/// signs, counted, reported, and the bounce is still Done) and keeps its
/// overs in a float file.
#[test]
fn clipped_samples_are_reported() {
    let _g = serial();
    let p = project(|b, _, path| {
        b.mixer(MixerCmd::SetGain {
            strip: path,
            gain_db: 12.0,
        })
        .unwrap();
    });
    let loud = p.dir.path().join("loud.wav");
    let j = wait_job(
        &p.s,
        p.s.bounce(BounceRequestRow {
            depth: BitDepthRow::Int16,
            ..mix_to(&p, &loud)
        })
        .unwrap(),
    );
    assert_eq!(j.state, JobStateRow::Done, "{j:?}");
    assert!(
        j.problems
            .iter()
            .any(|m| m.contains("loud.wav") && m.contains("samples clipped")),
        "{j:?}"
    );
    let res = p.s.bounce_result(j.id).unwrap().unwrap();
    assert!(res.files[0].clipped > 0);
    let (.., s) = read_wav(&loud);
    assert!(
        s.contains(&32_767) && s.contains(&-32_768),
        "clipped at both ends"
    );
    let float = p.dir.path().join("loud float.wav");
    let j = wait_job(
        &p.s,
        p.s.bounce(BounceRequestRow {
            depth: BitDepthRow::Float32,
            ..mix_to(&p, &float)
        })
        .unwrap(),
    );
    assert_eq!(j.state, JobStateRow::Done, "{j:?}");
    assert!(j.problems.is_empty(), "a float file does not clip: {j:?}");
    let (l, r) = (wav_channel(&float, 0), wav_channel(&float, 1));
    assert!(
        l.iter().any(|x| *x > 1.0) && r.iter().any(|x| *x < -1.0),
        "overs kept above 0 dBFS"
    );
    // Final review I1: Add to project in a 24-bit project clips the hot
    // heard stem at the import, and says so (singular and plural forms are
    // the unit tests' in src/bounce.rs).
    let p24 = project(|b, _, path| {
        b.mixer(MixerCmd::RenameStrip {
            strip: path,
            name: "Hot".into(),
        })
        .unwrap();
        b.mixer(MixerCmd::SetGain {
            strip: path,
            gain_db: 12.0,
        })
        .unwrap();
    });
    let f = p24
        .s
        .set_project_format(ProjectFormatRow {
            sample_rate: 48_000,
            depth: BitDepthRow::Int24,
        })
        .unwrap();
    assert_eq!(wait_job(&p24.s, f).state, JobStateRow::Done);
    let j = wait_job(
        &p24.s,
        p24.s
            .bounce(BounceRequestRow {
                source: BounceSourceRow::Stems {
                    strips: vec![p24.path],
                },
                path: None,
                add_to_project: true,
                ..mix_to(&p24, p24.dir.path())
            })
            .unwrap(),
    );
    assert_eq!(j.state, JobStateRow::Done, "{j:?}");
    assert_eq!(j.created.len(), 1, "{j:?}");
    assert!(
        j.problems.iter().any(
            |m| m.starts_with("Hot (added to the project): ") && m.ends_with("samples clipped")
        ),
        "{j:?}"
    );
}

/// Ruling 3, pre-flight I4, M6, M7: each is `Model`, and nothing is written.
#[test]
fn hostile_bounce_requests_are_refused() {
    let _g = serial();
    let p = project(|_, _, _| {});
    let d = p.dir.path();
    std::fs::create_dir(d.join("folder.wav")).unwrap();
    // A real folder, so the `..` case fails for its `..` alone (pre-check M-4).
    std::fs::create_dir(d.join("sub")).unwrap();
    std::fs::write(d.join("file.txt"), b"x").unwrap();
    // Another project's package (final review T4-M10).
    std::fs::create_dir_all(d.join("Other.vizdaw/Media")).unwrap();
    // A folder this process cannot write (final review m3). Root writes
    // anywhere, so those cases are left out when the effective user is
    // root (re-review n2): a file this process creates is owned by it.
    let root = {
        use std::os::unix::fs::MetadataExt;
        let probe = d.join("uid-probe");
        std::fs::write(&probe, b"").unwrap();
        let uid = std::fs::metadata(&probe).unwrap().uid();
        std::fs::remove_file(&probe).unwrap();
        uid == 0
    };
    let ro = d.join("ro");
    std::fs::create_dir(&ro).unwrap();
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&ro, std::fs::Permissions::from_mode(0o555)).unwrap();
    }
    // A folder so deep that a file fits 1024 bytes and its partial does not.
    let mut deep = d.to_path_buf();
    while deep.as_os_str().len() < 1024 - 250 {
        deep.push("d".repeat(200));
    }
    std::fs::create_dir_all(&deep).unwrap();
    let fill = 1020 - deep.as_os_str().len() - 1 - ".wav".len();
    let deep_file = deep.join(format!("{}.wav", "f".repeat(fill)));
    assert_eq!(deep_file.as_os_str().len(), 1020);
    // ... and one where a stem's file does not fit.
    let deeper = deep.join("e".repeat(1015 - deep.as_os_str().len() - 1));
    std::fs::create_dir(&deeper).unwrap();
    let before = names(d);
    let ok = mix_to(&p, &d.join("x.wav"));
    let stems = |strips: Vec<u64>| BounceRequestRow {
        source: BounceSourceRow::Stems { strips },
        path: Some(d.display().to_string()),
        ..ok.clone()
    };
    let at = |path: String| BounceRequestRow {
        path: Some(path),
        ..ok.clone()
    };
    let cases: Vec<(&str, BounceRequestRow)> = vec![
        (
            "unknown seq",
            BounceRequestRow {
                seq: 9_999,
                ..ok.clone()
            },
        ),
        ("empty stems", stems(vec![])),
        ("a strip twice", stems(vec![p.path, p.path])),
        ("an unknown strip", stems(vec![99_999])),
        ("10 000 strips", stems((0..10_000).collect())),
        (
            "start = end",
            BounceRequestRow {
                range: BounceRangeRow::Ticks {
                    start: BARS4,
                    end: BARS4,
                },
                ..ok.clone()
            },
        ),
        (
            "start > end",
            BounceRequestRow {
                range: BounceRangeRow::Ticks {
                    start: BARS4,
                    end: Q,
                },
                ..ok.clone()
            },
        ),
        (
            "negative start",
            BounceRequestRow {
                range: BounceRangeRow::Ticks { start: -1, end: Q },
                ..ok.clone()
            },
        ),
        (
            "end past i64::MAX/2",
            BounceRequestRow {
                range: BounceRangeRow::Ticks {
                    start: 0,
                    end: i64::MAX,
                },
                ..ok.clone()
            },
        ),
        (
            "rate 0",
            BounceRequestRow {
                rate: 0,
                ..ok.clone()
            },
        ),
        (
            "rate 7 999",
            BounceRequestRow {
                rate: 7_999,
                ..ok.clone()
            },
        ),
        (
            "rate 384 001",
            BounceRequestRow {
                rate: 384_001,
                ..ok.clone()
            },
        ),
        (
            "rate 1 000 000",
            BounceRequestRow {
                rate: 1_000_000,
                ..ok.clone()
            },
        ),
        (
            "neither path nor add",
            BounceRequestRow {
                path: None,
                add_to_project: false,
                ..ok.clone()
            },
        ),
        ("a relative path", at("relative.wav".into())),
        (
            "a missing folder",
            at(d.join("no/such/x.wav").display().to_string()),
        ),
        (
            "a wrong extension",
            at(d.join("x.caf").display().to_string()),
        ),
        (
            "a mix path that is a folder",
            BounceRequestRow {
                replace: true,
                ..at(d.join("folder.wav").display().to_string())
            },
        ),
        (
            "a stems path that is a file",
            BounceRequestRow {
                path: Some(d.join("file.txt").display().to_string()),
                ..stems(vec![p.path])
            },
        ),
        (
            "inside the package",
            at(p.pkg.join("x.wav").display().to_string()),
        ),
        (
            "inside its media",
            at(p.pkg.join("Media/x.wav").display().to_string()),
        ),
        (
            "a .. component",
            at(format!("{}/sub/../x.wav", d.display())),
        ),
        (
            "a 4097-byte path",
            at(format!("{}/{}.wav", d.display(), "a".repeat(4097))),
        ),
        // Final review m3: names that can never be written, refused before
        // the render.
        ("a NUL byte", at(format!("{}/a\u{0}b.wav", d.display()))),
        (
            "a name whose partial is over 255 bytes",
            at(d.join(format!("{}.wav", "n".repeat(247)))
                .display()
                .to_string()),
        ),
        (
            "a partial path over 1024 bytes",
            at(deep_file.display().to_string()),
        ),
        (
            "a stem path over 1024 bytes",
            BounceRequestRow {
                path: Some(deeper.display().to_string()),
                ..stems(vec![p.path])
            },
        ),
        (
            "a folder that cannot be written",
            at(ro.join("x.wav").display().to_string()),
        ),
        (
            "a stems folder that cannot be written",
            BounceRequestRow {
                path: Some(ro.display().to_string()),
                ..stems(vec![p.path])
            },
        ),
        (
            "inside another project",
            at(d.join("Other.vizdaw/Media/x.wav").display().to_string()),
        ),
        (
            "inside another project's package itself",
            BounceRequestRow {
                path: Some(d.join("Other.vizdaw").display().to_string()),
                ..stems(vec![p.path])
            },
        ),
    ];
    for (what, req) in cases {
        if root && what.contains("cannot be written") {
            continue;
        }
        let e = p.s.bounce(req.clone()).unwrap_err();
        assert!(matches!(e, VizError::Model { .. }), "{what}: {e:?}");
        assert!(
            matches!(p.s.bounce_check(req), Err(VizError::Model { .. })),
            "{what}: the check refuses it too"
        );
        if what.starts_with("inside") {
            assert!(e.to_string().contains("outside the project"), "{what}: {e}");
        }
    }
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&ro, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    assert_eq!(names(d), before, "nothing was written");
    assert!(names(&ro).is_empty() && names(&deeper).is_empty());
    assert_eq!(names(&deep).len(), 1, "only the deeper folder");
    assert!(names(&d.join("Other.vizdaw/Media")).is_empty());
    assert!(!p.pkg.join("x.wav").exists() && !p.pkg.join("Media/x.wav").exists());
}

/// Ruling 4: the tail limit is process-wide and clamped.
#[test]
fn the_tail_limit_is_clamped() {
    let _g = serial();
    assert_eq!(bounce_settings().tail_cap_ms, 10_000);
    set_bounce_settings(BounceSettingsRow {
        tail_cap_ms: 99_000,
    });
    assert_eq!(bounce_settings().tail_cap_ms, 30_000);
    set_bounce_settings(BounceSettingsRow { tail_cap_ms: 0 });
    assert_eq!(bounce_settings().tail_cap_ms, 0);
    set_bounce_settings(BounceSettingsRow {
        tail_cap_ms: 10_000,
    });
}

// macOS only: rate conversion goes through ExtAudioFile (the CAF importer converts depth only).
#[cfg(target_os = "macos")]
#[test]
fn a_bounce_at_another_rate_converts() {
    let _g = serial();
    let p = project(|_, _, _| {});
    let dest = p.dir.path().join("44k1.wav");
    let j = wait_job(
        &p.s,
        p.s.bounce(BounceRequestRow {
            rate: 44_100,
            ..mix_to(&p, &dest)
        })
        .unwrap(),
    );
    assert_eq!(j.state, JobStateRow::Done, "{j:?}");
    let (rate, _, _, _, frames, _) = read_wav(&dest);
    assert_eq!(rate, 44_100);
    let want = BARS4_FRAMES * 44_100 / 48_000;
    assert!(frames.abs_diff(want) <= 1, "{frames} vs {want}");
}

/// Final review m4: without the system converter a rate change is refused
/// before the render (a `Model` error), not after it.
#[cfg(not(target_os = "macos"))]
#[test]
fn a_rate_change_without_the_system_converter_is_refused() {
    let _g = serial();
    let p = project(|_, _, _| {});
    let dest = p.dir.path().join("44k1.wav");
    let req = BounceRequestRow {
        rate: 44_100,
        ..mix_to(&p, &dest)
    };
    for e in [
        p.s.bounce(req.clone()).unwrap_err(),
        p.s.bounce_check(req.clone()).unwrap_err(),
    ] {
        assert!(
            matches!(&e, VizError::Model { message } if message.contains("system importer")),
            "{e:?}"
        );
    }
    // Add to project alone imports at the project rate: no conversion.
    let j = wait_job(
        &p.s,
        p.s.bounce(BounceRequestRow {
            path: None,
            add_to_project: true,
            ..req
        })
        .unwrap(),
    );
    assert_eq!(j.state, JobStateRow::Done, "{j:?}");
    assert!(!dest.exists() && !p.dir.path().join(".44k1.wav.partial").exists());
    assert!(incoming(&p).is_empty());
}

/// Final review T4-1 (Ruling 12): one "Add to project" import that fails
/// aborts the whole bounce: nothing is added, nothing exported, the first
/// import is removed, and the one problem names the stem.
#[test]
fn a_failed_import_aborts_the_bounce() {
    let _g = serial();
    let mut bus = StripId(0);
    let p = project(|b, _, path| {
        b.mixer(MixerCmd::RenameStrip {
            strip: path,
            name: "Track".into(),
        })
        .unwrap();
        // Hot, so the first import clips in the 24-bit project (re-review n1).
        b.mixer(MixerCmd::SetGain {
            strip: path,
            gain_db: 12.0,
        })
        .unwrap();
        bus = b.bus("Bus").unwrap();
    });
    let f =
        p.s.set_project_format(ProjectFormatRow {
            sample_rate: 48_000,
            depth: BitDepthRow::Int24,
        })
        .unwrap();
    assert_eq!(wait_job(&p.s, f).state, JobStateRow::Done);
    let items = p.s.media().unwrap().len();
    let lanes = p.s.lanes(p.seq).unwrap().len();
    let media_before = names(&p.pkg.join("Media"));
    let folder = p.dir.path().join("stems");
    std::fs::create_dir(&folder).unwrap();
    p.s.debug_bounce_fail_import(Some(1)).unwrap();
    let j = wait_job(
        &p.s,
        p.s.bounce(BounceRequestRow {
            source: BounceSourceRow::Stems {
                strips: vec![p.path, bus.0],
            },
            path: Some(folder.display().to_string()),
            add_to_project: true,
            ..mix_to(&p, &folder)
        })
        .unwrap(),
    );
    assert_eq!(j.state, JobStateRow::Failed, "{j:?}");
    assert!(
        j.message.as_deref().is_some_and(|m| m.contains("Bus")),
        "the message names the second stem: {j:?}"
    );
    assert!(
        !j.problems
            .iter()
            .any(|m| m.contains("added to the project")),
        "nothing was added, so no clip line says so: {j:?}"
    );
    assert!(names(&folder).is_empty(), "no export, no partial");
    assert!(incoming(&p).is_empty(), "no temporary");
    assert_eq!(p.s.media().unwrap().len(), items, "no media item");
    assert_eq!(p.s.lanes(p.seq).unwrap().len(), lanes, "no lane");
    // Hidden folders (`.incoming`, the overview cache) aside.
    let files = |mut v: Vec<String>| {
        v.retain(|n| !n.starts_with('.'));
        v
    };
    assert_eq!(
        files(names(&p.pkg.join("Media"))),
        files(media_before),
        "the first import was removed"
    );
    assert!(p.s.bounce_result(j.id).unwrap().unwrap().files.is_empty());
    assert_eq!(vizdaw_ffi::plugins::bounce_hosts(), 0);
    // The hook is one-shot: the next bounce imports both.
    let j = wait_job(
        &p.s,
        p.s.bounce(BounceRequestRow {
            source: BounceSourceRow::Stems {
                strips: vec![p.path, bus.0],
            },
            path: None,
            add_to_project: true,
            ..mix_to(&p, &folder)
        })
        .unwrap(),
    );
    assert_eq!((j.state, j.created.len()), (JobStateRow::Done, 2), "{j:?}");
}

/// Final review T4-M8: a rename that fails after an earlier one landed
/// fails the job; the file already in place stays and is named.
#[test]
fn a_failed_rename_keeps_the_files_already_in_place() {
    let _g = serial();
    let mut bus = StripId(0);
    let p = project(|b, _, path| {
        b.mixer(MixerCmd::RenameStrip {
            strip: path,
            name: "Track".into(),
        })
        .unwrap();
        bus = b.bus("Bus").unwrap();
    });
    let folder = p.dir.path().join("stems");
    std::fs::create_dir(&folder).unwrap();
    p.s.debug_hold_jobs(true).unwrap();
    let id =
        p.s.bounce(BounceRequestRow {
            source: BounceSourceRow::Stems {
                strips: vec![p.path, bus.0],
            },
            path: Some(folder.display().to_string()),
            ..mix_to(&p, &folder)
        })
        .unwrap();
    // Every name the second stem could take is taken after the plan: its
    // own by a file (so its partial is still written), the numbered ones
    // by folders.
    std::fs::write(folder.join("Bus.wav"), b"taken").unwrap();
    for n in 2..=9999 {
        std::fs::create_dir(folder.join(format!("Bus {n}.wav"))).unwrap();
    }
    p.s.debug_hold_jobs(false).unwrap();
    let j = wait_job(&p.s, id);
    assert_eq!(j.state, JobStateRow::Failed, "{j:?}");
    assert!(
        j.message.as_deref().is_some_and(|m| m.contains("Bus.wav")),
        "{j:?}"
    );
    let track = folder.join("Track.wav");
    assert!(
        read_wav(&track).4 >= BARS4_FRAMES,
        "the first file stays whole"
    );
    let res = p.s.bounce_result(id).unwrap().unwrap();
    assert_eq!(
        res.files.iter().map(|f| f.path.clone()).collect::<Vec<_>>(),
        vec![track.display().to_string()]
    );
    assert_eq!(std::fs::read(folder.join("Bus.wav")).unwrap(), b"taken");
    let partials: Vec<String> = names(&folder)
        .into_iter()
        .filter(|n| n.ends_with(".partial"))
        .collect();
    assert!(partials.is_empty(), "{partials:?}");
    assert!(incoming(&p).is_empty());
}

/// Final review T4-M2: a live value sent to an automated parameter (here
/// on a muted automation lane) does not replace its kept static value in
/// the bounce's copy, as Save keeps it (plan 5e-1 Ruling 15).
#[test]
fn a_ride_of_an_automated_parameter_keeps_its_static_value() {
    let _g = serial();
    let p = project(|b, seq, path| {
        let slot = b
            .insert(path, plugin("vizdaw.test.gain", "Test Gain"))
            .unwrap();
        let l = b.lane(seq, "Auto", None).unwrap();
        b.event(
            seq,
            l,
            0,
            EventBody::Automation {
                target: AutomationTarget::PluginParam(slot, ParamId(0)),
                value: 0.0,
            },
        )
        .unwrap();
        b.run(Command::Lane(LaneCmd::SetFlags {
            seq,
            lane: l,
            mute: Some(true),
            solo: None,
            arm: None,
        }))
        .unwrap();
    });
    idle_until(&p.s, |s| {
        s.plugin_slots().unwrap()[0].status == SlotStatus::Live
    });
    let slot = p.s.plugin_slots().unwrap()[0].id;
    p.s.set_param(slot, 0, 0.0).unwrap();
    let dest = p.dir.path().join("kept.wav");
    let j = wait_job(&p.s, p.s.bounce(mix_to(&p, &dest)).unwrap());
    assert_eq!(j.state, JobStateRow::Done, "{j:?}");
    assert!(
        sounds(&wav_channel(&dest, 0)),
        "the static 1.0 is kept, not the live 0"
    );
}

/// Final review T4-M4: a session dropped without `close` while a bounce
/// with plugin copies is held ends the copies and leaves nothing behind.
#[test]
fn dropping_the_session_ends_a_held_bounce() {
    let _g = serial();
    let p = project(|b, _, path| {
        b.insert(path, plugin("vizdaw.test.gain", "Test Gain"))
            .unwrap();
    });
    idle_until(&p.s, |s| {
        s.plugin_slots().unwrap()[0].status == SlotStatus::Live
    });
    let folder = p.dir.path().join("stems");
    std::fs::create_dir(&folder).unwrap();
    p.s.debug_hold_jobs(true).unwrap();
    let req = BounceRequestRow {
        source: BounceSourceRow::Stems {
            strips: vec![p.path],
        },
        path: Some(folder.display().to_string()),
        add_to_project: true,
        ..mix_to(&p, &folder)
    };
    p.s.bounce(req).unwrap();
    assert_eq!(vizdaw_ffi::plugins::bounce_hosts(), 1);
    let Project { dir, pkg, s, .. } = p;
    drop(s);
    assert_eq!(vizdaw_ffi::plugins::bounce_hosts(), 0, "the copies ended");
    let other = project(|_, _, _| {});
    idle_until(&other.s, |_| vizdaw_ffi::host::closing_hosts_waiting() == 0);
    assert!(names(&folder).is_empty(), "no file, no partial");
    let incoming = pkg.join("Media/.incoming");
    assert!(!incoming.is_dir() || names(&incoming).is_empty());
    drop(dir);
}

/// Final review T4-M5: a session dropped without `close` while a bounce
/// with no plugin copies runs stops that bounce too: it writes nothing
/// after the drop.
#[test]
fn dropping_the_session_stops_a_bounce_without_copies() {
    let _g = serial();
    let p = project(|_, _, _| {});
    let dest = p.dir.path().join("late.wav");
    p.s.bounce(BounceRequestRow {
        range: BounceRangeRow::Song,
        ..mix_to(&p, &dest)
    })
    .unwrap();
    let Project { dir, pkg, s, .. } = p;
    drop(s);
    // The drop joined the job: nothing may appear afterwards. A job left
    // running would finish this 10-second render well within the wait.
    let until = Instant::now() + std::time::Duration::from_secs(3);
    while Instant::now() < until {
        assert!(!dest.exists(), "a bounce wrote after its session went");
        std::thread::yield_now();
    }
    assert!(!dir.path().join(".late.wav.partial").exists());
    let incoming = pkg.join("Media/.incoming");
    assert!(!incoming.is_dir() || names(&incoming).is_empty());
}
