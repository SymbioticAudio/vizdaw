//! The external sample editor in the core (plan 5f-3 Task 3; spec §11,
//! §6.5; Rulings 2, 3, 6; pre-flight C1, I1–I5, I10, I11, M3, M6–M8, M10;
//! owner answers 2026-11-03). Projects are built here, not from the demo
//! (pre-flight C1: the demo's media does not exist): a 48 kHz project in
//! the depth under test, saved in a temporary folder (or untitled) with a
//! private Trash, with a generated 48 kHz stereo CAF imported at tick 0
//! (the CAF importer reads it on Linux too), gain −6 dB, fades and a split
//! at 1 s, so two regions use the tone. One test at a time: the engine
//! host is process-wide.
//!
//! Applying an edit imports a WAV: those tests are macOS only.

mod common;

use core_engine::fixtures::Builder;
use core_media::caf::{CafReader, CafWriter, WriteMode};
use core_model::command::{Command, MediaCmd};
use core_model::document::MediaOrigin;
use core_model::event::Fade;
use core_model::format::BitDepth;
use core_model::ids::AnyId;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Instant;
use vizdaw_ffi::bounce::{BounceRangeRow, BounceRequestRow, BounceSourceRow, ContainerRow};
use vizdaw_ffi::external::copy_name;
use vizdaw_ffi::host::{force_offline, reset_host};
use vizdaw_ffi::{
    AudioEdit, BitDepthRow, EditCommand, EventEdit, EventKindRow, EventRow, ExternalEditRow,
    ExternalEditStateRow, FadeRow, FadeShapeRow, ImportTargetRow, JobRow, JobStateRow,
    ProjectFormatRow, SequenceEdit, Session, VizError,
};
#[cfg(target_os = "macos")]
use vizdaw_ffi::{MediaOriginRow, MediaRow};

static SERIAL: Mutex<()> = Mutex::new(());

fn serial() -> MutexGuard<'static, ()> {
    let g = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    force_offline();
    reset_host();
    g
}

/// The tone: 2 s at 48 kHz.
const FRAMES: u64 = 96_000;
const GONE: &str = "The edit copy is gone; Open in External Editor again.";

fn value(f: u64) -> f32 {
    ((f % 1000) as f32 / 1000.0 - 0.5) * 0.8
}

struct Project {
    dir: tempfile::TempDir,
    /// The package (`None`: untitled).
    pkg: Option<PathBuf>,
    s: Arc<Session>,
    seq: u64,
    lane: u64,
    /// The tone's media item.
    media: u64,
    /// The split's first part (the imported region's ID).
    first: u64,
    /// Its second part (from 1 s, with the fade out): the region edited.
    second: u64,
}

impl Project {
    fn trash(&self) -> PathBuf {
        self.dir.path().join("Trash")
    }

    fn pkg(&self) -> PathBuf {
        self.pkg.clone().unwrap()
    }

