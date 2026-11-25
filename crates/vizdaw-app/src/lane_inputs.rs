//! A track's input controls (Swift `LaneInputs`, spec §6.9): the input
//! picker (audio channels by the device's names, or a MIDI source and
//! channel), the monitor button that cycles A / I / –, and the input meter
//! with its clip mark. Choices are lane edits, one undo step each; while
//! recording the core refuses an input change (Busy) and the status line
//! says so.
//!
//! Nothing here is slow on the UI thread. The picker's sources (the MIDI
//! sources and the chosen device's channels, read without the engine) are
//! read off it ([`SourceRead`], run by the store's spawner) with a
//! generation guard; the meters read the core's peaks in the poll
//! ([`DocumentModel::poll_meters`]), never in render. One peak read feeds
//! two holds, the meters' and the control channel's (`state.get input`),
//! so each sees every peak.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use core_control::registry::ArgValue;
use core_control::{ControlError, ErrorCode};
use vizdaw_ffi::InputInfoRow;
use vizdaw_ffi::VizError;
use vizdaw_ffi::edit::{EditCommand, LaneEdit, LaneInputRow, MonitorModeRow};
use vizdaw_ffi::live::InputChannelsRow;
use vizdaw_ffi::snapshot::LaneRow;

use crate::commands::{Ran, check_names, ran};
use crate::document::DocumentModel;
use crate::editor::GESTURE_OPEN;
use crate::mixer::MixerMath;
use crate::mixer::commands::thrown;
use crate::session::{CoreCalls, LiveCore};

/// The channel's track input commands (key-only: they take arguments).
pub const LANE_INPUT_COMMANDS: &[&str] = &["lane.set_input", "lane.set_monitor"];

/// The status line while the input opens, on macOS (Swift
/// `LaneInputs.openingNote`).
pub const OPENING_NOTE_MACOS: &str =
    "Opening the audio input\u{2026} If macOS asks for microphone access, answer its prompt.";
/// The same elsewhere: the first sentence of the macOS text (no new
/// string; plan 8c Global Constraints).
pub const OPENING_NOTE_OTHER: &str = "Opening the audio input\u{2026}";

/// The opening note of the platform the app runs on.
pub fn opening_note() -> &'static str {
    if cfg!(target_os = "macos") {
        OPENING_NOTE_MACOS
    } else {
        OPENING_NOTE_OTHER
    }
}

/// How long a clip mark stays lit (plan 5d-1 Ruling 27).
pub const CLIP_HOLD: Duration = Duration::from_secs(2);

/// The most input channels the picker lists (the channel's `channel`
/// range).
const MAX_CHANNELS: usize = 256;

/// One entry of a track's input picker.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InputChoice {
    pub title: String,
    pub input: Option<LaneInputRow>,
}

impl InputChoice {
    pub fn new(title: impl Into<String>, input: Option<LaneInputRow>) -> Self {
        InputChoice {
            title: title.into(),
            input,
        }
    }
}

/// A titled section of the input picker ("" for the first, untitled one).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InputGroup {
    pub title: &'static str,
    pub choices: Vec<InputChoice>,
}

/// The MIDI sources and the chosen device's channels as last read.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct InputSources {
    pub midi: Vec<String>,
    pub device: Option<InputChannelsRow>,
}

/// A read of the picker's sources, to run off the UI thread.
pub struct SourceRead {
    /// The document's refresh this is (`None`: the launch warm-up).
    generation: Option<u64>,
    /// The device's channels are read too (the engine's input is closed).
    wants_device: bool,
    core: Arc<dyn CoreCalls>,
}

/// A source read's answer.
#[derive(Debug, Clone, PartialEq)]
pub struct SourcesRead {
    pub(crate) generation: Option<u64>,
    pub(crate) midi: Vec<String>,
    /// `None`: the device was not read (the engine's input is open).
    pub(crate) device: Option<Option<InputChannelsRow>>,
}

