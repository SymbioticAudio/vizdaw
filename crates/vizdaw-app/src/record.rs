//! Recording in a document (Swift `RecordModel` and `RecordOptions`, the
//! model half): what Record asks the core for, what the core reports while
//! it records, the record popover's options and punch drafts, and the
//! readouts (time left, loop counters, overdub note, the mode switch).
//! Replace, punch and loop come from the transport (overdub, the punch
//! marks, Loop); count-in, pre-roll and MIDI passes as takes are kept here
//! for the popover and `record.set`, starting from Settings › Recording's
//! defaults. The Recovered Recordings offer is `document/recovery.rs`.

use std::collections::HashMap;

use core_control::registry::ArgValue;
use core_control::{ControlError, ErrorCode};
use vizdaw_ffi::BitDepthRow;
use vizdaw_ffi::VizError;
use vizdaw_ffi::edit::LaneInputRow;
use vizdaw_ffi::record::{RecordSpecRow, RecordStateRow};
use vizdaw_ffi::selection::TickRange;

use crate::alerts::error_message;
use crate::commands::{self, Ran, check_names, from_viz, ran};
use crate::document::DocumentModel;
use crate::editor::GESTURE_OPEN;
use crate::mixer::commands::{control_args, thrown};
use crate::transport_fields::{TransportField, commit_field, field_text};

/// Why Record is refused with no sequence shown.
pub const NO_SEQUENCE: &str = "Select a sequence first.";
/// Why Record is refused while step entry is on (spec §6.4).
pub const STEP_ENTRY_ON: &str = "Step entry is on: turn it off to record.";
/// Why Record is refused while a punch draft is open (plan 7d controller
/// ruling I5).
pub const OPEN_DRAFT: &str = "Press Return to set the punch, or Esc to discard it.";
/// Why keep and discard do nothing.
pub const NOT_LOOP_RECORDING: &str = "Keep and discard act on loop passes while recording.";
/// Spec §6.4: one track plays one region at a time.
pub const OVERDUB_NOTE: &str =
    "Audio overdub replaces what it overlaps: record into another track or take to layer.";
/// File › Save while recording (plan 5d-1 Ruling 19).
pub const SAVED_DURING_PASS: &str = "Saved. The take is saved when recording stops.";
/// Save of a new, untitled project while recording.
pub const SAVE_UNTITLED_WHILE_RECORDING: &str =
    "Stop recording to save this new project: its take is being written into it.";
/// Save As while recording: the take is written inside this project.
pub const SAVE_AS_WHILE_RECORDING: &str =
    "Stop recording to use Save As: the take is being written into this project.";
/// Recording stops below this much free space (spec §6.2: 100 MiB).
pub const STOP_BYTES: u64 = 100 << 20;
/// The mode switch's help.
pub const MODE_HELP: &str =
    "Replace removes the notes in the recorded range; Overdub keeps them (transport.overdub)";
/// "MIDI passes as takes"'s help (plan 5c-2 takes Ruling 1).
pub const MIDI_TAKES_HELP: &str = "Loop recording: each MIDI pass becomes a new take. Off: passes layer into the playing take. Audio passes always become takes.";
/// The text-editing owner while a popover punch field has focus.
pub const POPOVER_OWNER: &str = "record_popover";
/// `record.set` while a pass records.
pub const STOP_RECORDING_FIRST: &str = "Stop recording first.";
/// `record.recover` with nothing to recover.
pub const NO_RECOVERED: &str = "There are no recovered recordings.";
/// `midi.inject` while another document holds the audio engine.
pub const INJECT_NO_ENGINE: &str =
    "Play or record first: this document does not hold the audio engine.";
/// `midi.inject`'s `bytes` (or `port`) are not what it takes.
const INJECT_HEX: &str = "`bytes` are hex bytes such as \"90 3c 64\"";
/// The most bytes `midi.inject` takes at once.
const INJECT_MAX: usize = 1024;
/// The most bars of count-in and pre-roll.
pub const MAX_COUNT_IN: u8 = 4;
pub const MAX_PRE_ROLL: u8 = 8;

/// Where the record popover shows (plan 7a final review I2): the document
/// window's transport bar or the mixer window's.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PopoverHost {
    #[default]
    Document,
    MixerWindow,
}

