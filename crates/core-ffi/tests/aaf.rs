//! AAF export and import from the app (plan 6f Task 5; spec §6, §7.5,
//! §11, §12; Rulings 2, 7, 8, 12). Projects are 6d's saved packages in
//! temporary folders at 48 kHz 24-bit with generated CAF media: track
//! "Mono" plays `Media/mono.caf` (one channel) and "Stereo" plays
//! `Media/stereo.caf` (two). One test at a time: the engine host and the
//! scratch folder are process-wide. Tests marked macOS convert WAVs through
//! the system importer, which Linux CI lacks.
// `im` maps nest deeply; sending a `Session` to a thread needs more depth.
#![recursion_limit = "256"]

mod common;

use common::dawproject::*;

use core_engine::fixtures::{Builder, Q};
use core_io::aaf::export as ax;
use core_model::command::{Command, LaneCmd};
#[cfg(target_os = "macos")]
use core_model::document::Document;
#[cfg(target_os = "macos")]
use core_model::event::EventBody;
use core_model::event::{Fade, FadeShape};
use core_model::ids::SeqId;
use core_model::lane::LaneInput;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};
use vizdaw_ffi::{
    AafDepthRow, AafExportRequestRow, AafFpsRow, AafImportRequestRow, AafStereoRow, BitDepthRow,
    DawprojectMergeRequestRow, JobKindRow, JobRow, JobStateRow, MergeStepRow, ProjectFormatRow,
    RecordSpecRow, ReportKindRow, ReportRow, Session, VizError,
};

// ---------------------------------------------------------------- helpers

/// An export of `seq` into the folder `parent`: embedded, 24-bit, mono
/// pairs, 25 fps from 00:00:00:00, with clip gain and automation.
fn areq(seq: u64, parent: &Path) -> AafExportRequestRow {
    AafExportRequestRow {
        seq,
        path: text(parent),
        embed: true,
        depth: AafDepthRow::Int24,
        stereo: AafStereoRow::MonoPairs,
        hour_start: false,
        fps: AafFpsRow::Fps25,
        clip_gain: true,
        automation: true,
    }
}

/// An import of `path` as a new project, pairs joined, nothing else given.
fn ireq(path: &Path) -> AafImportRequestRow {
    AafImportRequestRow {
        path: text(path),
        new_project: true,
        midi_path: None,
        media_folder: None,
        join_pairs: true,
    }
}

/// Exports `r` and waits for it to end Done; the folder it wrote.
fn export_aaf(s: &Session, r: AafExportRequestRow) -> (JobRow, PathBuf) {
    let j = wait_job(s, s.export_aaf(r).unwrap());
    assert_eq!(j.kind, JobKindRow::ExportAaf);
    assert_eq!(j.state, JobStateRow::Done, "{j:?}");
    assert_eq!(j.written.len(), 1, "{j:?}");
    let folder = PathBuf::from(&j.written[0]);
    (j, folder)
}

/// Starts the import `r` and waits for its job to end.
fn import_aaf(s: &Session, r: AafImportRequestRow) -> JobRow {
    let j = wait_job(s, s.import_aaf(r).unwrap());
    assert_eq!(j.kind, JobKindRow::ImportAaf);
    j
}

/// 6d's project with a "Keys" track on the built-in synth playing two
/// notes, and a second region on "Mono" at bar 2 with a 480-frame fade-in
/// and a 960-frame equal-power fade-out.
fn keys_project() -> Project {
    project_with(|b, seq, m1, _| {
        let synth = b.dest("Synth", "builtin:synth", 0).unwrap();
        let keys = b.lane(seq, "Keys", Some(synth)).unwrap();
        b.notes(seq, keys, &[(0, 60, 0x8000, Q), (Q, 64, 0x8000, Q)])
            .unwrap();
        let mono = b.doc().sequence(seq).unwrap().lanes[0].id;
        b.region(
            seq,
            mono,
            4 * Q,
            m1,
            100,
            FRAMES - 200,
            0.0,
            Fade {
                length_frames: 480,
                shape: FadeShape::Linear,
            },
            Fade {
                length_frames: 960,
                shape: FadeShape::EqualPower,
            },
        )
        .unwrap();
    })
}

/// A new project to import into: 48 kHz 24-bit, no tracks, saved as
/// `I.vizdaw` in its own folder, with a private Trash.
struct Target {
    _dir: tempfile::TempDir,
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
    Target { _dir: dir, pkg, s }
}

#[cfg(target_os = "macos")]
fn doc_of(s: &Session) -> Document {
    core_model::file::from_json(&s.debug_document_json()).unwrap()
}

fn first_seq(s: &Session) -> SeqId {
    SeqId(s.snapshot().unwrap().sequences[0].id)
}

/// The files (not folders) under `<root>/Media`, hidden ones included.
fn media_files(root: &Path) -> Vec<String> {
    listing(&root.join("Media"))
        .into_iter()
        .filter(|f| !f.ends_with('/'))
        .collect()
}

/// The CAF files directly in `<root>/Media`.
fn media_cafs(root: &Path) -> Vec<String> {
    media_files(root)
        .into_iter()
        .filter(|f| !f.contains('/') && f.ends_with(".caf"))
        .collect()
}

fn items(r: &ReportRow, kind: ReportKindRow, what: &str) -> u64 {
    r.items
        .iter()
        .filter(|i| i.kind == kind && i.what == what)
        .map(|i| i.count)
        .sum()
}

fn busy<T: std::fmt::Debug>(r: Result<T, VizError>, what: &str) {
    assert!(matches!(r, Err(VizError::Busy { .. })), "{what}: {r:?}");
}

/// Every file under `dir` with its bytes.
fn contents(dir: &Path) -> Vec<(String, Vec<u8>)> {
    listing(dir)
        .into_iter()
        .filter(|f| !f.ends_with('/'))
        .map(|f| {
            let b = std::fs::read(dir.join(&f)).unwrap();
            (f, b)
        })
        .collect()
}

