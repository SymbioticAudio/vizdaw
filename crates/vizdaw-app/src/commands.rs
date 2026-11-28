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

use vizdaw_ffi::automation::automation_suspended;

use crate::alerts::error_message;
use crate::document::DocumentModel;
use crate::editor::{DetailEditorKind, GESTURE_OPEN, NudgeDirection};
pub use crate::lane_inputs::LANE_INPUT_COMMANDS;
use crate::mixer::commands::{SELECTION_FILLED, SUSPEND, mode_of, suspend};
use crate::mixer::strips_text;
pub use crate::modify_commands::MODIFY_COMMANDS;
use crate::prefs::Prefs;
use crate::select_modify::model::HIDDEN_SELECTION;
use crate::step_entry::STEP_COMMANDS;
use crate::transport_bar::TransportLayout;
use crate::transport_fields::TransportField;

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

/// The registry IDs of the editor commands `run` handles (the detail pane,
/// the bottom pane that holds it, the grid and the nudges). The menus, keys
/// and channel route them once their views exist.
pub const EDITOR_COMMANDS: &[&str] = &[
    "editors.snap",
    "editors.piano_roll",
    "editors.list",
    "editors.strip_chart",
    "editors.close_detail",
    "editors.insert_event",
    "editors.nudge_earlier",
    "editors.nudge_later",
    "editors.nudge_up",
    "editors.nudge_down",
];

/// The registry IDs of the View menu's commands `run` handles: the main
/// views, the pane toggles and `ui.pane` (Swift `ViewCommands`).
pub const VIEW_COMMANDS: &[&str] = &[
    "ui.view_arrange",
    "ui.view_mix",
    "ui.toggle_left",
    "ui.toggle_bottom",
    "ui.toggle_right",
    "ui.pane",
];

/// The registry IDs of the mixer's commands `run` handles: the Mixer
/// window (the UI opens it, see [`Ran::open_mixer_window`]), the channel's
/// automation commands, the Automation menu's modes, New Bus, the strip
/// edits, the plug-in edits and New Plug-in Instrument Track, the strip
/// colour, the group and VCA edits (registry
/// `GROUP_IDS`) and a MIDI strip's mix. (Make Mine is Co-Producer's: not
/// yet.)
pub const MIXER_COMMANDS: &[&str] = &[
    "window.mixer",
    "automation.set_mode",
    "automation.gesture",
    "automation.release_all",
    "automation.mode.off",
    "automation.mode.read",
    "automation.mode.touch",
    "automation.mode.latch",
    "automation.mode.write",
    "automation.mode.trim",
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
    "track.new_plugin_instrument",
    "mixer.remove_bus",
    "mixer.set_colour",
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
    "destination.set_midi_mix",
];

const NO_DETAIL: &str = "No detail editor is open.";
const NO_LIST: &str = "Insert Event needs a list editor that has the keyboard.";
/// The nudges' refusals (`M/ControlCommands.swift:643-647`).
const NUDGE_NEEDS: &str = "Nudge needs selected events and no text being edited.";
const NUDGE_REFUSED: &str = "The nudge was refused.";

/// The registry IDs of the transport commands `run` handles: the keys,
/// Record and its loop passes, the record modes, the click, arming, the
/// field keys and the locators (Swift `TransportCommands.ids`).
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
    "transport.record",
    "record.keep_pass",
    "record.discard_pass",
    "transport.overdub",
    "transport.punch",
    "transport.click",
    "transport.set_click_level",
    "transport.set_click_mode",
    "transport.arm_previous",
    "transport.arm_next",
    "transport.go_punch",
    "locator.go_0",
    "locator.set_0",
    "locator.clear_0",
    "locator.go_1",
    "locator.set_1",
    "locator.clear_1",
    "locator.go_2",
    "locator.set_2",
    "locator.clear_2",
    "locator.go_3",
    "locator.set_3",
    "locator.clear_3",
    "locator.go_4",
    "locator.set_4",
    "locator.clear_4",
    "locator.go_5",
    "locator.set_5",
    "locator.clear_5",
    "locator.go_6",
    "locator.set_6",
    "locator.clear_6",
    "locator.go_7",
    "locator.set_7",
    "locator.clear_7",
    "locator.go_8",
    "locator.set_8",
    "locator.clear_8",
    "locator.go_9",
    "locator.set_9",
    "locator.clear_9",
];

/// The record commands only the channel sends (Swift `ControlCommands`'
/// arms): Record Options, recovery, the marker chores and MIDI injection.
/// `run` handles them; see `record::run_record`.
pub const RECORD_COMMANDS: &[&str] = &[
    "record.set",
    "record.recover",
    "marker.add",
    "marker.rename",
    "marker.remove",
    "midi.inject",
];

