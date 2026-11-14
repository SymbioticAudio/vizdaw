//! DAWproject export for the app (plan 6d Task 9; spec §6, §7.8, §11): the
//! request rows and checks (every refusal a `Model` error, nothing
//! written), the job that stages the WAV copies, writes the package as a
//! partial, renames it into place and keeps the snapshot, the plug-in lines
//! only the session can know, the AU property-list codec and the snapshots
//! a Save keeps (Ruling 16); and the import (Task 10): the info read, the
//! job that extracts and converts the audio and maps the file, applied by
//! the session as a new project.

use crate::blocks::BlockRule;
use crate::error::VizError;
use crate::guard::panic_message;
use crate::interchange::{file_dest, io_error};
use crate::jobs::Work;
use core_io::IoError;
use core_io::dawproject::export::{
    self as dp, AudioFile, ClipRule, DpExport, ExportEnv, ExportOptions, LeftOutAudio, RegionGain,
};
use core_io::dawproject::plugins::PlistCodec;
use core_io::dawproject::record::{KEEP_SNAPSHOTS, SNAPSHOT_DIR};
use core_io::report::{ItemKind, Report};
use core_io::write::{PendingFolder, create_partial, stage_folder, write_partial};
use core_io::zip::{EntryMethod, ZipWriter};
use core_media::caf::CafReader;
use core_media::export::{Container, ExportFormat, PcmWriter, display_name, partial_path};
use core_model::document::{DestKind, Document};
use core_model::format::BitDepth;
use core_model::ids::{MediaId, PluginInstanceId, SeqId};
use core_model::time::Tick;
use std::collections::{BTreeMap, BTreeSet};
use std::io::{BufWriter, Write};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};

// ---- rows ----

/// What happens to a region's gain (plan 6d Ruling 10).
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum RegionGainRow {
    /// Each region with gain gets its own audio copy, with the gain applied.
    Apply,
    /// Regions share their media's copy; the gain is listed as left out.
    LeaveOut,
}

/// Export a sequence as a DAWproject file.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct DawprojectExportRequestRow {
    /// The sequence.
    pub seq: u64,
    /// The file (absolute, ending in `.dawproject`; ignored by the check).
    pub path: String,
    /// Replace an existing file (ignored by the check).
    pub replace: bool,
    /// What happens to region gain.
    pub region_gain: RegionGainRow,
    /// How a track's notes are grouped into clips.
    pub blocks: BlockRule,
}

// ---- wording ----

/// The refusal for a destination with another extension.
pub const NOT_DAWPROJECT_NAME: &str = "Choose a name ending in .dawproject.";
/// Left out: a media copy whose WAV would pass 4 GiB (S9), by name; its
/// clips are written with no audio (see [`left_out_audio`]).
pub const TOO_LONG_FOR_WAV: &str = "Audio too long for a WAV file";
/// Left out: offline media, by name; its clips are written with no audio
/// (see [`left_out_audio`]).
pub const OFFLINE_AUDIO: &str = "Offline audio";

/// The free-space failure: the job needs `bytes`.
pub fn not_enough_space(bytes: u64) -> String {
    format!(
        "Not enough free space to export (it needs {} MB).",
        bytes.div_ceil(1_000_000)
    )
}

/// Note: the snapshot could not be written; the export is still Done.
pub fn snapshot_not_kept(why: &str) -> String {
    format!("The merge record could not be kept in the project: {why}")
}

/// Note: a running plug-in did not give its current settings; the saved
/// ones were written.
pub fn not_captured(name: &str) -> String {
    format!("Plug-in '{name}' did not give its current settings; its saved settings were written")
}

/// The capture error the `debug_dawproject_capture_fails` test hook gives.
#[cfg(feature = "test-hooks")]
pub const CAPTURE_FAILS_HOOK: &str = "the test hook failed this capture";

/// The detail of a Left out settings line for a slot with nothing stored.
const NOTHING_STORED: &str = "it is not running here and has no saved settings";

// ---- the codec ----

/// The AU property-list codec through `platform_macos::plist`.
#[cfg(target_os = "macos")]
pub(crate) struct MacPlist;

#[cfg(target_os = "macos")]
impl PlistCodec for MacPlist {
    fn to_xml(&self, binary: &[u8]) -> Result<Vec<u8>, String> {
        platform_macos::plist::to_xml(binary)
    }

    fn to_binary(&self, plist: &[u8]) -> Result<Vec<u8>, String> {
        platform_macos::plist::to_binary(plist)
    }
}

/// The platform's codec: [`MacPlist`] on macOS, `NoPlist` elsewhere.
pub(crate) fn plist() -> &'static dyn PlistCodec {
    #[cfg(target_os = "macos")]
    {
        &MacPlist
    }
    #[cfg(not(target_os = "macos"))]
    {
        &core_io::dawproject::plugins::NoPlist
    }
}

// ---- request checks ----

fn model(message: impl Into<String>) -> VizError {
    VizError::Model {
        message: message.into(),
    }
}

/// The longest file name most file systems take, in bytes.
const MAX_NAME_BYTES: usize = 255;
/// The longest path the export writes, in bytes (as `bounce` allows).
const MAX_PATH_BYTES: usize = 4096;

/// The export options of `req` in `doc` (its path is not looked at), with
/// nothing left out yet: the caller sets `left_out` with
/// [`left_out_audio`] once the media is refreshed.
pub(crate) fn options(
    doc: &Document,
    req: &DawprojectExportRequestRow,
) -> Result<ExportOptions, VizError> {
    let seq = SeqId(req.seq);
    doc.sequence(seq)
        .map_err(|_| model(format!("sequence {} is not in this project", req.seq)))?;
    Ok(ExportOptions {
        seq,
        region_gain: match req.region_gain {
            RegionGainRow::Apply => RegionGain::Apply,
            RegionGainRow::LeaveOut => RegionGain::LeaveOut,
        },
        clips: clip_rule(req.blocks),
        left_out: LeftOutAudio::new(),
    })
}

/// The clip rule of a block rule (minimums of 1).
pub(crate) fn clip_rule(b: BlockRule) -> ClipRule {
    match b {
        BlockRule::SilenceGap { ticks } => ClipRule::SilenceGap(Tick(ticks.max(1))),
        BlockRule::FixedBars { bars } => ClipRule::FixedBars(bars.max(1)),
        BlockRule::WholeLane => ClipRule::WholeTrack,
    }
}

/// The audio staging folder's destination: `<dest>.audio` (its staging
/// folder is `.<name>.audio.partial`).
fn audio_dest(dest: &Path) -> PathBuf {
    let mut name = dest.file_name().unwrap_or_default().to_os_string();
    name.push(".audio");
    dest.with_file_name(name)
}

/// The file the export writes, checked as `file_dest` checks it, ending in
/// `.dawproject`, never over something that is not a file (or a link to
/// one, or a dangling link), and
/// with the longest name the job makes (`.<name>.audio.partial`, S6)
/// short enough.
pub(crate) fn dest(text: &str, replace: bool, roots: &[PathBuf]) -> Result<PathBuf, VizError> {
    let p = file_dest(text, replace, roots)?;
    if !p
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("dawproject"))
    {
        return Err(model(NOT_DAWPROJECT_NAME));
    }
    if let Ok(m) = std::fs::symlink_metadata(&p) {
        // A link is replaced itself, never followed; one to something
        // other than a file (or nothing), or a loop, is refused.
        let plain = m.is_file()
            || (m.file_type().is_symlink()
                && match std::fs::metadata(&p) {
                    Ok(t) => t.is_file(),
                    Err(e) => e.kind() == std::io::ErrorKind::NotFound,
                });
        if !plain {
            return Err(model(format!(
                "{} is not a file; choose another name",
                display_name(&p)
            )));
        }
    }
    let staging = partial_path(&audio_dest(&p));
    let long = staging
        .file_name()
        .is_some_and(|n| n.len() > MAX_NAME_BYTES)
        || staging.as_os_str().len() > MAX_PATH_BYTES;
    if long {
        return Err(model(format!(
            "{}: the file name is too long for the files the export makes beside it; choose a shorter one",
            display_name(&p)
        )));
    }
    if std::fs::symlink_metadata(&staging).is_ok() {
        return Err(model(format!(
            "{} is left from an earlier export; remove it or choose another name",
            display_name(&staging)
        )));
    }
    Ok(p)
}

