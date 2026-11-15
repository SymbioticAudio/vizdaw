//! AAF export and import for the app (plan 6f Task 5; spec §6, §7.5, §11):
//! the request rows and checks, the info read, the export job (6b's staged
//! folder, 6d's staged WAVs, `aaf::export::write`) and the import job (6d's
//! extraction and conversion, the L/R join, the one-channel split, the MIDI
//! file beside and the mapping), applied by the session.
//!
//! **The export job:** the destination is checked in the job (`outside`,
//! `free_folder`, `partial_fits`), then the free space (6d's
//! [`SPACE_FACTOR`] × the staged WAV bytes), then each staged file is
//! written as a WAV at the chosen depth, into `Audio/` when external or
//! `.essence/` when embedded; then `<Song> MIDI.mid`, then `<Song>.aaf`
//! (its embedded WAVs streamed in), and the staging folder is renamed into
//! place. Files are dithered when the chosen depth is below the project's.
//!
//! **The import job:** the media folder is checked (lstat), the file
//! opened, its audio extracted and converted (6d's `read_and_convert`),
//! L/R pairs joined into stereo CAFs, each channel a clip plays alone of a
//! stereo file made a mono CAF, the MIDI file beside read, and the
//! composition mapped. Every stage runs inside `catch_unwind`; what it
//! converted is returned so the session removes it when the import is not
//! applied.

use crate::dawproject::{
    ImportFiles, ImportJob, MediaMap, SPACE_FACTOR, Stop, not_enough_space, read_and_convert,
    stage_audio,
};
use crate::error::VizError;
use crate::guard::panic_message;
use crate::interchange::{ReportRow, file_path, io_error, report_row};
use crate::jobs::Work;
use core_io::IoError;
use core_io::aaf::export::{self as ax, AafOptions, Fps, Stereo};
use core_io::aaf::import::{AafEnv, AafPlace, Pair, map, pairs, tempo_and_start};
use core_io::aaf::timeline::{AafFile, Composition};
use core_io::dawproject::export::{AudioFile, LeftOutAudio};
use core_io::dawproject::import::{DpImport, MediaSource};
use core_io::dawproject::package::{AudioPackage, AudioRef};
use core_io::limits::AafLimits;
use core_io::midi_in::MidiImport;
use core_io::report::{ItemKind, Report};
use core_io::write::{free_folder, stage_folder, write_partial};
use core_media::caf::{CafReader, CafWriter, WriteMode};
use core_media::export::{display_name, partial_path};
use core_media::import::{Imported, MEDIA_DIR};
use core_model::document::Document;
use core_model::format::{AudioFormat, BitDepth};
use core_model::ids::SeqId;
use core_model::sequence::Sequence;
use std::collections::{BTreeMap, BTreeSet};
use std::io::Write;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};

// ---- rows ----

/// How a stereo track is written (Ruling 7).
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum AafStereoRow {
    /// Two mono tracks, "<track> L" and "<track> R" (the default).
    MonoPairs,
    /// One two-channel track.
    Stereo,
}

/// The start timecode's frame rate (Ruling 3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum AafFpsRow {
    /// 24 fps.
    Fps24,
    /// 25 fps.
    Fps25,
    /// 29.97 fps drop-frame.
    Fps2997Drop,
    /// 30 fps.
    Fps30,
}

/// The staged WAVs' depth.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum AafDepthRow {
    /// 16-bit.
    Int16,
    /// 24-bit.
    Int24,
}

/// Export a sequence as an AAF folder.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct AafExportRequestRow {
    /// The sequence.
    pub seq: u64,
    /// The folder the export folder goes in; "" for a check.
    pub path: String,
    /// Audio inside the `.aaf` (else WAVs in `Audio/`).
    pub embed: bool,
    /// The WAVs' depth.
    pub depth: AafDepthRow,
    /// How stereo tracks are written.
    pub stereo: AafStereoRow,
    /// The timecode starts at 01:00:00:00 (else 00:00:00:00).
    pub hour_start: bool,
    /// The timecode's rate.
    pub fps: AafFpsRow,
    /// Write region gain.
    pub clip_gain: bool,
    /// Write track volume and pan.
    pub automation: bool,
}