/// What a new document's record options start as (Settings › Recording;
/// plan 5d-2 Ruling 3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct RecordDefaults {
    pub count_in_bars: u8,
    pub pre_roll_bars: u8,
    pub midi_passes_as_takes: bool,
}

/// A document's record options, popover and last record state.
#[derive(Debug, Clone)]
pub struct RecordModel {
    count_in_bars: u8,
    /// The last non-zero count-in, for the Count-in toggle (plan 5f-4
    /// Ruling 16).
    last_count_in: u8,
    pre_roll_bars: u8,
    midi_passes_as_takes: bool,
    shows_popover: bool,
    popover_host: PopoverHost,
    /// Text typed in the popover's punch fields, until Return, focus
    /// moving within or leaving the pair, or the popover closing.
    punch_drafts: HashMap<TransportField, String>,
    /// The bar's Punch In or Out while it is typed in, and its text.
    bar_punch_draft: Option<(TransportField, String)>,
    /// What the core last reported.
    state: Option<RecordStateRow>,
}

impl Default for RecordModel {
    fn default() -> Self {
        RecordModel::new(RecordDefaults::default())
    }
}

impl RecordModel {
    pub fn new(d: RecordDefaults) -> Self {
        RecordModel {
            count_in_bars: d.count_in_bars.min(MAX_COUNT_IN),
            last_count_in: d.count_in_bars.clamp(1, MAX_COUNT_IN),
            pre_roll_bars: d.pre_roll_bars.min(MAX_PRE_ROLL),
            midi_passes_as_takes: d.midi_passes_as_takes,
            shows_popover: false,
            popover_host: PopoverHost::Document,
            punch_drafts: HashMap::new(),
            bar_punch_draft: None,
            state: None,
        }
    }

    /// Bars of count-in (0–4).
    pub fn count_in_bars(&self) -> u8 {
        self.count_in_bars
    }

    /// Sets the count-in, at most 4 bars.
    pub fn set_count_in(&mut self, bars: u8) {
        self.count_in_bars = bars.min(MAX_COUNT_IN);
        if self.count_in_bars > 0 {
            self.last_count_in = self.count_in_bars;
        }
    }

    /// Count-in off ↔ its last bar count (the Count-in toggle). It only
    /// changes the next Record.
    pub fn toggle_count_in(&mut self) {
        let bars = if self.count_in_bars > 0 {
            0
        } else {
            self.last_count_in
        };
        self.set_count_in(bars);
    }

    /// Bars of pre-roll (0–8).
    pub fn pre_roll_bars(&self) -> u8 {
        self.pre_roll_bars
    }

    /// Sets the pre-roll, at most 8 bars.
    pub fn set_pre_roll(&mut self, bars: u8) {
        self.pre_roll_bars = bars.min(MAX_PRE_ROLL);
    }

    /// Kept MIDI loop passes become takes; off, they layer into the playing
    /// take (plan 5c-2 takes Ruling 1). Audio passes always do.
    pub fn midi_passes_as_takes(&self) -> bool {
        self.midi_passes_as_takes
    }

    pub fn set_midi_passes_as_takes(&mut self, on: bool) {
        self.midi_passes_as_takes = on;
    }

    /// What the core last reported.
    pub fn state(&self) -> Option<&RecordStateRow> {
        self.state.as_ref()
    }

    /// A pass is recording (as last read).
    pub fn is_recording(&self) -> bool {
        self.state.as_ref().is_some_and(|s| s.recording)
    }

    /// A loop pass is recording: keep and discard apply.
    pub fn is_loop_recording(&self) -> bool {
        self.is_recording()
            && self
                .state
                .as_ref()
                .and_then(|s| s.spec.as_ref())
                .is_some_and(|s| s.looped)
    }

    /// The popover is open (in either window).
    pub fn shows_popover(&self) -> bool {
        self.shows_popover
    }

    /// The popover is open in `host`.
    pub fn shows_popover_in(&self, host: PopoverHost) -> bool {
        self.shows_popover && self.popover_host == host
    }

    /// The window whose chevron opened the popover.
    pub fn popover_host(&self) -> PopoverHost {
        self.popover_host
    }