// ---- the plug-in lines ----

/// Why a slot was not captured for an export: `None` when it is not
/// running here, else the capture's error.
pub(crate) type Uncaptured = BTreeMap<PluginInstanceId, Option<String>>;

/// Every plug-in slot of `doc`: the plug-in instruments', then the
/// strips'.
pub(crate) fn all_slots(doc: &Document) -> Vec<PluginInstanceId> {
    let mut out: Vec<PluginInstanceId> = doc
        .instruments()
        .values()
        .filter_map(|d| match &d.kind {
            DestKind::PluginInstrument { slot } => Some(slot.id),
            DestKind::MidiEndpoint { .. } => None,
        })
        .collect();
    out.extend(doc.mixer().slots().map(|(_, s)| s.id));
    out
}

/// Takes one count of the `kind` item `what` out of `r`.
fn take_one(r: &mut Report, kind: ItemKind, what: &str) -> bool {
    let Some(i) = r
        .items
        .iter()
        .position(|i| i.kind == kind && i.what == what)
    else {
        return false;
    };
    let Some(item) = r.items.get_mut(i) else {
        return false;
    };
    item.count = item.count.saturating_sub(1);
    if item.count == 0 {
        r.items.remove(i);
    }
    true
}

/// The lines only the session knows (spec §11; plan 6d Task 4's
/// `not_loaded`) for the `exported` slots (the export's device slots): an
/// exported slot that was not captured writes its saved settings with a
/// Note; with none saved, its settings are Left out. A slot in
/// `will_capture` (a check on a thread without the plug-in host: the job
/// captures it) has no "no saved settings" line.
pub(crate) fn plugin_lines(
    doc: &Document,
    exported: &BTreeSet<PluginInstanceId>,
    uncaptured: &Uncaptured,
    will_capture: &BTreeSet<PluginInstanceId>,
    report: &mut Report,
) {
    for id in will_capture.intersection(exported) {
        if let Some(slot) = doc.plugin_slot(*id)
            && slot.state.is_none()
        {
            take_one(
                report,
                ItemKind::Note,
                &dp::no_saved_settings(&slot.plugin.name),
            );
        }
    }
    for (id, why) in uncaptured {
        if !exported.contains(id) {
            continue;
        }
        let Some(slot) = doc.plugin_slot(*id) else {
            continue;
        };
        let name = &slot.plugin.name;
        let stored = slot.state.and_then(|b| doc.state_blob(b)).is_some();
        if stored {
            // Its saved settings were written, unless the conversion
            // already left them out.
            let left_out = report
                .items
                .iter()
                .any(|i| i.kind == ItemKind::LeftOut && i.what == dp::plugin_settings(name));
            if !left_out {
                let line = match why {
                    None => dp::not_loaded(name),
                    Some(_) => not_captured(name),
                };
                report.add(ItemKind::Note, &line, why.clone());
            }
        } else {
            take_one(report, ItemKind::Note, &dp::no_saved_settings(name));
            let detail = why.clone().unwrap_or_else(|| NOTHING_STORED.to_owned());
            report.add(ItemKind::LeftOut, &dp::plugin_settings(name), Some(detail));
        }
    }
}

/// What the export job would report for `opts` (nothing is written). The
/// left-out audio is `opts.left_out`, as in the job, so offline and
/// over-long media get the job's lines.
pub(crate) fn check(
    doc: &Document,
    opts: &ExportOptions,
    uncaptured: &Uncaptured,
    will_capture: &BTreeSet<PluginInstanceId>,
) -> Result<Report, VizError> {
    let cancel = AtomicBool::new(false);
    let dp::DpCheck { mut report, slots } =
        dp::check(doc, opts, plist(), &cancel).map_err(io_error)?;
    plugin_lines(doc, &slots, uncaptured, will_capture, &mut report);
    Ok(report)
}

// ---- the job ----

/// Everything the job needs (all of it owned: it runs on its thread).
pub(crate) struct ExportJob {
    /// The document as it was when the job started (captured, stamped).
    pub doc: Document,
    /// What to export.
    pub opts: ExportOptions,
    /// The file.
    pub dest: PathBuf,
    /// Replace an existing file.
    pub replace: bool,
    /// The media root: the CAFs are read from it, the snapshot goes in its
    /// `Interchange/`.
    pub root: PathBuf,
    /// The project's ID.
    pub project_id: String,
    /// This export's ID.
    pub export_id: String,
    /// When, in Unix seconds.
    pub exported_at: u64,
    /// The slots that were not captured.
    pub uncaptured: Uncaptured,
    /// The stage after which it cancels itself (test hook).
    pub cancel_at: Option<u32>,
    /// The free bytes to assume (test hook; `None`: the disk's).
    pub free: Option<u64>,
}

/// How a step of a job ended early (the DAWproject and AAF jobs).
pub(crate) enum Stop {
    /// It was cancelled.
    Cancelled,
    /// It failed: the sentence says why.
    Failed(String),
}

impl From<IoError> for Stop {
    fn from(e: IoError) -> Stop {
        match e {
            IoError::Cancelled => Stop::Cancelled,
            other => Stop::Failed(other.to_string()),
        }
    }
}

/// The free-space check's multiple: the staged copies and the package
/// (or the AAF file).
pub(crate) const SPACE_FACTOR: u64 = 2;
/// Frames read and written per step.
const CHUNK_FRAMES: usize = 16_384;

/// The WAV header's bytes VizDAW writes for `depth` (with `fact` for float).
fn wav_header(depth: BitDepth) -> u64 {
    if depth == BitDepth::Float32 { 56 } else { 44 }
}

/// The sample bytes of `frames` × `channels` at `depth`, saturating.
fn data_bytes(frames: u64, channels: u16, depth: BitDepth) -> u64 {
    frames
        .saturating_mul(u64::from(channels))
        .saturating_mul(depth.bytes() as u64)
}

/// Whether a WAV of that many sample bytes passes the 32-bit sizes.
fn too_long(data: u64, depth: BitDepth) -> bool {
    let max = u64::from(u32::MAX).saturating_sub(wav_header(depth).saturating_sub(8) + 1);
    data > max
}

/// The media an export writes no audio for, with its Left out line (the
/// one classification the check and the job share): media whose WAV at
/// the project depth would pass 4 GiB ([`TOO_LONG_FOR_WAV`], S9), offline
/// or not (relinking would not make it fit), then offline media
/// ([`OFFLINE_AUDIO`]). core-io writes their clips with no content, so
/// `project.xml` names no entry the package lacks.
pub(crate) fn left_out_audio(doc: &Document, offline: &BTreeSet<MediaId>) -> LeftOutAudio {
    let depth = doc.settings().format.depth;
    doc.media()
        .iter()
        .filter_map(|(id, m)| {
            let what = if too_long(data_bytes(m.length_frames, m.channels, depth), depth) {
                TOO_LONG_FOR_WAV
            } else if offline.contains(id) {
                OFFLINE_AUDIO
            } else {
                return None;
            };
            Some((*id, what.to_owned()))
        })
        .collect()
}

/// The bytes the job needs: the WAVs at the project depth plus the XML
/// and the other entries, twice (staged, then in the package).
fn needed_bytes(doc: &Document, x: &DpExport, audio: &[AudioFile]) -> u64 {
    let depth = doc.settings().format.depth;
    let wavs = audio.iter().fold(0u64, |n, a| {
        let one = doc.media().get(&a.media).map_or(0, |m| {
            data_bytes(m.length_frames, m.channels, depth).saturating_add(wav_header(depth))
        });
        n.saturating_add(one)
    });
    let text = [&x.project_xml, &x.metadata_xml, &x.record_json]
        .iter()
        .fold(0u64, |n, s| n.saturating_add(s.len() as u64));
    let states = x
        .states
        .iter()
        .fold(0u64, |n, (_, b)| n.saturating_add(b.len() as u64));
    wavs.saturating_add(text)
        .saturating_add(states)
        .saturating_mul(SPACE_FACTOR)
}

/// The file name of an audio entry (`audio/001.wav` → `001.wav`).
fn entry_file(entry: &str) -> &str {
    entry.rsplit('/').next().unwrap_or(entry)
}