impl SourceRead {
    /// The launch warm-up's read: the sources and the device (Swift
    /// `warmSources`).
    pub(crate) fn warm(core: Arc<dyn CoreCalls>) -> Self {
        SourceRead {
            generation: None,
            wants_device: true,
            core,
        }
    }

    /// Reads (slow: a CoreMIDI listing and a HAL read; call it off the UI
    /// thread).
    pub fn run(self) -> SourcesRead {
        let midi = self.core.midi_source_ports();
        let device = self.wants_device.then(|| self.core.audio_input_channels());
        SourcesRead {
            generation: self.generation,
            midi,
            device,
        }
    }
}

/// A document's input controls' state.
pub struct LaneInputs {
    core: Arc<dyn CoreCalls>,
    /// The engine's input as last read (when a picker opens).
    info: Option<InputInfoRow>,
    /// The chosen device's channels as last read without the engine.
    device: Option<InputChannelsRow>,
    /// The MIDI sources as the last refresh read them.
    midi_sources: Vec<String>,
    /// The surfaces' input ports, which reach no track (plan 5f-1 Ruling
    /// 17). Empty until surfaces come to this app.
    claimed: Vec<String>,
    /// The last refresh started, and the newest one applied.
    started: u64,
    applied: u64,
    /// The refresh waiting to run (the store takes it).
    pending: Option<SourceRead>,
    /// Each metered track's level (0…2, peaks falling back slowly).
    levels: HashMap<u64, f32>,
    /// Tracks whose clip mark is lit, and until when.
    clip_until: HashMap<u64, Instant>,
    /// Each track's clipped-sample count in this recording.
    clipped_counts: HashMap<u64, u64>,
    /// Software Monitoring as last read.
    monitoring_on: bool,
    /// The input is opening while a track shows a meter.
    opening: bool,
    /// The peaks since the meters' last read, and since the channel's.
    meter_held: Vec<f32>,
    channel_held: Vec<f32>,
    /// Destination names by snapshot version (the return badge), read
    /// with the snapshot's lanes, never when a view draws.
    dest_names: Option<(u64, HashMap<u64, String>)>,
}

impl Default for LaneInputs {
    fn default() -> Self {
        LaneInputs::new(Arc::new(LiveCore))
    }
}

impl LaneInputs {
    /// Reads Software Monitoring from `core` (an atomic read).
    pub fn new(core: Arc<dyn CoreCalls>) -> Self {
        let monitoring_on = core.audio_settings().software_monitoring;
        LaneInputs {
            core,
            info: None,
            device: None,
            midi_sources: vec![],
            claimed: vec![],
            started: 0,
            applied: 0,
            pending: None,
            levels: HashMap::new(),
            clip_until: HashMap::new(),
            clipped_counts: HashMap::new(),
            monitoring_on,
            opening: false,
            meter_held: vec![],
            channel_held: vec![],
            dest_names: None,
        }
    }

    /// The store's core and the sources the last read found (a new
    /// document's first picker lists them).
    pub(crate) fn attach(&mut self, core: Arc<dyn CoreCalls>, last: &InputSources) {
        self.core = core;
        self.midi_sources = last.midi.clone();
        self.device = last.device.clone();
        self.refresh_monitoring();
    }

    // ---- The picker ----

    /// The engine's input as last read.
    pub fn info(&self) -> Option<&InputInfoRow> {
        self.info.as_ref()
    }

    /// The chosen device's channels as last read without the engine.
    pub fn device(&self) -> Option<&InputChannelsRow> {
        self.device.as_ref()
    }

    /// The MIDI sources the picker lists (one refresh old at most).
    pub fn midi_sources(&self) -> &[String] {
        &self.midi_sources
    }

    /// The surfaces' claimed input ports (marked in the picker).
    pub fn set_claimed_ports(&mut self, ports: Vec<String>) {
        self.claimed = ports;
    }

