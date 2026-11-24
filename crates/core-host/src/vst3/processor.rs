//! A VST3 plug-in's real-time half (host track H1 Task 4): translates a
//! [`ProcessCtx`] into an `IAudioProcessor::process` call. Every buffer,
//! list and table is allocated before the processor reaches a render
//! thread ([`Vst3Processor::new`], main thread). // RT
//!
//! - **Audio.** Every audio bus gets buffers: the main input and the
//!   sidechain (the first `kAux` input bus) are copied from the context,
//!   other inputs are silent; outputs are the context's channels, port
//!   after port, with scratch channels for those it lacks.
//! - **Parameters.** The host works in plain values, VST3 in normalised
//!   ones. The main thread samples each parameter's
//!   `normalizedParamToPlain` into a [`ParamMap`] at activation, so the
//!   audio thread converts without calling the controller. The first
//!   block carries the values set while inactive; edits the plug-in's
//!   controller made (`performEdit`) arrive normalised from the instance.
//!   What the processor reports (`outputParameterChanges`) goes back to
//!   the controller, and writable parameters also to the host as
//!   [`ParamEvent::Value`]s; host values go back to the controller too, so
//!   its editor follows automation.
//! - **MIDI.** Notes and poly pressure become events on event bus 0. CC,
//!   channel aftertouch and pitch bend have no VST3 event: they become
//!   parameter changes of the parameters the controller assigns them
//!   through `IMidiMapping` (read at activation), and are dropped when
//!   unassigned.
//! - **Transport.** A `ProcessContext` from the block's `BlockInfo`
//!   (tempo, position in quarter notes and samples, bar start, playing),
//!   in 4/4 as the CLAP host reports it.

use super::ffi::MIDI_CONTROLLERS;
use super::ffi::sdk;
use super::process_ffi::{HOST_QUEUES, RtData};
use crate::info::{PluginInfo, PortRole};
use crate::instance::AudioProcessor;
use crate::process::{MAX_EVENTS, MAX_PARAM_EVENTS, ParamEvent, ProcessCtx};
use core_engine::graph::ProcessStatus;
use core_model::mixer::ParamId;
use crossbeam_queue::ArrayQueue;
use sdk::Vst::Event_::EventTypes_;
use sdk::Vst::ProcessContext_::StatesAndFlags_;
use sdk::Vst::{
    ControllerNumbers_, Event, Event__type0, NoteOffEvent, NoteOnEvent, ParamID, PolyPressureEvent,
    ProcessModes_, kNoParamId,
};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

/// Segments a continuous parameter's map is sampled in when its mapping
/// is not linear.
const MAP_SEGMENTS: usize = 128;
/// Stepped parameters with more steps than this are mapped as continuous.
const MAX_MAPPED_STEPS: i32 = 4096;

/// A parameter's normalised-to-plain mapping, sampled on the main thread
/// at evenly spaced normalised points: two for a linear mapping, one per
/// step for a stepped one, else [`MAP_SEGMENTS`] + 1. Conversions in
/// both directions interpolate between the samples (exact for linear and
/// stepped mappings). // RT (the conversions)
#[derive(Debug, Clone)]
pub(crate) struct ParamMap {
    id: ParamID,
    /// The host may send it values (not read-only).
    writable: bool,
    stepped: bool,
    plain: Box<[f64]>,
    /// The samples rise or fall throughout.
    monotonic: bool,
}

impl ParamMap {
    /// Samples `to_plain` (the controller's `normalizedParamToPlain`) for
    /// parameter `id` with `steps` steps (0: continuous). Main thread.
    pub(crate) fn sample(
        id: ParamID,
        steps: i32,
        writable: bool,
        to_plain: impl Fn(f64) -> f64,
    ) -> ParamMap {
        let at = |n: f64| {
            let v = to_plain(n);
            if v.is_finite() { v } else { n }
        };
        let (stepped, plain): (bool, Vec<f64>) = if steps > 0 && steps <= MAX_MAPPED_STEPS {
            let s = f64::from(steps);
            (true, (0..=steps).map(|k| at(f64::from(k) / s)).collect())
        } else {
            let probe: Vec<f64> = [0.0, 0.25, 0.5, 0.75, 1.0].iter().map(|n| at(*n)).collect();
            let (first, last) = (
                probe.first().copied().unwrap_or(0.0),
                probe.last().copied().unwrap_or(1.0),
            );
            let tolerance = 1e-9 * (last - first).abs().max(1.0);
            let linear = [0.25, 0.5, 0.75]
                .iter()
                .zip(probe.iter().skip(1))
                .all(|(n, v)| (first + n * (last - first) - v).abs() <= tolerance);
            if linear {
                (false, vec![first, last])
            } else {
                let k = MAP_SEGMENTS as f64;
                (
                    false,
                    (0..=MAP_SEGMENTS).map(|i| at(i as f64 / k)).collect(),
                )
            }
        };
        let rising = plain.windows(2).all(|w| matches!(w, [a, b] if a <= b));
        let falling = plain.windows(2).all(|w| matches!(w, [a, b] if a >= b));
        ParamMap {
            id,
            writable,
            stepped,
            plain: plain.into_boxed_slice(),
            monotonic: rising || falling,
        }
    }

