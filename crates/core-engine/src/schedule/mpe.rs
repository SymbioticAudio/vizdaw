//! MPE output at compile time (plan 5g-2 Ruling 10, P2): an MPE
//! destination's flattened events are collected as [`LiveMsg`]s, sorted,
//! and run through one [`MpeEncoder`] onto rotating member channels.

use super::compile::{CompileReport, Pending, mark};
use super::targets::{MAX_TARGETS, TargetIdx};
use super::{Entry, WireMsg};
use crate::input_fx::{Expr, LiveMsg, NoteExpr};
use crate::midi1::{Class, Midi1Msg, translate};
use crate::mpe::MpeEncoder;
use core_model::event::{EventBody, PerNoteKind};
use core_model::ids::DestId;
use core_model::mpe::MpeConfig;
use core_model::query::ResolvedEvent;
use core_model::time::Tick;
use std::collections::BTreeMap;

/// One collected event of an MPE destination, encoded after the main loop
/// (plan 5g-2 Ruling 10).
pub(super) struct MpeItem {
    frame: u64,
    class: Class,
    order: u64,
    msg: LiveMsg,
}

/// An MPE destination's notes by `(destination, pitch)`, as `(start, end)`
/// ticks, and the poly pressure at each note's start, which becomes that
/// note's starting pressure (Ruling 10).
#[derive(Default)]
pub(super) struct MpeNotes {
    spans: BTreeMap<(DestId, u8), Vec<(Tick, Tick)>>,
    start_pressure: BTreeMap<(DestId, u8, Tick), u32>,
}

impl MpeNotes {
    /// Scans the flattened events of the destinations `is_mpe` accepts.
    pub(super) fn scan(flat: &[ResolvedEvent], is_mpe: impl Fn(DestId) -> bool) -> MpeNotes {
        let mut n = MpeNotes::default();
        for r in flat {
            let Some(dest) = r.dest.filter(|d| is_mpe(*d)) else {
                continue;
            };
            match &r.body {
                EventBody::Note {
                    pitch, duration, ..
                } => n
                    .spans
                    .entry((dest, pitch & 0x7F))
                    .or_default()
                    .push((r.tick, r.tick + *duration)),
                EventBody::PolyPressure { pitch, value } => {
                    n.start_pressure
                        .insert((dest, pitch & 0x7F, r.tick), *value);
                }
                _ => {}
            }
        }
        n
    }

    fn starts_at(&self, dest: DestId, pitch: u8, tick: Tick) -> bool {
        self.spans
            .get(&(dest, pitch))
            .is_some_and(|v| v.iter().any(|&(s, _)| s == tick))
    }

    /// The start of a note of `pitch` sounding at `tick` (after its start).
    fn inside(&self, dest: DestId, pitch: u8, tick: Tick) -> Option<Tick> {
        self.spans
            .get(&(dest, pitch))?
            .iter()
            .find(|&&(s, e)| s < tick && tick < e)
            .map(|&(s, _)| s)
    }
}

/// Collects one event of an MPE destination (Ruling 10) as [`MpeItem`]s,
/// applying its mute (P2): notes and per-note messages of a muted
/// destination are silenced; channel messages still play.
pub(super) struct Collect<'a, F: Fn(Tick) -> u64> {
    pub(super) dest: DestId,
    pub(super) cfg: MpeConfig,
    pub(super) muted: bool,
    pub(super) tick: Tick,
    pub(super) frame: &'a F,
    pub(super) notes: &'a MpeNotes,
    pub(super) items: &'a mut Vec<MpeItem>,
    pub(super) order: &'a mut u64,
    pub(super) report: &'a mut CompileReport,
    pub(super) played: bool,
    pub(super) silenced: bool,
}

