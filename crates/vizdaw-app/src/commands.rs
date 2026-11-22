//! The document commands the menus, keys and control channel share: Undo,
//! Redo and the Track menu (Swift: `TrackCommands`, and the undo/redo arm
//! of `ControlCommands`). Recording and open-drag refusals come with those
//! features.

use core_control::registry::{self, ArgValue};
use core_control::{ControlError, ErrorCode};
use vizdaw_ffi::VizError;
use vizdaw_ffi::edit::{EditCommand, LaneEdit};
use vizdaw_ffi::tracks::TrackKindRow;

use crate::alerts::error_message;
use crate::document::DocumentModel;

/// The registry IDs `run` handles.
pub const DOCUMENT_COMMANDS: &[&str] = &[
    "edit.undo",
    "edit.redo",
    "track.new_audio",
    "track.new_instrument",
    "track.new_midi",
    "track.duplicate",
    "track.delete",
    "track.rename",
];

const NO_SEQUENCE: &str = "Select a sequence first.";
const NO_TRACK: &str = "Select a track in the arrangement first.";
/// What the status line says after a MIDI track is made with no destination.
const NO_MIDI_DESTINATION_YET: &str =
    "No external MIDI destination yet: choose one in the track's destination menu.";

/// What a command left behind, for the channel's reply.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ran {
    /// The undo label afterwards.
    pub undo: Option<String>,
    /// The status line afterwards.
    pub status: Option<String>,
}

fn kind_of(id: &str) -> Option<TrackKindRow> {
    match id {
        "track.new_audio" => Some(TrackKindRow::Audio),
        "track.new_instrument" => Some(TrackKindRow::Instrument),
        "track.new_midi" => Some(TrackKindRow::Midi),
        _ => None,
    }
}

/// Whether `id` can run on `doc` now (its menu item is enabled).
pub fn enabled(id: &str, doc: &DocumentModel) -> bool {
    if !DOCUMENT_COMMANDS.contains(&id) {
        return false;
    }
    match id {
        "edit.undo" => doc.can_undo(),
        "edit.redo" => doc.can_redo(),
        _ if kind_of(id).is_some() => doc.selected_seq().is_some(),
        _ => doc.selected_seq().is_some() && doc.selected_lane().is_some(),
    }
}

/// The menu and key entry: acts on the selection. Undo and Redo swallow a
/// busy core; Rename asks the UI to start the header's rename field. What
/// cannot run shows in the status line (and is returned).
pub fn perform_from_menu(id: &str, doc: &mut DocumentModel) -> Result<Ran, ControlError> {
    if !DOCUMENT_COMMANDS.contains(&id) {
        return Err(unavailable(&format!("There is no command {id}.")));
    }
    if matches!(id, "edit.undo" | "edit.redo") {
        let undo = id == "edit.undo";
        if !(if undo { doc.can_undo() } else { doc.can_redo() }) {
            return Err(refused(
                doc,
                unavailable(if undo {
                    "Nothing to undo."
                } else {
                    "Nothing to redo."
                }),
            ));
        }
        // The model's own step: Busy is silent, any other failure an alert.
        if undo {
            doc.undo();
        } else {
            doc.redo();
        }
        return Ok(ran(doc));
    }
    if doc.selected_seq().is_none() && kind_of(id).is_some() {
        return Err(refused(doc, unavailable(NO_SEQUENCE)));
    }
    if kind_of(id).is_none() {
        if doc.selected_seq().is_none() {
            return Err(refused(doc, unavailable(NO_SEQUENCE)));
        }
        if doc.selected_lane().is_none() {
            return Err(refused(doc, unavailable(NO_TRACK)));
        }
    }
    if id == "track.rename" {
        return Err(ControlError::dialog("Rename Track", &["name"]));
    }
    run(id, &[], doc).map_err(|e| refused(doc, e))
}

/// A refusal shows in the status line, and is returned.
fn refused(doc: &mut DocumentModel, e: ControlError) -> ControlError {
    doc.set_status(Some(e.message.clone()));
    e
}

fn ran(doc: &DocumentModel) -> Ran {
    Ran {
        undo: doc.snapshot().undo_label.clone(),
        status: doc.status().map(String::from),
    }
}

fn unavailable(message: &str) -> ControlError {
    ControlError::new(ErrorCode::Unavailable, message)
}

fn failed(message: impl Into<String>) -> ControlError {
    ControlError::new(ErrorCode::Failed, message)
}

fn no_track(lane: u64) -> ControlError {
    failed(format!("There is no track {lane} in this sequence."))
}

/// A core error as a control error: a busy core is `busy`.
fn from_viz(e: &VizError) -> ControlError {
    let code = if matches!(e, VizError::Busy { .. }) {
        ErrorCode::Busy
    } else {
        ErrorCode::Failed
    };
    ControlError::new(code, error_message(e))
}

/// The whole-number argument `name`: absent, a value, or an error.
fn number(id: &str, args: &[(String, ArgValue)], name: &str) -> Result<Option<u64>, ControlError> {
    let Some((_, v)) = args.iter().find(|(n, _)| n == name) else {
        return Ok(None);
    };
    match v {
        ArgValue::Int(i) => u64::try_from(*i).map(Some).ok(),
        _ => None,
    }
    .ok_or_else(|| ControlError::params(id, format!("`{name}` is a whole number from 0")))
}