    fn region(&self, id: u64) -> EventRow {
        region_rows(&self.s, self.seq, self.lane)
            .into_iter()
            .find(|r| r.id == id)
            .unwrap()
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

fn region_rows(s: &Session, seq: u64, lane: u64) -> Vec<EventRow> {
    s.lane_events(seq, lane)
        .unwrap()
        .events
        .into_iter()
        .filter(|e| e.kind == EventKindRow::AudioRegion)
        .collect()
}

fn fade(n: i64) -> FadeRow {
    FadeRow {
        length: n,
        shape: FadeShapeRow::Linear,
    }
}

/// A 48 kHz project in `depth`, saved as `P.vizdaw` (`saved`) or untitled,
/// with a private Trash: sequence "Song" whose lane plays `tone.caf`
/// (imported: 48 kHz stereo float, value(f) left, −value(f) right) from
/// tick 0 at −6 dB with a 4800-frame fade in and a 2400-frame fade out,
/// split at 1 s, so two regions use the tone (pre-flight C1).
fn build(depth: BitDepthRow, saved: bool) -> Project {
    let dir = tempfile::tempdir().unwrap();
    let s = Session::new_project(ProjectFormatRow {
        sample_rate: 48_000,
        depth,
    })
    .unwrap();
    s.debug_set_trash_dir(Some(dir.path().join("Trash").display().to_string()));
    let pkg = saved.then(|| dir.path().join("P.vizdaw"));
    if let Some(p) = &pkg {
        s.save_as(p.display().to_string()).unwrap();
    }
    s.edit(
        EditCommand::Sequence {
            edit: SequenceEdit::New {
                name: "Song".into(),
            },
        },
        None,
    )
    .unwrap();
    let seq = s.snapshot().unwrap().sequences[0].id;
    let lane = s.lanes(seq).unwrap()[0].id;
    let src = dir.path().join("tone.caf");
    let mut w = CafWriter::create(&src, 48_000, 2, BitDepth::Float32, WriteMode::Normal).unwrap();
    w.write(
        &(0..FRAMES)
            .flat_map(|f| [value(f), -value(f)])
            .collect::<Vec<f32>>(),
    )
    .unwrap();
    w.finish().unwrap();
    let job = s
        .import_audio(
            vec![src.display().to_string()],
            Some(ImportTargetRow {
                seq,
                lane: Some(lane),
                tick: 0,
            }),
        )
        .unwrap();
    assert_eq!(wait_job(&s, job).state, JobStateRow::Done);
    let media = s.media().unwrap()[0].id;
    let first = region_rows(&s, seq, lane)[0].id;
    for edit in [
        AudioEdit::SetGain {
            event: first,
            gain_db: -6.0,
        },
        AudioEdit::SetFades {
            event: first,
            fade_in: fade(4800),
            fade_out: fade(2400),
        },
        AudioEdit::Split {
            event: first,
            at: s.ticks_at(seq, vec![1.0]).unwrap()[0],
        },
    ] {
        s.edit(EditCommand::Audio { edit }, None).unwrap();
    }
    let second = region_rows(&s, seq, lane)
        .into_iter()
        .find(|r| r.id != first)
        .unwrap()
        .id;
    let p = Project {
        dir,
        pkg,
        s,
        seq,
        lane,
        media,
        first,
        second,
    };
    let r = p.region(second);
    assert!(r.source_offset > 0 && r.media == Some(media), "{r:?}");
    p
}

/// The package's `Media/*.caf` names (pre-flight M8: `Media/Edits`,
/// `.overviews` and `.incoming` are left out).
fn cafs(root: &Path) -> BTreeSet<String> {
    std::fs::read_dir(root.join("Media"))
        .unwrap()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| n.ends_with(".caf"))
        .collect()
}

/// `(rate, channels, bits, float, frames, samples)` of a WAV (5f-2's reader).
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

/// Frames `[start, start + n)` of media `id`'s file under `root`, as the
/// integers (or float bits) a WAV of `depth` holds.
fn media_samples(
    p: &Project,
    root: &Path,
    id: u64,
    start: u64,
    n: u64,
    depth: BitDepth,
) -> Vec<i64> {
    let row =
        p.s.media()
            .unwrap()
            .into_iter()
            .find(|r| r.id == id)
            .unwrap();
    let r = CafReader::open(&root.join(&row.path)).unwrap();
    let ch = usize::from(r.info().channels);
    let mut buf = vec![0f32; n as usize * ch];
    assert_eq!(r.read(start, &mut buf, &mut Vec::new()).unwrap() as u64, n);
    buf.iter()
        .map(|x| match depth {
            BitDepth::Int16 => (x * 32_768.0).round() as i64,
            BitDepth::Int24 => (x * 8_388_608.0).round() as i64,
            BitDepth::Float32 => i64::from(x.to_bits()),
        })
        .collect()
}

/// A 16-bit WAV of silence (`media_pool.rs`'s, any channel count;
/// pre-flight M7: `PcmWriter` refuses three channels).
#[cfg(target_os = "macos")]
fn wav_at(path: &Path, rate: u32, channels: u16, frames: u32) {
    let data = frames * u32::from(channels) * 2;
    let mut b = Vec::new();
    b.extend_from_slice(b"RIFF");
    b.extend_from_slice(&(36 + data).to_le_bytes());
    b.extend_from_slice(b"WAVEfmt ");
    b.extend_from_slice(&16u32.to_le_bytes());
    b.extend_from_slice(&1u16.to_le_bytes());
    b.extend_from_slice(&channels.to_le_bytes());
    b.extend_from_slice(&rate.to_le_bytes());
    b.extend_from_slice(&(rate * u32::from(channels) * 2).to_le_bytes());
    b.extend_from_slice(&(channels * 2).to_le_bytes());
    b.extend_from_slice(&16u16.to_le_bytes());
    b.extend_from_slice(b"data");
    b.extend_from_slice(&data.to_le_bytes());
    b.resize(b.len() + data as usize, 0);
    std::fs::write(path, b).unwrap();
}

/// The editor's save: the copy removed and written again (`wav_at`).
#[cfg(target_os = "macos")]
fn replace_with_wav(copy: &Path, channels: u16, rate: u32, frames: u32) {
    let _ = std::fs::remove_file(copy);
    wav_at(copy, rate, channels, frames);
}

#[cfg(target_os = "macos")]
fn media_row(s: &Session, id: u64) -> MediaRow {
    s.media().unwrap().into_iter().find(|m| m.id == id).unwrap()
}

/// Ruling 2 (pre-flight I4): the copy is the region's span — not its gain
/// or fades — as a WAV in the project's rate and depth, the media's own
/// samples bit for bit (no dither); a second Open takes the next number.
#[test]
fn the_copy_is_the_span_as_wav_in_the_project_format() {
    let _g = serial();
    for (row, depth) in [
        (BitDepthRow::Int16, BitDepth::Int16),
        (BitDepthRow::Int24, BitDepth::Int24),
        (BitDepthRow::Float32, BitDepth::Float32),
    ] {
        let p = build(row, true);
        let pkg = p.pkg();
        let r = p.region(p.second);
        assert!(
            r.gain_db == -6.0 && r.fade_out.length == 2400,
            "the region has gain and a fade: {r:?}"
        );
        let e = p.s.open_external(p.second).unwrap();
        assert_eq!((e.event, e.from), (p.second, p.media));
        assert_eq!(
            PathBuf::from(&e.path),
            pkg.join("Media/Edits/tone edit 1.wav")
        );
        let (rate, ch, bits, float, frames, samples) = read_wav(Path::new(&e.path));
        assert_eq!(
            (rate, ch, bits, float, frames),
            (
                48_000,
                2,
                depth.bits() as u16,
                depth == BitDepth::Float32,
                r.frames
            ),
            "{depth:?}"
        );
        assert_eq!(
            samples,
            media_samples(&p, &pkg, p.media, r.source_offset, r.frames, depth),
            "{depth:?}: the media's samples"
        );
        assert_eq!(
            PathBuf::from(p.s.open_external(p.second).unwrap().path),
            pkg.join("Media/Edits/tone edit 2.wav")
        );
        p.s.close().unwrap();
    }
}

/// Review Focus 4 (pre-flight M3): safe names, free numbers, the folder.
#[test]
fn edit_copies_get_safe_free_names() {
    assert_eq!(copy_name("a/b:c\u{7}", 3), "a-b-c- edit 3.wav");
    assert_eq!(
        copy_name(".hidden", 1),
        "-hidden edit 1.wav",
        "never hidden"
    );
    assert_eq!(copy_name("", 1), "Audio edit 1.wav");
    assert_eq!(copy_name("   ", 1), "Audio edit 1.wav");
    assert_eq!(
        copy_name(&"a".repeat(300), 2),
        format!("{} edit 2.wav", "a".repeat(200))
    );
    assert_eq!(
        copy_name(&"€".repeat(100), 1),
        format!("{} edit 1.wav", "€".repeat(66)),
        "cut at a character boundary, within 200 bytes"
    );
    let _g = serial();
    let p = build(BitDepthRow::Int24, true);
    let pkg = p.pkg();
    assert!(!pkg.join("Media/Edits").exists());
    p.s.rename_media(p.media, "a/b:c".into()).unwrap();
    let first = p.s.open_external(p.second).unwrap().path;
    assert_eq!(
        PathBuf::from(&first),
        pkg.join("Media/Edits/a-b-c edit 1.wav"),
        "the folder is made, the name made safe"
    );
    // A copy from an earlier edit is never written over.
    std::fs::write(pkg.join("Media/Edits/a-b-c edit 2.wav"), b"an earlier edit").unwrap();
    assert_eq!(
        PathBuf::from(p.s.open_external(p.second).unwrap().path),
        pkg.join("Media/Edits/a-b-c edit 3.wav")
    );
    assert_eq!(
        std::fs::read(pkg.join("Media/Edits/a-b-c edit 2.wav")).unwrap(),
        b"an earlier edit"
    );
    p.s.close().unwrap();
    // A file where the folder goes: refused with a message, nothing written.
    let q = build(BitDepthRow::Int24, true);
    let qpkg = q.pkg();
    std::fs::write(qpkg.join("Media/Edits"), b"not a folder").unwrap();
    let before = cafs(&qpkg);
    let e = q.s.open_external(q.second).unwrap_err();
    assert!(
        matches!(&e, VizError::Io { message } if message.contains("Media/Edits")),
        "{e}"
    );
    assert_eq!(
        std::fs::read(qpkg.join("Media/Edits")).unwrap(),
        b"not a folder"
    );
    assert_eq!(cafs(&qpkg), before);
    assert_eq!(
        q.s.external_edit(q.second).unwrap(),
        ExternalEditStateRow::NoEdit
    );
    q.s.close().unwrap();
}

/// Review Focus 5 (pre-flight I11): hostile and racing calls are refused,
/// and nothing is written. On Linux the held job ends Failed (the CAF
/// importer reads no WAV), which the checks allow.
#[test]
fn hostile_external_calls_are_refused() {
    let _g = serial();
    let p = build(BitDepthRow::Int24, true);
    let pkg = p.pkg();
    let model = |r: Result<u64, VizError>| matches!(r, Err(VizError::Model { .. }));
    // Open: a note, an unknown ID, an offline region.
    let mut note = EventRow::blank(0, 0, EventKindRow::Note);
    note.pitch = 60;
    note.velocity = 0x8000;
    note.duration = 960;
    p.s.edit(
        EditCommand::Event {
            edit: EventEdit::Insert {
                seq: p.seq,
                lane: p.lane,
                events: vec![note],
            },
        },
        None,
    )
    .unwrap();
    let note =
        p.s.lane_events(p.seq, p.lane)
            .unwrap()
            .events
            .into_iter()
            .find(|e| e.kind == EventKindRow::Note)
            .unwrap()
            .id;
    for ev in [note, 999_999] {
        assert!(
            matches!(p.s.open_external(ev), Err(VizError::Model { .. })),
            "{ev}"
        );
        assert!(p.s.external_edit(ev).is_err(), "{ev}");
    }
    let tone = pkg.join("Media/tone.caf");
    let away = p.dir.path().join("tone-away.caf");
    std::fs::rename(&tone, &away).unwrap();
    p.s.recheck_media().unwrap();
    let e = p.s.open_external(p.second).unwrap_err();
    assert_eq!(
        e.to_string(),
        "The region's audio is offline; relink it first."
    );
    std::fs::rename(&away, &tone).unwrap();
    p.s.recheck_media().unwrap();
    assert!(!pkg.join("Media/Edits").exists(), "nothing was written");
    // Apply: no copy yet; then every path that is not the copy.
    let copy = pkg
        .join("Media/Edits/tone edit 1.wav")
        .display()
        .to_string();
    let e = p.s.apply_external_edit(p.second, copy.clone()).unwrap_err();
    assert_eq!(
        e.to_string(),
        "This region has no external edit; use Open in External Editor."
    );
    assert_eq!(p.s.open_external(p.second).unwrap().path, copy);
    let before = cafs(&pkg);
    for bad in [
        tone.display().to_string(),
        "Media/Edits/tone edit 1.wav".to_string(),
        format!("/{}", "a".repeat(70_000)),
        format!(
            "{}/../Edits/tone edit 1.wav",
            pkg.join("Media/Edits").display()
        ),
        // Odd characters (dispatch): a NUL, a newline. (A doubled `/`
        // is the same path to `Path`'s `==`, so it is the copy.)
        format!("{}/tone edit 1.wav\0", pkg.join("Media/Edits").display()),
        format!("{}/tone edit 1.wav\n", pkg.join("Media/Edits").display()),
        String::new(),
    ] {
        assert!(
            model(p.s.apply_external_edit(p.second, bad.clone())),
            "{}",
            &bad[..bad.len().min(80)]
        );
    }
    assert!(
        model(p.s.apply_external_edit(p.first, copy.clone())),
        "another region's copy"
    );
    assert_eq!(cafs(&pkg), before, "nothing was imported");
    // The copy goes missing: gone.
    let moved = p.dir.path().join("away.wav");
    std::fs::rename(&copy, &moved).unwrap();
    assert_eq!(
        p.s.apply_external_edit(p.second, copy.clone())
            .unwrap_err()
            .to_string(),
        GONE
    );
    std::fs::rename(&moved, &copy).unwrap();
    // Races: one apply at a time, and none beside a Format job, both ways.
    let int16 = ProjectFormatRow {
        sample_rate: 48_000,
        depth: BitDepthRow::Int16,
    };
    p.s.debug_hold_jobs(true).unwrap();
    let job = p.s.apply_external_edit(p.second, copy.clone()).unwrap();
    assert!(
        matches!(
            p.s.apply_external_edit(p.second, copy.clone()),
            Err(VizError::Busy { .. })
        ),
        "a second apply while one is unapplied"
    );
    assert!(
        matches!(p.s.set_project_format(int16), Err(VizError::Busy { .. })),
        "a Format job while an edit is unapplied"
    );
    p.s.debug_hold_jobs(false).unwrap();
    wait_job(&p.s, job);
    p.s.debug_hold_jobs(true).unwrap();
    let format = p.s.set_project_format(int16).unwrap();
    assert!(
        matches!(
            p.s.apply_external_edit(p.second, copy.clone()),
            Err(VizError::Busy { .. })
        ),
        "an apply while a Format job is unapplied"
    );
    assert!(
        matches!(p.s.open_external(p.second), Err(VizError::Busy { .. })),
        "an Open while a Format job is unapplied"
    );
    p.s.debug_hold_jobs(false).unwrap();
    wait_job(&p.s, format);
    // A running Bounce makes an apply wait too (`media_jobs_idle`; dispatch).
    p.s.debug_hold_jobs(true).unwrap();
    let bounce =
        p.s.bounce(BounceRequestRow {
            seq: p.seq,
            source: BounceSourceRow::Mix,
            range: BounceRangeRow::Song,
            container: ContainerRow::Wav,
            depth: BitDepthRow::Int16,
            rate: 48_000,
            dither: false,
            path: Some(p.dir.path().join("mix.wav").display().to_string()),
            add_to_project: false,
            replace: false,
        })
        .unwrap();
    assert!(
        matches!(
            p.s.apply_external_edit(p.second, copy.clone()),
            Err(VizError::Busy { .. })
        ),
        "an apply while a Bounce job is unapplied"
    );
    p.s.cancel_job(bounce).unwrap();
    p.s.debug_hold_jobs(false).unwrap();
    wait_job(&p.s, bounce);
    // A region in a sequence that is not the current one (dispatch): the
    // calls take event IDs, so it is opened like any other.
    p.s.edit(
        EditCommand::Sequence {
            edit: SequenceEdit::New {
                name: "Other".into(),
            },
        },
        None,
    )
    .unwrap();
    let other = p.s.open_external(p.first).unwrap();
    assert_eq!(other.from, p.media);
    assert!(other.path.ends_with("tone edit 2.wav"), "{}", other.path);
    p.s.close().unwrap();
}

/// Ruling 3 (pre-flight I1): each Save keeps the live copy; closing sends
/// it to the Trash, never unlinked.
#[test]
fn copies_are_kept_until_close_then_trashed() {
    let _g = serial();
    let p = build(BitDepthRow::Int24, true);
    let copy = PathBuf::from(p.s.open_external(p.second).unwrap().path);
    for _ in 0..2 {
        p.s.save().unwrap();
        assert!(copy.is_file(), "kept while the project is open");
        assert!(!p.trash().join("tone edit 1.wav").exists());
    }
    p.s.close().unwrap();
    assert!(!copy.exists(), "closing lets it go");
    assert!(
        p.trash().join("tone edit 1.wav").is_file(),
        "to the Trash, never unlinked"
    );
    assert!(p.pkg().join("Media/tone.caf").is_file(), "the media stays");
}

/// Ruling 3 (pre-flight I1, M14): a titled Save As neither moves nor
/// copies the copy; it stays watched at the old package (and on macOS
/// still applies), and closing the new package leaves it.
#[test]
fn save_as_leaves_the_copies_where_they_are() {
    let _g = serial();
    let p = build(BitDepthRow::Int24, true);
    let old = p.pkg();
    let row = p.s.open_external(p.second).unwrap();
    let copy = PathBuf::from(&row.path);
    let new = p.dir.path().join("Q.vizdaw");
    p.s.save_as(new.display().to_string()).unwrap();
    assert!(copy.is_file() && copy.starts_with(&old), "not moved");
    assert!(!new.join("Media/Edits").exists(), "not copied");
    assert_eq!(
        p.s.external_edit(p.second).unwrap(),
        ExternalEditStateRow::Copy {
            row: ExternalEditRow {
                event: p.second,
                path: row.path.clone(),
                from: p.media,
            }
        }
    );
    p.s.save().unwrap();
    assert!(
        copy.is_file(),
        "the new package's tidy leaves the old one's files"
    );
    #[cfg(target_os = "macos")]
    {
        // macOS only: importing a WAV goes through ExtAudioFile (the CAF
        // importer reads CAF only).
        replace_with_wav(&copy, 2, 48_000, 24_000);
        let job = p.s.apply_external_edit(p.second, row.path.clone()).unwrap();
        assert_eq!(wait_job(&p.s, job).state, JobStateRow::Done);
        let m = media_row(&p.s, p.region(p.second).media.unwrap());
        assert!(new.join(&m.path).is_file(), "imported into the new package");
    }
    p.s.close().unwrap();
    assert!(
        copy.is_file(),
        "the old package keeps it until it is saved or closed"
    );
}

/// Fix round 1 (review I2): Save As onto the same package spelled another
/// way (a trailing `/`, a symlinked parent, on macOS another case) keeps the
/// live copy: retention never trashes it, and it still applies there.
#[test]
fn save_as_onto_the_same_package_spelled_otherwise_keeps_the_copy() {
    let _g = serial();
    let p = build(BitDepthRow::Int24, true);
    let row = p.s.open_external(p.second).unwrap();
    let copy = PathBuf::from(&row.path);
    let link = p.dir.path().join("link");
    #[cfg(unix)]
    std::os::unix::fs::symlink(p.dir.path(), &link).unwrap();
    let mut spellings = vec![
        format!("{}/", p.pkg().display()),
        link.join("P.vizdaw").display().to_string(),
    ];
    if cfg!(target_os = "macos") {
        // APFS is case-insensitive by default.
        spellings.push(p.dir.path().join("p.vizdaw").display().to_string());
    }
    for spelling in spellings {
        p.s.save_as(spelling.clone()).unwrap();
        p.s.save().unwrap();
        assert!(copy.is_file(), "{spelling}: the live copy is kept");
        assert!(
            !p.trash().join("tone edit 1.wav").exists(),
            "{spelling}: never trashed"
        );
        match p.s.external_edit(p.second).unwrap() {
            ExternalEditStateRow::Copy { row: r } => {
                assert!(Path::new(&r.path).is_file(), "{spelling}: {r:?}")
            }
            other => panic!("{spelling}: {other:?}"),
        }
    }
    p.s.close().unwrap();
    assert!(
        p.trash().join("tone edit 1.wav").is_file(),
        "closing lets it go"
    );
}

/// Final review I1: after Save As onto the same package spelled another
/// way, the app still watches the copy under the spelling Open returned;
/// an apply with that spelling is taken (it names the same file), while
/// another file, and a `..` spelling, are still refused.
#[test]
fn the_apps_spelling_of_the_copy_applies_after_save_as_spelled_otherwise() {
    let _g = serial();
    let p = build(BitDepthRow::Int24, true);
    // What the app holds and sends with every apply.
    let held = p.s.open_external(p.second).unwrap().path;
    let link = p.dir.path().join("link");
    #[cfg(unix)]
    std::os::unix::fs::symlink(p.dir.path(), &link).unwrap();
    let mut spellings = vec![
        format!("{}/", p.pkg().display()),
        link.join("P.vizdaw").display().to_string(),
    ];
    if cfg!(target_os = "macos") {
        // APFS is case-insensitive by default.
        spellings.push(p.dir.path().join("p.vizdaw").display().to_string());
    }
    for spelling in spellings {
        p.s.save_as(spelling.clone()).unwrap();
        let job =
            p.s.apply_external_edit(p.second, held.clone())
                .unwrap_or_else(|e| panic!("{spelling}: {e}"));
        let j = wait_job(&p.s, job);
        // macOS only: importing a WAV goes through ExtAudioFile (the CAF
        // importer reads CAF only); elsewhere the job ends Failed.
        #[cfg(target_os = "macos")]
        assert_eq!(j.state, JobStateRow::Done, "{spelling}: {j:?}");
        let _ = j;
    }
    let other = Path::new(&held).with_file_name("other.wav");
    std::fs::copy(&held, &other).unwrap();
    let dotted = format!(
        "{}/../Edits/tone edit 1.wav",
        Path::new(&held).parent().unwrap().display()
    );
    for bad in [other.display().to_string(), dotted] {
        assert!(
            matches!(
                p.s.apply_external_edit(p.second, bad.clone()),
                Err(VizError::Model { .. })
            ),
            "{bad}"
        );
    }
    p.s.close().unwrap();
}

/// Owner answer 2026-11-03 (pre-flight I2): an untitled project's copies
/// go to the Trash at close and at Save As, never unlinked; after Save As
/// the copy is gone.
#[test]
fn an_untitled_projects_copies_go_to_the_trash() {
    let _g = serial();
    let p = build(BitDepthRow::Int24, false);
    let copy = PathBuf::from(p.s.open_external(p.second).unwrap().path);
    // <scratch package>/Media/Edits/<copy>
    let scratch = copy.ancestors().nth(3).unwrap().to_path_buf();
    assert!(copy.is_file() && scratch.extension().is_some_and(|e| e == "vizdaw"));
    p.s.close().unwrap();
    assert!(!scratch.exists(), "the scratch package is removed");
    assert!(
        p.trash().join("tone edit 1.wav").is_file(),
        "its copy went to the Trash first"
    );
    let q = build(BitDepthRow::Int24, false);
    let copy = PathBuf::from(q.s.open_external(q.second).unwrap().path);
    let pkg = q.dir.path().join("Named.vizdaw");
    q.s.save_as(pkg.display().to_string()).unwrap();
    assert!(!copy.exists() && q.trash().join("tone edit 1.wav").is_file());
    assert!(
        !pkg.join("Media/Edits").exists(),
        "Save As does not carry it"
    );
    assert_eq!(
        q.s.external_edit(q.second).unwrap(),
        ExternalEditStateRow::Gone
    );
    assert_eq!(
        q.s.apply_external_edit(q.second, copy.display().to_string())
            .unwrap_err()
            .to_string(),
        GONE
    );
    q.s.close().unwrap();
}

/// Ruling 7 (pre-flight I10): what Reload finds.
#[test]
fn reload_finds_the_copy_or_says_it_is_gone() {
    let _g = serial();
    let p = build(BitDepthRow::Int24, true);
    assert_eq!(
        p.s.external_edit(p.second).unwrap(),
        ExternalEditStateRow::NoEdit
    );
    let row = p.s.open_external(p.second).unwrap();
    assert_eq!(
        p.s.external_edit(p.second).unwrap(),
        ExternalEditStateRow::Copy { row: row.clone() }
    );
    assert_eq!(
        p.s.external_edit(p.first).unwrap(),
        ExternalEditStateRow::NoEdit,
        "the other part has none"
    );
    std::fs::remove_file(&row.path).unwrap();
    assert_eq!(
        p.s.external_edit(p.second).unwrap(),
        ExternalEditStateRow::Gone
    );
    p.s.close().unwrap();
    // A reopened project whose region plays an external edit's media: no
    // copy in this session.
    let dir = tempfile::tempdir().unwrap();
    let pkg = dir.path().join("E.vizdaw");
    let mut b = Builder::new();
    let (seq, lane) = b.sequence("Song", "Audio", None).unwrap();
    let a = b.media("Media/a.caf", 4_800, 2).unwrap();
    let r = b
        .run(Command::Media(MediaCmd::Add {
            path: "Media/a edit 1.caf".into(),
            sample_rate: 48_000,
            channels: 2,
            length_frames: 4_800,
            content_hash: None,
            name: Some("a edit 1".into()),
            origin: Some(MediaOrigin::ExternalEdit { from: a }),
        }))
        .unwrap();
    let edited = r
        .created
        .iter()
        .find_map(|id| match id {
            AnyId::Media(m) => Some(*m),
            _ => None,
        })
        .unwrap();
    let on_edit = b
        .region(
            seq,
            lane,
            0,
            edited,
            0,
            4_800,
            0.0,
            Fade::default(),
            Fade::default(),
        )
        .unwrap();
    let on_a = b
        .region(
            seq,
            lane,
            96_000,
            a,
            0,
            4_800,
            0.0,
            Fade::default(),
            Fade::default(),
        )
        .unwrap();
    core_model::file::save(&b.finish(), &pkg).unwrap();
    let s = Session::open(pkg.display().to_string()).unwrap();
    s.debug_set_trash_dir(Some(dir.path().join("Trash").display().to_string()));
    assert_eq!(
        s.external_edit(on_edit.0).unwrap(),
        ExternalEditStateRow::Gone
    );
    assert_eq!(
        s.external_edit(on_a.0).unwrap(),
        ExternalEditStateRow::NoEdit
    );
    assert!(matches!(
        s.external_edit(999_999),
        Err(VizError::Model { .. })
    ));
    s.close().unwrap();
}

/// Review Focus 2: a mono, shorter edit at another rate relinks that
/// region alone as one step "External Edit"; its fades are clamped.
#[cfg(target_os = "macos")] // macOS only: importing a WAV goes through ExtAudioFile (the CAF importer reads CAF only)
#[test]
fn a_shorter_mono_edit_relinks_the_region() {
    let _g = serial();
    let p = build(BitDepthRow::Int24, true);
    // Long fades, so the shorter edit must clamp them.
    p.s.edit(
        EditCommand::Audio {
            edit: AudioEdit::SetFades {
                event: p.second,
                fade_in: fade(20_000),
                fade_out: fade(20_000),
            },
        },
        None,
    )
    .unwrap();
    let r = p.region(p.second);
    let other = p.region(p.first);
    let copy = p.s.open_external(p.second).unwrap().path;
    // 30 % shorter, mono, at 44.1 kHz in this 48 kHz project.
    let src = ((r.frames as f64) * 0.7 * 44_100.0 / 48_000.0).round() as u32;
    replace_with_wav(Path::new(&copy), 1, 44_100, src);
    let job = p.s.apply_external_edit(p.second, copy).unwrap();
    let j = wait_job(&p.s, job);
    assert_eq!(j.state, JobStateRow::Done, "{j:?}");
    assert_eq!(
        p.s.snapshot().unwrap().undo_label.as_deref(),
        Some("External Edit")
    );
    let new = p.region(p.second);
    let m = media_row(&p.s, new.media.unwrap());
    assert_eq!(j.created, vec![m.id]);
    assert_eq!(
        (m.origin, m.channels, m.sample_rate, m.name.as_str()),
        (MediaOriginRow::ExternalEdit, 1, 48_000, "tone edit 1")
    );
    let want = (f64::from(src) * 48_000.0 / 44_100.0).round() as u64;
    assert!(m.frames.abs_diff(want) <= 1, "{} vs {want}", m.frames);
    assert_eq!(
        (new.source_offset, new.frames, new.tick, new.gain_db),
        (0, m.frames, r.tick, r.gain_db)
    );
    assert_eq!(
        (new.fade_in.length, new.fade_out.length),
        (20_000, m.frames as i64 - 20_000),
        "fade in first, then the fade out within what is left"
    );
    assert_eq!(p.region(p.first), other, "the other region is unaffected");
    assert_eq!(media_row(&p.s, p.media).origin, MediaOriginRow::Imported);
    assert!(p.pkg().join("Media/tone.caf").is_file(), "write-once");
    p.s.close().unwrap();
}

#[cfg(target_os = "macos")] // macOS only: importing a WAV goes through ExtAudioFile (the CAF importer reads CAF only)
#[test]
fn undo_restores_the_original_span() {
    let _g = serial();
    let p = build(BitDepthRow::Int24, true);
    let before = p.region(p.second);
    let copy = p.s.open_external(p.second).unwrap().path;
    replace_with_wav(Path::new(&copy), 2, 48_000, 12_000);
    let job = p.s.apply_external_edit(p.second, copy).unwrap();
    assert_eq!(wait_job(&p.s, job).state, JobStateRow::Done);
    let after = p.region(p.second);
    assert_eq!((after.source_offset, after.frames), (0, 12_000));
    p.s.undo().unwrap();
    assert_eq!(
        p.region(p.second),
        before,
        "the original media, offset and length"
    );
    assert_eq!(
        p.s.snapshot().unwrap().redo_label.as_deref(),
        Some("External Edit")
    );
    p.s.redo().unwrap();
    assert_eq!(p.region(p.second), after);
    p.s.close().unwrap();
}

/// One bad edit: what it is, how the editor writes it, the message.
#[cfg(target_os = "macos")]
type Case<'a> = (&'a str, &'a dyn Fn(&Path), Option<&'a str>);

