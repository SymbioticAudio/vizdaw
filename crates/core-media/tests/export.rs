//! Bounce files (plan 5f-2 Task 1; spec §9.2, §12.3; Rulings 1, 3, 12, 13).

use core_media::caf::{CafReader, CafWriter, WriteMode};
use core_media::export::{
    Container, ExportFormat, PcmWriter, check_destination, check_folder, export, partial_path,
    write_partial,
};
use core_media::import::CafImporter;
use core_model::format::BitDepth;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

fn signal(frames: usize) -> Vec<f32> {
    (0..frames)
        .flat_map(|f| {
            let x = ((f % 97) as f32 / 97.0) - 0.5;
            [x * 0.9, -x * 0.5]
        })
        .collect()
}

fn float_caf(dir: &Path, frames: usize) -> PathBuf {
    let p = dir.join("render.caf");
    let mut w = CafWriter::create(&p, 48_000, 2, BitDepth::Float32, WriteMode::Normal).unwrap();
    w.write(&signal(frames)).unwrap();
    w.finish().unwrap();
    p
}

fn fmt(container: Container, depth: BitDepth, dither: bool) -> ExportFormat {
    ExportFormat {
        container,
        depth,
        rate: 48_000,
        dither,
        dither_stream: 0,
    }
}

/// `(rate, channels, bits, float, frames, samples as i32 or f32 bits)` of a WAV.
fn read_wav(p: &Path) -> (u32, u16, u16, bool, u64, Vec<i64>) {
    let b = std::fs::read(p).unwrap();
    assert_eq!(&b[0..4], b"RIFF");
    assert_eq!(
        u32::from_le_bytes(b[4..8].try_into().unwrap()) as usize,
        b.len() - 8,
        "RIFF size patched"
    );
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

/// `(form type, channels, frames, bits, first 8 bytes of SSND samples)` of an AIFF/AIFC.
fn read_aiff(p: &Path) -> ([u8; 4], u16, u32, u16, Vec<u8>) {
    let b = std::fs::read(p).unwrap();
    assert_eq!(&b[0..4], b"FORM");
    assert_eq!(
        u32::from_be_bytes(b[4..8].try_into().unwrap()) as usize,
        b.len() - 8
    );
    let form: [u8; 4] = b[8..12].try_into().unwrap();
    let mut at = 12;
    let (mut ch, mut frames, mut bits, mut ssnd) = (0, 0, 0, Vec::new());
    while at + 8 <= b.len() {
        let id = &b[at..at + 4];
        let n = u32::from_be_bytes(b[at + 4..at + 8].try_into().unwrap()) as usize;
        let body = &b[at + 8..at + 8 + n];
        if id == b"COMM" {
            ch = u16::from_be_bytes(body[0..2].try_into().unwrap());
            frames = u32::from_be_bytes(body[2..6].try_into().unwrap());
            bits = u16::from_be_bytes(body[6..8].try_into().unwrap());
            assert_eq!(&body[8..10], &[0x40, 0x0E], "48 kHz as an 80-bit extended");
            if &form == b"AIFC" {
                assert_eq!(&body[18..22], b"fl32");
            }
        }
        if id == b"SSND" {
            ssnd = body[8..16.min(body.len())].to_vec();
        }
        at += 8 + n + (n & 1);
    }
    (form, ch, frames, bits, ssnd)
}

fn run(
    src: &Path,
    dest: &Path,
    f: &ExportFormat,
    replace: bool,
) -> Result<core_media::export::Exported, core_media::MediaError> {
    export(
        &CafImporter,
        src,
        dest,
        f,
        None,
        replace,
        &mut |_| {},
        &AtomicBool::new(false),
    )
}

/// Every file in `dir`, by name.
fn names(dir: &Path) -> Vec<String> {
    let mut v: Vec<String> = std::fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    v.sort();
    v
}

#[test]
fn every_container_and_depth_writes_the_frames() {
    let d = tempfile::tempdir().unwrap();
    let src = float_caf(d.path(), 1000);
    for (c, depth, bits) in [
        (Container::Wav, BitDepth::Int16, 16),
        (Container::Wav, BitDepth::Int24, 24),
        (Container::Wav, BitDepth::Float32, 32),
        (Container::Aiff, BitDepth::Int16, 16),
        (Container::Aiff, BitDepth::Int24, 24),
        (Container::Aiff, BitDepth::Float32, 32),
        (Container::Caf, BitDepth::Int24, 24),
    ] {
        let dest = d.path().join(format!("out {bits}.{}", c.extension()));
        let e = run(&src, &dest, &fmt(c, depth, false), false).unwrap();
        assert_eq!((e.frames, e.channels), (1000, 2), "{c:?} {depth:?}");
        assert!(!partial_path(&dest).exists());
        match c {
            Container::Wav => {
                let (rate, ch, b, float, frames, _) = read_wav(&dest);
                assert_eq!(
                    (rate, ch, b, float, frames),
                    (48_000, 2, bits, depth == BitDepth::Float32, 1000)
                );
            }
            Container::Aiff => {
                let (form, ch, frames, b, _) = read_aiff(&dest);
                assert_eq!(
                    &form,
                    if depth == BitDepth::Float32 {
                        b"AIFC"
                    } else {
                        b"AIFF"
                    }
                );
                assert_eq!((ch, frames, b), (2, 1000, bits));
            }
            Container::Caf => {
                let r = CafReader::open(&dest).unwrap();
                assert_eq!((r.info().frames, r.info().depth), (1000, BitDepth::Int24));
            }
        }
    }
}

#[test]
fn wav_and_caf_quantize_alike() {
    let d = tempfile::tempdir().unwrap();
    let src = float_caf(d.path(), 500);
    run(
        &src,
        &d.path().join("a.wav"),
        &fmt(Container::Wav, BitDepth::Int16, true),
        false,
    )
    .unwrap();
    run(
        &src,
        &d.path().join("a.caf"),
        &fmt(Container::Caf, BitDepth::Int16, true),
        false,
    )
    .unwrap();
    let (_, _, _, _, _, wav) = read_wav(&d.path().join("a.wav"));
    let r = CafReader::open(&d.path().join("a.caf")).unwrap();
    let mut back = vec![0f32; 1000];
    r.read(0, &mut back, &mut Vec::new()).unwrap();
    for (w, c) in wav.iter().zip(back.iter()) {
        assert_eq!(*w, (*c * 32_768.0).round() as i64);
    }
}

/// At 16 and 24 bits (pre-flight M1: the 24-bit branch dithers when asked).
#[test]
fn dither_changes_only_the_lowest_bit() {
    let d = tempfile::tempdir().unwrap();
    let src = float_caf(d.path(), 4000);
    for depth in [BitDepth::Int16, BitDepth::Int24] {
        let a = d.path().join(format!("plain {depth:?}.wav"));
        let b = d.path().join(format!("dither {depth:?}.wav"));
        run(&src, &a, &fmt(Container::Wav, depth, false), false).unwrap();
        run(&src, &b, &fmt(Container::Wav, depth, true), false).unwrap();
        let (.., x) = read_wav(&a);
        let (.., y) = read_wav(&b);
        assert!(
            x.iter().zip(&y).all(|(p, q)| (p - q).abs() <= 1),
            "{depth:?}"
        );
        assert!(
            x.iter().zip(&y).any(|(p, q)| p != q),
            "dither is there: {depth:?}"
        );
    }
}

/// Final review m5: each dither stream is its own sequence (the files of
/// one bounce do not carry the same dither), and one stream always gives
/// the same integers.
#[test]
fn each_dither_stream_is_its_own() {
    let d = tempfile::tempdir().unwrap();
    let src = float_caf(d.path(), 4000);
    for (container, depth) in [
        (Container::Wav, BitDepth::Int16),
        (Container::Aiff, BitDepth::Int24),
        (Container::Caf, BitDepth::Int16),
    ] {
        let ext = container.extension();
        let samples = |name: &str, stream: u64| {
            let dest = d.path().join(format!("{name} {depth:?}.{ext}"));
            let f = ExportFormat {
                dither_stream: stream,
                ..fmt(container, depth, true)
            };
            run(&src, &dest, &f, false).unwrap();
            std::fs::read(&dest).unwrap()
        };
        let (a, again, b) = (samples("a", 0), samples("again", 0), samples("b", 1));
        assert_eq!(a, again, "{container:?} {depth:?}: one stream, one result");
        assert_eq!(a.len(), b.len());
        assert_ne!(
            a, b,
            "{container:?} {depth:?}: another stream, other dither"
        );
    }
}

#[test]
fn float_ignores_dither() {
    let d = tempfile::tempdir().unwrap();
    let src = float_caf(d.path(), 300);
    run(
        &src,
        &d.path().join("a.wav"),
        &fmt(Container::Wav, BitDepth::Float32, false),
        false,
    )
    .unwrap();
    run(
        &src,
        &d.path().join("b.wav"),
        &fmt(Container::Wav, BitDepth::Float32, true),
        false,
    )
    .unwrap();
    assert_eq!(
        std::fs::read(d.path().join("a.wav")).unwrap(),
        std::fs::read(d.path().join("b.wav")).unwrap()
    );
}

/// Both signs count (Ruling 13); a float file keeps the overs.
#[test]
fn clipping_is_counted() {
    let d = tempfile::tempdir().unwrap();
    let p = d.path().join("loud.wav");
    let mut w = PcmWriter::create(&p, &fmt(Container::Wav, BitDepth::Int16, false), 1).unwrap();
    // Within half a step of full scale rounds to full scale: not clipped
    // (final review m6).
    w.write(&[
        0.5,
        1.5,
        -2.0,
        1.0,
        -1.0,
        32_767.4 / 32_768.0,
        -32_768.4 / 32_768.0,
    ])
    .unwrap();
    assert_eq!(
        w.finish().unwrap().clipped,
        3,
        "1.0 is one past the largest 16-bit value; -1.0 fits"
    );
    let (.., s) = read_wav(&p);
    assert_eq!(s[5..], [32_767, -32_768]);
    let f = d.path().join("loud float.wav");
    let mut w = PcmWriter::create(&f, &fmt(Container::Wav, BitDepth::Float32, false), 1).unwrap();
    w.write(&[0.5, 1.5, -2.0]).unwrap();
    assert_eq!(w.finish().unwrap().clipped, 0);
    let (.., s) = read_wav(&f);
    assert_eq!(
        f32::from_bits(s[1] as u32),
        1.5,
        "a float file keeps samples above 0 dBFS"
    );
}

#[test]
fn a_frame_limit_cuts_the_file() {
    let d = tempfile::tempdir().unwrap();
    let src = float_caf(d.path(), 1000);
    let dest = d.path().join("cut.wav");
    let e = export(
        &CafImporter,
        &src,
        &dest,
        &fmt(Container::Wav, BitDepth::Int24, false),
        Some(640),
        false,
        &mut |_| {},
        &AtomicBool::new(false),
    )
    .unwrap();
    assert_eq!(e.frames, 640);
}

#[test]
fn an_existing_file_is_refused_unless_replaced() {
    let d = tempfile::tempdir().unwrap();
    let src = float_caf(d.path(), 100);
    let dest = d.path().join("taken.wav");
    std::fs::write(&dest, b"keep me").unwrap();
    let e = run(
        &src,
        &dest,
        &fmt(Container::Wav, BitDepth::Int16, true),
        false,
    )
    .unwrap_err();
    assert!(e.to_string().contains("exists"), "{e}");
    assert_eq!(std::fs::read(&dest).unwrap(), b"keep me");
    run(
        &src,
        &dest,
        &fmt(Container::Wav, BitDepth::Int16, true),
        true,
    )
    .unwrap();
    assert_eq!(read_wav(&dest).4, 100);
}

/// Review Focus 3: a cancel once more than half is written, over an
/// existing file with `replace`, leaves the old file byte-identical.
#[test]
fn replace_keeps_the_old_file_until_the_new_one_is_whole() {
    let d = tempfile::tempdir().unwrap();
    let src = float_caf(d.path(), 80_000);
    let dest = d.path().join("old.wav");
    std::fs::write(&dest, b"old").unwrap();
    let cancel = AtomicBool::new(false);
    let mut seen = Vec::new();
    let e = export(
        &CafImporter,
        &src,
        &dest,
        &fmt(Container::Wav, BitDepth::Int16, true),
        None,
        true,
        &mut |p| {
            seen.push(p);
            if p > 0.5 {
                cancel.store(true, Ordering::Relaxed);
            }
        },
        &cancel,
    );
    assert!(matches!(e, Err(core_media::MediaError::Cancelled)), "{e:?}");
    assert!(
        seen.iter().any(|p| *p > 0.5 && *p < 1.0),
        "cancelled mid-write: {seen:?}"
    );
    assert_eq!(
        std::fs::read(&dest).unwrap(),
        b"old",
        "a cancelled export leaves the old file"
    );
    assert!(!partial_path(&dest).exists());
    assert_eq!(names(d.path()), ["old.wav", "render.caf"]);
}

#[test]
fn cancel_and_failure_leave_no_partial_file() {
    let d = tempfile::tempdir().unwrap();
    let src = float_caf(d.path(), 80_000);
    let dest = d.path().join("never.wav");
    // A cancel after the first chunk.
    let cancel = AtomicBool::new(false);
    let mut chunks = 0;
    let e = export(
        &CafImporter,
        &src,
        &dest,
        &fmt(Container::Wav, BitDepth::Int16, true),
        None,
        false,
        &mut |_| {
            chunks += 1;
            cancel.store(true, Ordering::Relaxed);
        },
        &cancel,
    );
    assert!(matches!(e, Err(core_media::MediaError::Cancelled)));
    assert!(chunks >= 1, "the partial was being written");
    assert!(!dest.exists() && !partial_path(&dest).exists());
    assert_eq!(names(d.path()), ["render.caf"]);
    // A failure: the partial cannot be created (a folder holds its name).
    std::fs::create_dir(partial_path(&dest)).unwrap();
    assert!(
        run(
            &src,
            &dest,
            &fmt(Container::Wav, BitDepth::Int16, true),
            false
        )
        .is_err()
    );
    assert!(!dest.exists());
    assert!(
        partial_path(&dest).is_dir(),
        "never removes what it did not write"
    );
}

/// Ruling 12 (pre-flight I1): the destination is touched only by `commit`;
/// a dropped pending export leaves nothing.
#[test]
fn a_written_export_is_renamed_only_by_commit() {
    let d = tempfile::tempdir().unwrap();
    let src = float_caf(d.path(), 2000);
    let dest = d.path().join("mix.wav");
    std::fs::write(&dest, b"old").unwrap();
    let f = fmt(Container::Wav, BitDepth::Int24, false);
    let p = write_partial(
        &CafImporter,
        &src,
        &dest,
        &f,
        None,
        true,
        &mut |_| {},
        &AtomicBool::new(false),
    )
    .unwrap();
    assert_eq!(p.exported().frames, 2000);
    assert!(partial_path(&dest).is_file());
    assert_eq!(
        std::fs::read(&dest).unwrap(),
        b"old",
        "written, not yet renamed"
    );
    drop(p);
    assert!(
        !partial_path(&dest).exists(),
        "a dropped pending export removes its partial"
    );
    assert_eq!(std::fs::read(&dest).unwrap(), b"old");
    let p = write_partial(
        &CafImporter,
        &src,
        &dest,
        &f,
        None,
        true,
        &mut |_| {},
        &AtomicBool::new(false),
    )
    .unwrap();
    let e = p.commit().unwrap();
    assert_eq!((e.path.clone(), read_wav(&dest).4), (dest.clone(), 2000));
    assert_eq!(names(d.path()), ["mix.wav", "render.caf"]);
    // Without `replace`, a destination that appeared meanwhile is refused at the rename.
    let late = d.path().join("late.wav");
    let p = write_partial(
        &CafImporter,
        &src,
        &late,
        &f,
        None,
        false,
        &mut |_| {},
        &AtomicBool::new(false),
    )
    .unwrap();
    std::fs::write(&late, b"someone else's").unwrap();
    assert!(p.commit().unwrap_err().to_string().contains("exists"));
    assert_eq!(std::fs::read(&late).unwrap(), b"someone else's");
    assert!(!partial_path(&late).exists());
}

/// Owner answer 2: stems never replace; a taken name is numbered.
#[test]
fn numbered_commits_never_replace() {
    let d = tempfile::tempdir().unwrap();
    let src = float_caf(d.path(), 100);
    let dest = d.path().join("Bass.wav");
    std::fs::write(&dest, b"keep").unwrap();
    std::fs::write(d.path().join("Bass 2.wav"), b"keep 2").unwrap();
    let f = fmt(Container::Wav, BitDepth::Int16, true);
    let p = write_partial(
        &CafImporter,
        &src,
        &dest,
        &f,
        None,
        false,
        &mut |_| {},
        &AtomicBool::new(false),
    )
    .unwrap();
    let e = p.commit_numbered().unwrap();
    assert_eq!(e.path, d.path().join("Bass 3.wav"));
    assert_eq!(std::fs::read(&dest).unwrap(), b"keep");
    assert_eq!(
        std::fs::read(d.path().join("Bass 2.wav")).unwrap(),
        b"keep 2"
    );
    assert_eq!(read_wav(&e.path).4, 100);
    assert_eq!(
        names(d.path()),
        ["Bass 2.wav", "Bass 3.wav", "Bass.wav", "render.caf"]
    );
    // A name VizDAW numbered, taken meanwhile, numbers on from its base
    // (pre-check M-11).
    let p = write_partial(
        &CafImporter,
        &src,
        &d.path().join("Bass 2.wav"),
        &f,
        None,
        false,
        &mut |_| {},
        &AtomicBool::new(false),
    )
    .unwrap();
    assert_eq!(
        p.commit_numbered_from(&dest).unwrap().path,
        d.path().join("Bass 4.wav")
    );
    assert_eq!(
        std::fs::read(d.path().join("Bass 2.wav")).unwrap(),
        b"keep 2"
    );
}

/// Ruling T1-2: the user's own trailing digits are part of the name, never
/// a number to advance: "Kick 808.wav" taken gives "Kick 808 2.wav", then
/// "Kick 808 3.wav".
#[test]
fn numbered_names_keep_the_users_own_digits() {
    let d = tempfile::tempdir().unwrap();
    let src = float_caf(d.path(), 10);
    let dest = d.path().join("Kick 808.wav");
    std::fs::write(&dest, b"keep").unwrap();
    let f = fmt(Container::Wav, BitDepth::Int16, true);
    for want in ["Kick 808 2.wav", "Kick 808 3.wav"] {
        let p = write_partial(
            &CafImporter,
            &src,
            &dest,
            &f,
            None,
            false,
            &mut |_| {},
            &AtomicBool::new(false),
        )
        .unwrap();
        assert_eq!(p.commit_numbered().unwrap().path, d.path().join(want));
    }
    assert_eq!(std::fs::read(&dest).unwrap(), b"keep");
}

#[test]
fn hostile_exports_are_refused() {
    let d = tempfile::tempdir().unwrap();
    let src = float_caf(d.path(), 10);
    for f in [
        ExportFormat {
            rate: 0,
            ..fmt(Container::Wav, BitDepth::Int16, true)
        },
        ExportFormat {
            rate: 7_999,
            ..fmt(Container::Wav, BitDepth::Int16, true)
        },
        ExportFormat {
            rate: 384_001,
            ..fmt(Container::Wav, BitDepth::Int16, true)
        },
    ] {
        assert!(f.check().is_err() && run(&src, &d.path().join("x.wav"), &f, false).is_err());
    }
    assert!(
        run(
            &src,
            &d.path().join("no/such/folder/x.wav"),
            &fmt(Container::Wav, BitDepth::Int16, true),
            false
        )
        .is_err()
    );
    // The extension must be the container's (pre-flight M19).
    let e = run(
        &src,
        &d.path().join("x.caf"),
        &fmt(Container::Wav, BitDepth::Int16, true),
        false,
    )
    .unwrap_err();
    assert!(e.to_string().contains(".wav"), "{e}");
    // A destination that is a folder.
    std::fs::create_dir(d.path().join("folder.wav")).unwrap();
    assert!(
        run(
            &src,
            &d.path().join("folder.wav"),
            &fmt(Container::Wav, BitDepth::Int16, true),
            true
        )
        .is_err()
    );
    assert!(d.path().join("folder.wav").is_dir());
    // A source that is not a VizDAW CAF.
    std::fs::write(d.path().join("junk.caf"), b"junk").unwrap();
    assert!(
        run(
            &d.path().join("junk.caf"),
            &d.path().join("j.wav"),
            &fmt(Container::Wav, BitDepth::Int16, true),
            false
        )
        .is_err()
    );
    // Final review m3: refused before anything is written, not at the
    // partial: a NUL byte, a partial name over 255 bytes, a folder that
    // cannot be written.
    let f16 = fmt(Container::Wav, BitDepth::Int16, true);
    let e = check_destination(&d.path().join("a\u{0}b.wav"), &f16, false).unwrap_err();
    assert!(e.to_string().contains("NUL"), "{e}");
    let long = d.path().join(format!("{}.wav", "n".repeat(247)));
    assert_eq!(long.file_name().unwrap().len(), 251, "a name that fits");
    let e = check_destination(&long, &f16, false).unwrap_err();
    assert!(e.to_string().contains("too long"), "{e}");
    assert!(run(&src, &long, &f16, false).is_err());
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let ro = d.path().join("ro");
        std::fs::create_dir(&ro).unwrap();
        std::fs::set_permissions(&ro, std::fs::Permissions::from_mode(0o555)).unwrap();
        let e = check_destination(&ro.join("x.wav"), &f16, false).unwrap_err();
        std::fs::set_permissions(&ro, std::fs::Permissions::from_mode(0o755)).unwrap();
        assert!(e.to_string().contains("cannot write"), "{e}");
        assert!(check_folder(&ro).is_ok(), "writable again");
        std::fs::remove_dir(&ro).unwrap();
    }
    assert!(
        Container::Aiff.accepts("AIFF")
            && Container::Aiff.accepts("aif")
            && !Container::Wav.accepts("caf")
    );
    let other_rate = ExportFormat {
        rate: 44_100,
        ..fmt(Container::Wav, BitDepth::Int16, true)
    };
    let e = run(&src, &d.path().join("rate.wav"), &other_rate, false).unwrap_err();
    assert!(
        e.to_string().contains("system importer"),
        "the CAF importer converts depth only: {e}"
    );
    assert!(
        !d.path().join("rate.wav").exists() && !partial_path(&d.path().join("rate.wav")).exists()
    );
    assert_eq!(names(d.path()), ["folder.wav", "junk.caf", "render.caf"]);
}

