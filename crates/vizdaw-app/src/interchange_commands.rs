//! The interchange File commands over the control channel (Swift
//! `InterchangeCommands`): `file.import_midi`, `file.export_midi`,
//! `file.export_midi_clip`, `file.export_for_daw`,
//! `file.import_dawproject`, `file.export_dawproject`, `file.import_aaf`,
//! `file.export_aaf` and `file.interchange_report`. The Open and Save
//! dialogs' answers come as parameters and are checked here, before the
//! core sees them; the core checks the destinations.
//!
//! [`request`] turns a call's arguments into an [`InterchangeRequest`] or
//! the call's refusal, with Swift's strings. Nothing here opens a panel,
//! makes a project, reads a file or renders: a dry run, and the first
//! read of a file an import makes a project for, are requests the app
//! runs off the main thread.

use core_control::error::{ControlError, ErrorCode};
use core_control::registry::ArgValue;
use serde_json::Value;
use vizdaw_ffi::{
    AafDepthRow, AafExportRequestRow, AafFpsRow, AafImportRequestRow, AafStereoRow, BitDepthRow,
    ChannelMapRow, ClipExportRequestRow, ContainerRow, DawprojectExportRequestRow,
    DawprojectImportRequestRow, DawprojectInfoRow, DawprojectMergeRequestRow, ForDawRequestRow,
    MergeStepRow, MidiImportRequestRow, RegionGainRow, ReportRow, SmfExportRequestRow, VizError,
};

use crate::document::DocumentModel;
use crate::editor::GESTURE_OPEN;
use crate::formats::{FALLBACK_FORMAT, flag, has, int, is_digits, parse_depth, text};
use crate::jobs::FileJob;
use crate::mixer::commands::thrown;
use crate::session::DocSession;

pub const IMPORT_MIDI: &str = "file.import_midi";
pub const EXPORT_MIDI: &str = "file.export_midi";
pub const EXPORT_CLIP: &str = "file.export_midi_clip";
pub const EXPORT_FOR_DAW: &str = "file.export_for_daw";
pub const IMPORT_DAWPROJECT: &str = "file.import_dawproject";
pub const EXPORT_DAWPROJECT: &str = "file.export_dawproject";
pub const IMPORT_AAF: &str = "file.import_aaf";
pub const EXPORT_AAF: &str = "file.export_aaf";
pub const REPORT: &str = "file.interchange_report";

/// Every command [`request`] parses.
pub const IDS: [&str; 9] = [
    IMPORT_MIDI,
    EXPORT_MIDI,
    EXPORT_CLIP,
    EXPORT_FOR_DAW,
    IMPORT_DAWPROJECT,
    EXPORT_DAWPROJECT,
    IMPORT_AAF,
    EXPORT_AAF,
    REPORT,
];

/// The title of a failed dry run.
pub const CHECK_FAILED: &str = "Export Check Failed";
/// The title of a failed `file.interchange_report` read.
pub const REPORT_UNAVAILABLE: &str = "Report Unavailable";
/// The title of a failed track read (a MIDI export of every track).
pub const TRACKS_UNAVAILABLE: &str = "Tracks Unavailable";
/// The title of a refused import into a project a command made, and of
/// a refused first read of the file.
pub const IMPORT_FAILED: &str = "Import Failed";
/// The rate of the project a MIDI import makes (at the default depth).
pub const MIDI_PROJECT_RATE: u32 = 48_000;

/// `mode` merge on a file that did not come from VizDAW.
pub const MERGE_NEEDS_VIZDAW: &str =
    "file.import_dawproject: `mode` merge needs a file that came from VizDAW.";
/// `project` names a project that is open.
pub const PROJECT_ALREADY_OPEN: &str =
    "file.import_dawproject: `project` is already open; leave out project to merge into it.";
/// `project` is not a folder.
pub const PROJECT_NOT_VIZDAW: &str = "file.import_dawproject: `project` is not a VizDAW project.";

