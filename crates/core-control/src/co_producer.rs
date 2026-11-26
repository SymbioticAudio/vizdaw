//! The Co-Producer gate (Co-Producer spec §6.5–§6.7, plan 8a Rulings 5, 6,
//! 7 and 11, pre-flight P1–P3). A `Role::CoProducer` server runs every
//! validated call through [`admit`] before it reaches the app:
//!
//! - only `ping`, `commands.list`, `state.get` (not `input`), `wait`,
//!   `ui.screenshot` (window and scale only), `command.run` and the fifteen
//!   `co_producer.*` commands are allowed;
//! - `command.run` runs read, add and outward commands only; a change or
//!   remove goes through `co_producer.preview` and the composer's yes;
//! - a plan holds only the commands 8a can preview ([`PREVIEWABLE`]), with
//!   explicit targets;
//! - `replace: true` is refused everywhere, and every outward `path` is
//!   checked by [`check_output`] so no file is ever replaced.
//!
//! Every function here is total: hostile input gives an error, never a
//! panic.

use crate::error::{ControlError, ErrorCode};
use crate::registry::{self, ArgValue, COMMANDS, Class, CommandSpec};
use crate::request::{Command, StateTarget};
use serde_json::{Value, json};
use std::path::{Component, Path, PathBuf};

/// The change and remove commands Co-Producer can preview (8a Ruling 6;
/// 8b Ruling 9 adds `mixer.set_colour`; 8c Rulings 2 and 4 add the note
/// chores and the marker changes; 8d Ruling 1 adds `mixer.new_bus` with a
/// `strip` and the twelve mixer edits; 8e Ruling 1 adds `automation.write`;
/// 8e Task 6b adds the release-fixes fade shape, Built-in Synth sound and
/// track output; groups-vcas-transport Task 7 adds the fourteen group and
/// VCA commands, `registry::GROUP_IDS`, whose new group and new VCA preview
/// only with `strips`).
pub const PREVIEWABLE: &[&str] = &[
    "track.rename",
    "track.delete",
    "edit.undo",
    "edit.redo",
    "file.save",
    "mixer.set_colour",
    "modify.quantize_notes",
    "modify.transpose_notes",
    "modify.velocity_notes",
    "marker.rename",
    "marker.remove",
    "mixer.new_bus",
    "mixer.set_strip",
    "mixer.set_output",
    "mixer.add_send",
    "mixer.set_send",
    "mixer.remove_send",
    "mixer.add_insert",
    "mixer.remove_insert",
    "mixer.move_insert",
    "mixer.set_bypass",
    "mixer.set_eq",
    "mixer.set_param",
    "mixer.remove_bus",
    "automation.write",
    "region.set_fade_shape",
    "destination.set_sound",
    "lane.set_output",
    "mixer.new_group",
    "mixer.rename_group",
    "mixer.set_group_colour",
    "mixer.add_to_group",
    "mixer.remove_from_group",
    "mixer.set_group_links",
    "mixer.delete_group",
    "mixer.new_vca",
    "mixer.rename_vca",
    "mixer.set_vca_colour",
    "mixer.assign_vca",
    "mixer.unassign_vca",
    "mixer.set_vca",
    "mixer.delete_vca",
];

/// The adds Co-Producer cannot run in any form in 26.0.0 (release gate
/// B2): each writes notes, and Co-Producer writes none. `file.import_midi`
/// lands a file's notes on a track and `track.duplicate` copies a track's
/// notes. The developer channel keeps their registry class; the
/// Co-Producer gate ([`effective_class`]) and its list ([`list_json`])
/// both read this one list, so they cannot disagree.
pub const REFUSED_IN_26: &[&str] = &["file.import_midi", "track.duplicate"];

/// The most IDs in one [`id_list`] (plan 8c Rulings 1 and 2).
pub const MAX_IDS: usize = 64;

/// The largest ID an [`id_list`] takes, as the registry's ID parameters.
const MAX_ID: u64 = (i64::MAX / 2) as u64;

