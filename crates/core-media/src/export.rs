//! Bounce files (sub-project 5 spec §9.2; plan 5f-2 Rulings 1, 3, 12, 13):
//! VizDAW's own WAV, AIFF(-C) and CAF writers over the CAF writer's
//! quantizer and TPDF dither, and the export of a float render.
//!
//! Media is write-once: an export writes `.<name>.partial` beside the
//! destination and renames it into place only on [`PendingExport::commit`]
//! (replacing only when asked, after the new file is complete and synced)
//! or [`PendingExport::commit_numbered`] (never replacing). A cancel or an
//! error removes the partial; nothing else is ever removed.
//!
//! A rate change goes through the [`MediaImporter`]'s decoder (macOS:
//! ExtAudioFile); the CAF importer converts nothing, so on other platforms
//! an export at another rate fails cleanly.

use crate::MediaError;
use crate::caf::{CafReader, CafWriter, INT16_SCALE, INT24_SCALE, Tpdf, WriteMode, quantize};
use crate::import::{MediaImporter, rename_excl};
use core_model::format::BitDepth;
use std::ffi::OsString;
use std::fs::{File, OpenOptions};
use std::io::{Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

/// The lowest export rate in Hz.
pub const MIN_RATE: u32 = 8_000;
/// The highest export rate in Hz.
pub const MAX_RATE: u32 = 384_000;
/// The most channels an export writes (a VizDAW render is mono or stereo).
const MAX_CHANNELS: u16 = crate::caf::MAX_CHANNELS;
/// The highest number `commit_numbered` tries.
const MAX_NUMBER: u32 = 9999;

/// A file container.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Container {
    /// RIFF WAVE.
    Wav,
    /// AIFF (integers) or AIFF-C (float).
    Aiff,
    /// Core Audio Format.
    Caf,
}

impl Container {
    /// "wav", "aif", "caf".
    pub fn extension(self) -> &'static str {
        match self {
            Container::Wav => "wav",
            Container::Aiff => "aif",
            Container::Caf => "caf",
        }
    }

    /// Whether `ext` (lowercase compared) names this container ("aiff" too).
    pub fn accepts(self, ext: &str) -> bool {
        let ext = ext.to_lowercase();
        ext == self.extension() || (self == Container::Aiff && ext == "aiff")
    }

    /// Display name ("WAV", "AIFF", "CAF").
    fn label(self) -> &'static str {
        match self {
            Container::Wav => "WAV",
            Container::Aiff => "AIFF",
            Container::Caf => "CAF",
        }
    }
}

/// What an export writes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExportFormat {
    /// The container.
    pub container: Container,
    /// The sample format.
    pub depth: BitDepth,
    /// The rate in Hz ([`MIN_RATE`]–[`MAX_RATE`]).
    pub rate: u32,
    /// TPDF dither at 16 and 24 bits (float ignores it).
    pub dither: bool,
}

impl ExportFormat {
    /// Refuses a rate outside [`MIN_RATE`]–[`MAX_RATE`].
    pub fn check(&self) -> Result<(), MediaError> {
        if !(MIN_RATE..=MAX_RATE).contains(&self.rate) {
            return Err(MediaError::Unsupported(format!(
                "a sample rate of {} Hz (exports take {MIN_RATE} to {MAX_RATE} Hz)",
                self.rate
            )));
        }
        Ok(())
    }
}

/// What an export wrote.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Exported {
    /// The file.
    pub path: PathBuf,
    /// Frames written.
    pub frames: u64,
    /// Channels.
    pub channels: u16,
    /// Integer samples clipped at full scale (either sign).
    pub clipped: u64,
}

/// Where a WAV or AIFF header's sizes are patched on `finish`.
#[derive(Debug)]
struct Header {
    /// RIFF (little-endian sizes) or FORM (big-endian).
    riff: bool,
    /// The RIFF or FORM size.
    outer_at: u64,
    /// The frame count (`fact` in a WAV, `COMM` in an AIFF), if any.
    frames_at: Option<u64>,
    /// The `data` or `SSND` size.
    data_at: u64,
    /// Bytes of the `SSND` chunk before its samples (offset, block size).
    data_extra: u32,
}