/// Review Focus 2 (pre-flight I3): an empty, garbage or 3-channel edit is
/// a Failed job, the document unchanged, what it imported removed.
#[cfg(target_os = "macos")] // macOS only: importing a WAV goes through ExtAudioFile (the CAF importer reads CAF only)
#[test]
fn an_unreadable_or_wide_edit_changes_nothing() {
    let _g = serial();
    let p = build(BitDepthRow::Int24, true);
    let pkg = p.pkg();
    let copy = PathBuf::from(p.s.open_external(p.second).unwrap().path);
    let region = p.region(p.second);
    let undo = p.s.snapshot().unwrap().undo_label;
    let before = cafs(&pkg);
    let cases: [Case; 3] = [
        ("empty", &|c: &Path| std::fs::write(c, b"").unwrap(), None),
        (
            "garbage",
            &|c: &Path| std::fs::write(c, b"not audio at all, only bytes").unwrap(),
            None,
        ),
        (
            "wide",
            &|c: &Path| wav_at(c, 48_000, 3, 4_800),
            Some("The edit has 3 channels; regions hold one or two."),
        ),
    ];
    for (what, write, message) in cases {
        let _ = std::fs::remove_file(&copy);
        write(&copy);
        let job =
            p.s.apply_external_edit(p.second, copy.display().to_string())
                .unwrap();
        let j = wait_job(&p.s, job);
        assert_eq!(j.state, JobStateRow::Failed, "{what}: {j:?}");
        match message {
            Some(m) => assert_eq!(j.message.as_deref(), Some(m), "{what}"),
            None => assert!(j.message.is_some(), "{what}"),
        }
        assert_eq!(p.region(p.second), region, "{what}: unchanged");
        assert_eq!(p.s.snapshot().unwrap().undo_label, undo, "{what}");
        assert_eq!(cafs(&pkg), before, "{what}: what it imported is removed");
    }
    p.s.close().unwrap();
}