/// Writes `a` from its media item's CAF into `folder` as a WAV at `depth`
/// and the media rate, its gain applied (`10^(g/20)`): every channel, or
/// only `channel` (from 0) as a mono file. `dither` adds TPDF dither at an
/// integer depth (stream `dither_stream`).
#[allow(clippy::too_many_arguments)]
pub(crate) fn stage_audio(
    doc: &Document,
    root: &Path,
    folder: &Path,
    a: &AudioFile,
    channel: Option<u16>,
    depth: BitDepth,
    dither: bool,
    cancel: &AtomicBool,
) -> Result<PathBuf, Stop> {
    let m = doc
        .media()
        .get(&a.media)
        .ok_or_else(|| Stop::Failed(format!("{}: the audio is not in the project", a.name)))?;
    let failed = |e: core_media::MediaError| Stop::Failed(format!("{}: {e}", a.name));
    let src = CafReader::open(&root.join(&m.path)).map_err(failed)?;
    let channels = src.info().channels;
    if channel.is_some_and(|k| k >= channels) {
        return Err(Stop::Failed(format!(
            "{}: the audio has no channel {}",
            a.name,
            channel.map_or(0, |k| u32::from(k) + 1)
        )));
    }
    let fmt = ExportFormat {
        container: Container::Wav,
        depth,
        rate: m.sample_rate,
        dither,
        dither_stream: u64::from(channel.unwrap_or(0)),
    };
    let path = folder.join(entry_file(&a.entry));
    let out_channels = if channel.is_some() { 1 } else { channels };
    let mut w = PcmWriter::create(&path, &fmt, out_channels).map_err(failed)?;
    let gain = 10f32.powf(a.gain_db / 20.0);
    let gain = if gain.is_finite() { gain } else { 1.0 };
    let mut buf = vec![0f32; CHUNK_FRAMES.saturating_mul(usize::from(channels))];
    let mut one = Vec::new();
    let mut scratch = Vec::new();
    let mut at = 0u64;
    loop {
        if cancel.load(Ordering::Relaxed) {
            return Err(Stop::Cancelled);
        }
        let n = src.read(at, &mut buf, &mut scratch).map_err(failed)?;
        if n == 0 {
            break;
        }
        let samples = buf
            .get_mut(..n.saturating_mul(usize::from(channels)))
            .unwrap_or_default();
        if gain != 1.0 {
            for s in samples.iter_mut() {
                *s *= gain;
            }
        }
        match channel {
            Some(k) => {
                one.clear();
                one.extend(
                    samples
                        .chunks_exact(usize::from(channels))
                        .filter_map(|f| f.get(usize::from(k)).copied()),
                );
                w.write(&one).map_err(failed)?;
            }
            None => w.write(samples).map_err(failed)?,
        }
        at = at.saturating_add(n as u64);
    }
    w.finish().map_err(failed)?;
    Ok(path)
}

/// Writes the package's partial: the XML and the record (Deflated), the
/// staged audio (Stored) and the plug-in states (Deflated), then syncs it.
fn write_package(
    x: &DpExport,
    staged: &[(String, PathBuf)],
    dest: &Path,
    replace: bool,
    stage: &dyn Fn(u32),
    cancel: &AtomicBool,
) -> Result<core_io::write::PendingFile, Stop> {
    use core_io::dawproject::package::{METADATA_ENTRY, PROJECT_ENTRY};
    use core_io::dawproject::record::RECORD_ENTRY;
    let (pending, file) = create_partial(dest, replace)?;
    let mut zip = ZipWriter::new(BufWriter::new(file));
    zip.add(
        METADATA_ENTRY,
        EntryMethod::Deflated,
        x.metadata_xml.as_bytes(),
    )?;
    zip.add(
        PROJECT_ENTRY,
        EntryMethod::Deflated,
        x.project_xml.as_bytes(),
    )?;
    stage(3);
    if cancel.load(Ordering::Relaxed) {
        return Err(Stop::Cancelled);
    }
    zip.add(
        RECORD_ENTRY,
        EntryMethod::Deflated,
        x.record_json.as_bytes(),
    )?;
    for (entry, path) in staged {
        let mut f = std::fs::File::open(path)
            .map_err(|e| Stop::Failed(format!("{}: {e}", display_name(path))))?;
        zip.add_from(entry, EntryMethod::Stored, &mut f, cancel)?;
    }
    for (entry, bytes) in &x.states {
        if cancel.load(Ordering::Relaxed) {
            return Err(Stop::Cancelled);
        }
        zip.add(entry, EntryMethod::Deflated, bytes)?;
    }
    let out = zip.finish()?;
    let file = out
        .into_inner()
        .map_err(|e| Stop::Failed(format!("{}: {}", display_name(dest), e.error())))?;
    file.sync_all()
        .map_err(|e| Stop::Failed(format!("{}: {e}", display_name(dest))))?;
    drop(file);
    stage(4);
    if cancel.load(Ordering::Relaxed) {
        return Err(Stop::Cancelled);
    }
    Ok(pending)
}

/// Makes `dir` when it is missing; refuses a link or a file there (lstat).
fn real_dir(dir: &Path) -> Result<(), String> {
    match std::fs::symlink_metadata(dir) {
        Ok(m) if m.is_dir() => Ok(()),
        Ok(_) => Err(format!("{} is not a folder", display_name(dir))),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", display_name(dir)))?;
            // Made now, or raced into place: a real folder either way.
            match std::fs::symlink_metadata(dir) {
                Ok(m) if m.is_dir() => Ok(()),
                _ => Err(format!("{} is not a folder", display_name(dir))),
            }
        }
        Err(e) => Err(format!("{}: {e}", display_name(dir))),
    }
}

/// Writes the snapshot `<root>/Interchange/<export id>.json` (never over
/// anything; L6).
fn keep_snapshot(root: &Path, export_id: &str, json: &str) -> Result<(), String> {
    let dir = root.join(SNAPSHOT_DIR);
    real_dir(&dir)?;
    write_partial(
        &dir.join(format!("{export_id}.json")),
        json.as_bytes(),
        false,
    )
    .and_then(|p| p.commit())
    .map(|_| ())
    .map_err(|e| e.to_string())
}

/// The job's body: the report, or how it stopped (`Stop::Cancelled` when
/// cancelled).
fn body(job: &ExportJob, progress: &AtomicU32, cancel: &AtomicBool) -> Result<Report, Stop> {
    let stage = |n: u32| {
        if job.cancel_at == Some(n) {
            cancel.store(true, Ordering::Relaxed);
        }
    };
    let cancelled = || cancel.load(Ordering::Relaxed);
    // 1. The files.
    let env = ExportEnv {
        plist: plist(),
        app_version: env!("CARGO_PKG_VERSION"),
        project_id: &job.project_id,
        export_id: &job.export_id,
        exported_at: job.exported_at,
    };
    let mut x = dp::export(&job.doc, &job.opts, &env, cancel)?;
    plugin_lines(
        &job.doc,
        &x.slots,
        &job.uncaptured,
        &BTreeSet::new(),
        &mut x.report,
    );
    stage(1);
    if cancelled() {
        return Err(Stop::Cancelled);
    }
    progress.store(0.2f32.to_bits(), Ordering::Relaxed);
    // 2. The free space.
    let audio = std::mem::take(&mut x.audio);
    let need = needed_bytes(&job.doc, &x, &audio);
    let folder = job.dest.parent().unwrap_or(&job.dest);
    let free = job.free.or_else(|| crate::record::free_bytes(folder));
    if free.is_some_and(|f| need > f) {
        return Err(Stop::Failed(not_enough_space(need)));
    }
    // 3. The audio, staged.
    let mut staged = Vec::with_capacity(audio.len());
    let mut folder_guard: Option<PendingFolder> = None;
    if !audio.is_empty() {
        let pending = stage_folder(&audio_dest(&job.dest))?;
        for (k, a) in audio.iter().enumerate() {
            let p = stage_audio(
                &job.doc,
                &job.root,
                pending.path(),
                a,
                None,
                job.doc.settings().format.depth,
                false,
                cancel,
            )?;
            staged.push((a.entry.clone(), p));
            if k == 0 {
                stage(2);
            }
            if cancelled() {
                return Err(Stop::Cancelled);
            }
        }
        folder_guard = Some(pending);
    }
    progress.store(0.6f32.to_bits(), Ordering::Relaxed);
    // 4. The package's partial.
    let pending = write_package(&x, &staged, &job.dest, job.replace, &stage, cancel)?;
    // 5. Into place; the staged audio goes (dropped uncommitted).
    pending.commit()?;
    drop(folder_guard);
    progress.store(0.9f32.to_bits(), Ordering::Relaxed);
    // 6. The snapshot.
    if let Err(why) = keep_snapshot(&job.root, &job.export_id, &x.snapshot_json) {
        x.report.add(ItemKind::Note, &snapshot_not_kept(&why), None);
    }
    Ok(x.report)
}

