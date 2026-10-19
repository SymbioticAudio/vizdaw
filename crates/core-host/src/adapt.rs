//! Plugin processors as the engine's inserts ([`PluginEffect`]) and
//! instruments ([`PluginSynth`]) (3a Plan Ruling 12). Buffers and queues
//! are allocated here, before the processor reaches a render thread. When
//! an adapter is dropped (always off the audio thread: the engine retires
//! graphs and sinks through its collector) it sends its processor back to
//! the host, which deactivates it on the main thread. // RT (processing)

use crate::info::{PluginInfo, PortRole};
use crate::instance::AudioProcessor;
use crate::process::{MAX_EVENTS, MAX_PARAM_EVENTS, MidiEvent, ParamChange, ParamOut, ProcessCtx};
use crate::queue::{GestureSender, ParamReceiver};
use core_engine::backend::{BlockInfo, InstrumentNode, StereoBuf};
use core_engine::graph::dsp::is_faulty;
use core_engine::graph::exec::FAULT_LIMIT;
use core_engine::graph::{EffectNode, ProcessStatus};
use core_model::automation::param_plain;
use core_model::ids::PluginInstanceId;
use core_model::mixer::ParamId;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::Sender;

/// How often a full automation queue collapsed to the latest value per
/// parameter (diagnostics; the `rt_alloc_plugins` test checks the branch
/// ran). A relaxed counter: allocation- and lock-free.
static COLLAPSES: AtomicU64 = AtomicU64::new(0);

/// How often a full automation queue collapsed since the process started.
pub fn automation_collapses() -> u64 {
    COLLAPSES.load(Ordering::Relaxed)
}

/// A parameter's range, for automation (Plan 4d Ruling 12).
#[derive(Debug, Clone, Copy)]
struct ParamRange {
    id: ParamId,
    min: f64,
    max: f64,
    stepped: bool,
}

/// A processor coming back from the engine.
pub struct Returned {
    /// Its instance.
    pub id: PluginInstanceId,
    /// The processor.
    pub processor: Box<dyn AudioProcessor>,
}

impl std::fmt::Debug for Returned {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Returned").field("id", &self.id).finish()
    }
}

fn channels(n: usize, frames: u32) -> Vec<Box<[f32]>> {
    (0..n)
        .map(|_| vec![0.0f32; frames as usize].into_boxed_slice())
        .collect()
}

/// What every adapter holds.
struct Core {
    id: PluginInstanceId,
    proc: Option<Box<dyn AudioProcessor>>,
    latency: u32,
    max_frames: u32,
    outputs: Vec<Box<[f32]>>,
    /// `(first channel, channels)` of every output port.
    out_ports: Vec<(usize, usize)>,
    params: ParamReceiver,
    gestures: GestureSender,
    params_buf: Box<[ParamChange; MAX_PARAM_EVENTS]>,
    out: ParamOut,
    back: Option<Sender<Returned>>,
    /// Parameter ranges by ID (sorted).
    ranges: Box<[ParamRange]>,
    /// Automation's changes for the next block, in arrival order.
    auto: Box<[ParamChange; MAX_PARAM_EVENTS]>,
    auto_len: usize,
}

impl Core {
    fn new(
        id: PluginInstanceId,
        proc: Box<dyn AudioProcessor>,
        info: &PluginInfo,
        max_frames: u32,
        queues: (ParamReceiver, GestureSender),
        back: Sender<Returned>,
    ) -> Core {
        let mut out_ports = Vec::with_capacity(info.outputs.len());
        let mut at = 0;
        for p in &info.outputs {
            let c = usize::from(p.channels);
            out_ports.push((at, c));
            at += c;
        }
        Core {
            id,
            proc: Some(proc),
            latency: info.latency,
            max_frames,
            outputs: channels(at, max_frames),
            out_ports,
            params: queues.0,
            gestures: queues.1,
            params_buf: Box::new([ParamChange::default(); MAX_PARAM_EVENTS]),
            out: ParamOut::default(),
            back: Some(back),
            ranges: {
                let mut r: Vec<ParamRange> = info
                    .params
                    .iter()
                    .map(|p| ParamRange {
                        id: p.id,
                        min: p.min,
                        max: p.max,
                        stepped: p.stepped,
                    })
                    .collect();
                r.sort_by_key(|p| p.id);
                r.into_boxed_slice()
            },
            auto: Box::new([ParamChange::default(); MAX_PARAM_EVENTS]),
            auto_len: 0,
        }
    }