    /// The input channels the picker offers, by name: the engine's open
    /// input, else the chosen device's (`None`: unknown).
    fn known(&self) -> Option<(usize, &[String])> {
        let count = |c: u32| usize::try_from(c).unwrap_or(MAX_CHANNELS).min(MAX_CHANNELS);
        if let Some(i) = &self.info
            && i.channels > 0
        {
            return Some((count(i.channels), &i.names));
        }
        if let Some(d) = &self.device
            && d.channels > 0
        {
            return Some((count(d.channels), &d.names));
        }
        None
    }

    /// How many input channels the device has, when known.
    pub fn known_channels(&self) -> Option<usize> {
        self.known().map(|(n, _)| n)
    }

    /// Whether `lane` records audio and/or MIDI: by its input, else by
    /// its output or destination; a track with neither offers both.
    pub fn kinds(lane: &LaneRow) -> (bool, bool) {
        match &lane.input {
            Some(LaneInputRow::Audio { .. }) => (true, false),
            Some(LaneInputRow::Midi { .. }) => (false, true),
            None => {
                let audio = lane.audio_out.is_some();
                let midi = lane.midi_dest.is_some();
                if audio == midi {
                    (true, true)
                } else {
                    (audio, midi)
                }
            }
        }
    }

    /// The picker's sections for `lane`.
    pub fn groups(&self, lane: &LaneRow) -> Vec<InputGroup> {
        let mut out = vec![InputGroup {
            title: "",
            choices: vec![InputChoice::new("No Input", None)],
        }];
        let (audio, midi) = Self::kinds(lane);
        if audio {
            let return_of = match &lane.input {
                Some(LaneInputRow::Audio { return_of, .. }) => *return_of,
                _ => None,
            };
            let n = self.known_channels().unwrap_or(2);
            let at = |c: usize, stereo: bool| {
                Some(LaneInputRow::Audio {
                    first: u16::try_from(c).unwrap_or(u16::MAX),
                    stereo,
                    return_of,
                })
            };
            let mut choices: Vec<InputChoice> = (0..n)
                .map(|c| InputChoice::new(self.channel_title(c), at(c, false)))
                .collect();
            for c in (0..n.saturating_sub(1)).step_by(2) {
                choices.push(InputChoice::new(
                    format!("In {}+{}", c + 1, c + 2),
                    at(c, true),
                ));
            }
            out.push(InputGroup {
                title: "Audio",
                choices,
            });
        }
        if midi {
            let (port, channel) = match &lane.input {
                Some(LaneInputRow::Midi { port, channel }) => (port.clone(), *channel),
                _ => (None, None),
            };
            let mut ports = vec![InputChoice::new(
                "All MIDI Inputs",
                Some(LaneInputRow::Midi {
                    port: None,
                    channel,
                }),
            )];
            ports.extend(self.midi_sources.iter().map(|p| {
                let title = if self.claimed.contains(p) {
                    format!("{p} (used by a surface)")
                } else {
                    p.clone()
                };
                InputChoice::new(
                    title,
                    Some(LaneInputRow::Midi {
                        port: Some(p.clone()),
                        channel,
                    }),
                )
            }));
            out.push(InputGroup {
                title: "MIDI Input",
                choices: ports,
            });
            let mut channels = vec![InputChoice::new(
                "Any Channel",
                Some(LaneInputRow::Midi {
                    port: port.clone(),
                    channel: None,
                }),
            )];
            channels.extend((0..16u8).map(|c| {
                InputChoice::new(
                    format!("Channel {}", c + 1),
                    Some(LaneInputRow::Midi {
                        port: port.clone(),
                        channel: Some(c),
                    }),
                )
            }));
            out.push(InputGroup {
                title: "MIDI Channel",
                choices: channels,
            });
        }
        out
    }

    fn channel_title(&self, c: usize) -> String {
        let name = self
            .known()
            .and_then(|(_, names)| names.get(c))
            .map_or("", String::as_str);
        if name.is_empty() {
            format!("In {}", c + 1)
        } else {
            format!("In {} ({name})", c + 1)
        }
    }