    fn segments(&self) -> usize {
        self.plain.len().saturating_sub(1)
    }

    fn at(&self, i: usize) -> f64 {
        self.plain.get(i).copied().unwrap_or(0.0)
    }

    /// A normalised value as plain. // RT
    pub(crate) fn to_plain(&self, n: f64) -> f64 {
        let n = n.clamp(0.0, 1.0);
        let segs = self.segments();
        if segs == 0 {
            return self.at(0);
        }
        let x = n * segs as f64;
        if self.stepped {
            return self.at(x.round() as usize);
        }
        let i = (x.floor() as usize).min(segs - 1);
        let frac = x - i as f64;
        self.at(i) + (self.at(i + 1) - self.at(i)) * frac
    }

    /// A plain value as normalised (`None` for a non-finite one). // RT
    pub(crate) fn to_normalized(&self, v: f64) -> Option<f64> {
        if !v.is_finite() {
            return None;
        }
        let segs = self.segments();
        if segs == 0 {
            return Some(0.0);
        }
        if self.stepped {
            // The nearest step.
            let mut best = (0usize, f64::INFINITY);
            for (k, p) in self.plain.iter().enumerate() {
                let d = (p - v).abs();
                if d < best.1 {
                    best = (k, d);
                }
            }
            return Some(best.0 as f64 / segs as f64);
        }
        let seg = |i: usize| -> Option<f64> {
            let (a, b) = (self.at(i), self.at(i + 1));
            let (lo, hi) = if a <= b { (a, b) } else { (b, a) };
            if v < lo || v > hi {
                return None;
            }
            let frac = if b == a { 0.0 } else { (v - a) / (b - a) };
            Some((i as f64 + frac) / segs as f64)
        };
        let i = if self.monotonic {
            // The first segment whose far end reaches `v`.
            let rising = self.at(segs) >= self.at(0);
            let far = |i: usize| self.at(i + 1);
            let (mut lo, mut hi) = (0usize, segs - 1);
            while lo < hi {
                let mid = (lo + hi) / 2;
                let reached = if rising { far(mid) >= v } else { far(mid) <= v };
                if reached { hi = mid } else { lo = mid + 1 }
            }
            seg(lo)
        } else {
            (0..segs).find_map(seg)
        };
        Some(i.unwrap_or_else(|| {
            // Outside the range: the nearer end.
            if (v - self.at(0)).abs() <= (v - self.at(segs)).abs() {
                0.0
            } else {
                1.0
            }
        }))
    }
}

/// What the main thread hands the processor at activation.
pub(crate) struct ProcessorSetup {
    pub data: RtData,
    pub sample_rate: f64,
    pub max_frames: u32,
    pub offline: bool,
    /// Every listed parameter's map, sorted by ID.
    pub maps: Vec<ParamMap>,
    /// `IMidiMapping` assignments (see `Plugin::midi_mapping`), or empty.
    pub midi: Vec<ParamID>,
    /// Normalised values the first block carries.
    pub first: Vec<(ParamID, f64)>,
    /// Edits from the controller (normalised), for the next block.
    pub edits: Arc<ArrayQueue<(ParamID, f64)>>,
    /// Where a processor dropped before its first block leaves `first`,
    /// then the edit it popped but had no queue for (capacity
    /// `1 + first.len()`; older than anything left in `edits`).
    pub returned: Arc<ArrayQueue<(ParamID, f64)>>,
    /// Values for the controller (normalised): host values and what the
    /// processor reported.
    pub echoes: Arc<ArrayQueue<(ParamID, f64)>>,
    /// Cleared when the processor is dropped.
    pub alive: Arc<AtomicBool>,
    /// Set by a state load: what the processor still has from before it
    /// (`first`, the held edit) is older than the load and is dropped.
    pub stale: Arc<AtomicBool>,
    /// Set once a processed block has carried `first` (the component's
    /// state holds those values from then on).
    pub first_sent: Arc<AtomicBool>,
}

