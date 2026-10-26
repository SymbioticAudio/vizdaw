//! Editor commands (spec §3.2): one variant per core command of the
//! groups 4a needs (Event, Reference, Lane, Sequence, Audio) and of the
//! Drum group (4e, `crate::drum`); field types
//! are FFI records. The view filter travels beside the command
//! (Plan Ruling 6).

use crate::error::VizError;
use crate::events::{
    EventKindRow, EventRow, FadeRow, GroupChannel, MarkerRow, MeterEntryRow, PerNoteKindRow,
    PerNoteValueRow, TempoPointRow, bad, fade_of, kind_of, loops, per_note_kind, spec,
};
use crate::snapshot::KeySlotRow;
use core_model::command::{
    AudioCmd, Command, EventCmd, FadeEdge, LaneCmd, ReferenceCmd, SequenceCmd,
};
use core_model::ids::{DestId, EventId, LaneId, MediaId, OutputPathId, SeqId, TakeId};
use core_model::lane::QuantizeSpec;
use core_model::meter::{KeySig, Meter, MeterEntry, MeterMap};
use core_model::selection::ViewFilter;
use core_model::sequence::{GeneratorParams, KeySlot, Marker, SeqKind, SeqLength};
use core_model::tempo::{Bpm, TempoMap, TempoPoint};
use core_model::time::Tick;
use std::collections::HashMap;

/// A lane of a sequence.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, uniffi::Record)]
pub struct LaneRef {
    /// Sequence ID.
    pub seq: u64,
    /// Lane ID.
    pub lane: u64,
}

/// Non-destructive play quantize.
#[derive(Debug, Clone, PartialEq, Eq, Hash, uniffi::Record)]
pub struct QuantizeRow {
    /// Grid in ticks (> 0).
    pub grid: i64,
    /// Strength 0–100.
    pub strength: u8,
    /// Groove offsets per grid line (empty: a plain grid; Plan 4e
    /// Ruling 10).
    #[uniffi(default)]
    pub groove: Vec<i64>,
}

/// The core quantize of a row.
pub fn quantize_of(q: &QuantizeRow) -> QuantizeSpec {
    QuantizeSpec {
        grid: Tick(q.grid),
        strength: q.strength,
        groove: q.groove.iter().copied().map(Tick).collect(),
    }
}

/// A core quantize as a row.
pub fn quantize_row(q: &QuantizeSpec) -> QuantizeRow {
    QuantizeRow {
        grid: q.grid.0,
        strength: q.strength,
        groove: q.groove.iter().map(|t| t.0).collect(),
    }
}

/// Generator parameters of a generated sequence.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct GeneratorRow {
    /// Generator name.
    pub generator: String,
    /// Parameters.
    pub params: HashMap<String, String>,
}

/// What a view hides from commands that act on the selection.
#[derive(Debug, Clone, PartialEq, Eq, Hash, uniffi::Record)]
pub struct ViewFilterRow {
    /// Hidden event kinds.
    pub hidden_kinds: Vec<EventKindRow>,
    /// Hidden lanes.
    pub hidden_lanes: Vec<u64>,
}

/// Every edit an editor sends (spec §3.2).
#[derive(Debug, Clone, PartialEq, uniffi::Enum)]
pub enum EditCommand {
    /// Event edits.
    Event {
        /// The edit.
        edit: EventEdit,
    },
    /// Reference edits.
    Reference {
        /// The edit.
        edit: ReferenceEdit,
    },
    /// Lane and take edits.
    Lane {
        /// The edit.
        edit: LaneEdit,
    },
    /// Sequence edits.
    Sequence {
        /// The edit.
        edit: SequenceEdit,
    },
    /// Audio-region edits.
    Audio {
        /// The edit.
        edit: AudioEdit,
    },
    /// Select by rule, then optionally modify the selection or copies (4c).
    SelectAndModify {
        /// Rule (None: the selection as it is).
        rule: Option<crate::modify::RuleRow>,
        /// How the matches combine with the selection.
        mode: crate::modify::SelectModeRow,
        /// Modification (None: select only, no undo step).
        modify: Option<crate::modify::ModifyRow>,
        /// Modify copies beside the originals.
        to_copy: bool,
    },
    /// Repeat the last modification on the current selection (4c, Plan
    /// Ruling 8; `Session.edit` only).
    ModifyAgain,
    /// Document settings: modify templates (4c).
    Settings {
        /// The edit.
        edit: crate::modify::SettingsEdit,
    },
    /// Drum-lane edits (4e).
    Drum {
        /// The edit.
        edit: crate::drum::DrumEdit,
    },
}