    /// Whether the record popover may close for a press: a press on a
    /// Record button with a punch draft open keeps it open, so Record
    /// refuses with the draft still shown and its field keeping the
    /// keyboard. Every other close is allowed (and drops the drafts). The
    /// bar's punch fields keep their edit by the same rule.
    pub fn close_allowed(press_on_record: bool, draft_open: bool) -> bool {
        !(press_on_record && draft_open)
    }
}

/// "1 h 5 min left" for `seconds` of recording.
pub fn remaining(seconds: f64) -> String {
    if seconds < 60.0 {
        return "Less than a minute left".into();
    }
    let minutes = (seconds / 60.0) as u64;
    if minutes < 60 {
        return format!("{minutes} min left");
    }
    let (h, r) = (minutes / 60, minutes % 60);
    if r == 0 {
        format!("{h} h left")
    } else {
        format!("{h} h {r} min left")
    }
}

fn unavailable(message: &str) -> ControlError {
    ControlError::new(ErrorCode::Unavailable, message)
}

fn refused(message: &str) -> ControlError {
    ControlError::new(ErrorCode::Failed, message)
}

/// `text` as hex byte pairs separated by white space (`90 3c 64`): one
/// to 1024 of them, or `None` (Swift `ControlLanes.hexBytes`).
pub fn hex_bytes(text: &str) -> Option<Vec<u8>> {
    let parts: Vec<&str> = text.split_whitespace().collect();
    if parts.is_empty() || parts.len() > INJECT_MAX {
        return None;
    }
    parts
        .into_iter()
        .map(|p| {
            (p.len() == 2 && p.bytes().all(|b| b.is_ascii_hexdigit()))
                .then(|| u8::from_str_radix(p, 16).ok())
                .flatten()
        })
        .collect()
}

fn arg<'a>(args: &'a [(String, ArgValue)], name: &str) -> Option<&'a ArgValue> {
    args.iter().find(|(n, _)| n == name).map(|(_, v)| v)
}

/// The flag `name`: absent, or true or false.
fn flag_arg(
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

/// The bar count `name`, at most `max`: absent, or a whole number in range.
fn bars_arg(
    id: &str,
    args: &[(String, ArgValue)],
    name: &str,
    max: u8,
) -> Result<Option<u8>, ControlError> {
    match arg(args, name) {
        None => Ok(None),
        Some(ArgValue::Int(n)) if (0..=i64::from(max)).contains(n) => Ok(u8::try_from(*n).ok()),
        Some(_) => Err(ControlError::params(id, format!("`{name}` is 0 to {max}"))),
    }
}

fn text_arg<'a>(args: &'a [(String, ArgValue)], name: &str) -> Option<&'a str> {
    match arg(args, name) {
        Some(ArgValue::Text(t)) => Some(t),
        _ => None,
    }
}

/// Whether a record command (`commands::RECORD_COMMANDS`) can run now:
/// Record Options while no pass records, and none of them but MIDI
/// injection during a drag.
pub fn record_command_enabled(id: &str, doc: &DocumentModel) -> bool {
    match id {
        "midi.inject" => true,
        "record.set" => !doc.record.is_recording() && !doc.gesture_open(),
        _ => !doc.gesture_open(),
    }
}

/// Runs a record command (`commands::RECORD_COMMANDS`) with the channel's
/// `args` (Swift `ControlCommands`' arms). None raises an alert: a failure
/// is the call's.
pub fn run_record(
    id: &str,
    args: &[(String, ArgValue)],
    doc: &mut DocumentModel,
) -> Result<Ran, ControlError> {
    check_names(id, args)?;
    match id {
        "record.set" => doc.record_set(id, args)?,
        "record.recover" => doc.recover_from_channel(id, args)?,
        "midi.inject" => doc.inject_midi(id, args)?,
        _ => {
            // A marker chore (plan 8c Ruling 4): one undo step, the
            // arguments as given; the core checks them.
            if doc.gesture_open() {
                return Err(unavailable(GESTURE_OPEN));
            }
            let result = doc.session.run_chore(id.to_owned(), control_args(args));
            doc.apply_result(result).map_err(|e| from_viz(&e))?;
        }
    }
    Ok(ran(doc))
}

