//! The Co-Producer gate (Co-Producer spec §6.5–§6.7, plan 8a Rulings 5, 6,
//! 7 and 11, pre-flight P1–P3). A `Role::CoProducer` server runs every
//! validated call through [`admit`] before it reaches the app:
//!
//! - only `ping`, `commands.list`, `state.get` (not `input`), `wait`,
//!   `ui.screenshot` (window and scale only), `command.run` and the five
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
use std::path::{Component, Path};

/// The change and remove commands 8a can preview (Ruling 6).
pub const PREVIEWABLE: &[&str] = &[
    "track.rename",
    "track.delete",
    "edit.undo",
    "edit.redo",
    "file.save",
];

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

/// The class of `spec` run with `args` (Ruling 5, P3): a new bus with a
/// send from the composer's `strip` is a change, and importing MIDI into a
/// new project (which opens a window) is refused.
pub fn effective_class(spec: &CommandSpec, args: &[(String, ArgValue)]) -> Class {
    match spec.id {
        "mixer.new_bus" if arg(args, "strip").is_some() => Class::Change,
        "file.import_midi" if flag(args, "new_project") => Class::Refused,
        _ => spec.class,
    }
}

/// The rule for an outward command's `path` (Ruling 11, P1); None when `id`
/// is not outward.
pub fn output(id: &str, args: &[(String, ArgValue)]) -> Option<Output> {
    match id {
        "file.export_midi" => Some(Output::NewFile(&["mid"])),
        // The clip export's own Save panel writes `.midi2` (P1: no looser
        // than the composer's Save dialog). One track only: a folder for
        // several exists, so the new-file rule refuses it.
        "file.export_midi_clip" => Some(Output::NewFile(&["midi2"])),
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
/// needs one of the rule's extensions and no leading dot. Relative paths
/// and `..` are refused. Each refusal is `not_allowed`, naming the path.
pub fn check_output(path: &Path, rule: Output) -> Result<(), ControlError> {
    if !path.is_absolute() {
        return Err(refuse(path, "the path must be absolute"));
    }
    if path.components().any(|c| c == Component::ParentDir) {
        return Err(refuse(path, "the path must not hold `..`"));
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
    if !PREVIEWABLE.contains(&id) {
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
        "track.rename" | "track.delete"
            if arg(&p.args, "seq").is_none() || arg(&p.args, "lane").is_none() =>
        {
            Err(ControlError::params(
                id,
                "name the track's `seq` and `lane`",
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
        | Command::CoProducerStatus { .. } => Ok(()),
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
        | Command::AppQuit => Err(ControlError::new(
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
/// its `class` and whether 8a can preview it.
pub fn list_json() -> Value {
    let commands: Vec<Value> = COMMANDS
        .iter()
        .filter(|c| c.class != Class::Refused)
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
