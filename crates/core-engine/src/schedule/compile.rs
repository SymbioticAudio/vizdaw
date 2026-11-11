//! The schedule compiler (non-RT): flatten → route → time → translate →
//! sort.

use super::automation::{AutoDest, Automation, MAX_AUTO_LANES};
use super::targets::{BuiltinRoute, TargetIdx, TargetKind, kinds_for};
use super::tempo::TempoTable;
use super::{ChaseIndex, Entry, MAX_TARGETS, Schedule, WireMsg};
use crate::backend::MidiPortInfo;
use crate::error::EngineError;
use crate::input_fx::{Expr, LiveMsg, NoteExpr};
use crate::midi1::{Class, Midi1Msg, When, translate};
use crate::mpe::{MpeEncoder, configuration};
use core_model::automation::{
    AutomationMode, AutomationTarget, strip_of, target_exists, velocity_scale_percent,
};
use core_model::document::{DestKind, Document, SlotOwner};
use core_model::event::{EventBody, PerNoteKind};
use core_model::ids::{DestId, MediaId, PluginInstanceId, SeqId};
use core_model::mpe::MpeConfig;
use core_model::query::{PathStep, flatten, seq_length};
use core_model::selection::ViewFilter;
use core_model::time::Tick;
use std::collections::{BTreeMap, BTreeSet};
use std::time::{Duration, Instant};

/// Compiler inputs besides the document.
#[derive(Debug, Clone, PartialEq)]
pub struct CompileOptions {
    /// Output sample rate.
    pub sample_rate: f64,
    /// Where `builtin:synth` destinations play.
    pub route: BuiltinRoute,
    /// MIDI destinations that currently exist.
    pub ports: Vec<MidiPortInfo>,
    /// Media that cannot be read: their regions play silence (5a).
    pub offline_media: BTreeSet<MediaId>,
    /// Automation is suspended (spec §7.5): Off strips play theirs too
    /// (plan 5e-1 Ruling 3).
    pub suspend_automation: bool,
}

impl CompileOptions {
    /// Built-in routing, no MIDI destinations.
    pub fn new(sample_rate: f64) -> CompileOptions {
        CompileOptions {
            sample_rate,
            route: BuiltinRoute::Builtin,
            ports: Vec::new(),
            offline_media: BTreeSet::new(),
            suspend_automation: false,
        }
    }
}

/// An event that was not played, with where it came from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Unroutable {
    /// Path from the compiled sequence to the source event.
    pub path: Vec<PathStep>,
    /// Why it cannot play.
    pub reason: String,
}

/// What a compile did; nothing is silently dropped.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct CompileReport {
    /// Flattened MIDI events that play.
    pub events: usize,
    /// Scheduled entries (messages × targets).
    pub entries: usize,
    /// Events that cannot play.
    pub unroutable: Vec<Unroutable>,
    /// Per-note controllers and per-note pitch bends dropped.
    pub dropped_per_note: usize,
    /// Note attributes dropped.
    pub dropped_note_attributes: usize,
    /// 8-bit SysEx events dropped.
    pub dropped_sysex8: usize,
    /// Events from UMP groups other than 0 (sent without their group).
    pub group_folded: usize,
    /// Audio regions that play (uses: a looped or referenced region counts
    /// once per use).
    pub audio_regions: usize,
    /// Segments the regions became.
    pub audio_segments: usize,
    /// Regions on lanes without an output path (not played).
    pub audio_unrouted: usize,
    /// Segments whose media is offline (silence).
    pub audio_offline: usize,
    /// One line per kind of problem, for display.
    pub warnings: Vec<String>,
    /// Wall-clock compile time, excluding target (sink) creation.
    pub compile_time: Duration,
    /// The mixer graph compiled with this schedule (`Engine::load`).
    pub graph: crate::graph::GraphReport,
    /// Automation lanes (targets) the schedule plays (4d).
    pub automation_lanes: usize,
    /// Automation points in those lanes.
    pub automation_points: usize,
    /// Automation points whose strip, send, plugin or destination is gone
    /// (skipped, Plan 4d Ruling 3).
    pub automation_missing: usize,
    /// Lanes beyond [`MAX_AUTO_LANES`], dropped.
    pub automation_dropped_lanes: usize,
    /// Plugin-parameter lanes bound to a plugin instrument's target
    /// (set by `Engine::load`).
    pub automation_instrument_lanes: usize,
    /// Lanes whose MIDI thru cannot play, and why (set by `Engine::load`
    /// from the live input routes, spec §6.6; each also has a warning).
    pub thru_unroutable: Vec<(core_model::ids::LaneId, String)>,
    /// MPE notes stolen at compile time: more notes than member channels
    /// (plan 5g-2 Ruling 10).
    pub mpe_stolen: usize,
}

