//! Bounce (plan 5f-2 Task 4; spec §9, §12.1; Rulings 3–6, 8–10, 12, 13):
//! the request, check and result rows, the process-wide bounce settings,
//! the stem file names, the request's validation and the job's body.
//!
//! The job renders every source to a temporary float CAF in
//! `Media/.incoming/` with a dedicated engine (0–0.8), imports the sources
//! "Add to project" places (0.8–0.9), writes every export to its
//! `.partial` (0.9–1.0) and renames the exports last. A cancel or an error
//! before the renames removes everything it wrote; a file renamed into
//! place is never removed (Ruling 12).

use crate::error::VizError;
use crate::format::BitDepthRow;
use crate::jobs::{ImportTargetRow, Work};
use core_engine::backend::AudioBackend;
use core_engine::bounce::{BounceError, BounceSpec, BounceStats, HeardStem, SilentMidi};
use core_engine::engine::{Engine, EngineConfig, SinkFactory};
use core_engine::graph::NoInserts;
use core_engine::offline::{OfflineBackend, OfflineSinks};
use core_engine::schedule::TempoTable;
use core_media::MediaError;
use core_media::caf::{CafWriter, WriteMode};
use core_media::export::{Container, ExportFormat, PendingExport, display_name, partial_path};
use core_media::import::{INCOMING_DIR, Imported};
use core_media::stream::{DiskMode, MediaFile};
use core_model::document::Document;
use core_model::event::EventBody;
use core_model::format::{AudioFormat, BitDepth};
use core_model::ids::{MediaId, SeqId, StripId};
use core_model::selection::ViewFilter;
use core_model::time::Tick;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// What a bounce renders.
#[derive(Debug, Clone, PartialEq, uniffi::Enum)]
pub enum BounceSourceRow {
    /// Master, as heard.
    Mix,
    /// Each strip post-fader, one file (or media item) each, in this order.
    Stems {
        /// Strip IDs.
        strips: Vec<u64>,
    },
}

/// The range a bounce renders (plus the tail).
#[derive(Debug, Clone, Copy, PartialEq, uniffi::Enum)]
pub enum BounceRangeRow {
    /// Bar 1 to the sequence's end.
    Song,
    /// `[start, end)` in ticks.
    Ticks {
        /// First tick.
        start: i64,
        /// End tick (exclusive).
        end: i64,
    },
}

/// A file container.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum ContainerRow {
    /// RIFF WAVE.
    Wav,
    /// AIFF (AIFF-C for float).
    Aiff,
    /// Core Audio Format.
    Caf,
}

impl From<ContainerRow> for Container {
    fn from(c: ContainerRow) -> Container {
        match c {
            ContainerRow::Wav => Container::Wav,
            ContainerRow::Aiff => Container::Aiff,
            ContainerRow::Caf => Container::Caf,
        }
    }
}

/// A bounce as the sheet (or `file.bounce`) asks for it.
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct BounceRequestRow {
    /// The sequence.
    pub seq: u64,
    /// Mix or stems.
    pub source: BounceSourceRow,
    /// The range.
    pub range: BounceRangeRow,
    /// The files' container.
    pub container: ContainerRow,
    /// The files' depth.
    pub depth: BitDepthRow,
    /// The files' rate (8 000–384 000 Hz).
    pub rate: u32,
    /// TPDF dither at 16 and 24 bits.
    pub dither: bool,
    /// A file (mix) or an existing folder (stems); None: add to the project only.
    pub path: Option<String>,
    /// Place the result on new lanes at the range start (one undo step).
    pub add_to_project: bool,
    /// Replace an existing mix file (the save panel's confirmation); ignored for stems (owner answer 2).
    pub replace: bool,
}

/// What a bounce would do, before it starts.
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct BounceCheckRow {
    /// Lanes in the range that play external MIDI (silent in a bounce; Ruling 8).
    pub external_lanes: u32,
    /// The range's frames (the files hold at least this many).
    pub frames: u64,
    /// The files it would write, in order (stems numbered, Ruling 3). For
    /// the channel and the tests: the sheet shows the names only after the
    /// bounce (ruling T6-1).
    pub files: Vec<String>,
    /// Ruling 8's sentences, or None.
    pub warning: Option<String>,
}

/// A file a bounce wrote outside the project.
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct BouncedFileRow {
    /// The file.
    pub path: String,
    /// Frames.
    pub frames: u64,
    /// Rate in Hz.
    pub rate: u32,
    /// "16-bit", "24-bit" or "32-bit float".
    pub bits: String,
    /// Channels.
    pub channels: u16,
    /// Integer samples clipped at full scale (either sign).
    pub clipped: u64,
}

/// What a finished bounce wrote. `files` in source order; `media` in source order (Ruling 10).
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct BounceResultRow {
    /// The files written outside the project (already in place).
    pub files: Vec<BouncedFileRow>,
    /// The media items "Add to project" made.
    pub media: Vec<u64>,
}

/// The process-wide bounce settings.
#[derive(Debug, Clone, Copy, PartialEq, uniffi::Record)]
pub struct BounceSettingsRow {
    /// The longest tail, 0–30 000 ms (Ruling 4).
    pub tail_cap_ms: u32,
}

