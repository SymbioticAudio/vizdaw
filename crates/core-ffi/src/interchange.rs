//! MIDI file import and export for the app (plan 6a Task 7): the rows, the
//! request checks (every refusal a `Model` error, nothing written) and the
//! job bodies. The session runs the jobs and applies an import as one undo
//! step ([`crate::session::Session::import_midi`]).

use crate::bounce::{dest_path, outside, partial_fits};
use crate::error::VizError;
use crate::guard::panic_message;
use crate::jobs::Work;
use core_io::IoError;
use core_io::limits::{Limits, MAX_MIDI_FILE_BYTES};
use core_io::midi_in::{self, DestChoice, ImportOptions, MidiFileKind, MidiImport, Placement};
use core_io::midi_out::{self, ClipOptions, SmfOptions};
use core_io::report::{ItemKind, Report};
use core_io::write::write_partial;
use core_media::export::{display_name, partial_path};
use core_model::document::{Document, SongInfo};
use core_model::ids::{DestId, LaneId, SeqId};
use core_model::rules::MAX_TICKS;
use core_model::time::Tick;
use std::collections::{BTreeMap, BTreeSet};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};

// ---- rows ----

/// How a report item affected the result.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum ReportKindRow {
    /// The result differs from the input.
    Changed,
    /// Something in the input is not in the result.
    LeftOut,
    /// Worth knowing; nothing was lost.
    Note,
}

/// One kind of thing that happened, with how often and where.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct ReportItemRow {
    /// How it affected the result.
    pub kind: ReportKindRow,
    /// What it is.
    pub what: String,
    /// How many times it happened.
    pub count: u64,
    /// The first places it happened (at most 50).
    pub details: Vec<String>,
}

/// What an import or export did, for the report sheet (Ruling 15).
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct ReportRow {
    /// "Imported 12 tracks, 4 210 notes and the tempo map" (or "Exported …").
    pub summary: String,
    /// What was changed, left out or noted, in the order it happened.
    pub items: Vec<ReportItemRow>,
}

/// The project's song info (spec §6.4): 13 optional texts.
#[derive(Debug, Clone, Default, PartialEq, Eq, uniffi::Record)]
pub struct SongInfoRow {
    /// Title.
    pub title: Option<String>,
    /// Artist.
    pub artist: Option<String>,
    /// Album.
    pub album: Option<String>,
    /// Original artist.
    pub original_artist: Option<String>,
    /// Composer.
    pub composer: Option<String>,
    /// Songwriter.
    pub songwriter: Option<String>,
    /// Producer.
    pub producer: Option<String>,
    /// Arranger.
    pub arranger: Option<String>,
    /// Year.
    pub year: Option<String>,
    /// Genre.
    pub genre: Option<String>,
    /// Copyright.
    pub copyright: Option<String>,
    /// Website.
    pub website: Option<String>,
    /// Comment (may span lines).
    pub comment: Option<String>,
}

/// What kind of MIDI file it is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum MidiFileKindRow {
    /// Standard MIDI File, type 0.
    Smf0,
    /// Standard MIDI File, type 1.
    Smf1,
    /// MIDI Clip File.
    Clip,
}

/// One `(group, channel)` a file plays on (both 0-based).
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Record)]
pub struct ChannelUseRow {
    /// Group, 0–15.
    pub group: u8,
    /// Channel, 0–15.
    pub channel: u8,
    /// Notes on it.
    pub notes: u64,
    /// The destination suggested for it (Ruling 5); `None`: a new built-in synth.
    pub suggested: Option<u64>,
}

/// What the import sheet shows before importing.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct MidiFileInfoRow {
    /// The kind of file.
    pub kind: MidiFileKindRow,
    /// Tracks the import makes (with the default split).
    pub tracks: u32,
    /// The pairs its channel messages use, in order.
    pub channels: Vec<ChannelUseRow>,
    /// The file gives a tempo.
    pub has_tempo: bool,
    /// A track begins with an MPE Configuration Message.
    pub mpe_found: bool,
}

