//! Export for another DAW (plan 6b Task 2; spec §7.1, §7.6, §10): the
//! request and check rows, the request's checks (every refusal a `Model`
//! error, nothing looked at on disk, Ruling 8) and the job's body, which
//! wraps the bounce (Ruling 7).
//!
//! The job checks the destination and makes the staging folder
//! `.<folder>.partial` beside it, writes the MIDI files into it, renders
//! the stems into its `Audio/` through [`crate::bounce::run`], writes the
//! About file, looks at the cancel flag a last time and renames the folder
//! into place. Any failure or cancel removes the staging folder; the
//! "Add" imports are returned in every outcome after the render (F1), and
//! the session applies them only when the export is in place.

use crate::bounce::{self, ContainerRow, Plan, dest_path, outside, partial_fits};
use crate::error::VizError;
use crate::format::BitDepthRow;
use crate::guard::panic_message;
use crate::interchange::{ReportRow, io_error, report_row};
use crate::jobs::{ImportTargetRow, Work};
use core_engine::schedule::TempoTable;
use core_io::IoError;
use core_io::for_daw::{self as layout, AboutAudio, Layout};
use core_io::midi_out::{self, SmfOptions};
use core_io::report::{ItemKind, Report};
use core_io::write::{PendingFolder, free_folder, stage_folder, write_partial};
use core_media::export::{Container, partial_path};
use core_model::document::Document;
use core_model::ids::{MediaId, SeqId, StripId};
use core_model::time::Tick;
use std::collections::BTreeMap;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};

/// The MIDI files' resolution (Ruling 6).
const PPQ: u16 = 960;
/// The changed item that carries the render's problems (Ruling 10).
const RENDER_PROBLEMS: &str = "Problems while rendering the audio";
/// A cancel's `Err` in [`Work::ExportForDaw`].
pub(crate) const CANCELLED: &str = "Cancelled";

/// An export for another DAW, as the sheet (or `file.export_for_daw`)
/// asks for it.
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct ForDawRequestRow {
    /// The sequence.
    pub seq: u64,
    /// The strips whose stems are written: None, the defaults (Ruling 4);
    /// `Some([])`, MIDI files only.
    pub strips: Option<Vec<u64>>,
    /// The stems' container.
    pub container: ContainerRow,
    /// The stems' depth.
    pub depth: BitDepthRow,
    /// The stems' rate (8 000–384 000 Hz).
    pub rate: u32,
    /// TPDF dither at 16 and 24 bits.
    pub dither: bool,
    /// The folder the export folder goes in (Ruling 1); "" for a check
    /// that names no folder.
    pub path: String,
    /// Add the stems to this project, as heard, in one undo step "Add
    /// Stems".
    pub add_to_project: bool,
}

/// What an export for another DAW would do, before it starts.
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct ForDawCheckRow {
    /// "What won't transfer": the route's report (verb "Exported").
    pub report: ReportRow,
    /// The export folder's name, numbered when `path` is given.
    pub folder: String,
    /// The files, relative to the folder, in the order they are written.
    pub files: Vec<String>,
    /// The strips it would render (the defaults for None).
    pub strips: Vec<u64>,
    /// Where every file starts, as a sentence.
    pub start: String,
    /// The stems' frames before the tail, with the leading silence, at
    /// the export rate (F7).
    pub frames: u64,
    /// The bounce check's sentences (external MIDI, offline media, space).
    pub warning: Option<String>,
}

fn model(message: impl Into<String>) -> VizError {
    VizError::Model {
        message: message.into(),
    }
}

/// A checked request.
pub(crate) struct Checked {
    /// The folder the export folder goes in (None: a check without one).
    pub parent: Option<PathBuf>,
    /// The files, by path inside the export folder.
    pub layout: Layout,
    /// The stems' bounce, its files relative to the export folder.
    pub plan: Plan,
}