/// What a call asks for, once its arguments are checked.
#[derive(Debug, Clone, PartialEq)]
pub enum InterchangeRequest {
    /// `file.interchange_report`: read the session's last report
    /// (refused: [`REPORT_UNAVAILABLE`]); answered as `interchange`,
    /// never with a sheet.
    Report,
    /// Start this job on the document; the reply waits for its end.
    Job(FileJob),
    /// A dry run, run off the main thread; its report answers as
    /// `interchange` with `written: []` (refused: [`CHECK_FAILED`]).
    Check(FileCheck),
    /// `file.import_midi` with `new_project`: read the file
    /// (`midi_file_info`) off the main thread; a refusal makes no project
    /// ([`IMPORT_FAILED`]). Else make a [`MIDI_PROJECT_RATE`] project at
    /// the default depth and import there ([`MidiIntoNewProject::request_for`]).
    MidiIntoNewProject(MidiIntoNewProject),
    /// `file.import_dawproject`: read the file (`dawproject_info`) off the
    /// main thread, then [`DawprojectRead::after_read`].
    ReadDawproject(DawprojectRead),
    /// `file.import_aaf` with `new_project` (the default): read the file
    /// (`aaf_info`) off the main thread, then make a project at its rate
    /// and import there; a refusal of either makes (keeps) no project.
    AafIntoNewProject(AafImportRequestRow),
}

/// A dry run's request (the path is "" when none was given).
#[derive(Debug, Clone, PartialEq)]
pub enum FileCheck {
    ExportMidi(SmfExportRequestRow),
    ExportMidiClip(ClipExportRequestRow),
    ForDaw(ForDawRequestRow),
    ExportDawproject(DawprojectExportRequestRow),
    ExportAaf(AafExportRequestRow),
}

impl FileCheck {
    /// Runs the check on `s` (slow: off the main thread): its report.
    pub fn run(&self, s: &dyn DocSession) -> Result<ReportRow, VizError> {
        match self.clone() {
            FileCheck::ExportMidi(r) => s.export_midi_check(r),
            FileCheck::ExportMidiClip(r) => s.export_midi_clip_check(r),
            FileCheck::ForDaw(r) => s.export_for_daw_check(r).map(|c| c.report),
            FileCheck::ExportDawproject(r) => s.export_dawproject_check(r),
            FileCheck::ExportAaf(r) => s.export_aaf_check(r),
        }
    }
}

/// A MIDI import into a project the command makes.
#[derive(Debug, Clone, PartialEq)]
pub struct MidiIntoNewProject {
    /// The import as checked on this document (`seq` 0 unless given).
    pub request: MidiImportRequestRow,
    /// No `seq` was given: import into the new project's shown sequence.
    pub pick_seq: bool,
}

impl MidiIntoNewProject {
    /// The request for the made project, whose shown sequence is `shown`.
    pub fn request_for(&self, shown: Option<u64>) -> MidiImportRequestRow {
        let mut r = self.request.clone();
        if self.pick_seq
            && let Some(s) = shown
        {
            r.seq = s;
        }
        r
    }
}

/// `file.import_dawproject`'s `mode`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DawprojectMode {
    New,
    Merge,
}

/// `file.import_dawproject` as checked before the file is read.
#[derive(Debug, Clone, PartialEq)]
pub struct DawprojectRead {
    /// The file.
    pub path: String,
    pub mode: Option<DawprojectMode>,
    /// The project to merge into (absolute).
    pub project: Option<String>,
    /// The export to compare with (an export ID's shape).
    pub snapshot: Option<String>,
    /// Preview the merge only.
    pub preview: bool,
    /// The first merge-only parameter given (`project`, `snapshot`,
    /// `preview`, in that order).
    pub merge_only: Option<&'static str>,
}

/// What `file.import_dawproject` does once the file is read.
#[derive(Debug, Clone, PartialEq)]
pub enum DawprojectNext {
    /// Make a project at `rate` (the default depth) and import into it; a
    /// refusal to start closes it again ([`IMPORT_FAILED`]).
    NewProject { path: String, rate: u32 },
    /// Merge into `project` (opened as `file.open` opens it, after
    /// [`check_merge_project`]), else the open project with the file's
    /// project ID, else the one holding its export ID, else the "Open"
    /// dialog naming `project` ([`merge_target_dialog`]). The job is
    /// [`DawprojectRead::merge_job`]; a refusal is titled "DAWproject
    /// Merge Failed".
    Merge { project: Option<String> },
}