/// The export job: every step inside `catch_unwind`.
pub(crate) fn job(job: ExportJob) -> impl FnOnce(&AtomicU32, &AtomicBool) -> Work + Send + 'static {
    move |progress, cancel| {
        if cancel.load(Ordering::Relaxed) {
            return Work::Cancelled;
        }
        let ran = catch_unwind(AssertUnwindSafe(|| body(&job, progress, cancel)));
        progress.store(1f32.to_bits(), Ordering::Relaxed);
        match ran {
            Ok(Ok(report)) => Work::ExportMidi {
                result: Ok(report),
                written: vec![job.dest.display().to_string()],
            },
            Ok(Err(Stop::Cancelled)) => Work::Cancelled,
            Ok(Err(Stop::Failed(why))) => Work::ExportMidi {
                result: Err(why),
                written: Vec::new(),
            },
            Err(p) => Work::ExportMidi {
                result: Err(format!("internal error: {}", panic_message(p.as_ref()))),
                written: Vec::new(),
            },
        }
    }
}

// ---- the snapshots a Save keeps (Ruling 16) ----

/// Whether `name` is a snapshot's: `<uuid>.json`.
fn snapshot_name(name: &str) -> bool {
    name.strip_suffix(".json")
        .is_some_and(core_model::document::check_project_id)
}

/// The regular files named `<uuid>.json` in `dir` (lstat: links and
/// folders are skipped), with their modification times.
fn snapshots_in(dir: &Path) -> Vec<(std::time::SystemTime, String)> {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    rd.filter_map(Result::ok)
        .filter_map(|e| {
            let name = e.file_name().to_str()?.to_owned();
            if !snapshot_name(&name) {
                return None;
            }
            let m = std::fs::symlink_metadata(e.path()).ok()?;
            if !m.is_file() {
                return None;
            }
            Some((m.modified().ok()?, name))
        })
        .collect()
}

/// Copies `src` to `dst` (never over anything), keeping its modification
/// time, so the copies prune as the originals would.
fn copy_snapshot(src: &Path, dst: &Path) -> Result<(), String> {
    let why = |e: std::io::Error| format!("{}: {e}", display_name(src));
    let meta = std::fs::symlink_metadata(src).map_err(why)?;
    if !meta.is_file() {
        return Ok(());
    }
    let mut from = std::fs::File::open(src).map_err(why)?;
    let mut to = match std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(dst)
    {
        Ok(f) => f,
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => return Ok(()),
        Err(e) => return Err(why(e)),
    };
    std::io::copy(&mut from, &mut to).map_err(why)?;
    to.flush().map_err(why)?;
    if let Ok(t) = meta.modified() {
        let _ = to.set_modified(t);
    }
    to.sync_all().map_err(why)
}

/// On Save (Ruling 16, L6): makes `to`'s `Interchange/` when needed (a
/// link or a file there is refused), copies `from`'s snapshots into it
/// when `from` is another package (regular files named `<uuid>.json`;
/// links skipped, nothing replaced), then prunes it to the
/// [`KEEP_SNAPSHOTS`] newest by modification time (ties by name).
pub(crate) fn keep_snapshots(from: &Path, to: &Path) -> Result<(), String> {
    let src = from.join(SNAPSHOT_DIR);
    let dst = to.join(SNAPSHOT_DIR);
    let same =
        from == to || matches!((from.canonicalize(), to.canonicalize()), (Ok(a), Ok(b)) if a == b);
    let has_src = !same && std::fs::symlink_metadata(&src).is_ok_and(|m| m.is_dir());
    if !has_src && std::fs::symlink_metadata(&dst).is_err() {
        // Nothing to keep and nowhere to prune.
        return Ok(());
    }
    real_dir(&dst)?;
    let mut first_error = None;
    if has_src {
        for (_, name) in snapshots_in(&src) {
            if let Err(e) = copy_snapshot(&src.join(&name), &dst.join(&name)) {
                first_error.get_or_insert(e);
            }
        }
    }
    let mut all = snapshots_in(&dst);
    all.sort();
    let extra = all.len().saturating_sub(KEEP_SNAPSHOTS);
    for (_, name) in all.into_iter().take(extra) {
        if let Err(e) = std::fs::remove_file(dst.join(&name)) {
            first_error.get_or_insert(format!("{name}: {e}"));
        }
    }
    first_error.map_or(Ok(()), Err)
}

// ---- the import (Task 10) ----

/// What the app reads of a DAWproject file before it imports it (L8).
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct DawprojectInfoRow {
    /// The rate of the project to make (Ruling 17).
    pub sample_rate: u32,
    /// Note and audio tracks (S14).
    pub tracks: u32,
    /// Distinct audio files.
    pub audio_files: u32,
    /// The file came from VizDAW (Ruling 16).
    pub from_vizdaw: bool,
    /// The export it came from: its record's, else its song info's export
    /// line's.
    pub export_id: Option<String>,
    /// The project it came from: its record head's (plan 6e Task 8).
    #[uniffi(default = None)]
    pub project_id: Option<String>,
}

/// Import a DAWproject file into this (new, empty) project.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct DawprojectImportRequestRow {
    /// The file (absolute).
    pub path: String,
}

/// What [`crate::interchange::file_path`] calls a DAWproject file.
pub(crate) const DAWPROJECT_FILE: &str = "a DAWproject file";

/// The import's free-space failure: it needs `bytes`.
pub fn not_enough_space_to_import(bytes: u64) -> String {
    format!(
        "Not enough free space to import this file (it needs {} MB).",
        bytes.div_ceil(1_000_000)
    )
}

/// The import's failure when a decoder delivers more audio than its file
/// says ([`core_media::import::LONGER_THAN_PROBED`]).
pub fn audio_longer_than_it_says(file: &str) -> String {
    format!("The audio file {file} holds more audio than it says, so the import stopped.")
}

/// The import's failure when a file that gives no length converts past
/// [`UNKNOWN_LENGTH_CAP`] while the free space cannot be read.
pub fn audio_too_long_to_convert(file: &str) -> String {
    format!(
        "The audio file {file} gives no length and converts to more than 4 GB, so the import stopped."
    )
}

/// The most one converted file that gives no length may take when the
/// free space cannot be read (final review follow-up).
const UNKNOWN_LENGTH_CAP: u64 = 4 << 30;
/// The check's refusal at [`UNKNOWN_LENGTH_CAP`], worded per file by
/// [`Halt::of`].
const NO_LENGTH_TOO_LONG: &str = "no length, past the cap";

/// Why converting one audio file ended without its media.
enum Halt {
    /// The job was cancelled.
    Cancelled,
    /// The job stops: the sentence says why.
    Refused(String),
    /// The file goes offline: why.
    Offline(String),
}

impl Halt {
    /// A conversion's error, for the file the report calls `file`.
    fn of(e: core_media::MediaError, file: &str) -> Halt {
        use core_media::MediaError;
        match e {
            MediaError::Cancelled => Halt::Cancelled,
            MediaError::Refused(s) if s == core_media::import::LONGER_THAN_PROBED => {
                Halt::Refused(audio_longer_than_it_says(file))
            }
            MediaError::Refused(s) if s == NO_LENGTH_TOO_LONG => {
                Halt::Refused(audio_too_long_to_convert(file))
            }
            MediaError::Refused(s) => Halt::Refused(s),
            e => Halt::Offline(e.to_string()),
        }
    }
}

