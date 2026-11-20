//! The metronome through a session (groups-vcas-transport Task 10,
//! Ruling 19): its settings are the project's `Settings.click`, each
//! change one undo step ("Click On", "Click Off", "Click Level", "Click
//! Mode") that marks the song unsaved. The host sends them to the live
//! engine after every load ([`click_spec`]); a bounce and every export
//! build their own engine and never send them, so nothing clicks there.

use super::Session;
use crate::control::ControlArg;
use crate::error::VizError;
use crate::guard::ffi_guard;
use crate::snapshot::DocSnapshot;
use core_control::registry::{self, ArgValue};
use core_engine::metronome::ClickSpec;
use core_model::command::{Command, SettingsCmd};
use core_model::document::{CLICK_LEVEL_MAX_DB, CLICK_LEVEL_MIN_DB, ClickMode, ClickSettings};

/// When the metronome clicks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum ClickModeRow {
    /// Every beat while the transport plays.
    PlayAndRecord,
    /// Only while a record pass runs, its pre-roll included.
    RecordOnly,
}

/// The project's metronome settings.
#[derive(Debug, Clone, Copy, PartialEq, uniffi::Record)]
pub struct ClickRow {
    /// Whether it clicks.
    pub on: bool,
    /// Its level in dB, −60 to 0.
    pub level_db: f64,
    /// When it clicks.
    pub mode: ClickModeRow,
}

impl From<ClickSettings> for ClickRow {
    fn from(c: ClickSettings) -> Self {
        ClickRow {
            on: c.on,
            level_db: c.level_db,
            mode: match c.mode {
                ClickMode::PlayAndRecord => ClickModeRow::PlayAndRecord,
                ClickMode::RecordOnly => ClickModeRow::RecordOnly,
            },
        }
    }
}

impl From<ClickRow> for ClickSettings {
    fn from(c: ClickRow) -> Self {
        ClickSettings {
            on: c.on,
            level_db: c.level_db,
            mode: match c.mode {
                ClickModeRow::PlayAndRecord => ClickMode::PlayAndRecord,
                ClickModeRow::RecordOnly => ClickMode::RecordOnly,
            },
        }
    }
}

/// The mode as `transport.set_click_mode` and `state.get transport`
/// spell it.
pub fn mode_text(m: ClickModeRow) -> &'static str {
    match m {
        ClickModeRow::PlayAndRecord => "play_and_record",
        ClickModeRow::RecordOnly => "record_only",
    }
}

/// What the live engine is sent for `c`.
pub fn click_spec(c: &ClickSettings) -> ClickSpec {
    let gain = 10f64.powf(c.level_db.clamp(CLICK_LEVEL_MIN_DB, CLICK_LEVEL_MAX_DB) / 20.0);
    ClickSpec {
        on: c.on && c.level_db.is_finite(),
        gain: gain as f32,
        record_only: c.mode == ClickMode::RecordOnly,
    }
}

/// The three click commands, in registry order.
pub const CLICK_IDS: &[&str] = &[
    "transport.click",
    "transport.set_click_level",
    "transport.set_click_mode",
];

fn model(message: impl Into<String>) -> VizError {
    VizError::Model {
        message: message.into(),
    }
}

/// The metronome (Ruling 19).
#[uniffi::export]
impl Session {
    /// The project's metronome settings.
    pub fn click(&self) -> Result<ClickRow, VizError> {
        ffi_guard(|| Ok(self.lock_inner().doc().settings().click.into()))
    }

    /// Replaces the metronome settings, one undo step named after what
    /// changed; the song is unsaved after it. `Model` for a level that is
    /// not finite or not within −60..=0 dB; the same settings change
    /// nothing.
    pub fn set_click(&self, click: ClickRow) -> Result<DocSnapshot, VizError> {
        ffi_guard(|| {
            let click: ClickSettings = click.into();
            click.check().map_err(|e| model(e.to_string()))?;
            self.execute(Command::Settings(SettingsCmd::SetClick { click }))
        })
    }

    /// Runs `transport.click {on?}` (absent flips it),
    /// `transport.set_click_level {level_db}` or
    /// `transport.set_click_mode {mode}` with `args` as the channel gives
    /// them: validated by the registry (an unknown or doubled argument, a
    /// level that is not a plain decimal from −60 to 0, a mode not in
    /// `CLICK_MODES`, a non-flag `on` are each `Model`, changing nothing),
    /// then [`Session::set_click`].
    pub fn run_click(&self, id: String, args: Vec<ControlArg>) -> Result<DocSnapshot, VizError> {
        ffi_guard(|| {
            if !CLICK_IDS.contains(&id.as_str()) {
                let shown: String = id.chars().take(64).collect();
                return Err(model(format!("{shown:?} is not a click command.")));
            }
            let args = crate::co_producer::validated(&id, &args)?;
            let mut click = self.lock_inner().doc().settings().click;
            let arg = |name: &str| args.iter().find(|(n, _)| n == name).map(|(_, v)| v);
            match id.as_str() {
                "transport.click" => {
                    click.on = match arg("on") {
                        Some(ArgValue::Flag(on)) => *on,
                        _ => !click.on,
                    };
                }
                "transport.set_click_level" => {
                    let Some(ArgValue::Text(t)) = arg("level_db") else {
                        return Err(model("transport.set_click_level: give `level_db`"));
                    };
                    let (min, max) = registry::CLICK_LEVEL_DB;
                    click.level_db = registry::decimal(t, min, max)
                        .map_err(|m| model(format!("`level_db` {m}")))?;
                }
                _ => {
                    click.mode = match arg("mode") {
                        Some(ArgValue::Text(t)) if t == "play_and_record" => {
                            ClickMode::PlayAndRecord
                        }
                        Some(ArgValue::Text(t)) if t == "record_only" => ClickMode::RecordOnly,
                        _ => {
                            return Err(model("`mode` is play_and_record or record_only"));
                        }
                    };
                }
            }
            self.set_click(click.into())
        })
    }
}