/// Checks `req` against `doc` (Ruling 8: text and the model only): the
/// sequence, the path's text (`need_path`: an export needs one), the
/// strips, the format, and "Add" with stems to add.
pub(crate) fn check(
    doc: &Document,
    req: &ForDawRequestRow,
    need_path: bool,
    offline: &BTreeMap<MediaId, crate::media::OfflineReasonRow>,
) -> Result<Checked, VizError> {
    let seq = SeqId(req.seq);
    let s = doc.sequence(seq)?;
    let parent = if req.path.is_empty() {
        if need_path {
            return Err(model("Choose where to export."));
        }
        None
    } else {
        Some(dest_path(&req.path)?)
    };
    if req.add_to_project && req.strips.as_ref().is_some_and(Vec::is_empty) {
        return Err(model(
            "Choose at least one strip to add its stem to the project.",
        ));
    }
    let strips: Vec<StripId> = match &req.strips {
        None => layout::default_strips(doc, seq).map_err(io_error)?,
        Some(v) => v.iter().map(|id| StripId(*id)).collect(),
    };
    let fmt = bounce::export_format(req.container, req.depth, req.rate, req.dither);
    let layout = layout::layout(doc, seq, &strips, fmt.container.extension()).map_err(io_error)?;
    let start = Tick(0i64.saturating_sub(s.start_offset.0));
    let end = core_model::query::seq_length(doc, seq)?;
    let lead = lead(doc, seq)?;
    let files = layout.audio.iter().map(|(_, f)| PathBuf::from(f)).collect();
    let plan = bounce::stems_plan(
        doc,
        seq,
        &strips,
        start,
        end,
        fmt,
        files,
        lead,
        req.add_to_project,
        offline,
    )?;
    Ok(Checked {
        parent,
        layout,
        plan,
    })
}

/// The frames, at the project rate, from the MIDI files' start
/// ([`midi_out::file_start`]) to the pickup's start, where the render
/// begins (Ruling 2, F7).
fn lead(doc: &Document, seq: SeqId) -> Result<u64, VizError> {
    let s = doc.sequence(seq)?;
    let rate = f64::from(doc.settings().format.sample_rate);
    let table = TempoTable::build(&s.tempo, midi_out::file_start(s), rate);
    Ok(table.tick_to_frame(Tick(0i64.saturating_sub(s.start_offset.0))))
}

/// `n` frames at `from` Hz as the export writes them at `to` Hz (rounded
/// as `core_media::export` scales a length).
fn scale(n: u64, from: u32, to: u32) -> u64 {
    if from == to || from == 0 {
        return n;
    }
    let (n, from, to) = (u128::from(n), u128::from(from), u128::from(to));
    u64::try_from(
        n.saturating_mul(to)
            .saturating_add(from / 2)
            .checked_div(from)
            .unwrap_or(0),
    )
    .unwrap_or(u64::MAX)
}

/// The report of the MIDI files and the route, without writing anything:
/// only All MIDI's report (the tempo map's without MIDI tracks) is
/// rendered (Ruling 6, F12).
fn route_report(
    doc: &Document,
    seq: SeqId,
    l: &Layout,
    strips: &[StripId],
) -> Result<Report, VizError> {
    let cancel = AtomicBool::new(false);
    let midi = if l.midi.is_empty() {
        midi_out::to_tempo_map_smf(doc, seq, PPQ, &cancel)
            .map_err(io_error)?
            .1
    } else {
        let opts = SmfOptions {
            seq,
            lanes: l.midi.iter().map(|(x, _)| *x).collect(),
            type0: false,
            ppq: PPQ,
            mpe: true,
        };
        midi_out::to_smf_until(doc, &opts, &cancel)
            .map_err(io_error)?
            .1
    };
    Ok(layout::route_report(doc, seq, strips, midi))
}