fn is_punch(f: TransportField) -> bool {
    matches!(f, TransportField::PunchIn | TransportField::PunchOut)
}

impl DocumentModel {
    pub fn record(&self) -> &RecordModel {
        &self.record
    }

    /// The options (count-in, pre-roll, MIDI passes as takes).
    pub fn record_mut(&mut self) -> &mut RecordModel {
        &mut self.record
    }

    /// Settings › Recording's defaults for this document's options (the
    /// app calls it for a new document).
    pub fn apply_record_defaults(&mut self, d: RecordDefaults) {
        let state = self.record.state.take();
        self.record = RecordModel {
            state,
            ..RecordModel::new(d)
        };
    }

    /// What Record asks for now.
    pub fn record_spec(&self) -> RecordSpecRow {
        let t = &self.layout().transport;
        RecordSpecRow {
            replace: !t.overdub,
            punch: t.punch.then_some(TickRange {
                start: t.punch_in,
                end: t.punch_out,
            }),
            looped: self.looping(),
            count_in_bars: self.record.count_in_bars,
            pre_roll_bars: self.record.pre_roll_bars,
            passes_as_takes: self.record.midi_passes_as_takes,
        }
    }

    /// Step entry is on (Task 5's step entry model sets it).
    pub fn step_entry_on(&self) -> bool {
        self.step_entry_on
    }

    /// Turns the step-entry flag Record reads on or off.
    pub fn set_step_entry_on(&mut self, on: bool) {
        self.step_entry_on = on;
    }

    /// `transport.record`: records the shown sequence, or, while
    /// recording, stops (which finishes the pass). The record state is
    /// read quietly first: a pass can end by itself with no poll since.
    pub fn toggle_record(&mut self) -> Result<(), ControlError> {
        self.refresh_record(true);
        if self.record.is_recording() {
            self.session.stop().map_err(|e| from_viz(&e))?;
            self.after_record_call();
            return Ok(());
        }
        let Some(seq) = self.selected_seq() else {
            return Err(unavailable(NO_SEQUENCE));
        };
        if self.step_entry_on {
            return Err(refused(STEP_ENTRY_ON));
        }
        if self.gesture_open() {
            return Err(unavailable(GESTURE_OPEN));
        }
        // A punch time typed but not yet set refuses the pass: it must
        // never punch at the old range with a typed time on screen.
        if self.has_open_punch_draft() {
            return Err(refused(OPEN_DRAFT));
        }
        let spec = self.record_spec();
        self.session.record(seq, spec).map_err(|e| from_viz(&e))?;
        self.after_record_call();
        Ok(())
    }

    /// `record.set`: the transport's modes and the record options, all or
    /// nothing. The state is read quietly first (a pass can end by
    /// itself); every argument is checked before any is applied, and the
    /// loop, which can fail, goes first (its failure is the call's, not an
    /// alert).
    fn record_set(&mut self, id: &str, args: &[(String, ArgValue)]) -> Result<(), ControlError> {
        self.refresh_record(true);
        if self.record.is_recording() {
            return Err(unavailable(STOP_RECORDING_FIRST));
        }
        if self.gesture_open() {
            return Err(unavailable(GESTURE_OPEN));
        }
        let count_in = bars_arg(id, args, "count_in", MAX_COUNT_IN)?;
        let pre_roll = bars_arg(id, args, "pre_roll", MAX_PRE_ROLL)?;
        let looped = flag_arg(id, args, "loop")?;
        let replace = flag_arg(id, args, "replace")?;
        let punch = flag_arg(id, args, "punch")?;
        let takes = flag_arg(id, args, "passes_as_takes")?;
        if let Some(on) = looped
            && on != self.looping()
        {
            self.toggle_loop();
            if self.looping() != on {
                return Err(match self.alert.take() {
                    Some(a) => refused(&format!("{}: {}", a.title, a.message)),
                    None => refused("Loop Failed"),
                });
            }
        }
        if replace.is_some() || punch.is_some() {
            self.update_layout(|l| {
                if let Some(r) = replace {
                    l.transport.overdub = !r;
                }
                if let Some(p) = punch {
                    l.transport.punch = p;
                }
            });
        }
        if let Some(n) = count_in {
            self.record.set_count_in(n);
        }
        if let Some(n) = pre_roll {
            self.record.set_pre_roll(n);
        }
        if let Some(on) = takes {
            self.record.set_midi_passes_as_takes(on);
        }
        Ok(())
    }

