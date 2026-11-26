//! The app's automation settings (Swift `AutomationPreferences`; plan
//! 5e-2 Ruling 7): they belong to the machine, so they live in the
//! preferences under `automation`, never in a project. Every change goes
//! to the core first and is kept only once the core took it; a refusal
//! leaves the setting as it was and says why in `problem`. Decoding is
//! tolerant: a missing or damaged field keeps its default.

use serde::{Deserialize, Deserializer, Serialize};
use serde_json::Value;
use vizdaw_ffi::VizError;
use vizdaw_ffi::automation::{AutomationModeRow, AutomationSettingsRow, ThinningRow};

use crate::alerts::error_message;
use crate::prefs::Prefs;
use crate::session::CoreCalls;

/// The preferences key (Swift "VizDAW.automation", without the prefix).
pub const AUTOMATION_KEY: &str = "automation";
/// The longest return time, ms.
pub const MAX_RETURN_MS: u32 = 2000;
/// The return time's step in the tab, ms.
pub const RETURN_STEP: u32 = 50;
/// The After Write choices (id, title): never Write.
pub const AFTER_WRITE_CHOICES: [(&str, &str); 5] = [
    ("off", "Off"),
    ("read", "Read"),
    ("touch", "Touch"),
    ("latch", "Latch"),
    ("trim", "Trim"),
];
/// The thinning choices (id, title).
pub const THINNING_CHOICES: [(&str, &str); 3] =
    [("low", "Low"), ("medium", "Medium"), ("high", "High")];
/// Under the Thinning picker (Swift `AutomationSettingsView.swift:45`).
pub const THINNING_HELP: &str = "How closely written automation follows the move: 0.1 dB on faders and sends, 0.25 % elsewhere at Medium.";
/// The always-on toggle's label (:48).
pub const PARAMS_ONLY_LABEL: &str = "Write plugin parameters only when touched";
/// Under that toggle (:50).
pub const PARAMS_ONLY_HELP: &str =
    "Always on: writing every parameter of every plugin would bury the tracks.";
/// The tab's last line (:54).
pub const APPLIES_TEXT: &str = "Changes apply from the next Play.";

/// The Return time stepper's label.
pub fn return_text(ms: u32) -> String {
    format!("Return time: {ms} ms")
}

/// The settings, with Swift's fields, defaults and JSON names.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AutomationSettings {
    /// "off", "read", "touch", "latch" or "trim" (not "write").
    pub after_write: String,
    pub return_ms: u32,
    /// "low", "medium" or "high".
    pub thinning: String,
}

impl Default for AutomationSettings {
    fn default() -> Self {
        AutomationSettings {
            after_write: "latch".into(),
            return_ms: 250,
            thinning: "medium".into(),
        }
    }
}

fn field<T: serde::de::DeserializeOwned>(v: &Value, key: &str) -> Option<T> {
    v.get(key)
        .and_then(|x| serde_json::from_value(x.clone()).ok())
}

fn is_choice(choices: &[(&str, &str)], id: &str) -> bool {
    choices.iter().any(|(i, _)| *i == id)
}

impl AutomationSettings {
    /// The settings in `v`; each damaged field keeps its default and the
    /// others are read (a value that is no object is all defaults).
    pub fn from_value(v: &Value) -> Self {
        let d = AutomationSettings::default();
        AutomationSettings {
            after_write: field(v, "afterWrite")
                .filter(|a: &String| is_choice(&AFTER_WRITE_CHOICES, a))
                .unwrap_or(d.after_write),
            return_ms: field(v, "returnMs")
                .filter(|m| *m <= MAX_RETURN_MS)
                .unwrap_or(d.return_ms),
            thinning: field(v, "thinning")
                .filter(|t: &String| is_choice(&THINNING_CHOICES, t))
                .unwrap_or(d.thinning),
        }
    }