/// Import an AAF file.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct AafImportRequestRow {
    /// The file (absolute, ending in `.aaf`).
    pub path: String,
    /// Into this new, empty project (else added to this one, one undo
    /// step "Import AAF").
    pub new_project: bool,
    /// The MIDI file to read with it (absolute); `None`: the one beside
    /// it, if any (Ruling 8).
    pub midi_path: Option<String>,
    /// A folder to look in for audio the file's locators do not find
    /// (absolute).
    pub media_folder: Option<String>,
    /// Join tracks named as left and right into stereo tracks (Ruling 7).
    pub join_pairs: bool,
}

/// What the app reads of an AAF file before it imports it.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct AafInfoRow {
    /// The rate of the project to make (6d Ruling 17).
    pub sample_rate: u32,
    /// Audio tracks.
    pub tracks: u32,
    /// Distinct audio files.
    pub audio_files: u32,
    /// Of those, the ones that will be offline.
    pub offline: u32,
    /// The file came from VizDAW (Ruling 12).
    pub from_vizdaw: bool,
    /// The MIDI file beside it the import would read (Ruling 8).
    pub midi_beside: Option<String>,
}

// ---- wording ----

/// The refusal of an AAF path with another extension.
pub const NOT_AAF_NAME: &str = "Choose a file ending in .aaf.";

/// The failure of an import into this project whose sequence's tempo map
/// or file start changed while the job ran (the audio was placed on the
/// old one).
pub const TEMPO_CHANGED: &str = "The tempo changed while the AAF was importing. Import it again.";

/// What [`file_path`] calls an AAF file.
pub(crate) const AAF_FILE: &str = "an AAF file";

/// Left out (Ruling 8): the MIDI file found beside the AAF could not be
/// read.
pub fn midi_not_read(why: &str) -> String {
    format!("The MIDI file beside the AAF could not be read: {why}")
}

/// The import's failure when the MIDI file it was given cannot be read.
pub fn midi_file_not_read(name: &str, why: &str) -> String {
    format!("The MIDI file {name} could not be read: {why}")
}

/// The import's failure when its media folder is not a folder.
pub fn not_a_folder(name: &str) -> String {
    format!("{name} is not a folder")
}

fn model(message: impl Into<String>) -> VizError {
    VizError::Model {
        message: message.into(),
    }
}

/// A refusal's own sentence.
fn sentence(e: VizError) -> String {
    match e {
        VizError::Model { message }
        | VizError::Io { message }
        | VizError::Busy { message }
        | VizError::Internal { message } => message,
        other => other.to_string(),
    }
}

fn failed(e: VizError) -> Stop {
    Stop::Failed(sentence(e))
}

fn io_stop(e: IoError) -> Stop {
    match e {
        IoError::Cancelled => Stop::Cancelled,
        other => Stop::Failed(other.to_string()),
    }
}

// ---- request checks ----

/// The export options of `req` in `doc` (its path is not looked at), with
/// nothing left out yet: the caller sets `left_out` once the media is
/// refreshed.
pub(crate) fn options(doc: &Document, req: &AafExportRequestRow) -> Result<AafOptions, VizError> {
    let seq = SeqId(req.seq);
    doc.sequence(seq)
        .map_err(|_| model(format!("sequence {} is not in this project", req.seq)))?;
    Ok(AafOptions {
        seq,
        embed: req.embed,
        depth: match req.depth {
            AafDepthRow::Int16 => BitDepth::Int16,
            AafDepthRow::Int24 => BitDepth::Int24,
        },
        stereo: match req.stereo {
            AafStereoRow::MonoPairs => Stereo::MonoPairs,
            AafStereoRow::Stereo => Stereo::Stereo,
        },
        hour_start: req.hour_start,
        fps: match req.fps {
            AafFpsRow::Fps24 => Fps::F24,
            AafFpsRow::Fps25 => Fps::F25,
            AafFpsRow::Fps2997Drop => Fps::F2997Drop,
            AafFpsRow::Fps30 => Fps::F30,
        },
        clip_gain: req.clip_gain,
        automation: req.automation,
        left_out: LeftOutAudio::new(),
    })
}

/// "<Song> AAF" for sequence `seq` (not numbered: the job numbers it).
pub(crate) fn folder_name(doc: &Document, seq: u64) -> Result<String, VizError> {
    let seq = SeqId(seq);
    doc.sequence(seq)
        .map_err(|_| model(format!("sequence {} is not in this project", seq.0)))?;
    ax::folder_name(doc, seq).map_err(io_error)
}