/// The real-time half of a [`super::Vst3Instance`].
pub struct Vst3Processor {
    data: RtData,
    started: bool,
    mode: i32,
    sample_rate: f64,
    max_frames: u32,
    in_counts: Box<[u16]>,
    main_in: Option<usize>,
    sidechain_in: Option<usize>,
    /// Every input bus's channels, bus after bus.
    inputs: Box<[Box<[f32]>]>,
    out_counts: Box<[u16]>,
    /// Stand-ins for output channels the context does not offer.
    out_pad: Box<[Box<[f32]>]>,
    maps: Box<[ParamMap]>,
    midi: Box<[ParamID]>,
    first: Box<[(ParamID, f64)]>,
    first_sent: bool,
    /// [`ProcessorSetup::first_sent`], published after the block.
    first_published: Arc<AtomicBool>,
    edits: Arc<ArrayQueue<(ParamID, f64)>>,
    /// An edit popped while every queue was taken: sent first next block.
    held: Option<(ParamID, f64)>,
    returned: Arc<ArrayQueue<(ParamID, f64)>>,
    echoes: Arc<ArrayQueue<(ParamID, f64)>>,
    alive: Arc<AtomicBool>,
    stale: Arc<AtomicBool>,
    continuous: i64,
}

impl std::fmt::Debug for Vst3Processor {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Vst3Processor").finish_non_exhaustive()
    }
}

/// `Event::type` values (the SDK's `EventTypes`).
const NOTE_ON: u16 = EventTypes_::kNoteOnEvent as u16;
const NOTE_OFF: u16 = EventTypes_::kNoteOffEvent as u16;
const POLY_PRESSURE: u16 = EventTypes_::kPolyPressureEvent as u16;

/// An event on event bus 0 at `frame`.
fn event(frame: u32, kind: u16, body: Event__type0) -> Event {
    Event {
        busIndex: 0,
        sampleOffset: i32::try_from(frame).unwrap_or(i32::MAX),
        ppqPosition: 0.0,
        flags: 0,
        r#type: kind,
        __field0: body,
    }
}

impl Vst3Processor {
    pub(crate) fn new(s: ProcessorSetup, info: &PluginInfo) -> Vst3Processor {
        let in_counts: Box<[u16]> = info.inputs.iter().map(|p| p.channels).collect();
        let out_counts: Box<[u16]> = info.outputs.iter().map(|p| p.channels).collect();
        let in_total: usize = in_counts.iter().map(|c| usize::from(*c)).sum();
        let out_total: usize = out_counts.iter().map(|c| usize::from(*c)).sum();
        let frames = s.max_frames.max(1) as usize;
        let bufs = |n: usize| -> Box<[Box<[f32]>]> {
            (0..n)
                .map(|_| vec![0.0f32; frames].into_boxed_slice())
                .collect()
        };
        let mut maps = s.maps;
        maps.sort_by_key(|m| m.id);
        Vst3Processor {
            data: s.data,
            started: false,
            mode: if s.offline {
                ProcessModes_::kOffline as i32
            } else {
                ProcessModes_::kRealtime as i32
            },
            sample_rate: s.sample_rate,
            max_frames: s.max_frames.max(1),
            main_in: info.inputs.iter().position(|p| p.role == PortRole::Main),
            sidechain_in: info
                .inputs
                .iter()
                .position(|p| p.role == PortRole::Sidechain),
            inputs: bufs(in_total),
            in_counts,
            out_pad: bufs(out_total),
            out_counts,
            maps: maps.into_boxed_slice(),
            midi: s.midi.into_boxed_slice(),
            first: s.first.into_boxed_slice(),
            first_sent: false,
            first_published: s.first_sent,
            edits: s.edits,
            held: None,
            returned: s.returned,
            echoes: s.echoes,
            alive: s.alive,
            stale: s.stale,
            continuous: 0,
        }
    }