    /// The settings as the core spells them.
    pub fn row(&self) -> AutomationSettingsRow {
        AutomationSettingsRow {
            after_write: match self.after_write.as_str() {
                "off" => AutomationModeRow::Off,
                "read" => AutomationModeRow::Read,
                "touch" => AutomationModeRow::Touch,
                "trim" => AutomationModeRow::Trim,
                _ => AutomationModeRow::Latch,
            },
            return_ms: self.return_ms,
            thinning: match self.thinning.as_str() {
                "low" => ThinningRow::Low,
                "high" => ThinningRow::High,
                _ => ThinningRow::Medium,
            },
        }
    }
}

impl<'de> Deserialize<'de> for AutomationSettings {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        Ok(AutomationSettings::from_value(&Value::deserialize(d)?))
    }
}

/// The automation settings the app keeps and the last refusal.
#[derive(Debug, Clone, Default)]
pub struct AutomationPrefs {
    settings: AutomationSettings,
    problem: Option<String>,
}

impl AutomationPrefs {
    /// The settings in `prefs`; a missing or damaged value is the
    /// defaults (and is kept as it is until one is set).
    pub fn load(prefs: &Prefs) -> AutomationPrefs {
        AutomationPrefs {
            settings: prefs
                .raw(AUTOMATION_KEY)
                .map(AutomationSettings::from_value)
                .unwrap_or_default(),
            problem: None,
        }
    }

    pub fn settings(&self) -> &AutomationSettings {
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

    /// Hands the settings to the core (at launch, before a document
    /// opens); the core's refusal, if any, is the answer.
    pub fn apply_all(&self, core: &dyn CoreCalls) -> Option<String> {
        core.set_automation_settings(self.settings.row())
            .err()
            .map(|e| error_message(&e))
    }

    /// Core first; kept and saved only once it took `s`. A failed write
    /// is the `problem` (and the error); the core keeps the value for
    /// this run.
    fn change(
        &mut self,
        s: AutomationSettings,
        prefs: &mut Prefs,
        core: &dyn CoreCalls,
    ) -> Result<(), String> {
        if let Err(e) = core.set_automation_settings(s.row()) {
            return Err(self.refused(e));
        }
        self.settings = s;
        self.problem = None;
        let saved = serde_json::to_value(&self.settings)
            .map_err(|e| e.to_string())
            .and_then(|v| prefs.set_raw(AUTOMATION_KEY, v));
        if let Err(e) = &saved {
            self.problem = Some(e.clone());
        }
        saved
    }

    fn refused(&mut self, e: VizError) -> String {
        let m = error_message(&e);
        self.problem = Some(m.clone());
        m
    }

    /// "off", "read", "touch", "latch" or "trim"; any other name
    /// (Write included) changes nothing.
    pub fn set_after_write(
        &mut self,
        mode: &str,
        prefs: &mut Prefs,
        core: &dyn CoreCalls,
    ) -> Result<(), String> {
        if !is_choice(&AFTER_WRITE_CHOICES, mode) {
            return Ok(());
        }
        let mut s = self.settings.clone();
        s.after_write = mode.into();
        self.change(s, prefs, core)
    }

    /// The return time (ms), clamped to 0-2000.
    pub fn set_return_ms(
        &mut self,
        ms: u32,
        prefs: &mut Prefs,
        core: &dyn CoreCalls,
    ) -> Result<(), String> {
        let mut s = self.settings.clone();
        s.return_ms = ms.min(MAX_RETURN_MS);
        self.change(s, prefs, core)
    }

    /// "low", "medium" or "high"; any other name changes nothing.
    pub fn set_thinning(
        &mut self,
        thinning: &str,
        prefs: &mut Prefs,
        core: &dyn CoreCalls,
    ) -> Result<(), String> {
        if !is_choice(&THINNING_CHOICES, thinning) {
            return Ok(());
        }
        let mut s = self.settings.clone();
        s.thinning = thinning.into();
        self.change(s, prefs, core)
    }
}