    /// Queues automation's value for `param` at `frame` of the next block:
    /// normalised -> plain by the parameter's range (rounded when stepped);
    /// an unknown parameter is dropped (Plan 4d Ruling 12). A full queue
    /// first collapses to the latest value per parameter, so the newest
    /// value of every automated parameter always fits (Plan 4d final
    /// review I1); false only when even that leaves no room (more distinct
    /// parameters than the queue holds). // RT
    fn automate(&mut self, frame: u32, param: ParamId, normalized: f64) -> bool {
        let Ok(i) = self.ranges.binary_search_by_key(&param, |r| r.id) else {
            return true;
        };
        let Some(r) = self.ranges.get(i) else {
            return true;
        };
        let value = param_plain(normalized, r.min, r.max, r.stepped);
        if self.auto_len >= MAX_PARAM_EVENTS {
            COLLAPSES.fetch_add(1, Ordering::Relaxed);
            self.collapse();
        }
        let Some(slot) = self.auto.get_mut(self.auto_len) else {
            return false;
        };
        *slot = ParamChange {
            frame,
            id: param,
            value,
        };
        self.auto_len += 1;
        true
    }

    /// The block went by without the plugin running: collapses the queued
    /// automation to the latest value per parameter, all at frame 0, so a
    /// long bypass neither piles values up nor loses where automation got
    /// to (an unchanged value is never resent). Bounded, no allocation.
    /// // RT
    fn collapse(&mut self) {
        let mut kept = 0;
        for i in 0..self.auto_len {
            let Some(&c) = self.auto.get(i) else {
                break;
            };
            let same = self
                .auto
                .get(..kept)
                .and_then(|k| k.iter().position(|k| k.id == c.id));
            let at = same.unwrap_or(kept);
            if let Some(slot) = self.auto.get_mut(at) {
                *slot = ParamChange { frame: 0, ..c };
            }
            if same.is_none() {
                kept += 1;
            }
        }
        self.auto_len = kept;
    }

    /// The parameters the plugin declares stepped. Not RT.
    fn stepped(&self) -> Vec<ParamId> {
        self.ranges
            .iter()
            .filter(|r| r.stepped)
            .map(|r| r.id)
            .collect()
    }

    /// Copies output port `port` into `dst` (a mono port feeds both
    /// sides; a missing port gives silence). // RT
    fn write_port(&self, port: usize, dst: &mut StereoBuf, n: usize) {
        let Some(&(start, count)) = self.out_ports.get(port) else {
            dst.clear(n as u32);
            return;
        };
        let left = self.outputs.get(start);
        let right = if count >= 2 {
            self.outputs.get(start + 1)
        } else {
            left
        };
        for (d, s) in [(&mut dst.left, left), (&mut dst.right, right)] {
            match (d.get_mut(..n), s.and_then(|s| s.get(..n))) {
                (Some(d), Some(s)) => d.copy_from_slice(s),
                (Some(d), None) => d.fill(0.0),
                _ => {}
            }
        }
    }

    /// Runs the processor with `input`, `sidechain` and `events`. // RT
    fn run(
        &mut self,
        frames: u32,
        input: &mut [Box<[f32]>],
        sidechain: &mut [Box<[f32]>],
        events: &[MidiEvent],
        info: &BlockInfo,
    ) -> ProcessStatus {
        // Automation must always fit beside the host's values (Plan 4d
        // final review I1): past three quarters of the queue it collapses
        // to the latest value per parameter, and the host's values take
        // only the room left (the rest wait in their queue for the next
        // block).
        if self.auto_len > MAX_PARAM_EVENTS / 4 * 3 {
            self.collapse();
        }
        let room = MAX_PARAM_EVENTS.saturating_sub(self.auto_len);
        let k = self
            .params
            .drain_into(self.params_buf.get_mut(..room).unwrap_or(&mut []));
        // Automation follows the host's values, so it wins a tie (Plan 4d
        // Ruling 12); then the list is sorted by frame.
        let last = frames.min(self.max_frames).saturating_sub(1);
        let mut n = k;
        for c in self.auto.get(..self.auto_len).unwrap_or(&[]) {
            if let Some(slot) = self.params_buf.get_mut(n) {
                *slot = ParamChange {
                    frame: c.frame.min(last),
                    ..*c
                };
                n += 1;
            }
        }
        self.auto_len = 0;
        sort_params(self.params_buf.get_mut(..n).unwrap_or(&mut []));
        self.out.clear();
        let Some(proc) = self.proc.as_mut() else {
            return ProcessStatus::Error;
        };
        let mut ctx = ProcessCtx {
            frames: frames.min(self.max_frames),
            input,
            sidechain,
            outputs: &mut self.outputs,
            events,
            params_in: self.params_buf.get(..n).unwrap_or(&[]),
            params_out: &mut self.out,
            transport: info,
        };
        let status = proc.process(&mut ctx);
        self.gestures.send_all(self.out.as_slice());
        status
    }
}

