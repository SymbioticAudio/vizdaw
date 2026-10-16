//! The schedule compiler (non-RT): flatten → route → time → translate →
//! sort.

use super::targets::{BuiltinRoute, TargetIdx, TargetKind, kinds_for};
use super::tempo::TempoTable;
use super::{Entry, MAX_TARGETS, Schedule, WireMsg};
use crate::backend::MidiPortInfo;
use crate::error::EngineError;
use crate::midi1::{Class, Midi1Msg, When, translate};
use core_model::document::{DestKind, Document};
use core_model::event::EventBody;
use core_model::ids::{DestId, SeqId};
use core_model::query::{PathStep, flatten, seq_length};
use core_model::selection::ViewFilter;
use core_model::time::Tick;
use std::collections::BTreeMap;
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
}

impl CompileOptions {
    /// Built-in routing, no MIDI destinations.
    pub fn new(sample_rate: f64) -> CompileOptions {
        CompileOptions {
            sample_rate,
            route: BuiltinRoute::Builtin,
            ports: Vec::new(),
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
    /// Audio regions skipped (audio playback is not in this engine yet).
    pub skipped_audio: usize,
    /// One line per kind of problem, for display.
    pub warnings: Vec<String>,
    /// Wall-clock compile time, excluding target (sink) creation.
    pub compile_time: Duration,
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

    let mut report = CompileReport::default();
    let mut routes: BTreeMap<DestId, Routes> = BTreeMap::new();
    let mut used: BTreeMap<DestId, ()> = BTreeMap::new();
    let mut pending: Vec<Pending> = Vec::new();
    let mut arena: Vec<u8> = Vec::new();
    let mut used_channels = [0u16; MAX_TARGETS];
    let mut order = 0u64;
    let mut resolving = Duration::ZERO;

    for r in flatten(doc, seq, None, &ViewFilter::default())? {
        if matches!(r.body, EventBody::AudioRegion { .. }) {
            report.skipped_audio += 1;
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
        let (group, channel) = r.group_channel.unwrap_or((0, 0));
        if group != 0 {
            report.group_folded += 1;
        }
        let t = translate(&r.body, channel);
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
        for out in t.msgs() {
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
            let start = u32::try_from(arena.len()).unwrap_or(u32::MAX);
            arena.push(0xF0);
            arena.extend_from_slice(data);
            arena.push(0xF7);
            let len = u32::try_from(data.len() + 2).unwrap_or(u32::MAX);
            let f = frame(r.tick);
            for target in &targets {
                pending.push(Pending {
                    frame: f,
                    class: Class::Control,
                    group: 1,
                    order,
                    entry: Entry {
                        frame: f,
                        target: *target,
                        msg: WireMsg::SysEx { start, len },
                    },
                });
                order += 1;
            }
            played = true;
        }
        if played {
            report.events += 1;
            used.insert(dest, ());
        }
    }

    // Default patches of the destinations that play, at frame 0, before
    // every event on that frame of the same class.
    for dest in used.keys() {
        let Some(d) = doc.instruments().get(dest) else {
            continue;
        };
        let (Some(patch), DestKind::MidiEndpoint { channel, .. }) = (d.default_patch, &d.kind)
        else {
            continue;
        };
        let Some(Ok(targets)) = routes.get(dest) else {
            continue;
        };
        let ch = channel & 0x0F;
        let mut msgs: Vec<Midi1Msg> = Vec::new();
        if let Some((msb, lsb)) = patch.bank {
            msgs.push(Midi1Msg::three(0xB0 | ch, 0, msb & 0x7F));
            msgs.push(Midi1Msg::three(0xB0 | ch, 32, lsb & 0x7F));
        }
        msgs.push(Midi1Msg::two(0xC0 | ch, patch.program & 0x7F));
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

    pending.sort_by_key(|p| (p.frame, p.class, p.group, p.order));
    let entries: Box<[Entry]> = pending.iter().map(|p| p.entry).collect();
    report.entries = entries.len();
    let length_frames = frame(len);
    let bar1 = frame(Tick::ZERO);
    let schedule = Schedule {
        sample_rate: sr,
        entries,
        sysex: arena.into_boxed_slice(),
        tempo: TempoTable::build(&s.tempo, origin, sr),
        length_frames,
        loop_range: (bar1 < length_frames).then_some((bar1, length_frames)),
        used_channels,
    };
    report.warnings = warnings(&report);
    report.compile_time = started.elapsed().saturating_sub(resolving);
    Ok((schedule, report))
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
    if r.skipped_audio > 0 {
        w.push(format!(
            "{} audio region(s) skipped (audio playback is not available yet)",
            r.skipped_audio
        ));
    }
    w
}