/// Fix round 1 (review I1, ruling T1-1): a partial a crash left behind is
/// stale (nobody holds its lock), so it is replaced: with and without
/// `replace`, and for a numbered stem.
#[test]
fn a_stale_partial_does_not_block_the_export() {
    let d = tempfile::tempdir().unwrap();
    let src = float_caf(d.path(), 100);
    let f = fmt(Container::Wav, BitDepth::Int16, true);
    let dest = d.path().join("Song.wav");
    std::fs::write(partial_path(&dest), b"left by a crash").unwrap();
    assert_eq!(run(&src, &dest, &f, false).unwrap().frames, 100);
    assert!(!partial_path(&dest).exists());
    std::fs::write(partial_path(&dest), b"left by a crash").unwrap();
    assert_eq!(run(&src, &dest, &f, true).unwrap().frames, 100);
    assert_eq!(read_wav(&dest).4, 100);
    assert!(!partial_path(&dest).exists());
    let stem = d.path().join("Bass.wav");
    std::fs::write(&stem, b"keep").unwrap();
    std::fs::write(partial_path(&stem), b"left by a crash").unwrap();
    let p = write_partial(
        &CafImporter,
        &src,
        &stem,
        &f,
        None,
        false,
        &mut |_| {},
        &AtomicBool::new(false),
    )
    .unwrap();
    assert_eq!(
        p.commit_numbered().unwrap().path,
        d.path().join("Bass 2.wav")
    );
    assert_eq!(std::fs::read(&stem).unwrap(), b"keep");
    assert_eq!(
        names(d.path()),
        ["Bass 2.wav", "Bass.wav", "Song.wav", "render.caf"]
    );
}

