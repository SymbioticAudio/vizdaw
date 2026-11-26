//! The app's editing settings (Swift `EditingPreferences`,
//! `Nudge.swift:24-124`): both nudge steps, the default crossfade, zero-
//! crossing snapping and the external sample editor. They belong to the
//! machine, so they live in the preferences (keys `nudge`, `crossfadeMs`,
//! `zeroCrossings`, `externalEditor`), never in a project. Decoding is
//! tolerant: a missing or damaged field keeps its default. Nothing reads
//! them yet: nudge, crossfade length, zero-crossing snapping and Open in
//! External Editor arrive with their features.

use std::path::{Path, PathBuf};

use serde_json::{Value, json};

use crate::arrangement::PPQ;
use crate::editor::Division;
use crate::prefs::Prefs;

pub const NUDGE_KEY: &str = "nudge";
pub const CROSSFADE_KEY: &str = "crossfadeMs";
pub const ZERO_CROSSINGS_KEY: &str = "zeroCrossings";
pub const EXTERNAL_EDITOR_KEY: &str = "externalEditor";

/// The default crossfade's range and default, ms (`Nudge.swift:97-98`).
pub const CROSSFADE_MIN: f64 = 1.;
pub const CROSSFADE_MAX: f64 = 1000.;
pub const CROSSFADE_DEFAULT: f64 = 10.;
/// The ticks stepper: 1 to 61 440 in steps of 60 (Swift `:86`).
pub const TICKS_MIN: u32 = 1;
pub const TICKS_MAX: u32 = 61_440;
pub const TICKS_STEP: u32 = 60;
pub const TICKS_DEFAULT: u32 = 240;
/// The pitch amount's range.
pub const AMOUNT_MIN: u8 = 1;
pub const AMOUNT_MAX: u8 = 12;

/// The zero-crossing toggle's title and help (Swift `:56-58`).
pub const ZERO_CROSSINGS_TITLE: &str = "Snap audio edits to zero crossings";
pub const ZERO_CROSSINGS_HELP: &str = "Moves a trimmed start or end to the nearest zero crossing within 3 ms, so the cut does not click.";
/// The panel's message (Swift `ExternalEditing.swift:105`).
pub const EDITOR_PANEL_MESSAGE: &str =
    "Choose the application that edits audio files for Open in External Editor.";
/// The panel's title (:104).
pub const EDITOR_PANEL_TITLE: &str = "Choose External Sample Editor";
/// The pitch roots (Swift `PitchName.names`).
pub const ROOT_NAMES: [&str; 12] = [
    "C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B",
];

/// The ticks stepper's text: `240 ticks (15360 per quarter note)`.
pub fn ticks_text(n: u32) -> String {
    format!("{n} ticks ({PPQ} per quarter note)")
}

/// "10 ms", "12.5 ms": one decimal place only when there is one.
pub fn ms_text(v: f64) -> String {
    if v == v.round() {
        format!("{} ms", v as i64)
    } else {
        format!("{v:.1} ms")
    }
}

/// The scales nudge can follow (Swift `Scale`, `PitchMath.swift:18-45`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ScaleKind {
    #[default]
    Major,
    NaturalMinor,
    HarmonicMinor,
    Dorian,
    Mixolydian,
    MajorPentatonic,
    MinorPentatonic,
}

impl ScaleKind {
    pub const ALL: [ScaleKind; 7] = [
        ScaleKind::Major,
        ScaleKind::NaturalMinor,
        ScaleKind::HarmonicMinor,
        ScaleKind::Dorian,
        ScaleKind::Mixolydian,
        ScaleKind::MajorPentatonic,
        ScaleKind::MinorPentatonic,
    ];

    pub fn title(self) -> &'static str {
        match self {
            ScaleKind::Major => "Major",
            ScaleKind::NaturalMinor => "Natural Minor",
            ScaleKind::HarmonicMinor => "Harmonic Minor",
            ScaleKind::Dorian => "Dorian",
            ScaleKind::Mixolydian => "Mixolydian",
            ScaleKind::MajorPentatonic => "Major Pentatonic",
            ScaleKind::MinorPentatonic => "Minor Pentatonic",
        }
    }

    /// Swift's raw value (the JSON name).
    pub fn name(self) -> &'static str {
        match self {
            ScaleKind::Major => "major",
            ScaleKind::NaturalMinor => "naturalMinor",
            ScaleKind::HarmonicMinor => "harmonicMinor",
            ScaleKind::Dorian => "dorian",
            ScaleKind::Mixolydian => "mixolydian",
            ScaleKind::MajorPentatonic => "majorPentatonic",
            ScaleKind::MinorPentatonic => "minorPentatonic",
        }
    }

    fn from_name(n: &str) -> Option<ScaleKind> {
        Self::ALL.into_iter().find(|s| s.name() == n)
    }
}