/// The tail cap a process starts with (spec §9.1: 10 s).
pub const DEFAULT_TAIL_CAP_MS: u32 = 10_000;
/// The longest tail cap (Ruling 4).
pub const MAX_TAIL_CAP_MS: u32 = 30_000;
/// The bounce engine's block and the plugin copies' (Ruling 6).
pub const BLOCK: u32 = 1024;
/// What a clip line about an "Add to project" import says after its
/// name: it describes the project's copy, not an exported file.
pub(crate) const ADDED_CLIPPED: &str = " (added to the project): ";
/// How long the job waits for the plugin copies to load (Ruling 6).
const READY_WAIT: Duration = Duration::from_secs(60);
/// The job's poll while it waits for the copies.
const READY_POLL: Duration = Duration::from_millis(5);
/// The longest destination path (Ruling 3).
const MAX_PATH_BYTES: usize = 1024;
/// The longest file name.
const MAX_NAME_BYTES: usize = 255;
/// The longest number suffix `commit_numbered` gives (" 9999").
const NUMBER_BYTES: usize = 5;
/// The highest number a stem name gets (the one `commit_numbered` tries).
const MAX_NUMBER: u32 = core_media::export::MAX_NUMBER;

static TAIL_CAP_MS: AtomicU32 = AtomicU32::new(DEFAULT_TAIL_CAP_MS);

/// The bounce settings. Infallible: a panic answers the defaults.
#[uniffi::export]
pub fn bounce_settings() -> BounceSettingsRow {
    std::panic::catch_unwind(|| BounceSettingsRow {
        tail_cap_ms: TAIL_CAP_MS.load(Ordering::Relaxed),
    })
    .unwrap_or(BounceSettingsRow {
        tail_cap_ms: DEFAULT_TAIL_CAP_MS,
    })
}

/// Sets the bounce settings (the tail cap clamped to 0–30 000 ms).
/// Infallible: a panic changes nothing.
#[uniffi::export]
pub fn set_bounce_settings(settings: BounceSettingsRow) {
    let _ = std::panic::catch_unwind(|| {
        TAIL_CAP_MS.store(settings.tail_cap_ms.min(MAX_TAIL_CAP_MS), Ordering::Relaxed);
    });
}

/// `name` made safe for a file (Ruling 3): `/`, `:` and control characters
/// become `-`, a leading `.` becomes `-`, and it is cut at a character
/// boundary so that `.<name> NNNN.<ext>.partial` fits 255 bytes.
fn safe_name(name: &str, ext: &str) -> String {
    let mut s: String = name
        .chars()
        .map(|c| {
            if c == '/' || c == ':' || c.is_control() {
                '-'
            } else {
                c
            }
        })
        .collect();
    if s.starts_with('.') {
        s.replace_range(..1, "-");
    }
    let max = MAX_NAME_BYTES
        .saturating_sub(1 + NUMBER_BYTES + 1 + ext.len() + ".partial".len())
        .max(1);
    if s.len() > max {
        let mut cut = max;
        while cut > 0 && !s.is_char_boundary(cut) {
            cut -= 1;
        }
        s.truncate(cut);
    }
    if s.is_empty() { "Stem".to_string() } else { s }
}

/// Per name, `(base, file)`: the safe name's file in `folder`, and that
/// name numbered (" 2", " 3", …) when this bounce or the folder already
/// has it (compared without case, as macOS volumes do).
fn stem_plan(folder: &Path, names: &[String], ext: &str) -> Vec<(PathBuf, PathBuf)> {
    let mut chosen: BTreeSet<String> = BTreeSet::new();
    let mut out = Vec::with_capacity(names.len());
    for name in names {
        let safe = safe_name(name, ext);
        let base = folder.join(format!("{safe}.{ext}"));
        let free = |p: &Path, chosen: &BTreeSet<String>| {
            let lower = p
                .file_name()
                .map(|n| n.to_string_lossy().to_lowercase())
                .unwrap_or_default();
            !chosen.contains(&lower) && std::fs::symlink_metadata(p).is_err()
        };
        let mut pick = base.clone();
        let mut n = 2;
        while !free(&pick, &chosen) && n <= MAX_NUMBER {
            pick = folder.join(format!("{safe} {n}.{ext}"));
            n += 1;
        }
        chosen.insert(
            pick.file_name()
                .map(|n| n.to_string_lossy().to_lowercase())
                .unwrap_or_default(),
        );
        out.push((base, pick));
    }
    out
}

fn model(message: impl Into<String>) -> VizError {
    VizError::Model {
        message: message.into(),
    }
}

/// A media error's own words (without "unsupported audio: ").
fn media_words(e: &MediaError) -> String {
    match e {
        MediaError::Io(s) | MediaError::Unsupported(s) => s.clone(),
        other => other.to_string(),
    }
}

/// A strip's display name ("Strip <id>" when it has none).
fn strip_name(doc: &Document, s: StripId) -> String {
    match doc.mixer().strip(s) {
        Ok(x) if !x.name.trim().is_empty() => x.name.clone(),
        _ => format!("Strip {}", s.0),
    }
}

/// A plugin slot's name, for the problems.
fn slot_name(doc: &Document, id: core_model::ids::PluginInstanceId) -> String {
    doc.plugin_slot(id)
        .map_or_else(|| format!("plugin {}", id.0), |s| s.plugin.name.clone())
}