impl DawprojectRead {
    /// The next step for the file read as `info`.
    pub fn after_read(&self, info: &DawprojectInfoRow) -> Result<DawprojectNext, ControlError> {
        match self.mode {
            None if info.from_vizdaw => Err(ControlError::dialog("Return", &["mode"])),
            Some(DawprojectMode::Merge) => {
                if !info.from_vizdaw {
                    return Err(invalid(MERGE_NEEDS_VIZDAW));
                }
                Ok(DawprojectNext::Merge {
                    project: self.project.clone(),
                })
            }
            _ => {
                if let Some(name) = self.merge_only {
                    return Err(merge_only_refusal(name));
                }
                Ok(DawprojectNext::NewProject {
                    path: self.path.clone(),
                    rate: info.sample_rate,
                })
            }
        }
    }

    /// The merge job for the file, dated `date` ([`merge_date_today`]).
    pub fn merge_job(&self, date: String) -> FileJob {
        FileJob::MergeDawproject(DawprojectMergeRequestRow {
            path: self.path.clone(),
            snapshot: self.snapshot.clone(),
            step: if self.preview {
                MergeStepRow::Preview
            } else {
                MergeStepRow::Apply
            },
            date,
        })
    }
}

/// Checks a merge's `project` before it is opened: one already open
/// (`open`: the store found it by path) or not a folder (one lstat; a
/// symbolic link is not) is refused.
pub fn check_merge_project(project: &str, open: bool) -> Result<(), ControlError> {
    if open {
        return Err(invalid(PROJECT_ALREADY_OPEN));
    }
    let folder = std::fs::symlink_metadata(project).is_ok_and(|m| m.is_dir());
    if !folder {
        return Err(invalid(PROJECT_NOT_VIZDAW));
    }
    Ok(())
}

/// No open project to merge into and no `project`.
pub fn merge_target_dialog() -> ControlError {
    ControlError::dialog("Open", &["project"])
}

fn invalid(message: &str) -> ControlError {
    ControlError::new(ErrorCode::InvalidParams, message)
}

fn merge_only_refusal(name: &str) -> ControlError {
    invalid(&format!("{IMPORT_DAWPROJECT}: `{name}` is for mode merge."))
}

fn unavailable(message: &str) -> ControlError {
    ControlError::new(ErrorCode::Unavailable, message)
}

/// Whether `text` has an export ID's shape: 36 bytes, each a lowercase
/// hex digit or a hyphen (the core checks the rest).
pub fn is_export_id_shape(text: &str) -> bool {
    text.len() == 36
        && text
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b) || b == b'-')
}

/// Whether `text` cannot be a path argument: a NUL, or over 4 KiB.
fn path_problem(text: &str) -> bool {
    text.len() > 4096 || text.contains('\0')
}