#[derive(Debug)]
enum Inner {
    /// Boxed: a CAF writer holds a hasher's state.
    Caf(Box<CafWriter>),
    Pcm {
        file: File,
        header: Header,
        dither: Tpdf,
        /// Sample bytes written.
        data_bytes: u64,
        /// The most sample bytes the container's 32-bit sizes allow.
        max_data: u64,
        /// A write failed: the file may hold bytes the counts leave out.
        poisoned: bool,
    },
}

impl std::fmt::Debug for Tpdf {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Tpdf")
    }
}

/// Writes one file (never over an existing one).
#[derive(Debug)]
pub struct PcmWriter {
    path: PathBuf,
    fmt: ExportFormat,
    channels: u16,
    frames: u64,
    clipped: u64,
    bytes: Vec<u8>,
    inner: Inner,
}

/// `rate` as an 80-bit IEEE extended, big-endian (AIFF's `COMM` rate).
fn extended(rate: u32) -> [u8; 10] {
    let mut out = [0u8; 10];
    if rate == 0 {
        return out;
    }
    let shift = rate.leading_zeros();
    let exponent = (16_383 + 31 - shift) as u16;
    let mantissa = u64::from(rate) << (32 + shift);
    let (e, m) = out.split_at_mut(2);
    e.copy_from_slice(&exponent.to_be_bytes());
    m.copy_from_slice(&mantissa.to_be_bytes());
    out
}

/// A WAV or AIFF header for `fmt`, with every size 0 until `finish`.
fn header(fmt: &ExportFormat, channels: u16) -> (Vec<u8>, Header) {
    let float = fmt.depth == BitDepth::Float32;
    let bits = fmt.depth.bits() as u16;
    let block = fmt.depth.bytes() as u16 * channels;
    let mut h: Vec<u8> = Vec::with_capacity(96);
    if fmt.container == Container::Wav {
        h.extend_from_slice(b"RIFF\0\0\0\0WAVE");
        h.extend_from_slice(b"fmt ");
        h.extend_from_slice(&16u32.to_le_bytes());
        h.extend_from_slice(&(if float { 3u16 } else { 1u16 }).to_le_bytes());
        h.extend_from_slice(&channels.to_le_bytes());
        h.extend_from_slice(&fmt.rate.to_le_bytes());
        h.extend_from_slice(&(fmt.rate.saturating_mul(u32::from(block))).to_le_bytes());
        h.extend_from_slice(&block.to_le_bytes());
        h.extend_from_slice(&bits.to_le_bytes());
        let frames_at = if float {
            h.extend_from_slice(b"fact");
            h.extend_from_slice(&4u32.to_le_bytes());
            let at = h.len() as u64;
            h.extend_from_slice(&0u32.to_le_bytes());
            Some(at)
        } else {
            None
        };
        h.extend_from_slice(b"data");
        let data_at = h.len() as u64;
        h.extend_from_slice(&0u32.to_le_bytes());
        return (
            h,
            Header {
                riff: true,
                outer_at: 4,
                frames_at,
                data_at,
                data_extra: 0,
            },
        );
    }
    h.extend_from_slice(b"FORM\0\0\0\0");
    h.extend_from_slice(if float { b"AIFC" } else { b"AIFF" });
    if float {
        h.extend_from_slice(b"FVER");
        h.extend_from_slice(&4u32.to_be_bytes());
        h.extend_from_slice(&0xA280_5140u32.to_be_bytes());
    }
    const NAME: &[u8] = b"32-bit floating point";
    // The pascal string: its count byte and text, padded to an even length.
    let pascal = 1 + NAME.len() + ((1 + NAME.len()) & 1);
    let comm = if float { 18 + 4 + pascal } else { 18 };
    h.extend_from_slice(b"COMM");
    h.extend_from_slice(&(comm as u32).to_be_bytes());
    h.extend_from_slice(&channels.to_be_bytes());
    let frames_at = h.len() as u64;
    h.extend_from_slice(&0u32.to_be_bytes());
    h.extend_from_slice(&bits.to_be_bytes());
    h.extend_from_slice(&extended(fmt.rate));
    if float {
        h.extend_from_slice(b"fl32");
        h.push(NAME.len() as u8);
        h.extend_from_slice(NAME);
        if (1 + NAME.len()) & 1 == 1 {
            h.push(0);
        }
    }
    h.extend_from_slice(b"SSND");
    let data_at = h.len() as u64;
    h.extend_from_slice(&0u32.to_be_bytes());
    // Offset and block size.
    h.extend_from_slice(&[0; 8]);
    (
        h,
        Header {
            riff: false,
            outer_at: 4,
            frames_at: Some(frames_at),
            data_at,
            data_extra: 8,
        },
    )
}