/// "1,2,3": 1–64 unique ids, each ≥ 1; the message names the rule broken.
/// Spaces around an ID are allowed; anything else but ASCII digits is not.
pub fn id_list(text: &str) -> Result<Vec<u64>, String> {
    if text.trim().is_empty() {
        return Err(format!("give 1 to {MAX_IDS} IDs, comma-separated"));
    }
    let mut ids = Vec::new();
    for part in text.split(',') {
        if ids.len() == MAX_IDS {
            return Err(format!("give at most {MAX_IDS} IDs"));
        }
        let part = part.trim();
        let id = if !part.is_empty() && part.bytes().all(|b| b.is_ascii_digit()) {
            part.parse::<u64>().ok().filter(|i| *i <= MAX_ID)
        } else {
            None
        };
        let Some(id) = id else {
            return Err(format!(
                "{:?} is not an ID: give 1 to {MAX_IDS} IDs, comma-separated",
                crate::protocol::truncate(part, 32)
            ));
        };
        if id == 0 {
            return Err("IDs start at 1".into());
        }
        if ids.contains(&id) {
            return Err(format!("ID {id} is given twice"));
        }
        ids.push(id);
    }
    Ok(ids)
}

/// The most commands in one plan (Ruling 11).
pub const MAX_PLAN: usize = 32;

/// The largest `ui.screenshot` scale Co-Producer may ask for (Ruling 11).
pub const MAX_SCREENSHOT_SCALE: f64 = 1.0;

/// One planned command of a preview or apply.
#[derive(Debug, Clone, PartialEq)]
pub struct Planned {
    /// Registry ID.
    pub id: String,
    /// Its validated arguments, in registry order.
    pub args: Vec<(String, ArgValue)>,
}

/// Ruling 11: how an outward command's `path` may be used.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Output {
    /// A new file that must not exist, ending in one of these extensions.
    NewFile(&'static [&'static str]),
    /// An existing folder that is not a link; the export makes a new
    /// numbered folder inside it.
    IntoFolder,
}

fn arg<'a>(args: &'a [(String, ArgValue)], name: &str) -> Option<&'a ArgValue> {
    args.iter().find(|(n, _)| n == name).map(|(_, v)| v)
}

fn text<'a>(args: &'a [(String, ArgValue)], name: &str) -> Option<&'a str> {
    match arg(args, name) {
        Some(ArgValue::Text(t)) => Some(t),
        _ => None,
    }
}

fn flag(args: &[(String, ArgValue)], name: &str) -> bool {
    matches!(arg(args, name), Some(ArgValue::Flag(true)))
}

/// The class of `spec` run with `args` on the Co-Producer socket (Ruling
/// 5, P3): a new bus with a send from the composer's `strip` is a change,
/// and so is a new group or VCA given `strips` (groups-vcas-transport
/// Ruling 10): an empty one is an add.
/// Release gate B2 refuses [`REFUSED_IN_26`] in every form, and an audio
/// import with a `lane`, whose region would play over what is on that
/// track; without one it goes on new tracks.
pub fn effective_class(spec: &CommandSpec, args: &[(String, ArgValue)]) -> Class {
    match spec.id {
        id if REFUSED_IN_26.contains(&id) => Class::Refused,
        "mixer.new_bus" if arg(args, "strip").is_some() => Class::Change,
        "mixer.new_group" | "mixer.new_vca" if arg(args, "strips").is_some() => Class::Change,
        "media.import" if arg(args, "lane").is_some() => Class::Refused,
        _ => spec.class,
    }
}

