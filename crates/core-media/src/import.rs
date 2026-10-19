//! Importing audio (sub-project 5 spec §5.1; plan 5a Rulings 7, 8): a
//! platform [`MediaImporter`] decodes a file to float at the project rate;
//! this module writes VizDAW CAF files in the project's depth into
//! `Media/.incoming/` and links them into `Media/` under free names. A
//! file wider than two channels becomes one mono file per channel. An
//! error or a cancel removes everything this import wrote.

use crate::MediaError;
use crate::caf::{CafReader, CafWriter, WriteMode, Written};
use core_model::format::AudioFormat;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

/// The media folder inside a package.
pub const MEDIA_DIR: &str = "Media";
/// Where conversions are written before they get their names.
pub const INCOMING_DIR: &str = "Media/.incoming";
/// Frames per chunk the CAF importer hands on.
const CHUNK_FRAMES: usize = 8192;

/// What a source file holds.
#[derive(Debug, Clone, PartialEq)]
pub struct SourceInfo {
    /// Format name ("WAVE", "AIFF", "FLAC", "CAF", …).
    pub format_name: String,
    /// Sample rate in Hz.
    pub sample_rate: f64,
    /// Bits per sample (0 for compressed formats).
    pub bits: u32,
    /// Channels.
    pub channels: u16,
    /// Frames at the source rate.
    pub frames: u64,
}

/// Decodes audio files (macOS: `platform_macos::media::ExtAudioImporter`).
pub trait MediaImporter: Send + Sync {
    /// What `path` holds.
    fn probe(&self, path: &Path) -> Result<SourceInfo, MediaError>;
    /// Decodes `path` to interleaved float at `rate` (all of its
    /// channels), handing chunks of whole frames to `sink`; stops with
    /// `Cancelled` once `cancel` is set. Returns the frames delivered.
    fn decode(
        &self,
        path: &Path,
        rate: u32,
        sink: &mut dyn FnMut(&[f32]) -> Result<(), MediaError>,
        cancel: &AtomicBool,
    ) -> Result<u64, MediaError>;
}

/// One media file an import made.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Imported {
    /// Package-relative path (`Media/<name>.caf`).
    pub path: String,
    /// Display name (the file stem).
    pub name: String,
    /// The source file's name.
    pub original_name: String,
    /// 1 or 2.
    pub channels: u16,
    /// The project rate it was written at.
    pub sample_rate: u32,
    /// Frames.
    pub length_frames: u64,
    /// BLAKE3 hex of the samples.
    pub content_hash: String,
    /// Samples clipped at full scale.
    pub clipped: u64,
}

/// Reads VizDAW CAF files at the project rate only (Ruling 8): tests,
/// depth-only conversions and platforms without a system importer.
#[derive(Debug, Clone, Copy, Default)]
pub struct CafImporter;

impl MediaImporter for CafImporter {
    fn probe(&self, path: &Path) -> Result<SourceInfo, MediaError> {
        let r = CafReader::open(path)?;
        let i = r.info();
        Ok(SourceInfo {
            format_name: "CAF".into(),
            sample_rate: f64::from(i.sample_rate),
            bits: i.depth.bits(),
            channels: i.channels,
            frames: i.frames,
        })
    }

    fn decode(
        &self,
        path: &Path,
        rate: u32,
        sink: &mut dyn FnMut(&[f32]) -> Result<(), MediaError>,
        cancel: &AtomicBool,
    ) -> Result<u64, MediaError> {
        let r = CafReader::open(path)?;
        let (src, ch) = (r.info().sample_rate, usize::from(r.info().channels));
        if src != rate {
            return Err(MediaError::Unsupported(format!(
                "{src} Hz to {rate} Hz: resampling needs the system importer"
            )));
        }
        let mut buf = vec![0.0f32; CHUNK_FRAMES * ch];
        let mut scratch = Vec::new();
        let mut at = 0u64;
        loop {
            if cancel.load(Ordering::Relaxed) {
                return Err(MediaError::Cancelled);
            }
            let n = r.read(at, &mut buf, &mut scratch)?;
            if n == 0 {
                return Ok(at);
            }
            sink(buf.get(..n * ch).unwrap_or(&[]))?;
            at += n as u64;
        }
    }
}

/// A fresh name for an incoming file (no `uuid` crate: process, time and
/// a counter make it unique).
fn incoming_name() -> String {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    format!(
        "{:x}-{:x}-{:x}.caf",
        std::process::id(),
        nanos,
        NEXT.fetch_add(1, Ordering::Relaxed)
    )
}