    /// The map of parameter `id`. // RT
    fn map(&self, id: ParamID) -> Option<&ParamMap> {
        self.maps
            .binary_search_by_key(&id, |m| m.id)
            .ok()
            .and_then(|i| self.maps.get(i))
    }

    /// First channel index of input bus `bus`.
    fn in_start(&self, bus: usize) -> usize {
        self.in_counts
            .iter()
            .take(bus)
            .map(|c| usize::from(*c))
            .sum()
    }

    /// Copies the context's main and sidechain channels into the input
    /// buses; other buses get silence. Points the input buses at them. // RT
    fn fill_inputs(&mut self, ctx: &ProcessCtx<'_>, n: usize) {
        for ch in self.inputs.iter_mut() {
            if let Some(c) = ch.get_mut(..n) {
                c.fill(0.0);
            }
        }
        for (bus, src) in [
            (self.main_in, &*ctx.input),
            (self.sidechain_in, &*ctx.sidechain),
        ] {
            let Some(bus) = bus else {
                continue;
            };
            let start = self.in_start(bus);
            let count = usize::from(self.in_counts.get(bus).copied().unwrap_or(0));
            for k in 0..count {
                // A mono source fills every channel of a stereo bus.
                let s = src.get(k).or_else(|| src.first());
                if let (Some(d), Some(s)) = (self.inputs.get_mut(start + k), s) {
                    if let (Some(d), Some(s)) = (d.get_mut(..n), s.get(..n)) {
                        d.copy_from_slice(s);
                    }
                }
            }
        }
        for (k, ch) in self.inputs.iter_mut().enumerate() {
            self.data.ins.set(k, ch);
        }
    }

    /// Points the output buses at the context's channels (scratch for the
    /// ones it lacks or that are too short), cleared. // RT
    fn fill_outputs(&mut self, ctx: &mut ProcessCtx<'_>, n: usize) {
        let mut rest: &mut [Box<[f32]>] = ctx.outputs;
        let mut pad: &mut [Box<[f32]>] = &mut self.out_pad;
        let total: usize = self.out_counts.iter().map(|c| usize::from(*c)).sum();
        for k in 0..total {
            let theirs = match std::mem::take(&mut rest).split_first_mut() {
                Some((h, t)) => {
                    rest = t;
                    Some(h).filter(|h| h.len() >= n)
                }
                None => None,
            };
            let buf = match theirs {
                Some(b) => b,
                None => match std::mem::take(&mut pad).split_first_mut() {
                    Some((h, t)) => {
                        pad = t;
                        h
                    }
                    None => continue,
                },
            };
            if let Some(c) = buf.get_mut(..n) {
                c.fill(0.0);
            }
            self.data.outs.set(k, buf);
        }
    }

