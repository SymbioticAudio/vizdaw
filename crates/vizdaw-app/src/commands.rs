//! The document commands the menus, keys and control channel share: Undo,
//! Redo, the Track menu and the transport (Swift: `TrackCommands`,
//! `TransportCommands`, and the arms of `ControlCommands`). Recording and
//! open-drag refusals come with those features. A transport command reads
//! the transport fresh from the session, as Swift polls first.

use core_control::registry::{self, ArgValue};
use core_control::{ControlError, ErrorCode};
use vizdaw_ffi::VizError;
use vizdaw_ffi::edit::{EditCommand, LaneEdit};
use vizdaw_ffi::tracks::TrackKindRow;

use crate::alerts::error_message;
use crate::document::DocumentModel;
use crate::editor::{DetailEditorKind, GESTURE_OPEN};
use crate::prefs::Prefs;
use crate::transport_bar::TransportLayout;

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

/// The registry IDs of the editor commands `run` handles (the detail pane
/// and the grid). The menus, keys and channel route them once their views
/// exist.
pub const EDITOR_COMMANDS: &[&str] = &["editors.snap", "editors.list", "editors.close_detail"];

const NO_DETAIL: &str = "No detail editor is open.";

/// The registry IDs of the transport commands `run` handles. Recording,
/// the click, locators and the punch and edit-out fields come later.
pub const TRANSPORT_COMMANDS: &[&str] = &[
    "transport.play_stop",
    "transport.stop",
    "transport.play",
    "transport.play_selection",
    "transport.play_from_selection",
    "transport.play_from_edit_in",
    "transport.scrub",
    "transport.return_to_start",
    "transport.rewind",
    "transport.fast_forward",
    "transport.previous_marker",
    "transport.next_marker",
    "transport.loop",
    "transport.go_position",
    "transport.go_edit_in",
];

/// The registry IDs `run_app` handles: app-wide, so they need no document.
pub const APP_COMMANDS: &[&str] = &["ui.set_transport_bar", "ui.reset_transport_bar"];

/// Why a bare transport key does nothing while a field is typed in.
pub const TEXT_FIELD_HAS_KEYBOARD: &str = "A text field has the keyboard.";
const NO_PLAY_SEQUENCE: &str = "Select a sequence to play.";
const NOT_YET: &str = "Scrubbing is not available yet.";
const NO_SELECTION: &str = "Select something in the shown sequence first.";
const NO_EDIT_IN: &str = "Set an edit-in point (select a time range) first.";

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
    /// A transport field the UI is to focus (Go to Position, Go to Edit In).
    pub focus: Option<FieldFocus>,
}

/// A transport bar field a command asks the UI to focus. The UI answers
/// `unavailable` "The window shows no transport fields." when the bar is
/// folded or absent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FieldFocus {
    Position,
    EditIn,
}

/// Runs an app-wide command (`APP_COMMANDS`) with the channel's `args`
/// against the preferences; no document is needed.
pub fn run_app(
    id: &str,
    args: &[(String, ArgValue)],
    prefs: &mut Prefs,
) -> Result<Ran, ControlError> {
    let status = |s: &str| Ran {
        undo: None,
        status: Some(s.into()),
        focus: None,
    };
    let write = |r: Result<(), String>| r.map_err(failed);
    match id {
        "ui.set_transport_bar" => {
            check_names(id, args)?;
            let Some((_, ArgValue::Text(items))) = args.iter().find(|(n, _)| n == "items") else {
                return Err(ControlError::params(
                    id,
                    "`items` lists the bar's items, such as \"cluster,loop\"",
                ));
            };
            let layout = TransportLayout::parse(items).map_err(|m| ControlError::params(id, m))?;
            write(prefs.set_transport_bar(&layout))?;
            Ok(status(&format!(
                "transport bar: {} items",
                layout.items.len()
            )))
        }
        "ui.reset_transport_bar" => {
            check_names(id, args)?;
            write(prefs.reset_transport_bar())?;
            Ok(status("transport bar: the default"))
        }
        _ => Err(ControlError::new(
            ErrorCode::UnknownId,
            format!("There is no command {id}."),
        )),
    }
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
    if TRANSPORT_COMMANDS.contains(&id) {
        return transport_enabled(id, doc);
    }
    if EDITOR_COMMANDS.contains(&id) {
        return id != "editors.close_detail" || doc.layout().detail.is_some();
    }
    if !DOCUMENT_COMMANDS.contains(&id) {
        return false;
    }
    match id {
        "edit.undo" => doc.can_undo() && !doc.gesture_open(),
        "edit.redo" => doc.can_redo() && !doc.gesture_open(),
        _ if kind_of(id).is_some() => doc.selected_seq().is_some(),
        _ => doc.selected_seq().is_some() && doc.selected_lane().is_some(),
    }
}

