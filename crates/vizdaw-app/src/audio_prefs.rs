//! The app's audio and recording settings (Swift `AudioPreferences` and
//! `AudioReadouts`; plan 5d-2 Ruling 2): they belong to the machine, so
//! they live in the preferences under `audio`, never in a project. Every
//! change goes to the core first and is kept only once the core took it;
//! a refusal leaves the setting as it was and says why in `problem`.
//! Decoding is tolerant: a missing or damaged field keeps its default.

use serde::{Deserialize, Deserializer, Serialize};
use serde_json::Value;
use vizdaw_ffi::audio::BUFFER_SIZES as CORE_BUFFER_SIZES;
use vizdaw_ffi::bounce::BounceSettingsRow;
use vizdaw_ffi::edit::MonitorModeRow;
use vizdaw_ffi::host::InputDeviceRow;
use vizdaw_ffi::{InputInfoRow, VizError};

use crate::alerts::error_message;
use crate::prefs::Prefs;
use crate::record::RecordDefaults;
use crate::session::CoreCalls;

/// The preferences key (Swift "VizDAW.audio", without the prefix).
pub const AUDIO_KEY: &str = "audio";
/// The I/O buffer sizes offered, frames (the core's list).
pub const BUFFER_SIZES: [u32; 6] = CORE_BUFFER_SIZES;
/// The RAM cache sizes offered, MiB (64-8192; spec §5.5: 512).
pub const CACHE_CHOICES: [u32; 8] = [64, 128, 256, 512, 1024, 2048, 4096, 8192];
/// The monitor modes' names.
pub const MONITORS: [&str; 3] = ["auto", "input", "off"];
/// The largest latency trim either way, frames.
pub const MAX_TRIM: i32 = 192_000;
/// Spec §6.2's thresholds, shown only.
pub const DISK_TEXT: &str = "Recording warns below 1 GB of free disk space and stops below 100 MB.";

/// The settings, with Swift's fields, defaults and JSON names.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AudioSettings {
    pub software_monitoring: bool,
    pub buffer_frames: u32,
    /// The input device's name; `None`: "Same as output". Kept beside the
    /// UID for the picker's "(not connected)" title, and alone in
    /// settings saved before devices were kept by UID.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub input_device: Option<String>,
    /// The input device's UID: what the core gets when set, so two
    /// devices with one name are kept apart.
    #[serde(rename = "inputDeviceUID", skip_serializing_if = "Option::is_none")]
    pub input_device_uid: Option<String>,
    pub latency_trim: i32,
    pub cache_mb: u32,
    pub read_ahead_seconds: u32,
    /// "auto", "input" or "off".
    pub default_monitor: String,
    pub count_in_bars: u8,
    pub pre_roll_bars: u8,
    /// Loop recording: each MIDI pass becomes a take. The old
    /// `passesAsTakes` key is not read, so the new default holds.
    pub midi_passes_as_takes: bool,
    /// The bounce's longest tail in seconds, 0-30.
    pub bounce_tail_seconds: u32,
}

impl Default for AudioSettings {
    fn default() -> Self {
        AudioSettings {
            software_monitoring: true,
            buffer_frames: 128,
            input_device: None,
            input_device_uid: None,
            latency_trim: 0,
            cache_mb: 512,
            read_ahead_seconds: 3,
            default_monitor: "auto".into(),
            count_in_bars: 0,
            pre_roll_bars: 0,
            midi_passes_as_takes: false,
            bounce_tail_seconds: 10,
        }
    }
}

/// `v[key]` as a `T`; a missing or damaged one is `None`.
fn field<T: serde::de::DeserializeOwned>(v: &Value, key: &str) -> Option<T> {
    v.get(key)
        .and_then(|x| serde_json::from_value(x.clone()).ok())
}

impl AudioSettings {
    /// The settings in `v`; each damaged field keeps its default and the
    /// others are read (a value that is no object is all defaults).
    pub fn from_value(v: &Value) -> Self {
        let d = AudioSettings::default();
        let uid: Option<String> = field(v, "inputDeviceUID");
        AudioSettings {
            software_monitoring: field(v, "softwareMonitoring").unwrap_or(d.software_monitoring),
            buffer_frames: field(v, "bufferFrames")
                .filter(|b| BUFFER_SIZES.contains(b))
                .unwrap_or(d.buffer_frames),
            input_device: field(v, "inputDevice"),
            input_device_uid: uid.filter(|u| !u.is_empty()),
            latency_trim: field(v, "latencyTrim")
                .filter(|t: &i32| t.abs() <= MAX_TRIM)
                .unwrap_or(d.latency_trim),
            cache_mb: field(v, "cacheMb")
                .filter(|m| (64..=8192).contains(m))
                .unwrap_or(d.cache_mb),
            read_ahead_seconds: field(v, "readAheadSeconds")
                .filter(|s| (2..=8).contains(s))
                .unwrap_or(d.read_ahead_seconds),
            default_monitor: field(v, "defaultMonitor")
                .filter(|m: &String| MONITORS.contains(&m.as_str()))
                .unwrap_or(d.default_monitor),
            count_in_bars: field::<u8>(v, "countInBars")
                .unwrap_or(d.count_in_bars)
                .min(4),
            pre_roll_bars: field::<u8>(v, "preRollBars")
                .unwrap_or(d.pre_roll_bars)
                .min(8),
            midi_passes_as_takes: field(v, "midiPassesAsTakes").unwrap_or(d.midi_passes_as_takes),
            bounce_tail_seconds: field(v, "bounceTailSeconds")
                .filter(|t| *t <= 30)
                .unwrap_or(d.bounce_tail_seconds),
        }
    }