/// The time step of a nudge.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum NudgeTime {
    /// The edit grid's step.
    #[default]
    Grid,
    /// A note value, optionally a triplet.
    Division(Division, bool),
    Ticks(u32),
}

/// The Editing page's nudge settings (Swift `NudgeSettings`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NudgeSettings {
    pub time: NudgeTime,
    /// Scale steps rather than semitones.
    pub pitch_scale_steps: bool,
    /// How many semitones or scale steps, 1-12.
    pub amount: u8,
    pub follow_key_signature: bool,
    /// 0-11.
    pub root: u8,
    pub scale: ScaleKind,
}

impl Default for NudgeSettings {
    fn default() -> Self {
        NudgeSettings {
            time: NudgeTime::Grid,
            pitch_scale_steps: false,
            amount: 1,
            follow_key_signature: true,
            root: 0,
            scale: ScaleKind::Major,
        }
    }
}

fn division_name(d: Division) -> &'static str {
    match d {
        Division::ThirtySecond => "thirtySecond",
        other => other.name(),
    }
}

fn division_from(n: &str) -> Option<Division> {
    Division::ALL
        .into_iter()
        .find(|d| d.name() == n || division_name(*d) == n)
}

/// The one payload of a Swift synthesised enum case (`{"case":{"_0":x}}`).
fn case<'a>(v: &'a Value, name: &str) -> Option<&'a Value> {
    v.get(name)
}

impl NudgeSettings {
    /// The settings in `v`; each damaged field keeps its default.
    pub fn from_value(v: &Value) -> Self {
        let d = NudgeSettings::default();
        let time = v
            .get("time")
            .and_then(|t| {
                if case(t, "grid").is_some() {
                    Some(NudgeTime::Grid)
                } else if let Some(c) = case(t, "division") {
                    let div = division_from(c.get("_0")?.as_str()?)?;
                    Some(NudgeTime::Division(div, c.get("triplet")?.as_bool()?))
                } else {
                    let n = case(t, "ticks")?.get("_0")?.as_i64()?;
                    Some(NudgeTime::Ticks(
                        n.clamp(i64::from(TICKS_MIN), i64::from(TICKS_MAX)) as u32,
                    ))
                }
            })
            .unwrap_or(d.time);
        let (pitch_scale_steps, amount) = v
            .get("pitch")
            .and_then(|p| {
                let (steps, c) = match case(p, "semitones") {
                    Some(c) => (false, c),
                    None => (true, case(p, "scaleSteps")?),
                };
                let n = c.get("_0")?.as_i64()?;
                Some((
                    steps,
                    n.clamp(i64::from(AMOUNT_MIN), i64::from(AMOUNT_MAX)) as u8,
                ))
            })
            .unwrap_or((d.pitch_scale_steps, d.amount));
        NudgeSettings {
            time,
            pitch_scale_steps,
            amount,
            follow_key_signature: v
                .get("followKeySignature")
                .and_then(Value::as_bool)
                .unwrap_or(d.follow_key_signature),
            root: v
                .get("root")
                .and_then(Value::as_u64)
                .map_or(d.root, |r| (r % 12) as u8),
            scale: v
                .get("scale")
                .and_then(Value::as_str)
                .and_then(ScaleKind::from_name)
                .unwrap_or(d.scale),
        }
    }

    /// The settings as Swift's `Codable` writes them.
    pub fn to_value(&self) -> Value {
        let time = match self.time {
            NudgeTime::Grid => json!({"grid": {}}),
            NudgeTime::Division(d, t) => {
                json!({"division": {"_0": division_name(d), "triplet": t}})
            }
            NudgeTime::Ticks(n) => json!({"ticks": {"_0": n}}),
        };
        let pitch = if self.pitch_scale_steps {
            json!({"scaleSteps": {"_0": self.amount}})
        } else {
            json!({"semitones": {"_0": self.amount}})
        };
        json!({
            "time": time,
            "pitch": pitch,
            "followKeySignature": self.follow_key_signature,
            "root": self.root,
            "scale": self.scale.name(),
        })
    }
}