/// The click's three commands: Click toggles it; the level and mode take
/// their argument from the channel. Project settings, one undo step each.
const CLICK_COMMANDS: &[&str] = &[
    "transport.click",
    "transport.set_click_level",
    "transport.set_click_mode",
];

/// What a key or menu item of a click setting says: it has no argument.
pub const CLICK_LEVEL_NEEDS: &str = "Click Level needs a level, such as -12 dB.";
pub const CLICK_MODE_NEEDS: &str = "Click Mode needs a mode: play_and_record or record_only.";
/// Arming moved nothing and the core said nothing.
pub const ARM_DID_NOT_MOVE: &str = "Record arm did not move.";

/// The registry IDs `run_app` handles: app-wide, so they need no document.
pub const APP_COMMANDS: &[&str] = &[
    "ui.set_transport_bar",
    "ui.reset_transport_bar",
    "ui.customize_transport",
    "automation.suspend",
    "audio.set",
    "ui.theme",
];

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
    /// The UI is to open this document's Mixer window, or bring it to the
    /// front (`window.mixer`).
    pub open_mixer_window: bool,
    /// The UI is to show this document's Select and Modify panel, or
    /// bring it to the front (a Modify menu task).
    pub open_modify_panel: bool,
}

/// A transport bar field a command asks the UI to focus. The UI answers
/// `unavailable` "The window shows no transport fields." when the bar is
/// folded or absent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FieldFocus {
    Position,
    EditIn,
    /// Go to Punch Fields: Punch In.
    Punch,
}

impl FieldFocus {
    /// The field that takes the keyboard.
    pub fn field(self) -> TransportField {
        match self {
            FieldFocus::Position => TransportField::Position,
            FieldFocus::EditIn => TransportField::EditIn,
            FieldFocus::Punch => TransportField::PunchIn,
        }
    }
}