/// Mirrors `core_model::command::EventCmd`.
#[allow(clippy::large_enum_variant)] // boxing would change the generated Swift API
#[derive(Debug, Clone, PartialEq, uniffi::Enum)]
pub enum EventEdit {
    /// Insert rows (IDs ignored) into a lane's active take.
    Insert {
        /// Sequence.
        seq: u64,
        /// Lane.
        lane: u64,
        /// The events.
        events: Vec<EventRow>,
    },
    /// Delete events.
    Delete {
        /// The events.
        events: Vec<u64>,
    },
    /// Move in time and optionally to another lane.
    Move {
        /// The events.
        events: Vec<u64>,
        /// Time offset.
        delta: i64,
        /// Destination lane.
        to_lane: Option<LaneRef>,
    },
    /// Set pitch.
    SetPitch {
        /// The events.
        events: Vec<u64>,
        /// Pitch 0–127.
        pitch: u8,
    },
    /// Set velocity.
    SetVelocity {
        /// The events.
        events: Vec<u64>,
        /// 16-bit velocity.
        velocity: u16,
    },
    /// Set note duration.
    SetDuration {
        /// The events.
        events: Vec<u64>,
        /// Ticks (>= 0).
        duration: i64,
    },
    /// Set the 32-bit value.
    SetValue {
        /// The events.
        events: Vec<u64>,
        /// Value.
        value: u32,
    },
    /// Set or clear the destination override.
    SetDest {
        /// The events.
        events: Vec<u64>,
        /// Destination.
        dest: Option<u64>,
    },
    /// Set or clear the group/channel override.
    SetGroupChannel {
        /// The events.
        events: Vec<u64>,
        /// Group and channel.
        group_channel: Option<GroupChannel>,
    },
    /// Mute or unmute.
    SetMute {
        /// The events.
        events: Vec<u64>,
        /// Mute flag.
        mute: bool,
    },
    /// Nudge in time and pitch.
    Nudge {
        /// The events.
        events: Vec<u64>,
        /// Time offset.
        ticks: i64,
        /// Pitch offset.
        semitones: i8,
    },
    /// Copy events in their lanes, moved and transposed (4b).
    Duplicate {
        /// The events.
        events: Vec<u64>,
        /// Time offset of the copies.
        ticks: i64,
        /// Pitch offset of the copies.
        semitones: i8,
    },
    /// Lengthen or shorten notes; each keeps at least 1 tick (4b).
    Resize {
        /// The events.
        events: Vec<u64>,
        /// Duration change.
        delta: i64,
    },
    /// Move pitches by scale degrees (4b).
    TransposeInScale {
        /// The events.
        events: Vec<u64>,
        /// Degrees (negative: down).
        steps: i8,
        /// Root pitch class 0-11.
        root: u8,
        /// 12-bit scale mask.
        mask: u16,
    },
    /// Replace one per-note kind's values in `from..=to` (4b).
    SetPerNote {
        /// The notes.
        events: Vec<u64>,
        /// Which per-note controller.
        kind: PerNoteKindRow,
        /// First tick replaced.
        from: i64,
        /// Last tick replaced.
        to: i64,
        /// The new values (absolute ticks).
        points: Vec<PerNoteValueRow>,
    },
    /// Replace an event with a row of the same kind (a list edit, 4c).
    Replace {
        /// The event.
        event: u64,
        /// What it becomes (its ID is ignored).
        row: EventRow,
    },
}