/// Where one `(group, channel)` plays.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Record)]
pub struct ChannelMapRow {
    /// Group, 0–15.
    pub group: u8,
    /// Channel, 0–15.
    pub channel: u8,
    /// The destination; `None`: a new built-in synth.
    pub dest: Option<u64>,
}

/// Import a MIDI file (SMF or MIDI Clip File).
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct MidiImportRequestRow {
    /// The file (absolute).
    pub path: String,
    /// Into this new, empty project (Ruling 6): no undo step.
    pub new_project: bool,
    /// The sequence.
    pub seq: u64,
    /// The drop track (`None`: new tracks at the end).
    pub lane: Option<u64>,
    /// Where file position 0 goes (a negative tick is 0).
    pub tick: i64,
    /// Destinations by pair; a pair not listed takes its suggestion.
    pub channel_map: Vec<ChannelMapRow>,
    /// Split tracks by channel; `None`: type 0 files only (Ruling 11).
    pub split_channels: Option<bool>,
    /// Decode every track as MPE.
    pub mpe: bool,
    /// Use the file's tempo and meter (always, into a sequence without
    /// events).
    pub use_tempo: bool,
}

/// Export a Standard MIDI File.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct SmfExportRequestRow {
    /// The sequence.
    pub seq: u64,
    /// The tracks (empty: every track).
    pub lanes: Vec<u64>,
    /// The file (absolute; ignored by the check).
    pub path: String,
    /// Replace an existing file (ignored by the check).
    pub replace: bool,
    /// Type 0 (one track) instead of type 1.
    pub type0: bool,
    /// Ticks per quarter: 480, 960 or 15 360.
    pub ppq: u16,
    /// Per-note expression as MPE.
    pub mpe: bool,
}

/// Export MIDI Clip Files, one per track.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct ClipExportRequestRow {
    /// The sequence.
    pub seq: u64,
    /// The tracks (at least one).
    pub lanes: Vec<u64>,
    /// A file for one track, a folder for several (ignored by the check).
    pub path: String,
    /// Replace existing files (ignored by the check).
    pub replace: bool,
    /// The range's start for every track (`None`: the pickup's start).
    pub start: Option<i64>,
    /// The range's end (`None`: the sequence's end).
    pub end: Option<i64>,
}

impl From<&SongInfo> for SongInfoRow {
    fn from(i: &SongInfo) -> Self {
        SongInfoRow {
            title: i.title.clone(),
            artist: i.artist.clone(),
            album: i.album.clone(),
            original_artist: i.original_artist.clone(),
            composer: i.composer.clone(),
            songwriter: i.songwriter.clone(),
            producer: i.producer.clone(),
            arranger: i.arranger.clone(),
            year: i.year.clone(),
            genre: i.genre.clone(),
            copyright: i.copyright.clone(),
            website: i.website.clone(),
            comment: i.comment.clone(),
        }
    }
}

impl From<SongInfoRow> for SongInfo {
    fn from(i: SongInfoRow) -> Self {
        SongInfo {
            title: i.title,
            artist: i.artist,
            album: i.album,
            original_artist: i.original_artist,
            composer: i.composer,
            songwriter: i.songwriter,
            producer: i.producer,
            arranger: i.arranger,
            year: i.year,
            genre: i.genre,
            copyright: i.copyright,
            website: i.website,
            comment: i.comment,
        }
    }
}

/// `r` as a row, its summary line starting with `verb`.
pub(crate) fn report_row(r: &Report, verb: &str) -> ReportRow {
    ReportRow {
        summary: r.summary_line(verb),
        items: r
            .items
            .iter()
            .map(|i| ReportItemRow {
                kind: match i.kind {
                    ItemKind::Changed => ReportKindRow::Changed,
                    ItemKind::LeftOut => ReportKindRow::LeftOut,
                    ItemKind::Note => ReportKindRow::Note,
                },
                what: i.what.clone(),
                count: i.count,
                details: i.details.clone(),
            })
            .collect(),
    }
}