/// `channel_map`'s JSON as the core's rows: every element checked.
pub fn channel_map(text: &str, id: &str) -> Result<Vec<ChannelMapRow>, ControlError> {
    let bad = |why: &str| ControlError::params(id, why);
    let usage = "`channel_map` is a JSON array like [{\"group\":0,\"channel\":9,\"dest\":3}]";
    let Ok(Value::Array(list)) = serde_json::from_str::<Value>(text) else {
        return Err(bad(usage));
    };
    if list.len() > 256 {
        return Err(bad("`channel_map` lists at most 256 pairs"));
    }
    // A whole number (never a flag), as Swift's `NSNumber as? Int`.
    fn whole(v: Option<&Value>) -> Option<i64> {
        match v? {
            Value::Number(n) => n.as_i64().or_else(|| {
                n.as_f64()
                    .filter(|f| f.fract() == 0.0 && f.abs() < 9.0e15)
                    .map(|f| f as i64)
            }),
            _ => None,
        }
    }
    let mut rows = vec![];
    let mut seen = vec![];
    for element in &list {
        let Some(o) = element.as_object() else {
            return Err(bad(usage));
        };
        if o.keys()
            .any(|k| !["group", "channel", "dest"].contains(&k.as_str()))
        {
            return Err(bad(usage));
        }
        let (Some(g), Some(c)) = (whole(o.get("group")), whole(o.get("channel"))) else {
            return Err(bad(usage));
        };
        if !(0..=15).contains(&g) || !(0..=15).contains(&c) {
            return Err(bad("`channel_map`: group and channel are 0 to 15"));
        }
        if seen.contains(&(g, c)) {
            return Err(bad(&format!(
                "`channel_map` lists group {g} channel {c} twice"
            )));
        }
        seen.push((g, c));
        let dest = match o.get("dest") {
            None | Some(Value::Null) => None,
            d => match whole(d) {
                Some(n) if n >= 0 => Some(n as u64),
                _ => return Err(bad("`channel_map`: `dest` is a destination ID or null")),
            },
        };
        rows.push(ChannelMapRow {
            group: g as u8,
            channel: c as u8,
            dest,
        });
    }
    Ok(rows)
}

/// IDs separated by commas (each once): `None` for a malformed list,
/// `Err(n)` for one listed twice.
fn id_list(text: &str) -> Option<Result<Vec<u64>, u64>> {
    let mut out = vec![];
    for part in text.split(',') {
        let n = is_digits(part)
            .then(|| part.parse::<u64>().ok())
            .flatten()?;
        if out.contains(&n) {
            return Some(Err(n));
        }
        out.push(n);
    }
    Some(Ok(out))
}

/// `lanes` as track IDs (each once); none given: every track of `seq`.
fn tracks(
    args: &[(String, ArgValue)],
    seq: u64,
    id: &str,
    doc: &DocumentModel,
) -> Result<Vec<u64>, ControlError> {
    let Some(list) = text(id, args, "lanes")? else {
        return doc
            .session()
            .lanes(seq)
            .map(|l| l.iter().map(|l| l.id).collect())
            .map_err(|e| thrown(&e, TRACKS_UNAVAILABLE));
    };
    match id_list(list) {
        None => Err(ControlError::params(
            id,
            "`lanes` is track IDs separated by commas, e.g. \"1,2\"",
        )),
        Some(Err(n)) => Err(ControlError::params(
            id,
            format!("`lanes` lists track {n} twice"),
        )),
        Some(Ok(l)) => Ok(l),
    }
}

/// `seq`, else the shown sequence; `or_none` lets a call that needs none
/// (an import into a new project) go on with 0, replaced once it exists.
fn seq_arg(
    args: &[(String, ArgValue)],
    id: &str,
    doc: &DocumentModel,
    or_none: bool,
) -> Result<u64, ControlError> {
    if let Some(s) = int(id, args, "seq")? {
        return u64::try_from(s).map_err(|_| ControlError::params(id, "`seq` is a sequence ID"));
    }
    if or_none {
        return Ok(0);
    }
    doc.selected_seq()
        .ok_or_else(|| ControlError::new(ErrorCode::Failed, format!("{id}: no sequence is shown")))
}

/// The import `args` ask for on `doc`'s shown sequence (`seq` else).
fn import_midi_request(
    args: &[(String, ArgValue)],
    path: &str,
    doc: &DocumentModel,
) -> Result<MidiImportRequestRow, ControlError> {
    let id = IMPORT_MIDI;
    let fresh = flag(id, args, "new_project")?.unwrap_or(false);
    let lane = int(id, args, "lane")?;
    if fresh && lane.is_some() {
        return Err(ControlError::params(
            id,
            "`lane` names a track of this project; a new project has none",
        ));
    }
    let seq = seq_arg(args, id, doc, fresh)?;
    let lane = lane
        .map(|l| u64::try_from(l).map_err(|_| ControlError::params(id, "`lane` is a track ID")))
        .transpose()?;
    let channel_map = match text(id, args, "channel_map")? {
        Some(t) => channel_map(t, id)?,
        None => vec![],
    };
    Ok(MidiImportRequestRow {
        path: path.to_owned(),
        new_project: fresh,
        seq,
        lane,
        tick: int(id, args, "tick")?.unwrap_or(0),
        channel_map,
        split_channels: flag(id, args, "split_channels")?,
        mpe: flag(id, args, "mpe")?.unwrap_or(false),
        use_tempo: flag(id, args, "use_tempo")?.unwrap_or(false),
    })
}