type Routes = Result<Vec<TargetIdx>, String>;

/// Resolves a target kind to an index: `Ok(Ok(idx))` plays, `Ok(Err(why))`
/// makes the events unroutable, `Err` aborts the compile.
pub type Resolver<'a> =
    dyn FnMut(&TargetKind) -> Result<Result<TargetIdx, String>, EngineError> + 'a;

struct Pending {
    frame: u64,
    class: Class,
    group: u8,
    order: u64,
    entry: Entry,
}

/// One collected event of an MPE destination, encoded after the main loop
/// (plan 5g-2 Ruling 10).
struct MpeItem {
    frame: u64,
    class: Class,
    order: u64,
    msg: LiveMsg,
}

/// An MPE destination's notes by `(destination, pitch)`, as `(start, end)`
/// ticks, and the poly pressure at each note's start, which becomes that
/// note's starting pressure (Ruling 10).
#[derive(Default)]
struct MpeNotes {
    spans: BTreeMap<(DestId, u8), Vec<(Tick, Tick)>>,
    start_pressure: BTreeMap<(DestId, u8, Tick), u32>,
}

impl MpeNotes {
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

/// Compiles sequence `seq` of `doc` into a schedule.
pub fn compile(
    doc: &Document,
    seq: SeqId,
    opts: &CompileOptions,
    resolve: &mut Resolver<'_>,
) -> Result<(Schedule, CompileReport), EngineError> {
    let started = Instant::now();
    let s = doc.sequence(seq)?;
    let len = seq_length(doc, seq)?;
    let origin = -s.start_offset;
    let sr = opts.sample_rate;
    let base = s.tempo.tick_to_seconds(origin);
    let frame = |t: Tick| -> u64 {
        let f = ((s.tempo.tick_to_seconds(t) - base) * sr).round();
        if f <= 0.0 { 0 } else { f as u64 }
    };

    let frame_signed = |t: Tick| -> i64 {
        let f = ((s.tempo.tick_to_seconds(t) - base) * sr).round();
        // Float-to-int `as` saturates.
        f as i64
    };
    let mut report = CompileReport::default();
    let mut regions = Vec::new();
    let mut routes: BTreeMap<DestId, Routes> = BTreeMap::new();
    let mut used: BTreeMap<DestId, ()> = BTreeMap::new();
    let mut mutes: BTreeMap<DestId, bool> = BTreeMap::new();
    let mut pending: Vec<Pending> = Vec::new();
    let mut arena: Vec<u8> = Vec::new();
    let mut used_channels = [0u16; MAX_TARGETS];
    let mut order = 0u64;
    let mut resolving = Duration::ZERO;

    let flat: Vec<_> = flatten(doc, seq, None, &ViewFilter::default())?.collect();
    // Automation (Plan 4d Rulings 7-9): lanes for the renderer; velocity
    // scale is applied to the note-ons below.
    let mut raw: Vec<(AutoDest, u64, f64)> = Vec::new();
    let mut vscale: BTreeMap<DestId, Vec<(Tick, f64)>> = BTreeMap::new();
    for r in &flat {
        let EventBody::Automation { target, value } = r.body else {
            continue;
        };
        if !target_exists(doc, target) {
            report.automation_missing += 1;
            continue;
        }
        // An Off strip plays its static values (spec §4.6), unless
        // automation is suspended (every strip then behaves as Read).
        let off = strip_of(doc, target)
            .and_then(|s| doc.mixer().strip(s).ok())
            .is_some_and(|s| s.automation == AutomationMode::Off);
        if off && !opts.suspend_automation {
            continue;
        }
        match (AutoDest::of(target), target) {
            (Some(d), _) => raw.push((d, frame(r.tick), value)),
            (None, AutomationTarget::VelocityScale(dest)) => {
                vscale.entry(dest).or_default().push((r.tick, value));
            }
            (None, _) => {}
        }
    }
    for pts in vscale.values_mut() {
        // Stable: play order stays on a shared tick.
        pts.sort_by_key(|p| p.0);
    }
    let (automation, dropped) = Automation::build(raw);
    report.automation_lanes = automation.lanes().len();
    report.automation_points = automation.point_count();
    report.automation_dropped_lanes = dropped;
    // A plugin instrument's parameter lanes need its target even when no
    // note is routed to it (Plan 4d P10): resolve it here, for the lanes
    // that play. A failure leaves the lane unbound and is reported.
    let mut wanted: Vec<PluginInstanceId> = Vec::new();
    for l in automation.lanes() {
        if let AutoDest::Plugin(id, _) = l.dest {
            if matches!(doc.find_slot(id), Some(SlotOwner::Instrument(_))) && !wanted.contains(&id)
            {
                wanted.push(id);
            }
        }
    }
    for id in wanted {
        let t0 = Instant::now();
        let got = resolve(&TargetKind::PluginInstrument(id))?;
        resolving += t0.elapsed();
        if let Err(why) = got {
            report.unroutable.push(Unroutable {
                path: Vec::new(),
                reason: format!("automation of plugin instance {id}: {why}"),
            });
        }
    }

    // MPE destinations (Ruling 10): where their notes are, so poly
    // pressure inside a note becomes per-note pressure.
    let mpe_of = |dest: DestId| doc.instruments().get(&dest).and_then(|d| d.mpe);
    let mut mpe_notes = MpeNotes::default();
    for r in &flat {
        let Some(dest) = r.dest.filter(|d| mpe_of(*d).is_some()) else {
            continue;
        };
        match &r.body {
            EventBody::Note {
                pitch, duration, ..
            } => mpe_notes
                .spans
                .entry((dest, pitch & 0x7F))
                .or_default()
                .push((r.tick, r.tick + *duration)),
            EventBody::PolyPressure { pitch, value } => {
                mpe_notes
                    .start_pressure
                    .insert((dest, pitch & 0x7F, r.tick), *value);
            }
            _ => {}
        }
    }
    let mut mpe_events: BTreeMap<DestId, Vec<MpeItem>> = BTreeMap::new();

    for r in flat {
        if matches!(r.body, EventBody::AudioRegion { .. }) {
            regions.push(r);
            continue;
        }
        if !r.body.is_midi() || matches!(r.body, EventBody::Text { .. }) {
            continue;
        }
        let Some(dest) = r.dest else {
            report.unroutable.push(Unroutable {
                path: r.path,
                reason: "no destination".into(),
            });
            continue;
        };
        let targets = match routes.get(&dest) {
            Some(t) => t.clone(),
            None => {
                let t0 = Instant::now();
                let t = route_dest(doc, dest, opts, resolve)?;
                resolving += t0.elapsed();
                routes.insert(dest, t.clone());
                t
            }
        };
        let targets = match targets {
            Ok(t) => t,
            Err(reason) => {
                report.unroutable.push(Unroutable {
                    path: r.path,
                    reason,
                });
                continue;
            }
        };
        let Some((group, channel)) = r.group_channel else {
            report.unroutable.push(Unroutable {
                path: r.path,
                reason: "no group/channel".into(),
            });
            continue;
        };
        if group != 0 {
            report.group_folded += 1;
        }
        let scaled = vscale
            .get(&dest)
            .and_then(|pts| scale_velocity(&r.body, pts, r.tick));
        // A muted external destination plays no notes, but its controllers
        // still play (plan 5g-1b Ruling 3).
        let muted = *mutes.entry(dest).or_insert_with(|| {
            doc.instruments()
                .get(&dest)
                .is_some_and(|d| d.kind.is_external() && d.midi_mix.mute)
        });
        if let Some(cfg) = mpe_of(dest) {
            let body = scaled.as_ref().unwrap_or(&r.body);
            let mut c = Collect {
                dest,
                cfg,
                muted,
                tick: r.tick,
                frame: &frame,
                notes: &mpe_notes,
                items: mpe_events.entry(dest).or_default(),
                order: &mut order,
                report: &mut report,
                played: false,
                silenced: false,
            };
            c.collect(body);
            let (mut played, silenced) = (c.played, c.silenced);
            // SysEx goes as for any destination.
            let sysex = match body {
                EventBody::SysEx { .. } => translate(body, cfg.manager()).sysex,
                _ => None,
            };
            if let Some(data) = sysex {
                push_sysex(
                    &mut pending,
                    &mut arena,
                    &mut order,
                    data,
                    frame(r.tick),
                    &targets,
                );
                played = true;
            }
            if played {
                report.events += 1;
            }
            if played || silenced {
                used.insert(dest, ());
            }
            continue;
        }
        let t = translate(scaled.as_ref().unwrap_or(&r.body), channel);
        report.dropped_per_note += t.drops.per_note;
        report.dropped_note_attributes += usize::from(t.drops.attribute);
        report.dropped_sysex8 += usize::from(t.drops.sysex8);
        let end = match &r.body {
            EventBody::Note { duration, .. } => r.tick + *duration,
            _ => r.tick,
        };
        let on = frame(r.tick);
        // A note-off never sorts at or before its note-on (ruling O20).
        let off = frame(end).max(on.saturating_add(1));
        let mut played = false;
        let mut silenced = false;
        for out in t.msgs() {
            if muted && is_note(&out.msg) {
                silenced = true;
                continue;
            }
            let f = if out.when == When::End { off } else { on };
            for target in &targets {
                pending.push(Pending {
                    frame: f,
                    class: out.class,
                    group: 1,
                    order,
                    entry: Entry {
                        frame: f,
                        target: *target,
                        msg: WireMsg::Midi1(out.msg),
                    },
                });
                order += 1;
                mark(&mut used_channels, *target, channel);
            }
            played = true;
        }
        if let Some(data) = t.sysex {
            push_sysex(
                &mut pending,
                &mut arena,
                &mut order,
                data,
                frame(r.tick),
                &targets,
            );
            played = true;
        }
        if played {
            report.events += 1;
        }
        if played || silenced {
            // A muted destination is still in use: its patch and mix go out.
            used.insert(dest, ());
        }
    }

    // Each MPE destination's events through one encoder (Ruling 10), onto
    // its targets in the encoder's order.
    for (dest, mut items) in mpe_events {
        let (Some(cfg), Some(Ok(targets))) = (mpe_of(dest), routes.get(&dest)) else {
            continue;
        };
        items.sort_by_key(|i| (i.frame, i.class, i.order));
        let mut enc = MpeEncoder::new(cfg);
        let mut out: Vec<Midi1Msg> = Vec::new();
        for item in items {
            out.clear();
            enc.encode(item.msg, &mut |m| out.push(m));
            for m in &out {
                // Channel messages keep their class (bank select sorts
                // with its program change); the encoder's own output
                // sorts by its status.
                let class = match item.msg {
                    LiveMsg::Channel(_) => item.class,
                    _ => Class::of(m.bytes[0]),
                };
                for target in targets {
                    pending.push(Pending {
                        frame: item.frame,
                        class,
                        group: 1,
                        order,
                        entry: Entry {
                            frame: item.frame,
                            target: *target,
                            msg: WireMsg::Midi1(*m),
                        },
                    });
                    order += 1;
                }
            }
        }
        report.mpe_stolen = report
            .mpe_stolen
            .saturating_add(usize::try_from(enc.take_stolen()).unwrap_or(usize::MAX));
        // The zone's channels are in use: controller resets cover them.
        let channels =
            std::iter::once(cfg.manager()).chain((0..cfg.members).filter_map(|k| cfg.member(k)));
        for ch in channels {
            for target in targets {
                mark(&mut used_channels, *target, ch);
            }
        }
    }

    // At frame 0, before every event on that frame of the same class: an
    // MPE destination's configuration (Ruling 7), then the default patches
    // of the destinations that play, then an external destination's mix
    // (CC 7, CC 10; plan 5g-1b). An MPE destination's patch and mix go on
    // its manager channel, for a plugin too (P1); others only play on a
    // MIDI endpoint's channel.
    let mut preamble: Vec<Entry> = Vec::new();
    for dest in used.keys() {
        let Some(d) = doc.instruments().get(dest) else {
            continue;
        };
        let ch = match (&d.kind, d.mpe) {
            (_, Some(cfg)) => cfg.manager(),
            (DestKind::MidiEndpoint { channel, .. }, None) => channel & 0x0F,
            _ => continue,
        };
        let Some(Ok(targets)) = routes.get(dest) else {
            continue;
        };
        if let Some(cfg) = d.mpe {
            for msg in configuration(&cfg) {
                for target in targets {
                    let entry = Entry {
                        frame: 0,
                        target: *target,
                        msg: WireMsg::Midi1(msg),
                    };
                    pending.push(Pending {
                        frame: 0,
                        class: Class::Patch,
                        group: 0,
                        order,
                        entry,
                    });
                    preamble.push(entry);
                    order += 1;
                    mark(&mut used_channels, *target, msg.bytes[0]);
                }
            }
        }
        let mut msgs: Vec<Midi1Msg> = Vec::new();
        if let Some(patch) = d.default_patch {
            if let Some((msb, lsb)) = patch.bank {
                msgs.push(Midi1Msg::three(0xB0 | ch, 0, msb & 0x7F));
                msgs.push(Midi1Msg::three(0xB0 | ch, 32, lsb & 0x7F));
            }
            msgs.push(Midi1Msg::two(0xC0 | ch, patch.program & 0x7F));
        }
        if d.kind.is_external() {
            if let Some(v) = d.midi_mix.volume {
                msgs.push(Midi1Msg::three(0xB0 | ch, 7, v & 0x7F));
            }
            if let Some(p) = d.midi_mix.pan {
                msgs.push(Midi1Msg::three(0xB0 | ch, 10, p & 0x7F));
            }
        }
        for msg in msgs {
            for target in targets {
                pending.push(Pending {
                    frame: 0,
                    class: Class::Patch,
                    group: 0,
                    order,
                    entry: Entry {
                        frame: 0,
                        target: *target,
                        msg: WireMsg::Midi1(msg),
                    },
                });
                order += 1;
                mark(&mut used_channels, *target, ch);
            }
        }
    }

    let audio = super::audio::build(
        doc,
        &regions,
        &frame_signed,
        &opts.offline_media,
        &mut report,
    );
    pending.sort_by_key(|p| (p.frame, p.class, p.group, p.order));
    let entries: Box<[Entry]> = pending.iter().map(|p| p.entry).collect();
    report.entries = entries.len();
    let length_frames = frame(len);
    let bar1 = frame(Tick::ZERO);
    let chase = ChaseIndex::build(&entries);
    let schedule = Schedule {
        sample_rate: sr,
        entries,
        sysex: arena.into_boxed_slice(),
        tempo: TempoTable::build(&s.tempo, origin, sr),
        length_frames,
        loop_range: (bar1 < length_frames).then_some((bar1, length_frames)),
        used_channels,
        chase,
        preamble: preamble.into_boxed_slice(),
        automation,
        audio,
    };
    report.warnings = warnings(&report);
    report.compile_time = started.elapsed().saturating_sub(resolving);
    Ok((schedule, report))
}

/// A velocity-scale lane's value at `tick` (Plan 4d Ruling 8).
fn lane_value(points: &[(Tick, f64)], tick: Tick) -> f64 {
    let n = points.partition_point(|p| p.0 <= tick);
    match (n.checked_sub(1).and_then(|i| points.get(i)), points.get(n)) {
        (None, Some(next)) => next.1,
        (Some(prev), Some(next)) if next.0 > prev.0 => {
            prev.1 + (next.1 - prev.1) * (tick - prev.0).0 as f64 / (next.0 - prev.0).0 as f64
        }
        (Some(prev), _) => prev.1,
        (None, None) => 0.5,
    }
}

/// A note `body` with its velocity scaled by the lane at `tick` (clamped
/// 1-65 535, Plan 4d Ruling 9); `None` for anything but a note.
fn scale_velocity(body: &EventBody, points: &[(Tick, f64)], tick: Tick) -> Option<EventBody> {
    let EventBody::Note { velocity, .. } = body else {
        return None;
    };
    let pct = velocity_scale_percent(lane_value(points, tick));
    let v = (f64::from(*velocity) * pct / 100.0)
        .round()
        .clamp(1.0, 65_535.0) as u16;
    let mut b = body.clone();
    if let EventBody::Note { velocity, .. } = &mut b {
        *velocity = v;
    }
    Some(b)
}

/// Collects one event of an MPE destination (Ruling 10) as [`MpeItem`]s,
/// applying its mute (P2): notes and per-note messages of a muted
/// destination are silenced; channel messages still play.
struct Collect<'a, F: Fn(Tick) -> u64> {
    dest: DestId,
    cfg: MpeConfig,
    muted: bool,
    tick: Tick,
    frame: &'a F,
    notes: &'a MpeNotes,
    items: &'a mut Vec<MpeItem>,
    order: &'a mut u64,
    report: &'a mut CompileReport,
    played: bool,
    silenced: bool,
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
        let class = if f <= on {
            Class::NoteOn
        } else {
            Class::Control
        };
        self.push(f.max(on), class, LiveMsg::PerNote { pitch, expr, value });
    }

    fn collect(&mut self, body: &EventBody) {
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

/// Stores `data` in the arena and schedules it at `frame` on `targets`.
fn push_sysex(
    pending: &mut Vec<Pending>,
    arena: &mut Vec<u8>,
    order: &mut u64,
    data: &[u8],
    frame: u64,
    targets: &[TargetIdx],
) {
    let start = u32::try_from(arena.len()).unwrap_or(u32::MAX);
    arena.push(0xF0);
    arena.extend_from_slice(data);
    arena.push(0xF7);
    let len = u32::try_from(data.len() + 2).unwrap_or(u32::MAX);
    for target in targets {
        pending.push(Pending {
            frame,
            class: Class::Control,
            group: 1,
            order: *order,
            entry: Entry {
                frame,
                target: *target,
                msg: WireMsg::SysEx { start, len },
            },
        });
        *order += 1;
    }
}

/// A MIDI 1.0 note-on or note-off.
fn is_note(msg: &Midi1Msg) -> bool {
    matches!(msg.bytes[0] & 0xF0, 0x80 | 0x90)
}

fn mark(used: &mut [u16; MAX_TARGETS], target: TargetIdx, channel: u8) {
    if let Some(m) = used.get_mut(usize::from(target.0)) {
        *m |= 1 << (channel & 0x0F);
    }
}

fn route_dest(
    doc: &Document,
    dest: DestId,
    opts: &CompileOptions,
    resolve: &mut Resolver<'_>,
) -> Result<Routes, EngineError> {
    let Some(d) = doc.instruments().get(&dest) else {
        return Ok(Err(format!("unknown destination {dest}")));
    };
    let kinds = match kinds_for(&d.kind, opts.route, &opts.ports) {
        Ok(k) => k,
        Err(why) => return Ok(Err(format!("{}: {why}", d.name))),
    };
    let mut out = Vec::with_capacity(kinds.len());
    for k in &kinds {
        match resolve(k)? {
            Ok(i) => out.push(i),
            Err(why) => return Ok(Err(format!("{}: {why}", d.name))),
        }
    }
    Ok(Ok(out))
}

fn warnings(r: &CompileReport) -> Vec<String> {
    let mut w = Vec::new();
    let mut reasons: BTreeMap<&str, usize> = BTreeMap::new();
    for u in &r.unroutable {
        *reasons.entry(u.reason.as_str()).or_default() += 1;
    }
    for (reason, n) in reasons {
        w.push(format!("{n} event(s) not played: {reason}"));
    }
    if r.group_folded > 0 {
        w.push(format!(
            "{} event(s) from UMP groups other than 0 sent without their group",
            r.group_folded
        ));
    }
    if r.dropped_per_note > 0 {
        w.push(format!(
            "{} per-note controller(s) dropped (MIDI 1.0 output)",
            r.dropped_per_note
        ));
    }
    if r.mpe_stolen > 0 {
        w.push(format!(
            "{} MPE note(s) stolen (more notes than member channels)",
            r.mpe_stolen
        ));
    }
    if r.dropped_note_attributes > 0 {
        w.push(format!(
            "{} note attribute(s) dropped (MIDI 1.0 output)",
            r.dropped_note_attributes
        ));
    }
    if r.dropped_sysex8 > 0 {
        w.push(format!(
            "{} 8-bit SysEx event(s) dropped (MIDI 1.0 output)",
            r.dropped_sysex8
        ));
    }
    if r.audio_unrouted > 0 {
        w.push(format!(
            "{} audio region(s) not played: their track has no output path",
            r.audio_unrouted
        ));
    }
    if r.audio_offline > 0 {
        w.push(format!(
            "{} audio region(s) play silence: their media is offline",
            r.audio_offline
        ));
    }
    if r.automation_missing > 0 {
        w.push(format!(
            "{} automation point(s) skipped: their strip, send, plugin or destination is gone",
            r.automation_missing
        ));
    }
    if r.automation_dropped_lanes > 0 {
        w.push(format!(
            "{} automation lane(s) dropped: at most {MAX_AUTO_LANES} play",
            r.automation_dropped_lanes
        ));
    }
    w
}