fn clamp_crossfade(v: f64) -> f64 {
    if v.is_nan() {
        CROSSFADE_DEFAULT
    } else {
        v.clamp(CROSSFADE_MIN, CROSSFADE_MAX)
    }
}

/// The name the Application line shows: an `.app` without its suffix
/// (Swift `displayName`), any other file by name.
pub fn editor_display_name(path: &Path) -> String {
    let name = path.file_name().map_or_else(
        || path.display().to_string(),
        |n| n.to_string_lossy().into(),
    );
    match name.strip_suffix(".app") {
        Some(stem) if !stem.is_empty() => stem.to_owned(),
        _ => name,
    }
}

/// The editing settings the app keeps and the last write failure.
#[derive(Debug, Clone)]
pub struct EditingPrefs {
    nudge: NudgeSettings,
    crossfade_ms: f64,
    zero_crossings: bool,
    external_editor: Option<PathBuf>,
    problem: Option<String>,
}

impl Default for EditingPrefs {
    fn default() -> Self {
        EditingPrefs {
            nudge: NudgeSettings::default(),
            crossfade_ms: CROSSFADE_DEFAULT,
            zero_crossings: true,
            external_editor: None,
            problem: None,
        }
    }
}

impl EditingPrefs {
    /// The settings in `prefs`; a missing or damaged field is its default.
    pub fn load(prefs: &Prefs) -> EditingPrefs {
        let d = EditingPrefs::default();
        EditingPrefs {
            nudge: prefs
                .raw(NUDGE_KEY)
                .map(NudgeSettings::from_value)
                .unwrap_or(d.nudge),
            crossfade_ms: prefs
                .raw(CROSSFADE_KEY)
                .and_then(Value::as_f64)
                .map_or(d.crossfade_ms, clamp_crossfade),
            // A number other than 0 or 1, and anything else damaged, is on.
            zero_crossings: match prefs.raw(ZERO_CROSSINGS_KEY) {
                Some(Value::Bool(b)) => *b,
                Some(Value::Number(n)) if n.as_i64() == Some(0) => false,
                _ => d.zero_crossings,
            },
            external_editor: prefs
                .raw(EXTERNAL_EDITOR_KEY)
                .and_then(Value::as_str)
                .filter(|s| !s.is_empty())
                .map(PathBuf::from),
            problem: None,
        }
    }

    pub fn nudge(&self) -> &NudgeSettings {
        &self.nudge
    }

    pub fn crossfade_ms(&self) -> f64 {
        self.crossfade_ms
    }

    pub fn zero_crossings(&self) -> bool {
        self.zero_crossings
    }

    pub fn external_editor(&self) -> Option<&Path> {
        self.external_editor.as_deref()
    }

    /// Why the last write failed (`None` after one that took).
    pub fn problem(&self) -> Option<&str> {
        self.problem.as_deref()
    }

    /// Keeps the value for this run and saves it; a failed write is the
    /// `problem` (and the error).
    fn save(&mut self, key: &str, value: Value, prefs: &mut Prefs) -> Result<(), String> {
        let r = prefs.set_raw(key, value);
        self.problem = r.clone().err();
        r
    }

    pub fn set_nudge(&mut self, nudge: NudgeSettings, prefs: &mut Prefs) -> Result<(), String> {
        let v = nudge.to_value();
        self.nudge = nudge;
        self.save(NUDGE_KEY, v, prefs)
    }

    /// Clamped to 1-1000 ms; not a number is the default.
    pub fn set_crossfade_ms(&mut self, ms: f64, prefs: &mut Prefs) -> Result<(), String> {
        self.crossfade_ms = clamp_crossfade(ms);
        self.save(CROSSFADE_KEY, json!(self.crossfade_ms), prefs)
    }

    pub fn set_zero_crossings(&mut self, on: bool, prefs: &mut Prefs) -> Result<(), String> {
        self.zero_crossings = on;
        self.save(ZERO_CROSSINGS_KEY, json!(on), prefs)
    }

    pub fn set_external_editor(
        &mut self,
        path: Option<PathBuf>,
        prefs: &mut Prefs,
    ) -> Result<(), String> {
        let v = path
            .as_ref()
            .map_or(Value::Null, |p| json!(p.to_string_lossy()));
        self.external_editor = path;
        self.save(EXTERNAL_EDITOR_KEY, v, prefs)
    }
}
