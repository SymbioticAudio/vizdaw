//! Nudge Earlier, Later, Up and Down (Swift `M/Nudge.swift:140-178`): the
//! selected events move one time step or one pitch step, one undo step.
//! The steps are the app's [`NudgeSettings`].

use vizdaw_ffi::edit::{EditCommand, EventEdit};

use crate::document::DocumentModel;
use crate::editing_prefs::NudgeSettings;

/// Which way a nudge moves the selection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NudgeDirection {
    Earlier,
    Later,
    Up,
    Down,
}

impl NudgeDirection {
    /// The direction command `id` nudges (`editors.nudge_*`).
    pub fn of_command(id: &str) -> Option<NudgeDirection> {
        Some(match id {
            "editors.nudge_earlier" => NudgeDirection::Earlier,
            "editors.nudge_later" => NudgeDirection::Later,
            "editors.nudge_up" => NudgeDirection::Up,
            "editors.nudge_down" => NudgeDirection::Down,
            _ => return None,
        })
    }
}

impl DocumentModel {
    /// Where a nudge reads its grid and key: the selection's range start,
    /// else its earliest selected event among the selection's tracks, else
    /// tick 0, as `(seq, tick)`; `None` without selected tracks
    /// (`Nudge.swift:140-150`).
    pub fn nudge_anchor(&self) -> Option<(u64, i64)> {
        let s = self.selection();
        let first = s.lanes.first()?;
        if let Some(r) = s.time_range {
            return Some((first.seq, r.start));
        }
        let earliest = s
            .lanes
            .iter()
            .flat_map(|l| {
                self.lane_events(l.seq, l.lane)
                    .iter()
                    .filter(|e| self.is_event_selected(e.id))
                    .map(|e| e.tick)
                    .collect::<Vec<_>>()
            })
            .min();
        Some((first.seq, earliest.unwrap_or(0)))
    }

    /// Nudges the selected events one step `dir` (one undo step). False
    /// when nothing is selected or the edit was refused (the refusal is
    /// the status).
    pub fn nudge(&mut self, dir: NudgeDirection, settings: &NudgeSettings) -> bool {
        let events = self.selection().events.clone();
        if events.is_empty() {
            return false;
        }
        let anchor = self.nudge_anchor();
        let meter = anchor
            .and_then(|(seq, _)| self.sequence_info(seq))
            .map(|i| i.meter.clone())
            .unwrap_or_default();
        let tick = anchor.map_or(0, |(_, t)| t);
        let sign: i8 = match dir {
            NudgeDirection::Later | NudgeDirection::Up => 1,
            NudgeDirection::Earlier | NudgeDirection::Down => -1,
        };
        let edit = match dir {
            NudgeDirection::Earlier | NudgeDirection::Later => {
                let t = settings.ticks(tick, &meter, &self.layout().snap);
                EventEdit::Nudge {
                    events,
                    ticks: i64::from(sign) * t,
                    semitones: 0,
                }
            }
            NudgeDirection::Up | NudgeDirection::Down => {
                let n = sign.saturating_mul(settings.amount.clamp(1, i8::MAX as u8) as i8);
                if settings.pitch_scale_steps {
                    let (root, mask) = settings.key(tick, &meter);
                    EventEdit::TransposeInScale {
                        events,
                        steps: n,
                        root,
                        mask,
                    }
                } else {
                    EventEdit::Nudge {
                        events,
                        ticks: 0,
                        semitones: n,
                    }
                }
            }
        };
        self.edit(EditCommand::Event { edit })
    }

    /// The app's nudge steps the nudge commands use (handed by the UI).
    pub fn nudge_settings(&self) -> &NudgeSettings {
        &self.nudge_settings
    }

    /// Hands the app's nudge steps to the document before a command runs.
    pub fn set_nudge_settings(&mut self, settings: NudgeSettings) {
        self.nudge_settings = settings;
    }
}
