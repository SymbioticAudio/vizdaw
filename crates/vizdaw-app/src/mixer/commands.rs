//! Automation and the mixer from the control channel (Swift
//! `AutomationCommands`, `MixerCommands`): `automation.set_mode` sets
//! strips' modes as one undo step, `automation.gesture` touches, moves
//! and releases a target as the channel's own holder,
//! `automation.release_all` frees the channel's touches, and
//! `mixer.new_bus` adds a bus (with a send from `strip`, one undo step).
//! The strip edits (`mixer.set_strip`, `set_output`, `add_send`,
//! `set_send`, `remove_send`, `remove_bus`) and the plug-in edits
//! (`mixer.add_insert`, `remove_insert`, `move_insert`, `set_bypass`,
//! `set_eq`, `set_param`) are each one `run_mixer` call
//! with the arguments as given (the core checks them, and counts sends
//! from 1 as the channel does), and so is every group and VCA edit
//! (registry `GROUP_IDS`); `track.new_plugin_instrument` is one too,
//! with the selection's sequence and track filled in; `mixer.set_colour`
//! sets a strip's colour; `automation.mode.*` set the selected strips'
//! mode; `automation.suspend` is the app-wide switch;
//! `destination.set_midi_mix` sets a MIDI strip's mix.
//!
//! Every argument is checked before the core is called and is
//! `invalid_params` naming the parameter. The core's refusals are the
//! call's `failed` (with the Swift alert title in front), its Busy is
//! `busy`. Nothing here opens a dialog.

use core_control::registry::{self, ArgValue};
use core_control::{ControlError, ErrorCode};
use vizdaw_ffi::VizError;
use vizdaw_ffi::automation::{
    AutomationModeRow, GestureHolderRow, GesturePhaseRow, automation_suspended,
    parse_automation_target, set_automation_suspended,
};
use vizdaw_ffi::control::{ControlArg, ControlValue};
use vizdaw_ffi::mixer::StripKindRow;

use super::math::{AutomationModes, MixerMath};
use super::midi::SET_MIDI_MIX_FAILED;
use super::palette::StripPalette;
use crate::alerts::error_message;
use crate::commands::{Ran, check_names, from_viz, ran};
use crate::document::DocumentModel;
use crate::editor::GESTURE_OPEN;

/// Why master gets no new bus.
pub const MASTER_HAS_NO_SENDS: &str = "Master has no sends.";
/// The longest `strips` text read (the registry's string limit).
pub const MAX_STRIPS_TEXT: usize = 4096;
/// The longest `value` text read: more digits than a double holds.
pub const MAX_VALUE_TEXT: usize = 32;

const BAD_MODE: &str = "automation.set_mode: `mode` is off, read, touch, latch, write or trim";
const BAD_STRIPS: &str = "automation.set_mode: `strips` is all or strip IDs such as \"1,2\"";
const BAD_TARGET: &str = "automation.gesture: `target` is fader:<strip>, pan:<strip>, \
                          mute:<strip>, solo:<strip>, send:<strip>:<n> (n from 1), \
                          param:<slot>:<param> or vca:<vca>";
const BAD_VALUE: &str = "automation.gesture: `value` is a number from 0 to 1 such as \"0.5\"";
const NEEDS_VALUE: &str = "automation.gesture: `value` is needed for value (0 to 1)";
const RELEASE_VALUE: &str = "automation.gesture: release takes no `value`";
const BAD_PHASE: &str = "automation.gesture: `phase` is touch, value or release";
const AUTOMATION_FAILED: &str = "Automation Failed";
/// Why a mode command has nothing to set.
pub const NO_STRIPS_SELECTED: &str = "Select strips in the mixer first.";
/// The app-wide automation switch.
pub const SUSPEND: &str = "automation.suspend";
/// A MIDI strip's mix from the channel.
pub const SET_MIDI_MIX: &str = "destination.set_midi_mix";
const MIDI_MIX_USAGE: &str = "destination.set_midi_mix: `dest` names an external MIDI \
                              destination; `volume` and `pan` are 0 to 127 or null, `mute` is \
                              true or false; give at least one";

/// The strip edits and the plug-in edits (Swift
/// `MixerCommands.coProducerEdits`): one `run_mixer` call each, answered
/// with the registry's title.
pub const STRIP_EDITS: &[&str] = &[
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
];

