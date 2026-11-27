//! What the app does with surface events (Swift `SurfaceRelay`, plan 5f-1
//! Ruling 12; `M/SurfaceRelay.swift:40-128`): commands run, selection and
//! arm change the document, jog wheels locate.
//!
//! The relay turns a drained batch into ordered [`RelayStep`]s; the
//! document applies them ([`DocumentModel::apply_surface_step`]) and hands
//! the commands to its window. GPUI has no nested run loop, so Swift's
//! queue for events arriving while one applies is not ported: one batch
//! runs per tick, in order.
//!
//! [`DocumentModel::apply_surface_step`]: crate::DocumentModel::apply_surface_step

use std::time::{Duration, Instant};

use vizdaw_ffi::surfaces::SurfaceEventRow;

/// How long jog detents are summed into one locate (`:8`): each locate
/// lands a writing pass, so a spun wheel is one locate (pre-flight M3).
pub const JOG_WINDOW: Duration = Duration::from_millis(150);

/// Commands that can open a dialog (`:23-27`): they run without the
/// keyboard bypass, which would otherwise hold for as long as the dialog
/// is open (final review R3). They have no keyboard guard to bypass.
pub const DIALOG_COMMANDS: &[&str] = &[
    "file.open",
    "file.close",
    "file.save",
    "file.save_as",
    "file.project_settings",
    "keys.save",
    "keys.load",
    "media.import",
    "media.relink",
    "media.rename",
    "record.recover",
    "region.normalize",
    "file.bounce",
];

/// One thing a surface asked of its document, in the order asked.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RelayStep {
    /// A registry command; `programmatic`: caused by `surface.inject` or
    /// `midi.inject`.
    Command { id: String, programmatic: bool },
    /// Select this strip.
    Select(u64),
    /// Toggle the arm of the input track of the strip's path (`None`: a
    /// strip that is no AudioPath, which does nothing).
    Arm(Option<u64>),
    /// Toggle the record count-in (plan 5f-4 Ruling 16).
    CountIn,
    /// Locate by these detents from where the transport stands.
    Locate(i64),
}

/// A command a surface asked for, for the document's window to run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SurfaceCommand {
    pub id: String,
    /// Caused by the control channel: it runs as `command.run` does and
    /// never opens a dialog (pre-flight I9).
    pub programmatic: bool,
    /// A device's button that runs while a text field has the keyboard
    /// (ruling T9-1); never for a command that can open a dialog.
    pub bypass: bool,
}

impl SurfaceCommand {
    pub fn new(id: &str, programmatic: bool) -> Self {
        SurfaceCommand {
            id: id.to_owned(),
            programmatic,
            bypass: !programmatic && !DIALOG_COMMANDS.contains(&id),
        }
    }
}

/// The jog detents not yet located by, and when the first of them came.
#[derive(Debug, Default)]
pub struct SurfaceRelay {
    detents: i64,
    since: Option<Instant>,
}

impl SurfaceRelay {
    /// The steps for `events`, in order. Detents waiting for the window
    /// are located before any other event (`:61-64`), so a Play or Record
    /// right after a spin starts from the jogged place (review I1).
    pub fn apply(&mut self, events: Vec<SurfaceEventRow>, now: Instant) -> Vec<RelayStep> {
        let mut steps = vec![];
        for e in events {
            if !matches!(e, SurfaceEventRow::Jog { .. })
                && let Some(l) = self.take_pending()
            {
                steps.push(l);
            }
            match e {
                SurfaceEventRow::Command { id, programmatic } => {
                    steps.push(RelayStep::Command { id, programmatic });
                }
                SurfaceEventRow::Select { strip } => steps.push(RelayStep::Select(strip)),
                SurfaceEventRow::Arm { path, .. } => steps.push(RelayStep::Arm(path)),
                SurfaceEventRow::CountIn => steps.push(RelayStep::CountIn),
                SurfaceEventRow::Jog { detents } => {
                    self.since.get_or_insert(now);
                    self.detents += i64::from(detents);
                }
            }
        }
        steps
    }