/// The sheet's view of `imp` in `doc`.
pub(crate) fn file_info(doc: &Document, imp: &MidiImport) -> MidiFileInfoRow {
    let channels = midi_in::channels(imp)
        .into_iter()
        .map(|((group, channel), notes)| ChannelUseRow {
            group,
            channel,
            notes,
            suggested: match midi_in::suggested_dest(doc, group, channel) {
                DestChoice::Existing(d) => Some(d.0),
                DestChoice::NewBuiltin => None,
            },
        })
        .collect();
    MidiFileInfoRow {
        kind: match imp.kind {
            MidiFileKind::Smf0 => MidiFileKindRow::Smf0,
            MidiFileKind::Smf1 => MidiFileKindRow::Smf1,
            MidiFileKind::Clip => MidiFileKindRow::Clip,
        },
        tracks: u32::try_from(imp.tracks.len()).unwrap_or(u32::MAX),
        channels,
        has_tempo: imp.has_tempo,
        mpe_found: imp.tracks.iter().any(|t| t.mpe.is_some()),
    }
}

/// A reader's or writer's refusal as the app sees it.
pub(crate) fn io_error(e: IoError) -> VizError {
    match e {
        IoError::Refused(message) => VizError::Model { message },
        IoError::Io(message) => VizError::Io { message },
        IoError::Cancelled => VizError::Model {
            message: "Cancelled".into(),
        },
    }
}

fn model(message: impl Into<String>) -> VizError {
    VizError::Model {
        message: message.into(),
    }
}

// ---- request checks ----

/// The path text of a MIDI file to read or write, as `bounce` checks it.
pub(crate) fn file_path(text: &str) -> Result<PathBuf, VizError> {
    if text.is_empty() {
        return Err(model("choose a MIDI file"));
    }
    dest_path(text)
}

/// Sequence `seq`, or a refusal naming it.
fn sequence(doc: &Document, seq: u64) -> Result<SeqId, VizError> {
    doc.sequence(SeqId(seq))
        .map(|_| SeqId(seq))
        .map_err(|_| model(format!("sequence {seq} is not in this project")))
}

/// Track `lane` of `seq`, or a refusal naming it.
fn track(doc: &Document, seq: SeqId, lane: u64) -> Result<LaneId, VizError> {
    doc.lane(seq, LaneId(lane))
        .map(|_| LaneId(lane))
        .map_err(|_| model(format!("track {lane} is not in this sequence")))
}

/// `lanes` checked against `seq`: each once, each of it.
fn tracks(doc: &Document, seq: SeqId, lanes: &[u64]) -> Result<Vec<LaneId>, VizError> {
    let mut seen = BTreeSet::new();
    let mut out = Vec::with_capacity(lanes.len().min(1024));
    for l in lanes {
        let id = track(doc, seq, *l)?;
        if !seen.insert(*l) {
            return Err(model(format!("track {l} is listed twice")));
        }
        out.push(id);
    }
    Ok(out)
}

/// A checked import: the file, its parse options and where it lands.
pub(crate) struct ImportPlan {
    path: PathBuf,
    opts: ImportOptions,
    place: Placement,
}

/// Checks `req` against `doc` (nothing is read yet).
pub(crate) fn import_plan(
    doc: &Document,
    req: &MidiImportRequestRow,
) -> Result<ImportPlan, VizError> {
    let path = file_path(&req.path)?;
    let seq = sequence(doc, req.seq)?;
    let lane = req.lane.map(|l| track(doc, seq, l)).transpose()?;
    if !(-MAX_TICKS..=MAX_TICKS).contains(&req.tick) {
        return Err(model(format!("tick {} is outside the timeline", req.tick)));
    }
    if req.new_project && doc.sequences().values().any(|s| !s.lanes.is_empty()) {
        return Err(model("A new-project import needs an empty project."));
    }
    let mut channel_map = BTreeMap::new();
    for m in &req.channel_map {
        if m.group > 15 {
            return Err(model(format!(
                "channel map: group {} is not 0 to 15",
                m.group
            )));
        }
        if m.channel > 15 {
            return Err(model(format!(
                "channel map: channel {} is not 0 to 15",
                m.channel
            )));
        }
        let choice = match m.dest {
            None => DestChoice::NewBuiltin,
            Some(d) if doc.instruments().contains_key(&DestId(d)) => {
                DestChoice::Existing(DestId(d))
            }
            Some(d) => {
                return Err(model(format!(
                    "channel map: destination {d} is not in this project"
                )));
            }
        };
        if channel_map.insert((m.group, m.channel), choice).is_some() {
            return Err(model(format!(
                "channel map: group {} channel {} is listed twice",
                m.group, m.channel
            )));
        }
    }
    Ok(ImportPlan {
        path,
        opts: ImportOptions {
            split_channels: req.split_channels,
            mpe: req.mpe,
        },
        place: Placement {
            seq,
            lane,
            tick: Tick(req.tick),
            channel_map,
            use_tempo: req.use_tempo,
            new_project: req.new_project,
        },
    })
}