    /// What the picker shows for an input.
    pub fn title(input: Option<&LaneInputRow>) -> String {
        match input {
            None => "No Input".into(),
            Some(LaneInputRow::Audio { first, stereo, .. }) => {
                let a = u32::from(*first) + 1;
                if *stereo {
                    format!("In {a}+{}", a + 1)
                } else {
                    format!("In {a}")
                }
            }
            Some(LaneInputRow::Midi { port, channel }) => {
                let p = port.as_deref().unwrap_or("All MIDI");
                match channel {
                    Some(c) => format!("{p} \u{00B7} Ch {}", u32::from(*c) + 1),
                    None => p.to_owned(),
                }
            }
        }
    }

    /// What every place that titles a track's input shows: "Port · MPE"
    /// for a MIDI input with MPE on (the channel dropped), else `title`.
    pub fn title_of(lane: &LaneRow) -> String {
        if let Some(LaneInputRow::Midi { port, .. }) = &lane.input
            && lane.mpe.is_some()
        {
            return format!("{} \u{00B7} MPE", port.as_deref().unwrap_or("All MIDI"));
        }
        Self::title(lane.input.as_ref())
    }

    /// The strip's picker title: the input, led by the track's name when
    /// another track plays through the same path.
    pub fn picker_title_shared(lane: &LaneRow, shares_path: bool) -> String {
        if shares_path {
            format!("{}: {}", lane.name, Self::title_of(lane))
        } else {
            Self::title_of(lane)
        }
    }

    // ---- The monitor button ----

    /// Auto → Input → Off → Auto.
    pub fn next_monitor(m: MonitorModeRow) -> MonitorModeRow {
        next_monitor(m)
    }

    /// The button's letter: A, I or –.
    pub fn monitor_label(m: MonitorModeRow) -> &'static str {
        match m {
            MonitorModeRow::Auto => "A",
            MonitorModeRow::Input => "I",
            MonitorModeRow::Off => "\u{2013}",
        }
    }

    /// The button's help and accessibility text.
    pub fn monitor_help(m: MonitorModeRow) -> &'static str {
        monitor_help(m)
    }

    // ---- The meters ----

    /// Software Monitoring as the last read found it.
    pub fn monitoring_on(&self) -> bool {
        self.monitoring_on
    }

    /// Reads Software Monitoring now (the meters follow it at once).
    pub fn refresh_monitoring(&mut self) {
        self.monitoring_on = self.core.audio_settings().software_monitoring;
    }

    /// A track shows its input meter while it is armed, or monitors its
    /// audio input with Software Monitoring on.
    pub fn shows_meter(&self, lane: &LaneRow) -> bool {
        shows_meter(lane, self.monitoring_on)
    }

    /// The input is opening while a track shows a meter.
    pub fn opening(&self) -> bool {
        self.opening
    }

    /// Track `lane`'s meter level (0 when it shows none).
    pub fn level(&self, lane: u64) -> f32 {
        self.levels.get(&lane).copied().unwrap_or(0.0)
    }

    /// Every metered track's level.
    pub fn levels(&self) -> &HashMap<u64, f32> {
        &self.levels
    }

    /// Track `lane`'s clip mark is lit.
    pub fn is_clipped(&self, lane: u64) -> bool {
        self.clip_until.contains_key(&lane)
    }

    /// The tracks whose clip mark is lit.
    pub fn clipped(&self) -> Vec<u64> {
        let mut v: Vec<u64> = self.clip_until.keys().copied().collect();
        v.sort_unstable();
        v
    }

    /// One core read (it resets the core's peaks) into both holds.
    fn take_peaks(&mut self, raw: &[f32]) {
        self.meter_held = merged(&self.meter_held, raw);
        self.channel_held = merged(&self.channel_held, raw);
    }

    /// The refresh waiting to run, taken (the store runs it off the UI
    /// thread).
    pub(crate) fn take_source_read(&mut self) -> Option<SourceRead> {
        self.pending.take()
    }

    /// A source read landed: applied unless a newer one already was.
    /// True when it was applied (the store keeps it for new documents).
    pub(crate) fn land_sources(&mut self, read: &SourcesRead) -> bool {
        let Some(mine) = read.generation else {
            return false;
        };
        if mine <= self.applied {
            return false;
        }
        self.applied = mine;
        self.midi_sources = read.midi.clone();
        if let Some(device) = &read.device {
            self.device = device.clone();
        }
        true
    }
}