/// The rule for an outward command's `path` (Ruling 11, P1); None when `id`
/// is not outward.
pub fn output(id: &str, args: &[(String, ArgValue)]) -> Option<Output> {
    match id {
        "file.export_midi" => Some(Output::NewFile(&["mid"])),
        // The clip export's own Save panel writes `.midi2` (P1: no looser
        // than the composer's Save dialog) for one named track. Several
        // tracks, or every track (no `lanes`), go into an existing folder
        // (plan 8c): the app names each file and never replaces one.
        "file.export_midi_clip" => Some(match text(args, "lanes") {
            Some(l) if !l.contains(',') => Output::NewFile(&["midi2"]),
            _ => Output::IntoFolder,
        }),
        "file.export_dawproject" => Some(Output::NewFile(&["dawproject"])),
        "file.export_for_daw" | "file.export_aaf" => Some(Output::IntoFolder),
        "file.bounce" => Some(match text(args, "source") {
            Some("stems") => Output::IntoFolder,
            _ => Output::NewFile(match text(args, "format") {
                Some("wav") => &["wav"],
                Some("aiff") => &["aif", "aiff"],
                Some("caf") => &["caf"],
                _ => &["wav", "aif", "aiff", "caf"],
            }),
        }),
        _ => None,
    }
}

fn refuse(path: &Path, reason: &str) -> ControlError {
    ControlError::not_allowed(format!(
        "Co-Producer will not write {}: {reason}.",
        path.display()
    ))
}

/// Whether `path` is its own components joined again: no trailing `/`,
/// no `.` part and no doubled `/`.
pub fn is_normal(path: &Path) -> bool {
    path.as_os_str() == path.components().collect::<PathBuf>().as_os_str()
}

/// Whether a component names a `.vizdaw` package, ignoring case as APFS does.
fn is_package(c: Component<'_>) -> bool {
    match c {
        Component::Normal(n) => n.to_string_lossy().to_lowercase().ends_with(".vizdaw"),
        _ => false,
    }
}

/// Checks an outward `path` against `rule` (Ruling 11, P1). The folder the
/// new item goes in (the parent of a new file, or the folder itself) must
/// be an existing directory whose last component is not a link, and its
/// canonical path must not be inside any `.vizdaw` package. A new file
/// must not exist (by `symlink_metadata`, so a dangling link counts), and
/// needs one of the rule's extensions and no leading dot. Relative paths,
/// `..` and paths not in normal form ([`is_normal`]) are refused. Each refusal is `not_allowed`, naming the path.
pub fn check_output(path: &Path, rule: Output) -> Result<(), ControlError> {
    if !path.is_absolute() {
        return Err(refuse(path, "the path must be absolute"));
    }
    if path.components().any(|c| c == Component::ParentDir) {
        return Err(refuse(path, "the path must not hold `..`"));
    }
    // Before any link or "already there" check: a final `/` or `/.` makes
    // the system follow a final link, which `symlink_metadata` and
    // `parent` would not see (review I1).
    if !is_normal(path) {
        return Err(refuse(
            path,
            "the path must be in normal form, with no `.` part, no doubled `/` and no `/` at the end",
        ));
    }
    let folder = match rule {
        Output::NewFile(exts) => {
            let Some(name) = path.file_name().map(|n| n.to_string_lossy()) else {
                return Err(refuse(path, "the path names no file"));
            };
            if name.starts_with('.') {
                return Err(refuse(path, "the file name must not start with a dot"));
            }
            let ext = path.extension().map(|e| e.to_string_lossy());
            if !ext.as_deref().is_some_and(|e| exts.contains(&e)) {
                return Err(refuse(
                    path,
                    &format!("the file must end in .{}", exts.join(" or .")),
                ));
            }
            match std::fs::symlink_metadata(path) {
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Ok(_) => {
                    return Err(refuse(
                        path,
                        "something is already there, and Co-Producer never replaces a file",
                    ));
                }
                Err(e) => return Err(refuse(path, &format!("it cannot be checked ({e})"))),
            }
            match path.parent() {
                Some(p) if !p.as_os_str().is_empty() => p,
                _ => return Err(refuse(path, "the path has no folder")),
            }
        }
        Output::IntoFolder => path,
    };
    match std::fs::symlink_metadata(folder) {
        Ok(m) if m.file_type().is_symlink() => {
            return Err(refuse(path, "its folder is a link"));
        }
        Ok(m) if m.is_dir() => {}
        Ok(_) => return Err(refuse(path, "its folder is not a folder but a file")),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Err(refuse(path, "its folder does not exist"));
        }
        Err(e) => return Err(refuse(path, &format!("its folder cannot be checked ({e})"))),
    }
    let canonical = std::fs::canonicalize(folder)
        .map_err(|e| refuse(path, &format!("its folder cannot be checked ({e})")))?;
    if canonical.components().any(is_package) {
        return Err(refuse(path, "it is inside a project package"));
    }
    Ok(())
}