/// The export options of `req` (its path is not looked at; F2).
pub(crate) fn smf_options(
    doc: &Document,
    req: &SmfExportRequestRow,
) -> Result<SmfOptions, VizError> {
    let seq = sequence(doc, req.seq)?;
    let lanes = tracks(doc, seq, &req.lanes)?;
    if !matches!(req.ppq, 480 | 960 | 15_360) {
        return Err(model(format!(
            "{} ticks per quarter note: MIDI files are written at 480, 960 or 15 360 ticks per quarter note",
            req.ppq
        )));
    }
    Ok(SmfOptions {
        seq,
        lanes,
        type0: req.type0,
        ppq: req.ppq,
        mpe: req.mpe,
    })
}

/// A checked clip export: one file per track, with its name in a folder.
pub(crate) struct ClipPlan {
    opts: Vec<ClipOptions>,
    names: Vec<String>,
}

/// The clip exports of `req`, in the sequence's track order, the range
/// applying to every track (F24). The path is not looked at (F2).
pub(crate) fn clip_plan(doc: &Document, req: &ClipExportRequestRow) -> Result<ClipPlan, VizError> {
    let seq = sequence(doc, req.seq)?;
    if req.lanes.is_empty() {
        return Err(model("choose at least one track to export"));
    }
    let chosen: BTreeSet<LaneId> = tracks(doc, seq, &req.lanes)?.into_iter().collect();
    let s = doc.sequence(seq)?;
    let range = match (req.start, req.end) {
        (None, None) => None,
        (start, end) => {
            let a = start.unwrap_or(-s.start_offset.0);
            let b = match end {
                Some(b) => b,
                None => core_model::query::seq_length(doc, seq)?.0,
            };
            let inside = |t: i64| (-MAX_TICKS..=MAX_TICKS).contains(&t);
            if !inside(a) || !inside(b) {
                return Err(model("the range to export lies outside the timeline"));
            }
            if a >= b {
                return Err(model(format!("the range to export ({a} to {b}) is empty")));
            }
            Some((Tick(a), Tick(b)))
        }
    };
    let (mut opts, mut names) = (Vec::new(), Vec::new());
    for l in s.lanes.iter().filter(|l| chosen.contains(&l.id)) {
        opts.push(ClipOptions {
            seq,
            lane: l.id,
            range,
        });
        names.push(l.name.clone());
    }
    Ok(ClipPlan { opts, names })
}

/// "02 Keys.midi2": the clip file of the `k`th track (0-based) of several.
fn clip_name(k: usize, track: &str) -> String {
    format!(
        "{:02} {}.midi2",
        k.saturating_add(1),
        track.replace(['/', ':'], "-")
    )
}

/// Refuses `p` when it is taken (without `replace`, or by a folder) or its
/// partial is left from an earlier export (F23).
fn free(p: &Path, replace: bool) -> Result<(), VizError> {
    if std::fs::symlink_metadata(p).is_ok() {
        if p.is_dir() {
            return Err(model(format!(
                "{} is a folder; choose a file",
                display_name(p)
            )));
        }
        if !replace {
            return Err(model(format!(
                "{} exists; choose another name or replace it",
                display_name(p)
            )));
        }
    }
    let partial = partial_path(p);
    if std::fs::symlink_metadata(&partial).is_ok() {
        return Err(model(format!(
            "{} is left from an earlier export; remove it or choose another name",
            display_name(&partial)
        )));
    }
    Ok(())
}