/// The input monitor's next mode: Auto, Input, Off, Auto.
pub fn next_monitor(m: MonitorModeRow) -> MonitorModeRow {
    match m {
        MonitorModeRow::Auto => MonitorModeRow::Input,
        MonitorModeRow::Input => MonitorModeRow::Off,
        MonitorModeRow::Off => MonitorModeRow::Auto,
    }
}

/// The input monitor's help for mode `m` (Swift `LaneInputs.help`).
pub fn monitor_help(m: MonitorModeRow) -> &'static str {
    match m {
        MonitorModeRow::Auto => {
            "Input monitoring: Auto (heard while armed and stopped, or recording in range)"
        }
        MonitorModeRow::Input => "Input monitoring: always the input",
        MonitorModeRow::Off => "Input monitoring: off",
    }
}

/// [`LaneInputs::shows_meter`] with Software Monitoring given.
pub fn shows_meter(lane: &LaneRow, software_monitoring: bool) -> bool {
    if !matches!(lane.input, Some(LaneInputRow::Audio { .. })) {
        return false;
    }
    lane.arm || (software_monitoring && lane.monitor == MonitorModeRow::Input)
}

/// Elementwise louder of the two, sanitised; as long as the longer.
pub fn merged(a: &[f32], b: &[f32]) -> Vec<f32> {
    let at = |v: &[f32], i: usize| v.get(i).map_or(0.0, |p| MixerMath::sanitized_peak(*p));
    (0..a.len().max(b.len()))
        .map(|i| at(a, i).max(at(b, i)))
        .collect()
}

/// The first session whose input names a device: the one whose
/// latencies Settings › Audio shows (Swift `AudioPreferences.inputHolder`).
pub fn input_holder(sessions: &[Arc<dyn crate::DocSession>]) -> Option<Arc<dyn crate::DocSession>> {
    sessions
        .iter()
        .find(|s| s.input_info().is_ok_and(|i| !i.device.is_empty()))
        .cloned()
}

impl DocumentModel {
    /// The track input controls' state.
    pub fn inputs(&self) -> &LaneInputs {
        &self.inputs
    }

    pub fn inputs_mut(&mut self) -> &mut LaneInputs {
        &mut self.inputs
    }

    /// Some shown track meters its input: the poll runs fast.
    pub fn metering(&self) -> bool {
        self.lanes().iter().any(|l| self.inputs.shows_meter(l))
    }

    /// A picker opens: reads the engine's input now (a session read), and
    /// asks for the MIDI sources, and the device's channels while the
    /// engine's input is closed, off the UI thread (the store runs the
    /// read; [`crate::DocumentStore::refresh_inputs`]).
    pub fn refresh_input_info(&mut self) {
        if let Ok(i) = self.session.input_info() {
            self.inputs.info = Some(i);
        }
        let inputs = &mut self.inputs;
        inputs.started += 1;
        let wants_device = inputs.info.as_ref().is_none_or(|i| i.channels == 0);
        inputs.pending = Some(SourceRead {
            generation: Some(inputs.started),
            wants_device,
            core: inputs.core.clone(),
        });
    }