/// A new instrument track with a plug-in (or the built-in synth) on its
/// strip: one `run_mixer` call, answered with the new track's name.
pub const NEW_PLUGIN_INSTRUMENT: &str = "track.new_plugin_instrument";

/// The commands a key (or a menu item) runs with the selected strips as
/// `strips`: the channel never acts on the selection.
pub const SELECTION_FILLED: &[&str] = &["mixer.new_group", "mixer.new_vca"];

/// The Automation menu's mode commands.
pub const MODE_COMMANDS: &[(&str, AutomationModeRow)] = &[
    ("automation.mode.off", AutomationModeRow::Off),
    ("automation.mode.read", AutomationModeRow::Read),
    ("automation.mode.touch", AutomationModeRow::Touch),
    ("automation.mode.latch", AutomationModeRow::Latch),
    ("automation.mode.write", AutomationModeRow::Write),
    ("automation.mode.trim", AutomationModeRow::Trim),
];

/// The mode `id` (an `automation.mode.*`) sets.
pub fn mode_of(id: &str) -> Option<AutomationModeRow> {
    MODE_COMMANDS
        .iter()
        .find(|(m, _)| *m == id)
        .map(|(_, r)| *r)
}

/// `text` as a mode, spelt as the registry's choice does.
pub fn mode(text: &str) -> Option<AutomationModeRow> {
    Some(match text {
        "off" => AutomationModeRow::Off,
        "read" => AutomationModeRow::Read,
        "touch" => AutomationModeRow::Touch,
        "latch" => AutomationModeRow::Latch,
        "write" => AutomationModeRow::Write,
        "trim" => AutomationModeRow::Trim,
        _ => return None,
    })
}

/// `text` as strip IDs: `"all"` (every strip in `all`, in its order) or
/// IDs separated by commas, ASCII digits only; a repeated ID is kept once
/// at its first place. `None` for anything else (an empty item, a sign, a
/// space, an overflow, more than 4096 bytes).
pub fn strips(text: &str, all: &[u64]) -> Option<Vec<u64>> {
    if text == "all" {
        return Some(all.to_vec());
    }
    if text.is_empty() || text.len() > MAX_STRIPS_TEXT {
        return None;
    }
    let mut out = Vec::new();
    for item in text.split(',') {
        if item.is_empty() || !item.bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }
        let id: u64 = item.parse().ok()?;
        if !out.contains(&id) {
            out.push(id);
        }
    }
    Some(out)
}

/// `text` as a normalised value: a plain decimal, ASCII digits with at
/// most one `.` and nothing else (no sign, exponent, hex or spaces), at
/// most 32 characters, from 0 to 1.
pub fn value(text: &str) -> Option<f64> {
    if text.is_empty() || text.len() > MAX_VALUE_TEXT {
        return None;
    }
    let mut dots = 0;
    let mut digits = 0;
    for b in text.bytes() {
        match b {
            b'.' => dots += 1,
            b'0'..=b'9' => digits += 1,
            _ => return None,
        }
    }
    if dots > 1 || digits == 0 {
        return None;
    }
    let v: f64 = text.parse().ok()?;
    (v.is_finite() && (0.0..=1.0).contains(&v)).then_some(v)
}

/// A core error as the call's failure: Busy is `busy` in the core's
/// words; anything else is `failed`, "<title>: <message>" (Swift
/// `ControlFailure.thrown`).
pub fn thrown(e: &VizError, title: &str) -> ControlError {
    if let VizError::Busy { message } = e {
        return ControlError::new(ErrorCode::Busy, message.clone());
    }
    ControlError::new(ErrorCode::Failed, format!("{title}: {}", error_message(e)))
}

fn invalid(message: impl Into<String>) -> ControlError {
    ControlError::new(ErrorCode::InvalidParams, message)
}

fn arg<'a>(args: &'a [(String, ArgValue)], name: &str) -> Option<&'a ArgValue> {
    args.iter().find(|(n, _)| n == name).map(|(_, v)| v)
}

fn text_arg<'a>(args: &'a [(String, ArgValue)], name: &str) -> Option<&'a str> {
    match arg(args, name) {
        Some(ArgValue::Text(t)) => Some(t),
        _ => None,
    }
}