/// The file an export writes, checked: absolute, outside `roots` and any
/// project, its partial's path short enough, and free.
pub(crate) fn file_dest(text: &str, replace: bool, roots: &[PathBuf]) -> Result<PathBuf, VizError> {
    let p = file_path(text)?;
    let parent = p
        .parent()
        .ok_or_else(|| model(format!("{text}: choose a file")))?;
    outside(parent, roots)?;
    partial_fits(&p)?;
    free(&p, replace)?;
    Ok(p)
}

/// The files a clip export writes: `req.path` for one track; for several,
/// `NN <track>.midi2` in the folder `req.path`, every name checked before
/// any is written.
pub(crate) fn clip_dests(
    plan: &ClipPlan,
    req: &ClipExportRequestRow,
    roots: &[PathBuf],
) -> Result<Vec<PathBuf>, VizError> {
    if plan.names.len() == 1 {
        return Ok(vec![file_dest(&req.path, req.replace, roots)?]);
    }
    let folder = file_path(&req.path)?;
    if std::fs::symlink_metadata(&folder).is_ok() && !folder.is_dir() {
        return Err(model(format!("{} is not a folder", req.path)));
    }
    outside(&folder, roots)?;
    let mut out = Vec::with_capacity(plan.names.len());
    for (k, name) in plan.names.iter().enumerate() {
        let f = folder.join(clip_name(k, name));
        partial_fits(&f)?;
        free(&f, req.replace)?;
        out.push(f);
    }
    Ok(out)
}

// ---- checks (the sheet's dry run) ----

/// The bytes and report of a Standard MIDI File export.
fn render_smf(doc: &Document, opts: &SmfOptions) -> Result<(Vec<Vec<u8>>, Report), IoError> {
    let (file, report) = midi_out::to_smf(doc, opts)?;
    Ok((vec![core_io::smf::write(&file)?], report))
}

/// The bytes of each clip file and their merged report.
fn render_clips(doc: &Document, opts: &[ClipOptions]) -> Result<(Vec<Vec<u8>>, Report), IoError> {
    let mut report = Report::default();
    let mut files = Vec::with_capacity(opts.len());
    for o in opts {
        let (clip, r) = midi_out::to_clip(doc, o)?;
        files.push(core_io::clip::write(&clip));
        report.merge(r);
    }
    Ok((files, report))
}

/// What a Standard MIDI File export of `opts` would report.
pub(crate) fn smf_check(doc: &Document, opts: &SmfOptions) -> Result<ReportRow, VizError> {
    let (_, report) = render_smf(doc, opts).map_err(io_error)?;
    Ok(report_row(&report, "Exported"))
}

/// What a clip export of `plan` would report.
pub(crate) fn clip_check(doc: &Document, plan: &ClipPlan) -> Result<ReportRow, VizError> {
    let (_, report) = render_clips(doc, &plan.opts).map_err(io_error)?;
    Ok(report_row(&report, "Exported"))
}

// ---- jobs ----

/// The message of a panic caught on a job thread.
fn internal(payload: &(dyn std::any::Any + Send)) -> String {
    format!("internal error: {}", panic_message(payload))
}

/// The import job: reads `plan.path` (a regular file of at most 64 MiB) and
/// parses it, both inside `catch_unwind`.
pub(crate) fn import_job(
    plan: ImportPlan,
) -> impl FnOnce(&AtomicU32, &AtomicBool) -> Work + Send + 'static {
    move |progress, cancel| {
        if cancel.load(Ordering::Relaxed) {
            return Work::Cancelled;
        }
        let ImportPlan { path, opts, place } = plan;
        let parsed = catch_unwind(AssertUnwindSafe(|| {
            let bytes = core_io::read::read_regular(&path, MAX_MIDI_FILE_BYTES)?;
            midi_in::parse(&bytes, &opts, &Limits::default(), cancel)
        }));
        let result = match parsed {
            Ok(Ok(imp)) => Ok(Box::new(imp)),
            Ok(Err(IoError::Cancelled)) => return Work::Cancelled,
            Ok(Err(e)) => Err(e.to_string()),
            Err(p) => Err(internal(p.as_ref())),
        };
        progress.store(1f32.to_bits(), Ordering::Relaxed);
        Work::ImportMidi { result, place }
    }
}