    /// The strip's picker title for `lane`, sharing read from the shown
    /// tracks.
    pub fn input_picker_title(&self, lane: &LaneRow) -> String {
        let shared = self
            .lanes()
            .iter()
            .filter(|l| l.audio_out == lane.audio_out)
            .count()
            > 1;
        LaneInputs::picker_title_shared(lane, shared)
    }

    /// The synth whose return `lane` records (`Synth` when its name is
    /// unknown); `None` for other tracks.
    pub fn input_return_name(&self, lane: &LaneRow) -> Option<String> {
        let Some(LaneInputRow::Audio {
            return_of: Some(r), ..
        }) = &lane.input
        else {
            return None;
        };
        Some(self.dest_name(*r).unwrap_or_else(|| "Synth".into()))
    }

    /// The link badge of a track that records a synth's return: "⇠ ‹synth›".
    pub fn input_badge(&self, lane: &LaneRow) -> Option<String> {
        self.input_return_name(lane)
            .map(|n| format!("\u{21E0} {n}"))
    }

    /// A destination's name as the snapshot's read found it (no session
    /// call: views draw the badge).
    fn dest_name(&self, id: u64) -> Option<String> {
        self.inputs
            .dest_names
            .as_ref()
            .and_then(|(_, m)| m.get(&id).cloned())
    }

    /// Reads the destinations' names for the return badges, once per
    /// snapshot version, while a shown track records a synth's return
    /// (called when the lanes are fetched again).
    pub(crate) fn warm_dest_names(&mut self) {
        let returns = self.lanes().iter().any(|l| {
            matches!(
                l.input,
                Some(LaneInputRow::Audio {
                    return_of: Some(_),
                    ..
                })
            )
        });
        let v = self.snapshot().version;
        if !returns
            || self
                .inputs
                .dest_names
                .as_ref()
                .is_some_and(|(at, _)| *at == v)
        {
            return;
        }
        let mut map = HashMap::new();
        for d in self.session.destinations().unwrap_or_default() {
            map.entry(d.id).or_insert(d.name);
        }
        self.inputs.dest_names = Some((v, map));
    }

    /// Sets `lane`'s input (one undo step); nothing when it is unchanged.
    /// A refusal (Busy while recording) is the status line.
    pub fn choose_input(&mut self, choice: &InputChoice, lane: &LaneRow) {
        let Some(seq) = self.selected_seq() else {
            return;
        };
        if choice.input == lane.input {
            return;
        }
        self.edit(EditCommand::Lane {
            edit: LaneEdit::SetInput {
                seq,
                lane: lane.id,
                input: choice.input.clone(),
            },
        });
    }

    /// The monitor button: the next mode (one undo step).
    pub fn cycle_lane_monitor(&mut self, lane: &LaneRow) {
        let Some(seq) = self.selected_seq() else {
            return;
        };
        self.edit(EditCommand::Lane {
            edit: LaneEdit::SetMonitor {
                seq,
                lane: lane.id,
                monitor: next_monitor(lane.monitor),
            },
        });
    }

    /// The input's peaks per channel since the channel's last read
    /// (`state.get input`); the meters' reads do not take them. A failed
    /// read keeps the hold for the next.
    pub fn take_channel_levels(&mut self) -> Result<Vec<f32>, VizError> {
        let raw = self.session.input_levels()?;
        self.inputs.take_peaks(&raw);
        Ok(std::mem::take(&mut self.inputs.channel_held))
    }

    fn set_opening(&mut self, on: bool) {
        if on == self.inputs.opening {
            return;
        }
        self.inputs.opening = on;
        let note = opening_note();
        if on {
            // Never over another message, such as a refusal from the same
            // click.
            if self.status().is_none() {
                self.set_status(Some(note.into()));
            }
        } else if self.status() == Some(note) {
            self.set_status(None);
        }
    }

