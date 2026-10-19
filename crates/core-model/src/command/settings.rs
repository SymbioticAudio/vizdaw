//! Document-settings commands.

use super::Report;
use super::media::{MediaSwap, swap_media};
use crate::document::Document;
use crate::error::{ModelError, Result};
use crate::format::AudioFormat;
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
}

impl SettingsCmd {
    /// Stable command name.
    pub fn name(&self) -> &'static str {
        match self {
            SettingsCmd::SetTemplate { .. } => "Set Modify Template",
            SettingsCmd::SetHistoryDepth { .. } => "Set Undo Depth",
            SettingsCmd::SetAutosaveKeep { .. } => "Set Autosave Count",
            SettingsCmd::SetFormat { .. } => "Change Project Format",
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
        SettingsCmd::SetFormat { .. } => {}
    }
    Ok(())
}
