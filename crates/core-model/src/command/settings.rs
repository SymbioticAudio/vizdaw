//! Document-settings commands.

use super::Report;
use crate::document::Document;
use crate::error::{ModelError, Result};
use crate::modify::ModifyTemplate;

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
}

impl SettingsCmd {
    /// Stable command name.
    pub fn name(&self) -> &'static str {
        match self {
            SettingsCmd::SetTemplate { .. } => "Set Modify Template",
            SettingsCmd::SetHistoryDepth { .. } => "Set Undo Depth",
            SettingsCmd::SetAutosaveKeep { .. } => "Set Autosave Count",
        }
    }
}

pub(crate) fn apply(d: &mut Document, cmd: &SettingsCmd, _r: &mut Report) -> Result<()> {
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
    }
    Ok(())
}