/// Writes every file's partial, then commits them in order (bounce Ruling
/// 12). `Ok(None)`: cancelled, with the partials removed. A commit failure
/// names the files already in place.
fn write_all(
    files: Vec<(PathBuf, Vec<u8>)>,
    replace: bool,
    cancel: &AtomicBool,
) -> Result<Option<Vec<String>>, String> {
    let mut pending = Vec::with_capacity(files.len());
    for (dest, bytes) in files {
        if cancel.load(Ordering::Relaxed) {
            return Ok(None);
        }
        pending.push(write_partial(&dest, &bytes, replace).map_err(|e| e.to_string())?);
    }
    if cancel.load(Ordering::Relaxed) {
        return Ok(None);
    }
    let mut done: Vec<String> = Vec::with_capacity(pending.len());
    for p in pending {
        match p.commit() {
            Ok(path) => done.push(path.display().to_string()),
            Err(e) if done.is_empty() => return Err(e.to_string()),
            Err(e) => {
                return Err(format!("{e} (already written: {})", done.join(", ")));
            }
        }
    }
    Ok(Some(done))
}

/// An export job: renders on the document as it was when the job started,
/// then writes `dests` in order, all inside `catch_unwind`.
fn export_job(
    render: impl FnOnce() -> Result<(Vec<Vec<u8>>, Report), IoError> + Send + 'static,
    dests: Vec<PathBuf>,
    replace: bool,
) -> impl FnOnce(&AtomicU32, &AtomicBool) -> Work + Send + 'static {
    move |progress, cancel| {
        if cancel.load(Ordering::Relaxed) {
            return Work::Cancelled;
        }
        let ran = catch_unwind(AssertUnwindSafe(|| {
            let (bytes, report) = render().map_err(|e| e.to_string())?;
            progress.store(0.5f32.to_bits(), Ordering::Relaxed);
            let written = write_all(dests.into_iter().zip(bytes).collect(), replace, cancel)?;
            Ok(written.map(|w| (w, report)))
        }));
        progress.store(1f32.to_bits(), Ordering::Relaxed);
        let result = match ran {
            Ok(Ok(Some(done))) => Ok(done),
            Ok(Ok(None)) => return Work::Cancelled,
            Ok(Err(why)) => Err(why),
            Err(p) => Err(internal(p.as_ref())),
        };
        Work::ExportMidi { result }
    }
}

/// The Standard MIDI File export job.
pub(crate) fn smf_job(
    doc: Document,
    opts: SmfOptions,
    dest: PathBuf,
    replace: bool,
) -> impl FnOnce(&AtomicU32, &AtomicBool) -> Work + Send + 'static {
    export_job(move || render_smf(&doc, &opts), vec![dest], replace)
}

/// The MIDI Clip File export job.
pub(crate) fn clip_job(
    doc: Document,
    plan: ClipPlan,
    dests: Vec<PathBuf>,
    replace: bool,
) -> impl FnOnce(&AtomicU32, &AtomicBool) -> Work + Send + 'static {
    export_job(move || render_clips(&doc, &plan.opts), dests, replace)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clip_names_number_and_clean_the_track() {
        assert_eq!(clip_name(0, "Keys"), "01 Keys.midi2");
        assert_eq!(clip_name(11, "A/B:C"), "12 A-B-C.midi2");
    }

    #[test]
    fn song_info_converts_both_ways() {
        let row = SongInfoRow {
            title: Some("T".into()),
            comment: Some("c".into()),
            website: Some("w".into()),
            ..SongInfoRow::default()
        };
        let info: SongInfo = row.clone().into();
        assert_eq!(SongInfoRow::from(&info), row);
    }
}