/// The new document state after a gesture, when it moved (a static
/// gesture's edit, a released touch's step).
fn catch_up(doc: &mut DocumentModel) {
    if let Ok(s) = doc.session().snapshot()
        && s.version != doc.snapshot().version
    {
        doc.apply(s);
    }
}

/// Runs `automation.set_mode|gesture|release_all|mode.*`, `mixer.new_bus`,
/// `mixer.set_colour`, a strip edit or a group or VCA edit.
pub(crate) fn run(
    id: &str,
    args: &[(String, ArgValue)],
    doc: &mut DocumentModel,
) -> Result<Ran, ControlError> {
    match id {
        "automation.set_mode" => set_mode(args, doc),
        "automation.gesture" => gesture(args, doc),
        "automation.release_all" => release_all(args, doc),
        "mixer.new_bus" => new_bus(args, doc),
        "mixer.set_colour" => set_colour(args, doc),
        SET_MIDI_MIX => set_midi_mix(args, doc),
        NEW_PLUGIN_INSTRUMENT => new_plugin_instrument(args, doc),
        _ if STRIP_EDITS.contains(&id) || registry::GROUP_IDS.contains(&id) => {
            strip_edit(id, args, doc)
        }
        _ if mode_of(id).is_some() => mode_command(id, doc),
        _ => Err(ControlError::new(
            ErrorCode::UnknownId,
            format!("There is no command {id}."),
        )),
    }
}

fn set_mode(args: &[(String, ArgValue)], doc: &mut DocumentModel) -> Result<Ran, ControlError> {
    check_names("automation.set_mode", args)?;
    let mode = text_arg(args, "mode")
        .and_then(mode)
        .ok_or_else(|| invalid(BAD_MODE))?;
    let text = text_arg(args, "strips").ok_or_else(|| invalid(BAD_STRIPS))?;
    if doc.gesture_open() {
        return Err(ControlError::new(ErrorCode::Unavailable, GESTURE_OPEN));
    }
    let all: Vec<u64> = if text == "all" {
        let m = doc
            .session()
            .mixer()
            .map_err(|e| thrown(&e, AutomationModes::FAILED))?;
        m.strips.iter().map(|s| s.id).collect()
    } else {
        vec![]
    };
    let ids = strips(text, &all).ok_or_else(|| invalid(BAD_STRIPS))?;
    let result = doc.session().set_automation_mode(ids, mode);
    doc.apply_result(result)
        .map_err(|e| thrown(&e, AutomationModes::FAILED))?;
    Ok(ran(doc))
}

fn gesture(args: &[(String, ArgValue)], doc: &mut DocumentModel) -> Result<Ran, ControlError> {
    check_names("automation.gesture", args)?;
    let target = text_arg(args, "target")
        .and_then(|t| parse_automation_target(t.to_owned()))
        .ok_or_else(|| invalid(BAD_TARGET))?;
    let given = arg(args, "value").is_some();
    let v = text_arg(args, "value").and_then(value);
    if given && v.is_none() {
        return Err(invalid(BAD_VALUE));
    }
    let phase = match text_arg(args, "phase") {
        Some("touch") => GesturePhaseRow::Touch { value: v },
        Some("value") => GesturePhaseRow::Value {
            value: v.ok_or_else(|| invalid(NEEDS_VALUE))?,
        },
        Some("release") if given => return Err(invalid(RELEASE_VALUE)),
        Some("release") => GesturePhaseRow::Release,
        _ => return Err(invalid(BAD_PHASE)),
    };
    let alone = matches!(arg(args, "alone"), Some(ArgValue::Flag(true)));
    doc.session()
        .gesture_as(target, phase, GestureHolderRow::Channel, alone)
        .map_err(|e| thrown(&e, AUTOMATION_FAILED))?;
    catch_up(doc);
    Ok(ran(doc))
}

/// What `automation.release_all` says for `n` released touches.
pub fn released_status(n: u32) -> String {
    match n {
        0 => "The channel holds no touch.".into(),
        1 => "Released 1 touch.".into(),
        n => format!("Released {n} touches."),
    }
}