/// One audio region as the round trip compares it.
#[cfg(target_os = "macos")]
#[derive(Debug, Clone, PartialEq)]
struct Reg {
    tick: i64,
    offset: u64,
    length: u64,
    fade_in: (u64, FadeShape),
    fade_out: (u64, FadeShape),
    crossfade: bool,
}

/// Per track name: its audio regions in tick order, and its media's
/// channels.
#[cfg(target_os = "macos")]
fn regions(doc: &Document, seq: SeqId) -> Vec<(String, Vec<Reg>, Vec<u16>)> {
    let s = doc.sequence(seq).unwrap();
    s.lanes
        .iter()
        .map(|l| {
            let mut v = Vec::new();
            let mut ch = Vec::new();
            for e in l.active().into_iter().flat_map(|t| t.events.values()) {
                if let EventBody::AudioRegion {
                    media,
                    source_offset,
                    length,
                    fade_in,
                    fade_out,
                    crossfade,
                    ..
                } = &e.body
                {
                    v.push(Reg {
                        tick: e.tick.0,
                        offset: *source_offset,
                        length: *length,
                        fade_in: (fade_in.length_frames, fade_in.shape),
                        fade_out: (fade_out.length_frames, fade_out.shape),
                        crossfade: *crossfade,
                    });
                    ch.push(doc.media().get(media).unwrap().channels);
                }
            }
            v.sort_by_key(|r| r.tick);
            (l.name.clone(), v, ch)
        })
        .collect()
}

// ---------------------------------------------------------------- export

/// Ruling 2: "<Song> AAF" holds `<Song>.aaf` and `<Song> MIDI.mid`; the
/// audio is inside the `.aaf` (no `Audio/`), or with `embed: false` in
/// `Audio/NNN <name>.wav`. Nothing is left staged.
#[test]
fn export_writes_the_folder() {
    let _g = serial();
    let p = keys_project();
    let (j, folder) = export_aaf(&p.s, areq(p.seq, &p.out));
    assert_eq!(folder, p.out.join("Song AAF"));
    assert!(j.report.is_some());
    assert_eq!(
        listing(&p.out),
        ["Song AAF/", "Song AAF/Song MIDI.mid", "Song AAF/Song.aaf"]
    );
    let midi = std::fs::read(folder.join("Song MIDI.mid")).unwrap();
    assert!(midi.starts_with(b"MThd"));
    let aaf = std::fs::read(folder.join("Song.aaf")).unwrap();
    assert!(
        aaf.starts_with(&[0xd0, 0xcf, 0x11, 0xe0]),
        "a compound file"
    );
    std::fs::remove_dir_all(&folder).unwrap();

    let (_, folder) = export_aaf(
        &p.s,
        AafExportRequestRow {
            embed: false,
            ..areq(p.seq, &p.out)
        },
    );
    let all = listing(&p.out);
    let wavs: Vec<&String> = all
        .iter()
        .filter(|f| f.starts_with("Song AAF/Audio/") && f.ends_with(".wav"))
        .collect();
    // Mono, and Stereo's two halves.
    assert_eq!(wavs.len(), 3, "{all:?}");
    assert!(wavs[0].starts_with("Song AAF/Audio/001 "), "{all:?}");
    assert!(all.contains(&"Song AAF/Song.aaf".to_owned()));
    assert!(all.iter().all(|f| !f.contains(".partial")), "{all:?}");
    assert!(all.iter().all(|f| !f.contains(".essence")), "{all:?}");
    // The halves are mono, 24-bit, at the media rate.
    let b = std::fs::read(folder.join(wavs[1].trim_start_matches("Song AAF/"))).unwrap();
    assert_eq!(u16::from_le_bytes([b[22], b[23]]), 1, "mono");
    assert_eq!(u16::from_le_bytes([b[34], b[35]]), 24, "24-bit");
}

/// Ruling 2: a second export is numbered; the first folder stays as it
/// was, byte for byte.
#[test]
fn existing_folder_is_numbered() {
    let _g = serial();
    let p = project();
    let (_, first) = export_aaf(&p.s, areq(p.seq, &p.out));
    let before = contents(&first);
    let (_, second) = export_aaf(&p.s, areq(p.seq, &p.out));
    assert_eq!(second, p.out.join("Song AAF 2"));
    assert_eq!(contents(&first), before);
    assert_eq!(p.s.aaf_folder_name(p.seq).unwrap(), "Song AAF");
    assert!(matches!(
        p.s.aaf_folder_name(999),
        Err(VizError::Model { .. })
    ));
}

/// The dry run gives Task 4's lines and writes nothing.
#[test]
fn check_writes_nothing() {
    let _g = serial();
    let p = keys_project();
    let r = p.s.export_aaf_check(areq(p.seq, Path::new(""))).unwrap();
    assert!(r.summary.starts_with("Exported"), "{}", r.summary);
    assert_eq!(items(&r, ReportKindRow::Note, ax::MIDI_BESIDE), 1, "{r:?}");
    assert_eq!(
        items(&r, ReportKindRow::Note, ax::BUILTIN_SYNTH),
        1,
        "{r:?}"
    );
    assert!(items(&r, ReportKindRow::LeftOut, ax::INSERTS) >= 1, "{r:?}");
    assert_eq!(listing(&p.out), Vec::<String>::new());
    // The check names no folder; an unknown sequence is refused.
    let r = p.s.export_aaf_check(areq(999, Path::new("")));
    assert!(matches!(r, Err(VizError::Model { .. })), "{r:?}");
    assert_eq!(listing(&p.out), Vec::<String>::new());
}