/// A checked bounce request.
#[derive(Debug, Clone)]
pub(crate) struct Plan {
    /// The sequence.
    pub seq: SeqId,
    /// The range.
    pub start: Tick,
    /// The range's end (exclusive).
    pub end: Tick,
    /// The range's frames at the project rate.
    pub frames: u64,
    /// A Mix (else stems of `strips`).
    pub mix: bool,
    /// The stems' strips.
    pub strips: Vec<StripId>,
    /// The files' format.
    pub fmt: ExportFormat,
    /// The mix file.
    pub mix_path: Option<PathBuf>,
    /// Per stem with a `path`: `(base, file)` (Ruling 3).
    pub stem_files: Vec<(PathBuf, PathBuf)>,
    /// Replace an existing mix file.
    pub replace: bool,
    /// Add to project.
    pub add: bool,
    /// The names of the imports (the Mix, or each heard stem).
    pub import_names: Vec<String>,
    /// Lanes playing external MIDI in the range.
    pub external_lanes: u32,
    /// Regions in the range whose media is offline.
    pub offline_regions: usize,
    /// The project rate `frames` is counted at.
    pub project_rate: u32,
    /// Zero frames (at the project rate) each file stem starts with, before
    /// the render (plan 6b Ruling 2); 0 for a bounce.
    pub lead: u64,
}

impl Plan {
    /// The sources the render makes.
    pub(crate) fn sources(&self) -> usize {
        if self.mix {
            1
        } else {
            self.stem_files.len() + if self.add { self.strips.len() } else { 0 }
        }
    }

    /// "<n> regions are offline and will be silent." (pre-flight M5).
    pub fn offline_sentence(&self) -> Option<String> {
        match self.offline_regions {
            0 => None,
            1 => Some("1 region is offline and will be silent.".into()),
            n => Some(format!("{n} regions are offline and will be silent.")),
        }
    }

    /// The files it would write, in order.
    fn files(&self) -> Vec<String> {
        match &self.mix_path {
            Some(p) => vec![p.display().to_string()],
            None => self
                .stem_files
                .iter()
                .map(|(_, f)| f.display().to_string())
                .collect(),
        }
    }

    /// The check's row; `free` is the free space where the temporaries go.
    pub fn check_row(&self, tail_cap_ms: u32, free: Option<u64>) -> BounceCheckRow {
        let mut sentences = Vec::new();
        match self.external_lanes {
            0 => {}
            1 => sentences.push(
                "1 track plays external MIDI and will be silent. Track it as audio first: Track Synth…"
                    .to_string(),
            ),
            n => sentences.push(format!(
                "{n} tracks play external MIDI and will be silent. Track them as audio first: Track Synth…"
            )),
        }
        sentences.extend(self.offline_sentence());
        let tail = tail_frames(tail_cap_ms, self.project_rate);
        let per_source = self.frames.saturating_add(tail).saturating_mul(8);
        let need = per_source
            .saturating_mul(self.sources() as u64)
            .saturating_mul(if self.add { 2 } else { 1 });
        if let Some(free) = free.filter(|f| need > *f) {
            sentences.push(format!(
                "The bounce needs about {} MB of temporary space; {} MB are free.",
                need.div_ceil(1_000_000),
                free / 1_000_000
            ));
        }
        BounceCheckRow {
            external_lanes: self.external_lanes,
            frames: self.frames,
            files: self.files(),
            warning: (!sentences.is_empty()).then(|| sentences.join(" ")),
        }
    }
}

/// `ms` of tail at `rate` Hz, in frames.
fn tail_frames(ms: u32, rate: u32) -> u64 {
    u64::from(ms).saturating_mul(u64::from(rate)) / 1000
}

/// Checks a destination path's text (Ruling 3): at most 1024 bytes,
/// no NUL byte (final review m3), absolute, no `..`.
pub(crate) fn dest_path(s: &str) -> Result<PathBuf, VizError> {
    if s.len() > MAX_PATH_BYTES {
        return Err(model(format!(
            "the path is longer than {MAX_PATH_BYTES} bytes"
        )));
    }
    if s.contains('\0') {
        return Err(model("a path with a NUL character is not accepted"));
    }
    let p = PathBuf::from(s);
    if !p.is_absolute() {
        return Err(model(format!("{s}: choose an absolute path")));
    }
    if p.components().any(|c| matches!(c, Component::ParentDir)) {
        return Err(model(format!("{s}: a path with \"..\" is not accepted")));
    }
    Ok(p)
}

/// Refuses a `folder` that does not exist or lies inside one of `roots`
/// (the package and the scratch root) or inside any project package (a
/// `*.vizdaw` folder; final review T4-M10), after canonicalising it.
pub(crate) fn outside(folder: &Path, roots: &[PathBuf]) -> Result<(), VizError> {
    if !folder.is_dir() {
        return Err(model(format!("{}: no such folder", folder.display())));
    }
    let real = folder
        .canonicalize()
        .map_err(|e| model(format!("{}: {e}", folder.display())))?;
    for r in roots {
        let inside = r.canonicalize().is_ok_and(|c| real.starts_with(&c)) || real.starts_with(r);
        if inside {
            return Err(model(format!(
                "{} is inside the project; choose a place outside the project",
                folder.display()
            )));
        }
    }
    let package = |p: &Path| {
        p.extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("vizdaw"))
    };
    if real.ancestors().any(package) || folder.ancestors().any(package) {
        return Err(model(format!(
            "{} is inside a VizDAW project; choose a place outside the project",
            folder.display()
        )));
    }
    Ok(())
}