impl Drop for Core {
    fn drop(&mut self) {
        if let (Some(processor), Some(back)) = (self.proc.take(), self.back.take()) {
            // If the host is gone, the processor is dropped here instead.
            let _ = back.send(Returned {
                id: self.id,
                processor,
            });
        }
    }
}

/// A plugin as an insert.
pub struct PluginEffect {
    core: Core,
    input: Vec<Box<[f32]>>,
    sidechain: Vec<Box<[f32]>>,
}

impl PluginEffect {
    /// Wraps an activated processor (made for blocks of `max_frames`).
    pub fn new(
        id: PluginInstanceId,
        proc: Box<dyn AudioProcessor>,
        info: &PluginInfo,
        max_frames: u32,
        queues: (ParamReceiver, GestureSender),
        back: Sender<Returned>,
    ) -> PluginEffect {
        let main = info
            .main_input()
            .and_then(|i| info.inputs.get(i))
            .map_or(0, |p| usize::from(p.channels));
        let sc = info
            .inputs
            .iter()
            .find(|p| p.role == PortRole::Sidechain)
            .map_or(0, |p| usize::from(p.channels));
        PluginEffect {
            core: Core::new(id, proc, info, max_frames, queues, back),
            input: channels(main, max_frames),
            sidechain: channels(sc, max_frames),
        }
    }
}

/// Copies a stereo buffer into `dst` channels (one channel: the mix). //
/// RT
fn copy_in(dst: &mut [Box<[f32]>], src: &StereoBuf, n: usize) {
    match dst {
        [mono] => {
            if let (Some(d), Some(l), Some(r)) =
                (mono.get_mut(..n), src.left.get(..n), src.right.get(..n))
            {
                for ((d, l), r) in d.iter_mut().zip(l).zip(r) {
                    *d = 0.5 * (l + r);
                }
            }
        }
        [left, right, ..] => {
            for (d, s) in [(left, &src.left), (right, &src.right)] {
                if let (Some(d), Some(s)) = (d.get_mut(..n), s.get(..n)) {
                    d.copy_from_slice(s);
                }
            }
        }
        [] => {}
    }
}

impl PluginEffect {
    /// The instance it serves.
    pub fn id(&self) -> PluginInstanceId {
        self.core.id
    }

    /// The parameters the plugin declares stepped.
    pub fn stepped_params(&self) -> Vec<ParamId> {
        self.core.stepped()
    }

    /// Unwraps the processor (it is not sent back).
    pub fn into_processor(mut self) -> Option<Box<dyn AudioProcessor>> {
        self.core.proc.take()
    }
}

impl EffectNode for PluginEffect {
    fn latency(&self) -> u32 {
        self.core.latency
    }

    fn process(
        &mut self,
        frames: u32,
        io: &mut StereoBuf,
        sidechain: Option<&StereoBuf>,
    ) -> ProcessStatus {
        self.process_block(frames, io, sidechain, &BlockInfo::default())
    }

