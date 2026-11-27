//! What the bar's Click, DSP Load, Audio Device and command buttons show
//! and do, apart from their views (Swift `TransportItemEntries`,
//! `CommandButtons`, `Formatting.device`): the Click menu's entries, the
//! load and device texts, and a command button's face and availability.

use core_control::registry::{self, ArgValue};
use vizdaw_ffi::session::click::ClickModeRow;
use vizdaw_ffi::snapshot::TransportState;

use crate::commands;
use crate::document::DocumentModel;

/// The Click item's title, its toggle's help and its ⌄ (Swift
/// `V/TransportItemViews.swift:325-331`).
pub const CLICK: &str = "Click";
pub const CLICK_HELP: &str = "The metronome on or off";
pub const CLICK_OPTIONS: &str = "Click Options";
pub const CLICK_OPTIONS_HELP: &str = "Click level and mode";
/// What a channel click on Click's menu says (`:370`).
pub const CLICK_ON_CHANNEL: &str = "Click's menu does not open for the control channel; use transport.set_click_level or transport.set_click_mode.";
/// The DSP Load and Audio Device items' titles.
pub const DSP_LOAD: &str = "DSP Load";
pub const AUDIO_DEVICE: &str = "Audio Device";
/// A command button's item title.
pub const COMMAND_BUTTON: &str = "Command Button";
/// A stale command button's tooltip (Swift `CommandButtonAction`).
pub const STALE_TOOLTIP: &str = "This command is no longer in VizDAW";

/// Click's levels in dB, loudest first (Swift `clickLevels`).
pub const CLICK_LEVELS: [i32; 9] = [0, -3, -6, -12, -18, -24, -36, -48, -60];

/// A level as the Level menu shows it: "0 dB", "−12 dB" (U+2212).
pub fn level_title(db: i32) -> String {
    if db < 0 {
        format!("\u{2212}{} dB", -db)
    } else {
        format!("{db} dB")
    }
}

/// A level as `transport.set_click_level` takes it: "-12".
pub fn level_argument(db: i32) -> String {
    db.to_string()
}

/// The Mode menu's titles.
pub fn mode_title(mode: ClickModeRow) -> &'static str {
    match mode {
        ClickModeRow::PlayAndRecord => "Play and Record",
        ClickModeRow::RecordOnly => "Record Only",
    }
}

fn mode_argument(mode: ClickModeRow) -> &'static str {
    match mode {
        ClickModeRow::PlayAndRecord => "play_and_record",
        ClickModeRow::RecordOnly => "record_only",
    }
}

/// What a Click menu entry does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BarAction {
    /// `transport.click`, as its menu item runs it.
    ToggleClick,
    /// `transport.set_click_level` with this level in dB.
    ClickLevel(i32),
    /// `transport.set_click_mode`.
    ClickMode(ClickModeRow),
}

/// One entry of a bar item's menu.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MenuEntry {
    pub title: String,
    /// Shows ✓.
    pub checked: bool,
    pub enabled: bool,
    pub kind: EntryKind,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EntryKind {
    Action(BarAction),
    Submenu(Vec<MenuEntry>),
}

/// Whether the metronome is on (off while the session has not said).
pub fn click_is_on(d: &DocumentModel) -> bool {
    d.last_transport().click.is_some_and(|c| c.on)
}

/// Click's own menu (its right-click and its ⌄): Level › and Mode ›,
/// the current ones checked (a level within 0.05 dB). Disabled while
/// the session has not given the settings.
pub fn click_menu(d: &DocumentModel) -> Vec<MenuEntry> {
    let c = d.last_transport().click;
    let known = c.is_some();
    let levels = CLICK_LEVELS
        .iter()
        .map(|&db| MenuEntry {
            title: level_title(db),
            checked: c.is_some_and(|c| (c.level_db - f64::from(db)).abs() < 0.05),
            enabled: known,
            kind: EntryKind::Action(BarAction::ClickLevel(db)),
        })
        .collect();
    let modes = [ClickModeRow::PlayAndRecord, ClickModeRow::RecordOnly]
        .into_iter()
        .map(|mode| MenuEntry {
            title: mode_title(mode).to_owned(),
            checked: c.is_some_and(|c| c.mode == mode),
            enabled: known,
            kind: EntryKind::Action(BarAction::ClickMode(mode)),
        })
        .collect();
    vec![
        MenuEntry {
            title: "Level".into(),
            checked: false,
            enabled: known,
            kind: EntryKind::Submenu(levels),
        },
        MenuEntry {
            title: "Mode".into(),
            checked: false,
            enabled: known,
            kind: EntryKind::Submenu(modes),
        },
    ]
}