/// Spec §11: running out of space fails the export and the import, and
/// leaves nothing.
#[test]
fn not_enough_space_refuses() {
    let _g = serial();
    let p = project();
    p.s.debug_free_bytes(Some(1)).unwrap();
    let j = wait_job(&p.s, p.s.export_aaf(areq(p.seq, &p.out)).unwrap());
    assert_eq!(j.state, JobStateRow::Failed, "{j:?}");
    let m = j.message.unwrap();
    assert!(
        m.starts_with("Not enough free space to export (it needs ") && m.ends_with(" MB)."),
        "{m}"
    );
    assert_eq!(listing(&p.out), Vec::<String>::new());
    p.s.debug_free_bytes(None).unwrap();
    let (_, folder) = export_aaf(&p.s, areq(p.seq, &p.out));

    let t = target();
    let before = media_files(&t.pkg);
    t.s.debug_free_bytes(Some(1)).unwrap();
    let j = import_aaf(&t.s, ireq(&folder.join("Song.aaf")));
    assert_eq!(j.state, JobStateRow::Failed, "{j:?}");
    let m = j.message.unwrap();
    assert!(
        m.starts_with("Not enough free space to import this file (it needs "),
        "{m}"
    );
    assert_eq!(media_files(&t.pkg), before);
    assert!(t.s.lanes(first_seq(&t.s).0).unwrap().is_empty());
    t.s.debug_free_bytes(None).unwrap();
}

/// Lessons: a cancel before the run and after each export stage (1 staged,
/// 2 MIDI written, 3 `.aaf` written) leaves the parent folder as it was;
/// a cancel after the import's open (5) and first extraction (6) leaves
/// `Media/` and the document as they were.
#[test]
fn cancel_at_every_stage_leaves_nothing() {
    let _g = serial();
    let p = keys_project();
    p.s.debug_hold_jobs(true).unwrap();
    let id = p.s.export_aaf(areq(p.seq, &p.out)).unwrap();
    p.s.cancel_job(id).unwrap();
    p.s.debug_hold_jobs(false).unwrap();
    assert_eq!(wait_job(&p.s, id).state, JobStateRow::Cancelled);
    assert_eq!(listing(&p.out), Vec::<String>::new());
    for stage in 1..=3 {
        p.s.debug_aaf_cancel_at(Some(stage)).unwrap();
        let j = wait_job(&p.s, p.s.export_aaf(areq(p.seq, &p.out)).unwrap());
        assert_eq!(j.state, JobStateRow::Cancelled, "stage {stage}: {j:?}");
        assert_eq!(listing(&p.out), Vec::<String>::new(), "stage {stage}");
    }
    // The hook is used once.
    let (_, folder) = export_aaf(&p.s, areq(p.seq, &p.out));
    let aaf = folder.join("Song.aaf");

    let t = target();
    let before = media_files(&t.pkg);
    let doc = t.s.debug_document_json();
    t.s.debug_hold_jobs(true).unwrap();
    let id = t.s.import_aaf(ireq(&aaf)).unwrap();
    t.s.cancel_job(id).unwrap();
    t.s.debug_hold_jobs(false).unwrap();
    assert_eq!(wait_job(&t.s, id).state, JobStateRow::Cancelled);
    assert_eq!(media_files(&t.pkg), before);
    for stage in 5..=6 {
        t.s.debug_aaf_cancel_at(Some(stage)).unwrap();
        let j = import_aaf(&t.s, ireq(&aaf));
        assert_eq!(j.state, JobStateRow::Cancelled, "stage {stage}: {j:?}");
        assert_eq!(media_files(&t.pkg), before, "stage {stage}");
        assert_eq!(t.s.debug_document_json(), doc, "stage {stage}");
    }
}

/// L1: one interchange job at a time. Recording, an open edit, a running
/// DAWproject export, a Convert job and a held merge each make both AAF
/// calls `Busy`; a running AAF job makes `export_dawproject` `Busy`.
#[test]
fn busy_states_refuse() {
    let _g = serial();
    let p = project();
    let (_, folder) = export_aaf(&p.s, areq(p.seq, &p.out));
    let aaf = folder.join("Song.aaf");
    let into = AafImportRequestRow {
        new_project: false,
        ..ireq(&aaf)
    };
    let both = |s: &Session, what: &str| {
        busy(
            s.export_aaf(areq(p.seq, &p.out)),
            &format!("export, {what}"),
        );
        busy(s.import_aaf(into.clone()), &format!("import, {what}"));
    };
    // A running DAWproject export.
    p.s.debug_hold_jobs(true).unwrap();
    let e =
        p.s.export_dawproject(req(p.seq, &p.out.join("x.dawproject")))
            .unwrap();
    both(&p.s, "a DAWproject export");
    p.s.cancel_job(e).unwrap();
    p.s.debug_hold_jobs(false).unwrap();
    wait_job(&p.s, e);
    // A Convert job.
    p.s.debug_hold_jobs(true).unwrap();
    let c = p.s.convert_media().unwrap();
    both(&p.s, "a Convert job");
    p.s.cancel_job(c).unwrap();
    p.s.debug_hold_jobs(false).unwrap();
    wait_job(&p.s, c);
    // Running AAF jobs hold DAWproject exports and each other.
    for import in [false, true] {
        p.s.debug_hold_jobs(true).unwrap();
        let a = if import {
            p.s.import_aaf(into.clone()).unwrap()
        } else {
            p.s.export_aaf(areq(p.seq, &p.out)).unwrap()
        };
        busy(
            p.s.export_dawproject(req(p.seq, &p.out.join("y.dawproject"))),
            "a DAWproject export under an AAF job",
        );
        both(&p.s, "another AAF job");
        p.s.cancel_job(a).unwrap();
        p.s.debug_hold_jobs(false).unwrap();
        assert_eq!(wait_job(&p.s, a).state, JobStateRow::Cancelled);
    }
    // An open edit.
    p.s.begin_edit("Drag".into()).unwrap();
    both(&p.s, "an open edit");
    p.s.end_edit().unwrap();
    // A held merge.
    let dp = p.out.join("m.dawproject");
    export(&p.s, req(p.seq, &dp));
    let m = wait_job(
        &p.s,
        p.s.merge_dawproject(DawprojectMergeRequestRow {
            path: text(&dp),
            snapshot: None,
            step: MergeStepRow::Hold,
            date: "2026-11-13".into(),
        })
        .unwrap(),
    );
    assert_eq!(m.state, JobStateRow::Done, "{m:?}");
    both(&p.s, "a held merge");
    p.s.cancel_merge(m.id).unwrap();

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
    busy(r.export_aaf(areq(rseq.0, &p.out)), "export, recording");
    busy(r.import_aaf(into.clone()), "import, recording");
    r.stop().unwrap();
    r.close().unwrap();
}