/// The most sample bytes a WAV or AIFF file of `fmt` holds: its outer
/// size (header less 8, data, a pad byte) must fit 32 bits. `None` for CAF.
fn max_data_bytes(fmt: &ExportFormat, channels: u16) -> Option<u64> {
    if fmt.container == Container::Caf {
        return None;
    }
    let head = header(fmt, channels).0.len() as u64;
    Some(u64::from(u32::MAX).saturating_sub(head.saturating_sub(8) + 1))
}

/// The refusal for data a WAV or AIFF file cannot hold.
fn too_big(fmt: &ExportFormat) -> MediaError {
    MediaError::Unsupported(format!(
        "{} files hold at most 4 GB; choose CAF",
        fmt.container.label()
    ))
}

impl PcmWriter {
    /// Creates `path` (never overwriting) for `channels` (1–2) in `fmt`.
    pub fn create(path: &Path, fmt: &ExportFormat, channels: u16) -> Result<PcmWriter, MediaError> {
        fmt.check()?;
        if !(1..=MAX_CHANNELS).contains(&channels) {
            return Err(MediaError::Unsupported(format!(
                "{channels} channels (exports hold 1 or 2)"
            )));
        }
        let inner = if fmt.container == Container::Caf {
            let mut w = CafWriter::create(path, fmt.rate, channels, fmt.depth, WriteMode::Normal)?;
            w.set_dither(fmt.dither);
            Inner::Caf(Box::new(w))
        } else {
            let mut file = OpenOptions::new().write(true).create_new(true).open(path)?;
            let max_data = max_data_bytes(fmt, channels).unwrap_or(0);
            let (bytes, header) = header(fmt, channels);
            if let Err(e) = file.write_all(&bytes) {
                drop(file);
                let _ = std::fs::remove_file(path);
                return Err(e.into());
            }
            Inner::Pcm {
                file,
                header,
                dither: Tpdf::new(),
                data_bytes: 0,
                max_data,
                poisoned: false,
            }
        };
        Ok(PcmWriter {
            path: path.to_path_buf(),
            fmt: *fmt,
            channels,
            frames: 0,
            clipped: 0,
            bytes: Vec::new(),
            inner,
        })
    }

    /// Appends interleaved float frames (a whole number of frames).
    /// Integer depths clip, counting each clipped sample; float keeps
    /// samples above full scale. Non-finite samples are treated as 0
    /// (silence; dithered like silence when dither is on).
    pub fn write(&mut self, interleaved: &[f32]) -> Result<(), MediaError> {
        let ch = usize::from(self.channels);
        let frames = (interleaved.len() / ch) as u64;
        if interleaved.len() % ch != 0 {
            return Err(MediaError::Unsupported(format!(
                "{} samples are not whole {ch}-channel frames",
                interleaved.len()
            )));
        }
        let (file, dither, data_bytes, max_data, poisoned) = match &mut self.inner {
            Inner::Caf(w) => {
                let before = w.clipped();
                w.write(interleaved)?;
                self.clipped += w.clipped() - before;
                self.frames += frames;
                return Ok(());
            }
            Inner::Pcm {
                file,
                dither,
                data_bytes,
                max_data,
                poisoned,
                ..
            } => (file, dither, data_bytes, *max_data, poisoned),
        };
        if *poisoned {
            return Err(MediaError::Io("an earlier write failed".into()));
        }
        let size = self.fmt.depth.bytes();
        let adding = (interleaved.len() * size) as u64;
        if data_bytes.saturating_add(adding) > max_data {
            return Err(too_big(&self.fmt));
        }
        let little = self.fmt.container == Container::Wav;
        let mut clipped = 0u64;
        self.bytes.clear();
        self.bytes.reserve(interleaved.len() * size);
        for &x in interleaved {
            let x = if x.is_finite() { x } else { 0.0 };
            let scale = match self.fmt.depth {
                BitDepth::Float32 => {
                    let b = if little {
                        x.to_le_bytes()
                    } else {
                        x.to_be_bytes()
                    };
                    self.bytes.extend_from_slice(&b);
                    continue;
                }
                BitDepth::Int16 => INT16_SCALE,
                BitDepth::Int24 => INT24_SCALE,
            };
            let d = if self.fmt.dither { dither.next() } else { 0.0 };
            let (s, over) = quantize(x, scale, d);
            clipped += u64::from(over);
            if little {
                let b = s.to_le_bytes();
                self.bytes.extend_from_slice(b.get(..size).unwrap_or(&[]));
            } else {
                let b = s.to_be_bytes();
                self.bytes
                    .extend_from_slice(b.get(4 - size..).unwrap_or(&[]));
            }
        }
        if let Err(e) = file.write_all(&self.bytes) {
            *poisoned = true;
            return Err(e.into());
        }
        *data_bytes += adding;
        self.clipped += clipped;
        self.frames += frames;
        Ok(())
    }