/// `automation.release_all`: the core releases every touch the control
/// channel holds; the mouse's and the surfaces' holds stay. Takes no
/// arguments. The count shows in the status line.
fn release_all(args: &[(String, ArgValue)], doc: &mut DocumentModel) -> Result<Ran, ControlError> {
    if !args.is_empty() {
        return Err(invalid("automation.release_all takes no parameters"));
    }
    let n = doc
        .session()
        .release_channel_touches()
        .map_err(|e| thrown(&e, AUTOMATION_FAILED))?;
    catch_up(doc);
    doc.set_status(Some(released_status(n)));
    Ok(ran(doc))
}

/// `mixer.new_bus`: with `strip`, a bus and a post-fader 0 dB send to it
/// from that strip, one undo step "New Bus"; without, a bus alone ("Add
/// Bus"). `name` defaults to the next free "Bus N". The status line (and
/// the answer) is the bus's name.
fn new_bus(args: &[(String, ArgValue)], doc: &mut DocumentModel) -> Result<Ran, ControlError> {
    check_names("mixer.new_bus", args)?;
    if doc.gesture_open() {
        return Err(ControlError::new(ErrorCode::Unavailable, GESTURE_OPEN));
    }
    let strip = match arg(args, "strip") {
        None => None,
        Some(ArgValue::Int(i)) if *i >= 0 => Some(*i as u64),
        Some(_) => {
            return Err(invalid(
                "mixer.new_bus: `strip` is a strip ID (see state.get mixer)",
            ));
        }
    };
    let given = match arg(args, "name") {
        None => None,
        Some(ArgValue::Text(t)) if !t.contains('\0') => {
            let trimmed = t.trim();
            if trimmed.is_empty() {
                return Err(invalid("mixer.new_bus: `name` is not blank"));
            }
            Some(trimmed.to_owned())
        }
        Some(_) => return Err(invalid("mixer.new_bus: `name` is text without a NUL")),
    };
    const FAILED: &str = "New Bus Failed";
    let mixer = doc.session().mixer().map_err(|e| thrown(&e, FAILED))?;
    if let Some(strip) = strip {
        let Some(row) = mixer.strips.iter().find(|s| s.id == strip) else {
            return Err(ControlError::new(
                ErrorCode::UnknownId,
                format!("The mixer has no strip {strip}."),
            ));
        };
        if row.kind == StripKindRow::Master {
            return Err(invalid(MASTER_HAS_NO_SENDS));
        }
    }
    let name = given.unwrap_or_else(|| MixerMath::next_bus_name(&mixer.buses));
    let result = match strip {
        Some(strip) => doc.session().new_bus_from(strip, &name),
        None => doc.session().add_bus(&name),
    };
    doc.apply_result(result).map_err(|e| thrown(&e, FAILED))?;
    doc.set_status(Some(name));
    Ok(ran(doc))
}

/// The channel's arguments as the core's.
pub(crate) fn control_args(args: &[(String, ArgValue)]) -> Vec<ControlArg> {
    args.iter()
        .map(|(name, v)| ControlArg {
            name: name.clone(),
            value: match v {
                ArgValue::Text(value) => ControlValue::Text {
                    value: value.clone(),
                },
                ArgValue::Flag(value) => ControlValue::Flag { value: *value },
                ArgValue::Int(value) => ControlValue::Int { value: *value },
            },
        })
        .collect()
}

/// A strip, group or VCA edit: one `run_mixer` call with the arguments as given (a send
/// `index` counts from 1, as the core takes it). The status line is the
/// command's title; a refusal is `failed` in the session's words, Busy
/// `busy`.
fn strip_edit(
    id: &str,
    args: &[(String, ArgValue)],
    doc: &mut DocumentModel,
) -> Result<Ran, ControlError> {
    if doc.gesture_open() {
        return Err(ControlError::new(ErrorCode::Unavailable, GESTURE_OPEN));
    }
    let result = doc.session().run_mixer(id, control_args(args));
    doc.apply_result(result).map_err(|e| from_viz(&e))?;
    doc.set_status(registry::find(id).map(|c| c.title.to_owned()));
    Ok(ran(doc))
}