/// Review Focus 3 (pre-flight M10): an edit of a region deleted meanwhile
/// is dropped, what it imported removed, and the copy forgotten.
#[cfg(target_os = "macos")] // macOS only: importing a WAV goes through ExtAudioFile (the CAF importer reads CAF only)
#[test]
fn an_edit_of_a_deleted_region_is_dropped() {
    let _g = serial();
    let p = build(BitDepthRow::Int24, true);
    let pkg = p.pkg();
    let copy = p.s.open_external(p.second).unwrap().path;
    replace_with_wav(Path::new(&copy), 2, 48_000, 12_000);
    let before = cafs(&pkg);
    p.s.debug_hold_jobs(true).unwrap();
    let job = p.s.apply_external_edit(p.second, copy.clone()).unwrap();
    p.s.edit(
        EditCommand::Event {
            edit: EventEdit::Delete {
                events: vec![p.second],
            },
        },
        None,
    )
    .unwrap();
    p.s.debug_hold_jobs(false).unwrap();
    let j = wait_job(&p.s, job);
    assert_eq!(
        (j.state, j.message.as_deref()),
        (
            JobStateRow::Failed,
            Some("The region was deleted; the edit was not applied.")
        )
    );
    assert_eq!(cafs(&pkg), before, "the imported file is removed");
    assert!(p.s.external_edit(p.second).is_err(), "the region is gone");
    assert!(
        matches!(
            p.s.apply_external_edit(p.second, copy),
            Err(VizError::Model { .. })
        ),
        "its copy is forgotten"
    );
    p.s.close().unwrap();
}