    /// Patches the sizes and syncs the file.
    pub fn finish(self) -> Result<Exported, MediaError> {
        let (frames, clipped) = match self.inner {
            Inner::Caf(w) => {
                let written = w.finish()?;
                (written.frames, written.clipped)
            }
            Inner::Pcm {
                mut file,
                header,
                data_bytes,
                poisoned,
                ..
            } => {
                if poisoned {
                    return Err(MediaError::Io(
                        "a write failed; the file is incomplete".into(),
                    ));
                }
                let pad = data_bytes & 1;
                if pad == 1 {
                    file.write_all(&[0])?;
                }
                let len = file.stream_position()?;
                let size32 = |n: u64| -> Result<[u8; 4], MediaError> {
                    let n = u32::try_from(n)
                        .map_err(|_| MediaError::Unsupported("a file over 4 GB".into()))?;
                    Ok(if header.riff {
                        n.to_le_bytes()
                    } else {
                        n.to_be_bytes()
                    })
                };
                // Data first, so a crash cannot persist sizes beyond the data.
                file.sync_data()?;
                let patches = [
                    Some((header.outer_at, len.saturating_sub(8))),
                    header.frames_at.map(|at| (at, self.frames)),
                    Some((header.data_at, data_bytes + u64::from(header.data_extra))),
                ];
                for (at, value) in patches.into_iter().flatten() {
                    file.seek(SeekFrom::Start(at))?;
                    file.write_all(&size32(value)?)?;
                }
                file.sync_all()?;
                (self.frames, self.clipped)
            }
        };
        Ok(Exported {
            path: self.path,
            frames,
            channels: self.channels,
            clipped,
        })
    }
}

/// The partial file [`write_partial`] writes for `dest`:
/// `<dir>/.<file name>.partial`.
pub fn partial_path(dest: &Path) -> PathBuf {
    let mut name = OsString::from(".");
    if let Some(n) = dest.file_name() {
        name.push(n);
    }
    name.push(".partial");
    dest.with_file_name(name)
}

/// The display name of `p` (its file name).
fn display_name(p: &Path) -> String {
    p.file_name().map_or_else(
        || p.display().to_string(),
        |n| n.to_string_lossy().into_owned(),
    )
}

/// The refusal for a destination that is taken.
fn exists(dest: &Path) -> MediaError {
    MediaError::Unsupported(format!(
        "{} exists; choose another name or replace it",
        display_name(dest)
    ))
}

/// Syncs `p`'s folder, so a rename in it is on the disk (best effort).
fn sync_parent(p: &Path) {
    if let Some(dir) = p.parent().filter(|d| !d.as_os_str().is_empty()) {
        let _ = File::open(dir).and_then(|d| d.sync_all());
    }
}

/// A complete `.partial` waiting for its rename (Ruling 12). Dropping it
/// uncommitted removes the partial.
#[derive(Debug)]
pub struct PendingExport {
    partial: PathBuf,
    replace: bool,
    exported: Exported,
    committed: bool,
    /// The partial's advisory lock, held while it lives (Unix): another
    /// export of the same name sees it busy, not stale. Released after the
    /// partial is renamed or removed.
    _lock: Option<File>,
}