    /// A locate by the summed detents once [`JOG_WINDOW`] passed since the
    /// first one not yet located by (`:100-103`).
    pub fn flush(&mut self, now: Instant) -> Option<RelayStep> {
        let since = self.since?;
        if now.saturating_duration_since(since) < JOG_WINDOW {
            return None;
        }
        self.take_pending()
    }

    fn take_pending(&mut self) -> Option<RelayStep> {
        self.since.take()?;
        let detents = std::mem::take(&mut self.detents);
        (detents != 0).then_some(RelayStep::Locate(detents))
    }
}

impl crate::DocumentModel {
    /// The surfaces' part of `idle` (`M/DocumentModel.swift:856-868`):
    /// tells the core the count-in, for a keyboard's Count-in light, once
    /// per change and again after a refusal (final review T7 m3); drains
    /// the session's events into steps and adds the jog's locate once its
    /// window passed. True when steps wait.
    pub(crate) fn idle_surfaces(&mut self, now: Instant) -> bool {
        let count_in = self.record.count_in_bars() > 0;
        if self.sent_count_in != Some(count_in)
            && self.session.set_surface_count_in(count_in).is_ok()
        {
            self.sent_count_in = Some(count_in);
        }
        let before = self.surface_steps.len();
        if let Ok(events) = self.session.take_surface_events()
            && !events.is_empty()
        {
            let steps = self.surface_relay.apply(events, now);
            self.surface_steps.extend(steps);
        }
        if let Some(l) = self.surface_relay.flush(now) {
            self.surface_steps.push(l);
        }
        self.surface_steps.len() > before
    }

    /// The steps the surfaces asked for since the last call, in order.
    pub fn take_surface_steps(&mut self) -> Vec<RelayStep> {
        std::mem::take(&mut self.surface_steps)
    }

    /// Applies one step to this document; a command is returned for the
    /// window to run (`M/SurfaceRelay.swift:66-89`).
    pub fn apply_surface_step(&mut self, step: RelayStep) -> Option<SurfaceCommand> {
        match step {
            RelayStep::Command { id, programmatic } => {
                return Some(SurfaceCommand::new(&id, programmatic));
            }
            RelayStep::Select(strip) => self.set_selected_strips([strip].into()),
            RelayStep::Arm(path) => self.surface_arm(path),
            RelayStep::CountIn => self.record.toggle_count_in(),
            RelayStep::Locate(detents) => self.surface_locate(detents),
        }
        None
    }

    /// A surface's button is running (ruling T9-1): while set, the keys a
    /// text field holds back act.
    pub fn set_surface_acting(&mut self, on: bool) {
        self.surface_acting = on;
    }

    /// Locates by `detents` sixteenths from where the transport stands
    /// now: a fresh read, since the core may have moved it since the last
    /// poll (review M2), and never before 0 (`:108-121`).
    fn surface_locate(&mut self, detents: i64) {
        let tick = match self.poll_transport() {
            Ok(t) => t.tick,
            Err(_) => self.last_transport().tick,
        };
        let target = (tick + detents * crate::arrangement::PPQ / 4).max(0);
        if let Err(e) = self.move_to(target) {
            self.status = Some(crate::AlertItem::from_error(&e, "Locate Failed").message);
        }
    }

    /// Toggles the arm of the input track of `path`, as the track header
    /// and `MixerModel::toggle_arm` do; no path, sequence or track: nothing.
    fn surface_arm(&mut self, path: Option<u64>) {
        let Some(path) = path else { return };
        let Some(seq) = self.selected_seq() else {
            return;
        };
        let Some((lane, arm)) = self.input_lane(path).map(|l| (l.id, l.arm)) else {
            return;
        };
        self.edit(vizdaw_ffi::edit::EditCommand::Lane {
            edit: vizdaw_ffi::edit::LaneEdit::SetFlags {
                seq,
                lane,
                mute: None,
                solo: None,
                arm: Some(!arm),
            },
        });
    }
}