/// Hostile requests: each call gives `Model` or `Io` at once, or a job that
/// ends Failed with a sentence, never `Internal` or "internal error:". Text
/// faults are refused at once; faults found on disk fail the job (the main
/// thread does no I/O). `aaf_info` refuses every bad path at once.
#[test]
fn hostile_requests() {
    let _g = serial();
    let p = project();
    let (_, folder) = export_aaf(&p.s, areq(p.seq, &p.out));
    let good = folder.join("Song.aaf");
    let bad = p.dir.path().join("bad");
    std::fs::create_dir(&bad).unwrap();
    let fifo = bad.join("fifo.aaf");
    assert!(
        std::process::Command::new("mkfifo")
            .arg(&fifo)
            .status()
            .unwrap()
            .success()
    );
    let lp = bad.join("loop.aaf");
    std::os::unix::fs::symlink(&lp, &lp).unwrap();
    let dir = bad.join("dir.aaf");
    std::fs::create_dir(&dir).unwrap();
    let zero = bad.join("zero.aaf");
    std::os::unix::fs::symlink("/dev/zero", &zero).unwrap();
    let file = bad.join("plain.txt");
    std::fs::write(&file, b"x").unwrap();
    let long = format!("/{}.aaf", "a/".repeat(2100));
    // A file name as the FFI's UTF-8 strings carry a non-UTF-8 one: its
    // lossy form.
    let lossy = text(&bad.join("caf\u{FFFD}.aaf"));

    /// What a case gives.
    #[derive(Debug, Clone, Copy, PartialEq)]
    enum Gives {
        /// Refused at once (`Model` or `Io`).
        AtOnce,
        /// A job that ends Failed with a sentence.
        Job,
    }
    use Gives::*;
    let fine = |e: &VizError| matches!(e, VizError::Model { .. } | VizError::Io { .. });
    let failed = |s: &Session, id: u64, what: &str| {
        let j = wait_job(s, id);
        assert_eq!(j.state, JobStateRow::Failed, "{what}: {j:?}");
        let m = j.message.unwrap_or_default();
        assert!(
            !m.is_empty() && !m.starts_with("internal error"),
            "{what}: {m}"
        );
    };
    let check = |what: &str, want: Gives, r: Result<u64, VizError>, s: &Session| match (want, r) {
        (AtOnce, Err(e)) => assert!(fine(&e), "{what}: {e:?}"),
        (Job, Ok(id)) => failed(s, id, what),
        (w, r) => panic!("{what}: wanted {w:?}, got {r:?}"),
    };

    // The AAF path, for `import_aaf` and `aaf_info`.
    let t = target();
    let aaf_cases: Vec<(String, &str, Gives)> = vec![
        (String::new(), "empty", AtOnce),
        ("relative.aaf".into(), "relative", AtOnce),
        (text(&dir), "a directory", Job),
        (text(&fifo), "a FIFO", Job),
        ("/dev/zero".into(), "/dev/zero", AtOnce),
        (text(&zero), "a link to /dev/zero", Job),
        (text(&lp), "a symlink loop", Job),
        (long.clone(), "over 4 KiB", AtOnce),
        (format!("{}/a\0b.aaf", text(&bad)), "a NUL", AtOnce),
        (lossy.clone(), "a non-UTF-8 name, lossy", Job),
        (text(&file), "not ending .aaf", AtOnce),
    ];
    for (path, what, want) in &aaf_cases {
        match vizdaw_ffi::aaf_info(path.clone()) {
            Err(e) => assert!(fine(&e), "info, {what}: {e:?}"),
            Ok(i) => panic!("info, {what}: {i:?}"),
        }
        let r = t.s.import_aaf(AafImportRequestRow {
            path: path.clone(),
            ..ireq(&good)
        });
        check(&format!("import, {what}"), *want, r, &t.s);
    }
    // `/dev/zero` has no `.aaf`: the name is refused before the device.
    match t.s.import_aaf(AafImportRequestRow {
        path: text(&file),
        ..ireq(&good)
    }) {
        Err(VizError::Model { message }) => {
            assert_eq!(message, vizdaw_ffi::aaf::NOT_AAF_NAME)
        }
        other => panic!("{other:?}"),
    }
    // `midi_path` and `media_folder`: relative refused at once; missing,
    // or a file given as the folder, fail the job.
    let side: Vec<(AafImportRequestRow, &str, Gives)> = vec![
        (
            AafImportRequestRow {
                midi_path: Some("x.mid".into()),
                ..ireq(&good)
            },
            "a relative MIDI path",
            AtOnce,
        ),
        (
            AafImportRequestRow {
                midi_path: Some(text(&bad.join("missing.mid"))),
                ..ireq(&good)
            },
            "a missing MIDI file",
            Job,
        ),
        (
            AafImportRequestRow {
                media_folder: Some("media".into()),
                ..ireq(&good)
            },
            "a relative media folder",
            AtOnce,
        ),
        (
            AafImportRequestRow {
                media_folder: Some(text(&bad.join("missing"))),
                ..ireq(&good)
            },
            "a missing media folder",
            Job,
        ),
        (
            AafImportRequestRow {
                media_folder: Some(text(&file)),
                ..ireq(&good)
            },
            "a media folder that is a file",
            Job,
        ),
    ];
    for (r, what, want) in side {
        check(what, want, t.s.import_aaf(r), &t.s);
    }
    assert!(media_cafs(&t.pkg).is_empty());
    assert!(t.s.lanes(first_seq(&t.s).0).unwrap().is_empty());

    // The export's `path` (the parent folder).
    let before = listing(&p.out);
    let export_cases: Vec<(String, &str, Gives)> = vec![
        (String::new(), "empty", AtOnce),
        ("relative".into(), "relative", AtOnce),
        (text(&file), "a file, not a folder", Job),
        (text(&fifo), "a FIFO", Job),
        ("/dev/zero".into(), "/dev/zero", Job),
        (text(&lp), "a symlink loop", Job),
        (long, "over 4 KiB", AtOnce),
        (format!("{}/a\0b", text(&bad)), "a NUL", AtOnce),
        (format!("{}/../x", text(&bad)), "a .. component", AtOnce),
        (lossy, "a non-UTF-8 name, lossy", Job),
        (text(&p.pkg.join("Media")), "inside the package", Job),
    ];
    for (path, what, want) in &export_cases {
        let r = p.s.export_aaf(AafExportRequestRow {
            path: path.clone(),
            ..areq(p.seq, &p.out)
        });
        check(&format!("export, {what}"), *want, r, &p.s);
    }
    let r = p.s.export_aaf(areq(999, &p.out));
    assert!(matches!(r, Err(VizError::Model { .. })), "{r:?}");
    assert_eq!(listing(&p.out), before, "nothing written");
    assert!(
        listing(&p.pkg).iter().all(|f| !f.contains("AAF")),
        "nothing in the package"
    );
}