/// Refuses a file whose partial (`.<name>.partial`, written first) would
/// be longer than [`MAX_PATH_BYTES`] (final review m3).
pub(crate) fn partial_fits(file: &Path) -> Result<(), VizError> {
    if partial_path(file).as_os_str().len() > MAX_PATH_BYTES {
        return Err(model(format!(
            "{}: the path is longer than {MAX_PATH_BYTES} bytes with its partial file; choose a shorter one",
            display_name(file)
        )));
    }
    Ok(())
}

/// Checks `req` against `doc` (every refusal a `Model` error, nothing
/// written): the sequence, the strips, the range, the format, the
/// destinations (outside every one of `roots`) and that there is something
/// to do. `offline` is the session's offline media.
pub(crate) fn plan(
    doc: &Document,
    req: &BounceRequestRow,
    roots: &[PathBuf],
    offline: &BTreeMap<MediaId, crate::media::OfflineReasonRow>,
) -> Result<Plan, VizError> {
    let seq = SeqId(req.seq);
    doc.sequence(seq)?;
    let (mix, strips) = match &req.source {
        BounceSourceRow::Mix => (true, Vec::new()),
        BounceSourceRow::Stems { strips } => {
            if strips.is_empty() {
                return Err(model("choose at least one strip to bounce"));
            }
            (false, strips.iter().map(|s| StripId(*s)).collect())
        }
    };
    check_strips(doc, &strips)?;
    let (start, end) = match req.range {
        BounceRangeRow::Song => (0, core_model::query::seq_length(doc, seq)?.0),
        BounceRangeRow::Ticks { start, end } => (start, end),
    };
    if start < 0 || end > i64::MAX / 2 {
        return Err(model("the range lies outside the song"));
    }
    if start >= end {
        return Err(model("the range is empty"));
    }
    let (start, end) = (Tick(start), Tick(end));
    let fmt = export_format(req.container, req.depth, req.rate, req.dither);
    let common = common(doc, seq, start, end, &fmt, offline)?;
    // The CAF importer converts depth only: without the system converter a
    // file at another rate is refused now, not after the render (final
    // review m4).
    #[cfg(not(target_os = "macos"))]
    if req.path.is_some() {
        same_rate(&fmt, common.project_rate)?;
    }
    let ext = fmt.container.extension();
    let (mut mix_path, mut stem_files) = (None, Vec::new());
    if let Some(text) = &req.path {
        let p = dest_path(text)?;
        if mix {
            let parent = p
                .parent()
                .ok_or_else(|| model(format!("{text}: choose a file")))?;
            outside(parent, roots)?;
            partial_fits(&p)?;
            core_media::export::check_destination(&p, &fmt, req.replace)
                .map_err(|e| model(media_words(&e)))?;
            mix_path = Some(p);
        } else {
            if std::fs::symlink_metadata(&p).is_ok() && !p.is_dir() {
                return Err(model(format!("{text} is not a folder")));
            }
            outside(&p, roots)?;
            core_media::export::check_folder(&p).map_err(|e| model(media_words(&e)))?;
            let names: Vec<String> = strips.iter().map(|s| strip_name(doc, *s)).collect();
            stem_files = stem_plan(&p, &names, ext);
            for (_, f) in &stem_files {
                partial_fits(f)?;
            }
        }
    } else if !req.add_to_project {
        return Err(model("choose a file or Add to project"));
    }
    let import_names = if !req.add_to_project {
        Vec::new()
    } else if mix {
        vec![
            mix_path
                .as_deref()
                .and_then(Path::file_stem)
                .map_or_else(|| "Mix".to_string(), |s| s.to_string_lossy().into_owned()),
        ]
    } else {
        import_names(doc, &strips, &stem_files)
    };
    Ok(Plan {
        seq,
        start,
        end,
        frames: common.frames,
        mix,
        strips,
        fmt,
        mix_path,
        stem_files,
        replace: req.replace,
        add: req.add_to_project,
        import_names,
        external_lanes: common.external_lanes,
        offline_regions: common.offline_regions,
        project_rate: common.project_rate,
        lead: 0,
    })
}

/// The files' format from a request's fields.
pub(crate) fn export_format(
    container: ContainerRow,
    depth: BitDepthRow,
    rate: u32,
    dither: bool,
) -> ExportFormat {
    ExportFormat {
        container: container.into(),
        depth: match depth {
            BitDepthRow::Int16 => BitDepth::Int16,
            BitDepthRow::Int24 => BitDepth::Int24,
            BitDepthRow::Float32 => BitDepth::Float32,
        },
        rate,
        dither,
        dither_stream: 0,
    }
}

/// Refuses a rate other than the project's (no system converter).
#[cfg(not(target_os = "macos"))]
fn same_rate(fmt: &ExportFormat, project: u32) -> Result<(), VizError> {
    if fmt.rate != project {
        return Err(model(format!(
            "{project} Hz to {} Hz: resampling needs the system importer; bounce at {project} Hz",
            fmt.rate
        )));
    }
    Ok(())
}