    /// Fills the input parameter changes and events. // RT
    fn fill_changes(&mut self, ctx: &ProcessCtx<'_>, n: usize) {
        let last = i32::try_from(n.saturating_sub(1)).unwrap_or(i32::MAX);
        let at = |frame: u32| i32::try_from(frame).unwrap_or(i32::MAX).min(last).max(0);
        // A state load since the last block: what is still here from
        // before it would land on the loaded state.
        if self.stale.swap(false, Ordering::AcqRel) {
            self.first_sent = true;
            self.held = None;
        }
        let params = self.data.params_in();
        params.clear();
        if !self.first_sent {
            // The list has a queue for each of them (`RtData::new`).
            for (id, v) in self.first.iter() {
                params.add(*id, 0, *v, false);
            }
            self.first_sent = true;
        }
        // Edits in order, in the queues not kept for the host's values;
        // one with no queue left waits for the next block, and so do the
        // ones after it.
        while params.has_room_beyond(HOST_QUEUES) {
            let Some((id, v)) = self.held.take().or_else(|| self.edits.pop()) else {
                break;
            };
            if !params.add(id, 0, v, false) {
                self.held = Some((id, v));
                break;
            }
        }
        for p in ctx.params_in.iter().take(MAX_PARAM_EVENTS) {
            let Ok(id) = ParamID::try_from(p.id.0) else {
                continue;
            };
            let Some(v) = self
                .map(id)
                .filter(|m| m.writable)
                .and_then(|m| m.to_normalized(p.value))
            else {
                continue;
            };
            // Always fits: the list keeps a queue for each host value
            // (`HOST_QUEUES`), and they come in frame order, so a full
            // queue takes the latest in place of its last point. The host
            // counts these values as delivered after the block.
            let added = params.add(id, at(p.frame), v, true);
            debug_assert!(added, "a host value had no room");
        }
        let events = self.data.events_in();
        events.clear();
        for m in ctx.events.iter().take(MAX_EVENTS) {
            let [status, d1, d2] = m.bytes;
            let channel = i16::from(status & 0x0F);
            let pitch = i16::from(d1 & 0x7F);
            let frame = m.frame.min(u32::try_from(last).unwrap_or(0));
            let controller = |ctrl: usize, v: f64| {
                let id = self
                    .midi
                    .get(usize::from(status & 0x0F) * MIDI_CONTROLLERS + ctrl)
                    .copied()
                    .unwrap_or(kNoParamId);
                if id != kNoParamId {
                    params.add(id, at(frame), v, true);
                }
            };
            match status & 0xF0 {
                0x90 if d2 > 0 => {
                    events.push(event(
                        frame,
                        NOTE_ON,
                        Event__type0 {
                            noteOn: NoteOnEvent {
                                channel,
                                pitch,
                                tuning: 0.0,
                                velocity: f32::from(d2 & 0x7F) / 127.0,
                                length: 0,
                                noteId: -1,
                            },
                        },
                    ));
                }
                0x80 | 0x90 => {
                    events.push(event(
                        frame,
                        NOTE_OFF,
                        Event__type0 {
                            noteOff: NoteOffEvent {
                                channel,
                                pitch,
                                velocity: f32::from(d2 & 0x7F) / 127.0,
                                noteId: -1,
                                tuning: 0.0,
                            },
                        },
                    ));
                }
                0xA0 => {
                    events.push(event(
                        frame,
                        POLY_PRESSURE,
                        Event__type0 {
                            polyPressure: PolyPressureEvent {
                                channel,
                                pitch,
                                pressure: f32::from(d2 & 0x7F) / 127.0,
                                noteId: -1,
                            },
                        },
                    ));
                }
                0xB0 => controller(usize::from(d1 & 0x7F), f64::from(d2 & 0x7F) / 127.0),
                0xD0 => controller(
                    ControllerNumbers_::kAfterTouch as usize,
                    f64::from(d1 & 0x7F) / 127.0,
                ),
                0xE0 => {
                    let bend = (u16::from(d2 & 0x7F) << 7) | u16::from(d1 & 0x7F);
                    controller(
                        ControllerNumbers_::kPitchBend as usize,
                        f64::from(bend) / 16383.0,
                    );
                }
                _ => {}
            }
        }
    }

    /// The block's transport. // RT
    fn fill_context(&mut self, ctx: &ProcessCtx<'_>) {
        let t = ctx.transport;
        let rate = self.sample_rate;
        let continuous = self.continuous;
        let c = self.data.context_mut();
        let mut state = StatesAndFlags_::kTempoValid
            | StatesAndFlags_::kTimeSigValid
            | StatesAndFlags_::kProjectTimeMusicValid
            | StatesAndFlags_::kBarPositionValid
            | StatesAndFlags_::kContTimeValid;
        if t.playing {
            state |= StatesAndFlags_::kPlaying;
        }
        #[allow(
            clippy::unnecessary_cast,
            reason = "the SDK's flag type is signed on some platforms"
        )]
        let state = state as u32;
        c.state = state;
        c.sampleRate = rate;
        c.projectTimeSamples = (t.beats * 60.0 / t.tempo_bpm.max(1.0) * rate).round() as i64;
        c.continousTimeSamples = continuous;
        c.projectTimeMusic = t.beats;
        c.barPositionMusic = (t.beats / 4.0).floor() * 4.0;
        c.tempo = t.tempo_bpm;
        c.timeSigNumerator = 4;
        c.timeSigDenominator = 4;
    }

    /// Hands what the processor reported to the host (writable parameters)
    /// and, with the host's own values, to the controller. // RT
    fn read_changes(&mut self, ctx: &mut ProcessCtx<'_>) {
        for q in self.data.params_in().queues().filter(|q| q.host_sourced()) {
            if let Some((_, v)) = q.last() {
                let _ = self.echoes.push((q.id(), v));
            }
        }
        for q in self.data.params_out().queues() {
            // A plug-in's NaN or infinity reaches neither the host's
            // document nor the controller.
            let Some((_, v)) = q.last().filter(|(_, v)| v.is_finite()) else {
                continue;
            };
            let _ = self.echoes.push((q.id(), v));
            if let Some(m) = self.map(q.id()).filter(|m| m.writable) {
                let plain = m.to_plain(v);
                ctx.params_out
                    .push(ParamEvent::Value(ParamId(u64::from(q.id())), plain));
            }
        }
    }
}