/// Plan 6f Task 7: 32 byte-damaged copies of a default export (flips,
/// inserts, deletes and truncations from a fixed seed) each end Done or
/// Failed through `import_aaf`, never with `internal error:`, and a Failed
/// one leaves `Media/` as it was. Linux runs it: there the unconverted
/// WAVs go offline, and Done is allowed.
#[test]
fn hostile_files_fail_cleanly() {
    let _g = serial();
    let p = project();
    let (_, folder) = export_aaf(&p.s, areq(p.seq, &p.out));
    let clean = std::fs::read(folder.join("Song.aaf")).unwrap();
    // xorshift64, from a fixed seed.
    let mut x: u64 = 0x9E37_79B9_7F4A_7C15;
    let mut next = move || {
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        x
    };
    let dir = tempfile::tempdir().unwrap();
    let (mut done, mut failed) = (0, 0);
    for k in 0..32 {
        let mut b = clean.clone();
        for _ in 0..1 + next() % 4 {
            if b.is_empty() {
                break;
            }
            let at = (next() % b.len() as u64) as usize;
            match next() % 4 {
                0 => b[at] ^= (next() % 255 + 1) as u8,
                1 => b.insert(at, next() as u8),
                2 => {
                    b.remove(at);
                }
                _ => b.truncate(at),
            }
        }
        let path = dir.path().join(format!("Hostile {k}.aaf"));
        std::fs::write(&path, &b).unwrap();
        idle_until(&p.s, |s| s.debug_overviews_idle());
        let before = media_files(&p.pkg);
        let j = import_aaf(
            &p.s,
            AafImportRequestRow {
                new_project: false,
                ..ireq(&path)
            },
        );
        match j.state {
            JobStateRow::Done => done += 1,
            JobStateRow::Failed => {
                failed += 1;
                let m = j.message.clone().unwrap_or_default();
                assert!(!m.starts_with("internal error:"), "copy {k}: {m}");
                idle_until(&p.s, |s| s.debug_overviews_idle());
                assert_eq!(media_files(&p.pkg), before, "copy {k}: {m}");
            }
            _ => panic!("copy {k}: {j:?}"),
        }
    }
    assert_eq!(done + failed, 32);
}

// ---------------------------------------------------------------- import (macOS: conversion)

/// Spec §12: an export read back by the import job into a fresh project at
/// `aaf_info`'s rate has the same tracks, region ticks and fades; the info
/// names VizDAW and the MIDI file beside, and the report holds Ruling 12's
/// Note.
#[cfg(target_os = "macos")]
#[test]
fn round_trip_through_the_jobs() {
    use core_io::aaf::timeline::FROM_VIZDAW;
    let _g = serial();
    let p = keys_project();
    let (_, folder) = export_aaf(&p.s, areq(p.seq, &p.out));
    let aaf = folder.join("Song.aaf");
    let info = vizdaw_ffi::aaf_info(text(&aaf)).unwrap();
    assert_eq!(info.sample_rate, 48_000);
    assert!(info.from_vizdaw);
    assert_eq!(
        info.midi_beside.as_deref(),
        Some(text(&folder.join("Song MIDI.mid")).as_str())
    );
    // Mono, Stereo L and Stereo R.
    assert_eq!(info.tracks, 3);
    assert_eq!(info.offline, 0);

    let t = target();
    let j = import_aaf(&t.s, ireq(&aaf));
    assert_eq!(j.state, JobStateRow::Done, "{j:?}");
    let report = j.report.clone().expect("a report");
    assert_eq!(
        items(&report, ReportKindRow::Note, FROM_VIZDAW),
        1,
        "{report:?}"
    );
    assert_eq!(t.s.interchange_report().unwrap(), Some(report));
    assert_eq!(t.s.snapshot().unwrap().undo_label, None, "nothing to undo");

    let a = regions(&doc_of(&p.s), SeqId(p.seq));
    let i = regions(&doc_of(&t.s), first_seq(&t.s));
    assert_eq!(i.len(), a.len(), "{i:?}");
    for (name, ra, ca) in &a {
        let (_, ri, ci) = i
            .iter()
            .find(|(n, ..)| n == name)
            .unwrap_or_else(|| panic!("no {name} in {i:?}"));
        assert_eq!(ri, ra, "{name}");
        assert_eq!(ci, ca, "{name}: channels");
    }
    // Every item is online.
    assert!(t.s.media().unwrap().iter().all(|m| m.offline.is_none()));
}