    fn process_block(
        &mut self,
        frames: u32,
        io: &mut StereoBuf,
        sidechain: Option<&StereoBuf>,
        info: &BlockInfo,
    ) -> ProcessStatus {
        let n = (frames.min(self.core.max_frames) as usize)
            .min(io.left.len())
            .min(io.right.len());
        copy_in(&mut self.input, io, n);
        match sidechain {
            Some(sc) => copy_in(&mut self.sidechain, sc, n),
            None => {
                for c in self.sidechain.iter_mut() {
                    if let Some(c) = c.get_mut(..n) {
                        c.fill(0.0);
                    }
                }
            }
        }
        let status = self
            .core
            .run(n as u32, &mut self.input, &mut self.sidechain, &[], info);
        self.core.write_port(0, io, n);
        status
    }

    fn reset(&mut self) {
        if let Some(p) = self.core.proc.as_mut() {
            p.reset();
        }
    }

    fn automate(&mut self, frame: u32, param: ParamId, normalized: f64) -> bool {
        self.core.automate(frame, param, normalized)
    }

    fn skipped(&mut self) {
        self.core.collapse();
    }
}

/// Stable insertion sort of parameter changes by frame (no allocation).
/// // RT
fn sort_params(p: &mut [ParamChange]) {
    for i in 1..p.len() {
        let mut j = i;
        while j > 0 {
            let (Some(a), Some(b)) = (p.get(j - 1), p.get(j)) else {
                break;
            };
            if a.frame <= b.frame {
                break;
            }
            p.swap(j - 1, j);
            j -= 1;
        }
    }
}

/// Stable insertion sort by frame: arrival order is frame order except for
/// releases sent at offset 0 after earlier events, and same-frame order
/// must be kept. No allocation (`slice::sort_by_key` may allocate). // RT
fn sort_by_frame(events: &mut [MidiEvent]) {
    for i in 1..events.len() {
        let mut j = i;
        while j > 0 {
            let (Some(a), Some(b)) = (events.get(j - 1), events.get(j)) else {
                break;
            };
            if a.frame <= b.frame {
                break;
            }
            events.swap(j - 1, j);
            j -= 1;
        }
    }
}

/// A plugin as a plugin instrument destination's sink.
pub struct PluginSynth {
    core: Core,
    events: Box<[MidiEvent; MAX_EVENTS]>,
    count: usize,
    outputs: u16,
    /// Consecutive faulty blocks (spec §7).
    bad_run: u32,
    /// Bypassed (silent) after `FAULT_LIMIT` faulty blocks.
    faulted: bool,
    /// The fault has not been taken by the renderer yet.
    unreported: bool,
}

/// A message that ends sound and must survive a skipped block: a
/// note-off, a sustain-pedal release (CC64 below 64), Reset All
/// Controllers (CC121, which also releases the pedal), All Sound Off or
/// All Notes Off. // RT
fn releases(e: &MidiEvent) -> bool {
    matches!(
        (e.bytes[0] & 0xF0, e.bytes[1], e.bytes[2]),
        (0x80, ..) | (0x90, _, 0) | (0xB0, 120 | 121 | 123, _)
    ) || matches!((e.bytes[0] & 0xF0, e.bytes[1], e.bytes[2]), (0xB0, 64, v) if v < 64)
}

impl PluginSynth {
    /// Wraps an activated processor (made for blocks of `max_frames`).
    pub fn new(
        id: PluginInstanceId,
        proc: Box<dyn AudioProcessor>,
        info: &PluginInfo,
        max_frames: u32,
        queues: (ParamReceiver, GestureSender),
        back: Sender<Returned>,
    ) -> PluginSynth {
        let outputs = u16::try_from(info.outputs.len().clamp(1, 16)).unwrap_or(1);
        PluginSynth {
            core: Core::new(id, proc, info, max_frames, queues, back),
            events: Box::new([MidiEvent::default(); MAX_EVENTS]),
            count: 0,
            outputs,
            bad_run: 0,
            faulted: false,
            unreported: false,
        }
    }
}

impl PluginSynth {
    /// The instance it serves.
    pub fn id(&self) -> PluginInstanceId {
        self.core.id
    }

    /// The parameters the plugin declares stepped.
    pub fn stepped_params(&self) -> Vec<ParamId> {
        self.core.stepped()
    }

    /// Unwraps the processor (it is not sent back).
    pub fn into_processor(mut self) -> Option<Box<dyn AudioProcessor>> {
        self.core.proc.take()
    }
}