    /// The poll's meter step (every poll, whatever the transport read
    /// did): Software Monitoring, then, while a track meters, the opening
    /// flag and the core's peaks once. Each metered track takes the louder
    /// of its channels against its decayed level; a peak at full scale, or
    /// a recording's new clipped samples, lights its clip mark for
    /// [`CLIP_HOLD`]. With no metered track the levels, clip marks, the
    /// meters' hold and the opening note clear. True when what the meters
    /// show changed.
    pub fn poll_meters(&mut self, now: Instant) -> bool {
        let before = (
            self.inputs.levels.clone(),
            self.inputs.clipped(),
            self.inputs.opening,
        );
        self.inputs.refresh_monitoring();
        // Clip counts are per recording.
        if !self.record().is_recording() {
            self.inputs.clipped_counts.clear();
        }
        if !self.metering() {
            self.inputs.meter_held.clear();
            self.set_opening(false);
            self.inputs.levels.clear();
            self.inputs.clip_until.clear();
        } else {
            let opening = self.session.input_info().is_ok_and(|i| i.opening);
            self.set_opening(opening);
            if let Ok(raw) = self.session.input_levels() {
                self.inputs.take_peaks(&raw);
            }
            let raw = std::mem::take(&mut self.inputs.meter_held);
            let mut next = HashMap::new();
            let counts: HashMap<u64, u64> = self
                .record()
                .state()
                .map(|s| s.lanes.iter().map(|l| (l.lane, l.clipped)).collect())
                .unwrap_or_default();
            let metered: Vec<(u64, u16, bool)> = self
                .lanes()
                .iter()
                .filter(|l| self.inputs.shows_meter(l))
                .filter_map(|l| match &l.input {
                    Some(LaneInputRow::Audio { first, stereo, .. }) => {
                        Some((l.id, *first, *stereo))
                    }
                    _ => None,
                })
                .collect();
            let inputs = &mut self.inputs;
            for (lane, first, stereo) in metered {
                let f = usize::from(first);
                let chans: &[usize] = if stereo { &[f, f + 1] } else { &[f] };
                let peak = chans
                    .iter()
                    .filter_map(|c| raw.get(*c).copied())
                    .fold(0.0f32, f32::max);
                next.insert(lane, peak.max(inputs.level(lane) * MixerMath::METER_DECAY));
                let count = counts.get(&lane).copied().unwrap_or(0);
                if peak >= 1.0 || count > inputs.clipped_counts.get(&lane).copied().unwrap_or(0) {
                    inputs.clip_until.insert(lane, now + CLIP_HOLD);
                }
                inputs.clipped_counts.insert(lane, count);
            }
            inputs
                .clip_until
                .retain(|lane, until| *until > now && next.contains_key(lane));
            inputs.levels = next;
        }
        before
            != (
                self.inputs.levels.clone(),
                self.inputs.clipped(),
                self.inputs.opening,
            )
    }
}

/// The track a call names: `seq` and `lane`, both whole numbers.
fn track_of(args: &[(String, ArgValue)]) -> Option<(u64, u64)> {
    let num = |name: &str| match arg(args, name) {
        Some(ArgValue::Int(i)) => u64::try_from(*i).ok(),
        _ => None,
    };
    Some((num("seq")?, num("lane")?))
}

fn arg<'a>(args: &'a [(String, ArgValue)], name: &str) -> Option<&'a ArgValue> {
    args.iter().find(|(n, _)| n == name).map(|(_, v)| v)
}

fn invalid(message: String) -> ControlError {
    ControlError::new(ErrorCode::InvalidParams, message)
}