/// Mirrors `core_model::command::ReferenceCmd` (`loop_count` 0 = Fill).
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Enum)]
pub enum ReferenceEdit {
    /// Insert a reference.
    Insert {
        /// Owning sequence.
        seq: u64,
        /// Owning lane.
        lane: u64,
        /// Position.
        tick: i64,
        /// Referenced sequence.
        target: u64,
        /// Per-use length.
        length: Option<i64>,
        /// Repeats (0 = Fill).
        loop_count: u32,
        /// Transpose.
        transpose: i8,
    },
    /// Move the selection into new sequence(s) and reference them.
    FromSelection {
        /// The sequence the selection is in.
        seq: u64,
        /// One reference per lane.
        per_lane: bool,
        /// Widen the range to barlines.
        round_to_bars: bool,
        /// Repeats of the new reference(s) (0 = Fill).
        loop_count: u32,
    },
    /// Point a reference at another sequence.
    RePoint {
        /// The reference event.
        event: u64,
        /// New target.
        target: u64,
    },
    /// Clone the referenced sequence for this use.
    MakePrivateCopy {
        /// The reference event.
        event: u64,
    },
    /// Replace the reference with its events.
    Flatten {
        /// The reference event.
        event: u64,
    },
    /// Set per-use length, repeats and transpose.
    SetParams {
        /// The reference event.
        event: u64,
        /// Per-use length.
        length: Option<i64>,
        /// Repeats (0 = Fill).
        loop_count: u32,
        /// Transpose.
        transpose: i8,
    },
}

/// Mirrors `core_model::command::LaneCmd`.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Enum)]
pub enum LaneEdit {
    /// Add a lane at `index` or at the end.
    Add {
        /// Sequence.
        seq: u64,
        /// Name.
        name: String,
        /// Position.
        index: Option<u32>,
    },
    /// Remove a lane.
    Remove {
        /// Sequence.
        seq: u64,
        /// Lane.
        lane: u64,
    },
    /// Move a lane to position `to`.
    Reorder {
        /// Sequence.
        seq: u64,
        /// Lane.
        lane: u64,
        /// New index.
        to: u32,
    },
    /// Rename a lane.
    Rename {
        /// Sequence.
        seq: u64,
        /// Lane.
        lane: u64,
        /// New name.
        name: String,
    },
    /// Set the MIDI destination.
    SetDest {
        /// Sequence.
        seq: u64,
        /// Lane.
        lane: u64,
        /// Destination.
        dest: Option<u64>,
    },
    /// Set the audio output path.
    SetOutput {
        /// Sequence.
        seq: u64,
        /// Lane.
        lane: u64,
        /// Output.
        output: Option<u64>,
    },
    /// Set mute/solo/arm; `None` leaves a flag unchanged.
    SetFlags {
        /// Sequence.
        seq: u64,
        /// Lane.
        lane: u64,
        /// Mute.
        mute: Option<bool>,
        /// Solo.
        solo: Option<bool>,
        /// Record arm.
        arm: Option<bool>,
    },
    /// Set the lane's own length, loop flag and lock.
    SetLength {
        /// Sequence.
        seq: u64,
        /// Lane.
        lane: u64,
        /// Own length.
        length: Option<i64>,
        /// Loop.
        looped: bool,
        /// Lock against recording.
        locked: bool,
    },
    /// Set play quantize.
    SetPlayQuantize {
        /// Sequence.
        seq: u64,
        /// Lane.
        lane: u64,
        /// Quantize.
        quantize: Option<QuantizeRow>,
    },
    /// Set play shift.
    SetPlayShift {
        /// Sequence.
        seq: u64,
        /// Lane.
        lane: u64,
        /// Shift in ticks.
        shift: i64,
    },
    /// Add an empty take and make it active.
    AddTake {
        /// Sequence.
        seq: u64,
        /// Lane.
        lane: u64,
        /// Take name.
        name: String,
    },
    /// Choose the playing take.
    SetActiveTake {
        /// Sequence.
        seq: u64,
        /// Lane.
        lane: u64,
        /// Take.
        take: u64,
    },
    /// Set what the lane records (sub-project 5 spec §4.5).
    SetInput {
        /// Sequence.
        seq: u64,
        /// Lane.
        lane: u64,
        /// The input, or none.
        input: Option<LaneInputRow>,
    },
    /// Set when the lane's input is heard (spec §6.6).
    SetMonitor {
        /// Sequence.
        seq: u64,
        /// Lane.
        lane: u64,
        /// The mode.
        monitor: MonitorModeRow,
    },
}

/// What a lane records (mirrors `core_model::lane::LaneInput`).
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Enum)]
pub enum LaneInputRow {
    /// Device input channel `first` (0-based), and the next one too when
    /// `stereo`.
    Audio {
        /// The first (left) channel.
        first: u16,
        /// A pair.
        stereo: bool,
    },
    /// MIDI.
    Midi {
        /// The source's name; none: every source.
        port: Option<String>,
        /// The channel 0–15; none: every channel.
        channel: Option<u8>,
    },
}

