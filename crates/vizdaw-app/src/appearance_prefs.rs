//! Settings > Appearance (Swift `AppearancePreferences` and
//! `AppearanceCommands`): the theme and the channel colour bars. A
//! preference of this machine under the key `appearance`, never saved in
//! a project. Decoding is tolerant per field: a missing or damaged field
//! keeps its default. A saved id that names no theme is kept as it is
//! (nothing is written until a choice is made) and the app draws Classic
//! Light (`AppearancePreferences.swift:28-32`).

use serde_json::{Value, json};

use core_control::registry::ArgValue;
use core_control::{ControlError, ErrorCode};

use crate::commands::{Ran, check_names};
use crate::prefs::Prefs;
use crate::theme::{self, DEFAULT_THEME_ID, Theme};

/// The preferences key (Swift "VizDAW.appearance", without the prefix).
pub const APPEARANCE_KEY: &str = "appearance";

/// The saved settings, over `Prefs`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppearancePrefs {
    /// The theme id as saved, even when it names no theme now.
    pub theme: String,
    pub colour_bars: bool,
    /// Why the last write failed (`None` after one that took).
    pub problem: Option<String>,
}

impl Default for AppearancePrefs {
    fn default() -> Self {
        AppearancePrefs {
            theme: DEFAULT_THEME_ID.into(),
            colour_bars: true,
            problem: None,
        }
    }
}

impl AppearancePrefs {
    /// The preference in `prefs`; each missing or damaged field keeps its
    /// default, and nothing is rewritten.
    pub fn load(prefs: &Prefs) -> AppearancePrefs {
        let d = AppearancePrefs::default();
        let Some(v) = prefs.raw(APPEARANCE_KEY) else {
            return d;
        };
        AppearancePrefs {
            theme: v
                .get("theme")
                .and_then(Value::as_str)
                .map(str::to_owned)
                .unwrap_or(d.theme),
            colour_bars: v
                .get("colourBars")
                .and_then(Value::as_bool)
                .unwrap_or(d.colour_bars),
            problem: None,
        }
    }

    /// The theme to draw: the saved one, or Classic Light for an id that
    /// names no theme.
    pub fn resolved(&self) -> &'static Theme {
        theme::theme_or_default(&self.theme)
    }

    /// Makes `id` the theme and saves it. An id that names no theme is
    /// refused and nothing changes. A failed write still changes the
    /// theme for this run and is the `problem`.
    pub fn set_theme(&mut self, id: &str, prefs: &mut Prefs) -> Result<(), String> {
        if theme::theme(id).is_none() {
            return Err(not_a_theme(id));
        }
        self.theme = id.to_owned();
        self.save(prefs);
        Ok(())
    }

    pub fn set_colour_bars(&mut self, on: bool, prefs: &mut Prefs) {
        self.colour_bars = on;
        self.save(prefs);
    }

    fn save(&mut self, prefs: &mut Prefs) {
        self.problem = prefs
            .set_raw(
                APPEARANCE_KEY,
                json!({ "theme": self.theme, "colourBars": self.colour_bars }),
            )
            .err();
    }
}

/// Swift's refusal text (`AppearanceCommands.swift:15`).
pub fn not_a_theme(id: &str) -> String {
    let id: String = id.chars().take(80).collect();
    format!("{id} is not a theme.")
}

/// `ui.theme {theme}`: sets the theme. App-wide: it needs no document and
/// opens nothing. A missing or non-text `theme`, an extra parameter or an
/// id that names no theme is `invalid_params` and changes nothing.
pub fn run_ui_theme(
    args: &[(String, ArgValue)],
    appearance: &mut AppearancePrefs,
    prefs: &mut Prefs,
) -> Result<Ran, ControlError> {
    let id = "ui.theme";
    check_names(id, args)?;
    let Some((_, ArgValue::Text(theme))) = args.iter().find(|(n, _)| n == "theme") else {
        return Err(ControlError::params(id, "`theme` names the theme"));
    };
    appearance
        .set_theme(theme, prefs)
        .map_err(|m| ControlError::new(ErrorCode::InvalidParams, m))?;
    Ok(Ran {
        undo: None,
        status: Some(format!("theme {theme}")),
        focus: None,
        open_mixer_window: false,
        open_modify_panel: false,
    })
}