/// Fix round 1 (ruling T1-1): a partial another export holds (locked) is
/// refused and left alone; a symlink in its place is never followed.
#[cfg(unix)]
#[test]
fn a_held_partial_refuses() {
    let d = tempfile::tempdir().unwrap();
    let src = float_caf(d.path(), 100);
    let f = fmt(Container::Wav, BitDepth::Int16, true);
    let dest = d.path().join("Song.wav");
    let held = std::fs::File::create(partial_path(&dest)).unwrap();
    // Another export holds it (MSRV 1.85: no `File::try_lock`).
    use std::os::fd::AsRawFd;
    // SAFETY: `held` owns a valid open descriptor for the call.
    assert_eq!(
        unsafe { libc::flock(held.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) },
        0
    );
    let e = run(&src, &dest, &f, true).unwrap_err();
    assert!(
        e.to_string().contains("is being written by another bounce"),
        "{e}"
    );
    assert!(partial_path(&dest).is_file() && !dest.exists());
    // The writer that holds it finishes: the lock is free again.
    drop(held);
    assert_eq!(run(&src, &dest, &f, false).unwrap().frames, 100);
    // A symlink where the partial goes: refused, its target untouched.
    let target = d.path().join("target.txt");
    std::fs::write(&target, b"mine").unwrap();
    let other = d.path().join("Other.wav");
    std::os::unix::fs::symlink(&target, partial_path(&other)).unwrap();
    assert!(run(&src, &other, &f, false).is_err());
    assert_eq!(std::fs::read(&target).unwrap(), b"mine");
    assert!(
        std::fs::symlink_metadata(partial_path(&other))
            .unwrap()
            .file_type()
            .is_symlink()
    );
    assert!(!other.exists());
}