    /// The default monitor mode as the core spells it.
    pub fn monitor_row(&self) -> MonitorModeRow {
        match self.default_monitor.as_str() {
            "input" => MonitorModeRow::Input,
            "off" => MonitorModeRow::Off,
            _ => MonitorModeRow::Auto,
        }
    }

    /// What the core gets for the input device: the UID, else the name.
    fn core_device(&self) -> Option<String> {
        self.input_device_uid
            .clone()
            .or_else(|| self.input_device.clone())
    }
}

impl<'de> Deserialize<'de> for AudioSettings {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        Ok(AudioSettings::from_value(&Value::deserialize(d)?))
    }
}

/// The audio settings the app keeps and the last refusal.
#[derive(Debug, Clone, Default)]
pub struct AudioPrefs {
    settings: AudioSettings,
    problem: Option<String>,
}

impl AudioPrefs {
    /// The settings in `prefs`; a missing or damaged value is the
    /// defaults (and is kept as it is until one is set).
    pub fn load(prefs: &Prefs) -> AudioPrefs {
        AudioPrefs {
            settings: prefs
                .raw(AUDIO_KEY)
                .map(AudioSettings::from_value)
                .unwrap_or_default(),
            problem: None,
        }
    }

    pub fn settings(&self) -> &AudioSettings {
        &self.settings
    }

    /// Why the last change was refused (`None` after one that took).
    pub fn problem(&self) -> Option<&str> {
        self.problem.as_deref()
    }

    /// `apply_all`, and a refusal becomes the `problem` (at launch).
    pub fn apply_at_launch(&mut self, core: &dyn CoreCalls) {
        self.problem = self.apply_all(core);
    }

    /// Hands every setting to the core (at launch, before a document
    /// opens). The refusal of a buffer or stream size, if any, is the
    /// answer; a refused buffer size skips the stream settings call (as
    /// in Swift).
    pub fn apply_all(&self, core: &dyn CoreCalls) -> Option<String> {
        let s = &self.settings;
        core.set_software_monitoring(s.software_monitoring);
        core.set_audio_input_device(s.core_device());
        core.set_record_latency_trim(s.latency_trim);
        core.set_default_monitor_mode(s.monitor_row());
        core.set_bounce_settings(BounceSettingsRow {
            tail_cap_ms: s.bounce_tail_seconds * 1000,
        });
        let sizes = core
            .set_io_buffer_frames(s.buffer_frames)
            .and_then(|()| core.set_stream_settings(s.cache_mb, s.read_ahead_seconds));
        sizes.err().map(|e| error_message(&e))
    }

    /// What a new document's record options start as.
    pub fn record_defaults(&self) -> RecordDefaults {
        RecordDefaults {
            count_in_bars: self.settings.count_in_bars,
            pre_roll_bars: self.settings.pre_roll_bars,
            midi_passes_as_takes: self.settings.midi_passes_as_takes,
        }
    }

    /// The core took `s`: it is shown now, and saved. A failed write is
    /// the `problem` (and the error); the core keeps the new value for
    /// this run.
    fn keep(&mut self, s: AudioSettings, prefs: &mut Prefs) -> Result<(), String> {
        self.settings = s;
        self.problem = None;
        let saved = serde_json::to_value(&self.settings)
            .map_err(|e| e.to_string())
            .and_then(|v| prefs.set_raw(AUDIO_KEY, v));
        if let Err(e) = &saved {
            self.problem = Some(e.clone());
        }
        saved
    }

    /// A refusal of the core: the setting stays, the `problem` says why.
    fn refused(&mut self, e: VizError) -> String {
        let m = error_message(&e);
        self.problem = Some(m.clone());
        m
    }