/// Refuses an unknown strip, one listed twice, and more strips than the
/// mixer has.
fn check_strips(doc: &Document, strips: &[StripId]) -> Result<(), VizError> {
    if strips.len() > doc.mixer().order().len() {
        return Err(model("more strips than the mixer has"));
    }
    let mut seen = BTreeSet::new();
    for id in strips {
        doc.mixer()
            .strip(*id)
            .map_err(|_| model(format!("no strip {id}")))?;
        if !seen.insert(*id) {
            return Err(model(format!("strip {id} is listed twice")));
        }
    }
    Ok(())
}

/// What `plan` and `stems_plan` both work out for a range.
struct Common {
    frames: u64,
    external_lanes: u32,
    offline_regions: usize,
    project_rate: u32,
}

/// The range's frames at the project rate (refused when none), the format
/// checked, and the range's lanes on external MIDI and offline regions
/// (Ruling 8, pre-flight M5).
fn common(
    doc: &Document,
    seq: SeqId,
    start: Tick,
    end: Tick,
    fmt: &ExportFormat,
    offline: &BTreeMap<MediaId, crate::media::OfflineReasonRow>,
) -> Result<Common, VizError> {
    let s = doc.sequence(seq)?;
    fmt.check().map_err(|e| model(media_words(&e)))?;
    let project = doc.settings().format;
    let table = TempoTable::build(&s.tempo, -s.start_offset, f64::from(project.sample_rate));
    let frames = table
        .tick_to_frame(end)
        .saturating_sub(table.tick_to_frame(start));
    if frames == 0 {
        return Err(model("the range holds no audio frames"));
    }
    let mut external: BTreeSet<(SeqId, core_model::ids::LaneId)> = BTreeSet::new();
    let mut offline_regions = 0usize;
    for e in core_model::query::flatten(doc, seq, Some((start, end)), &ViewFilter::default())? {
        match &e.body {
            EventBody::Note { .. } => {
                let ext_dest = e
                    .dest
                    .and_then(|d| doc.instruments().get(&d))
                    .is_some_and(|d| d.kind.is_external());
                if let (true, Some(step)) = (ext_dest, e.path.last()) {
                    external.insert((step.seq, step.lane));
                }
            }
            EventBody::AudioRegion { media, .. } if offline.contains_key(media) => {
                offline_regions = offline_regions.saturating_add(1);
            }
            _ => {}
        }
    }
    Ok(Common {
        frames,
        external_lanes: u32::try_from(external.len()).unwrap_or(u32::MAX),
        offline_regions,
        project_rate: project.sample_rate,
    })
}

/// The imports' names: each file stem's file name without its extension,
/// else the strip's name.
fn import_names(doc: &Document, strips: &[StripId], files: &[(PathBuf, PathBuf)]) -> Vec<String> {
    strips
        .iter()
        .enumerate()
        .map(|(k, s)| {
            files
                .get(k)
                .and_then(|(_, f)| f.file_stem())
                .map_or_else(|| strip_name(doc, *s), |n| n.to_string_lossy().into_owned())
        })
        .collect()
}

/// A stems plan for an export for another DAW (plan 6b Rulings 2, 7):
/// `strips` rendered over `[start, end)` (`start` may be the pickup's
/// start, before bar 1) to `files`, named relative to the export folder
/// (`stem_files` is `(f, f)`; the job rebases them, F3), each file stem
/// starting with `lead` zero frames at the project rate. Nothing is
/// looked at on disk. With `add`, the heard stems are imported, named
/// after the files.
#[allow(clippy::too_many_arguments)]
pub(crate) fn stems_plan(
    doc: &Document,
    seq: SeqId,
    strips: &[StripId],
    start: Tick,
    end: Tick,
    fmt: ExportFormat,
    files: Vec<PathBuf>,
    lead: u64,
    add: bool,
    offline: &BTreeMap<MediaId, crate::media::OfflineReasonRow>,
) -> Result<Plan, VizError> {
    doc.sequence(seq)?;
    check_strips(doc, strips)?;
    if files.len() != strips.len() {
        return Err(VizError::Internal {
            message: "one file per strip".into(),
        });
    }
    if start >= end {
        return Err(model("the range is empty"));
    }
    let common = common(doc, seq, start, end, &fmt, offline)?;
    #[cfg(not(target_os = "macos"))]
    if !strips.is_empty() {
        same_rate(&fmt, common.project_rate)?;
    }
    let stem_files: Vec<(PathBuf, PathBuf)> = files.into_iter().map(|f| (f.clone(), f)).collect();
    let import_names = if add {
        import_names(doc, strips, &stem_files)
    } else {
        Vec::new()
    };
    Ok(Plan {
        seq,
        start,
        end,
        frames: common.frames,
        mix: false,
        strips: strips.to_vec(),
        fmt,
        mix_path: None,
        stem_files,
        replace: false,
        add,
        import_names,
        external_lanes: common.external_lanes,
        offline_regions: common.offline_regions,
        project_rate: common.project_rate,
        lead,
    })
}

/// The plugin copies a bounce renders with (Ruling 6).
pub(crate) struct Copies {
    /// The copies' insert factory.
    pub factory: core_host::shelf::ShelfFactory,
    /// Set once nothing is loading.
    pub ready: Arc<AtomicBool>,
    /// The copies that do not render offline.
    pub problems: Arc<Mutex<Vec<String>>>,
}