/// The export's dry run: its report, nothing written.
pub(crate) fn check(doc: &Document, opts: &AafOptions) -> Result<ReportRow, VizError> {
    let r = ax::check(doc, opts).map_err(io_error)?;
    Ok(report_row(&r, "Exported"))
}

/// The AAF path's text, checked as `file_path` checks it, ending in
/// `.aaf` (any case).
pub(crate) fn aaf_path(text: &str) -> Result<PathBuf, VizError> {
    let p = file_path(text, AAF_FILE)?;
    if !p.extension().is_some_and(|e| e.eq_ignore_ascii_case("aaf")) {
        return Err(model(NOT_AAF_NAME));
    }
    Ok(p)
}

/// The MIDI file beside `aaf` (Ruling 8): the first regular file of
/// `<stem> MIDI.mid`, `<stem>.mid` and `<stem>.midi` in its folder.
pub(crate) fn midi_beside(aaf: &Path) -> Option<PathBuf> {
    let stem = aaf.file_stem()?.to_string_lossy().into_owned();
    let dir = aaf.parent()?;
    [
        format!("{stem} MIDI.mid"),
        format!("{stem}.mid"),
        format!("{stem}.midi"),
    ]
    .into_iter()
    .map(|n| dir.join(n))
    .find(|p| std::fs::metadata(p).is_ok_and(|m| m.is_file()))
}

/// Opens the AAF at `text` and reads what [`AafInfoRow`] holds, with the
/// MIDI file beside it.
pub(crate) fn info(text: &str) -> Result<AafInfoRow, VizError> {
    let path = aaf_path(text)?;
    let file = AafFile::open(&path, None, &AafLimits::default(), &AtomicBool::new(false))
        .map_err(io_error)?;
    let i = file.info();
    Ok(AafInfoRow {
        sample_rate: i.sample_rate,
        tracks: i.tracks,
        audio_files: i.audio_files,
        offline: i.offline,
        from_vizdaw: i.from_vizdaw,
        midi_beside: midi_beside(&path).map(|p| p.display().to_string()),
    })
}

// ---- the export job ----

/// Everything the export job needs (all of it owned: it runs on its
/// thread).
pub(crate) struct ExportJob {
    /// The document as it was when the job started.
    pub doc: Document,
    /// What to export.
    pub opts: AafOptions,
    /// The folder the export folder goes in.
    pub parent: PathBuf,
    /// Where exports may not go (the package, the scratch root).
    pub roots: Vec<PathBuf>,
    /// The media root the CAFs are read from.
    pub root: PathBuf,
    /// This export's ID.
    pub export_id: String,
    /// When the job was started, in Unix seconds.
    pub modified: u64,
    /// The stage after which it cancels itself (test hook: 1 staged, 2
    /// the MIDI file written, 3 the `.aaf` written).
    pub cancel_at: Option<u32>,
    /// The free bytes to assume (test hook; `None`: the disk's).
    pub free: Option<u64>,
}

/// The staging folder's folder for embedded WAVs (removed before the
/// commit).
const ESSENCE_DIR: &str = ".essence";
/// The WAV header VizDAW writes at an integer depth.
const WAV_HEADER: u64 = 44;

/// The staged WAVs' bytes at `depth`.
fn staged_bytes(doc: &Document, x: &ax::AafExport, depth: BitDepth) -> u64 {
    x.audio.iter().fold(0u64, |n, s| {
        let one = doc.media().get(&s.media).map_or(0, |m| {
            let ch = if s.channel.is_some() { 1 } else { m.channels };
            m.length_frames
                .saturating_mul(u64::from(ch))
                .saturating_mul(depth.bytes() as u64)
                .saturating_add(WAV_HEADER)
        });
        n.saturating_add(one)
    })
}