/// The input `args` ask for (`Some(None)`: no input), or `None` when `kind`
/// is missing or wrong, a value is out of range, or a parameter belongs
/// to another kind (Swift `ControlLanes.input`).
pub fn input_of(args: &[(String, ArgValue)]) -> Option<Option<LaneInputRow>> {
    let has = |n: &str| arg(args, n).is_some();
    let audio_only = ["channel", "stereo", "return_of"];
    let midi_only = ["port", "midi_channel"];
    let int = |n: &str| match arg(args, n) {
        Some(ArgValue::Int(i)) => Some(*i),
        _ => None,
    };
    let kind = match arg(args, "kind") {
        Some(ArgValue::Text(t)) => t.as_str(),
        _ => return None,
    };
    match kind {
        "none" if audio_only.iter().chain(&midi_only).any(|n| has(n)) => None,
        "audio" if midi_only.iter().any(|n| has(n)) => None,
        "midi" if audio_only.iter().any(|n| has(n)) => None,
        "none" => Some(None),
        "audio" => {
            let channel = int("channel").unwrap_or(1);
            if !(1..=256).contains(&channel) {
                return None;
            }
            let return_of = match int("return_of") {
                Some(r) => Some(u64::try_from(r).ok()?),
                None => None,
            };
            let stereo = match arg(args, "stereo") {
                Some(ArgValue::Flag(f)) => *f,
                _ => false,
            };
            Some(Some(LaneInputRow::Audio {
                first: u16::try_from(channel - 1).ok()?,
                stereo,
                return_of,
            }))
        }
        "midi" => {
            let channel = match int("midi_channel") {
                Some(c) if (1..=16).contains(&c) => Some(u8::try_from(c - 1).ok()?),
                Some(_) => return None,
                None => None,
            };
            let port = match arg(args, "port") {
                Some(ArgValue::Text(t)) => Some(t.clone()),
                _ => None,
            };
            Some(Some(LaneInputRow::Midi { port, channel }))
        }
        _ => None,
    }
}

/// The monitor mode `text` names.
pub fn monitor_of(text: Option<&str>) -> Option<MonitorModeRow> {
    match text? {
        "auto" => Some(MonitorModeRow::Auto),
        "input" => Some(MonitorModeRow::Input),
        "off" => Some(MonitorModeRow::Off),
        _ => None,
    }
}

/// Whether a track input command can run now: not during a drag.
pub fn lane_input_enabled(doc: &DocumentModel) -> bool {
    !doc.gesture_open()
}

/// Runs `lane.set_input` or `lane.set_monitor` with the channel's `args`:
/// one lane edit, one undo step. Hostile or mismatched parameters are
/// `invalid_params`; an audio channel past the device's is the core's to
/// refuse; the core's Busy (recording, an open edit) is `busy`.
pub fn run_lane_input(
    id: &str,
    args: &[(String, ArgValue)],
    doc: &mut DocumentModel,
) -> Result<Ran, ControlError> {
    check_names(id, args)?;
    let Some((seq, lane)) = track_of(args) else {
        return Err(invalid(format!("{id}: `seq` and `lane` name the track")));
    };
    if doc.gesture_open() {
        return Err(ControlError::new(ErrorCode::Unavailable, GESTURE_OPEN));
    }
    let (edit, title) = if id == "lane.set_input" {
        let Some(input) = input_of(args) else {
            return Err(invalid(
                "lane.set_input: `kind` is none, audio or midi".into(),
            ));
        };
        if let Some(LaneInputRow::Audio {
            return_of: Some(dest),
            ..
        }) = &input
            && let Ok(all) = doc.session().destinations()
            && !all.iter().any(|d| d.external && d.id == *dest)
        {
            return Err(invalid(format!(
                "{id}: destination {dest} is not an external MIDI destination"
            )));
        }
        (LaneEdit::SetInput { seq, lane, input }, "Set Input Failed")
    } else {
        let mode = match arg(args, "mode") {
            Some(ArgValue::Text(t)) => monitor_of(Some(t)),
            _ => None,
        };
        let Some(monitor) = mode else {
            return Err(invalid(
                "lane.set_monitor: `mode` is auto, input or off".into(),
            ));
        };
        (
            LaneEdit::SetMonitor { seq, lane, monitor },
            "Set Monitor Failed",
        )
    };
    let result = doc.session().edit(EditCommand::Lane { edit });
    doc.apply_result(result).map_err(|e| thrown(&e, title))?;
    Ok(ran(doc))
}