impl From<&LaneInputRow> for core_model::lane::LaneInput {
    fn from(r: &LaneInputRow) -> Self {
        use core_model::lane::{InputChannels, LaneInput};
        match r {
            LaneInputRow::Audio { first, stereo } => LaneInput::Audio {
                channels: if *stereo {
                    InputChannels::Stereo(*first)
                } else {
                    InputChannels::Mono(*first)
                },
                // Task 8 of plan 5c-2 carries it over the FFI.
                return_of: None,
            },
            LaneInputRow::Midi { port, channel } => LaneInput::Midi {
                port: port.clone(),
                channel: *channel,
            },
        }
    }
}

impl From<&core_model::lane::LaneInput> for LaneInputRow {
    fn from(i: &core_model::lane::LaneInput) -> Self {
        use core_model::lane::{InputChannels, LaneInput};
        match i {
            LaneInput::Audio { channels, .. } => LaneInputRow::Audio {
                first: channels.first(),
                stereo: matches!(channels, InputChannels::Stereo(_)),
            },
            LaneInput::Midi { port, channel } => LaneInputRow::Midi {
                port: port.clone(),
                channel: *channel,
            },
        }
    }
}

/// When a lane's input is heard (mirrors `core_model::lane::MonitorMode`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum MonitorModeRow {
    /// Armed and stopped, or recording inside the record range.
    Auto,
    /// Always.
    Input,
    /// Never.
    Off,
}

impl From<MonitorModeRow> for core_model::lane::MonitorMode {
    fn from(m: MonitorModeRow) -> Self {
        use core_model::lane::MonitorMode;
        match m {
            MonitorModeRow::Auto => MonitorMode::Auto,
            MonitorModeRow::Input => MonitorMode::Input,
            MonitorModeRow::Off => MonitorMode::Off,
        }
    }
}

impl From<core_model::lane::MonitorMode> for MonitorModeRow {
    fn from(m: core_model::lane::MonitorMode) -> Self {
        use core_model::lane::MonitorMode;
        match m {
            MonitorMode::Auto => MonitorModeRow::Auto,
            MonitorMode::Input => MonitorModeRow::Input,
            MonitorMode::Off => MonitorModeRow::Off,
        }
    }
}

/// Mirrors `core_model::command::SequenceCmd`.
#[derive(Debug, Clone, PartialEq, uniffi::Enum)]
pub enum SequenceEdit {
    /// A new sequence with one empty lane.
    New {
        /// Name.
        name: String,
    },
    /// A deep copy.
    Duplicate {
        /// Source.
        seq: u64,
    },
    /// Rename.
    Rename {
        /// Target.
        seq: u64,
        /// New name.
        name: String,
    },
    /// Delete (a used sequence needs `delete_references`).
    Delete {
        /// Target.
        seq: u64,
        /// Also remove every reference to it.
        delete_references: bool,
    },
    /// Normal (`None`) or generated.
    SetKind {
        /// Target.
        seq: u64,
        /// Generator parameters.
        generator: Option<GeneratorRow>,
    },
    /// Assign or clear the keyboard slot.
    SetKeyboardSlot {
        /// Target.
        seq: u64,
        /// Slot.
        slot: Option<KeySlotRow>,
    },
    /// Replace the tempo map.
    SetTempo {
        /// Target.
        seq: u64,
        /// Points; the first at tick 0.
        tempo: Vec<TempoPointRow>,
    },
    /// Replace the meter map.
    SetMeter {
        /// Target.
        seq: u64,
        /// Entries; the first at tick 0, all on barlines.
        meter: Vec<MeterEntryRow>,
    },
    /// Replace the markers.
    SetMarkers {
        /// Target.
        seq: u64,
        /// Markers.
        markers: Vec<MarkerRow>,
    },
    /// Set the pickup length.
    SetStartOffset {
        /// Target.
        seq: u64,
        /// Offset (>= 0).
        offset: i64,
    },
    /// Lock the length (`Some`) or derive it (`None`).
    SetLength {
        /// Target.
        seq: u64,
        /// Locked length.
        locked: Option<i64>,
    },
    /// Set the comment.
    SetComment {
        /// Target.
        seq: u64,
        /// Comment.
        comment: String,
    },
}