impl AudioProcessor for Vst3Processor {
    fn process(&mut self, ctx: &mut ProcessCtx<'_>) -> ProcessStatus {
        let n = ctx.frames.min(self.max_frames) as usize;
        if !self.started {
            // The SDK allows `setProcessing` on the processing thread.
            self.data.set_processing(true);
            self.started = true;
        }
        self.fill_inputs(ctx, n);
        self.fill_outputs(ctx, n);
        self.fill_changes(ctx, n);
        self.fill_context(ctx);
        let ok = self
            .data
            .process(self.mode, i32::try_from(n).unwrap_or(i32::MAX));
        self.read_changes(ctx);
        if self.first_sent {
            self.first_published.store(true, Ordering::Release);
        }
        self.continuous = self.continuous.wrapping_add(n as i64);
        if ok {
            ProcessStatus::Ok
        } else {
            ProcessStatus::Error
        }
    }

    fn reset(&mut self) {
        // VST3 has no reset: stopping and restarting processing is the
        // SDK's way to clear voices and delay lines.
        if self.started {
            self.data.set_processing(false);
            self.data.set_processing(true);
        }
        self.continuous = 0;
    }
}

impl Drop for Vst3Processor {
    fn drop(&mut self) {
        if self.started {
            self.data.set_processing(false);
        }
        // What it never sent goes back, oldest first (`returned` has room
        // for all of it), unless a state load made it stale.
        if !self.stale.swap(false, Ordering::AcqRel) {
            if !self.first_sent {
                for v in self.first.iter() {
                    let _ = self.returned.push(*v);
                }
            }
            if let Some(e) = self.held.take() {
                let _ = self.returned.push(e);
            }
        }
        self.alive.store(false, Ordering::Release);
    }
}

#[cfg(test)]
mod tests {
    use super::ParamMap;

    fn close(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-9
    }

    #[test]
    fn a_linear_mapping_is_exact_both_ways() {
        let m = ParamMap::sample(1, 0, true, |n| -60.0 + 72.0 * n);
        assert_eq!(m.plain.len(), 2);
        for n in [0.0, 0.1, 0.5, 60.0 / 72.0, 1.0] {
            assert!(close(m.to_plain(n), -60.0 + 72.0 * n));
            assert!(close(m.to_normalized(-60.0 + 72.0 * n).unwrap(), n));
        }
        assert_eq!(m.to_normalized(100.0), Some(1.0));
        assert_eq!(m.to_normalized(-100.0), Some(0.0));
        assert_eq!(m.to_normalized(f64::NAN), None);
    }

    #[test]
    fn a_stepped_mapping_snaps_to_steps() {
        let m = ParamMap::sample(1, 2, true, |n| (n * 2.0).round());
        assert_eq!(m.plain.len(), 3);
        assert!(close(m.to_plain(0.3), 1.0));
        assert!(close(m.to_normalized(2.0).unwrap(), 1.0));
        assert!(close(m.to_normalized(0.9).unwrap(), 0.5));
    }

    #[test]
    fn a_curved_mapping_is_sampled_and_inverted() {
        let f = |n: f64| 20.0 * (1000.0f64).powf(n);
        let m = ParamMap::sample(1, 0, true, f);
        assert_eq!(m.plain.len(), super::MAP_SEGMENTS + 1);
        for n in [0.0, 0.123, 0.5, 0.9, 1.0] {
            assert!((m.to_plain(n) - f(n)).abs() / f(n) < 1e-3);
            let back = m.to_normalized(f(n)).unwrap();
            assert!((back - n).abs() < 1e-3, "{n} {back}");
        }
        // Falling mappings invert too.
        let m = ParamMap::sample(1, 0, true, |n| 1.0 - n * n);
        assert!((m.to_normalized(0.75).unwrap() - 0.5).abs() < 1e-3);
    }
}