/// A locator command's operation (`go`, `set`, `clear`) and number.
pub fn locator(id: &str) -> Option<(&str, usize)> {
    let rest = id.strip_prefix("locator.")?;
    let (op, n) = rest.rsplit_once('_')?;
    Some((op, n.parse().ok()?))
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
        open_mixer_window: false,
        open_modify_panel: false,
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
        // The View menu's sheet; over the channel it only asks for it.
        "ui.customize_transport" => Err(ControlError::dialog("Customize Transport Bar", &[])),
        "ui.reset_transport_bar" => {
            check_names(id, args)?;
            write(prefs.reset_transport_bar())?;
            Ok(status("transport bar: the default"))
        }
        SUSPEND => {
            suspend(args)?;
            Ok(Ran {
                undo: None,
                status: None,
                focus: None,
                open_mixer_window: false,
                open_modify_panel: false,
            })
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

/// The File menu's commands, in menu order (Swift `AppCommands`'s File
/// groups; `media.convert` is the banner's, not a menu item).
pub const FILE_COMMANDS: &[&str] = &[
    "file.new",
    "file.open",
    "file.open_copy",
    "media.import",
    "file.import_midi",
    "file.import_dawproject",
    "file.import_aaf",
    "file.create_demo",
    "file.create_demo_audio",
    "file.close",
    "file.save",
    "file.save_as",
    "file.project_settings",
    "file.bounce",
    "file.export_dawproject",
    "file.export_aaf",
    "file.export_midi",
    "file.export_midi_clip",
    "file.export_for_daw",
    "file.interchange_report",
    "media.relink",
];

/// Whether File menu item `id` is enabled with `doc` the front document
/// (Swift `AppCommands`): the new projects, the opens, Import Project and
/// Close always; Open Co-Producer Copy… when the project is saved and
/// has copies; Last Import or Export Report… when the UI has a report;
/// Relink Media… when an item is relinkable and no merge is held; the
/// rest with a document.
pub fn file_enabled(id: &str, doc: Option<&DocumentModel>) -> bool {
    match id {
        "file.new"
        | "file.open"
        | "file.import_dawproject"
        | "file.import_aaf"
        | "file.create_demo"
        | "file.create_demo_audio"
        | "file.close" => true,
        "file.open_copy" => doc.is_some_and(DocumentModel::has_copies),
        "file.interchange_report" => doc.is_some_and(|d| d.last_report().is_some()),
        "media.relink" => doc.is_some_and(|d| !d.merge_held() && !d.relinkable().is_empty()),
        _ => FILE_COMMANDS.contains(&id) && doc.is_some(),
    }
}

/// Whether `id` can run on `doc` now (its menu item is enabled).
pub fn enabled(id: &str, doc: &DocumentModel) -> bool {
    // The views and panes can change at any time; the mixer's commands
    // need only the document, the mode commands a strip selection.
    if mode_of(id).is_some() {
        return !doc.selected_strips().is_empty();
    }
    if id == SUSPEND || VIEW_COMMANDS.contains(&id) || MIXER_COMMANDS.contains(&id) {
        return true;
    }
    if TRANSPORT_COMMANDS.contains(&id) {
        return transport_enabled(id, doc);
    }
    if RECORD_COMMANDS.contains(&id) {
        return crate::record::record_command_enabled(id, doc);
    }
    if LANE_INPUT_COMMANDS.contains(&id) {
        return crate::lane_inputs::lane_input_enabled(doc);
    }
    if STEP_COMMANDS.contains(&id) {
        return crate::step_entry::step_enabled(id, doc);
    }
    if MODIFY_COMMANDS.contains(&id) {
        return crate::modify_commands::enabled(id, doc);
    }
    if EDITOR_COMMANDS.contains(&id) {
        return match id {
            "editors.close_detail" => doc.layout().detail.is_some(),
            "editors.insert_event" => insert_event_ready(doc),
            _ if NudgeDirection::of_command(id).is_some() => nudge_ready(doc),
            _ => true,
        };
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
    if id == SUSPEND {
        suspend(&[]).map_err(|e| refused(doc, e))?;
        return Ok(ran(doc));
    }
    if VIEW_COMMANDS.contains(&id) || MIXER_COMMANDS.contains(&id) {
        // A key's New Group and New VCA take the selected strips; the
        // channel's never do.
        let args: Vec<(String, ArgValue)> = if SELECTION_FILLED.contains(&id) {
            strips_text(&doc.selected_member_strips())
                .map(|t| ("strips".to_owned(), ArgValue::Text(t)))
                .into_iter()
                .collect()
        } else {
            vec![]
        };
        return run(id, &args, doc).map_err(|e| refused(doc, e));
    }
    if TRANSPORT_COMMANDS.contains(&id) {
        return transport_from_menu(id, doc);
    }
    if RECORD_COMMANDS.contains(&id) {
        return crate::record::run_record(id, &[], doc).map_err(|e| refused(doc, e));
    }
    if LANE_INPUT_COMMANDS.contains(&id) {
        return crate::lane_inputs::run_lane_input(id, &[], doc).map_err(|e| refused(doc, e));
    }
    if STEP_COMMANDS.contains(&id) {
        return crate::step_entry::run_step(id, &[], doc).map_err(|e| refused(doc, e));
    }
    if MODIFY_COMMANDS.contains(&id) {
        return crate::modify_commands::perform_from_menu(id, doc);
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

pub(crate) fn ran(doc: &DocumentModel) -> Ran {
    Ran {
        undo: doc.snapshot().undo_label.clone(),
        status: doc.status().map(String::from),
        focus: None,
        open_mixer_window: false,
        open_modify_panel: false,
    }
}

/// Whether `id`'s menu item shows a check mark: the current main view,
/// the panes the current view shows, Snap to Grid.
pub fn is_on(id: &str, doc: &DocumentModel) -> bool {
    match id {
        "editors.snap" => doc.layout().snap.enabled,
        "transport.overdub" => doc.layout().transport.overdub,
        "transport.punch" => doc.layout().transport.punch,
        "transport.click" => doc.last_transport().click.is_some_and(|c| c.on),
        "modify.exact_mode" => doc.exact_mode(),
        SUSPEND => automation_suspended(),
        _ if STEP_COMMANDS.contains(&id) => crate::step_entry::step_is_on(id, doc),
        _ => crate::view::is_on(id, doc),
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
    if RECORD_COMMANDS.contains(&id) {
        return crate::record::run_record(id, args, doc);
    }
    if LANE_INPUT_COMMANDS.contains(&id) {
        return crate::lane_inputs::run_lane_input(id, args, doc);
    }
    if STEP_COMMANDS.contains(&id) {
        return crate::step_entry::run_step(id, args, doc);
    }
    if MODIFY_COMMANDS.contains(&id) {
        return crate::modify_commands::run(id, args, doc);
    }
    if EDITOR_COMMANDS.contains(&id) {
        return run_editor(id, args, doc);
    }
    if VIEW_COMMANDS.contains(&id) {
        return crate::view::run(id, args, doc);
    }
    if id == "window.mixer" {
        check_names(id, args)?;
        return Ok(Ran {
            open_mixer_window: true,
            ..ran(doc)
        });
    }
    if MIXER_COMMANDS.contains(&id) {
        return crate::mixer::commands::run(id, args, doc);
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
pub(crate) fn check_names(id: &str, args: &[(String, ArgValue)]) -> Result<(), ControlError> {
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
pub(crate) fn on_flag(id: &str, args: &[(String, ArgValue)]) -> Result<Option<bool>, ControlError> {
    match args.iter().find(|(n, _)| n == "on") {
        None => Ok(None),
        Some((_, ArgValue::Flag(f))) => Ok(Some(*f)),
        Some(_) => Err(ControlError::params(id, "`on` is true or false")),
    }
}

/// An editor command: Snap to Grid (`on`, absent toggles), Piano Roll,
/// List and Strip Chart (each opens on the open detail's target or the
/// selection; nothing to show opens nothing), Close Detail Editor, Insert Event and the nudges (with the
/// settings the UI handed the document).
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
        "editors.piano_roll" => doc.show_detail(DetailEditorKind::PianoRoll),
        "editors.list" => doc.show_detail(DetailEditorKind::List),
        "editors.strip_chart" => doc.show_detail(DetailEditorKind::StripChart),
        "editors.insert_event" => {
            if !insert_event_ready(doc) {
                return Err(unavailable(NO_LIST));
            }
            doc.list_insert(None);
        }
        _ if NudgeDirection::of_command(id).is_some() => {
            let dir = NudgeDirection::of_command(id).unwrap_or(NudgeDirection::Later);
            // Owner decision 5 (plan release-fixes Task 5).
            if doc.selection_hidden() {
                return Err(unavailable(HIDDEN_SELECTION));
            }
            if !doc.transport_keys_enabled() || doc.selection().events.is_empty() {
                return Err(unavailable(NUDGE_NEEDS));
            }
            let settings = doc.nudge_settings().clone();
            if !doc.nudge(dir, &settings) {
                let why = doc.status().filter(|s| !s.is_empty());
                return Err(failed(why.unwrap_or(NUDGE_REFUSED)));
            }
        }
        _ => {
            if doc.layout().detail.is_none() {
                return Err(unavailable(NO_DETAIL));
            }
            doc.close_detail();
        }
    }
    Ok(ran(doc))
}

/// The nudges work with selected events in sight and no text being typed
/// in (Swift `AppCommands.swift:463-467`).
fn nudge_ready(doc: &DocumentModel) -> bool {
    doc.transport_keys_enabled() && !doc.selection().events.is_empty() && !doc.selection_hidden()
}

/// Insert Event works with a list open and no text being typed in.
fn insert_event_ready(doc: &DocumentModel) -> bool {
    doc.list_open() && doc.transport_keys_enabled()
}

/// Whether `id` waits while a text field has the keyboard: every
/// transport command but the loop passes (no key), arming (a chorded key)
/// and the click (Swift `TransportCommands.typesText`).
fn types_text(id: &str) -> bool {
    !matches!(
        id,
        "record.keep_pass"
            | "record.discard_pass"
            | "transport.arm_previous"
            | "transport.arm_next"
    ) && !CLICK_COMMANDS.contains(&id)
}

/// Whether a transport command can run now (its menu item is enabled).
fn transport_enabled(id: &str, doc: &DocumentModel) -> bool {
    if types_text(id) && !doc.transport_keys_enabled() {
        return false;
    }
    let seq = doc.selected_seq().is_some();
    match id {
        "transport.scrub" => false,
        "transport.record" => doc.record_button_enabled(),
        "record.keep_pass" | "record.discard_pass" => doc.record().is_loop_recording(),
        _ if CLICK_COMMANDS.contains(&id) => true,
        "transport.stop" => doc.is_playing(),
        "transport.play_stop" | "transport.play" => seq || doc.is_playing(),
        "transport.play_from_edit_in" => seq && doc.selection().time_range.is_some(),
        "transport.return_to_start" => seq && doc.return_key_enabled(),
        "transport.go_position" | "transport.go_edit_in" | "transport.go_punch" => true,
        "transport.overdub" | "transport.punch" => true,
        // Locators need no sequence, as in `run`.
        _ if locator(id).is_some() => true,
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
        // A key or menu item has no argument to give.
        "transport.set_click_level" => Err(refused(doc, unavailable(CLICK_LEVEL_NEEDS))),
        "transport.set_click_mode" => Err(refused(doc, unavailable(CLICK_MODE_NEEDS))),
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
    // A typed punch time not yet set refuses Record before the text-field
    // gate (the punch field holds the keyboard), on every path.
    if id == "transport.record" && !doc.record().is_recording() && doc.has_open_punch_draft() {
        return Err(failed(crate::record::OPEN_DRAFT));
    }
    match id {
        "transport.record" => {
            if let Some(e) = text_gate(doc) {
                return Err(e);
            }
            doc.toggle_record()?;
        }
        "record.keep_pass" | "record.discard_pass" => {
            doc.press_pass(id == "record.keep_pass")?;
        }
        _ if CLICK_COMMANDS.contains(&id) => run_click(id, args, doc)?,
        "transport.arm_previous" | "transport.arm_next" => {
            move_record_arm(id == "transport.arm_next", doc)?;
        }
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
        "transport.go_position" | "transport.go_edit_in" | "transport.go_punch" => {
            if let Some(e) = text_gate(doc) {
                return Err(e);
            }
            let mut ran = ran(doc);
            ran.focus = Some(match id {
                "transport.go_position" => FieldFocus::Position,
                "transport.go_edit_in" => FieldFocus::EditIn,
                _ => FieldFocus::Punch,
            });
            return Ok(ran);
        }
        "transport.overdub" | "transport.punch" => {
            if let Some(e) = text_gate(doc) {
                return Err(e);
            }
            // The layout's switch (saved in `ui.json`): no undo step.
            let overdub = id == "transport.overdub";
            doc.update_layout(|l| {
                let flag = if overdub {
                    &mut l.transport.overdub
                } else {
                    &mut l.transport.punch
                };
                *flag = on.unwrap_or(!*flag);
            });
        }
        _ if locator(id).is_some() => {
            if let Some(e) = text_gate(doc) {
                return Err(e);
            }
            if let Some((op, n)) = locator(id) {
                run_locator(op, n, doc)?;
            }
        }
        _ => run_key_command(id, doc)?,
    }
    Ok(ran(doc))
}

/// One of the click commands with the channel's `args` as given: the
/// session checks them and makes one undo step; the transport is read
/// again, so Click's check follows. A refusal raises no alert.
fn run_click(
    id: &str,
    args: &[(String, ArgValue)],
    doc: &mut DocumentModel,
) -> Result<(), ControlError> {
    if doc.gesture_open() {
        return Err(unavailable(GESTURE_OPEN));
    }
    let result = doc
        .session()
        .run_click(id.to_owned(), crate::mixer::commands::control_args(args));
    doc.apply_result(result).map_err(|e| from_viz(&e))?;
    let _ = doc.poll_transport();
    Ok(())
}

/// Arm Previous or Next Track on the shown sequence: one undo step "Move
/// Record Arm" (with none armed, Previous arms the last track). The core's
/// refusal, Busy too, shows in the status line.
fn move_record_arm(forward: bool, doc: &mut DocumentModel) -> Result<(), ControlError> {
    let Some(seq) = doc.selected_seq() else {
        return Err(unavailable(NO_SEQUENCE));
    };
    if doc.gesture_open() {
        return Err(unavailable(GESTURE_OPEN));
    }
    let result = doc.session().move_record_arm(seq, forward);
    doc.apply_result(result).map_err(|e| {
        let mut message = error_message(&e);
        if message.is_empty() {
            message = ARM_DID_NOT_MOVE.into();
        }
        doc.set_status(Some(message.clone()));
        let code = if matches!(e, VizError::Busy { .. }) {
            ErrorCode::Busy
        } else {
            ErrorCode::Failed
        };
        ControlError::new(code, message)
    })
}

/// Go to, set (at the transport's tick, read first) or clear locator
/// `n`: the layout's marks, no undo step and no sequence needed.
fn run_locator(op: &str, n: usize, doc: &mut DocumentModel) -> Result<(), ControlError> {
    let mut marks = doc.layout().transport.clone();
    match op {
        "go" => {
            let Some(t) = marks.locator(n) else {
                return Err(unavailable(&format!("Locator {n} is not set.")));
            };
            return doc.move_to(t).map_err(|e| from_viz(&e));
        }
        "set" => {
            let _ = doc.poll_transport();
            if !marks.set_locator(n, Some(doc.last_transport().tick)) {
                return Err(failed(format!(
                    "Locator {n} cannot be set at this position."
                )));
            }
        }
        _ => {
            if !marks.set_locator(n, None) {
                return Err(failed(format!("There is no locator {n}.")));
            }
        }
    }
    doc.update_layout(|l| l.transport = marks);
    Ok(())
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