fn never_replaces(args: &[(String, ArgValue)]) -> Result<(), ControlError> {
    if flag(args, "replace") {
        return Err(ControlError::not_allowed(
            "Co-Producer never replaces a file.",
        ));
    }
    Ok(())
}

fn cannot_run(id: &str) -> ControlError {
    ControlError::not_allowed(format!("Co-Producer cannot run {id}."))
}

fn spec_of(id: &str) -> Result<&'static CommandSpec, ControlError> {
    registry::find(id).ok_or_else(|| registry::unknown(id))
}

/// A `command.run` on the Co-Producer socket.
fn admit_run(id: &str, args: &[(String, ArgValue)]) -> Result<(), ControlError> {
    let spec = spec_of(id)?;
    let class = effective_class(spec, args);
    if class == Class::Refused {
        return Err(cannot_run(id));
    }
    never_replaces(args)?;
    match class {
        Class::Read | Class::Add => Ok(()),
        Class::Change | Class::Remove => Err(ControlError::not_allowed(format!(
            "{id} changes your work: preview it with co_producer.preview and apply it with the composer's yes."
        ))),
        Class::Outward => match (output(id, args), text(args, "path")) {
            (Some(rule), Some(p)) => check_output(Path::new(p), rule),
            // No path: the registry's dialog rule applied in `validate`, or
            // the bounce goes on new tracks inside the project.
            _ => Ok(()),
        },
        Class::Refused => Err(cannot_run(id)),
    }
}

/// One planned command (Ruling 6, P2, P3).
fn admit_planned(p: &Planned) -> Result<(), ControlError> {
    let id = p.id.as_str();
    let spec = spec_of(id)?;
    never_replaces(&p.args)?;
    // A free bus is an add (8d Ruling 1): only its `strip` form previews.
    // So is an empty group or VCA (groups-vcas-transport Ruling 10).
    let free_add = effective_class(spec, &p.args) == Class::Add;
    if !PREVIEWABLE.contains(&id) || free_add {
        return Err(match effective_class(spec, &p.args) {
            Class::Change | Class::Remove => {
                ControlError::not_allowed(format!("Co-Producer cannot preview {id} yet."))
            }
            Class::Refused => cannot_run(id),
            Class::Read | Class::Add | Class::Outward => ControlError::not_allowed(format!(
                "{id} needs no preview: run it with command.run."
            )),
        });
    }
    match id {
        "track.rename" | "track.delete" | "lane.set_output"
            if arg(&p.args, "seq").is_none() || arg(&p.args, "lane").is_none() =>
        {
            Err(ControlError::params(
                id,
                "name the track's `seq` and `lane`",
            ))
        }
        // Not the composer's selection (plan 8e Task 6b).
        "region.set_fade_shape"
            if ["seq", "lane", "event"]
                .iter()
                .any(|n| arg(&p.args, n).is_none()) =>
        {
            Err(ControlError::params(
                id,
                "name the region's `seq`, `lane` and `event`",
            ))
        }
        "file.save" if arg(&p.args, "path").is_some() => Err(ControlError::not_allowed(
            "Co-Producer saves only over the song's own file.",
        )),
        _ => Ok(()),
    }
}