/// Review Focus 3 (pre-flight M6): a region moved while the editor has it
/// is relinked where it is now; after a split the part keeping the ID
/// takes the whole edit (offset 0, its length) and the other keeps the
/// original.
#[cfg(target_os = "macos")] // macOS only: importing a WAV goes through ExtAudioFile (the CAF importer reads CAF only)
#[test]
fn a_split_or_moved_region_is_relinked_where_it_is() {
    let _g = serial();
    let p = build(BitDepthRow::Int24, true);
    let copy = p.s.open_external(p.second).unwrap().path;
    p.s.edit(
        EditCommand::Event {
            edit: EventEdit::Move {
                events: vec![p.second],
                delta: 61_440,
                to_lane: None,
            },
        },
        None,
    )
    .unwrap();
    let moved = p.region(p.second);
    p.s.edit(
        EditCommand::Audio {
            edit: AudioEdit::Split {
                event: p.second,
                at: moved.tick + moved.duration / 2,
            },
        },
        None,
    )
    .unwrap();
    let third = region_rows(&p.s, p.seq, p.lane)
        .into_iter()
        .find(|r| r.id != p.first && r.id != p.second)
        .unwrap();
    replace_with_wav(Path::new(&copy), 2, 48_000, 12_000);
    let job = p.s.apply_external_edit(p.second, copy).unwrap();
    assert_eq!(wait_job(&p.s, job).state, JobStateRow::Done);
    let r = p.region(p.second);
    assert_eq!(
        (r.tick, r.source_offset, r.frames),
        (moved.tick, 0, 12_000),
        "where it is now"
    );
    assert_ne!(r.media, Some(p.media));
    assert_eq!(
        p.region(third.id),
        third,
        "the split's other part keeps the original"
    );
    assert_eq!(p.region(p.first).media, Some(p.media));
    p.s.close().unwrap();
}