    /// `record.recover {action}`: imports the recordings a crash left
    /// (one undo step) or moves them to the Trash. It never opens the
    /// Recovered Recordings alert and leaves the window's offer alone; a
    /// failure is the call's, "Recovery Failed: …".
    fn recover_from_channel(
        &mut self,
        id: &str,
        args: &[(String, ArgValue)],
    ) -> Result<(), ControlError> {
        let Some(action) = text_arg(args, "action") else {
            return Err(ControlError::params(id, "missing `action`"));
        };
        let found = self.session.recovered_recordings().unwrap_or_default();
        if found.is_empty() {
            return Err(unavailable(NO_RECOVERED));
        }
        if self.gesture_open() {
            return Err(unavailable(GESTURE_OPEN));
        }
        let result = self.session.recover_recordings(action == "import");
        self.apply_result(result)
            .map_err(|e| thrown(&e, crate::document::RECOVERY_FAILED))
    }

    /// `midi.inject {port, bytes}`: feeds hex bytes as if `port` sent them.
    /// The core's refusal of the payload or port is `invalid_params`.
    fn inject_midi(&mut self, id: &str, args: &[(String, ArgValue)]) -> Result<(), ControlError> {
        let (Some(port), Some(bytes)) = (
            text_arg(args, "port"),
            text_arg(args, "bytes").and_then(hex_bytes),
        ) else {
            return Err(ControlError::params(id, INJECT_HEX));
        };
        match self.session.inject_midi(port.to_owned(), bytes) {
            Ok(true) => Ok(()),
            Ok(false) => Err(unavailable(INJECT_NO_ENGINE)),
            Err(VizError::Model { message }) => Err(ControlError::params(id, message)),
            Err(e) => Err(thrown(&e, "MIDI Injection Failed")),
        }
    }

    /// `record.keep_pass` (`keep`) or `record.discard_pass`.
    pub fn press_pass(&mut self, keep: bool) -> Result<(), ControlError> {
        self.refresh_record(true);
        if !self.record.is_loop_recording() {
            return Err(unavailable(NOT_LOOP_RECORDING));
        }
        let r = if keep {
            self.session.keep_pass()
        } else {
            self.session.discard_pass()
        };
        r.map_err(|e| from_viz(&e))?;
        self.refresh_record(false);
        Ok(())
    }

    /// After Record or Stop: the state, the transport and (a pass landed)
    /// the take's snapshot.
    fn after_record_call(&mut self) {
        self.refresh_record(false);
        let _ = self.poll_transport();
    }

    /// Reads what the core is doing (the poll calls this while playing,
    /// recording or counting in). `quiet`: a failure keeps the last report
    /// and leaves the status line alone (a command's pre-read). Busy is
    /// always quiet. A pass that ended brings its take's snapshot in.
    pub fn refresh_record(&mut self, quiet: bool) {
        match self.session.record_state() {
            Ok(s) => {
                let ended = self.record.is_recording() && !s.recording;
                self.record.state = Some(s);
                if ended {
                    self.take_landed_snapshot();
                }
            }
            Err(VizError::Busy { .. }) => {}
            Err(e) => {
                if !quiet {
                    self.set_status(Some(error_message(&e)));
                }
            }
        }
    }

    /// A pass landed (one undo step "Record"): the new snapshot, if the
    /// core has one.
    fn take_landed_snapshot(&mut self) {
        if let Ok(s) = self.session.snapshot()
            && s.version != self.snapshot().version
        {
            self.apply(s);
            self.refresh_selection();
        }
    }

    /// The poll's part after a good transport read: the record state while
    /// playing, recording or counting in, and a held recovery offer shown
    /// again. True when what the window shows changed.
    pub fn poll_record(&mut self) -> bool {
        let t = self.last_transport();
        let mut changed = false;
        if t.playing || t.counting_in || self.record.is_recording() {
            let before = self.record.state.clone();
            self.refresh_record(false);
            changed = before != self.record.state;
        }
        self.reoffer_if_held() || changed
    }