/// The Standard MIDI File export `args` ask for; `path` is "" for a check.
fn smf_request(
    args: &[(String, ArgValue)],
    doc: &DocumentModel,
) -> Result<SmfExportRequestRow, ControlError> {
    let id = EXPORT_MIDI;
    let seq = seq_arg(args, id, doc, false)?;
    let lanes = tracks(args, seq, id, doc)?;
    let ppq = text(id, args, "ppq")?
        .unwrap_or("960")
        .parse::<u16>()
        .ok()
        .filter(|p| [480, 960, 15360].contains(p))
        .ok_or_else(|| ControlError::params(id, "`ppq` is 480, 960 or 15360"))?;
    Ok(SmfExportRequestRow {
        seq,
        lanes,
        path: text(id, args, "path")?.unwrap_or("").to_owned(),
        replace: flag(id, args, "replace")?.unwrap_or(false),
        type0: text(id, args, "type")? == Some("0"),
        ppq,
        mpe: flag(id, args, "mpe")?.unwrap_or(false),
    })
}

/// The MIDI Clip File export `args` ask for; `path` is "" for a check.
fn clip_request(
    args: &[(String, ArgValue)],
    doc: &DocumentModel,
) -> Result<ClipExportRequestRow, ControlError> {
    let id = EXPORT_CLIP;
    let seq = seq_arg(args, id, doc, false)?;
    let lanes = tracks(args, seq, id, doc)?;
    let (start, end) = (int(id, args, "start")?, int(id, args, "end")?);
    if let Some(s) = start {
        let Some(e) = end else {
            return Err(ControlError::params(id, "`start` needs `end`"));
        };
        if s >= e {
            return Err(ControlError::params(id, "`start` must be before `end`"));
        }
    }
    Ok(ClipExportRequestRow {
        seq,
        lanes,
        path: text(id, args, "path")?.unwrap_or("").to_owned(),
        replace: flag(id, args, "replace")?.unwrap_or(false),
        start,
        end,
    })
}

/// The export for another DAW `args` ask for; `path` is "" for a check.
/// Defaults: the shown sequence, WAV at the project's rate and depth,
/// the usual strips, dither on at 16 bits only.
fn for_daw_request(
    args: &[(String, ArgValue)],
    doc: &DocumentModel,
) -> Result<ForDawRequestRow, ControlError> {
    let id = EXPORT_FOR_DAW;
    let seq = seq_arg(args, id, doc, false)?;
    let strips = match text(id, args, "strips")? {
        None => None,
        Some("none") => Some(vec![]),
        Some(list) => match id_list(list) {
            None => {
                return Err(ControlError::params(
                    id,
                    "`strips` is strip IDs separated by commas, e.g. \"1,2\", or none",
                ));
            }
            Some(Err(n)) => {
                return Err(ControlError::params(
                    id,
                    format!("`strips` lists strip {n} twice"),
                ));
            }
            Some(Ok(l)) => Some(l),
        },
    };
    let base = doc
        .session()
        .project_info()
        .map_or(FALLBACK_FORMAT, |i| i.format);
    let container = match text(id, args, "container")? {
        Some("aiff") => ContainerRow::Aiff,
        Some("caf") => ContainerRow::Caf,
        _ => ContainerRow::Wav,
    };
    let depth = text(id, args, "depth")?
        .and_then(parse_depth)
        .unwrap_or(base.depth);
    let rate = match int(id, args, "rate")? {
        None => base.sample_rate,
        Some(r) => u32::try_from(r)
            .ok()
            .filter(|n| (8_000..=384_000).contains(n))
            .ok_or_else(|| ControlError::params(id, "`rate` is 8000 to 384000 Hz"))?,
    };
    let dither_arg = flag(id, args, "dither")?;
    let dither = depth != BitDepthRow::Float32 && dither_arg.unwrap_or(depth == BitDepthRow::Int16);
    Ok(ForDawRequestRow {
        seq,
        strips,
        container,
        depth,
        rate,
        dither,
        path: text(id, args, "path")?.unwrap_or("").to_owned(),
        add_to_project: flag(id, args, "add_to_project")?.unwrap_or(false),
    })
}