/// Everything a bounce job needs (all of it owned: it runs on its thread).
pub(crate) struct Job {
    /// The checked request.
    pub plan: Plan,
    /// The document as captured (Ruling 6).
    pub doc: Document,
    /// The online media.
    pub media: Vec<MediaFile>,
    /// The media root.
    pub root: PathBuf,
    /// Names no new media file may take.
    pub reserved: BTreeSet<String>,
    /// The project format (the render's rate, the imports' format).
    pub format: AudioFormat,
    /// The tail cap (Ruling 4).
    pub tail_cap_ms: u32,
    /// Problems known when it started.
    pub problems: Vec<String>,
    /// The plugin copies (None: the document has no plugin slots).
    pub copies: Option<Copies>,
    /// The progress at which it cancels itself (test hook).
    pub cancel_at: Option<f32>,
    /// The "Add to project" source whose import fails as a disk error
    /// would (test hook; None outside tests).
    pub fail_import_at: Option<usize>,
}

/// The job's progress cell, and the test hook's cancel point.
struct Progress<'a> {
    cell: &'a AtomicU32,
    cancel: &'a AtomicBool,
    at: Option<f32>,
}

impl Progress<'_> {
    fn set(&self, p: f32) {
        self.cell.store(p.to_bits(), Ordering::Relaxed);
        if self.at.is_some_and(|a| p >= a) {
            self.cancel.store(true, Ordering::Relaxed);
        }
    }

    fn cancelled(&self) -> bool {
        self.cancel.load(Ordering::Relaxed)
    }
}

/// Files removed when dropped: the temporary CAFs, and the imports until
/// they are handed to the session.
#[derive(Default)]
struct Owned(Vec<PathBuf>);

impl Drop for Owned {
    fn drop(&mut self) {
        for p in &self.0 {
            let _ = std::fs::remove_file(p);
        }
    }
}

/// A temporary CAF's name in `Media/.incoming/`.
fn temp_name() -> String {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    format!(
        "bounce-{:x}-{:x}-{:x}.caf",
        std::process::id(),
        nanos,
        NEXT.fetch_add(1, Ordering::Relaxed)
    )
}

/// The bounce engine's sinks (Ruling 8): MIDI is always silent.
fn sinks() -> Box<dyn SinkFactory> {
    #[cfg(target_os = "macos")]
    if !crate::host::offline_mode() {
        // macOS only: the DLS synth; other platforms bounce through the offline sinks
        return Box::new(SilentMidi(platform_macos::sinks::MacSinks::new()));
    }
    Box::new(SilentMidi(OfflineSinks::new(1 << 16)))
}

/// "1 sample clipped", "<n> samples clipped" (final review T4-M7).
fn clipped_words(n: u64) -> String {
    match n {
        1 => "1 sample clipped".into(),
        n => format!("{n} samples clipped"),
    }
}

/// The problem line for non-finite samples (final review T2-M4, T4-M7).
fn nonfinite_line(n: u64) -> String {
    match n {
        1 => "1 non-finite sample was rendered and silenced".into(),
        n => format!("{n} non-finite samples were rendered and silenced"),
    }
}

/// The problem line for disk underruns (final review T4-M7).
fn underrun_line(n: u64) -> String {
    match n {
        1 => "the disk fell behind once while bouncing".into(),
        n => format!("the disk fell behind {n} times while bouncing"),
    }
}

/// How the render ended.
enum Rendered {
    Done(BounceStats),
    Cancelled,
    Failed(String),
}