/// Mirrors `core_model::command::AudioCmd`.
#[derive(Debug, Clone, PartialEq, uniffi::Enum)]
pub enum AudioEdit {
    /// Insert a region.
    Insert {
        /// Sequence.
        seq: u64,
        /// Lane.
        lane: u64,
        /// Start tick.
        tick: i64,
        /// Media item.
        media: u64,
        /// Start within the media (frames).
        source_offset: u64,
        /// Length (frames).
        length: u64,
        /// Gain in dB.
        gain_db: f32,
        /// Fade in.
        fade_in: FadeRow,
        /// Fade out.
        fade_out: FadeRow,
    },
    /// Trim frames from the front and back.
    Trim {
        /// The region.
        event: u64,
        /// Frames removed from the start.
        front_frames: i64,
        /// Frames removed from the end.
        back_frames: i64,
    },
    /// Split in two at `at`.
    Split {
        /// The region.
        event: u64,
        /// Split position.
        at: i64,
    },
    /// Set gain.
    SetGain {
        /// The region.
        event: u64,
        /// Gain in dB.
        gain_db: f32,
    },
    /// Set fades.
    SetFades {
        /// The region.
        event: u64,
        /// Fade in.
        fade_in: FadeRow,
        /// Fade out.
        fade_out: FadeRow,
    },
    /// Fade In to Cursor (`fade_in`) or Fade Out from Cursor, on every
    /// region `at` lies inside; other events are ignored (plan 5b
    /// Ruling 6).
    FadeTo {
        /// The events.
        events: Vec<u64>,
        /// The cursor tick.
        at: i64,
        /// The fade in (else the fade out).
        fade_in: bool,
    },
    /// Remove Fades from every region; other events are ignored.
    RemoveFades {
        /// The events.
        events: Vec<u64>,
    },
}

fn ids(v: &[u64]) -> Vec<EventId> {
    v.iter().copied().map(EventId).collect()
}

fn lane_ref(l: &LaneRef) -> (SeqId, LaneId) {
    (SeqId(l.seq), LaneId(l.lane))
}

fn index(i: u32) -> usize {
    usize::try_from(i).unwrap_or(usize::MAX)
}

/// The core command for an editor command.
pub fn to_command(c: &EditCommand) -> Result<Command, VizError> {
    Ok(match c {
        EditCommand::Event { edit } => Command::Event(event_cmd(edit)?),
        EditCommand::Reference { edit } => Command::Reference(reference_cmd(edit)),
        EditCommand::Lane { edit } => Command::Lane(lane_cmd(edit)),
        EditCommand::Sequence { edit } => Command::Sequence(sequence_cmd(edit)?),
        EditCommand::Audio { edit } => Command::Audio(audio_cmd(edit)),
        EditCommand::SelectAndModify {
            rule,
            mode,
            modify,
            to_copy,
        } => Command::SelectAndModify {
            rule: rule.as_ref().map(crate::modify::rule).transpose()?,
            mode: crate::modify::mode_of(*mode),
            modify: modify.as_ref().map(crate::modify::modify).transpose()?,
            to_copy: *to_copy,
        },
        EditCommand::ModifyAgain => {
            return Err(bad(
                "Modify Again repeats a modification; it is not a command",
            ));
        }
        EditCommand::Settings { edit } => Command::Settings(crate::modify::settings_cmd(edit)?),
        EditCommand::Drum { edit } => Command::Drum(crate::drum::drum_cmd(edit)),
    })
}

/// The core view filter (`None`: hide nothing).
pub fn view_filter(f: Option<&ViewFilterRow>) -> ViewFilter {
    f.map_or_else(ViewFilter::default, |f| ViewFilter {
        hidden_kinds: f.hidden_kinds.iter().map(|k| kind_of(*k)).collect(),
        hidden_lanes: f.hidden_lanes.iter().copied().map(LaneId).collect(),
    })
}