/// Spec §6.1: added to this project, the import is one undo step "Import
/// AAF"; one undo gives the old document and media pool.
#[cfg(target_os = "macos")]
#[test]
fn into_this_project_undoes_in_one_step() {
    let _g = serial();
    let p = project();
    let (_, folder) = export_aaf(&p.s, areq(p.seq, &p.out));
    let doc = p.s.debug_document_json();
    let pool = p.s.media().unwrap();
    let lanes = p.s.lanes(p.seq).unwrap().len();
    let j = import_aaf(
        &p.s,
        AafImportRequestRow {
            new_project: false,
            ..ireq(&folder.join("Song.aaf"))
        },
    );
    assert_eq!(j.state, JobStateRow::Done, "{j:?}");
    assert_eq!(p.s.lanes(p.seq).unwrap().len(), lanes * 2);
    assert_eq!(
        p.s.snapshot().unwrap().undo_label.as_deref(),
        Some("Import AAF")
    );
    p.s.undo().unwrap();
    assert_eq!(p.s.debug_document_json(), doc);
    assert_eq!(p.s.media().unwrap(), pool);
    // A new-project import into a project with tracks is refused.
    match p.s.import_aaf(ireq(&folder.join("Song.aaf"))) {
        Err(VizError::Model { message }) => {
            assert_eq!(message, core_io::aaf::import::NOT_NEW_AAF)
        }
        other => panic!("{other:?}"),
    }
}

/// Fix round 1 (review I1): an import into this project whose sequence's
/// tempo changes while the job runs is refused when it would be applied:
/// `Media/`, `Media/.incoming/` and the document stay as they were.
#[test]
fn a_tempo_change_mid_import_refuses_it() {
    use vizdaw_ffi::{EditCommand, SequenceEdit, TempoPointRow};
    let _g = serial();
    let p = project();
    let (_, folder) = export_aaf(&p.s, areq(p.seq, &p.out));
    idle_until(&p.s, |s| s.debug_overviews_idle());
    let media = media_files(&p.pkg);
    p.s.debug_hold_jobs(true).unwrap();
    let id =
        p.s.import_aaf(AafImportRequestRow {
            new_project: false,
            ..ireq(&folder.join("Song.aaf"))
        })
        .unwrap();
    p.s.edit(
        EditCommand::Sequence {
            edit: SequenceEdit::SetTempo {
                seq: p.seq,
                tempo: vec![TempoPointRow {
                    tick: 0,
                    bpm: 90.0,
                    ramp: false,
                }],
            },
        },
        None,
    )
    .unwrap();
    let doc = p.s.debug_document_json();
    p.s.debug_hold_jobs(false).unwrap();
    let j = wait_job(&p.s, id);
    assert_eq!(j.state, JobStateRow::Failed, "{j:?}");
    assert_eq!(j.message.as_deref(), Some(vizdaw_ffi::aaf::TEMPO_CHANGED));
    assert_eq!(p.s.debug_document_json(), doc);
    idle_until(&p.s, |s| s.debug_overviews_idle());
    assert_eq!(media_files(&p.pkg), media);
    assert!(
        listing(&p.pkg.join("Media/.incoming"))
            .iter()
            .all(|f| f.ends_with('/')),
        "nothing left in Media/.incoming"
    );
}

/// Review Focus 2: an external-audio export with one WAV moved elsewhere
/// imports that region offline (its item keeps a path to relink); with
/// `media_folder` naming where it went it is online. MXF media is offline.
#[cfg(target_os = "macos")]
#[test]
fn offline_and_found_media() {
    let _g = serial();
    let p = project();
    let (_, folder) = export_aaf(
        &p.s,
        AafExportRequestRow {
            embed: false,
            ..areq(p.seq, &p.out)
        },
    );
    let elsewhere = p.dir.path().join("elsewhere");
    std::fs::create_dir(&elsewhere).unwrap();
    let audio = folder.join("Audio");
    let first = listing(&audio).into_iter().next().unwrap();
    std::fs::rename(audio.join(&first), elsewhere.join(&first)).unwrap();
    let aaf = folder.join("Song.aaf");
    assert_eq!(vizdaw_ffi::aaf_info(text(&aaf)).unwrap().offline, 1);

    let t = target();
    let j = import_aaf(&t.s, ireq(&aaf));
    assert_eq!(j.state, JobStateRow::Done, "{j:?}");
    let media = t.s.media().unwrap();
    let off: Vec<_> = media.iter().filter(|m| m.offline.is_some()).collect();
    assert_eq!(off.len(), 1, "{media:?}");
    assert!(off[0].path.starts_with("Media/"), "{:?}", off[0]);
    assert!(off[0].uses >= 1, "its region is kept: {:?}", off[0]);

    let t = target();
    let j = import_aaf(
        &t.s,
        AafImportRequestRow {
            media_folder: Some(text(&elsewhere)),
            ..ireq(&aaf)
        },
    );
    assert_eq!(j.state, JobStateRow::Done, "{j:?}");
    let media = t.s.media().unwrap();
    assert!(media.iter().all(|m| m.offline.is_none()), "{media:?}");

    // MXF: a locator to an `.mxf` file.
    let doc = doc_of(&p.s);
    let mut x = ax::export(
        &doc,
        &options(SeqId(p.seq), false, ax::Stereo::Stereo),
        &env(),
        &std::sync::atomic::AtomicBool::new(false),
    )
    .unwrap();
    for o in &mut x.store.objects {
        if let Some(core_io::aaf::store::Prop::Data(u)) = o
            .props
            .get_mut(&core_io::aaf::dict::pid::NETWORK_LOCATOR_URL_STRING)
        {
            *u = utf16z("Audio/a.mxf");
        }
    }
    let mxf = p.dir.path().join("mxf.aaf");
    ax::write(
        &x,
        &mxf,
        &mut |_, _| Ok(()),
        &std::sync::atomic::AtomicBool::new(false),
    )
    .unwrap();
    let t = target();
    let j = import_aaf(&t.s, ireq(&mxf));
    assert_eq!(j.state, JobStateRow::Done, "{j:?}");
    let r = j.report.unwrap();
    assert!(
        r.items
            .iter()
            .flat_map(|i| &i.details)
            .any(|d| d.contains(core_io::aaf::timeline::MXF)),
        "{r:?}"
    );
    assert!(t.s.media().unwrap().iter().all(|m| m.offline.is_some()));
}