/// Final review M10: a second Open on an edited region names its copy after
/// the media the edits started from, so names do not grow on every round
/// trip ("tone edit 2", not "tone edit 1 edit 1").
#[cfg(target_os = "macos")] // macOS only: importing a WAV goes through ExtAudioFile (the CAF importer reads CAF only)
#[test]
fn a_round_trip_does_not_grow_the_name() {
    let _g = serial();
    let p = build(BitDepthRow::Int24, true);
    let mut names = Vec::new();
    for _ in 0..2 {
        let copy = p.s.open_external(p.second).unwrap().path;
        names.push(
            Path::new(&copy)
                .file_name()
                .unwrap()
                .to_string_lossy()
                .into_owned(),
        );
        replace_with_wav(Path::new(&copy), 2, 48_000, 12_000);
        let job = p.s.apply_external_edit(p.second, copy).unwrap();
        assert_eq!(wait_job(&p.s, job).state, JobStateRow::Done);
        names.push(media_row(&p.s, p.region(p.second).media.unwrap()).name);
    }
    assert_eq!(
        names,
        [
            "tone edit 1.wav",
            "tone edit 1",
            "tone edit 2.wav",
            "tone edit 2"
        ]
    );
    p.s.close().unwrap();
}

/// Final review M12: a region sharing edited media with a region whose
/// copy is live in this session (a split part with a new ID) has no edit
/// of its own: Reload says so, not that a copy is gone.
#[cfg(target_os = "macos")] // macOS only: importing a WAV goes through ExtAudioFile (the CAF importer reads CAF only)
#[test]
fn a_part_of_an_edited_region_has_no_edit_of_its_own() {
    let _g = serial();
    let p = build(BitDepthRow::Int24, true);
    let copy = p.s.open_external(p.second).unwrap().path;
    replace_with_wav(Path::new(&copy), 2, 48_000, 12_000);
    let job = p.s.apply_external_edit(p.second, copy).unwrap();
    assert_eq!(wait_job(&p.s, job).state, JobStateRow::Done);
    let edited = p.region(p.second);
    p.s.edit(
        EditCommand::Audio {
            edit: AudioEdit::Split {
                event: p.second,
                at: edited.tick + edited.duration / 2,
            },
        },
        None,
    )
    .unwrap();
    let part = region_rows(&p.s, p.seq, p.lane)
        .into_iter()
        .find(|r| r.id != p.first && r.id != p.second)
        .unwrap();
    assert_eq!(part.media, edited.media, "the part plays the edit");
    assert_eq!(
        p.s.external_edit(part.id).unwrap(),
        ExternalEditStateRow::NoEdit
    );
    assert!(matches!(
        p.s.external_edit(p.second).unwrap(),
        ExternalEditStateRow::Copy { .. }
    ));
    p.s.close().unwrap();
}