impl<F: Fn(Tick) -> u64> Collect<'_, F> {
    fn push(&mut self, frame: u64, class: Class, msg: LiveMsg) {
        if self.muted && !matches!(msg, LiveMsg::Channel(_)) {
            self.silenced = true;
            return;
        }
        self.items.push(MpeItem {
            frame,
            class,
            order: *self.order,
            msg,
        });
        *self.order += 1;
        self.played = true;
    }

    /// A per-note message at `tick` of a note that starts at frame `on`:
    /// one landing on the note-on's frame sorts after it.
    fn per_note(&mut self, tick: Tick, on: u64, pitch: u8, expr: Expr, value: u32) {
        let f = (self.frame)(tick);
        // Encoded after the note-on, but its bytes (class Control) re-sort
        // ahead of the 0x9n, so the note starts with this later value.
        let class = if f <= on {
            Class::NoteOn
        } else {
            Class::Control
        };
        self.push(f.max(on), class, LiveMsg::PerNote { pitch, expr, value });
    }

    pub(super) fn collect(&mut self, body: &EventBody) {
        let on = (self.frame)(self.tick);
        match body {
            EventBody::Note {
                pitch,
                velocity,
                release_velocity,
                duration,
                attribute,
                per_note,
            } => {
                let pitch = pitch & 0x7F;
                let end = self.tick + *duration;
                // A note-off never sorts at or before its note-on (O20).
                let off = (self.frame)(end).max(on.saturating_add(1));
                self.report.dropped_note_attributes += usize::from(attribute.is_some());
                let mut expr = NoteExpr::default();
                if let Some(p) = self
                    .notes
                    .start_pressure
                    .get(&(self.dest, pitch, self.tick))
                {
                    expr.pressure = *p;
                }
                let mut later: Vec<(Tick, Expr, u32)> = Vec::new();
                for pn in per_note {
                    let kind = match pn.kind {
                        PerNoteKind::PitchBend => Expr::Bend,
                        PerNoteKind::Registered(74) => Expr::Timbre,
                        _ => {
                            self.report.dropped_per_note += 1;
                            continue;
                        }
                    };
                    if pn.offset == Tick::ZERO {
                        match kind {
                            Expr::Bend => expr.bend = pn.value,
                            _ => expr.timbre = pn.value,
                        }
                    } else if pn.offset > Tick::ZERO && pn.offset < *duration {
                        later.push((self.tick + pn.offset, kind, pn.value));
                    } else {
                        self.report.dropped_per_note += 1;
                    }
                }
                self.push(
                    on,
                    Class::NoteOn,
                    LiveMsg::NoteOn {
                        pitch,
                        velocity: (*velocity).max(1),
                        expr,
                    },
                );
                for (t, kind, value) in later {
                    self.per_note(t, on, pitch, kind, value);
                }
                self.push(
                    off,
                    Class::NoteOff,
                    LiveMsg::NoteOff {
                        pitch,
                        velocity: *release_velocity,
                    },
                );
            }
            EventBody::PolyPressure { pitch, .. }
                if self.notes.starts_at(self.dest, pitch & 0x7F, self.tick) =>
            {
                // The note's starting pressure, sent with the note.
                if self.muted {
                    self.silenced = true;
                } else {
                    self.played = true;
                }
            }
            EventBody::PolyPressure { pitch, value } => {
                let pitch = pitch & 0x7F;
                match self.notes.inside(self.dest, pitch, self.tick) {
                    Some(start) => {
                        let start = (self.frame)(start);
                        self.per_note(self.tick, start, pitch, Expr::Pressure, *value);
                    }
                    None => self.channel(body, on),
                }
            }
            _ => self.channel(body, on),
        }
    }

    /// Any other event, translated onto the manager channel. SysEx is
    /// scheduled by the caller.
    fn channel(&mut self, body: &EventBody, on: u64) {
        let t = translate(body, self.cfg.manager());
        self.report.dropped_sysex8 += usize::from(t.drops.sysex8);
        for o in t.msgs() {
            self.push(on, o.class, LiveMsg::Channel(o.msg));
        }
    }
}

/// Runs one MPE destination's `items` through one encoder and schedules
/// the bytes on `targets` in the encoder's order; returns the steals.
pub(super) fn encode(
    cfg: MpeConfig,
    mut items: Vec<MpeItem>,
    targets: &[TargetIdx],
    pending: &mut Vec<Pending>,
    order: &mut u64,
    used_channels: &mut [u16; MAX_TARGETS],
) -> usize {
    items.sort_by_key(|i| (i.frame, i.class, i.order));
    let mut enc = MpeEncoder::new(cfg);
    let mut out: Vec<Midi1Msg> = Vec::new();
    for item in items {
        out.clear();
        enc.encode(item.msg, &mut |m| out.push(m));
        for m in &out {
            // Channel messages keep their class (bank select sorts with its
            // program change); the encoder's own output sorts by its status.
            let class = match item.msg {
                LiveMsg::Channel(_) => item.class,
                _ => Class::of(m.bytes[0]),
            };
            for target in targets {
                pending.push(Pending {
                    frame: item.frame,
                    class,
                    group: 1,
                    order: *order,
                    entry: Entry {
                        frame: item.frame,
                        target: *target,
                        msg: WireMsg::Midi1(*m),
                    },
                });
                *order += 1;
            }
        }
    }
    // The zone's channels are in use: controller resets cover them.
    let channels =
        std::iter::once(cfg.manager()).chain((0..cfg.members).filter_map(|k| cfg.member(k)));
    for ch in channels {
        for target in targets {
            mark(used_channels, *target, ch);
        }
    }
    usize::try_from(enc.take_stolen()).unwrap_or(usize::MAX)
}
