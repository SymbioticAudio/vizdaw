//! MIDI-learn maps whose targets are project objects (sub-project 5 spec
//! §4.8; plan 5f-1 Rulings 6 and 18). They live in `Settings.learn` and are
//! set and removed as undo steps by `SettingsCmd::SetLearn`/`RemoveLearn`.

use crate::automation::{AutomationTarget, target_exists};
use crate::command::event::note_name;
use crate::document::Document;
use crate::error::{ModelError, Result};
use crate::lane::MAX_PORT_NAME;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

/// The most learn maps a project holds (Ruling 6).
pub const MAX_LEARN_MAPS: usize = 512;

/// What a control sends (spec §4.8); channels 0–15.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum LearnKind {
    /// A 7-bit controller.
    Cc7 {
        /// MIDI channel 0–15.
        channel: u8,
        /// Controller 0–127.
        cc: u8,
    },
    /// A 14-bit controller pair: `cc` (0–31) is the MSB, `cc + 32` the LSB.
    Cc14 {
        /// MIDI channel 0–15.
        channel: u8,
        /// The MSB controller, 0–31.
        cc: u8,
    },
    /// Pitch bend.
    PitchBend {
        /// MIDI channel 0–15.
        channel: u8,
    },
    /// A note (on toggles a stepped target, sets a continuous one).
    Note {
        /// MIDI channel 0–15.
        channel: u8,
        /// Note number 0–127.
        note: u8,
    },
}

impl LearnKind {
    /// The kind's MIDI channel, 0–15.
    pub fn channel(self) -> u8 {
        match self {
            LearnKind::Cc7 { channel, .. }
            | LearnKind::Cc14 { channel, .. }
            | LearnKind::PitchBend { channel }
            | LearnKind::Note { channel, .. } => channel,
        }
    }
}

/// A port and what arrives on it.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LearnSource {
    /// The MIDI source port's name.
    pub port: String,
    /// The message.
    pub kind: LearnKind,
}

/// How a CC's values move the target.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LearnMode {
    /// The value is the position.
    #[default]
    Absolute,
    /// Relative detents: 1..=63 up, 65..=127 down (127 is -1).
    Relative2sComplement,
    /// Relative detents around 64 (65 is +1, 63 is -1).
    RelativeBinOffset,
}

/// One learned control and what it drives.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LearnMap {
    /// Where its messages come from.
    pub source: LearnSource,
    /// What it drives.
    pub target: AutomationTarget,
    /// How its values move the target.
    #[serde(default)]
    pub mode: LearnMode,
    /// Ruling 18: on unless switched off; absolute CC, 14-bit and pitch-bend maps only.
    #[serde(default = "pickup_default")]
    pub pickup: bool,
}

/// `true` (Ruling 18).
pub fn pickup_default() -> bool {
    true
}

fn range(field: &'static str, value: impl std::fmt::Display) -> Result<()> {
    Err(ModelError::range(field, value))
}

impl LearnSource {
    /// Ruling 6's checks: the port name is not empty, has no control
    /// characters and is at most 255 bytes; the channel is 0–15; a CC is
    /// 0–127 (0–31 for `Cc14`); a note is 0–127.
    pub fn check(&self) -> Result<()> {
        let p = &self.port;
        if p.is_empty() || p.len() > MAX_PORT_NAME || p.chars().any(char::is_control) {
            return range("learn.port", p.len());
        }
        let ch = self.kind.channel();
        if ch > 15 {
            return range("learn.channel", ch);
        }
        match self.kind {
            LearnKind::Cc7 { cc, .. } if cc > 127 => range("learn.cc", cc),
            LearnKind::Cc14 { cc, .. } if cc > 31 => range("learn.cc", cc),
            LearnKind::Note { note, .. } if note > 127 => range("learn.note", note),
            _ => Ok(()),
        }
    }

    /// "Port · CC 7 ch 1", "Port · CC 1/33 ch 1", "Port · Pitch Bend ch 2", "Port · Note C4 ch 1".
    pub fn describe(&self) -> String {
        let ch = u16::from(self.kind.channel()) + 1;
        let what = match self.kind {
            LearnKind::Cc7 { cc, .. } => format!("CC {cc}"),
            LearnKind::Cc14 { cc, .. } => format!("CC {cc}/{}", u16::from(cc) + 32),
            LearnKind::PitchBend { .. } => "Pitch Bend".to_string(),
            LearnKind::Note { note, .. } => format!("Note {}", note_name(note)),
        };
        format!("{} · {what} ch {ch}", self.port)
    }
}

impl LearnMap {
    /// Whether this map *can* use pick-up, from its kind and mode
    /// (absolute CC, 14-bit or pitch bend). Its `pickup` flag says whether
    /// it does: a map picks up when `pickup && picks_up()`.
    pub fn picks_up(&self) -> bool {
        self.mode == LearnMode::Absolute && !matches!(self.source.kind, LearnKind::Note { .. })
    }

    /// The checks a map passes in a file as well as in a command: the
    /// source's, and a target that is not a velocity scale. (A target that
    /// no longer exists is allowed here: the map stays and does nothing.)
    fn check(&self) -> Result<()> {
        self.source.check()?;
        if matches!(self.target, AutomationTarget::VelocityScale(_)) {
            return Err(ModelError::invalid("velocity scale cannot be learned"));
        }
        Ok(())
    }
}

/// Every problem in a project's learn maps, for `rules::validate`: each
/// map's checks, sources that repeat, and a count past [`MAX_LEARN_MAPS`].
pub(crate) fn problems(maps: &[LearnMap]) -> Vec<String> {
    let mut out = Vec::new();
    if maps.len() > MAX_LEARN_MAPS {
        out.push(format!(
            "settings.learn: {} maps (at most {MAX_LEARN_MAPS})",
            maps.len()
        ));
    }
    let mut seen = BTreeSet::new();
    for m in maps {
        if let Err(e) = m.check() {
            out.push(format!("settings.learn: {e}"));
        }
        if !seen.insert(&m.source) {
            out.push(format!(
                "settings.learn: two maps for {}",
                m.source.describe()
            ));
        }
    }
    out
}

/// `SettingsCmd::SetLearn`: adds `map`, or replaces the map with its source.
pub(crate) fn set(d: &mut Document, map: &LearnMap) -> Result<()> {
    map.check()?;
    if !target_exists(d, map.target) {
        return Err(ModelError::invalid("the learn target does not exist"));
    }
    let learn = &mut d.settings_mut().learn;
    if let Some(old) = learn.iter_mut().find(|m| m.source == map.source) {
        *old = map.clone();
    } else {
        if learn.len() >= MAX_LEARN_MAPS {
            return Err(ModelError::invalid(format!(
                "a project holds at most {MAX_LEARN_MAPS} learn maps"
            )));
        }
        learn.push(map.clone());
    }
    Ok(())
}

/// `SettingsCmd::RemoveLearn`: exactly one of `source` (that map) and
/// `target` (every map to it); a removal that matches nothing is refused.
pub(crate) fn remove(
    d: &mut Document,
    source: Option<&LearnSource>,
    target: Option<AutomationTarget>,
) -> Result<()> {
    if source.is_some() == target.is_some() {
        return Err(ModelError::invalid(
            "remove learn takes exactly one of source and target",
        ));
    }
    let learn = &mut d.settings_mut().learn;
    let before = learn.len();
    learn.retain(|m| source != Some(&m.source) && target != Some(m.target));
    if learn.len() == before {
        return Err(ModelError::invalid("no learn map matches"));
    }
    Ok(())
}