/// Renders the plan's sources into `temps` (one float CAF each, in source
/// order), adding what it learns to `problems`.
fn render(
    job: &Job,
    factory: Option<core_host::shelf::ShelfFactory>,
    temps: &mut Owned,
    problems: &mut Vec<String>,
    prog: &Progress<'_>,
) -> Rendered {
    let plan = &job.plan;
    let rate = f64::from(job.format.sample_rate);
    let mut cfg = EngineConfig::new(rate);
    cfg.max_frames = BLOCK;
    cfg.disk = DiskMode::Manual;
    let (mut engine, renderer) = match Engine::new(cfg, sinks()) {
        Ok(x) => x,
        Err(e) => return Rendered::Failed(e.to_string()),
    };
    match factory {
        Some(f) => engine.set_insert_factory(Box::new(f)),
        None => engine.set_insert_factory(Box::new(NoInserts)),
    }
    engine.set_media(job.media.clone());
    let report = match engine.load(&job.doc, plan.seq) {
        Ok(r) => r,
        Err(e) => return Rendered::Failed(e.to_string()),
    };
    for (id, _) in &report.graph.placeholders {
        problems.push(format!(
            "{} could not load for the bounce and was bypassed",
            slot_name(&job.doc, *id)
        ));
    }
    let mut out = OfflineBackend::new(rate, BLOCK);
    if let Err(e) = out.start(renderer) {
        return Rendered::Failed(e.to_string());
    }
    let files = plan.mix_path.is_some() || !plan.stem_files.is_empty();
    let spec = BounceSpec {
        mix: plan.mix,
        stems: if !plan.mix && files {
            plan.strips.clone()
        } else {
            Vec::new()
        },
        heard_stems: if !plan.mix && plan.add {
            plan.strips
                .iter()
                .map(|s| HeardStem::of(job.doc.mixer(), *s))
                .collect()
        } else {
            Vec::new()
        },
        start: plan.start,
        end: plan.end,
        tail_cap: tail_frames(job.tail_cap_ms, job.format.sample_rate),
    };
    let incoming = job.root.join(INCOMING_DIR);
    if let Err(e) = std::fs::create_dir_all(&incoming) {
        return Rendered::Failed(format!("{}: {e}", incoming.display()));
    }
    let mut writers = Vec::new();
    for _ in 0..plan.sources() {
        let p = incoming.join(temp_name());
        match CafWriter::create(
            &p,
            job.format.sample_rate,
            2,
            BitDepth::Float32,
            WriteMode::Normal,
        ) {
            Ok(w) => {
                temps.0.push(p);
                writers.push(w);
            }
            Err(e) => {
                // A writer that made its file but failed later left it.
                temps.0.push(p);
                return Rendered::Failed(media_words(&e));
            }
        }
    }
    // The file stems' leading silence (plan 6b Ruling 2): not the heard
    // stems, which land where their audio begins.
    if plan.lead > 0 && !plan.mix {
        let zeros = vec![0f32; BLOCK as usize * 2];
        for w in writers.iter_mut().take(plan.stem_files.len()) {
            let mut left = plan.lead;
            while left > 0 {
                let n = left.min(u64::from(BLOCK));
                let samples = usize::try_from(n.saturating_mul(2)).unwrap_or(usize::MAX);
                if let Err(e) = w.write(zeros.get(..samples).unwrap_or(&zeros)) {
                    return Rendered::Failed(media_words(&e));
                }
                left = left.saturating_sub(n);
            }
        }
    }
    let mut buf: Vec<f32> = Vec::new();
    let stats = core_engine::bounce::bounce(
        &mut engine,
        &mut out,
        &spec,
        BLOCK,
        &mut |k, l, r| {
            buf.clear();
            buf.extend(l.iter().zip(r).flat_map(|(a, b)| [*a, *b]));
            writers
                .get_mut(k)
                .ok_or_else(|| format!("no source {k}"))?
                .write(&buf)
                .map_err(|e| media_words(&e))
        },
        &mut |f| prog.set(0.8 * f.clamp(0.0, 1.0)),
        prog.cancel,
    );
    for (m, why) in engine.take_stream_errors() {
        let name = job
            .doc
            .media()
            .get(&m)
            .map_or_else(|| m.to_string(), |x| x.name.clone());
        problems.push(format!("{name}: {why}"));
    }
    // The copies' processors go back to their host with the engine.
    drop(out);
    drop(engine);
    let stats = match stats {
        Ok(s) => s,
        Err(BounceError::Cancelled) => return Rendered::Cancelled,
        Err(e) => return Rendered::Failed(e.to_string()),
    };
    for w in writers {
        if let Err(e) = w.finish() {
            return Rendered::Failed(media_words(&e));
        }
    }
    if stats.nonfinite > 0 {
        problems.push(nonfinite_line(stats.nonfinite));
    }
    if stats.underruns > 0 {
        problems.push(underrun_line(stats.underruns));
    }
    Rendered::Done(stats)
}

