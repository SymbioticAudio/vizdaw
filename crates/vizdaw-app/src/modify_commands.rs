//! The Modify menu's commands (Swift `S/AppCommands.swift:228-254` and the
//! arms of `M/ControlCommands.swift:299-302, 329, 653-673`): the five panel
//! tasks, Modify Again, Templates, Exact Mode, and the three key-only note
//! chores.

use core_control::registry::ArgValue;
use core_control::{ControlError, ErrorCode};
use vizdaw_ffi::VizError;

use crate::commands::{Ran, check_names, from_viz, on_flag, ran};
use crate::document::DocumentModel;
use crate::editor::GESTURE_OPEN;
use crate::mixer::commands::control_args;
use crate::select_modify::PanelTask;
use crate::select_modify::model::{HIDDEN_SELECTION, NO_MODIFICATION, NO_TEMPLATES};

/// The registry IDs of the Modify commands `run` handles.
pub const MODIFY_COMMANDS: &[&str] = &[
    "modify.select_by_rule",
    "modify.transpose",
    "modify.quantize",
    "modify.change",
    "modify.move",
    "modify.again",
    "modify.template",
    "modify.exact_mode",
    "modify.quantize_notes",
    "modify.transpose_notes",
    "modify.velocity_notes",
];

/// The key-only chores (Swift `ControlCommands.chores`), each one
/// `run_chore` call; the core checks their arguments again.
const CHORES: &[&str] = &[
    "modify.quantize_notes",
    "modify.transpose_notes",
    "modify.velocity_notes",
];

/// Modify Again refused with no word from the core.
const AGAIN_REFUSED: &str = "Modify Again was refused.";
/// A template refused with no word from the core.
const TEMPLATE_REFUSED: &str = "The template was refused.";

/// The panel task a Modify menu item opens.
fn panel_task(id: &str) -> Option<PanelTask> {
    PanelTask::ALL.into_iter().find(|t| t.command_id() == id)
}

/// Whether `id` can run on `doc` now (its menu item is enabled).
pub fn enabled(id: &str, doc: &DocumentModel) -> bool {
    match id {
        "modify.again" => doc.can_modify_again() && !doc.selection_hidden(),
        "modify.template" => !doc.modify_templates().is_empty(),
        _ if CHORES.contains(&id) => !doc.gesture_open(),
        _ => MODIFY_COMMANDS.contains(&id),
    }
}

/// The menu and key entry. A refusal shows in the status line, and is
/// returned.
pub fn perform_from_menu(id: &str, doc: &mut DocumentModel) -> Result<Ran, ControlError> {
    run(id, &[], doc).inspect_err(|e| doc.set_status(Some(e.message.clone())))
}

/// Runs Modify command `id` with the channel's `args` on `doc`. A panel
/// task asks the UI to show the panel (`Ran::open_modify_panel`).
pub fn run(
    id: &str,
    args: &[(String, ArgValue)],
    doc: &mut DocumentModel,
) -> Result<Ran, ControlError> {
    check_names(id, args)?;
    if let Some(task) = panel_task(id) {
        doc.open_modify(task, None);
        return Ok(Ran {
            open_modify_panel: true,
            ..ran(doc)
        });
    }
    match id {
        "modify.again" => {
            if doc.selection_hidden() {
                return Err(unavailable(HIDDEN_SELECTION));
            }
            if !doc.can_modify_again() {
                return Err(unavailable(NO_MODIFICATION));
            }
            doc.modify_again()
                .map_err(|status| refusal(status, AGAIN_REFUSED))?;
        }
        "modify.template" => {
            if doc.modify_templates().is_empty() {
                return Err(unavailable(NO_TEMPLATES));
            }
            let name = match args.iter().find(|(n, _)| n == "name") {
                None => String::new(),
                Some((_, ArgValue::Text(t))) => t.clone(),
                Some(_) => return Err(ControlError::params(id, "`name` is text")),
            };
            if !doc.modify_templates().iter().any(|t| t.name == name) {
                return Err(ControlError::params(
                    id,
                    format!("no template named \"{name}\""),
                ));
            }
            doc.run_modify_template(&name)
                .map_err(|status| refusal(status, TEMPLATE_REFUSED))?;
        }
        "modify.exact_mode" => {
            let on = on_flag(id, args)?.unwrap_or(!doc.exact_mode());
            doc.set_exact_mode(on);
        }
        _ if CHORES.contains(&id) => {
            // One undo step, the arguments as given (plan 8c Ruling 2).
            if doc.gesture_open() {
                return Err(unavailable(GESTURE_OPEN));
            }
            let result = doc.session.run_chore(id.to_owned(), control_args(args));
            doc.apply_result(result)
                .map_err(|e: VizError| from_viz(&e))?;
        }
        _ => {
            return Err(ControlError::new(
                ErrorCode::UnknownId,
                format!("There is no command {id}."),
            ));
        }
    }
    Ok(ran(doc))
}

fn unavailable(message: &str) -> ControlError {
    ControlError::new(ErrorCode::Unavailable, message)
}

/// A refused edit: the status line's words, else `fallback`.
fn refusal(status: String, fallback: &str) -> ControlError {
    let message = if status.is_empty() {
        fallback.to_owned()
    } else {
        status
    };
    ControlError::new(ErrorCode::Failed, message)
}