    pub fn set_software_monitoring(
        &mut self,
        on: bool,
        prefs: &mut Prefs,
        core: &dyn CoreCalls,
    ) -> Result<(), String> {
        let mut s = self.settings.clone();
        core.set_software_monitoring(on);
        s.software_monitoring = on;
        self.keep(s, prefs)
    }

    /// `Busy` while recording; a size off the list is refused.
    pub fn set_buffer_frames(
        &mut self,
        frames: u32,
        prefs: &mut Prefs,
        core: &dyn CoreCalls,
    ) -> Result<(), String> {
        let mut s = self.settings.clone();
        core.set_io_buffer_frames(frames)
            .map_err(|e| self.refused(e))?;
        s.buffer_frames = frames;
        self.keep(s, prefs)
    }

    /// The device (`None`: "Same as output"); the core gets its UID, else
    /// its name.
    pub fn set_input_device(
        &mut self,
        device: Option<InputDeviceRow>,
        prefs: &mut Prefs,
        core: &dyn CoreCalls,
    ) -> Result<(), String> {
        let mut s = self.settings.clone();
        s.input_device = device.as_ref().map(|d| d.name.clone());
        s.input_device_uid = device.map(|d| d.uid).filter(|u| !u.is_empty());
        core.set_audio_input_device(s.core_device());
        self.keep(s, prefs)
    }

    /// Frames, clamped to ±192 000.
    pub fn set_latency_trim(
        &mut self,
        frames: i32,
        prefs: &mut Prefs,
        core: &dyn CoreCalls,
    ) -> Result<(), String> {
        let mut s = self.settings.clone();
        s.latency_trim = frames.clamp(-MAX_TRIM, MAX_TRIM);
        core.set_record_latency_trim(s.latency_trim);
        self.keep(s, prefs)
    }

    /// Seconds, clamped to 0-30.
    pub fn set_bounce_tail_seconds(
        &mut self,
        seconds: u32,
        prefs: &mut Prefs,
        core: &dyn CoreCalls,
    ) -> Result<(), String> {
        let mut s = self.settings.clone();
        s.bounce_tail_seconds = seconds.min(30);
        core.set_bounce_settings(BounceSettingsRow {
            tail_cap_ms: s.bounce_tail_seconds * 1000,
        });
        self.keep(s, prefs)
    }

    /// The RAM cache in MiB, clamped to 64-8192.
    pub fn set_cache_mb(
        &mut self,
        mb: u32,
        prefs: &mut Prefs,
        core: &dyn CoreCalls,
    ) -> Result<(), String> {
        let (mb, secs) = (mb.clamp(64, 8192), self.settings.read_ahead_seconds);
        self.set_stream(mb, secs, prefs, core)
    }

    /// The disk read-ahead in seconds, clamped to 2-8.
    pub fn set_read_ahead_seconds(
        &mut self,
        seconds: u32,
        prefs: &mut Prefs,
        core: &dyn CoreCalls,
    ) -> Result<(), String> {
        let (mb, secs) = (self.settings.cache_mb, seconds.clamp(2, 8));
        self.set_stream(mb, secs, prefs, core)
    }

    fn set_stream(
        &mut self,
        cache_mb: u32,
        read_ahead_seconds: u32,
        prefs: &mut Prefs,
        core: &dyn CoreCalls,
    ) -> Result<(), String> {
        let mut s = self.settings.clone();
        core.set_stream_settings(cache_mb, read_ahead_seconds)
            .map_err(|e| self.refused(e))?;
        s.cache_mb = cache_mb;
        s.read_ahead_seconds = read_ahead_seconds;
        self.keep(s, prefs)
    }

    /// "auto", "input" or "off"; any other name changes nothing.
    pub fn set_default_monitor(
        &mut self,
        mode: &str,
        prefs: &mut Prefs,
        core: &dyn CoreCalls,
    ) -> Result<(), String> {
        if !MONITORS.contains(&mode) {
            return Ok(());
        }
        let mut s = self.settings.clone();
        s.default_monitor = mode.into();
        core.set_default_monitor_mode(s.monitor_row());
        self.keep(s, prefs)
    }

    /// Bars of count-in for new documents, at most 4 (no core call).
    pub fn set_count_in_bars(
        &mut self,
        bars: u8,
        prefs: &mut Prefs,
        _core: &dyn CoreCalls,
    ) -> Result<(), String> {
        let mut s = self.settings.clone();
        s.count_in_bars = bars.min(4);
        self.keep(s, prefs)
    }

    /// Bars of pre-roll for new documents, at most 8 (no core call).
    pub fn set_pre_roll_bars(
        &mut self,
        bars: u8,
        prefs: &mut Prefs,
        _core: &dyn CoreCalls,
    ) -> Result<(), String> {
        let mut s = self.settings.clone();
        s.pre_roll_bars = bars.min(8);
        self.keep(s, prefs)
    }