fn event_cmd(e: &EventEdit) -> Result<EventCmd, VizError> {
    Ok(match e {
        EventEdit::Insert { seq, lane, events } => EventCmd::Insert {
            seq: SeqId(*seq),
            lane: LaneId(*lane),
            events: events.iter().map(spec).collect::<Result<_, _>>()?,
        },
        EventEdit::Delete { events } => EventCmd::Delete {
            events: ids(events),
        },
        EventEdit::Move {
            events,
            delta,
            to_lane,
        } => EventCmd::Move {
            events: ids(events),
            delta: Tick(*delta),
            to_lane: to_lane.as_ref().map(lane_ref),
        },
        EventEdit::SetPitch { events, pitch } => EventCmd::SetPitch {
            events: ids(events),
            pitch: *pitch,
        },
        EventEdit::SetVelocity { events, velocity } => EventCmd::SetVelocity {
            events: ids(events),
            velocity: *velocity,
        },
        EventEdit::SetDuration { events, duration } => EventCmd::SetDuration {
            events: ids(events),
            duration: Tick(*duration),
        },
        EventEdit::SetValue { events, value } => EventCmd::SetValue {
            events: ids(events),
            value: *value,
        },
        EventEdit::SetDest { events, dest } => EventCmd::SetDest {
            events: ids(events),
            dest: dest.map(DestId),
        },
        EventEdit::SetGroupChannel {
            events,
            group_channel,
        } => EventCmd::SetGroupChannel {
            events: ids(events),
            group_channel: group_channel.map(|g| (g.group, g.channel)),
        },
        EventEdit::SetMute { events, mute } => EventCmd::SetMute {
            events: ids(events),
            mute: *mute,
        },
        EventEdit::Nudge {
            events,
            ticks,
            semitones,
        } => EventCmd::Nudge {
            events: ids(events),
            ticks: Tick(*ticks),
            semitones: *semitones,
        },
        EventEdit::Duplicate {
            events,
            ticks,
            semitones,
        } => EventCmd::Duplicate {
            events: ids(events),
            ticks: Tick(*ticks),
            semitones: *semitones,
        },
        EventEdit::Resize { events, delta } => EventCmd::Resize {
            events: ids(events),
            delta: Tick(*delta),
        },
        EventEdit::TransposeInScale {
            events,
            steps,
            root,
            mask,
        } => EventCmd::TransposeInScale {
            events: ids(events),
            steps: *steps,
            root: *root,
            mask: *mask,
        },
        EventEdit::SetPerNote {
            events,
            kind,
            from,
            to,
            points,
        } => EventCmd::SetPerNote {
            events: ids(events),
            kind: per_note_kind(*kind),
            from: Tick(*from),
            to: Tick(*to),
            points: points.iter().map(|p| (Tick(p.tick), p.value)).collect(),
        },
        EventEdit::Replace { event, row } => EventCmd::Replace {
            event: EventId(*event),
            spec: spec(row)?,
        },
    })
}

fn reference_cmd(e: &ReferenceEdit) -> ReferenceCmd {
    match e {
        ReferenceEdit::Insert {
            seq,
            lane,
            tick,
            target,
            length,
            loop_count,
            transpose,
        } => ReferenceCmd::Insert {
            seq: SeqId(*seq),
            lane: LaneId(*lane),
            tick: Tick(*tick),
            target: SeqId(*target),
            length: length.map(Tick),
            loop_count: loops(*loop_count),
            transpose: *transpose,
        },
        ReferenceEdit::FromSelection {
            seq,
            per_lane,
            round_to_bars,
            loop_count,
        } => ReferenceCmd::FromSelection {
            seq: SeqId(*seq),
            per_lane: *per_lane,
            round_to_bars: *round_to_bars,
            loop_count: loops(*loop_count),
        },
        ReferenceEdit::RePoint { event, target } => ReferenceCmd::RePoint {
            event: EventId(*event),
            target: SeqId(*target),
        },
        ReferenceEdit::MakePrivateCopy { event } => ReferenceCmd::MakePrivateCopy {
            event: EventId(*event),
        },
        ReferenceEdit::Flatten { event } => ReferenceCmd::Flatten {
            event: EventId(*event),
        },
        ReferenceEdit::SetParams {
            event,
            length,
            loop_count,
            transpose,
        } => ReferenceCmd::SetParams {
            event: EventId(*event),
            length: length.map(Tick),
            loop_count: loops(*loop_count),
            transpose: *transpose,
        },
    }
}