/// The export's body: the report and the folder put in place, or how it
/// stopped.
fn export_body(
    job: &ExportJob,
    progress: &AtomicU32,
    cancel: &AtomicBool,
) -> Result<(Report, PathBuf), Stop> {
    let stage = |n: u32| {
        if job.cancel_at == Some(n) {
            cancel.store(true, Ordering::Relaxed);
        }
    };
    let cancelled = || cancel.load(Ordering::Relaxed);
    let set = |f: f32| progress.store(f.to_bits(), Ordering::Relaxed);
    let seq = job.opts.seq;
    let env = ax::ExportEnv {
        app_version: env!("CARGO_PKG_VERSION"),
        export_id: &job.export_id,
        modified: job.modified,
    };
    let x = ax::export(&job.doc, &job.opts, &env, cancel).map_err(io_stop)?;
    let (aaf_name, midi_name) = ax::file_names(&job.doc, seq).map_err(io_stop)?;
    let folder = ax::folder_name(&job.doc, seq).map_err(io_stop)?;
    set(0.1);
    // 1. The destination, checked here (the main thread does no I/O).
    crate::bounce::outside(&job.parent, &job.roots).map_err(failed)?;
    let dest = free_folder(&job.parent, &folder).map_err(io_stop)?;
    let staging = partial_path(&dest);
    let mut files = vec![aaf_name.clone(), midi_name.clone()];
    if !job.opts.embed {
        files.extend(x.audio.iter().map(|s| s.file.clone()));
    }
    for f in &files {
        crate::bounce::partial_fits(&staging.join(f)).map_err(failed)?;
        crate::bounce::partial_fits(&dest.join(f)).map_err(failed)?;
    }
    // 2. The free space: the staged WAVs, and their copy in the file.
    let midi_bytes = x.midi.as_ref().map_or(0, |m| m.len() as u64);
    let need = staged_bytes(&job.doc, &x, job.opts.depth)
        .saturating_add(midi_bytes)
        .saturating_mul(SPACE_FACTOR);
    let free = job.free.or_else(|| crate::record::free_bytes(&job.parent));
    if free.is_some_and(|f| need > f) {
        return Err(Stop::Failed(not_enough_space(need)));
    }
    let pending = stage_folder(&dest).map_err(io_stop)?;
    // 3. The audio, staged.
    let audio_dir = pending
        .path()
        .join(if job.opts.embed { ESSENCE_DIR } else { "Audio" });
    if !x.audio.is_empty() {
        std::fs::create_dir(&audio_dir)
            .map_err(|e| Stop::Failed(format!("{}: {e}", display_name(&audio_dir))))?;
    }
    // Below the project's depth (a float project always) the WAVs are
    // dithered; at it, nothing is.
    let dither = job.opts.depth.bits() < job.doc.settings().format.depth.bits();
    let n = x.audio.len().max(1) as f32;
    let mut staged = Vec::with_capacity(x.audio.len());
    for (k, s) in x.audio.iter().enumerate() {
        let a = AudioFile {
            entry: s.file.clone(),
            media: s.media,
            gain_db: 0.0,
            name: s.name.clone(),
        };
        let p = stage_audio(
            &job.doc,
            &job.root,
            &audio_dir,
            &a,
            s.channel,
            job.opts.depth,
            dither,
            cancel,
        )?;
        staged.push(p);
        set(0.1 + 0.6 * (k as f32 + 1.0) / n);
        if cancelled() {
            return Err(Stop::Cancelled);
        }
    }
    stage(1);
    if cancelled() {
        return Err(Stop::Cancelled);
    }
    // 4. The MIDI file beside.
    if let Some(bytes) = &x.midi {
        write_partial(&pending.path().join(&midi_name), bytes, false)
            .and_then(|p| p.commit())
            .map_err(io_stop)?;
    }
    stage(2);
    if cancelled() {
        return Err(Stop::Cancelled);
    }
    // 5. The file, its embedded WAVs streamed in.
    let mut essence = |i: usize, w: &mut dyn Write| -> Result<(), IoError> {
        let p = staged
            .get(i)
            .ok_or_else(|| IoError::Io(format!("staged file {i} is missing")))?;
        let why = |e: std::io::Error| IoError::Io(format!("{}: {e}", display_name(p)));
        let mut f = std::fs::File::open(p).map_err(why)?;
        std::io::copy(&mut f, w).map(|_| ()).map_err(why)
    };
    ax::write(&x, &pending.path().join(&aaf_name), &mut essence, cancel).map_err(io_stop)?;
    if job.opts.embed && !x.audio.is_empty() {
        std::fs::remove_dir_all(&audio_dir)
            .map_err(|e| Stop::Failed(format!("{}: {e}", display_name(&audio_dir))))?;
    }
    set(0.95);
    stage(3);
    if cancelled() {
        return Err(Stop::Cancelled);
    }
    // 6. Into place.
    let dest = pending.commit().map_err(io_stop)?;
    Ok((x.report, dest))
}