/// The DAWproject export `args` ask for; `path` is "" for a check. The
/// blocks follow the editor's block rule; region gain is applied unless
/// `region_gain` is `leave_out`.
fn dawproject_request(
    args: &[(String, ArgValue)],
    doc: &DocumentModel,
) -> Result<DawprojectExportRequestRow, ControlError> {
    let id = EXPORT_DAWPROJECT;
    let seq = seq_arg(args, id, doc, false)?;
    Ok(DawprojectExportRequestRow {
        seq,
        path: text(id, args, "path")?.unwrap_or("").to_owned(),
        replace: flag(id, args, "replace")?.unwrap_or(false),
        region_gain: if text(id, args, "region_gain")? == Some("leave_out") {
            RegionGainRow::LeaveOut
        } else {
            RegionGainRow::Apply
        },
        blocks: doc.layout().block_rule.rule(),
    })
}

/// The AAF export `args` ask for; `path` is "" for a check. Defaults: the
/// shown sequence, embedded 24-bit audio, mono pairs for stereo, start at
/// 00:00:00:00, 25 fps, clip gain and automation written.
fn aaf_request(
    args: &[(String, ArgValue)],
    doc: &DocumentModel,
) -> Result<AafExportRequestRow, ControlError> {
    let id = EXPORT_AAF;
    let bad = |why: &str| ControlError::params(id, why);
    let seq = seq_arg(args, id, doc, false)?;
    let path = text(id, args, "path")?.unwrap_or("");
    if path_problem(path) {
        return Err(bad(
            "`path` is a folder path of at most 4096 bytes with no NUL",
        ));
    }
    let depth = match text(id, args, "depth")? {
        None | Some("24") => AafDepthRow::Int24,
        Some("16") => AafDepthRow::Int16,
        _ => return Err(bad("`depth` is 16 or 24")),
    };
    let stereo = match text(id, args, "stereo")? {
        None | Some("mono_pairs") => AafStereoRow::MonoPairs,
        Some("stereo") => AafStereoRow::Stereo,
        _ => return Err(bad("`stereo` is mono_pairs or stereo")),
    };
    let hour_start = match text(id, args, "start_tc")? {
        None | Some("00:00:00:00") => false,
        Some("01:00:00:00") => true,
        _ => return Err(bad("`start_tc` is 00:00:00:00 or 01:00:00:00")),
    };
    let fps = match text(id, args, "fps")? {
        None | Some("25") => AafFpsRow::Fps25,
        Some("24") => AafFpsRow::Fps24,
        Some("29.97df") => AafFpsRow::Fps2997Drop,
        Some("30") => AafFpsRow::Fps30,
        _ => return Err(bad("`fps` is 24, 25, 29.97df or 30")),
    };
    Ok(AafExportRequestRow {
        seq,
        path: path.to_owned(),
        embed: flag(id, args, "embed")?.unwrap_or(true),
        depth,
        stereo,
        hour_start,
        fps,
        clip_gain: flag(id, args, "clip_gain")?.unwrap_or(true),
        automation: flag(id, args, "automation")?.unwrap_or(true),
    })
}