/// The check's row (run off the session lock: it looks at `c.parent` with
/// lstat and renders the MIDI report). `roots`: where exports may not go;
/// `free`: the free space where the temporaries go.
pub(crate) fn check_row(
    doc: &Document,
    c: &Checked,
    roots: &[PathBuf],
    tail_cap_ms: u32,
    free: Option<u64>,
) -> Result<ForDawCheckRow, VizError> {
    let folder = match &c.parent {
        Some(p) => {
            outside(p, roots)?;
            let dest = free_folder(p, &c.layout.folder).map_err(io_error)?;
            dest.file_name().map_or_else(
                || c.layout.folder.clone(),
                |n| n.to_string_lossy().into_owned(),
            )
        }
        None => c.layout.folder.clone(),
    };
    let report = route_report(doc, c.plan.seq, &c.layout, &c.plan.strips)?;
    let s = doc.sequence(c.plan.seq)?;
    Ok(ForDawCheckRow {
        report: report_row(&report, "Exported"),
        folder,
        files: c.layout.files(),
        strips: c.plan.strips.iter().map(|s| s.0).collect(),
        start: layout::start_sentence(s),
        frames: scale(
            c.plan.lead.saturating_add(c.plan.frames),
            c.plan.project_rate,
            c.plan.fmt.rate,
        ),
        warning: c.plan.check_row(tail_cap_ms, free).warning,
    })
}

/// Everything the job needs (all of it owned: it runs on its thread).
pub(crate) struct ForDawJob {
    /// The stems' bounce, its files relative to the export folder; its
    /// document is the one the MIDI files are made from.
    pub bounce: bounce::Job,
    /// The folder the export folder goes in.
    pub parent: PathBuf,
    /// Where exports may not go (the package, the scratch root).
    pub roots: Vec<PathBuf>,
    /// The files.
    pub layout: Layout,
    /// The stage after which it cancels itself (test hook: 1 after the
    /// first MIDI file, 2 after the About file).
    pub cancel_at: Option<u32>,
}

/// How a step of the job ended early.
enum Stop {
    Cancelled,
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

impl From<VizError> for Stop {
    fn from(e: VizError) -> Stop {
        Stop::Failed(message(e))
    }
}

/// A refusal's own sentence.
fn message(e: VizError) -> String {
    match e {
        VizError::Model { message }
        | VizError::Io { message }
        | VizError::Busy { message }
        | VizError::Internal { message } => message,
        other => other.to_string(),
    }
}

/// Runs `f` inside `catch_unwind`: a panic fails the job as a bug.
fn guarded<T>(f: impl FnOnce() -> Result<T, Stop>) -> Result<T, Stop> {
    catch_unwind(AssertUnwindSafe(f)).unwrap_or_else(|p| {
        Err(Stop::Failed(format!(
            "internal error: {}",
            panic_message(p.as_ref())
        )))
    })
}

/// Step 1: the destination checked (in the job, Ruling 8), the folder's
/// free name, every file's partial short enough, and the staging folder
/// made with `Audio/` and `MIDI/` inside.
fn stage(parent: &Path, roots: &[PathBuf], l: &Layout) -> Result<PendingFolder, Stop> {
    outside(parent, roots)?;
    let dest = free_folder(parent, &l.folder)?;
    let staging = partial_path(&dest);
    for f in l.files() {
        partial_fits(&staging.join(&f))?;
        partial_fits(&dest.join(&f))?;
    }
    let pending = stage_folder(&dest)?;
    for sub in ["Audio", "MIDI"] {
        let p = pending.path().join(sub);
        std::fs::create_dir(&p).map_err(|e| Stop::Failed(format!("{}: {e}", p.display())))?;
    }
    Ok(pending)
}

/// Writes `bytes` to `rel` in the staging folder (never replacing).
fn put(folder: &Path, rel: &str, bytes: &[u8]) -> Result<(), Stop> {
    write_partial(&folder.join(rel), bytes, false)?.commit()?;
    Ok(())
}

/// The container's name for the About file.
fn container_name(c: Container) -> &'static str {
    match c {
        Container::Wav => "WAV",
        Container::Aiff => "AIFF",
        Container::Caf => "CAF",
    }
}

/// The route's report with the render's problems as one changed item
/// (Ruling 10).
fn full_report(doc: &Document, plan: &Plan, midi: Report, problems: &[String]) -> Report {
    let mut r = layout::route_report(doc, plan.seq, &plan.strips, midi);
    for p in problems {
        r.add(ItemKind::Changed, RENDER_PROBLEMS, Some(p.clone()));
    }
    r
}