impl Drop for PendingExport {
    fn drop(&mut self) {
        if !self.committed {
            let _ = std::fs::remove_file(&self.partial);
        }
    }
}

impl PendingExport {
    /// What the partial holds (`path` is the destination).
    pub fn exported(&self) -> &Exported {
        &self.exported
    }

    /// Renames the partial onto the destination: `rename` (replacing) when
    /// `write_partial` was given `replace`, else `rename_excl`, which fails
    /// with "exists" (and removes the partial) if the destination appeared.
    pub fn commit(mut self) -> Result<Exported, MediaError> {
        let dest = self.exported.path.clone();
        let done = if self.replace {
            std::fs::rename(&self.partial, &dest)
        } else {
            rename_excl(&self.partial, &dest)
        };
        match done {
            Ok(()) => {
                self.committed = true;
                sync_parent(&dest);
                Ok(self.exported.clone())
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => Err(exists(&dest)),
            Err(e) => Err(e.into()),
        }
    }

    /// Renames the partial onto the destination or, when that name is
    /// taken, onto "<stem> 2.<ext>", "<stem> 3.<ext>", … (never replacing;
    /// owner answer 2). The stem is kept whole, digits and all (Ruling
    /// T1-2): "Kick 808.wav" taken gives "Kick 808 2.wav". `Exported.path`
    /// is the name it took.
    pub fn commit_numbered(self) -> Result<Exported, MediaError> {
        let dest = self.exported.path.clone();
        self.commit_numbered_from(&dest)
    }

    /// [`PendingExport::commit_numbered`] for a destination VizDAW already
    /// numbered from `base` (a stem whose name was taken when the bounce
    /// was planned): when the destination is taken too, the numbers go on
    /// after its own " <n>" from `base`'s stem ("Bass 2.wav" from
    /// "Bass.wav": "Bass 3.wav", …). Only that suffix is advanced (Ruling
    /// T1-2); the files stay in the destination's folder.
    pub fn commit_numbered_from(mut self, base: &Path) -> Result<Exported, MediaError> {
        let dest = self.exported.path.clone();
        let stem = base
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default();
        let ext = dest
            .extension()
            .map(|e| format!(".{}", e.to_string_lossy()))
            .unwrap_or_default();
        let first = dest
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .and_then(|d| {
                d.strip_prefix(stem.as_str())
                    .and_then(|r| r.strip_prefix(' '))
                    .and_then(|n| n.parse::<u32>().ok())
            })
            .map_or(2, |n| n.saturating_add(1).max(2));
        let candidates = std::iter::once(dest.clone())
            .chain((first..=MAX_NUMBER).map(|n| dest.with_file_name(format!("{stem} {n}{ext}"))));
        for to in candidates {
            match rename_excl(&self.partial, &to) {
                Ok(()) => {
                    self.committed = true;
                    sync_parent(&to);
                    self.exported.path = to;
                    return Ok(self.exported.clone());
                }
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
                Err(e) => return Err(e.into()),
            }
        }
        Err(MediaError::Io(format!(
            "no free name for {}",
            display_name(&dest)
        )))
    }
}

/// Removes the partial it guards unless disarmed (a cancel or an error).
struct PartialGuard(Option<PathBuf>);

impl Drop for PartialGuard {
    fn drop(&mut self) {
        if let Some(p) = &self.0 {
            let _ = std::fs::remove_file(p);
        }
    }
}

/// `n` frames at `from` Hz in frames at `to` Hz, rounded.
fn scale_frames(n: u64, from: u32, to: u32) -> u64 {
    if from == to || from == 0 {
        return n;
    }
    let (n, from, to) = (u128::from(n), u128::from(from), u128::from(to));
    u64::try_from((n * to + from / 2) / from).unwrap_or(u64::MAX)
}

/// Checks a destination as [`write_partial`] does: `fmt` is valid, the
/// extension is the container's, the folder exists and `dest` is not a
/// folder; and, without `replace`, that `dest` does not exist ("<name>
/// exists; choose another name or replace it"; a dangling symlink counts).
/// For a caller that refuses before rendering (plan 5f-2 Ruling 3).
pub fn check_destination(dest: &Path, fmt: &ExportFormat, replace: bool) -> Result<(), MediaError> {
    check_dest(dest, fmt)?;
    if !replace && std::fs::symlink_metadata(dest).is_ok() {
        return Err(exists(dest));
    }
    Ok(())
}

/// The refusal for a partial another export holds.
fn busy(dest: &Path) -> MediaError {
    MediaError::Unsupported(format!(
        "{} is being written by another bounce.",
        display_name(dest)
    ))
}

/// Opens `partial` without following a symlink and takes its advisory lock
/// without waiting: `Ok(Some(file))` holds it, `Ok(None)` means another
/// export holds it.
#[cfg(unix)]
fn try_lock(partial: &Path) -> Result<Option<File>, MediaError> {
    use std::os::fd::AsRawFd;
    use std::os::unix::fs::OpenOptionsExt;
    let f = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW)
        .open(partial)?;
    // SAFETY: `f` owns a valid open descriptor for the call; the lock goes
    // with the descriptor when `f` is closed.
    let r = unsafe { libc::flock(f.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) };
    if r == 0 {
        return Ok(Some(f));
    }
    let e = std::io::Error::last_os_error();
    if e.raw_os_error() == Some(libc::EWOULDBLOCK) {
        Ok(None)
    } else {
        Err(e.into())
    }
}