fn lane_cmd(e: &LaneEdit) -> LaneCmd {
    match e {
        LaneEdit::Add {
            seq,
            name,
            index: i,
        } => LaneCmd::Add {
            seq: SeqId(*seq),
            name: name.clone(),
            index: i.map(index),
        },
        LaneEdit::Remove { seq, lane } => LaneCmd::Remove {
            seq: SeqId(*seq),
            lane: LaneId(*lane),
        },
        LaneEdit::Reorder { seq, lane, to } => LaneCmd::Reorder {
            seq: SeqId(*seq),
            lane: LaneId(*lane),
            to: index(*to),
        },
        LaneEdit::Rename { seq, lane, name } => LaneCmd::Rename {
            seq: SeqId(*seq),
            lane: LaneId(*lane),
            name: name.clone(),
        },
        LaneEdit::SetDest { seq, lane, dest } => LaneCmd::SetDest {
            seq: SeqId(*seq),
            lane: LaneId(*lane),
            dest: dest.map(DestId),
        },
        LaneEdit::SetOutput { seq, lane, output } => LaneCmd::SetOutput {
            seq: SeqId(*seq),
            lane: LaneId(*lane),
            output: output.map(OutputPathId),
        },
        LaneEdit::SetFlags {
            seq,
            lane,
            mute,
            solo,
            arm,
        } => LaneCmd::SetFlags {
            seq: SeqId(*seq),
            lane: LaneId(*lane),
            mute: *mute,
            solo: *solo,
            arm: *arm,
        },
        LaneEdit::SetLength {
            seq,
            lane,
            length,
            looped,
            locked,
        } => LaneCmd::SetLength {
            seq: SeqId(*seq),
            lane: LaneId(*lane),
            length: length.map(Tick),
            looped: *looped,
            locked: *locked,
        },
        LaneEdit::SetPlayQuantize {
            seq,
            lane,
            quantize,
        } => LaneCmd::SetPlayQuantize {
            seq: SeqId(*seq),
            lane: LaneId(*lane),
            quantize: quantize.as_ref().map(quantize_of),
        },
        LaneEdit::SetPlayShift { seq, lane, shift } => LaneCmd::SetPlayShift {
            seq: SeqId(*seq),
            lane: LaneId(*lane),
            shift: Tick(*shift),
        },
        LaneEdit::AddTake { seq, lane, name } => LaneCmd::AddTake {
            seq: SeqId(*seq),
            lane: LaneId(*lane),
            name: name.clone(),
        },
        LaneEdit::SetActiveTake { seq, lane, take } => LaneCmd::SetActiveTake {
            seq: SeqId(*seq),
            lane: LaneId(*lane),
            take: TakeId(*take),
        },
        LaneEdit::SetInput { seq, lane, input } => LaneCmd::SetInput {
            seq: SeqId(*seq),
            lane: LaneId(*lane),
            input: input.as_ref().map(Into::into),
        },
        LaneEdit::SetMonitor { seq, lane, monitor } => LaneCmd::SetMonitor {
            seq: SeqId(*seq),
            lane: LaneId(*lane),
            monitor: (*monitor).into(),
        },
    }
}

fn bpm(v: f64) -> Result<Bpm, VizError> {
    if !v.is_finite() || v <= 0.0 {
        return Err(bad(&format!("tempo {v} BPM is out of range")));
    }
    let milli = (v * 1000.0).round();
    if milli > f64::from(u32::MAX) {
        return Err(bad(&format!("tempo {v} BPM is out of range")));
    }
    Ok(Bpm(milli as u32))
}

