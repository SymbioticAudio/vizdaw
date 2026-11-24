//! The project formats as the control channel spells them (Swift
//! `ControlFormats`), the File commands' format requests (`file.new`,
//! `file.project_settings`), the typed reads of a `command.run`
//! argument list the File commands share, and the extension a save
//! panel's typed name must get.

use core_control::error::{ControlError, ErrorCode};
use core_control::registry::ArgValue;
use vizdaw_ffi::{BitDepthRow, ProjectFormatRow};

/// The refusal of a `rate` or `depth` that is not a project format.
pub const NOT_A_FORMAT: &str = "`rate` or `depth` is not a project format";

/// 48 kHz, 24-bit: what fills a format nothing else gives.
pub const FALLBACK_FORMAT: ProjectFormatRow = ProjectFormatRow {
    sample_rate: 48_000,
    depth: BitDepthRow::Int24,
};

/// `rate`/`depth` strings to a format, `base` filling what they leave
/// out; `None` when either is not a project rate or depth (Swift
/// `ControlFormats.parse`).
pub fn parse(
    rate: Option<&str>,
    depth: Option<&str>,
    base: ProjectFormatRow,
) -> Option<ProjectFormatRow> {
    let mut f = base;
    if let Some(r) = rate {
        let n: u32 = r.parse().ok()?;
        if !vizdaw_ffi::project_rates().contains(&n) {
            return None;
        }
        f.sample_rate = n;
    }
    if let Some(d) = depth {
        f.depth = parse_depth(d)?;
    }
    Some(f)
}

/// "16", "24" or "32f" as a depth.
pub fn parse_depth(d: &str) -> Option<BitDepthRow> {
    match d {
        "16" => Some(BitDepthRow::Int16),
        "24" => Some(BitDepthRow::Int24),
        "32f" => Some(BitDepthRow::Float32),
        _ => None,
    }
}

/// The format a call's `rate` and `depth` ask for, `base` filling what
/// they leave out.
pub fn format(
    id: &str,
    args: &[(String, ArgValue)],
    base: ProjectFormatRow,
) -> Result<Option<ProjectFormatRow>, ControlError> {
    Ok(parse(
        text(id, args, "rate")?,
        text(id, args, "depth")?,
        base,
    ))
}

/// `file.new`'s format: the default format with what `rate` and `depth`
/// change.
pub fn new_project_format(
    args: &[(String, ArgValue)],
    default: ProjectFormatRow,
) -> Result<ProjectFormatRow, ControlError> {
    format("file.new", args, default)?
        .ok_or_else(|| ControlError::new(ErrorCode::InvalidParams, NOT_A_FORMAT))
}

/// `file.project_settings`' new format, `base` (the project's format)
/// filling what is left out. Neither `rate` nor `depth` is the "Project
/// Settings" dialog.
pub fn project_settings_format(
    args: &[(String, ArgValue)],
    base: ProjectFormatRow,
) -> Result<ProjectFormatRow, ControlError> {
    let id = "file.project_settings";
    let given = text(id, args, "rate")?.is_some() || text(id, args, "depth")?.is_some();
    match format(id, args, base)? {
        Some(f) if given => Ok(f),
        _ if !given => Err(ControlError::dialog("Project Settings", &["rate"])),
        _ => Err(ControlError::new(ErrorCode::InvalidParams, NOT_A_FORMAT)),
    }
}

/// A stored `<rate>/<depth>` (the default format preference): the parts
/// given change 48 kHz / 24-bit; a damaged value is 48 kHz / 24-bit
/// (Swift `DocumentStore.defaultFormat`).
pub fn parse_stored(s: &str) -> ProjectFormatRow {
    let parts: Vec<&str> = s.split('/').filter(|p| !p.is_empty()).collect();
    parse(
        parts.first().copied(),
        parts.get(1).copied(),
        FALLBACK_FORMAT,
    )
    .unwrap_or(FALLBACK_FORMAT)
}

/// The path a save panel's typed name stands for: `path` as typed when
/// its extension is one of `accepted` (any case), else with `.ext`
/// added. The flag says it was added: the panel did not see that name,
/// so the app must ask before replacing a file there (inventory §7.3).
pub fn with_extension(path: &str, ext: &str, accepted: &[&str]) -> (String, bool) {
    let has = std::path::Path::new(path)
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| accepted.iter().any(|a| a.eq_ignore_ascii_case(e)));
    if has {
        (path.to_owned(), false)
    } else {
        (format!("{}.{ext}", path.trim_end_matches('/')), true)
    }
}

/// Whether `s` is one or more ASCII digits (Swift's `"0"..."9"` scalars).
pub(crate) fn is_digits(s: &str) -> bool {
    !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit())
}

fn arg<'a>(args: &'a [(String, ArgValue)], name: &str) -> Option<&'a ArgValue> {
    args.iter().find(|(n, _)| n == name).map(|(_, v)| v)
}

/// Whether argument `name` was given (any value).
pub(crate) fn has(args: &[(String, ArgValue)], name: &str) -> bool {
    arg(args, name).is_some()
}

/// Text argument `name`; another type is `invalid_params`.
pub(crate) fn text<'a>(
    id: &str,
    args: &'a [(String, ArgValue)],
    name: &str,
) -> Result<Option<&'a str>, ControlError> {
    match arg(args, name) {
        None => Ok(None),
        Some(ArgValue::Text(t)) => Ok(Some(t)),
        Some(_) => Err(ControlError::params(id, format!("`{name}` is text"))),
    }
}

/// Flag argument `name`; another type is `invalid_params`.
pub(crate) fn flag(
    id: &str,
    args: &[(String, ArgValue)],
    name: &str,
) -> Result<Option<bool>, ControlError> {
    match arg(args, name) {
        None => Ok(None),
        Some(ArgValue::Flag(f)) => Ok(Some(*f)),
        Some(_) => Err(ControlError::params(
            id,
            format!("`{name}` is true or false"),
        )),
    }
}

/// Integer argument `name`; another type is `invalid_params`.
pub(crate) fn int(
    id: &str,
    args: &[(String, ArgValue)],
    name: &str,
) -> Result<Option<i64>, ControlError> {
    match arg(args, name) {
        None => Ok(None),
        Some(ArgValue::Int(i)) => Ok(Some(*i)),
        Some(_) => Err(ControlError::params(
            id,
            format!("`{name}` is a whole number"),
        )),
    }
}