/// Removes `paths`, ignoring errors (cleanup after a failure).
fn remove_all(paths: &[PathBuf]) {
    for p in paths {
        let _ = std::fs::remove_file(p);
    }
}

/// [`import_as`] named after the source file's stem.
pub fn import_file(
    importer: &dyn MediaImporter,
    source: &Path,
    package: &Path,
    format: AudioFormat,
    progress: &mut dyn FnMut(f32),
    cancel: &AtomicBool,
) -> Result<Vec<Imported>, MediaError> {
    let stem = source
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    import_as(importer, source, package, format, &stem, progress, cancel)
}

/// Characters a file name may not hold: path separators and NUL become
/// "-"; leading dots are dropped (no hidden files).
fn clean_stem(stem: &str) -> String {
    let s: String = stem
        .chars()
        .map(|c| {
            if matches!(c, '/' | '\\' | ':' | '\0') {
                '-'
            } else {
                c
            }
        })
        .collect();
    let s = s.trim_start_matches('.').trim().to_string();
    if s.is_empty() { "Audio".into() } else { s }
}

/// Whether `e` means the volume cannot hard-link (exFAT, SMB, …) or the
/// link crosses volumes, so a rename into place is the way (Ruling F7).
fn no_hard_links(e: &std::io::Error) -> bool {
    #[cfg(unix)]
    if let Some(c) = e.raw_os_error() {
        return c == libc::EXDEV || c == libc::ENOTSUP || c == libc::EPERM;
    }
    #[cfg(not(unix))]
    let _ = e;
    false
}

/// Moves `from` to `to`, failing with `AlreadyExists` (and changing
/// nothing) if `to` exists. macOS: `renamex_np(RENAME_EXCL)`.
#[cfg(target_os = "macos")]
fn rename_excl(from: &Path, to: &Path) -> std::io::Result<()> {
    use std::os::unix::ffi::OsStrExt;
    let c = |p: &Path| {
        std::ffi::CString::new(p.as_os_str().as_bytes())
            .map_err(|_| std::io::Error::from(std::io::ErrorKind::InvalidInput))
    };
    let (f, t) = (c(from)?, c(to)?);
    // SAFETY: both pointers are valid NUL-terminated strings for the call.
    let r = unsafe { libc::renamex_np(f.as_ptr(), t.as_ptr(), libc::RENAME_EXCL) };
    if r == 0 {
        Ok(())
    } else {
        Err(std::io::Error::last_os_error())
    }
}

/// Moves `from` to `to`, failing with `AlreadyExists` (and changing
/// nothing) if `to` exists: copy into a new file, then remove the source.
#[cfg(not(target_os = "macos"))]
fn rename_excl(from: &Path, to: &Path) -> std::io::Result<()> {
    let mut src = std::fs::File::open(from)?;
    let mut dst = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(to)?;
    if let Err(e) = std::io::copy(&mut src, &mut dst) {
        let _ = std::fs::remove_file(to);
        return Err(e);
    }
    drop(dst);
    std::fs::remove_file(from)
}

/// Links `from` into `media` as `<stem>.caf`, `<stem> 2.caf`, … (the
/// first free name; neither `link` nor the exclusive rename replaces a
/// file); returns the name. Where hard links are unavailable the file is
/// renamed into place instead (the source is then gone).
fn link_free(from: &Path, media: &Path, stem: &str) -> Result<String, MediaError> {
    link_free_with(from, media, stem, false)
}

/// [`link_free`]; `force_fallback` skips the hard link (tests).
fn link_free_with(
    from: &Path,
    media: &Path,
    stem: &str,
    force_fallback: bool,
) -> Result<String, MediaError> {
    for n in 1..=10_000u32 {
        let name = if n == 1 {
            format!("{stem}.caf")
        } else {
            format!("{stem} {n}.caf")
        };
        let to = media.join(&name);
        let linked = if force_fallback {
            Err(std::io::Error::from(std::io::ErrorKind::Unsupported))
        } else {
            std::fs::hard_link(from, &to)
        };
        let done = match linked {
            Err(e) if force_fallback || no_hard_links(&e) => rename_excl(from, &to),
            other => other,
        };
        match done {
            Ok(()) => return Ok(name),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(e) => return Err(e.into()),
        }
    }
    Err(MediaError::Io(format!("no free name for {stem}")))
}