/// Runs `id` with the channel's `args` on `doc`. `seq` defaults to the
/// shown sequence, `lane` to the selection's first track and `after` (a New
/// command) to the selected track.
pub fn run(
    id: &str,
    args: &[(String, ArgValue)],
    doc: &mut DocumentModel,
) -> Result<Ran, ControlError> {
    if !DOCUMENT_COMMANDS.contains(&id) {
        return Err(ControlError::new(
            ErrorCode::UnknownId,
            format!("There is no command {id}."),
        ));
    }
    if matches!(id, "edit.undo" | "edit.redo") {
        return run_step(id == "edit.undo", doc);
    }
    check_names(id, args)?;
    let seq_arg = number(id, args, "seq")?;
    let lane_arg = number(id, args, "lane")?;
    let after_arg = number(id, args, "after")?;
    let name = match args.iter().find(|(n, _)| n == "name") {
        None => None,
        Some((_, ArgValue::Text(t))) => Some(t.clone()),
        Some(_) => return Err(ControlError::params(id, "`name` is text")),
    };

    let Some(seq) = seq_arg.or(doc.selected_seq()) else {
        return Err(unavailable(NO_SEQUENCE));
    };
    if !doc.snapshot().sequences.iter().any(|q| q.id == seq) {
        return Err(failed(format!("There is no sequence {seq}.")));
    }
    let shown = Some(seq) == doc.selected_seq();
    let rows = if shown {
        doc.lanes().to_vec()
    } else {
        doc.session().lanes(seq).unwrap_or_default()
    };
    let known = |lane: u64| rows.iter().any(|l| l.id == lane);
    if let Some(a) = after_arg
        && !known(a)
    {
        return Err(no_track(a));
    }
    let lane = match lane_arg {
        Some(l) if !known(l) => return Err(no_track(l)),
        Some(l) => Some(l),
        None if shown => doc.selected_lane(),
        None => None,
    };

    let before: Vec<u64> = rows.iter().map(|l| l.id).collect();
    if let Some(kind) = kind_of(id) {
        let anchor = after_arg.or(if shown { doc.selected_lane() } else { None });
        let result = doc.session().new_track(seq, kind, anchor);
        doc.apply_result(result).map_err(|e| from_viz(&e))?;
        select_new(doc, seq, &before);
        let made = doc.lanes().iter().find(|l| !before.contains(&l.id));
        if shown && kind == TrackKindRow::Midi && made.is_some_and(|l| l.midi_dest.is_none()) {
            doc.set_status(Some(NO_MIDI_DESTINATION_YET.into()));
        }
        return Ok(ran(doc));
    }
    let Some(lane) = lane else {
        return Err(unavailable(NO_TRACK));
    };
    match id {
        "track.duplicate" => {
            let result = doc.session().duplicate_track(seq, lane);
            doc.apply_result(result).map_err(|e| from_viz(&e))?;
            select_new(doc, seq, &before);
        }
        "track.delete" => lane_edit(doc, LaneEdit::Remove { seq, lane })?,
        _ => {
            let Some(name) = name else {
                return Err(ControlError::dialog("Rename Track", &["name"]));
            };
            lane_edit(doc, LaneEdit::Rename { seq, lane, name })?;
        }
    }
    Ok(ran(doc))
}

/// Refuses a parameter the command does not take, or one given twice.
fn check_names(id: &str, args: &[(String, ArgValue)]) -> Result<(), ControlError> {
    let spec = registry::find(id);
    for (i, (name, _)) in args.iter().enumerate() {
        if spec.is_some_and(|s| !s.params.iter().any(|p| p.name == name))
            || args[..i].iter().any(|(n, _)| n == name)
        {
            return Err(ControlError::params(
                id,
                format!("`{name}` is not a parameter, or is repeated"),
            ));
        }
    }
    Ok(())
}

fn lane_edit(doc: &mut DocumentModel, edit: LaneEdit) -> Result<(), ControlError> {
    let result = doc.session().edit(EditCommand::Lane { edit });
    doc.apply_result(result).map_err(|e| from_viz(&e))
}

/// Selects the lane that was not in `before` (the new track or the copy),
/// when it is in the shown sequence.
fn select_new(doc: &mut DocumentModel, seq: u64, before: &[u64]) {
    if Some(seq) != doc.selected_seq() {
        return;
    }
    if let Some(made) = doc.lanes().iter().find(|l| !before.contains(&l.id)) {
        let id = made.id;
        doc.select_track(id, false);
    }
}

/// Undo or Redo over the channel: a busy core is the call's `busy` error.
fn run_step(undo: bool, doc: &mut DocumentModel) -> Result<Ran, ControlError> {
    let has = if undo { doc.can_undo() } else { doc.can_redo() };
    if !has {
        return Err(unavailable(if undo {
            "Nothing to undo."
        } else {
            "Nothing to redo."
        }));
    }
    let result = if undo {
        doc.session().undo()
    } else {
        doc.session().redo()
    };
    let verb = if undo { "Undo" } else { "Redo" };
    doc.apply_result(result).map_err(|e| {
        let mut e = from_viz(&e);
        if e.code != ErrorCode::Busy {
            e.message = format!("{verb} Failed: {}", e.message);
        }
        e
    })?;
    Ok(ran(doc))
}