/// Clears the way for `dest`'s partial (review I1, ruling T1-1). A regular
/// file there whose lock is free was left by a crash: it is removed. One
/// whose lock is held is another export's: refused. Anything else (a
/// folder, a symlink) is left alone, and the exclusive create refuses it.
#[cfg(unix)]
fn clear_stale_partial(partial: &Path, dest: &Path) -> Result<(), MediaError> {
    let Ok(meta) = std::fs::symlink_metadata(partial) else {
        return Ok(());
    };
    if !meta.file_type().is_file() {
        return Ok(());
    }
    match try_lock(partial)? {
        Some(_stale) => {
            std::fs::remove_file(partial)?;
            Ok(())
        }
        None => Err(busy(dest)),
    }
}

/// Without advisory locks a leftover partial cannot be told from a live
/// one, so it is reported, never removed.
#[cfg(not(unix))]
fn clear_stale_partial(partial: &Path, dest: &Path) -> Result<(), MediaError> {
    if std::fs::symlink_metadata(partial).is_ok() {
        return Err(MediaError::Unsupported(format!(
            "a leftover {} blocks the export of {}; remove it",
            display_name(partial),
            display_name(dest)
        )));
    }
    Ok(())
}

/// Locks the partial this export just created, and checks that the name
/// still holds that file (another export may have taken it as stale
/// between the create and the lock).
#[cfg(unix)]
fn lock_own_partial(partial: &Path, dest: &Path) -> Result<Option<File>, MediaError> {
    use std::os::unix::fs::MetadataExt;
    let f = try_lock(partial)?.ok_or_else(|| busy(dest))?;
    let (ours, now) = (f.metadata()?, std::fs::symlink_metadata(partial)?);
    if (ours.dev(), ours.ino()) != (now.dev(), now.ino()) {
        return Err(busy(dest));
    }
    Ok(Some(f))
}

#[cfg(not(unix))]
fn lock_own_partial(_: &Path, _: &Path) -> Result<Option<File>, MediaError> {
    Ok(None)
}

/// Refuses a destination before anything is written.
fn check_dest(dest: &Path, fmt: &ExportFormat) -> Result<(), MediaError> {
    fmt.check()?;
    let name = display_name(dest);
    let ext = dest.extension().map(|e| e.to_string_lossy().into_owned());
    if !ext.is_some_and(|e| fmt.container.accepts(&e)) {
        return Err(MediaError::Unsupported(format!(
            "{name} must end in .{} for {}",
            fmt.container.extension(),
            fmt.container.label()
        )));
    }
    let parent = dest
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    if !parent.is_dir() {
        return Err(MediaError::Io(format!(
            "{}: no such folder",
            parent.display()
        )));
    }
    if dest.is_dir() {
        return Err(MediaError::Unsupported(format!("{name} is a folder")));
    }
    Ok(())
}