/// The job's body (Ruling 7).
pub(crate) fn run(job: ForDawJob, progress: &AtomicU32, cancel: &AtomicBool) -> Work {
    let ForDawJob {
        bounce: mut bj,
        parent,
        roots,
        layout: l,
        cancel_at,
    } = job;
    let target = bj.plan.add.then_some(ImportTargetRow {
        seq: bj.plan.seq.0,
        lane: None,
        tick: bj.plan.start.0,
    });
    let end = |files, result: Result<Report, String>, written| Work::ExportForDaw {
        files,
        target,
        result,
        written,
    };
    let stopped = |files, s: Stop| match s {
        Stop::Cancelled => end(files, Err(CANCELLED.to_string()), Vec::new()),
        Stop::Failed(why) => end(files, Err(why), Vec::new()),
    };
    let cancelled = || cancel.load(Ordering::Relaxed);
    if cancelled() {
        return stopped(Vec::new(), Stop::Cancelled);
    }
    // 1–2. The staging folder and the MIDI files (fast, so it fails early).
    let doc = bj.doc.clone();
    let seq = bj.plan.seq;
    let staged = guarded(|| {
        let pending = stage(&parent, &roots, &l)?;
        let (files, midi) = layout::midi_files(&doc, seq, &l, cancel)?;
        for (k, (rel, bytes)) in files.iter().enumerate() {
            if cancelled() {
                return Err(Stop::Cancelled);
            }
            put(pending.path(), rel, bytes)?;
            if k == 0 && cancel_at == Some(1) {
                cancel.store(true, Ordering::Relaxed);
            }
        }
        if cancelled() {
            return Err(Stop::Cancelled);
        }
        Ok((pending, midi))
    });
    let (pending, midi) = match staged {
        Ok(x) => x,
        Err(s) => return stopped(Vec::new(), s),
    };
    // 3. The stems, into `Audio/` (F3: the plan's paths are rebased here,
    // inside the staging folder).
    let folder = pending.path().to_path_buf();
    for (base, f) in &mut bj.plan.stem_files {
        *base = folder.join(&*base);
        *f = folder.join(&*f);
    }
    let plan = bj.plan.clone();
    let (imports, problems, frames) = if plan.strips.is_empty() {
        (Vec::new(), Vec::new(), None)
    } else {
        match bounce::run(bj, progress, cancel) {
            Work::Bounce {
                files,
                exported,
                problems,
                failed: None,
                ..
            } => (files, problems, exported.first().map(|e| e.frames)),
            Work::Bounce {
                failed: Some(why), ..
            } => return stopped(Vec::new(), Stop::Failed(why)),
            _ => return stopped(Vec::new(), Stop::Cancelled),
        }
    };
    // From here the imports are this job's (F1): returned in every outcome.
    if cancelled() {
        return stopped(imports, Stop::Cancelled);
    }
    // 4. The About file.
    let written = guarded(|| {
        let report = full_report(&doc, &plan, midi, &problems);
        let audio = frames.map(|frames| AboutAudio {
            container: container_name(plan.fmt.container),
            bits: plan.fmt.depth.label().to_string(),
            rate: plan.fmt.rate,
            frames,
        });
        let text = layout::about_text(&doc, seq, &l, audio.as_ref(), &report);
        put(&folder, &l.about, text.as_bytes())?;
        if cancel_at == Some(2) {
            cancel.store(true, Ordering::Relaxed);
        }
        Ok(report)
    });
    let report = match written {
        Ok(r) => r,
        Err(s) => return stopped(imports, s),
    };
    // 5. The last cancel; 6. the rename.
    if cancelled() {
        return stopped(imports, Stop::Cancelled);
    }
    match pending.commit() {
        Ok(dest) => {
            progress.store(1f32.to_bits(), Ordering::Relaxed);
            end(imports, Ok(report), vec![dest.display().to_string()])
        }
        Err(e) => stopped(imports, e.into()),
    }
}