/// `track.new_plugin_instrument {plugin, seq?, after?}` (Swift
/// `MixerCommands.edit`, `M/MixerCommands.swift:107-138`): `seq` defaults
/// to the shown sequence and `after` (in it) to the selection's first
/// track, as New Track passes them; the core puts the track at the end
/// without one. One `run_mixer` call; the status line is the new track's
/// name (else the registry's title) and the new track is selected when
/// its sequence is shown. A refusal is `failed` in the core's words, Busy
/// `busy`; an open gesture `unavailable`.
fn new_plugin_instrument(
    args: &[(String, ArgValue)],
    doc: &mut DocumentModel,
) -> Result<Ran, ControlError> {
    if doc.gesture_open() {
        return Err(ControlError::new(ErrorCode::Unavailable, GESTURE_OPEN));
    }
    let mut args = args.to_vec();
    let given = |args: &[(String, ArgValue)], name: &str| args.iter().any(|(n, _)| n == name);
    let seq = if given(&args, "seq") {
        match arg(&args, "seq") {
            Some(ArgValue::Int(i)) => u64::try_from(*i).ok(),
            _ => None,
        }
    } else {
        let shown = doc.selected_seq();
        if let Some(s) = shown.and_then(|s| i64::try_from(s).ok()) {
            args.push(("seq".into(), ArgValue::Int(s)));
        }
        shown
    };
    if !given(&args, "after")
        && seq.is_some()
        && seq == doc.selected_seq()
        && let Some(lane) = doc.selected_lane().and_then(|l| i64::try_from(l).ok())
    {
        args.push(("after".into(), ArgValue::Int(lane)));
    }
    let before: Vec<u64> = seq
        .map(|s| {
            doc.session()
                .lanes(s)
                .unwrap_or_default()
                .iter()
                .map(|l| l.id)
                .collect()
        })
        .unwrap_or_default();
    let result = doc
        .session()
        .run_mixer(NEW_PLUGIN_INSTRUMENT, control_args(&args));
    doc.apply_result(result).map_err(|e| from_viz(&e))?;
    doc.set_status(registry::find(NEW_PLUGIN_INSTRUMENT).map(|c| c.title.to_owned()));
    let made = seq.and_then(|s| {
        doc.session()
            .lanes(s)
            .unwrap_or_default()
            .into_iter()
            .find(|l| !before.contains(&l.id))
    });
    if let (Some(s), Some(made)) = (seq, made) {
        doc.set_status(Some(made.name.clone()));
        // The new track is selected, as New Track selects it.
        if Some(s) == doc.selected_seq() {
            doc.select_track(made.id, false);
        }
    }
    Ok(ran(doc))
}

/// `mixer.set_colour {strip, colour}`: `colour` is one of the palette's
/// names or `none` (the kind's colour). A strip the song lacks is the
/// session's refusal, in its words.
fn set_colour(args: &[(String, ArgValue)], doc: &mut DocumentModel) -> Result<Ran, ControlError> {
    const ID: &str = "mixer.set_colour";
    let strip = match arg(args, "strip") {
        Some(ArgValue::Int(i)) if *i >= 0 => *i as u64,
        _ => {
            return Err(invalid(format!(
                "{ID}: `strip` is a strip ID (see state.get mixer)"
            )));
        }
    };
    let colour = match text_arg(args, "colour") {
        Some("none") => None,
        Some(name) if StripPalette::named(name).is_some() => StripPalette::named(name),
        _ => {
            return Err(invalid(format!(
                "{ID}: `colour` is none, red, orange, yellow, green, teal, blue, purple or pink"
            )));
        }
    };
    if doc.gesture_open() {
        return Err(ControlError::new(ErrorCode::Unavailable, GESTURE_OPEN));
    }
    let result = doc.session().set_strip_colour(strip, colour);
    doc.apply_result(result).map_err(|e| from_viz(&e))?;
    Ok(ran(doc))
}

/// A controller of `destination.set_midi_mix`: `None` when absent,
/// `Some(None)` for null (unset, the registry's `none`), else 0…127.
/// `Err` for anything else.
fn controller(args: &[(String, ArgValue)], name: &str) -> Result<Option<Option<u8>>, ControlError> {
    match arg(args, name) {
        None => Ok(None),
        Some(ArgValue::Int(v)) => u8::try_from(*v)
            .ok()
            .filter(|v| *v <= 127)
            .map(|v| Some(Some(v)))
            .ok_or_else(|| invalid(MIDI_MIX_USAGE)),
        Some(ArgValue::Text(t)) if t == registry::NULL_TEXT => Ok(Some(None)),
        Some(_) => Err(invalid(MIDI_MIX_USAGE)),
    }
}