/// Ruling 7: a mono-pairs export joins back into one stereo item, its two
/// mono halves gone from `Media/`; without joining it is two mono tracks.
#[cfg(target_os = "macos")]
#[test]
fn pairs_join_into_stereo_media() {
    let _g = serial();
    let p = project();
    let (_, folder) = export_aaf(&p.s, areq(p.seq, &p.out));
    let aaf = folder.join("Song.aaf");

    let t = target();
    let j = import_aaf(&t.s, ireq(&aaf));
    assert_eq!(j.state, JobStateRow::Done, "{j:?}");
    let names: Vec<String> =
        t.s.lanes(first_seq(&t.s).0)
            .unwrap()
            .into_iter()
            .map(|l| l.name)
            .collect();
    assert_eq!(names, ["Mono", "Stereo"]);
    let media = t.s.media().unwrap();
    let mut ch: Vec<u16> = media.iter().map(|m| m.channels).collect();
    ch.sort();
    assert_eq!(ch, [1, 2], "{media:?}");
    assert_eq!(media_cafs(&t.pkg).len(), 2, "{:?}", media_files(&t.pkg));
    // The joined item's samples are the source's, channel by channel.
    let stereo = media.iter().find(|m| m.channels == 2).unwrap();
    let r = core_media::caf::CafReader::open(&t.pkg.join(&stereo.path)).unwrap();
    let mut buf = vec![0f32; 2 * 64];
    let mut scratch = Vec::new();
    assert_eq!(r.read(0, &mut buf, &mut scratch).unwrap(), 64);
    for f in 0..64u64 {
        for c in 0..2u64 {
            let got = buf[(f * 2 + c) as usize];
            assert!((got - sample(f, c)).abs() < 1e-5, "{f} {c}: {got}");
        }
    }

    let t = target();
    let j = import_aaf(
        &t.s,
        AafImportRequestRow {
            join_pairs: false,
            ..ireq(&aaf)
        },
    );
    assert_eq!(j.state, JobStateRow::Done, "{j:?}");
    let names: Vec<String> =
        t.s.lanes(first_seq(&t.s).0)
            .unwrap()
            .into_iter()
            .map(|l| l.name)
            .collect();
    assert_eq!(names, ["Mono", "Stereo L", "Stereo R"]);
    assert!(t.s.media().unwrap().iter().all(|m| m.channels == 1));
}

/// Hand-off A: two clips each playing one channel of one stereo WAV play
/// one mono item each, of the right channel, with no "play both channels"
/// line; a third clip playing the same file whole plays it in stereo.
#[cfg(target_os = "macos")]
#[test]
fn one_channel_of_a_stereo_file_plays_that_channel() {
    use core_io::aaf::dict::{class, pid};
    use core_io::aaf::store::Prop;
    let _g = serial();
    let mut b = Builder::new();
    let (seq, a) = b.sequence("Split", "A", None).unwrap();
    let bl = b.lane(seq, "B", None).unwrap();
    let cl = b.lane(seq, "C", None).unwrap();
    let m = b.media("Media/st.caf", FRAMES, 2).unwrap();
    for (lane, tick) in [(a, 0), (bl, Q), (cl, 2 * Q)] {
        b.region(
            seq,
            lane,
            tick,
            m,
            0,
            FRAMES,
            0.0,
            Fade::default(),
            Fade::default(),
        )
        .unwrap();
    }
    let doc = b.finish();
    let mut x = ax::export(
        &doc,
        &ax::AafOptions {
            clip_gain: false,
            automation: false,
            ..options(seq, true, ax::Stereo::Stereo)
        },
        &env(),
        &std::sync::atomic::AtomicBool::new(false),
    )
    .unwrap();
    assert_eq!(x.audio.len(), 1, "one stereo file");
    // The composition's clips (those that play a master mob): A plays
    // channel 1, B channel 2, C the whole file.
    let masters: Vec<Vec<u8>> = x
        .store
        .objects
        .iter()
        .filter(|o| o.class == class::MASTER_MOB)
        .filter_map(|o| match o.props.get(&pid::MOB_MOB_ID) {
            Some(Prop::Data(d)) => Some(d.clone()),
            _ => None,
        })
        .collect();
    let clips: Vec<usize> = x
        .store
        .objects
        .iter()
        .enumerate()
        .filter(|(_, o)| o.class == class::SOURCE_CLIP)
        .filter(|(_, o)| {
            matches!(o.props.get(&pid::SOURCE_REFERENCE_SOURCE_ID),
                Some(Prop::Data(d)) if masters.contains(d))
        })
        .map(|(i, _)| i)
        .collect();
    assert_eq!(clips.len(), 3);
    for (k, i) in clips.iter().take(2).enumerate() {
        x.store.objects[*i].props.insert(
            pid::SOURCE_REFERENCE_CHANNEL_IDS,
            Prop::Data((k as u32 + 1).to_le_bytes().to_vec()),
        );
    }
    // 24-bit, as the export's descriptor says: left at a quarter scale,
    // right at minus a half.
    let (l, r) = (1i32 << 21, -(1i32 << 22));
    let mut wav = ax_wav_header(FRAMES as u32);
    for _ in 0..FRAMES {
        wav.extend_from_slice(&l.to_le_bytes()[..3]);
        wav.extend_from_slice(&r.to_le_bytes()[..3]);
    }
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("split.aaf");
    ax::write(
        &x,
        &path,
        &mut |_, w| {
            w.write_all(&wav)
                .map_err(|e| core_io::IoError::Io(e.to_string()))
        },
        &std::sync::atomic::AtomicBool::new(false),
    )
    .unwrap();

    let t = target();
    let j = import_aaf(&t.s, ireq(&path));
    assert_eq!(j.state, JobStateRow::Done, "{j:?}");
    let report = j.report.unwrap();
    assert_eq!(
        items(
            &report,
            ReportKindRow::Changed,
            core_io::aaf::import::ONE_CHANNEL
        ),
        0,
        "{report:?}"
    );
    let d = doc_of(&t.s);
    let s = d.sequence(first_seq(&t.s)).unwrap();
    let first_sample = |lane: usize| {
        let e = s.lanes[lane]
            .active()
            .unwrap()
            .events
            .values()
            .find(|e| matches!(e.body, EventBody::AudioRegion { .. }))
            .unwrap()
            .clone();
        let EventBody::AudioRegion { media, .. } = e.body else {
            unreachable!()
        };
        let item = d.media().get(&media).unwrap().clone();
        let rd = core_media::caf::CafReader::open(&t.pkg.join(&item.path)).unwrap();
        let ch = usize::from(rd.info().channels);
        let mut buf = vec![0f32; ch * 16];
        let mut scratch = Vec::new();
        rd.read(0, &mut buf, &mut scratch).unwrap();
        (item.channels, buf[..ch].to_vec())
    };
    let (ca, va) = first_sample(0);
    let (cb, vb) = first_sample(1);
    let (cc, vc) = first_sample(2);
    assert_eq!((ca, cb, cc), (1, 1, 2));
    assert!((va[0] - 0.25).abs() < 1e-4, "{va:?}");
    assert!((vb[0] + 0.5).abs() < 1e-4, "{vb:?}");
    assert!(
        (vc[0] - 0.25).abs() < 1e-4 && (vc[1] + 0.5).abs() < 1e-4,
        "{vc:?}"
    );
    // Two mono items and the stereo one; nothing else in `Media/`.
    assert_eq!(t.s.media().unwrap().len(), 3);
    assert_eq!(media_cafs(&t.pkg).len(), 3, "{:?}", media_files(&t.pkg));
}