impl InstrumentNode for PluginSynth {
    fn midi(&mut self, offset: u32, msg: &[u8]) {
        // SysEx is not passed to plugins (Plan Ruling 10).
        if msg.is_empty() || msg.first() == Some(&0xF0) {
            return;
        }
        let ev = MidiEvent::new(offset, msg);
        if let Some(slot) = self.events.get_mut(self.count) {
            *slot = ev;
            self.count += 1;
        } else if releases(&ev) {
            // Full: a release must not be lost (a stuck note); it takes
            // the place of the newest event that is not one.
            let at = self.events.iter().rposition(|e| !releases(e));
            if let Some(slot) = at.and_then(|i| self.events.get_mut(i)) {
                *slot = ev;
            }
        }
    }

    fn skip(&mut self, _frames: u32) {
        // The block was not rendered: queued notes never sound, but their
        // releases still apply, at the start of the next block.
        let mut kept = 0;
        for i in 0..self.count {
            let Some(mut e) = self.events.get(i).copied() else {
                break;
            };
            if releases(&e) {
                e.frame = 0;
                if let Some(slot) = self.events.get_mut(kept) {
                    *slot = e;
                }
                kept += 1;
            }
        }
        self.count = kept;
    }

    fn take_fault(&mut self) -> Option<PluginInstanceId> {
        if std::mem::take(&mut self.unreported) {
            Some(self.core.id)
        } else {
            None
        }
    }

    fn render(&mut self, frames: u32, out: &mut StereoBuf) {
        self.render_outputs(frames, std::slice::from_mut(out), &BlockInfo::default());
    }

    fn automate(&mut self, frame: u32, param: ParamId, normalized: f64) -> bool {
        self.core.automate(frame, param, normalized)
    }

    fn render_outputs(&mut self, frames: u32, outs: &mut [StereoBuf], info: &BlockInfo) {
        if self.faulted {
            self.count = 0;
            self.core.collapse();
            for o in outs.iter_mut() {
                o.clear(frames);
            }
            return;
        }
        let n = frames.min(self.core.max_frames);
        let events = self.events.get_mut(..self.count).unwrap_or(&mut []);
        sort_by_frame(events);
        let status = self.core.run(n, &mut [], &mut [], events, info);
        self.count = 0;
        let mut bad = status == ProcessStatus::Error;
        for (k, o) in outs.iter_mut().enumerate() {
            if bad {
                o.clear(frames);
            } else {
                self.core.write_port(k, o, n as usize);
                bad = is_faulty(o, frames);
            }
        }
        if bad {
            // Zero every output; 16 bad blocks in a row bypass the
            // instance and raise a plugin fault (spec §7).
            for o in outs.iter_mut() {
                o.clear(frames);
            }
            self.bad_run = self.bad_run.saturating_add(1);
            if self.bad_run >= FAULT_LIMIT {
                self.faulted = true;
                self.unreported = true;
                if let Some(p) = self.core.proc.as_mut() {
                    p.reset();
                }
            }
        } else {
            self.bad_run = 0;
        }
    }

    fn reset(&mut self) {
        self.count = 0;
        if let Some(p) = self.core.proc.as_mut() {
            p.reset();
        }
    }

    fn latency(&self) -> u32 {
        self.core.latency
    }

    fn outputs(&self) -> u16 {
        self.outputs
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn r(msg: &[u8]) -> bool {
        releases(&MidiEvent::new(0, msg))
    }

    #[test]
    fn releases_include_pedal_ups_and_controller_resets() {
        assert!(r(&[0x80, 60, 0]));
        assert!(r(&[0x93, 60, 0]), "note-on with velocity 0");
        assert!(r(&[0xB0, 64, 0]), "sustain released");
        assert!(r(&[0xB5, 64, 63]));
        assert!(r(&[0xB0, 121, 0]), "reset all controllers");
        assert!(r(&[0xB0, 120, 0]));
        assert!(r(&[0xB0, 123, 0]));
        assert!(!r(&[0x90, 60, 100]));
        assert!(!r(&[0xB0, 64, 64]), "sustain pressed");
        assert!(!r(&[0xB0, 64, 127]));
        assert!(!r(&[0xB0, 1, 0]));
        assert!(!r(&[0xE0, 0, 64]));
    }
}
