//! `surface.*` from the control channel (Swift `SurfaceCommands`, plan
//! 5f-1 Task 9, Ruling 16): `surface.add` and `surface.remove` change the
//! app-wide surfaces (no document needed); `surface.inject` feeds bytes
//! to a surface as if from its input; `surface.learn` and
//! `surface.unlearn` start a MIDI learn or remove learned maps, for
//! exactly one of `target` (an automatable control of the project, spelt
//! as `automation.gesture` spells it) and `command` (a key-bindable
//! registry ID).
//!
//! Every argument is checked here before the core is called, and is
//! `invalid_params` naming the parameter. `bytes` is refused only when it
//! is not hex, is empty or holds more than 1024 bytes: any hex goes to
//! the core, which ignores and counts malformed MIDI. The core's
//! refusals are the call's `failed` (its `Busy` is `busy`). Nothing here
//! opens a dialog (`M/SurfaceCommands.swift:45-58`).

use core_control::registry::ArgValue;
use core_control::{ControlError, ErrorCode};
use vizdaw_ffi::events::AutomationTargetRow;
use vizdaw_ffi::surfaces::SurfaceKindRow;

use crate::CoreCalls;
use crate::commands::{Ran, check_names, from_viz, ran};
use crate::document::DocumentModel;
use crate::editor::GESTURE_OPEN;
use crate::record::hex_bytes;

pub const ADD: &str = "surface.add";
pub const REMOVE: &str = "surface.remove";
pub const INJECT: &str = "surface.inject";
pub const LEARN: &str = "surface.learn";
pub const UNLEARN: &str = "surface.unlearn";

/// The commands [`run_surface`] runs.
pub const SURFACE_COMMANDS: &[&str] = &[ADD, REMOVE, INJECT, LEARN, UNLEARN];

/// The ones that change the app-wide surfaces and need no document.
pub const APP_WIDE: &[&str] = &[ADD, REMOVE];

/// `surface.inject` and the learns without a document
/// (`M/SurfaceCommands.swift:108`).
pub const NO_DOCUMENT: &str = "No document is open.";

/// `s` as a unit type, spelt as the registry's choice does; `None` for
/// anything else (`M/SurfaceCommands.swift:70-78`).
pub fn parse_kind(s: &str) -> Option<SurfaceKindRow> {
    match s {
        "mcu" => Some(SurfaceKindRow::Mcu),
        "mcu_xt" => Some(SurfaceKindRow::McuXt),
        "hui" => Some(SurfaceKindRow::Hui),
        "kontrol_mk3" => Some(SurfaceKindRow::KontrolMk3),
        _ => None,
    }
}

/// Runs `surface.*` (`M/SurfaceCommands.swift:86-137`). `doc` `None`:
/// `surface.add` and `surface.remove` still run; the others check their
/// arguments, then are `no_document`.
pub fn run_surface(
    id: &str,
    args: &[(String, ArgValue)],
    doc: Option<&mut DocumentModel>,
    core: &dyn CoreCalls,
) -> Result<Ran, ControlError> {
    check_names(id, args)?;
    match id {
        ADD => return run_add(args, core),
        REMOVE => {
            let s = surface_id(args)
                .ok_or_else(|| ControlError::params(id, "`surface` is a surface ID"))?;
            core.surface_remove(s).map_err(|e| from_viz(&e))?;
            return Ok(app_ran(None));
        }
        INJECT | LEARN | UNLEARN => {}
        _ => {
            return Err(ControlError::new(
                ErrorCode::UnknownId,
                format!("VizDAW has no handler for {id}."),
            ));
        }
    }
    // Every argument is checked before the document is looked for.
    let checked = check(id, args)?;
    let Some(m) = doc else {
        return Err(ControlError::new(ErrorCode::NoDocument, NO_DOCUMENT));
    };
    // A result's `status` is this command's own.
    m.set_status(None);
    let viz = |e: vizdaw_ffi::VizError| from_viz(&e);
    match checked {
        Checked::Inject(s, bytes) => m.session().inject_surface(s, bytes).map_err(viz)?,
        Checked::LearnTarget(t) => m.session().learn_target(t).map_err(viz)?,
        Checked::LearnCommand(c) => {
            // The learn completes in this window's session.
            m.session().focus_surfaces().map_err(viz)?;
            core.learn_command(&c).map_err(viz)?;
        }
        Checked::UnlearnTarget(t) => {
            if m.gesture_open() {
                return Err(ControlError::new(ErrorCode::Unavailable, GESTURE_OPEN));
            }
            let result = m.session().unlearn_target(t);
            m.apply_result(result).map_err(viz)?;
        }
        Checked::UnlearnCommand(c) => core.remove_command_map(&c).map_err(viz)?,
    }
    Ok(ran(m))
}

