//! `file.bounce` over the control channel (Swift `BounceCommands`): the
//! Bounce dialog's answers as parameters, checked here before the core
//! sees them; the core checks the destinations.

use core_control::error::{ControlError, ErrorCode};
use core_control::registry::ArgValue;
use vizdaw_ffi::{BitDepthRow, BounceRangeRow, BounceRequestRow, BounceSourceRow, ContainerRow};

use crate::document::DocumentModel;
use crate::formats::{FALLBACK_FORMAT, flag, int, is_digits, parse_depth, text};
use crate::jobs::FileJob;

/// The command.
pub const ID: &str = "file.bounce";

fn bad(why: impl std::fmt::Display) -> ControlError {
    ControlError::params(ID, why)
}

/// The bounce `args` ask for on `doc`'s shown sequence: WAV at the
/// project's rate and depth by default, dither on at 16 bits (never at
/// 32-bit float). No `path` and no `add_to_project` (false counts as
/// none) is the "Bounce" dialog.
pub fn request(
    args: &[(String, ArgValue)],
    doc: &DocumentModel,
) -> Result<BounceRequestRow, ControlError> {
    let path = text(ID, args, "path")?;
    let add = flag(ID, args, "add_to_project")?.unwrap_or(false);
    if path.is_none() && !add {
        return Err(ControlError::dialog("Bounce", &["path"]));
    }
    let Some(seq) = doc.selected_seq() else {
        return Err(ControlError::new(
            ErrorCode::Failed,
            format!("{ID}: no sequence is shown"),
        ));
    };
    let base = doc
        .session()
        .project_info()
        .map_or(FALLBACK_FORMAT, |i| i.format);
    let source = text(ID, args, "source")?.unwrap_or("mix");
    let range_arg = text(ID, args, "range")?;
    if source == "selection" && range_arg == Some("song") {
        return Err(bad(
            "`source: selection` bounces the time selection; leave out `range: song`",
        ));
    }
    let container = match text(ID, args, "format")? {
        Some("aiff") => ContainerRow::Aiff,
        Some("caf") => ContainerRow::Caf,
        _ => ContainerRow::Wav,
    };
    let depth = text(ID, args, "bits")?
        .and_then(parse_depth)
        .unwrap_or(base.depth);
    let rate = int(ID, args, "rate")?
        .and_then(|r| u32::try_from(r).ok())
        .unwrap_or(base.sample_rate);
    let dither_arg = flag(ID, args, "dither")?;
    let dither = depth != BitDepthRow::Float32 && dither_arg.unwrap_or(depth == BitDepthRow::Int16);
    let mut src = BounceSourceRow::Mix;
    if source == "stems" {
        let Some(list) = text(ID, args, "strips")? else {
            return Err(bad("stems need `strips` (strip IDs, e.g. \"1,2\")"));
        };
        let mut ids: Vec<u64> = vec![];
        for part in list.split(',') {
            let Some(n) = is_digits(part).then(|| part.parse::<u64>().ok()).flatten() else {
                return Err(bad(
                    "`strips` is strip IDs separated by commas, e.g. \"1,2\"",
                ));
            };
            if ids.contains(&n) {
                return Err(bad(format!("strip {n} is listed twice")));
            }
            ids.push(n);
        }
        src = BounceSourceRow::Stems { strips: ids };
    } else if let Some(p) = path {
        // A pre-check only: the core's `Container::accepts` refuses again.
        let ext = std::path::Path::new(p)
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        let (fits, named) = match container {
            ContainerRow::Wav => (ext == "wav", "wav"),
            ContainerRow::Aiff => (ext == "aif" || ext == "aiff", "aif"),
            ContainerRow::Caf => (ext == "caf", "caf"),
        };
        if !fits {
            return Err(bad(format!("`path` must end in .{named} for this format")));
        }
    }
    let (start, end) = (int(ID, args, "start")?, int(ID, args, "end")?);
    let range = if source == "selection" || range_arg == Some("selection") {
        match (start, end) {
            (Some(s), Some(e)) => {
                if s >= e {
                    return Err(bad("`start` must be before `end`"));
                }
                BounceRangeRow::Ticks { start: s, end: e }
            }
            (None, None) => match doc.selection().time_range {
                Some(t) if t.start < t.end => BounceRangeRow::Ticks {
                    start: t.start,
                    end: t.end,
                },
                _ => return Err(bad("no time selection")),
            },
            _ => return Err(bad("give both `start` and `end`, or neither")),
        }
    } else {
        if start.is_some() || end.is_some() {
            return Err(bad("`start` and `end` go with `range: selection`"));
        }
        BounceRangeRow::Song
    };
    Ok(BounceRequestRow {
        seq,
        source: src,
        range,
        container,
        depth,
        rate,
        dither,
        path: path.map(str::to_owned),
        add_to_project: add,
        replace: flag(ID, args, "replace")?.unwrap_or(false),
    })
}

/// [`request`] as the job it starts.
pub fn job(args: &[(String, ArgValue)], doc: &DocumentModel) -> Result<FileJob, ControlError> {
    request(args, doc).map(FileJob::Bounce)
}