/// The job's body (Ruling 12).
pub(crate) fn run(mut job: Job, progress: &AtomicU32, cancel: &AtomicBool) -> Work {
    let prog = Progress {
        cell: progress,
        cancel,
        at: job.cancel_at,
    };
    let copies = job.copies.take();
    // 1. The copies load on the main thread (`plugins::tick_bounces`).
    if let Some(c) = &copies {
        let until = Instant::now() + READY_WAIT;
        while !c.ready.load(Ordering::SeqCst) && Instant::now() < until {
            if prog.cancelled() {
                return Work::Cancelled;
            }
            std::thread::sleep(READY_POLL);
        }
    }
    if prog.cancelled() {
        return Work::Cancelled;
    }
    let mut problems = std::mem::take(&mut job.problems);
    let (factory, copy_problems) = match copies {
        Some(c) => (Some(c.factory), Some(c.problems)),
        None => (None, None),
    };
    if let Some(p) = &copy_problems {
        problems.extend(crate::guard::lock(p).iter().cloned());
    }
    let plan = job.plan.clone();
    let target = plan.add.then_some(ImportTargetRow {
        seq: plan.seq.0,
        lane: None,
        tick: plan.start.0,
    });
    let failed = |problems: Vec<String>, why: String| Work::Bounce {
        files: Vec::new(),
        target,
        exported: Vec::new(),
        problems,
        failed: Some(why),
    };
    // 2–4. Render every source to a temporary float CAF (0–0.8).
    let mut temps = Owned::default();
    let stats = match render(&job, factory, &mut temps, &mut problems, &prog) {
        Rendered::Done(s) => s,
        Rendered::Cancelled => return Work::Cancelled,
        Rendered::Failed(why) => return failed(problems, why),
    };
    if prog.cancelled() {
        return Work::Cancelled;
    }
    let file_stems = if plan.mix { 0 } else { plan.stem_files.len() };
    // 5. "Add to project" imports the Mix or each heard stem (0.8–0.9).
    let mut imported = Owned::default();
    let mut files: Vec<(String, Result<Vec<Imported>, String>)> = Vec::new();
    // Clip lines join the problems only when the job succeeds, so a failed
    // import, export or rename never claims "added to the project"
    // (re-review n1).
    let mut clips: Vec<String> = Vec::new();
    if plan.add {
        let importer = crate::media::caf_importer();
        let n = plan.import_names.len().max(1) as f32;
        for (k, name) in plan.import_names.iter().enumerate() {
            let at = if plan.mix { 0 } else { file_stems + k };
            let Some(temp) = temps.0.get(at) else {
                return failed(problems, format!("{name}: no render"));
            };
            let r = if job.fail_import_at == Some(k) {
                Err(MediaError::Io("test".into()))
            } else {
                core_media::import::import_reserved(
                    importer.as_ref(),
                    temp,
                    &job.root,
                    job.format,
                    name,
                    &job.reserved,
                    &mut |f| prog.set(0.8 + 0.1 * (k as f32 + f.clamp(0.0, 1.0)) / n),
                    cancel,
                )
            };
            match r {
                Err(MediaError::Cancelled) => return Work::Cancelled,
                Ok(v) => {
                    imported.0.extend(v.iter().map(|i| job.root.join(&i.path)));
                    // The project format clips what the float render kept
                    // (final review I1; owner answer 1: clips are reported).
                    for i in v.iter().filter(|i| i.clipped > 0) {
                        clips.push(format!("{name}{ADDED_CLIPPED}{}", clipped_words(i.clipped)));
                    }
                    files.push((name.clone(), Ok(v)));
                }
                // One import that fails aborts the bounce (ruling T4-1,
                // Ruling 12): `imported` removes the imports already made
                // and `temps` the temporaries; no partial exists yet.
                Err(e) => return failed(problems, format!("{name}: {}", media_words(&e))),
            }
            if prog.cancelled() {
                return Work::Cancelled;
            }
        }
    }
    prog.set(0.9);
    // 6. Every export to its partial (0.9–1.0).
    let mut exports: Vec<(PathBuf, Option<PathBuf>, PathBuf)> = Vec::new();
    if let Some(p) = &plan.mix_path {
        if let Some(t) = temps.0.first() {
            exports.push((t.clone(), None, p.clone()));
        }
    }
    if !plan.mix {
        for (k, (base, f)) in plan.stem_files.iter().enumerate() {
            if let Some(t) = temps.0.get(k) {
                exports.push((t.clone(), Some(base.clone()), f.clone()));
            }
        }
    }
    let importer = crate::media::importer();
    let n = exports.len().max(1) as f32;
    let mut pending: Vec<(PendingExport, Option<PathBuf>)> = Vec::new();
    for (k, (temp, base, dest)) in exports.iter().enumerate() {
        let replace = base.is_none() && plan.replace;
        // Each file its own dither (final review m5): the source index.
        let fmt = ExportFormat {
            dither_stream: k as u64,
            ..plan.fmt
        };
        // A file stem holds its leading silence too (plan 6b Ruling 2).
        let frames = if base.is_some() {
            plan.lead.saturating_add(stats.frames)
        } else {
            stats.frames
        };
        let r = core_media::export::write_partial(
            importer.as_ref(),
            temp,
            dest,
            &fmt,
            Some(frames),
            replace,
            &mut |f| prog.set(0.9 + 0.1 * (k as f32 + f.clamp(0.0, 1.0)) / n),
            cancel,
        );
        match r {
            Ok(p) => pending.push((p, base.clone())),
            Err(MediaError::Cancelled) => return Work::Cancelled,
            Err(e) => {
                let name = display_name(dest);
                return failed(problems, format!("{name}: {}", media_words(&e)));
            }
        }
    }
    // 7. The last cancel; then the renames (Ruling 12).
    if prog.cancelled() {
        return Work::Cancelled;
    }
    let mut exported = Vec::new();
    for (p, base) in pending {
        let done = match &base {
            None => p.commit(),
            Some(b) => p.commit_numbered_from(b),
        };
        match done {
            Ok(e) => {
                let name = display_name(&e.path);
                if e.clipped > 0 {
                    problems.push(format!("{name}: {}", clipped_words(e.clipped)));
                }
                exported.push(BouncedFileRow {
                    path: e.path.display().to_string(),
                    frames: e.frames,
                    rate: plan.fmt.rate,
                    bits: plan.fmt.depth.label().to_string(),
                    channels: e.channels,
                    clipped: e.clipped,
                });
            }
            Err(e) => {
                // The files in place stay; the imports, the remaining
                // partials (dropped) and the temporaries go.
                return Work::Bounce {
                    files: Vec::new(),
                    target,
                    exported,
                    problems,
                    failed: Some(media_words(&e)),
                };
            }
        }
    }
    prog.set(1.0);
    // 8. The imports are the session's now; the temporaries go.
    imported.0.clear();
    problems.extend(clips);
    Work::Bounce {
        files,
        target,
        exported,
        problems,
        failed: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Final review T4-M7 and T2-M4: singular forms for one.
    #[test]
    fn counts_of_one_are_singular() {
        assert_eq!(clipped_words(1), "1 sample clipped");
        assert_eq!(clipped_words(2), "2 samples clipped");
        assert_eq!(
            nonfinite_line(1),
            "1 non-finite sample was rendered and silenced"
        );
        assert_eq!(
            nonfinite_line(3),
            "3 non-finite samples were rendered and silenced"
        );
        assert_eq!(underrun_line(1), "the disk fell behind once while bouncing");
        assert_eq!(
            underrun_line(4),
            "the disk fell behind 4 times while bouncing"
        );
    }
}