/// Opens the file at `text` (checked as `file_path` checks it) and reads
/// what [`DawprojectInfoRow`] holds. Nothing outside the package is looked
/// at.
pub(crate) fn info(text: &str) -> Result<DawprojectInfoRow, VizError> {
    use core_io::dawproject::package::open;
    use core_io::limits::PackageLimits;
    let path = crate::interchange::file_path(text, DAWPROJECT_FILE)?;
    let pkg = open(&path, &PackageLimits::default(), &AtomicBool::new(false)).map_err(io_error)?;
    let i = pkg.info();
    Ok(DawprojectInfoRow {
        sample_rate: i.sample_rate,
        tracks: i.tracks,
        audio_files: i.audio_files,
        from_vizdaw: i.ret.from_vizdaw(),
        export_id: i
            .ret
            .record
            .as_ref()
            .map(|r| r.export_id.clone())
            .or_else(|| i.ret.export_id.clone()),
        project_id: i.ret.record.as_ref().map(|r| r.project_id.clone()),
    })
}

/// Plug-ins installed here: `(format, ID)` of every catalog entry that is
/// enabled and usable (OK or not validated yet).
pub(crate) type Installed = std::collections::HashSet<(core_model::mixer::PluginFormat, String)>;

/// The installed plug-ins, from the catalog now.
pub(crate) fn installed() -> Installed {
    use core_host::catalog::PluginStatus;
    crate::catalog::with(|c| {
        c.rows()
            .into_iter()
            .filter(|r| {
                r.enabled && matches!(r.status, PluginStatus::Ok | PluginStatus::NotValidated)
            })
            .map(|r| (r.plugin.format, r.plugin.id))
            .collect()
    })
}

/// Everything the import job needs (all of it owned: it runs on its
/// thread).
pub(crate) struct ImportJob {
    /// The file.
    pub path: PathBuf,
    /// The media root the audio is converted into.
    pub root: PathBuf,
    /// The project's format: the audio is converted to it, and its rate is
    /// the mapping's.
    pub format: core_model::format::AudioFormat,
    /// Names no new media file may take
    /// (`History::referenced_media_anywhere`).
    pub reserved: BTreeSet<String>,
    /// The plug-ins installed here.
    pub installed: Installed,
    /// The stage after which it cancels itself (test hook: 5 opened, 6 the
    /// first entry extracted, 7 the first file converted, 8 mapped).
    pub cancel_at: Option<u32>,
    /// The free bytes to assume (test hook; `None`: the disk's).
    pub free: Option<u64>,
}

/// The files an import converted: `(name, items)`.
pub(crate) type ImportFiles = Vec<(String, Result<Vec<core_media::import::Imported>, String>)>;

/// A fresh name in `Media/.incoming/` for an extracted entry, keeping its
/// extension when it is a short ASCII one (the decoder may use it).
fn extracted_name(entry: &str) -> String {
    use std::sync::atomic::AtomicU64;
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    let ext = Path::new(entry)
        .extension()
        .and_then(|e| e.to_str())
        .filter(|e| !e.is_empty() && e.len() <= 8 && e.chars().all(|c| c.is_ascii_alphanumeric()))
        .unwrap_or("audio");
    format!(
        "dp-{:x}-{:x}-{:x}.{ext}",
        std::process::id(),
        nanos,
        NEXT.fetch_add(1, Ordering::Relaxed)
    )
}

use crate::record::LOW_DISK_STOP;

/// The bytes the free-space check allows each converted file beyond its
/// samples (its header and chunks).
const CAF_HEADROOM: u64 = 64 * 1024;

/// The import job's body: the mapped import, or how it stopped. Converted
/// media goes into `files` as it is made, so the caller can remove it
/// whatever happens.
fn import_body(
    job: &ImportJob,
    importer: &dyn core_media::import::MediaImporter,
    files: &mut ImportFiles,
    progress: &AtomicU32,
    cancel: &AtomicBool,
) -> Result<Box<core_io::dawproject::import::DpImport>, Stop> {
    use core_io::dawproject::import::{ImportEnv, map};
    use core_io::dawproject::package::open;
    use core_io::limits::PackageLimits;
    let stage = |n: u32| {
        if job.cancel_at == Some(n) {
            cancel.store(true, Ordering::Relaxed);
        }
    };
    let cancelled = || cancel.load(Ordering::Relaxed);
    // 1. Open.
    let pkg = open(&job.path, &PackageLimits::default(), cancel)?;
    stage(5);
    if cancelled() {
        return Err(Stop::Cancelled);
    }
    progress.store(0.1f32.to_bits(), Ordering::Relaxed);
    // 2–3. The free space and the audio.
    let media = read_and_convert(job, &pkg, importer, files, progress, cancel)?;
    // 4. The plan.
    let has =
        |f: core_model::mixer::PluginFormat, id: &str| job.installed.contains(&(f, id.to_owned()));
    let env = ImportEnv {
        installed: &has,
        plist: plist(),
        rate: job.format.sample_rate,
        ids: None,
    };
    let imp = map(&pkg, &media, &env, cancel)?;
    stage(8);
    if cancelled() {
        return Err(Stop::Cancelled);
    }
    Ok(Box::new(imp))
}

/// The media each audio file of `pkg` became, by its key.
pub(crate) type MediaMap = BTreeMap<usize, core_io::dawproject::import::MediaSource>;