/// The export job: every step inside `catch_unwind`.
pub(crate) fn export_job(
    job: ExportJob,
) -> impl FnOnce(&AtomicU32, &AtomicBool) -> Work + Send + 'static {
    move |progress, cancel| {
        if cancel.load(Ordering::Relaxed) {
            return Work::Cancelled;
        }
        let ran = catch_unwind(AssertUnwindSafe(|| export_body(&job, progress, cancel)));
        progress.store(1f32.to_bits(), Ordering::Relaxed);
        match ran {
            Ok(Ok((report, dest))) => Work::ExportAaf {
                result: Ok(report),
                written: vec![dest.display().to_string()],
            },
            Ok(Err(Stop::Cancelled)) => Work::Cancelled,
            Ok(Err(Stop::Failed(why))) => Work::ExportAaf {
                result: Err(why),
                written: Vec::new(),
            },
            Err(p) => Work::ExportAaf {
                result: Err(format!("internal error: {}", panic_message(p.as_ref()))),
                written: Vec::new(),
            },
        }
    }
}

// ---- the import job ----

/// Everything the import job needs (all of it owned: it runs on its
/// thread).
pub(crate) struct AafImportJob {
    /// 6d's import fields: the file, the media root, the format, the
    /// reserved names, the hooks (`installed` is empty: an AAF names no
    /// plug-ins). Its `cancel_at` counts 5 opened, 6 first extracted, 7
    /// first converted, 8 joined and split, 9 mapped.
    pub io: ImportJob,
    /// The MIDI file given; `None`: the one beside the AAF.
    pub midi_path: Option<PathBuf>,
    /// Where else to look for audio.
    pub media_folder: Option<PathBuf>,
    /// Join L/R pairs.
    pub join_pairs: bool,
    /// Where it lands.
    pub place: AafPlace,
    /// The sequence as it was when the job started (its tempo map and file
    /// start place the audio).
    pub seq: Sequence,
}

/// A mapped AAF import, applied by the session.
pub struct AafImported {
    /// The plan.
    pub imp: DpImport,
    /// The MIDI file read with it.
    pub midi: Option<MidiImport>,
    /// Where it lands.
    pub place: AafPlace,
    /// The tempo map the audio was placed on.
    pub tempo: core_model::tempo::TempoMap,
    /// The tick composition position 0 was placed on.
    pub start: core_model::time::Tick,
}

impl std::fmt::Debug for AafImported {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AafImported")
            .field("place", &self.place)
            .field("midi", &self.midi.is_some())
            .finish_non_exhaustive()
    }
}

/// Frames read and written per step of a join or split.
const CHUNK_FRAMES: usize = 16_384;

/// One input channel of a CAF a join or split writes.
struct Input<'a> {
    reader: CafReader,
    channels: usize,
    channel: usize,
    item: &'a Imported,
}