    /// The transport polls fast: recording or counting in (metering joins
    /// with the track inputs).
    pub fn record_polls_fast(&self) -> bool {
        self.record.is_recording() || self.last_transport().counting_in
    }

    /// The count-in is playing (the timeline waits).
    pub fn counting_in(&self) -> bool {
        self.last_transport().counting_in
    }

    /// The bar's Record button is enabled: a sequence is shown, or a pass
    /// records. A text edit holding the keyboard does not disable it: a
    /// click is no keystroke, and with a punch draft open it refuses and
    /// says why.
    pub fn record_button_enabled(&self) -> bool {
        self.selected_seq().is_some() || self.record.is_recording()
    }

    /// The bar's Record button: as the R key, and a refusal shows in the
    /// status line.
    pub fn press_record(&mut self) {
        if let Err(e) = self.toggle_record() {
            self.set_status(Some(e.message));
        }
    }

    // ---- Modes ----

    /// Replace (overdub off).
    pub fn record_replaces(&self) -> bool {
        !self.layout().transport.overdub
    }

    pub fn set_record_replace(&mut self, on: bool) {
        self.update_layout(|l| l.transport.overdub = !on);
    }

    /// Punch on.
    pub fn record_punches(&self) -> bool {
        self.layout().transport.punch
    }

    pub fn set_record_punch(&mut self, on: bool) {
        self.update_layout(|l| l.transport.punch = on);
    }