    /// Loop recording's MIDI passes as takes, for new documents (no core
    /// call).
    pub fn set_midi_passes_as_takes(
        &mut self,
        on: bool,
        prefs: &mut Prefs,
        _core: &dyn CoreCalls,
    ) -> Result<(), String> {
        let mut s = self.settings.clone();
        s.midi_passes_as_takes = on;
        self.keep(s, prefs)
    }
}

/// One entry of the input device picker.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeviceChoice {
    pub title: String,
    pub uid: Option<String>,
    pub name: Option<String>,
}

impl DeviceChoice {
    /// The choice's picker tag: its UID, else its name (`None`: "Same as
    /// output").
    pub fn tag(&self) -> Option<&str> {
        self.uid.as_deref().or(self.name.as_deref())
    }
}

/// The chosen device's picker tag: its UID; for a name alone (older
/// settings), the UID of the only listed device with that name, else the
/// name.
pub fn device_tag(s: &AudioSettings, devices: &[InputDeviceRow]) -> Option<String> {
    if let Some(uid) = &s.input_device_uid {
        return Some(uid.clone());
    }
    let name = s.input_device.as_ref()?;
    let named: Vec<_> = devices
        .iter()
        .filter(|d| &d.name == name && !d.uid.is_empty())
        .collect();
    match named.as_slice() {
        [one] => Some(one.uid.clone()),
        _ => Some(name.clone()),
    }
}

/// The input device picker: "Same as output", every device, and the
/// chosen one when it is not connected. Two devices with one name are
/// "<name>" and "<name> (2)", in list order.
pub fn device_choices(s: &AudioSettings, devices: &[InputDeviceRow]) -> Vec<DeviceChoice> {
    let mut out = vec![DeviceChoice {
        title: "Same as output".into(),
        uid: None,
        name: None,
    }];
    let mut seen: std::collections::HashMap<&str, u32> = std::collections::HashMap::new();
    for d in devices {
        let n = seen.entry(d.name.as_str()).or_insert(0);
        *n += 1;
        out.push(DeviceChoice {
            title: if *n == 1 {
                d.name.clone()
            } else {
                format!("{} ({})", d.name, n)
            },
            uid: Some(d.uid.clone()).filter(|u| !u.is_empty()),
            name: Some(d.name.clone()),
        });
    }
    if let Some(chosen) = device_tag(s, devices)
        && !out.iter().any(|c| c.tag() == Some(chosen.as_str()))
    {
        out.push(DeviceChoice {
            title: format!(
                "{} (not connected)",
                s.input_device.as_deref().unwrap_or(&chosen)
            ),
            uid: s.input_device_uid.clone(),
            name: s.input_device.clone(),
        });
    }
    out
}

/// The round-trip readout: the device's measured round trip once it has
/// opened, else the buffer's share of it.
pub fn round_trip_text(info: Option<&InputInfoRow>, buffer_frames: u32) -> String {
    match info {
        Some(i) if !i.device.is_empty() && i.sample_rate > 0.0 && i.round_trip_frames > 0 => {
            let ms = f64::from(i.round_trip_frames) / i.sample_rate * 1000.0;
            let mut text = format!(
                "Round trip {ms:.1} ms (input {} + output {} + 2 × {} frames)",
                i.input_latency_frames, i.output_latency_frames, i.buffer_frames
            );
            if i.buffer_frames != buffer_frames {
                text += &format!("; {} runs at {} frames", i.device, i.buffer_frames);
            }
            text
        }
        _ => {
            let ms = 2.0 * f64::from(buffer_frames) / 48_000.0 * 1000.0;
            format!("About {ms:.1} ms at 48 kHz, plus the device's own latency once it opens")
        }
    }
}

/// The latency trim's readout: "+12 frames (0.25 ms)".
pub fn trim_text(frames: i32, sample_rate: f64) -> String {
    let sign = if frames > 0 { "+" } else { "" };
    let rate = if sample_rate > 0.0 {
        sample_rate
    } else {
        48_000.0
    };
    format!(
        "{sign}{frames} frames ({:.2} ms)",
        f64::from(frames) / rate * 1000.0
    )
}

/// The RAM cache sizes, with `current` added in order when it is not on
/// the list (a picker never shows an empty selection).
pub fn cache_choices(current: u32) -> Vec<u32> {
    let mut v = CACHE_CHOICES.to_vec();
    if !v.contains(&current) {
        v.push(current);
        v.sort_unstable();
    }
    v
}

/// The count-in stepper's value: "off", "1 bar" or "n bars".
pub fn count_text(bars: u8) -> String {
    match bars {
        0 => "off".into(),
        1 => "1 bar".into(),
        n => format!("{n} bars"),
    }
}