/// Whether the Co-Producer socket admits `c`, a validated command
/// (Rulings 5, 6 and 11). The screenshot rate is the server's.
pub fn admit(c: &Command) -> Result<(), ControlError> {
    match c {
        Command::CommandsList
        | Command::Ping
        | Command::Wait { .. }
        | Command::CoProducerBegin { .. }
        | Command::CoProducerEnd { .. }
        | Command::CoProducerStatus { .. }
        | Command::CoProducerNotes { .. }
        | Command::CoProducerNote { .. }
        | Command::CoProducerPlugins { .. }
        | Command::CoProducerParams { .. }
        | Command::CoProducerMidiNotes { .. }
        | Command::CoProducerAutomation { .. }
        // Plan 8f Ruling 6: the time-record read, with no request open.
        | Command::CoProducerMetrics { .. }
        | Command::CoProducerExportFolder { parent: None, .. }
        | Command::CoProducerExport { parent: None, .. } => Ok(()),
        // Plan 8b Ruling 6 (and 8c Ruling 1 for the one-call export): a
        // given parent is checked before the app sees it.
        Command::CoProducerExportFolder {
            parent: Some(parent),
            ..
        }
        | Command::CoProducerExport {
            parent: Some(parent),
            ..
        } => check_output(parent, Output::IntoFolder),
        // Plan 8e Ruling 5: the whole spec is checked before the app sees
        // it, so a bad spec opens no window.
        Command::CoProducerStarter { spec } => crate::starter::parse(spec)
            .map(|_| ())
            .map_err(|m| ControlError::params("co_producer.starter", m)),
        Command::StateGet { target, .. } => match target {
            StateTarget::Input => Err(ControlError::not_allowed(
                "Co-Producer does not listen: state.get input is not available to it.",
            )),
            _ => Ok(()),
        },
        Command::UiScreenshot {
            path, scale, live, ..
        } => {
            let bad = |m: &str| Err(ControlError::params("ui.screenshot", m));
            if path.is_some() {
                return bad("Co-Producer cannot choose where a screenshot goes: leave out `path`");
            }
            if *live {
                return bad("Co-Producer cannot take a live screenshot: leave out `live`");
            }
            if scale.is_some_and(|s| s > MAX_SCREENSHOT_SCALE) {
                return bad("`scale` must be at most 1.0 on the Co-Producer socket");
            }
            Ok(())
        }
        Command::UiTree { .. }
        | Command::UiClick { .. }
        | Command::UiDrag { .. }
        | Command::UiKey { .. }
        | Command::AppNewDemo { .. }
        | Command::AppOpen { .. }
        | Command::AppQuit { .. } => Err(ControlError::new(
            ErrorCode::NotAllowed,
            "Co-Producer cannot click, type, open or quit: use commands.list's commands.",
        )),
        Command::CommandRun { id, args, .. } => admit_run(id, args),
        Command::CoProducerPreview { plan, .. } | Command::CoProducerApply { plan, .. } => {
            plan.iter().try_for_each(admit_planned)
        }
    }
}

/// The Co-Producer's `commands.list`: every command it may run, each with
/// its `class` and whether 8a can preview it. A command [`effective_class`]
/// refuses with no args ([`REFUSED_IN_26`]) is not listed.
pub fn list_json() -> Value {
    let commands: Vec<Value> = COMMANDS
        .iter()
        .filter(|c| effective_class(c, &[]) != Class::Refused)
        .map(|c| {
            let mut v = registry::command_json(c);
            if let Some(o) = v.as_object_mut() {
                o.insert("previewable".into(), json!(PREVIEWABLE.contains(&c.id)));
            }
            v
        })
        .collect();
    json!({ "commands": commands })
}

fn arg_json(v: &ArgValue) -> Value {
    match v {
        ArgValue::Text(t) => json!(t),
        ArgValue::Flag(b) => json!(b),
        ArgValue::Int(i) => json!(i),
    }
}

/// Ruling 7's canonical text of a plan: each command's id and its args in
/// registry order, as JSON (`[{"id":…,"args":[[name, value],…]},…]`).
/// Two plans give the same text only when they are the same commands with
/// the same values in the same order.
pub fn canonical(plan: &[Planned]) -> String {
    let v: Vec<Value> = plan
        .iter()
        .map(|p| {
            let args: Vec<Value> = p
                .args
                .iter()
                .map(|(n, a)| json!([n, arg_json(a)]))
                .collect();
            json!({ "id": p.id, "args": args })
        })
        .collect();
    Value::Array(v).to_string()
}
