//! Export for another DAW (plan 6b Task 2; spec §7.1, §7.6, §9, §10;
//! Rulings 1–10; pre-flight F1–F3, F6, F7, F19). Projects are saved
//! packages in temporary folders, built as `tests/bounce.rs` builds them:
//! 48 kHz float, with generated CAF media ("Gtr" plays a click from its
//! region's start and a smooth pulse half a second later; "Bass" plays a
//! note on the built-in synth, silent offline). One test at a time: the
//! engine host and the bounce settings are process-wide.

mod common;

use core_engine::fixtures::{Builder, Q};
use core_io::smf::{self, SmfEvent};
use core_media::caf::{CafWriter, WriteMode};
use core_model::command::{Command, LaneCmd, SequenceCmd, SettingsCmd};
use core_model::document::Document;
use core_model::event::Fade;
use core_model::format::{AudioFormat, BitDepth};
use core_model::ids::{SeqId, StripId};
use core_model::lane::LaneInput;
use core_model::mixer::{PluginFormat, PluginRef, StripKind};
use core_model::time::Tick;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};
use vizdaw_ffi::host::{force_offline, reset_host};
use vizdaw_ffi::{
    BitDepthRow, BounceRangeRow, BounceRequestRow, BounceSettingsRow, BounceSourceRow,
    ContainerRow, ForDawRequestRow, JobRow, JobStateRow, ProjectFormatRow, RecordSpecRow,
    ReportKindRow, Session, SlotStatus, VizError, set_bounce_settings,
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

/// The click region's frames: two seconds.
const CLICK_FRAMES: u64 = 96_000;
/// The click: frames 0–47 at this level.
const CLICK: f32 = 0.25;
/// The pulse: a Hann window of `PULSE_WIDTH` frames peaking at `PULSE_PEAK`.
const PULSE_PEAK: u64 = 24_000;
const PULSE_WIDTH: u64 = 960;
/// −60 dBFS.
const QUIET: f32 = 0.001;
const FOLDER: &str = "Song for another DAW";

/// The region's left channel at frame `f`.
fn sample(f: u64) -> f32 {
    let half = PULSE_WIDTH / 2;
    if f < 48 {
        CLICK
    } else if f + half >= PULSE_PEAK && f < PULSE_PEAK + half {
        let x = (f + half - PULSE_PEAK) as f64 / PULSE_WIDTH as f64;
        (0.5 * (0.5 - 0.5 * (2.0 * std::f64::consts::PI * x).cos())) as f32
    } else {
        0.0
    }
}

fn strip_of(d: &Document, want: impl Fn(&StripKind) -> bool) -> StripId {
    d.mixer()
        .order()
        .iter()
        .copied()
        .find(|s| d.mixer().strip(*s).is_ok_and(|x| want(&x.kind)))
        .unwrap()
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

struct Project {
    dir: tempfile::TempDir,
    pkg: PathBuf,
    s: Arc<Session>,
    seq: u64,
    /// "Gtr"'s AudioPath strip.
    gtr: u64,
    /// The built-in synth's strip ("Bass").
    synth: u64,
    /// The master strip.
    master: u64,
    /// The folder exports go in (outside the package).
    out: PathBuf,
}

fn write_caf(path: &Path, rate: u32, frames: u64) {
    let mut w = CafWriter::create(path, rate, 2, BitDepth::Float32, WriteMode::Normal).unwrap();
    w.write(
        &(0..frames)
            .flat_map(|f| [sample(f), sample(f)])
            .collect::<Vec<f32>>(),
    )
    .unwrap();
    w.finish().unwrap();
}

/// `P.vizdaw` in a temporary folder at 48 kHz float: "Song" at 120 BPM in
/// 4/4 with a pickup of `pickup` ticks; track "Gtr" plays `Media/click.caf`
/// from the pickup's start on its AudioPath strip, and "Bass" one note
/// there on the built-in synth. `extra` adds to the document before it is
/// saved, `post` to the package before it is opened.
fn project_with(
    pickup: i64,
    extra: impl FnOnce(&mut Builder, SeqId),
    post: impl FnOnce(&Path),
) -> Project {
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
    let (seq, gtr) = b.sequence("Song", "Gtr", None).unwrap();
    if pickup > 0 {
        b.run(Command::Sequence(SequenceCmd::SetStartOffset {
            seq,
            offset: Tick(pickup),
        }))
        .unwrap();
    }
    let m = b.media("Media/click.caf", CLICK_FRAMES, 2).unwrap();
    b.region(
        seq,
        gtr,
        -pickup,
        m,
        0,
        CLICK_FRAMES,
        0.0,
        Fade::default(),
        Fade::default(),
    )
    .unwrap();
    let synth = b.dest("Synth", "builtin:synth", 0).unwrap();
    let bass = b.lane(seq, "Bass", Some(synth)).unwrap();
    b.notes(seq, bass, &[(-pickup, 60, 0x8000, Q)]).unwrap();
    extra(&mut b, seq);
    let doc = b.finish();
    let gtr_strip = strip_of(&doc, |k| matches!(k, StripKind::AudioPath(_)));
    let synth_strip = strip_of(&doc, |k| *k == StripKind::BuiltinSynth);
    let master = doc.mixer().master();
    core_model::file::save(&doc, &pkg).unwrap();
    write_caf(&pkg.join("Media/click.caf"), 48_000, CLICK_FRAMES);
    post(&pkg);
    let s = Session::open(pkg.display().to_string()).unwrap();
    s.debug_set_trash_dir(Some(dir.path().join("Trash").display().to_string()));
    let out = dir.path().join("out");
    std::fs::create_dir(&out).unwrap();
    Project {
        dir,
        pkg,
        s,
        seq: seq.0,
        gtr: gtr_strip.0,
        synth: synth_strip.0,
        master: master.0,
        out,
    }
}

fn project(pickup: i64, extra: impl FnOnce(&mut Builder, SeqId)) -> Project {
    project_with(pickup, extra, |_| {})
}

fn text(p: &Path) -> String {
    p.display().to_string()
}

/// The default export into `p.out`: 48 kHz 24-bit WAV, the default strips.
fn req(p: &Project) -> ForDawRequestRow {
    ForDawRequestRow {
        seq: p.seq,
        strips: None,
        container: ContainerRow::Wav,
        depth: BitDepthRow::Int24,
        rate: 48_000,
        dither: false,
        path: text(&p.out),
        add_to_project: false,
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

/// Exports `r` and waits for it to end Done.
fn export(p: &Project, r: ForDawRequestRow) -> JobRow {
    let j = wait_job(&p.s, p.s.export_for_daw(r).unwrap());
    assert_eq!(j.state, JobStateRow::Done, "{j:?}");
    j
}

/// Every entry under `dir` (files, folders and links, hidden ones too),
/// relative to it, sorted; folders end in `/`.
fn listing(dir: &Path) -> Vec<String> {
    fn walk(root: &Path, at: &Path, out: &mut Vec<String>) {
        let Ok(rd) = std::fs::read_dir(at) else {
            return;
        };
        for e in rd {
            let e = e.unwrap();
            let p = e.path();
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

/// The files under `dir` (not folders), relative, sorted.
fn files_in(dir: &Path) -> Vec<String> {
    listing(dir)
        .into_iter()
        .filter(|f| !f.ends_with('/'))
        .collect()
}

/// The files in `Media/` and `Media/.incoming/`. Not the waveform cache
/// (`Media/.overviews/`): every import writes its file's overview there,
/// and retention sweeps those no item uses (as after a cancelled bounce).
fn media_files(p: &Project) -> Vec<String> {
    files_in(&p.pkg.join("Media"))
        .into_iter()
        .filter(|f| !f.starts_with(".overviews/"))
        .collect()
}

fn no_partial(dir: &Path) {
    let all = listing(dir);
    assert!(
        all.iter().all(|f| !f.contains(".partial")),
        "a partial is left: {all:?}"
    );
}

/// `n` with thousands separated by a space, as the About file writes it.
fn group(n: u64) -> String {
    let digits = n.to_string();
    let mut out = String::new();
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i) % 3 == 0 {
            out.push(' ');
        }
        out.push(c);
    }
    out
}

/// `(rate, channels, frames, left channel)` of a 16- or 24-bit WAV.
fn read_wav(p: &Path) -> (u32, u16, u64, Vec<f32>) {
    let b = std::fs::read(p).unwrap();
    assert_eq!(&b[0..4], b"RIFF");
    let mut at = 12;
    let (mut rate, mut ch, mut bits) = (0, 0, 0);
    let mut data = Vec::new();
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
            data = body.to_vec();
        }
        at += 8 + n + (n & 1);
    }
    let bytes = usize::from(bits / 8);
    let samples: Vec<f32> = data
        .chunks(bytes)
        .map(|c| match bytes {
            2 => f32::from(i16::from_le_bytes([c[0], c[1]])) / 32_768.0,
            _ => (i32::from_le_bytes([0, c[0], c[1], c[2]]) >> 8) as f32 / 8_388_608.0,
        })
        .collect();
    let frames = (samples.len() / usize::from(ch)) as u64;
    let left = samples.iter().step_by(usize::from(ch)).copied().collect();
    (rate, ch, frames, left)
}

/// The stems in `folder`, by name.
fn stems(folder: &Path) -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = std::fs::read_dir(folder.join("Audio"))
        .unwrap()
        .map(|e| e.unwrap().path())
        .collect();
    v.sort();
    v
}

fn stem_named(folder: &Path, track: &str) -> PathBuf {
    stems(folder)
        .into_iter()
        .find(|p| {
            p.file_name()
                .unwrap()
                .to_string_lossy()
                .ends_with(&format!(" {track}.wav"))
        })
        .unwrap_or_else(|| panic!("no stem of {track} in {:?}", stems(folder)))
}

fn first_loud(x: &[f32]) -> Option<usize> {
    x.iter().position(|v| v.abs() > QUIET)
}

fn about(folder: &Path) -> String {
    std::fs::read_to_string(folder.join("About these files.txt")).unwrap()
}

/// The absolute tick of the first note-on in the first track of the MIDI
/// file that holds one.
fn first_note(path: &Path) -> u64 {
    let bytes = std::fs::read(path).unwrap();
    let (f, _) = smf::read(
        &bytes,
        &core_io::limits::Limits::default(),
        &AtomicBool::new(false),
    )
    .unwrap();
    f.tracks
        .iter()
        .flat_map(|t| t.events.iter())
        .find_map(|(t, e)| match e {
            SmfEvent::Channel(m) if m.bytes[0] & 0xF0 == 0x90 && m.bytes[2] > 0 => Some(*t),
            _ => None,
        })
        .expect("a note")
}

/// Ruling 5, 7: the folder holds exactly the check's files, nothing is
/// left half-written, and the report is the session's last report.
#[test]
fn writes_the_folder_layout() {
    let _g = serial();
    let p = project(0, |_, _| {});
    let check =
        p.s.export_for_daw_check(ForDawRequestRow {
            path: String::new(),
            ..req(&p)
        })
        .unwrap();
    let j = export(&p, req(&p));
    let folder = p.out.join(FOLDER);
    assert_eq!(j.written, vec![text(&folder)]);
    let mut want = check.files.clone();
    want.sort();
    assert_eq!(files_in(&folder), want);
    assert_eq!(
        check.files.len(),
        6,
        "two stems, two MIDI files, the tempo map, About"
    );
    assert_eq!(
        check
            .files
            .iter()
            .filter(|f| f.starts_with("Audio/"))
            .count(),
        2
    );
    assert_eq!(
        check
            .files
            .iter()
            .filter(|f| f.starts_with("MIDI/"))
            .count(),
        2
    );
    no_partial(&p.out);
    assert_eq!(p.s.interchange_report().unwrap(), j.report);
    assert!(j.report.is_some());
    let (rate, ch, frames, _) = read_wav(&stem_named(&folder, "Gtr"));
    assert_eq!((rate, ch), (48_000, 2));
    let text = about(&folder);
    assert!(
        text.contains(&format!("Every audio file is {} frames", group(frames))),
        "{text}"
    );
    assert!(
        p.s.media().unwrap().len() == 1,
        "nothing added without Add to project"
    );
}

/// Review Focus 1: with a pickup of one beat, every stem and the MIDI
/// files start at the pickup's start.
#[test]
fn stems_and_midi_start_together() {
    let _g = serial();
    let p = project(Q, |_, _| {});
    let check =
        p.s.export_for_daw_check(ForDawRequestRow {
            path: String::new(),
            ..req(&p)
        })
        .unwrap();
    let j = export(&p, req(&p));
    let folder = PathBuf::from(&j.written[0]);
    let all = stems(&folder);
    assert_eq!(all.len(), 2);
    let mut lengths = Vec::new();
    let mut sounding = 0;
    for s in &all {
        let (_, _, frames, left) = read_wav(s);
        lengths.push(frames);
        if let Some(at) = first_loud(&left) {
            sounding += 1;
            assert!(at <= 1, "{}: first sound at {at}", s.display());
        }
    }
    assert!(sounding >= 1, "the Gtr stem sounds");
    assert!(lengths.iter().all(|l| *l == lengths[0]), "{lengths:?}");
    let tail = 10 * 48_000;
    assert!(
        check.frames <= lengths[0] && lengths[0] <= check.frames + tail,
        "{} vs {} plus the tail",
        lengths[0],
        check.frames
    );
    assert_eq!(first_note(&folder.join("MIDI/01 Bass.mid")), 0);
    let sentence = "1 beat before bar 1";
    assert!(check.start.contains(sentence), "{}", check.start);
    assert!(about(&folder).contains(sentence), "{}", about(&folder));
}

/// Ruling 2: a pickup of one and a half beats pads the MIDI files to a
/// whole bar, so each stem begins with 2.5 beats of exact silence.
#[test]
fn padded_pickup_gets_leading_silence() {
    let _g = serial();
    let p = project(Q * 3 / 2, |_, _| {});
    let check =
        p.s.export_for_daw_check(ForDawRequestRow {
            path: String::new(),
            ..req(&p)
        })
        .unwrap();
    let j = export(&p, req(&p));
    let folder = PathBuf::from(&j.written[0]);
    let lead = 60_000usize;
    let (_, _, frames, left) = read_wav(&stem_named(&folder, "Gtr"));
    let at = first_loud(&left).unwrap();
    assert!(at.abs_diff(lead) <= 1, "the click at {at}");
    assert!(
        left[..lead - 1].iter().all(|x| *x == 0.0),
        "exact zeros before the click"
    );
    for s in stems(&folder) {
        let (_, _, n, left) = read_wav(&s);
        assert_eq!(n, frames);
        assert!(
            left[..lead - 1].iter().all(|x| *x == 0.0),
            "{}",
            s.display()
        );
    }
    assert!(
        check.start.contains("1 bar before bar 1"),
        "{}",
        check.start
    );
    assert!(check.frames >= lead as u64);
    // The note sits 2.5 beats into the file (960 ticks a beat).
    assert_eq!(first_note(&folder.join("MIDI/01 Bass.mid")), 2_400);
}

/// Review Focus 2, Ruling 1: a taken name, or a dangling link of it,
/// numbers the folder; nothing is replaced.
#[test]
fn a_second_export_numbers_the_folder() {
    let _g = serial();
    let p = project(0, |_, _| {});
    let first = export(&p, req(&p));
    let one = p.out.join(FOLDER);
    assert_eq!(first.written, vec![text(&one)]);
    let snapshot = |d: &Path| -> BTreeMap<String, Vec<u8>> {
        files_in(d)
            .into_iter()
            .map(|f| {
                let b = std::fs::read(d.join(&f)).unwrap();
                (f, b)
            })
            .collect()
    };
    let before = snapshot(&one);
    let listed = listing(&one);
    let second = export(&p, req(&p));
    assert_eq!(
        second.written,
        vec![text(&p.out.join(format!("{FOLDER} 2")))]
    );
    assert_eq!(snapshot(&one), before, "the first folder is untouched");
    assert_eq!(listing(&one), listed);
    std::os::unix::fs::symlink(p.out.join("nowhere"), p.out.join(format!("{FOLDER} 3"))).unwrap();
    let fourth = export(&p, req(&p));
    assert_eq!(
        fourth.written,
        vec![text(&p.out.join(format!("{FOLDER} 4")))]
    );
    no_partial(&p.out);
}

/// Review Focus 4: a track on external MIDI gets its MIDI file, no stem,
/// and a Left out line naming it.
#[test]
fn external_midi_gets_a_midi_file_and_no_stem() {
    let _g = serial();
    let p = project(0, |b, seq| {
        let ext = b.dest("Ext", "port:Outboard", 0).unwrap();
        let lead = b.lane(seq, "Lead", Some(ext)).unwrap();
        b.notes(seq, lead, &[(0, 64, 0x8000, Q)]).unwrap();
    });
    let j = export(&p, req(&p));
    let folder = PathBuf::from(&j.written[0]);
    let files = files_in(&folder);
    assert!(files.contains(&"MIDI/02 Lead.mid".to_string()), "{files:?}");
    assert!(
        files
            .iter()
            .filter(|f| f.starts_with("Audio/"))
            .all(|f| !f.contains("Lead")),
        "{files:?}"
    );
    let report = j.report.unwrap();
    let item = report
        .items
        .iter()
        .find(|i| {
            i.kind == ReportKindRow::LeftOut
                && i.what
                    == "No audio for tracks playing external MIDI (their MIDI files are written)"
        })
        .unwrap_or_else(|| panic!("{report:#?}"));
    assert_eq!(item.details, vec!["Lead".to_string()]);
}

/// Spec §7.6, Ruling 2, F19: "Add" places one track per stem at the
/// pickup's start, as one undo step "Add Stems"; the folder stays.
#[test]
fn add_to_project_is_one_undo_step() {
    let _g = serial();
    let p = project(Q, |_, _| {});
    let before = p.s.debug_document_json();
    let tracks = p.s.lanes(p.seq).unwrap().len();
    let j = export(
        &p,
        ForDawRequestRow {
            add_to_project: true,
            ..req(&p)
        },
    );
    let lanes = p.s.lanes(p.seq).unwrap();
    assert_eq!(lanes.len(), tracks + 2, "one track per stem");
    for l in &lanes[tracks..] {
        let ev = p.s.lane_events(p.seq, l.id).unwrap();
        assert_eq!(ev.events.len(), 1, "{}", l.name);
        assert_eq!(ev.events[0].tick, -Q, "{} at the pickup's start", l.name);
    }
    assert_eq!(
        p.s.snapshot().unwrap().undo_label.as_deref(),
        Some("Add Stems")
    );
    p.s.undo().unwrap();
    assert_eq!(p.s.debug_document_json(), before, "one undo step");
    assert!(PathBuf::from(&j.written[0]).is_dir(), "the folder stays");
}

/// Review Focus 3, Ruling 7, F1, F6: a cancel at every stage leaves no
/// folder, no staging folder, no media and no change.
#[test]
fn cancel_at_every_stage_leaves_nothing() {
    let _g = serial();
    let p = project(0, |_, _| {});
    let doc = p.s.debug_document_json();
    let media = media_files(&p);
    for add in [false, true] {
        let r = ForDawRequestRow {
            add_to_project: add,
            ..req(&p)
        };
        let check = |stage: &str| {
            assert_eq!(listing(&p.out), Vec::<String>::new(), "{stage}, add {add}");
            assert_eq!(media_files(&p), media, "{stage}, add {add}");
            assert_eq!(p.s.debug_document_json(), doc, "{stage}, add {add}");
        };
        // Before the job runs.
        p.s.debug_hold_jobs(true).unwrap();
        let id = p.s.export_for_daw(r.clone()).unwrap();
        p.s.cancel_job(id).unwrap();
        p.s.debug_hold_jobs(false).unwrap();
        assert_eq!(wait_job(&p.s, id).state, JobStateRow::Cancelled);
        check("held");
        // After the first MIDI file.
        p.s.debug_for_daw_cancel_at(Some(1)).unwrap();
        let j = wait_job(&p.s, p.s.export_for_daw(r.clone()).unwrap());
        assert_eq!(j.state, JobStateRow::Cancelled, "{j:?}");
        check("MIDI files");
        // Mid-render.
        p.s.debug_bounce_cancel_at(Some(0.4)).unwrap();
        let j = wait_job(&p.s, p.s.export_for_daw(r.clone()).unwrap());
        assert_eq!(j.state, JobStateRow::Cancelled, "{j:?}");
        check("mid-render");
        // As the bounce finishes: the imports were handed over (F1).
        p.s.debug_bounce_cancel_at(Some(1.0)).unwrap();
        let j = wait_job(&p.s, p.s.export_for_daw(r.clone()).unwrap());
        assert_eq!(j.state, JobStateRow::Cancelled, "{j:?}");
        check("after the render");
        // After the About file, before the rename (F6).
        p.s.debug_for_daw_cancel_at(Some(2)).unwrap();
        let j = wait_job(&p.s, p.s.export_for_daw(r.clone()).unwrap());
        assert_eq!(j.state, JobStateRow::Cancelled, "{j:?}");
        check("before the rename");
    }
}

/// Review Focus 3: a failed "Add" import fails the export, leaving nothing.
#[test]
fn a_failed_add_leaves_nothing() {
    let _g = serial();
    let p = project(0, |_, _| {});
    let doc = p.s.debug_document_json();
    let media = media_files(&p);
    p.s.debug_bounce_fail_import(Some(0)).unwrap();
    let j = wait_job(
        &p.s,
        p.s.export_for_daw(ForDawRequestRow {
            add_to_project: true,
            ..req(&p)
        })
        .unwrap(),
    );
    assert_eq!(j.state, JobStateRow::Failed, "{j:?}");
    assert_eq!(listing(&p.out), Vec::<String>::new());
    assert_eq!(media_files(&p), media);
    assert_eq!(p.s.debug_document_json(), doc);
}

/// Ruling 1, 8, F12, F13: the check names the folder (numbered with a
/// path) and the files, and writes nothing.
#[test]
fn check_writes_nothing_and_names_the_folder() {
    let _g = serial();
    let p = project(0, |_, _| {});
    let c =
        p.s.export_for_daw_check(ForDawRequestRow {
            path: String::new(),
            ..req(&p)
        })
        .unwrap();
    assert_eq!(c.folder, FOLDER);
    // Mixer order: the output paths' strips come before the instruments'.
    assert_eq!(
        c.files,
        vec![
            "MIDI/01 Bass.mid",
            "MIDI/All MIDI.mid",
            "Tempo map.mid",
            "Audio/01 Gtr.wav",
            "Audio/02 Bass.wav",
            "About these files.txt",
        ]
    );
    assert_eq!(c.strips, vec![p.gtr, p.synth]);
    assert_eq!(c.start, "Every file starts at bar 1.");
    assert!(c.frames > 0);
    assert!(
        c.report.summary.starts_with("Exported"),
        "{}",
        c.report.summary
    );
    std::fs::create_dir(p.out.join(FOLDER)).unwrap();
    let listed = listing(&p.out);
    let c = p.s.export_for_daw_check(req(&p)).unwrap();
    assert_eq!(c.folder, format!("{FOLDER} 2"));
    assert_eq!(listing(&p.out), listed, "nothing written");
}

/// Spec §10's hostile rows: refused at once (`Model`) or failed in the
/// job with a sentence naming the path; never `Internal`, nothing written.
#[test]
fn hostile_requests_are_refused() {
    let _g = serial();
    let p = project(0, |b, _| {
        b.bus("Bus").unwrap();
    });
    let media = media_files(&p);
    let doc = p.s.debug_document_json();
    let at_once = |r: ForDawRequestRow, what: &str| {
        for result in [
            p.s.export_for_daw(r.clone()).map(|_| ()),
            p.s.export_for_daw_check(r).map(|_| ()),
        ] {
            assert!(
                matches!(result, Err(VizError::Model { .. })),
                "{what}: {result:?}"
            );
        }
    };
    let long = format!("/{}", "a".repeat(5_000));
    for (path, what) in [
        ("", "empty"),
        ("relative/folder", "relative"),
        ("/tmp/a\0b", "NUL"),
        (long.as_str(), "5 000 bytes"),
        ("/tmp/../tmp", ".."),
    ] {
        let r = ForDawRequestRow {
            path: path.into(),
            ..req(&p)
        };
        if path.is_empty() {
            // An empty path is a check without a folder; the export refuses it.
            let e = p.s.export_for_daw(r).unwrap_err();
            assert!(matches!(e, VizError::Model { .. }), "{what}: {e:?}");
        } else {
            at_once(r, what);
        }
    }
    for (strips, what) in [
        (vec![999], "unknown strip"),
        (vec![p.gtr, p.gtr], "twice"),
        (vec![p.master], "master"),
    ] {
        at_once(
            ForDawRequestRow {
                strips: Some(strips),
                ..req(&p)
            },
            what,
        );
    }
    for rate in [0, 7_999, 384_001] {
        at_once(ForDawRequestRow { rate, ..req(&p) }, "rate");
    }
    at_once(
        ForDawRequestRow {
            seq: 9_999,
            ..req(&p)
        },
        "sequence",
    );
    at_once(
        ForDawRequestRow {
            strips: Some(Vec::new()),
            add_to_project: true,
            ..req(&p)
        },
        "MIDI only with Add",
    );
    // In the job.
    let base = p.dir.path().join("hostile");
    std::fs::create_dir(&base).unwrap();
    let file = base.join("a file");
    std::fs::write(&file, b"x").unwrap();
    let fifo = base.join("fifo");
    assert!(
        std::process::Command::new("mkfifo")
            .arg(&fifo)
            .status()
            .unwrap()
            .success()
    );
    let (a, b) = (base.join("loop a"), base.join("loop b"));
    std::os::unix::fs::symlink(&b, &a).unwrap();
    std::os::unix::fs::symlink(&a, &b).unwrap();
    let other = base.join("Other.vizdaw").join("inside");
    std::fs::create_dir_all(&other).unwrap();
    let listed = listing(&base);
    for path in [
        file.clone(),
        fifo,
        PathBuf::from("/dev/zero"),
        a,
        base.join("missing"),
        p.pkg.join("Media"),
        other,
    ] {
        let r = ForDawRequestRow {
            path: text(&path),
            ..req(&p)
        };
        let j = wait_job(&p.s, p.s.export_for_daw(r).unwrap());
        assert_eq!(j.state, JobStateRow::Failed, "{}: {j:?}", path.display());
        let m = j.message.unwrap_or_default();
        assert!(m.contains(&text(&path)), "{}: {m}", path.display());
        assert!(!m.starts_with("internal"), "{m}");
    }
    assert_eq!(listing(&base), listed, "nothing written");
    assert_eq!(listing(&p.out), Vec::<String>::new());
    assert_eq!(media_files(&p), media);
    assert_eq!(p.s.debug_document_json(), doc);
}

fn mix_req(p: &Project, dest: &Path) -> BounceRequestRow {
    BounceRequestRow {
        seq: p.seq,
        source: BounceSourceRow::Mix,
        range: BounceRangeRow::Ticks {
            start: 0,
            end: 4 * Q,
        },
        container: ContainerRow::Wav,
        depth: BitDepthRow::Int24,
        rate: 48_000,
        dither: false,
        path: Some(text(dest)),
        add_to_project: false,
        replace: false,
    }
}

fn busy<T: std::fmt::Debug>(r: Result<T, VizError>, what: &str) {
    assert!(matches!(r, Err(VizError::Busy { .. })), "{what}: {r:?}");
}

/// Ruling 9, F2: one interchange job at a time; "Add" waits for media
/// jobs; media jobs and external edits wait for the export; closing the
/// session stops it.
#[test]
fn busy_states_refuse() {
    let _g = serial();
    let p = project_with(
        0,
        |b, _| {
            b.media("Media/other.caf", 48_000, 2).unwrap();
        },
        |pkg| write_caf(&pkg.join("Media/other.caf"), 44_100, 48_000),
    );
    let region =
        p.s.lane_events(p.seq, p.s.lanes(p.seq).unwrap()[0].id)
            .unwrap()
            .events[0]
            .id;
    p.s.debug_hold_jobs(true).unwrap();
    let id = p.s.export_for_daw(req(&p)).unwrap();
    busy(p.s.bounce(mix_req(&p, &p.out.join("mix.wav"))), "bounce");
    busy(
        p.s.export_midi(vizdaw_ffi::SmfExportRequestRow {
            seq: p.seq,
            lanes: Vec::new(),
            path: text(&p.out.join("a.mid")),
            replace: false,
            type0: false,
            ppq: 960,
            mpe: true,
        }),
        "export_midi",
    );
    busy(
        p.s.import_midi(vizdaw_ffi::MidiImportRequestRow {
            path: text(&p.out.join("a.mid")),
            ..import_req(p.seq)
        }),
        "import_midi",
    );
    busy(p.s.export_for_daw(req(&p)), "a second export");
    busy(p.s.convert_media(), "convert");
    busy(
        p.s.set_project_format(ProjectFormatRow {
            sample_rate: 44_100,
            depth: BitDepthRow::Int24,
        }),
        "format",
    );
    busy(p.s.open_external(region), "external edit");
    p.s.cancel_job(id).unwrap();
    p.s.debug_hold_jobs(false).unwrap();
    assert_eq!(wait_job(&p.s, id).state, JobStateRow::Cancelled);

    // While a bounce is held.
    p.s.debug_hold_jobs(true).unwrap();
    let b = p.s.bounce(mix_req(&p, &p.out.join("mix.wav"))).unwrap();
    busy(p.s.export_for_daw(req(&p)), "under a bounce");
    p.s.cancel_job(b).unwrap();
    p.s.debug_hold_jobs(false).unwrap();
    assert_eq!(wait_job(&p.s, b).state, JobStateRow::Cancelled);

    // "Add" while a Convert job is held.
    p.s.debug_hold_jobs(true).unwrap();
    let c = p.s.convert_media().unwrap();
    busy(
        p.s.export_for_daw(ForDawRequestRow {
            add_to_project: true,
            ..req(&p)
        }),
        "Add under a conversion",
    );
    p.s.cancel_job(c).unwrap();
    p.s.debug_hold_jobs(false).unwrap();
    wait_job(&p.s, c);

    // Closing the session stops a running export (F2).
    let media = media_files(&p);
    p.s.debug_hold_jobs(true).unwrap();
    p.s.export_for_daw(ForDawRequestRow {
        add_to_project: true,
        ..req(&p)
    })
    .unwrap();
    p.s.close().unwrap();
    assert_eq!(listing(&p.out), Vec::<String>::new(), "no folder");
    assert_eq!(media_files(&p), media, "no imports");

    // While recording (offline, an armed track).
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
    busy(
        r.export_for_daw(ForDawRequestRow {
            seq: rseq.0,
            ..req(&p)
        }),
        "while recording",
    );
    r.stop().unwrap();
    r.close().unwrap();
}

fn import_req(seq: u64) -> vizdaw_ffi::MidiImportRequestRow {
    vizdaw_ffi::MidiImportRequestRow {
        path: String::new(),
        new_project: false,
        seq,
        lane: None,
        tick: 0,
        channel_map: Vec::new(),
        split_channels: None,
        mpe: false,
        use_tempo: false,
    }
}

/// F7: at another rate the lead is scaled with the audio, and the check's
/// and the About file's frames are the written files' frames.
// Rate conversion needs the system importer.
#[cfg(target_os = "macos")]
#[test]
fn stems_align_at_another_rate() {
    let _g = serial();
    set_bounce_settings(BounceSettingsRow { tail_cap_ms: 0 });
    let p = project(Q * 3 / 2, |_, _| {});
    let r = ForDawRequestRow {
        rate: 44_100,
        ..req(&p)
    };
    let check =
        p.s.export_for_daw_check(ForDawRequestRow {
            path: String::new(),
            ..r.clone()
        })
        .unwrap();
    let j = export(&p, r);
    let folder = PathBuf::from(&j.written[0]);
    let (rate, _, frames, left) = read_wav(&stem_named(&folder, "Gtr"));
    assert_eq!(rate, 44_100);
    assert_eq!(check.frames, frames, "the check's frames are the file's");
    assert!(
        about(&folder).contains(&format!("Every audio file is {} frames", group(frames))),
        "{}",
        about(&folder)
    );
    // The pulse's peak, PULSE_PEAK frames after the region's start at 48 kHz.
    let peak = left
        .iter()
        .enumerate()
        .skip(1_000)
        .max_by(|a, b| a.1.abs().total_cmp(&b.1.abs()))
        .unwrap()
        .0 as i64;
    let lead = peak - (PULSE_PEAK as i64 * 44_100 / 48_000);
    assert!((lead - 55_125).abs() <= 1, "lead {lead}");
    set_bounce_settings(BounceSettingsRow {
        tail_cap_ms: 10_000,
    });
}

/// Review Focus 5: the plug-in copies end with the export, so a bounce
/// after it runs.
#[test]
fn plugin_copies_end_with_the_export() {
    let _g = serial();
    let p = project(0, |b, _| {
        let path = strip_of(b.doc(), |k| matches!(k, StripKind::AudioPath(_)));
        b.insert(path, plugin("vizdaw.test.gain", "Test Gain"))
            .unwrap();
    });
    idle_until(&p.s, |s| {
        s.plugin_slots().unwrap()[0].status == SlotStatus::Live
    });
    export(&p, req(&p));
    assert_eq!(vizdaw_ffi::plugins::bounce_hosts(), 0, "the copies ended");
    let j = wait_job(
        &p.s,
        p.s.bounce(mix_req(&p, &p.out.join("mix.wav"))).unwrap(),
    );
    assert_eq!(j.state, JobStateRow::Done, "{j:?}");
}

/// Review minor 3: "Add" with default strips that are empty (only external
/// MIDI plays) is refused as `Some([])` is.
#[test]
fn add_with_no_default_stems_is_refused() {
    let _g = serial();
    let dir = tempfile::tempdir().unwrap();
    let mut b = Builder::new();
    let ext = b.dest("Ext", "port:Outboard", 0).unwrap();
    let (seq, lead) = b.sequence("S", "Lead", Some(ext)).unwrap();
    b.notes(seq, lead, &[(0, 64, 0x8000, Q)]).unwrap();
    let s = Session::from_document(b.finish()).unwrap();
    s.debug_set_trash_dir(Some(text(&dir.path().join("Trash"))));
    let r = ForDawRequestRow {
        seq: seq.0,
        strips: None,
        container: ContainerRow::Wav,
        depth: BitDepthRow::Int24,
        rate: 48_000,
        dither: false,
        path: text(dir.path()),
        add_to_project: true,
    };
    let c = s
        .export_for_daw_check(ForDawRequestRow {
            add_to_project: false,
            ..r.clone()
        })
        .unwrap();
    assert!(c.strips.is_empty(), "no default stems: {:?}", c.strips);
    for result in [
        s.export_for_daw(r.clone()).map(|_| ()),
        s.export_for_daw_check(r).map(|_| ()),
    ] {
        match result {
            Err(VizError::Model { message }) => assert!(message.contains("strip"), "{message}"),
            other => panic!("{other:?}"),
        }
    }
    assert_eq!(listing(dir.path()), Vec::<String>::new());
    s.close().unwrap();
}

/// Review minor 10: MIDI files only write no audio, so the audio format
/// is not looked at.
#[test]
fn midi_only_ignores_the_audio_format() {
    let _g = serial();
    let p = project(0, |_, _| {});
    let r = ForDawRequestRow {
        strips: Some(Vec::new()),
        rate: 0,
        ..req(&p)
    };
    let c = p.s.export_for_daw_check(r.clone()).unwrap();
    assert!(
        c.files.iter().all(|f| !f.starts_with("Audio/")),
        "{:?}",
        c.files
    );
    let j = export(&p, r);
    let folder = PathBuf::from(&j.written[0]);
    assert_eq!(std::fs::read_dir(folder.join("Audio")).unwrap().count(), 0);
    assert!(
        about(&folder).contains("Audio: none; no strips were chosen."),
        "{}",
        about(&folder)
    );
}
