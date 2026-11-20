//! Document-settings commands.

use super::Report;
use super::media::{MediaSwap, swap_media};
use crate::automation::AutomationTarget;
use crate::document::{ClickSettings, Document, SongInfo};
use crate::error::{ModelError, Result};
use crate::format::AudioFormat;
use crate::learn::{self, LearnMap, LearnSource};
use crate::modify::ModifyTemplate;
use std::collections::BTreeSet;

/// Settings edits.
#[derive(Debug, Clone, PartialEq)]
pub enum SettingsCmd {
    /// Store (Some) or delete (None) a named select-and-modify template.
    SetTemplate {
        /// Template name.
        name: String,
        /// The template.
        template: Option<ModifyTemplate>,
    },
    /// Undo depth limit (>= 1).
    SetHistoryDepth {
        /// Steps kept.
        depth: u32,
    },
    /// Autosave copies kept (>= 1).
    SetAutosaveKeep {
        /// Copies kept.
        keep: u32,
    },
    /// Change the project's audio format (sub-project 5 spec §4.1): every
    /// media item gets a new file in the new format and its regions are
    /// rescaled. A project with media must list every item once; the
    /// same format is refused (that is `MediaCmd::Convert`).
    SetFormat {
        /// The new format (an allowed rate).
        format: AudioFormat,
        /// One new file per media item.
        swaps: Vec<MediaSwap>,
    },
    /// Add a MIDI-learn map, or replace the map with the same source
    /// (plan 5f-1 Ruling 6). The target must exist and cannot be a
    /// velocity scale; at most
    /// [`MAX_LEARN_MAPS`](crate::learn::MAX_LEARN_MAPS) maps.
    SetLearn {
        /// The map.
        map: LearnMap,
    },
    /// Remove the map with `source`, or every map to `target` (exactly
    /// one of the two; refused when nothing matches).
    RemoveLearn {
        /// That map's source.
        source: Option<LearnSource>,
        /// Every map to this target.
        target: Option<AutomationTarget>,
    },
    /// Replace the song info (plan 6a). Each field is trimmed (empty
    /// becomes unset), at most
    /// [`MAX_INFO_BYTES`](crate::document::MAX_INFO_BYTES) bytes and free of
    /// control characters other than newline and tab.
    SetInfo {
        /// The new song info.
        info: SongInfo,
    },
    /// Replace the metronome settings (groups-vcas-transport Ruling 19).
    /// The level must be finite and within
    /// [`CLICK_LEVEL_MIN_DB`](crate::document::CLICK_LEVEL_MIN_DB)..=
    /// [`CLICK_LEVEL_MAX_DB`](crate::document::CLICK_LEVEL_MAX_DB). Its undo
    /// label names what changed ([`SettingsCmd::label`]).
    SetClick {
        /// The new settings.
        click: ClickSettings,
    },
}

impl SettingsCmd {
    /// Stable command name.
    pub fn name(&self) -> &'static str {
        match self {
            SettingsCmd::SetTemplate { .. } => "Set Modify Template",
            SettingsCmd::SetHistoryDepth { .. } => "Set Undo Depth",
            SettingsCmd::SetAutosaveKeep { .. } => "Set Autosave Count",
            SettingsCmd::SetFormat { .. } => "Change Project Format",
            SettingsCmd::SetLearn { .. } => "MIDI Learn",
            SettingsCmd::RemoveLearn { .. } => "Remove MIDI Learn",
            SettingsCmd::SetInfo { .. } => "Set Song Info",
            SettingsCmd::SetClick { .. } => "Click",
        }
    }

    /// The undo label of this command applied to `before`: its
    /// [`SettingsCmd::name`], but for `SetClick` what it changes: "Click
    /// On" or "Click Off" when it switches the click, else "Click Level"
    /// or "Click Mode" (Ruling 19).
    pub fn label(&self, before: &Document) -> &'static str {
        let SettingsCmd::SetClick { click } = self else {
            return self.name();
        };
        let was = before.settings().click;
        if click.on != was.on {
            if click.on { "Click On" } else { "Click Off" }
        } else if click.level_db != was.level_db {
            "Click Level"
        } else if click.mode != was.mode {
            "Click Mode"
        } else {
            self.name()
        }
    }
}

fn set_format(d: &mut Document, format: AudioFormat, swaps: &[MediaSwap]) -> Result<()> {
    if !format.is_valid() {
        return Err(ModelError::range("format.sample_rate", format.sample_rate));
    }
    if format == d.settings().format {
        return Err(ModelError::invalid("the project is already in that format"));
    }
    let items: BTreeSet<_> = d.media().keys().copied().collect();
    let listed: BTreeSet<_> = swaps.iter().map(|s| s.media).collect();
    if listed != items || swaps.len() != items.len() {
        return Err(ModelError::invalid(format!(
            "a format change converts every media item once ({} given for {} items)",
            swaps.len(),
            items.len()
        )));
    }
    swap_media(d, swaps, format.sample_rate)?;
    d.settings_mut().format = format;
    Ok(())
}

pub(crate) fn apply(d: &mut Document, cmd: &SettingsCmd, _r: &mut Report) -> Result<()> {
    if let SettingsCmd::SetFormat { format, swaps } = cmd {
        return set_format(d, *format, swaps);
    }
    if let SettingsCmd::SetLearn { map } = cmd {
        return learn::set(d, map);
    }
    if let SettingsCmd::RemoveLearn { source, target } = cmd {
        return learn::remove(d, source.as_ref(), *target);
    }
    if let SettingsCmd::SetInfo { info } = cmd {
        let mut info = info.clone();
        for f in info.fields_mut() {
            *f = f
                .take()
                .map(|t| t.trim().to_owned())
                .filter(|t| !t.is_empty());
        }
        crate::rules::check_info(&info)?;
        d.settings_mut().info = info;
        return Ok(());
    }
    if let SettingsCmd::SetClick { click } = cmd {
        click.check()?;
        d.settings_mut().click = *click;
        return Ok(());
    }
    let s = d.settings_mut();
    match cmd {
        SettingsCmd::SetTemplate { name, template } => match template {
            Some(t) => {
                s.templates.insert(name.clone(), t.clone());
            }
            None => {
                s.templates.remove(name);
            }
        },
        SettingsCmd::SetHistoryDepth { depth } => {
            if *depth == 0 {
                return Err(ModelError::range("history_depth", depth));
            }
            s.history_depth = *depth;
        }
        SettingsCmd::SetAutosaveKeep { keep } => {
            if *keep == 0 {
                return Err(ModelError::range("autosave_keep", keep));
            }
            s.autosave_keep = *keep;
        }
        // Handled above.
        SettingsCmd::SetFormat { .. }
        | SettingsCmd::SetLearn { .. }
        | SettingsCmd::RemoveLearn { .. }
        | SettingsCmd::SetInfo { .. }
        | SettingsCmd::SetClick { .. } => {}
    }
    Ok(())
}