/// Click in »: a submenu `Click` ✓ of the toggle with its check, then
/// Level › and Mode ›.
pub fn click_entry(d: &DocumentModel) -> MenuEntry {
    let on = click_is_on(d);
    let mut entries = vec![MenuEntry {
        title: CLICK.into(),
        checked: on,
        enabled: d.last_transport().click.is_some(),
        kind: EntryKind::Action(BarAction::ToggleClick),
    }];
    entries.extend(click_menu(d));
    MenuEntry {
        title: CLICK.into(),
        checked: on,
        enabled: true,
        kind: EntryKind::Submenu(entries),
    }
}

/// Runs a Click entry; a refusal goes to the status line, as a menu
/// item's does.
pub fn run(action: BarAction, d: &mut DocumentModel) {
    let (id, args) = match action {
        BarAction::ToggleClick => {
            let _ = commands::perform_from_menu("transport.click", d);
            return;
        }
        BarAction::ClickLevel(db) => (
            "transport.set_click_level",
            vec![("level_db".to_owned(), ArgValue::Text(level_argument(db)))],
        ),
        BarAction::ClickMode(mode) => (
            "transport.set_click_mode",
            vec![(
                "mode".to_owned(),
                ArgValue::Text(mode_argument(mode).to_owned()),
            )],
        ),
    };
    if let Err(e) = commands::run(id, &args, d) {
        d.set_status(Some(e.message));
    }
}

/// The DSP load as the item shows it: "DSP 12%" (Swift `dspText`: the
/// load clamped to 0…99.99, one that is not a number 0).
pub fn dsp_text(load: f32) -> String {
    let pct = if load.is_finite() {
        load.clamp(0.0, 99.99) * 100.0
    } else {
        0.0
    };
    format!("DSP {pct:.0}%")
}

/// `<device> · 48 kHz`, or `No output` before the first Play (Swift
/// `Formatting.device`; the rate text is the banners').
pub fn device_text(t: &TransportState) -> String {
    if t.device.is_empty() {
        return "No output".into();
    }
    let rate = if t.sample_rate > 0.0 && t.sample_rate.is_finite() {
        crate::banners::khz(t.sample_rate)
    } else {
        String::new()
    };
    format!("{} \u{b7} {rate}", t.device)
}

/// The commands a button can run with no document (Swift
/// `CommandAvailability.appWide`, those the registry has).
const APP_WIDE: &[&str] = &[
    "file.new",
    "file.create_demo",
    "file.create_demo_audio",
    "file.open",
    "window.plugin_manager",
    "keys.save",
    "keys.load",
    "keys.reset",
    "audio.set",
    "automation.suspend",
    "ui.customize_transport",
    "ui.set_transport_bar",
    "ui.reset_transport_bar",
];

/// Whether a command button for `id` can run now: as its menu item is
/// enabled (File items as the File menu enables them, the rest as the
/// command can run on `doc`); with no document only an app-wide command.
pub fn command_enabled(id: &str, doc: Option<&DocumentModel>) -> bool {
    let Some(d) = doc else {
        return APP_WIDE.contains(&id);
    };
    // Needs no document, so it can always run.
    if id == "window.plugin_manager" {
        return true;
    }
    if commands::FILE_COMMANDS.contains(&id) {
        return commands::file_enabled(id, Some(d));
    }
    commands::enabled(id, d)
}

/// The status line when a button's command cannot run.
pub fn cannot_run(title: &str) -> String {
    format!("{title} can't run from a button right now.")
}

/// What a command button shows (Swift `CommandButtonFace`): its label
/// (the registry's title unless given), its tooltip and accessibility
/// label (the title) and whether it can run. A stale id (no registry
/// row) is greyed with [`STALE_TOOLTIP`], never dropped.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandButtonFace {
    pub label: String,
    pub tooltip: String,
    pub accessibility_label: String,
    pub enabled: bool,
    pub stale: bool,
}

impl CommandButtonFace {
    pub fn make(id: &str, label: Option<&str>, doc: Option<&DocumentModel>) -> CommandButtonFace {
        let Some(row) = registry::find(id) else {
            let label = label.unwrap_or(id).to_owned();
            return CommandButtonFace {
                accessibility_label: label.clone(),
                label,
                tooltip: STALE_TOOLTIP.into(),
                enabled: false,
                stale: true,
            };
        };
        CommandButtonFace {
            label: label.unwrap_or(row.title).to_owned(),
            tooltip: row.title.into(),
            accessibility_label: row.title.into(),
            enabled: command_enabled(id, doc),
            stale: false,
        }
    }
}

/// A command button's click (Swift `CommandButtonAction.run`): true when
/// its menu item's action is to run; otherwise the status line says it
/// cannot (a stale id by its id).
pub fn press_command(id: &str, d: &mut DocumentModel) -> bool {
    let Some(row) = registry::find(id) else {
        d.set_status(Some(cannot_run(id)));
        return false;
    };
    if command_enabled(id, Some(d)) {
        return true;
    }
    d.set_status(Some(cannot_run(row.title)));
    false
}