/// 6d's import after its open (stages 6–7), for the import, the merge and
/// the AAF import (plan 6f): the free-space check (external files are
/// looked at only now), then each audio file extracted and converted into
/// `job.root`'s `Media/`. Converted media goes into `files` as it is made,
/// so the caller can remove it whatever happens. Progress runs from 0.1 to
/// 0.9.
pub(crate) fn read_and_convert(
    job: &ImportJob,
    pkg: &dyn core_io::dawproject::package::AudioPackage,
    importer: &dyn core_media::import::MediaImporter,
    files: &mut ImportFiles,
    progress: &AtomicU32,
    cancel: &AtomicBool,
) -> Result<MediaMap, Stop> {
    use core_io::dawproject::import::MediaSource;
    use core_io::dawproject::package::AudioSource;
    use core_media::MediaError;
    use core_media::import::{INCOMING_DIR, import_checked};
    let stage = |n: u32| {
        if job.cancel_at == Some(n) {
            cancel.store(true, Ordering::Relaxed);
        }
    };
    let cancelled = || cancel.load(Ordering::Relaxed);
    let set = |f: f32| progress.store(f.to_bits(), Ordering::Relaxed);
    // The free space.
    let depth = u32::try_from(job.format.depth.bytes()).unwrap_or(u32::MAX);
    let need = pkg.needed_bytes(depth, cancel)?;
    let free = job.free.or_else(|| crate::record::free_bytes(&job.root));
    if free.is_some_and(|f| need > f) {
        return Err(Stop::Failed(not_enough_space_to_import(need)));
    }
    // The audio. Each file is checked again once probed (final review
    // I1): the file's own duration is not trusted, so the space its
    // conversion may take, at the probe's frame budget, must still be
    // free; and the conversion stops at that budget.
    let incoming = job.root.join(INCOMING_DIR);
    let n = pkg.audio().len().max(1) as f32;
    let mut media = BTreeMap::new();
    let (mut extracted, mut converted) = (false, false);
    // What this job has converted so far, counted against `job.free` (the
    // disk's own count already holds it).
    let mut used = 0u64;
    for (k, a) in pkg.audio().iter().enumerate() {
        let mut each = |f: f32| set(0.1 + 0.8 * (k as f32 + f.clamp(0.0, 1.0)) / n);
        let mut convert = |from: &Path, copy: u64, converted: &mut bool| {
            // The free space, read once per file (the disk's own count then
            // holds what earlier files took; the test hook's does not).
            let mut free_now: Option<Option<u64>> = None;
            let mut room = |info: &core_media::import::SourceInfo, reach: u64| {
                let need = reach
                    .saturating_mul(u64::from(info.channels))
                    .saturating_mul(u64::from(depth))
                    .saturating_add(CAF_HEADROOM.saturating_mul(u64::from(info.channels)));
                let left = *free_now.get_or_insert_with(|| match job.free {
                    Some(f) => Some(f.saturating_sub(used).saturating_sub(copy)),
                    None => crate::record::free_bytes(&job.root),
                });
                if info.frames > 0 {
                    // A known length: the whole budget must fit, keeping
                    // the same reserve free (final re-review O2).
                    let need = need.saturating_add(LOW_DISK_STOP);
                    if left.is_some_and(|f| need > f) {
                        return Err(MediaError::Refused(not_enough_space_to_import(need)));
                    }
                    return Ok(());
                }
                // No length (final review follow-up): bounded by the disk,
                // keeping the recorder's stop reserve free; with no free
                // space to go by, by a fixed cap per file.
                match left {
                    Some(f) if need.saturating_add(LOW_DISK_STOP) > f => Err(MediaError::Refused(
                        not_enough_space_to_import(need.saturating_add(LOW_DISK_STOP)),
                    )),
                    None if need > UNKNOWN_LENGTH_CAP => {
                        Err(MediaError::Refused(NO_LENGTH_TOO_LONG.to_owned()))
                    }
                    _ => Ok(()),
                }
            };
            let r = import_checked(
                importer,
                from,
                &job.root,
                job.format,
                &a.name,
                &job.reserved,
                &mut room,
                &mut each,
                cancel,
            );
            if !std::mem::replace(converted, true) {
                stage(7);
            }
            r
        };
        let r: Result<Vec<core_media::import::Imported>, Halt> = match &a.source {
            AudioSource::Entry(name) => {
                let copy = incoming.join(extracted_name(name));
                let made = std::fs::create_dir_all(&incoming)
                    .map_err(|e| IoError::Io(format!("{}: {e}", display_name(&incoming))))
                    .and_then(|()| pkg.extract(a, &copy, cancel));
                match made {
                    Err(IoError::Cancelled) => Err(Halt::Cancelled),
                    Err(e) => Err(Halt::Offline(e.to_string())),
                    Ok(size) => {
                        if !std::mem::replace(&mut extracted, true) {
                            stage(6);
                        }
                        let r = if cancelled() {
                            Err(MediaError::Cancelled)
                        } else {
                            convert(&copy, size, &mut converted)
                        };
                        let _ = std::fs::remove_file(&copy);
                        let file = entry_file(name);
                        r.map(|mut items| {
                            for i in &mut items {
                                i.original_name = file.to_owned();
                            }
                            items
                        })
                        .map_err(|e| match Halt::of(e, file) {
                            // The entry's name, not the extracted copy's.
                            Halt::Offline(why) => {
                                Halt::Offline(why.replace(&copy.display().to_string(), file))
                            }
                            h => h,
                        })
                    }
                }
            }
            AudioSource::External(p) => match core_io::read::external_file(p) {
                Ok(real) => {
                    convert(&real, 0, &mut converted).map_err(|e| Halt::of(e, &display_name(p)))
                }
                Err(why) => Err(Halt::Offline(why)),
            },
            AudioSource::Offline(why) => Err(Halt::Offline(why.clone())),
        };
        match r {
            Err(Halt::Cancelled) => return Err(Stop::Cancelled),
            Err(Halt::Refused(why)) => return Err(Stop::Failed(why)),
            Ok(items) => {
                for i in &items {
                    used = used.saturating_add(
                        i.length_frames
                            .saturating_mul(u64::from(i.channels))
                            .saturating_mul(u64::from(depth)),
                    );
                }
                files.push((a.name.clone(), Ok(items.clone())));
                media.insert(a.key, MediaSource::Online(items));
            }
            Err(Halt::Offline(why)) => {
                media.insert(a.key, MediaSource::Offline { why });
            }
        }
        if cancelled() {
            return Err(Stop::Cancelled);
        }
    }
    set(0.9);
    Ok(media)
}

/// The import job: every step inside `catch_unwind`. Whatever it
/// converted is returned with the result, so the session removes it when
/// the import is not applied (6b F1).
pub(crate) fn import_job(
    job: ImportJob,
) -> impl FnOnce(&AtomicU32, &AtomicBool) -> Work + Send + 'static {
    move |progress, cancel| {
        let mut files = ImportFiles::new();
        if cancel.load(Ordering::Relaxed) {
            return Work::ImportProject {
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
        Work::ImportProject { files, result }
    }
}

/// Removes `<root>/Media/.incoming` and `<root>/Media/.overviews` when
/// they are empty (backlog row "DAWproject import clean-up"). Only at
/// close, once no job and no overview worker can write into them (Task 8
/// review I2); a folder that holds a file stays.
pub(crate) fn remove_empty_media_folders(root: &Path) {
    let _ = std::fs::remove_dir(root.join(core_media::import::INCOMING_DIR));
    let _ = std::fs::remove_dir(root.join(core_media::overview::DIR));
}

// ---- the merge (plan 6e Task 8) ----

/// How far a merge job goes (Ruling 12).
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum MergeStepRow {
    /// Compute the plan and report; keep nothing, change nothing.
    Preview,
    /// Merge into a copy of the history and keep it, with its media, until
    /// `confirm_merge` or `cancel_merge` (the sheet).
    Hold,
    /// Merge and swap it in at once (the control channel).
    Apply,
}

/// Merge a returning DAWproject file into this project.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct DawprojectMergeRequestRow {
    /// The file (absolute).
    pub path: String,
    /// The export to compare with, when the file lost its export ID (an
    /// export ID).
    pub snapshot: Option<String>,
    /// How far to go.
    pub step: MergeStepRow,
    /// The app's local date, "YYYY-MM-DD": it names conflict takes
    /// ("Returned <date>", Ruling 6).
    pub date: String,
}

/// What a merge does (Ruling 15), before it is applied.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct MergePreviewRow {
    /// Units taken from the other DAW, or the same on both sides.
    pub applied: u32,
    /// Units kept as they are here.
    pub kept: u32,
    /// Things the other DAW added.
    pub added: u32,
    /// Things the other DAW removed.
    pub removed: u32,
    /// Units in conflict.
    pub conflicts: u32,
    /// One line per conflict, in order.
    pub conflict_lines: Vec<String>,
    /// "Merged: <a> applied, …", or "Nothing to merge: …".
    pub summary: String,
    /// The export the file came from.
    pub export_id: String,
    /// When it was exported, in Unix seconds.
    pub exported_at: u64,
}

/// One snapshot the project keeps (Ruling 13).
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct SnapshotRow {
    /// The export's ID.
    pub export_id: String,
    /// When it was exported, in Unix seconds.
    pub exported_at: u64,
    /// The tracks it exported.
    pub tracks: u32,
}

/// The refusal of a file whose record names another project, or whose
/// export this project has no snapshot of (Ruling 13).
pub const NOT_FROM_HERE: &str = "This file was not exported from that project.";
/// The refusal of a file that did not come from VizDAW.
pub const NOT_FROM_VIZDAW: &str =
    "This file did not come from VizDAW, so it can't be merged; open it as a new project.";
/// The refusal of a merge whose project changed after its preview (Ruling
/// 12, Review Focus 3).
pub const CHANGED_SINCE: &str = "The project changed after the preview; preview the merge again.";
/// The failure of an Apply job that ends while recording (plan 6e Task 8
/// review M1; Task 10 fix round 1 wording).
pub const RECORDING_APPLY: &str = "Stop recording, then merge again.";
/// Save As's refusal while a merge is held (plan 6e Task 8 review I1; Task
/// 10 fix round 1 wording).
pub const SAVE_AS_HELD: &str = "Finish or cancel the merge waiting to be confirmed, then save as.";
/// The refusal of a `confirm_merge` or `cancel_merge` for a job that is
/// not the held merge.
pub const NO_MERGE_WAITING: &str = "no merge is waiting";
/// The refusal of a merge date that is not "YYYY-MM-DD".
pub const BAD_DATE: &str = "the merge date must be YYYY-MM-DD";

/// `secs` since 1970 as "yyyy-mm-dd hh:mm UTC".
fn utc_minute(secs: u64) -> String {
    let day = secs % 86_400;
    format!(
        "{} {:02}:{:02} UTC",
        core_io::dawproject::merge::text::utc_date(secs),
        day / 3_600,
        day % 3_600 / 60
    )
}

/// Ruling 13's refusal of a file with no export ID, naming `rows` (at most
/// [`KEEP_SNAPSHOTS`]): "This file lost its export ID; give `snapshot`,
/// one of: <id> (<yyyy-mm-dd hh:mm UTC>, <n> tracks), …". With none,
/// [`NOT_FROM_HERE`].
pub fn needs_snapshot(rows: &[SnapshotRow]) -> String {
    if rows.is_empty() {
        return NOT_FROM_HERE.to_owned();
    }
    let list: Vec<String> = rows
        .iter()
        .take(KEEP_SNAPSHOTS)
        .map(|r| {
            let tracks = if r.tracks == 1 { "track" } else { "tracks" };
            format!(
                "{} ({}, {} {tracks})",
                r.export_id,
                utc_minute(r.exported_at),
                r.tracks
            )
        })
        .collect();
    format!(
        "This file lost its export ID; give `snapshot`, one of: {}",
        list.join(", ")
    )
}