fn sequence_cmd(e: &SequenceEdit) -> Result<SequenceCmd, VizError> {
    Ok(match e {
        SequenceEdit::New { name } => SequenceCmd::New { name: name.clone() },
        SequenceEdit::Duplicate { seq } => SequenceCmd::Duplicate { seq: SeqId(*seq) },
        SequenceEdit::Rename { seq, name } => SequenceCmd::Rename {
            seq: SeqId(*seq),
            name: name.clone(),
        },
        SequenceEdit::Delete {
            seq,
            delete_references,
        } => SequenceCmd::Delete {
            seq: SeqId(*seq),
            delete_references: *delete_references,
        },
        SequenceEdit::SetKind { seq, generator } => SequenceCmd::SetKind {
            seq: SeqId(*seq),
            kind: match generator {
                None => SeqKind::Normal,
                Some(g) => SeqKind::Generated {
                    generator: GeneratorParams {
                        generator: g.generator.clone(),
                        params: g
                            .params
                            .iter()
                            .map(|(k, v)| (k.clone(), v.clone()))
                            .collect(),
                    },
                },
            },
        },
        SequenceEdit::SetKeyboardSlot { seq, slot } => SequenceCmd::SetKeyboardSlot {
            seq: SeqId(*seq),
            slot: slot.as_ref().map(|k| KeySlot {
                key: k.key.clone(),
                modifiers: k.modifiers,
            }),
        },
        SequenceEdit::SetTempo { seq, tempo } => SequenceCmd::SetTempo {
            seq: SeqId(*seq),
            tempo: TempoMap::new(
                tempo
                    .iter()
                    .map(|p| {
                        Ok(TempoPoint {
                            tick: Tick(p.tick),
                            bpm: bpm(p.bpm)?,
                            ramp: p.ramp,
                        })
                    })
                    .collect::<Result<Vec<_>, VizError>>()?,
            )?,
        },
        SequenceEdit::SetMeter { seq, meter } => SequenceCmd::SetMeter {
            seq: SeqId(*seq),
            meter: MeterMap::new(
                meter
                    .iter()
                    .map(|m| MeterEntry {
                        tick: Tick(m.tick),
                        meter: Meter {
                            num: m.num,
                            den: m.den,
                        },
                        key: KeySig {
                            sharps: m.sharps,
                            minor: m.minor,
                        },
                    })
                    .collect(),
            )?,
        },
        SequenceEdit::SetMarkers { seq, markers } => SequenceCmd::SetMarkers {
            seq: SeqId(*seq),
            markers: markers
                .iter()
                .map(|m| Marker {
                    tick: Tick(m.tick),
                    name: m.name.clone(),
                })
                .collect(),
        },
        SequenceEdit::SetStartOffset { seq, offset } => SequenceCmd::SetStartOffset {
            seq: SeqId(*seq),
            offset: Tick(*offset),
        },
        SequenceEdit::SetLength { seq, locked } => SequenceCmd::SetLength {
            seq: SeqId(*seq),
            length: locked.map_or(SeqLength::Derived, |t| SeqLength::Locked(Tick(t))),
        },
        SequenceEdit::SetComment { seq, comment } => SequenceCmd::SetComment {
            seq: SeqId(*seq),
            comment: comment.clone(),
        },
    })
}

fn audio_cmd(e: &AudioEdit) -> AudioCmd {
    match e {
        AudioEdit::Insert {
            seq,
            lane,
            tick,
            media,
            source_offset,
            length,
            gain_db,
            fade_in,
            fade_out,
        } => AudioCmd::Insert {
            seq: SeqId(*seq),
            lane: LaneId(*lane),
            tick: Tick(*tick),
            media: MediaId(*media),
            source_offset: *source_offset,
            length: *length,
            gain_db: *gain_db,
            fade_in: fade_of(fade_in),
            fade_out: fade_of(fade_out),
        },
        AudioEdit::Trim {
            event,
            front_frames,
            back_frames,
        } => AudioCmd::Trim {
            event: EventId(*event),
            front_frames: *front_frames,
            back_frames: *back_frames,
        },
        AudioEdit::Split { event, at } => AudioCmd::Split {
            event: EventId(*event),
            at: Tick(*at),
        },
        AudioEdit::SetGain { event, gain_db } => AudioCmd::SetGain {
            event: EventId(*event),
            gain_db: *gain_db,
        },
        AudioEdit::SetFades {
            event,
            fade_in,
            fade_out,
        } => AudioCmd::SetFades {
            event: EventId(*event),
            fade_in: fade_of(fade_in),
            fade_out: fade_of(fade_out),
        },
        AudioEdit::FadeTo {
            events,
            at,
            fade_in,
        } => AudioCmd::FadeTo {
            events: ids(events),
            at: Tick(*at),
            edge: if *fade_in {
                FadeEdge::In
            } else {
                FadeEdge::Out
            },
        },
        AudioEdit::RemoveFades { events } => AudioCmd::RemoveFades {
            events: ids(events),
        },
    }
}