/// An app-wide call's result: no undo, `status` as given.
fn app_ran(status: Option<String>) -> Ran {
    Ran {
        undo: None,
        status,
        focus: None,
        open_mixer_window: false,
        open_modify_panel: false,
    }
}

/// A per-document call whose arguments were checked.
enum Checked {
    Inject(u32, Vec<u8>),
    LearnTarget(AutomationTargetRow),
    LearnCommand(String),
    UnlearnTarget(AutomationTargetRow),
    UnlearnCommand(String),
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

/// The `surface` argument as a surface ID.
fn surface_id(args: &[(String, ArgValue)]) -> Option<u32> {
    match arg(args, "surface") {
        Some(ArgValue::Int(n)) => u32::try_from(*n).ok(),
        _ => None,
    }
}

/// `M/SurfaceCommands.swift:154-179`.
fn check(id: &str, args: &[(String, ArgValue)]) -> Result<Checked, ControlError> {
    let bad = |message: &str| ControlError::params(id, message);
    if id == INJECT {
        let s = surface_id(args).ok_or_else(|| bad("`surface` is a surface ID"))?;
        let bytes = hex_bytes(text(args, "bytes").unwrap_or_default())
            .ok_or_else(|| bad("`bytes` are 1 to 1024 hex bytes such as \"90 68 7f\""))?;
        return Ok(Checked::Inject(s, bytes));
    }
    let learn = id == LEARN;
    let has_target = arg(args, "target").is_some();
    if has_target == arg(args, "command").is_some() {
        return Err(bad("give exactly one of `target` and `command`"));
    }
    if has_target {
        let t = text(args, "target")
            .and_then(|t| vizdaw_ffi::automation::parse_automation_target(t.to_owned()))
            .ok_or_else(|| {
                bad(
                    "`target` is fader:<strip>, pan:<strip>, mute:<strip>, solo:<strip>, \
                     send:<strip>:<n> (n from 1), param:<slot>:<param> or vca:<vca>",
                )
            })?;
        return Ok(if learn {
            Checked::LearnTarget(t)
        } else {
            Checked::UnlearnTarget(t)
        });
    }
    let c = text(args, "command")
        .filter(|c| !c.is_empty())
        .ok_or_else(|| bad("`command` is a registry command ID"))?
        .to_owned();
    Ok(if learn {
        Checked::LearnCommand(c)
    } else {
        Checked::UnlearnCommand(c)
    })
}

/// `surface.add`: answers the new surface's ID in `status` ("surface 3",
/// `M/SurfaceCommands.swift:182-205`). A HUI-compatible unit and the
/// keyboard are banks of their own, so they are added without a
/// position (as `SurfaceSettings::add`).
fn run_add(args: &[(String, ArgValue)], core: &dyn CoreCalls) -> Result<Ran, ControlError> {
    let bad = |message: &str| ControlError::params(ADD, message);
    let kind = text(args, "type")
        .and_then(parse_kind)
        .ok_or_else(|| bad("`type` is mcu, mcu_xt, hui or kontrol_mk3"))?;
    // The ports go as given: the core matches the port names and refuses
    // a blank one (`failed`).
    let (Some(input), Some(output)) = (text(args, "input"), text(args, "output")) else {
        return Err(bad("`input` and `output` name the MIDI ports"));
    };
    let position = match arg(args, "position") {
        None => None,
        Some(ArgValue::Int(p)) if (0..=3).contains(p) => u8::try_from(*p).ok(),
        Some(_) => return Err(bad("`position` is 0 to 3")),
    };
    let own_bank = matches!(kind, SurfaceKindRow::Hui | SurfaceKindRow::KontrolMk3);
    let position = if own_bank { None } else { position };
    let id = core
        .surface_add(kind, input, output, position)
        .map_err(|e| from_viz(&e))?;
    Ok(app_ran(Some(format!("surface {id}"))))
}