/// Refuses a merge date that is not "YYYY-MM-DD" (ASCII digits).
pub(crate) fn check_date(date: &str) -> Result<(), VizError> {
    let b = date.as_bytes();
    let ok = b.len() == 10
        && b.iter().enumerate().all(|(i, c)| match i {
            4 | 7 => *c == b'-',
            _ => c.is_ascii_digit(),
        });
    if ok { Ok(()) } else { Err(model(BAD_DATE)) }
}

/// Refuses a `snapshot` that is not an export ID.
pub(crate) fn check_snapshot(snapshot: Option<&str>) -> Result<(), VizError> {
    match snapshot {
        Some(s) if !core_model::document::check_project_id(s) => Err(model(
            "`snapshot` must be an export ID (36 lowercase hex characters and hyphens)",
        )),
        _ => Ok(()),
    }
}

/// The tracks a snapshot exported: its lanes when it knows them (a split
/// track is one), else its projection's tracks.
fn snapshot_tracks(s: &core_io::dawproject::record::Snapshot) -> u32 {
    let n = match &s.identity {
        Some(id) => id
            .tracks
            .iter()
            .map(|t| t.lane)
            .collect::<BTreeSet<_>>()
            .len(),
        None => s.projection.tracks.len(),
    };
    u32::try_from(n).unwrap_or(u32::MAX)
}

/// The most snapshot files [`snapshot_rows`] reads.
const MAX_SNAPSHOT_READS: usize = 4 * KEEP_SNAPSHOTS;

/// The project's snapshots (`<root>/Interchange/<uuid>.json` that read
/// whole and are `project_id`'s), newest first (by their export time, then
/// their file's), at most [`KEEP_SNAPSHOTS`]. Reads every snapshot file:
/// off the main thread. None without a project ID.
pub(crate) fn snapshot_rows(root: &Path, project_id: Option<&str>) -> Vec<SnapshotRow> {
    use core_io::dawproject::record::read_snapshot_file;
    let Some(project_id) = project_id else {
        return Vec::new();
    };
    // A folder stuffed by hand costs at most `MAX_SNAPSHOT_READS` reads
    // (review M5): the newest files by modification time.
    let mut candidates = snapshots_in(&root.join(SNAPSHOT_DIR));
    candidates.sort_by(|a, b| b.cmp(a));
    candidates.truncate(MAX_SNAPSHOT_READS);
    let mut found: Vec<(u64, std::time::SystemTime, SnapshotRow)> = candidates
        .into_iter()
        .filter_map(|(time, name)| {
            let id = name.strip_suffix(".json")?;
            let s = read_snapshot_file(root, id).ok()??;
            (s.project_id == project_id).then(|| {
                let row = SnapshotRow {
                    export_id: s.export_id.clone(),
                    exported_at: s.exported_at,
                    tracks: snapshot_tracks(&s),
                };
                (s.exported_at, time, row)
            })
        })
        .collect();
    found.sort_by(|a, b| (b.0, b.1, &b.2.export_id).cmp(&(a.0, a.1, &a.2.export_id)));
    found
        .into_iter()
        .take(KEEP_SNAPSHOTS)
        .map(|(_, _, r)| r)
        .collect()
}

/// Whether `<root>/Interchange/<export_id>.json` is a regular file: one
/// lstat; false for a malformed ID.
pub(crate) fn has_snapshot(root: &Path, export_id: &str) -> bool {
    core_model::document::check_project_id(export_id)
        && std::fs::symlink_metadata(root.join(SNAPSHOT_DIR).join(format!("{export_id}.json")))
            .is_ok_and(|m| m.is_file())
}

/// What the project was when a merge job started (Ruling 12): the swap
/// happens only while it still is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MergeBase {
    /// `History::present_serial()`.
    pub serial: u64,
    /// The present document's `referenced_states()` (a step-free capture
    /// changes them).
    pub states: BTreeSet<core_model::ids::StateBlobId>,
    /// The project's ID.
    pub project_id: Option<String>,
}

impl MergeBase {
    /// `h`'s present.
    pub(crate) fn of(h: &core_model::history::History) -> MergeBase {
        MergeBase {
            serial: h.present_serial(),
            states: h.doc().referenced_states(),
            project_id: h.doc().settings().project_id.clone(),
        }
    }
}

/// Everything the merge job needs (all of it owned: it runs on its
/// thread).
pub(crate) struct MergeJob {
    /// The file, the media root, the format, the reserved names, the
    /// plug-ins installed and the test hooks, as an import has them
    /// (stages 5–8 as the import's; 9 planned, 10 applied, 11 inside the
    /// apply).
    pub io: ImportJob,
    /// A clone of the session's history at spawn: the plan is made against
    /// its document and the merge applied to it.
    pub history: core_model::history::History,
    /// What the project was at spawn.
    pub base: MergeBase,
    /// The request.
    pub req: DawprojectMergeRequestRow,
}

/// A finished merge job: the preview, the report, the step and, for Hold
/// and Apply, the merged history and the project it was made from.
#[derive(Debug)]
pub struct Merged {
    /// The counts and conflict lines.
    pub preview: MergePreviewRow,
    /// The report (its summary counts are not the merge's: see
    /// [`merged_report`]).
    pub report: Report,
    /// The step asked for.
    pub step: MergeStepRow,
    /// The merged history (Hold and Apply).
    pub history: Option<core_model::history::History>,
    /// What the project was at spawn.
    pub base: MergeBase,
}

/// The merge's report row (Ruling 15): `report_row(.., "Merged")` with the
/// preview's summary.
pub(crate) fn merged_report(m: &Merged) -> crate::interchange::ReportRow {
    let mut r = crate::interchange::report_row(&m.report, "Merged");
    r.summary.clone_from(&m.preview.summary);
    r
}

/// The job's cancel flag as `apply_merge_until` asks it; with `trip` it
/// sets the flag at the first check (the stage-11 test hook).
struct TripAt<'a> {
    cancel: &'a AtomicBool,
    trip: bool,
}

impl core_io::dawproject::merge::apply::Stop for TripAt<'_> {
    fn stopped(&self) -> bool {
        if self.trip {
            self.cancel.store(true, Ordering::Relaxed);
        }
        self.cancel.load(Ordering::Relaxed)
    }
}