/// The menu and key entry: acts on the selection. Undo and Redo swallow a
/// busy core; Rename asks the UI to start the header's rename field. What
/// cannot run shows in the status line (and is returned).
pub fn perform_from_menu(id: &str, doc: &mut DocumentModel) -> Result<Ran, ControlError> {
    if TRANSPORT_COMMANDS.contains(&id) {
        return transport_from_menu(id, doc);
    }
    if EDITOR_COMMANDS.contains(&id) {
        return run_editor(id, &[], doc).map_err(|e| refused(doc, e));
    }
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
        if doc.gesture_open() {
            return Err(refused(doc, unavailable(GESTURE_OPEN)));
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
        focus: None,
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
pub(crate) fn from_viz(e: &VizError) -> ControlError {
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
    if TRANSPORT_COMMANDS.contains(&id) {
        return run_transport(id, args, doc);
    }
    if EDITOR_COMMANDS.contains(&id) {
        return run_editor(id, args, doc);
    }
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
    if doc.gesture_open() {
        return Err(unavailable(GESTURE_OPEN));
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

/// The `on` flag: absent, or true or false.
fn on_flag(id: &str, args: &[(String, ArgValue)]) -> Result<Option<bool>, ControlError> {
    match args.iter().find(|(n, _)| n == "on") {
        None => Ok(None),
        Some((_, ArgValue::Flag(f))) => Ok(Some(*f)),
        Some(_) => Err(ControlError::params(id, "`on` is true or false")),
    }
}

/// An editor command: Snap to Grid (`on`, absent toggles), List (opens on
/// the open detail's target or the selection; nothing to show opens
/// nothing) and Close Detail Editor.
fn run_editor(
    id: &str,
    args: &[(String, ArgValue)],
    doc: &mut DocumentModel,
) -> Result<Ran, ControlError> {
    check_names(id, args)?;
    match id {
        "editors.snap" => {
            let on = on_flag(id, args)?;
            doc.update_layout(|l| l.snap.enabled = on.unwrap_or(!l.snap.enabled));
        }
        "editors.list" => doc.show_detail(DetailEditorKind::List),
        _ => {
            if doc.layout().detail.is_none() {
                return Err(unavailable(NO_DETAIL));
            }
            doc.close_detail();
        }
    }
    Ok(ran(doc))
}

/// Whether a transport command can run now (its menu item is enabled).
fn transport_enabled(id: &str, doc: &DocumentModel) -> bool {
    if !doc.transport_keys_enabled() {
        return false;
    }
    let seq = doc.selected_seq().is_some();
    match id {
        "transport.scrub" => false,
        "transport.stop" => doc.is_playing(),
        "transport.play_stop" | "transport.play" => seq || doc.is_playing(),
        "transport.play_from_edit_in" => seq && doc.selection().time_range.is_some(),
        "transport.return_to_start" => seq && doc.return_key_enabled(),
        "transport.go_position" | "transport.go_edit_in" => true,
        _ => seq,
    }
}

/// A transport command from a menu item or key: what cannot run shows in
/// the status line, and Play, Stop, Return and Loop raise their alerts as
/// the model's own intents do.
fn transport_from_menu(id: &str, doc: &mut DocumentModel) -> Result<Ran, ControlError> {
    match id {
        "transport.play_stop" | "transport.return_to_start" | "transport.loop" => {
            let gate = match id {
                "transport.play_stop" => text_gate(doc).or_else(|| {
                    (doc.selected_seq().is_none() && !doc.is_playing())
                        .then(|| unavailable(NO_PLAY_SEQUENCE))
                }),
                "transport.return_to_start" => {
                    (!doc.return_key_enabled()).then(|| unavailable(RETURN_KEY_BUSY))
                }
                _ => text_gate(doc),
            };
            if let Some(e) = gate {
                return Err(refused(doc, e));
            }
            match id {
                "transport.play_stop" => doc.play_stop(),
                "transport.return_to_start" => doc.return_to_start(),
                _ => doc.toggle_loop(),
            }
            Ok(ran(doc))
        }
        _ => run_transport(id, &[], doc).map_err(|e| refused(doc, e)),
    }
}

const RETURN_KEY_BUSY: &str = "A text field or list has the keyboard.";

fn text_gate(doc: &DocumentModel) -> Option<ControlError> {
    (!doc.transport_keys_enabled()).then(|| unavailable(TEXT_FIELD_HAS_KEYBOARD))
}

/// A transport command over the channel (and the keys that share it).
fn run_transport(
    id: &str,
    args: &[(String, ArgValue)],
    doc: &mut DocumentModel,
) -> Result<Ran, ControlError> {
    check_names(id, args)?;
    let on = on_flag(id, args)?;
    match id {
        "transport.play_stop" => {
            let _ = doc.poll_transport();
            if doc.selected_seq().is_none() && !doc.is_playing() {
                return Err(unavailable(NO_PLAY_SEQUENCE));
            }
            if let Some(e) = text_gate(doc) {
                return Err(e);
            }
            doc.try_play_stop().map_err(|(playing, e)| {
                let mut e = from_viz(&e);
                if e.code != ErrorCode::Busy {
                    let verb = if playing { "Stop" } else { "Play" };
                    e.message = format!("{verb} Failed: {}", e.message);
                }
                e
            })?;
        }
        "transport.loop" => {
            if on.unwrap_or(!doc.looping()) != doc.looping() {
                doc.toggle_loop();
            }
        }
        "transport.return_to_start" => {
            if !doc.return_key_enabled() {
                return Err(unavailable(RETURN_KEY_BUSY));
            }
            doc.return_to_start();
        }
        "transport.go_position" | "transport.go_edit_in" => {
            if let Some(e) = text_gate(doc) {
                return Err(e);
            }
            let mut ran = ran(doc);
            ran.focus = Some(if id == "transport.go_position" {
                FieldFocus::Position
            } else {
                FieldFocus::EditIn
            });
            return Ok(ran);
        }
        _ => run_key_command(id, doc)?,
    }
    Ok(ran(doc))
}

/// The commands that wait for the text fields and then act on the session.
fn run_key_command(id: &str, doc: &mut DocumentModel) -> Result<(), ControlError> {
    if let Some(e) = text_gate(doc) {
        return Err(e);
    }
    // Read first, as Swift polls: a busy read keeps the last good one.
    let _ = doc.poll_transport();
    let playing = || doc.is_playing();
    match id {
        "transport.scrub" => return Err(unavailable(NOT_YET)),
        "transport.stop" => {
            if !playing() {
                return Err(unavailable("Nothing is playing."));
            }
            return doc.stop_and_return().map_err(|e| from_viz(&e));
        }
        "transport.play" => {
            // Play only starts: while playing it does nothing; a stopped
            // transport plays from where it stands.
            if playing() {
                return Ok(());
            }
            if doc.selected_seq().is_none() {
                return Err(unavailable(NO_PLAY_SEQUENCE));
            }
            return doc.start_playing(None, None).map_err(|e| from_viz(&e));
        }
        _ => {}
    }
    let Some(seq) = doc.selected_seq() else {
        return Err(unavailable(NO_SEQUENCE));
    };
    match id {
        "transport.play_selection" | "transport.play_from_selection" => {
            let span = doc
                .session()
                .selection_span(seq)
                .map_err(|e| from_viz(&e))?;
            let Some(span) = span else {
                return Err(unavailable(NO_SELECTION));
            };
            let to = (id == "transport.play_selection").then_some(span.end);
            doc.start_playing(Some(span.start), to)
                .map_err(|e| from_viz(&e))
        }
        "transport.play_from_edit_in" => {
            let Some(range) = doc.selection().time_range else {
                return Err(unavailable(NO_EDIT_IN));
            };
            doc.start_playing(Some(range.start), None)
                .map_err(|e| from_viz(&e))
        }
        "transport.previous_marker" | "transport.next_marker" => {
            let forward = id == "transport.next_marker";
            let tick = doc.last_transport().tick;
            let target = doc
                .session()
                .marker_step(seq, tick, forward)
                .map_err(|e| from_viz(&e))?;
            let Some(target) = target else {
                return Err(unavailable(if forward {
                    "No marker after the position."
                } else {
                    "No marker before the position."
                }));
            };
            doc.move_to(target).map_err(|e| from_viz(&e))
        }
        // One bar, as a click on the bar's button. A held key goes to
        // `Shuttle::key_down` instead.
        _ => doc
            .session()
            .transport_step_bar(id == "transport.fast_forward")
            .map(|_| ())
            .map_err(|e| from_viz(&e)),
    }
}