/// `source` (a VizDAW float CAF) → `.<name>.partial` beside `dest` in
/// `fmt`: at most `frames` source frames (scaled by the rate ratio).
/// Refuses before writing anything (a bad format, extension or folder, a
/// destination that is a folder, a source that is not a VizDAW CAF, a WAV
/// or AIFF over 4 GB, a partial another export is writing). A partial a
/// crash left behind is replaced (Unix: nobody holds its lock). A
/// destination that exists is left to the commit: [`PendingExport::commit`]
/// without `replace` refuses it at the rename, and
/// [`PendingExport::commit_numbered`] numbers around it. A cancel or error
/// removes the partial.
#[allow(clippy::too_many_arguments)]
pub fn write_partial(
    importer: &dyn MediaImporter,
    source: &Path,
    dest: &Path,
    fmt: &ExportFormat,
    frames: Option<u64>,
    replace: bool,
    progress: &mut dyn FnMut(f32),
    cancel: &AtomicBool,
) -> Result<PendingExport, MediaError> {
    check_dest(dest, fmt)?;
    let (src_rate, channels, src_frames) = {
        let r = CafReader::open(source)?;
        let i = r.info();
        (i.sample_rate, i.channels, i.frames)
    };
    let limit = frames.map(|f| scale_frames(f, src_rate, fmt.rate));
    let expected = limit.unwrap_or_else(|| scale_frames(src_frames, src_rate, fmt.rate));
    let bytes = expected
        .saturating_mul(u64::from(channels))
        .saturating_mul(fmt.depth.bytes() as u64);
    if max_data_bytes(fmt, channels).is_some_and(|m| bytes > m) {
        return Err(too_big(fmt));
    }
    let partial = partial_path(dest);
    clear_stale_partial(&partial, dest)?;
    let mut w = PcmWriter::create(&partial, fmt, channels)?;
    // From here on the partial is ours: any way out but success removes it.
    let mut guard = PartialGuard(Some(partial.clone()));
    let lock = match lock_own_partial(&partial, dest) {
        Ok(l) => l,
        Err(e) => {
            // Busy (`Unsupported`): another export holds the name now, so
            // remove nothing. An I/O error leaves the guard to remove ours.
            if matches!(e, MediaError::Unsupported(_)) {
                guard.0 = None;
            }
            return Err(e);
        }
    };
    let ch = usize::from(channels);
    let mut written = 0u64;
    let mut reached = false;
    let decoded = importer.decode(
        source,
        fmt.rate,
        &mut |chunk| {
            let have = (chunk.len() / ch) as u64;
            let take = limit.map_or(have, |l| have.min(l.saturating_sub(written)));
            let samples = usize::try_from(take)
                .unwrap_or(usize::MAX)
                .saturating_mul(ch);
            w.write(chunk.get(..samples).unwrap_or(chunk))?;
            written += take;
            progress(if expected == 0 {
                1.0
            } else {
                (written as f64 / expected as f64).min(1.0) as f32
            });
            if cancel.load(Ordering::Relaxed) {
                return Err(MediaError::Cancelled);
            }
            if limit.is_some_and(|l| written >= l) {
                // Enough: stop the decoder (not a cancel).
                reached = true;
                return Err(MediaError::Cancelled);
            }
            Ok(())
        },
        cancel,
    );
    match decoded {
        Ok(_) => {}
        Err(MediaError::Cancelled) if reached => {}
        Err(e) => return Err(e),
    }
    let mut exported = w.finish()?;
    exported.path = dest.to_path_buf();
    guard.0 = None;
    Ok(PendingExport {
        partial,
        replace,
        exported,
        committed: false,
        _lock: lock,
    })
}

/// [`write_partial`] then [`PendingExport::commit`]. Without `replace`, an
/// existing destination is refused before anything is written ("<name>
/// exists; choose another name or replace it"), and again at the rename if
/// it appeared meanwhile.
#[allow(clippy::too_many_arguments)]
pub fn export(
    importer: &dyn MediaImporter,
    source: &Path,
    dest: &Path,
    fmt: &ExportFormat,
    frames: Option<u64>,
    replace: bool,
    progress: &mut dyn FnMut(f32),
    cancel: &AtomicBool,
) -> Result<Exported, MediaError> {
    check_destination(dest, fmt, replace)?;
    write_partial(
        importer, source, dest, fmt, frames, replace, progress, cancel,
    )?
    .commit()
}