/// `destination.set_midi_mix {dest, volume?, pan?, mute?}` (Swift
/// `SynthCommands.setMidiMix`): an absent key keeps what is set, a null
/// volume or pan unsets it. A `dest` that names no external MIDI
/// destination is `invalid_params`; the core's refusal is `failed` ("Set
/// MIDI Mix Failed: …"), its Busy `busy`. One undo step "Set MIDI Mix".
fn set_midi_mix(args: &[(String, ArgValue)], doc: &mut DocumentModel) -> Result<Ran, ControlError> {
    check_names(SET_MIDI_MIX, args)?;
    if doc.gesture_open() {
        return Err(ControlError::new(ErrorCode::Unavailable, GESTURE_OPEN));
    }
    let dest = match arg(args, "dest") {
        Some(ArgValue::Int(i)) if *i >= 0 => *i as u64,
        _ => return Err(invalid(MIDI_MIX_USAGE)),
    };
    let volume = controller(args, "volume")?;
    let pan = controller(args, "pan")?;
    let mute = match arg(args, "mute") {
        None => None,
        Some(ArgValue::Flag(v)) => Some(*v),
        Some(_) => return Err(invalid(MIDI_MIX_USAGE)),
    };
    if volume.is_none() && pan.is_none() && mute.is_none() {
        return Err(invalid(MIDI_MIX_USAGE));
    }
    let all = doc.session().destinations().map_err(|_| {
        ControlError::new(
            ErrorCode::Failed,
            format!("{SET_MIDI_MIX_FAILED}: the destinations cannot be read"),
        )
    })?;
    let Some(row) = all.iter().find(|d| d.external && d.id == dest) else {
        return Err(invalid(format!(
            "{SET_MIDI_MIX}: destination {dest} is not an external MIDI destination"
        )));
    };
    let mut mix = row.midi_mix.clone();
    if let Some(v) = volume {
        mix.volume = v;
    }
    if let Some(p) = pan {
        mix.pan = p;
    }
    if let Some(m) = mute {
        mix.mute = m;
    }
    let result = doc.session().set_midi_mix(dest, mix);
    doc.apply_result(result)
        .map_err(|e| thrown(&e, SET_MIDI_MIX_FAILED))?;
    Ok(ran(doc))
}

/// `automation.mode.<mode>` from the menu, a key or the channel: the
/// selected strips that exist now get the mode, one undo step. Arguments
/// are ignored: the ID and the selection decide.
fn mode_command(id: &str, doc: &mut DocumentModel) -> Result<Ran, ControlError> {
    let Some(mode) = mode_of(id) else {
        return Err(ControlError::new(
            ErrorCode::UnknownId,
            format!("There is no command {id}."),
        ));
    };
    if doc.gesture_open() {
        return Err(ControlError::new(ErrorCode::Unavailable, GESTURE_OPEN));
    }
    if doc.selected_strips().is_empty() {
        return Err(ControlError::new(
            ErrorCode::Unavailable,
            NO_STRIPS_SELECTED,
        ));
    }
    let ids = doc
        .selected_strips_that_exist()
        .map_err(|e| thrown(&e, AutomationModes::FAILED))?;
    if ids.is_empty() {
        return Err(ControlError::new(
            ErrorCode::Unavailable,
            NO_STRIPS_SELECTED,
        ));
    }
    let result = doc.session().set_automation_mode(ids, mode);
    doc.apply_result(result)
        .map_err(|e| thrown(&e, AutomationModes::FAILED))?;
    Ok(ran(doc))
}

/// `automation.suspend {on?}`: sets the process-wide switch (absent:
/// toggles). Needs no document.
pub fn suspend(args: &[(String, ArgValue)]) -> Result<(), ControlError> {
    let on = match arg(args, "on") {
        None => !automation_suspended(),
        Some(ArgValue::Flag(on)) => *on,
        Some(_) => return Err(invalid("automation.suspend: `on` is true or false")),
    };
    set_automation_suspended(on);
    Ok(())
}