/// The cancels after conversion: stages 7 (first converted), 8 (joined)
/// and 9 (mapped) leave `Media/`, `Media/.incoming/` and the document as
/// they were.
#[cfg(target_os = "macos")]
#[test]
fn cancel_after_conversion_leaves_nothing() {
    let _g = serial();
    let p = project();
    let (_, folder) = export_aaf(&p.s, areq(p.seq, &p.out));
    let aaf = folder.join("Song.aaf");
    let t = target();
    let before = media_files(&t.pkg);
    let doc = t.s.debug_document_json();
    for stage in 7..=9 {
        t.s.debug_aaf_cancel_at(Some(stage)).unwrap();
        let j = import_aaf(&t.s, ireq(&aaf));
        assert_eq!(j.state, JobStateRow::Cancelled, "stage {stage}: {j:?}");
        idle_until(&t.s, |s| s.debug_overviews_idle());
        assert_eq!(media_cafs(&t.pkg), Vec::<String>::new(), "stage {stage}");
        assert_eq!(
            listing(&t.pkg.join("Media/.incoming"))
                .into_iter()
                .filter(|f| !f.ends_with('/'))
                .collect::<Vec<_>>(),
            Vec::<String>::new(),
            "stage {stage}"
        );
        assert_eq!(t.s.debug_document_json(), doc, "stage {stage}");
    }
    assert_eq!(media_files(&t.pkg), before);
    assert_eq!(import_aaf(&t.s, ireq(&aaf)).state, JobStateRow::Done);
}

// ---------------------------------------------------------------- fixtures built through core-io

/// `AafOptions` for `seq`: 24-bit, 25 fps, gain and automation on.
#[cfg(target_os = "macos")]
fn options(seq: SeqId, embed: bool, stereo: ax::Stereo) -> ax::AafOptions {
    ax::AafOptions {
        seq,
        embed,
        depth: core_model::format::BitDepth::Int24,
        stereo,
        hour_start: false,
        fps: ax::Fps::F25,
        clip_gain: true,
        automation: true,
        left_out: Default::default(),
    }
}

#[cfg(target_os = "macos")]
fn env() -> ax::ExportEnv<'static> {
    ax::ExportEnv {
        app_version: "0.1.0",
        export_id: "00000000-0000-4000-8000-000000000001",
        modified: 1_700_000_000,
    }
}

/// `s` as UTF-16LE with a NUL.
#[cfg(target_os = "macos")]
fn utf16z(s: &str) -> Vec<u8> {
    let mut out: Vec<u8> = s.encode_utf16().flat_map(u16::to_le_bytes).collect();
    out.extend_from_slice(&[0, 0]);
    out
}

/// The 44-byte header of a 24-bit stereo 48 kHz WAV of `frames` frames.
#[cfg(target_os = "macos")]
fn ax_wav_header(frames: u32) -> Vec<u8> {
    let data = frames * 6;
    let mut v = Vec::new();
    v.extend_from_slice(b"RIFF");
    v.extend_from_slice(&(36 + data).to_le_bytes());
    v.extend_from_slice(b"WAVEfmt ");
    v.extend_from_slice(&16u32.to_le_bytes());
    v.extend_from_slice(&1u16.to_le_bytes());
    v.extend_from_slice(&2u16.to_le_bytes());
    v.extend_from_slice(&48_000u32.to_le_bytes());
    v.extend_from_slice(&(48_000u32 * 6).to_le_bytes());
    v.extend_from_slice(&6u16.to_le_bytes());
    v.extend_from_slice(&24u16.to_le_bytes());
    v.extend_from_slice(b"data");
    v.extend_from_slice(&data.to_le_bytes());
    v
}