    /// The bar's mode switch title (plan 5c-2 takes Ruling 7).
    pub fn record_mode_title(&self) -> &'static str {
        if self.record_replaces() {
            "Replace"
        } else {
            "Overdub"
        }
    }

    /// The mode switch's accessibility label.
    pub fn record_mode_label(&self) -> String {
        format!("Record mode: {}", self.record_mode_title())
    }

    /// The bar's mode switch: `transport.overdub`, so a refusal (a text
    /// field holding the keyboard) is the key's.
    pub fn toggle_record_mode(&mut self) -> Result<(), ControlError> {
        commands::run("transport.overdub", &[], self).map(|_| ())
    }

    /// The mode switch pressed: a refusal shows in the status line.
    pub fn press_record_mode(&mut self) {
        if let Err(e) = self.toggle_record_mode() {
            self.set_status(Some(e.message));
        }
    }

    // ---- The popover and the punch drafts ----

    /// Opens or closes the popover where it is; closing drops the punch
    /// drafts and ends the popover's text editing.
    pub fn set_record_popover(&mut self, shown: bool) {
        self.record.shows_popover = shown;
        if !shown {
            self.drop_punch_drafts();
            self.end_text_editing(POPOVER_OWNER);
        }
    }

    /// The chevron in `host`: closes the popover open there, else opens it
    /// there (closing it elsewhere first, which drops the drafts).
    pub fn toggle_record_popover(&mut self, host: PopoverHost) {
        if self.record.shows_popover_in(host) {
            self.set_record_popover(false);
            return;
        }
        if self.record.shows_popover {
            self.set_record_popover(false);
        }
        self.record.popover_host = host;
        self.set_record_popover(true);
    }

    /// A popover punch field's text: the typed draft, else the field's.
    pub fn punch_text(&self, f: TransportField) -> String {
        self.record
            .punch_drafts
            .get(&f)
            .cloned()
            .unwrap_or_else(|| field_text(self, f))
    }

    /// Keeps a draft for the punch fields only.
    pub fn set_punch_text(&mut self, f: TransportField, text: &str) {
        if is_punch(f) {
            self.record.punch_drafts.insert(f, text.to_owned());
        }
    }

    /// The popover's punch focus moved: moving to the other punch field
    /// commits the one it left; leaving the pair drops its draft. Text
    /// editing is held while a field has focus.
    pub fn punch_focus_changed(
        &mut self,
        old: Option<TransportField>,
        new: Option<TransportField>,
    ) {
        if let Some(old) = old {
            if new.is_some() {
                self.commit_punch(old);
            } else {
                self.drop_punch(old);
            }
        }
        if new.is_some() {
            self.begin_text_editing(POPOVER_OWNER);
        } else {
            self.end_text_editing(POPOVER_OWNER);
        }
    }

    /// Return in a punch field, or moving to the other one: commits its
    /// draft (a refusal says why in the status line and keeps the old
    /// value; the draft is dropped either way). Whether it was taken.
    pub fn commit_punch(&mut self, f: TransportField) -> bool {
        match self.record.punch_drafts.remove(&f) {
            Some(text) => commit_field(self, f, &text),
            None => true,
        }
    }

    /// Drops `f`'s draft: the field shows the value in use again.
    pub fn drop_punch(&mut self, f: TransportField) {
        self.record.punch_drafts.remove(&f);
    }

    /// Drops every popover punch draft.
    pub fn drop_punch_drafts(&mut self) {
        self.record.punch_drafts.clear();
    }

    /// A punch field shows typed text that is not the value in use, in the
    /// popover or in the bar's own Punch In or Out: Record waits for
    /// Return or Esc.
    pub fn has_open_punch_draft(&self) -> bool {
        let open = |f: TransportField, text: &str| text != field_text(self, f);
        self.record
            .bar_punch_draft
            .as_ref()
            .is_some_and(|(f, t)| open(*f, t))
            || self.record.punch_drafts.iter().any(|(f, t)| open(*f, t))
    }

    /// The bar's Punch In or Out is being typed in (the field reports each
    /// change); other fields are ignored.
    pub fn bar_punch_typed(&mut self, f: TransportField, text: &str) {
        if is_punch(f) {
            self.record.bar_punch_draft = Some((f, text.to_owned()));
        }
    }

    /// The bar's punch field edit ended (committed or cancelled).
    pub fn bar_punch_ended(&mut self) {
        self.record.bar_punch_draft = None;
    }

    // ---- Readouts ----

    /// The channels the shown sequence's armed audio tracks record.
    pub fn armed_channels(&self) -> u32 {
        self.lanes()
            .iter()
            .filter(|l| l.arm)
            .map(|l| match l.input {
                Some(LaneInputRow::Audio { stereo, .. }) => 1 + u32::from(stereo),
                _ => 0,
            })
            .sum()
    }

    /// Seconds of recording the disk holds before recording stops, from
    /// the core's last free-space reading (it reads the project's volume
    /// when idle too) and the armed channels; `None` with no armed audio
    /// track or no reading.
    pub fn record_remaining_seconds(&self) -> Option<f64> {
        let format = &self.project_info()?.format;
        let channels = self.armed_channels();
        if channels == 0 {
            return None;
        }
        let bytes = match format.depth {
            BitDepthRow::Int16 => 2.0,
            BitDepthRow::Int24 => 3.0,
            BitDepthRow::Float32 => 4.0,
        };
        let per_second = f64::from(format.sample_rate) * f64::from(channels) * bytes;
        let free = self.record.state.as_ref()?.free_bytes?;
        (per_second > 0.0).then(|| free.saturating_sub(STOP_BYTES) as f64 / per_second)
    }

    /// "1 h 5 min left"; `None` with no armed audio track or no reading.
    pub fn record_remaining_text(&self) -> Option<String> {
        self.record_remaining_seconds().map(remaining)
    }

    /// The loop record counters; `None` unless loop recording.
    pub fn record_pass_text(&self) -> Option<String> {
        if !self.record.is_loop_recording() {
            return None;
        }
        let s = self.record.state.as_ref()?;
        let mut t = format!("Pass {}", s.pass);
        if s.kept > 0 {
            t += &format!(" \u{b7} {} kept", s.kept);
        }
        if s.discarded > 0 {
            t += &format!(" \u{b7} {} discarded", s.discarded);
        }
        Some(t)
    }

    /// Spec §6.4's note, shown in the popover with overdub on and an audio
    /// track armed.
    pub fn record_overdub_note(&self) -> Option<&'static str> {
        let audio = self
            .lanes()
            .iter()
            .any(|l| l.arm && matches!(l.input, Some(LaneInputRow::Audio { .. })));
        (!self.record_replaces() && audio).then_some(OVERDUB_NOTE)
    }
}