/// The merge job's body (Rulings 2, 12, 13): the merge, or how it stopped.
/// Converted media goes into `files` as it is made.
fn merge_body(
    job: &MergeJob,
    importer: &dyn core_media::import::MediaImporter,
    files: &mut ImportFiles,
    progress: &AtomicU32,
    cancel: &AtomicBool,
) -> Result<Box<Merged>, Stop> {
    use core_io::dawproject::import::{ImportEnv, map};
    use core_io::dawproject::merge::apply::{CANCELLED, LABEL, apply_merge_until};
    use core_io::dawproject::merge::decide::plan;
    use core_io::dawproject::merge::text::summary;
    use core_io::dawproject::merge::theirs::theirs;
    use core_io::dawproject::package::open;
    use core_io::dawproject::record::read_snapshot_file;
    use core_io::limits::PackageLimits;
    use core_model::error::ModelError;
    let stage = |n: u32| {
        if job.io.cancel_at == Some(n) {
            cancel.store(true, Ordering::Relaxed);
        }
    };
    let cancelled = || cancel.load(Ordering::Relaxed);
    let failed = |s: &str| Stop::Failed(s.to_owned());
    // 1. Open.
    let pkg = open(&job.io.path, &PackageLimits::default(), cancel)?;
    stage(5);
    if cancelled() {
        return Err(Stop::Cancelled);
    }
    progress.store(0.1f32.to_bits(), Ordering::Relaxed);
    // 2. The export. A hand file first, so it never gets the snapshot
    // sentence.
    let ret = &pkg.ret;
    if !ret.from_vizdaw() {
        return Err(failed(NOT_FROM_VIZDAW));
    }
    let ours = job.base.project_id.as_deref();
    if let Some(r) = &ret.record
        && Some(r.project_id.as_str()) != ours
    {
        return Err(failed(NOT_FROM_HERE));
    }
    let named = ret
        .record
        .as_ref()
        .map(|r| r.export_id.clone())
        .or_else(|| ret.export_id.clone());
    let chosen = named.is_none();
    let export_id = match (named, job.req.snapshot.as_ref()) {
        (Some(n), Some(s)) if n != *s => return Err(failed(NOT_FROM_HERE)),
        (Some(n), _) => n,
        (None, Some(s)) => s.clone(),
        (None, None) => {
            return Err(Stop::Failed(needs_snapshot(&snapshot_rows(
                &job.io.root,
                ours,
            ))));
        }
    };
    // 3. The snapshot: this project's own (its Interchange folder, its
    // project ID), never one the file points elsewhere to.
    let snap = match read_snapshot_file(&job.io.root, &export_id) {
        Ok(Some(s)) => s,
        Ok(None) => return Err(failed(NOT_FROM_HERE)),
        Err(e) => return Err(Stop::Failed(e.to_string())),
    };
    if Some(snap.project_id.as_str()) != ours {
        return Err(failed(NOT_FROM_HERE));
    }
    // Ruling 2: the file's XML IDs are identities only when its record
    // head names this snapshot's export and project. The record alone is
    // never the proof: the snapshot was found through this project.
    let trusted = ret
        .record
        .as_ref()
        .is_some_and(|r| r.export_id == snap.export_id && r.project_id == snap.project_id);
    // 4. The free space and the audio (stages 6–7).
    let media = read_and_convert(&job.io, &pkg, importer, files, progress, cancel)?;
    // 5. The mapping.
    let has = |f: core_model::mixer::PluginFormat, id: &str| {
        job.io.installed.contains(&(f, id.to_owned()))
    };
    let env = ImportEnv {
        installed: &has,
        plist: plist(),
        rate: job.io.format.sample_rate,
        ids: trusted.then_some(&snap.ids),
    };
    let imp = map(&pkg, &media, &env, cancel)?;
    drop(pkg);
    stage(8);
    if cancelled() {
        return Err(Stop::Cancelled);
    }
    // 6. Theirs and the plan.
    let th = theirs(imp, job.io.format, snap.origin, cancel)?;
    let (export_id, exported_at) = (snap.export_id.clone(), snap.exported_at);
    let plan = plan(snap, th, job.history.doc(), chosen, cancel)?;
    stage(9);
    if cancelled() {
        return Err(Stop::Cancelled);
    }
    let p = &plan.preview;
    let preview = MergePreviewRow {
        applied: p.applied,
        kept: p.kept,
        added: p.added,
        removed: p.removed,
        conflicts: p.conflicts,
        conflict_lines: p.conflict_lines.clone(),
        summary: summary(p),
        export_id,
        exported_at,
    };
    // 7. The merge, in one transaction on the copy.
    let (report, history) = match job.req.step {
        MergeStepRow::Preview => (plan.report.clone(), None),
        MergeStepRow::Hold | MergeStepRow::Apply => {
            let mut h = job.history.clone();
            h.begin(LABEL).map_err(|e| Stop::Failed(e.to_string()))?;
            let mut report = Report::default();
            // Stage 11 (test hook): the cancel lands inside the apply, at
            // its first check (review M2).
            let inside = TripAt {
                cancel,
                trip: job.io.cancel_at == Some(11),
            };
            let ran = catch_unwind(AssertUnwindSafe(|| {
                apply_merge_until(&mut h, &plan, &job.req.date, &mut report, &inside)
            }));
            match ran {
                Ok(Ok(())) => h.commit().map_err(|e| Stop::Failed(e.to_string()))?,
                Ok(Err(ModelError::Invalid { reason })) if reason == CANCELLED => {
                    let _ = h.abort();
                    return Err(Stop::Cancelled);
                }
                Ok(Err(e)) => {
                    let _ = h.abort();
                    return Err(Stop::Failed(e.to_string()));
                }
                Err(p) => {
                    let _ = h.abort();
                    return Err(Stop::Failed(format!(
                        "internal error: {}",
                        panic_message(p.as_ref())
                    )));
                }
            }
            stage(10);
            if cancelled() {
                return Err(Stop::Cancelled);
            }
            (report, Some(h))
        }
    };
    progress.store(1f32.to_bits(), Ordering::Relaxed);
    Ok(Box::new(Merged {
        preview,
        report,
        step: job.req.step,
        history,
        base: job.base.clone(),
    }))
}

/// The merge job: every parse, mapping, plan and apply inside
/// `catch_unwind`. Whatever it converted is returned with the result, so
/// the session removes it when the merge is not kept.
pub(crate) fn merge_job(
    job: MergeJob,
) -> impl FnOnce(&AtomicU32, &AtomicBool) -> Work + Send + 'static {
    move |progress, cancel| {
        let mut files = ImportFiles::new();
        if cancel.load(Ordering::Relaxed) {
            return Work::MergeDawproject {
                files,
                result: Err(crate::for_daw::CANCELLED.to_owned()),
            };
        }
        let importer = crate::media::importer();
        let ran = catch_unwind(AssertUnwindSafe(|| {
            merge_body(&job, importer.as_ref(), &mut files, progress, cancel)
        }));
        progress.store(1f32.to_bits(), Ordering::Relaxed);
        let result = match ran {
            Ok(Ok(m)) => Ok(m),
            Ok(Err(Stop::Cancelled)) => Err(crate::for_daw::CANCELLED.to_owned()),
            Ok(Err(Stop::Failed(why))) => Err(why),
            Err(p) => Err(format!("internal error: {}", panic_message(p.as_ref()))),
        };
        Work::MergeDawproject { files, result }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn block_rules_become_clip_rules_with_minimums() {
        assert_eq!(
            clip_rule(BlockRule::SilenceGap { ticks: 0 }),
            ClipRule::SilenceGap(Tick(1))
        );
        assert_eq!(
            clip_rule(BlockRule::SilenceGap { ticks: 960 }),
            ClipRule::SilenceGap(Tick(960))
        );
        assert_eq!(
            clip_rule(BlockRule::FixedBars { bars: 0 }),
            ClipRule::FixedBars(1)
        );
        assert_eq!(clip_rule(BlockRule::WholeLane), ClipRule::WholeTrack);
    }

    #[test]
    fn wav_sizes_past_four_gib_are_too_long() {
        let max_frames = (u64::from(u32::MAX) - 37) / 6;
        assert!(!too_long(
            data_bytes(max_frames, 2, BitDepth::Int24),
            BitDepth::Int24
        ));
        assert!(too_long(
            data_bytes(max_frames + 1, 2, BitDepth::Int24),
            BitDepth::Int24
        ));
        assert!(too_long(
            data_bytes(u64::MAX, 2, BitDepth::Float32),
            BitDepth::Float32
        ));
    }

    #[test]
    fn left_out_audio_lists_too_long_then_offline() {
        let mut b = core_engine::fixtures::Builder::new();
        let mut add = |path: &str, frames: u64| b.media(path, frames, 2).unwrap();
        let fine = add("Media/a.caf", 48_000);
        let gone = add("Media/b.caf", 48_000);
        let huge = add("Media/c.caf", u64::from(u32::MAX));
        let both = add("Media/d.caf", u64::from(u32::MAX));
        let doc = b.finish();
        let got = left_out_audio(&doc, &BTreeSet::from([gone, both]));
        assert_eq!(
            got,
            BTreeMap::from([
                (gone, OFFLINE_AUDIO.to_owned()),
                (huge, TOO_LONG_FOR_WAV.to_owned()),
                (both, TOO_LONG_FOR_WAV.to_owned()),
            ])
        );
        assert!(!got.contains_key(&fine));
    }

    #[test]
    fn snapshot_names_are_uuids() {
        assert!(snapshot_name("0f8fad5b-d9cb-469f-a165-70867728950e.json"));
        assert!(!snapshot_name(
            "0f8fad5b-d9cb-469f-a165-70867728950e.json.partial"
        ));
        assert!(!snapshot_name("notes.json"));
        assert!(!snapshot_name(".json"));
    }
}