/// Writes a new CAF in `<root>/Media/` named `<stem>.caf` (or `<stem>
/// N.caf`, the first free name not in `reserved`) at `format`, one channel
/// per input, and gives its record. Nothing is left on any error.
fn write_caf(
    root: &Path,
    inputs: &[Input<'_>],
    stem: &str,
    format: AudioFormat,
    reserved: &BTreeSet<String>,
    cancel: &AtomicBool,
) -> Result<Imported, Stop> {
    let media = root.join(MEDIA_DIR);
    let reserved: BTreeSet<String> = reserved.iter().map(|r| r.to_lowercase()).collect();
    let name = (1..=10_000u32)
        .map(|n| {
            if n == 1 {
                format!("{stem}.caf")
            } else {
                format!("{stem} {n}.caf")
            }
        })
        .find(|name| {
            !reserved.contains(&format!("{MEDIA_DIR}/{name}").to_lowercase())
                && std::fs::symlink_metadata(media.join(name)).is_err()
        })
        .ok_or_else(|| Stop::Failed(format!("no free name for {stem}")))?;
    let path = media.join(&name);
    let channels = u16::try_from(inputs.len()).unwrap_or(u16::MAX);
    let why = |e: core_media::MediaError| Stop::Failed(format!("{stem}: {e}"));
    let mut w = CafWriter::create(
        &path,
        format.sample_rate,
        channels,
        format.depth,
        WriteMode::Normal,
    )
    .map_err(why)?;
    // The inputs are at this depth already.
    w.set_dither(false);
    let written = (|| {
        let mut bufs: Vec<Vec<f32>> = inputs
            .iter()
            .map(|i| vec![0f32; CHUNK_FRAMES.saturating_mul(i.channels)])
            .collect();
        let mut out = Vec::with_capacity(CHUNK_FRAMES.saturating_mul(inputs.len()));
        let mut scratch = Vec::new();
        let mut at = 0u64;
        loop {
            if cancel.load(Ordering::Relaxed) {
                return Err(Stop::Cancelled);
            }
            let mut n = usize::MAX;
            for (i, b) in inputs.iter().zip(bufs.iter_mut()) {
                let got = i.reader.read(at, b, &mut scratch).map_err(why)?;
                n = n.min(got);
            }
            if n == 0 || n == usize::MAX {
                break;
            }
            out.clear();
            for f in 0..n {
                for (i, b) in inputs.iter().zip(&bufs) {
                    let s = f
                        .checked_mul(i.channels)
                        .and_then(|x| x.checked_add(i.channel))
                        .and_then(|x| b.get(x))
                        .copied()
                        .unwrap_or(0.0);
                    out.push(s);
                }
            }
            w.write(&out).map_err(why)?;
            at = at.saturating_add(n as u64);
        }
        w.finish().map_err(why)
    })();
    let written = match written {
        Ok(x) => x,
        Err(e) => {
            let _ = std::fs::remove_file(&path);
            return Err(e);
        }
    };
    let original_name = inputs
        .first()
        .map(|i| i.item.original_name.clone())
        .unwrap_or_default();
    Ok(Imported {
        path: format!("{MEDIA_DIR}/{name}"),
        name: name.trim_end_matches(".caf").to_owned(),
        original_name,
        channels,
        sample_rate: format.sample_rate,
        length_frames: written.frames,
        content_hash: written.content_hash,
        clipped: written.clipped,
    })
}

/// `name` without a left/right suffix (Ruling 7's pairs), else as it is.
fn pair_stem(name: &str) -> String {
    let lower = name.to_lowercase();
    for suffix in [" left", " l", ".l"] {
        if let Some(stem) = lower.strip_suffix(suffix)
            && !stem.trim().is_empty()
            && let Some(s) = name.get(..stem.len())
        {
            return s.trim_end().to_owned();
        }
    }
    name.to_owned()
}

/// The one online item of media `key`, when it has exactly one.
fn single(media: &MediaMap, key: usize) -> Option<Imported> {
    match media.get(&key) {
        Some(MediaSource::Online(items)) => match items.as_slice() {
            [one] => Some(one.clone()),
            _ => None,
        },
        _ => None,
    }
}

/// Takes `item`'s file out of `Media/` (with the overview its conversion
/// built, a cache) and out of the job's list.
fn drop_item(root: &Path, files: &mut ImportFiles, item: &Imported) {
    let _ = std::fs::remove_file(root.join(&item.path));
    if let Some(at) = core_media::overview::Overview::path(root, &item.content_hash) {
        let _ = std::fs::remove_file(at);
    }
    for (_, r) in files.iter_mut() {
        if let Ok(items) = r {
            items.retain(|i| i.path != item.path);
        }
    }
}

/// Opens `item`'s CAF in `root`.
fn open_item(root: &Path, item: &Imported) -> Result<CafReader, Stop> {
    CafReader::open(&root.join(&item.path)).map_err(|e| Stop::Failed(format!("{}: {e}", item.name)))
}

/// Joins each pair's two converted monos into one stereo CAF in `Media/`
/// (Ruling 7): `media[left]` becomes it, `media[right]` goes, the monos
/// are removed and `files` is updated.
fn join_pairs(
    job: &AafImportJob,
    joined: &[Pair],
    media: &mut MediaMap,
    files: &mut ImportFiles,
    cancel: &AtomicBool,
) -> Result<(), Stop> {
    let root = &job.io.root;
    for &(l, r) in joined.iter().flat_map(|p| &p.media) {
        let (Some(a), Some(b)) = (single(media, l), single(media, r)) else {
            continue;
        };
        let inputs = [
            Input {
                reader: open_item(root, &a)?,
                channels: 1,
                channel: 0,
                item: &a,
            },
            Input {
                reader: open_item(root, &b)?,
                channels: 1,
                channel: 0,
                item: &b,
            },
        ];
        let made = write_caf(
            root,
            &inputs,
            &pair_stem(&a.name),
            job.io.format,
            &job.io.reserved,
            cancel,
        )?;
        files.push((made.name.clone(), Ok(vec![made.clone()])));
        drop_item(root, files, &a);
        drop_item(root, files, &b);
        media.insert(l, MediaSource::Online(vec![made]));
        media.remove(&r);
    }
    Ok(())
}

/// Makes a mono CAF of each channel a clip plays alone of a stereo file
/// (Ruling 7; wider files already arrive as mono items): each becomes a new
/// audio ref and media key, which those clips play. A stereo file no clip
/// plays whole any more is removed.
fn split_channels(
    job: &AafImportJob,
    comp: &mut Composition,
    audio: &mut Vec<AudioRef>,
    media: &mut MediaMap,
    files: &mut ImportFiles,
    cancel: &AtomicBool,
) -> Result<(), Stop> {
    let root = &job.io.root;
    let stereo = |media: &MediaMap, key: usize| single(media, key).filter(|i| i.channels == 2);
    let mut next = audio
        .iter()
        .map(|a| a.key)
        .chain(media.keys().copied())
        .max()
        .map_or(0, |k| k.saturating_add(1));
    let mut wanted: BTreeMap<(usize, u16), usize> = BTreeMap::new();
    let mut whole: BTreeSet<usize> = BTreeSet::new();
    for c in comp.slots.iter().flat_map(|s| &s.clips) {
        match c.channel {
            Some(k) if k < 2 && stereo(media, c.media).is_some() => {
                wanted.entry((c.media, k)).or_insert_with(|| {
                    let n = next;
                    next = next.saturating_add(1);
                    n
                });
            }
            None => {
                whole.insert(c.media);
            }
            Some(_) => {}
        }
    }
    let mut split: BTreeSet<usize> = BTreeSet::new();
    for (&(key, k), &new) in &wanted {
        let Some(item) = stereo(media, key) else {
            continue;
        };
        let inputs = [Input {
            reader: open_item(root, &item)?,
            channels: 2,
            channel: usize::from(k),
            item: &item,
        }];
        let stem = format!("{} ch{}", item.name, u32::from(k) + 1);
        let made = write_caf(
            root,
            &inputs,
            &stem,
            job.io.format,
            &job.io.reserved,
            cancel,
        )?;
        files.push((made.name.clone(), Ok(vec![made.clone()])));
        media.insert(new, MediaSource::Online(vec![made]));
        if let Some(a) = audio.iter().find(|a| a.key == key).cloned() {
            audio.push(AudioRef {
                key: new,
                channels: 1,
                ..a
            });
        }
        split.insert(key);
    }
    for c in comp.slots.iter_mut().flat_map(|s| &mut s.clips) {
        if let Some(k) = c.channel
            && let Some(&new) = wanted.get(&(c.media, k))
            && media.contains_key(&new)
        {
            c.media = new;
            c.channel = None;
        }
    }
    for key in split {
        if whole.contains(&key) {
            continue;
        }
        if let Some(item) = single(media, key) {
            drop_item(root, files, &item);
        }
        media.remove(&key);
    }
    Ok(())
}

/// The MIDI file read with the AAF (Ruling 8): the one given (a failure
/// fails the job), else the one beside it (a failure is Left out, with
/// [`midi_not_read`]).
fn read_midi(
    job: &AafImportJob,
    report: &mut Report,
    cancel: &AtomicBool,
) -> Result<Option<MidiImport>, Stop> {
    use core_io::midi_in::{ImportOptions, parse_file};
    let opts = ImportOptions {
        split_channels: None,
        mpe: true,
    };
    let limits = core_io::limits::Limits::default();
    match &job.midi_path {
        Some(p) => match parse_file(p, &opts, &limits, cancel) {
            Ok(m) => Ok(Some(m)),
            Err(IoError::Cancelled) => Err(Stop::Cancelled),
            Err(e) => Err(Stop::Failed(midi_file_not_read(
                &display_name(p),
                &e.to_string(),
            ))),
        },
        None => match midi_beside(&job.io.path) {
            None => Ok(None),
            Some(p) => match parse_file(&p, &opts, &limits, cancel) {
                Ok(m) => Ok(Some(m)),
                Err(IoError::Cancelled) => Err(Stop::Cancelled),
                Err(e) => {
                    report.add(ItemKind::LeftOut, &midi_not_read(&e.to_string()), None);
                    Ok(None)
                }
            },
        },
    }
}

/// The import job's body: the mapped import, or how it stopped. Media it
/// makes goes into `files` as it is made, so the caller can remove it
/// whatever happens.
fn import_body(
    job: &AafImportJob,
    importer: &dyn core_media::import::MediaImporter,
    files: &mut ImportFiles,
    progress: &AtomicU32,
    cancel: &AtomicBool,
) -> Result<Box<AafImported>, Stop> {
    let stage = |n: u32| {
        if job.io.cancel_at == Some(n) {
            cancel.store(true, Ordering::Relaxed);
        }
    };
    let cancelled = || cancel.load(Ordering::Relaxed);
    // 1. The media folder (lstat), then the file.
    if let Some(dir) = &job.media_folder
        && !std::fs::symlink_metadata(dir).is_ok_and(|m| m.is_dir())
    {
        return Err(Stop::Failed(not_a_folder(&display_name(dir))));
    }
    let file = AafFile::open(
        &job.io.path,
        job.media_folder.as_deref(),
        &AafLimits::default(),
        cancel,
    )
    .map_err(io_stop)?;
    stage(5);
    if cancelled() {
        return Err(Stop::Cancelled);
    }
    progress.store(0.1f32.to_bits(), Ordering::Relaxed);
    // 2. The free space, extraction and conversion (stages 6, 7).
    let pkg: &dyn AudioPackage = &file;
    let mut media = read_and_convert(&job.io, pkg, importer, files, progress, cancel)?;
    let mut comp = file.composition().clone();
    let mut audio = pkg.audio().to_vec();
    // 3–5. The pairs joined, the channels split.
    let joined = pairs(&comp, &media, job.join_pairs);
    join_pairs(job, &joined, &mut media, files, cancel)?;
    split_channels(job, &mut comp, &mut audio, &mut media, files, cancel)?;
    stage(8);
    if cancelled() {
        return Err(Stop::Cancelled);
    }
    // 6. The MIDI file beside.
    let mut extra = Report::default();
    let midi = read_midi(job, &mut extra, cancel)?;
    // 7. The plan.
    let (tempo, start) = tempo_and_start(midi.as_ref(), &job.seq, job.place.new_project)
        .map_err(|e| Stop::Failed(e.to_string()))?;
    let env = AafEnv {
        rate: job.io.format.sample_rate,
        tempo: &tempo,
        start,
        join_pairs: job.join_pairs,
        audio: &audio,
    };
    let mut imp = map(&comp, &media, &joined, &env, cancel).map_err(io_stop)?;
    imp.report.merge(extra);
    stage(9);
    if cancelled() {
        return Err(Stop::Cancelled);
    }
    progress.store(0.95f32.to_bits(), Ordering::Relaxed);
    Ok(Box::new(AafImported {
        imp,
        midi,
        place: job.place,
        tempo,
        start,
    }))
}

/// The import job: every step inside `catch_unwind`. Whatever it made is
/// returned with the result, so the session removes it when the import is
/// not applied.
pub(crate) fn import_job(
    job: AafImportJob,
) -> impl FnOnce(&AtomicU32, &AtomicBool) -> Work + Send + 'static {
    move |progress, cancel| {
        let mut files = ImportFiles::new();
        if cancel.load(Ordering::Relaxed) {
            return Work::ImportAaf {
                files,
                result: Err(crate::for_daw::CANCELLED.to_owned()),
            };
        }
        let importer = crate::media::importer();
        let ran = catch_unwind(AssertUnwindSafe(|| {
            import_body(&job, importer.as_ref(), &mut files, progress, cancel)
        }));
        progress.store(1f32.to_bits(), Ordering::Relaxed);
        let result = match ran {
            Ok(Ok(imp)) => Ok(imp),
            Ok(Err(Stop::Cancelled)) => Err(crate::for_daw::CANCELLED.to_owned()),
            Ok(Err(Stop::Failed(why))) => Err(why),
            Err(p) => Err(format!("internal error: {}", panic_message(p.as_ref()))),
        };
        Work::ImportAaf { files, result }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pair_stems_drop_the_side() {
        assert_eq!(pair_stem("Gtr L"), "Gtr");
        assert_eq!(pair_stem("Gtr.l"), "Gtr");
        assert_eq!(pair_stem("Gtr Left"), "Gtr");
        assert_eq!(pair_stem("L"), "L");
        assert_eq!(pair_stem("Bell"), "Bell");
    }

    #[test]
    fn aaf_paths_need_the_extension() {
        assert!(aaf_path("/a/b.AAF").is_ok());
        assert!(
            matches!(aaf_path("/a/b.aif"), Err(VizError::Model { message }) if message == NOT_AAF_NAME)
        );
        assert!(aaf_path("b.aaf").is_err());
        assert!(aaf_path("").is_err());
    }
}