/// Converts `source` into `package`'s `Media/` in `format`, naming the
/// result after `stem`. One file for mono or stereo; one mono file per
/// channel (`<stem> ch1`, …) for wider sources. `progress` gets 0–1.
pub fn import_as(
    importer: &dyn MediaImporter,
    source: &Path,
    package: &Path,
    format: AudioFormat,
    stem: &str,
    progress: &mut dyn FnMut(f32),
    cancel: &AtomicBool,
) -> Result<Vec<Imported>, MediaError> {
    let info = importer.probe(source)?;
    if info.channels == 0 {
        return Err(MediaError::Unsupported("a file with no channels".into()));
    }
    let incoming = package.join(INCOMING_DIR);
    std::fs::create_dir_all(&incoming)?;
    let media = package.join(MEDIA_DIR);
    let split = info.channels > 2;
    let files = if split { usize::from(info.channels) } else { 1 };
    let per_file = if split { 1 } else { info.channels };
    let paths: Vec<PathBuf> = (0..files).map(|_| incoming.join(incoming_name())).collect();
    let result = (|| {
        let mut writers = paths
            .iter()
            .map(|p| {
                CafWriter::create(
                    p,
                    format.sample_rate,
                    per_file,
                    format.depth,
                    WriteMode::Normal,
                )
            })
            .collect::<Result<Vec<_>, _>>()?;
        let ch = usize::from(info.channels);
        let expected = if info.sample_rate > 0.0 {
            info.frames as f64 * f64::from(format.sample_rate) / info.sample_rate
        } else {
            0.0
        };
        let mut done = 0u64;
        let mut lanes: Vec<Vec<f32>> = vec![Vec::new(); if split { files } else { 0 }];
        let mut sink = |chunk: &[f32]| -> Result<(), MediaError> {
            if cancel.load(Ordering::Relaxed) {
                return Err(MediaError::Cancelled);
            }
            if split {
                for (k, lane) in lanes.iter_mut().enumerate() {
                    lane.clear();
                    lane.extend(chunk.iter().skip(k).step_by(ch).copied());
                }
                for (w, lane) in writers.iter_mut().zip(&lanes) {
                    w.write(lane)?;
                }
            } else if let Some(w) = writers.first_mut() {
                w.write(chunk)?;
            }
            done += (chunk.len() / ch) as u64;
            if expected > 0.0 {
                progress((done as f64 / expected).min(1.0) as f32);
            }
            Ok(())
        };
        let frames = importer.decode(source, format.sample_rate, &mut sink, cancel)?;
        if frames == 0 {
            return Err(MediaError::Unsupported("the file holds no audio".into()));
        }
        let written: Vec<Written> = writers
            .into_iter()
            .map(CafWriter::finish)
            .collect::<Result<_, _>>()?;
        std::fs::create_dir_all(&media)?;
        let base = clean_stem(stem);
        let original_name = source
            .file_name()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default();
        let mut out = Vec::with_capacity(files);
        let mut linked: Vec<PathBuf> = Vec::new();
        for (k, (p, w)) in paths.iter().zip(written).enumerate() {
            let s = if split {
                format!("{base} ch{}", k + 1)
            } else {
                base.clone()
            };
            let name = match link_free(p, &media, &s) {
                Ok(n) => n,
                Err(e) => {
                    remove_all(&linked);
                    return Err(e);
                }
            };
            linked.push(media.join(&name));
            let display = name.trim_end_matches(".caf").to_string();
            out.push(Imported {
                path: format!("{MEDIA_DIR}/{name}"),
                name: display,
                original_name: original_name.clone(),
                channels: per_file,
                sample_rate: format.sample_rate,
                length_frames: w.frames,
                content_hash: w.content_hash,
                clipped: w.clipped,
            });
        }
        progress(1.0);
        Ok(out)
    })();
    // The incoming files go either way: linked names keep the data.
    remove_all(&paths);
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::caf::hash_file;
    use core_model::format::BitDepth;

    /// Decodes nothing real: `channels` channels of `frames` frames where
    /// channel k of frame f is `(k + 1) * 0.01 + f * 1e-6`, delivered in
    /// chunks of 1000 frames; `cancel_after` sets the flag after that
    /// many chunks.
    struct Fake {
        channels: u16,
        frames: u64,
        cancel_after: Option<usize>,
    }

    impl MediaImporter for Fake {
        fn probe(&self, _: &Path) -> Result<SourceInfo, MediaError> {
            Ok(SourceInfo {
                format_name: "Fake".into(),
                sample_rate: 48_000.0,
                bits: 24,
                channels: self.channels,
                frames: self.frames,
            })
        }

        fn decode(
            &self,
            _: &Path,
            _: u32,
            sink: &mut dyn FnMut(&[f32]) -> Result<(), MediaError>,
            cancel: &AtomicBool,
        ) -> Result<u64, MediaError> {
            let ch = usize::from(self.channels);
            let mut f = 0u64;
            let mut chunks = 0usize;
            while f < self.frames {
                if self.cancel_after == Some(chunks) {
                    cancel.store(true, Ordering::Relaxed);
                }
                let n = (self.frames - f).min(1000);
                let buf: Vec<f32> = (0..n as usize * ch)
                    .map(|i| {
                        let (fr, k) = (f + (i / ch) as u64, i % ch);
                        (k + 1) as f32 * 0.01 + fr as f32 * 1e-6
                    })
                    .collect();
                sink(&buf)?;
                f += n;
                chunks += 1;
            }
            Ok(f)
        }
    }

    fn package() -> tempfile::TempDir {
        tempfile::tempdir().unwrap()
    }

    fn fmt(depth: BitDepth) -> AudioFormat {
        AudioFormat {
            sample_rate: 48_000,
            depth,
        }
    }

    fn import(
        imp: &dyn MediaImporter,
        src: &Path,
        pkg: &Path,
        depth: BitDepth,
    ) -> Result<Vec<Imported>, MediaError> {
        import_file(
            imp,
            src,
            pkg,
            fmt(depth),
            &mut |_| {},
            &AtomicBool::new(false),
        )
    }

    /// Files in `Media/` (not `.incoming`) and in `.incoming`.
    fn listing(pkg: &Path) -> (Vec<String>, Vec<String>) {
        let names = |d: PathBuf| -> Vec<String> {
            let mut v: Vec<String> = std::fs::read_dir(d)
                .map(|r| {
                    r.filter_map(|e| e.ok())
                        .filter(|e| e.path().is_file())
                        .map(|e| e.file_name().to_string_lossy().into_owned())
                        .collect()
                })
                .unwrap_or_default();
            v.sort();
            v
        };
        (names(pkg.join(MEDIA_DIR)), names(pkg.join(INCOMING_DIR)))
    }

    #[test]
    fn stereo_imports_into_one_file_in_the_project_depth() {
        let pkg = package();
        let fake = Fake {
            channels: 2,
            frames: 2500,
            cancel_after: None,
        };
        let mut last = 0.0;
        let got = import_file(
            &fake,
            Path::new("/somewhere/Kick Loop.aiff"),
            pkg.path(),
            fmt(BitDepth::Int16),
            &mut |p| last = p,
            &AtomicBool::new(false),
        )
        .unwrap();
        assert_eq!(last, 1.0);
        assert_eq!(got.len(), 1);
        let i = &got[0];
        assert_eq!(
            (
                i.path.as_str(),
                i.name.as_str(),
                i.original_name.as_str(),
                i.channels,
                i.length_frames
            ),
            (
                "Media/Kick Loop.caf",
                "Kick Loop",
                "Kick Loop.aiff",
                2,
                2500
            )
        );
        let p = pkg.path().join(&i.path);
        let r = CafReader::open(&p).unwrap();
        assert_eq!((r.info().depth, r.info().frames), (BitDepth::Int16, 2500));
        assert_eq!(hash_file(&p).unwrap(), i.content_hash);
        assert_eq!(
            listing(pkg.path()),
            (vec!["Kick Loop.caf".to_string()], vec![])
        );
    }

    #[test]
    fn six_channels_split_into_six_mono_items() {
        let pkg = package();
        let fake = Fake {
            channels: 6,
            frames: 1500,
            cancel_after: None,
        };
        let got = import(
            &fake,
            Path::new("/x/surround.wav"),
            pkg.path(),
            BitDepth::Float32,
        )
        .unwrap();
        let names: Vec<&str> = got.iter().map(|i| i.name.as_str()).collect();
        assert_eq!(
            names,
            vec![
                "surround ch1",
                "surround ch2",
                "surround ch3",
                "surround ch4",
                "surround ch5",
                "surround ch6"
            ]
        );
        for (k, i) in got.iter().enumerate() {
            assert_eq!((i.channels, i.length_frames), (1, 1500));
            let r = CafReader::open(&pkg.path().join(&i.path)).unwrap();
            let mut out = vec![0.0; 1500];
            r.read(0, &mut out, &mut Vec::new()).unwrap();
            assert_eq!(out[10], (k + 1) as f32 * 0.01 + 10.0 * 1e-6);
        }
    }

    #[test]
    fn names_never_collide() {
        let pkg = package();
        let fake = Fake {
            channels: 1,
            frames: 10,
            cancel_after: None,
        };
        let a = import(&fake, Path::new("/x/a.wav"), pkg.path(), BitDepth::Int24).unwrap();
        let b = import(&fake, Path::new("/y/a.mp3"), pkg.path(), BitDepth::Int24).unwrap();
        let c = import(&fake, Path::new("/y/.hidden"), pkg.path(), BitDepth::Int24).unwrap();
        assert_eq!(a[0].path, "Media/a.caf");
        assert_eq!(b[0].path, "Media/a 2.caf");
        assert_eq!(c[0].path, "Media/hidden.caf");
    }

    #[test]
    fn cancel_leaves_nothing_in_media() {
        let pkg = package();
        let fake = Fake {
            channels: 2,
            frames: 10_000,
            cancel_after: Some(3),
        };
        let r = import(&fake, Path::new("/x/a.wav"), pkg.path(), BitDepth::Int24);
        assert_eq!(r, Err(MediaError::Cancelled));
        assert_eq!(listing(pkg.path()), (vec![], vec![]));
    }

    #[test]
    fn garbage_and_empty_files_fail_alone() {
        let pkg = package();
        let src = package();
        let text = src.path().join("notes.wav");
        std::fs::write(&text, "not audio at all").unwrap();
        assert!(matches!(
            import(&CafImporter, &text, pkg.path(), BitDepth::Int24),
            Err(MediaError::NotCaf(_))
        ));
        let empty = Fake {
            channels: 2,
            frames: 0,
            cancel_after: None,
        };
        assert!(matches!(
            import(&empty, Path::new("/x/e.wav"), pkg.path(), BitDepth::Int24),
            Err(MediaError::Unsupported(_))
        ));
        assert_eq!(listing(pkg.path()), (vec![], vec![]));
        // The next import still works.
        let ok = Fake {
            channels: 1,
            frames: 5,
            cancel_after: None,
        };
        assert!(import(&ok, Path::new("/x/ok.wav"), pkg.path(), BitDepth::Int24).is_ok());
    }

    #[test]
    fn the_caf_importer_converts_depth_but_not_rate() {
        let pkg = package();
        let src = package();
        let p = src.path().join("f.caf");
        let mut w = CafWriter::create(&p, 48_000, 1, BitDepth::Float32, WriteMode::Normal).unwrap();
        w.write(&[0.5; 20_000]).unwrap();
        w.finish().unwrap();
        let got = import(&CafImporter, &p, pkg.path(), BitDepth::Int24).unwrap();
        assert_eq!(got[0].length_frames, 20_000);
        let r = CafReader::open(&pkg.path().join(&got[0].path)).unwrap();
        assert_eq!(r.info().depth, BitDepth::Int24);
        let at_44k = import_file(
            &CafImporter,
            &p,
            pkg.path(),
            AudioFormat {
                sample_rate: 44_100,
                depth: BitDepth::Int24,
            },
            &mut |_| {},
            &AtomicBool::new(false),
        );
        assert!(matches!(at_44k, Err(MediaError::Unsupported(_))));
    }

    #[test]
    fn rename_excl_moves_into_a_free_name() {
        let d = package();
        let (a, b) = (d.path().join("a"), d.path().join("b"));
        std::fs::write(&a, "x").unwrap();
        rename_excl(&a, &b).unwrap();
        assert!(!a.exists());
        assert_eq!(std::fs::read_to_string(&b).unwrap(), "x");
    }

    #[test]
    fn rename_excl_never_replaces() {
        let d = package();
        let (a, b) = (d.path().join("a"), d.path().join("b"));
        std::fs::write(&a, "new").unwrap();
        std::fs::write(&b, "old").unwrap();
        let e = rename_excl(&a, &b).unwrap_err();
        assert_eq!(e.kind(), std::io::ErrorKind::AlreadyExists);
        assert_eq!(std::fs::read_to_string(&a).unwrap(), "new");
        assert_eq!(std::fs::read_to_string(&b).unwrap(), "old");
    }

    #[test]
    fn the_fallback_walks_the_free_name_loop() {
        let d = package();
        let media = d.path().join("m");
        std::fs::create_dir_all(&media).unwrap();
        std::fs::write(media.join("s.caf"), "old").unwrap();
        let from = d.path().join("in.caf");
        std::fs::write(&from, "new").unwrap();
        let name = link_free_with(&from, &media, "s", true).unwrap();
        assert_eq!(name, "s 2.caf");
        assert!(!from.exists());
        assert_eq!(std::fs::read_to_string(media.join("s.caf")).unwrap(), "old");
        assert_eq!(
            std::fs::read_to_string(media.join("s 2.caf")).unwrap(),
            "new"
        );
    }
}