/// The AAF import `args` ask for (`path` given). `new_project` is true
/// unless given.
fn aaf_import_request(
    args: &[(String, ArgValue)],
    path: &str,
) -> Result<AafImportRequestRow, ControlError> {
    let id = IMPORT_AAF;
    let midi_path = text(id, args, "midi_path")?;
    let media_folder = text(id, args, "media_folder")?;
    for (name, value) in [
        ("path", Some(path)),
        ("midi_path", midi_path),
        ("media_folder", media_folder),
    ] {
        if value.is_some_and(path_problem) {
            return Err(ControlError::params(
                id,
                format!("`{name}` is a path of at most 4096 bytes with no NUL"),
            ));
        }
    }
    if media_folder.is_some_and(|f| !f.starts_with('/')) {
        return Err(ControlError::params(
            id,
            "`media_folder` must be an absolute path",
        ));
    }
    Ok(AafImportRequestRow {
        path: path.to_owned(),
        new_project: flag(id, args, "new_project")?.unwrap_or(true),
        midi_path: midi_path.map(str::to_owned),
        media_folder: media_folder.map(str::to_owned),
        join_pairs: flag(id, args, "join_pairs")?.unwrap_or(true),
    })
}

/// A path that answers its dialog: given and not empty.
fn given_path<'a>(
    id: &str,
    args: &'a [(String, ArgValue)],
) -> Result<Option<&'a str>, ControlError> {
    Ok(text(id, args, "path")?.filter(|p| !p.is_empty()))
}

fn save_dialog() -> ControlError {
    ControlError::dialog("Save", &["path"])
}

fn open_dialog() -> ControlError {
    ControlError::dialog("Open", &["path"])
}

/// What `id` (one of [`IDS`]) with `args` asks of document `doc`, or the
/// call's refusal. No session call but reads (the project format, a
/// sequence's tracks) happens here.
pub fn request(
    id: &str,
    args: &[(String, ArgValue)],
    doc: &DocumentModel,
) -> Result<InterchangeRequest, ControlError> {
    use InterchangeRequest as R;
    crate::commands::check_names(id, args)?;
    let dry_run = || flag(id, args, "dry_run").map(|d| d.unwrap_or(false));
    match id {
        REPORT => Ok(R::Report),
        IMPORT_MIDI => {
            let Some(path) = text(id, args, "path")? else {
                return Err(open_dialog());
            };
            let fresh = flag(id, args, "new_project")?.unwrap_or(false);
            if !fresh && doc.gesture_open() {
                return Err(unavailable(GESTURE_OPEN));
            }
            // Checked on this document's shown sequence before a project
            // is made.
            let request = import_midi_request(args, path, doc)?;
            if !fresh {
                return Ok(R::Job(FileJob::ImportMidi(request)));
            }
            Ok(R::MidiIntoNewProject(MidiIntoNewProject {
                request,
                pick_seq: !has(args, "seq"),
            }))
        }
        EXPORT_MIDI | EXPORT_CLIP => {
            let dry = dry_run()?;
            if !dry && text(id, args, "path")?.is_none() {
                return Err(save_dialog());
            }
            if id == EXPORT_MIDI {
                let r = smf_request(args, doc)?;
                Ok(if dry {
                    R::Check(FileCheck::ExportMidi(r))
                } else {
                    R::Job(FileJob::ExportMidi(r))
                })
            } else {
                let r = clip_request(args, doc)?;
                Ok(if dry {
                    R::Check(FileCheck::ExportMidiClip(r))
                } else {
                    R::Job(FileJob::ExportMidiClip(r))
                })
            }
        }
        EXPORT_FOR_DAW => {
            let dry = dry_run()?;
            let r = for_daw_request(args, doc)?;
            if dry {
                return Ok(R::Check(FileCheck::ForDaw(r)));
            }
            if given_path(id, args)?.is_none() {
                return Err(save_dialog());
            }
            Ok(R::Job(FileJob::ForDaw(r)))
        }
        IMPORT_DAWPROJECT => {
            let Some(path) = given_path(id, args)? else {
                return Err(open_dialog());
            };
            let mode = match text(id, args, "mode")? {
                None => None,
                Some("new") => Some(DawprojectMode::New),
                Some("merge") => Some(DawprojectMode::Merge),
                // Never taken as new.
                Some(_) => return Err(invalid(&format!("{id}: `mode` is new or merge."))),
            };
            let project = text(id, args, "project")?;
            if project.is_some_and(|p| !p.starts_with('/')) {
                return Err(invalid(&format!(
                    "{id}: `project` must be an absolute path."
                )));
            }
            // `project`, `snapshot` and `preview` mean nothing to a new
            // project.
            let merge_only = ["project", "snapshot", "preview"]
                .into_iter()
                .find(|n| has(args, n));
            if mode == Some(DawprojectMode::New)
                && let Some(name) = merge_only
            {
                return Err(merge_only_refusal(name));
            }
            let snapshot = text(id, args, "snapshot")?;
            if snapshot.is_some_and(|s| !is_export_id_shape(s)) {
                return Err(invalid(&format!(
                    "{id}: `snapshot` is an export ID (36 lowercase hex digits and hyphens)."
                )));
            }
            Ok(R::ReadDawproject(DawprojectRead {
                path: path.to_owned(),
                mode,
                project: project.map(str::to_owned),
                snapshot: snapshot.map(str::to_owned),
                preview: flag(id, args, "preview")?.unwrap_or(false),
                merge_only,
            }))
        }
        EXPORT_DAWPROJECT => {
            let dry = dry_run()?;
            let r = dawproject_request(args, doc)?;
            if dry {
                return Ok(R::Check(FileCheck::ExportDawproject(r)));
            }
            if given_path(id, args)?.is_none() {
                return Err(save_dialog());
            }
            Ok(R::Job(FileJob::ExportDawproject(r)))
        }
        IMPORT_AAF => {
            let Some(path) = given_path(id, args)? else {
                return Err(open_dialog());
            };
            let r = aaf_import_request(args, path)?;
            if r.new_project {
                return Ok(R::AafIntoNewProject(r));
            }
            if doc.gesture_open() {
                return Err(unavailable(GESTURE_OPEN));
            }
            Ok(R::Job(FileJob::ImportAaf(r)))
        }
        EXPORT_AAF => {
            let dry = dry_run()?;
            let r = aaf_request(args, doc)?;
            if dry {
                return Ok(R::Check(FileCheck::ExportAaf(r)));
            }
            if given_path(id, args)?.is_none() {
                return Err(save_dialog());
            }
            Ok(R::Job(FileJob::ExportAaf(r)))
        }
        _ => Err(ControlError::new(
            ErrorCode::UnknownId,
            format!("VizDAW has no handler for {id}."),
        )),
    }
}

/// The job a DAWproject import into a made project starts.
pub fn dawproject_import_job(path: &str) -> FileJob {
    FileJob::ImportDawproject(DawprojectImportRequestRow {
        path: path.to_owned(),
    })
}

/// The date a merge's request carries: "yyyy-MM-dd" (Gregorian) of the
/// instant `unix_secs` in a zone `offset_secs` east of UTC.
pub fn merge_date(unix_secs: i64, offset_secs: i64) -> String {
    let days = (unix_secs + offset_secs).div_euclid(86_400);
    // Howard Hinnant's civil-from-days.
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!("{y:04}-{m:02}-{d:02}")
}

/// Today's [`merge_date`] in the local time zone.
pub fn merge_date_today() -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() as i64);
    merge_date(now, local_offset(now))
}

/// The local zone's offset east of UTC at `unix_secs` (0 when unknown).
#[cfg(unix)]
fn local_offset(unix_secs: i64) -> i64 {
    let t = unix_secs as libc::time_t;
    // SAFETY: `localtime_r` writes only into `tm`, which outlives the call.
    let mut tm: libc::tm = unsafe { std::mem::zeroed() };
    let ok = unsafe { !libc::localtime_r(&t, &mut tm).is_null() };
    if ok { tm.tm_gmtoff as i64 } else { 0 }
}

#[cfg(not(unix))]
fn local_offset(_unix_secs: i64) -> i64 {
    0
}
