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
use core_engine::graph::{EffectNode, ProcessStatus};
use core_model::ids::PluginInstanceId;
use std::sync::mpsc::Sender;

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
        }
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
        let k = self.params.drain_into(&mut self.params_buf[..]);
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
            params_in: self.params_buf.get(..k).unwrap_or(&[]),
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
        }
    }
}

impl InstrumentNode for PluginSynth {
    fn midi(&mut self, offset: u32, msg: &[u8]) {
        // SysEx is not passed to plugins (Plan Ruling 10).
        if msg.first() == Some(&0xF0) {
            return;
        }
        if let Some(slot) = self.events.get_mut(self.count) {
            *slot = MidiEvent::new(offset, msg);
            self.count += 1;
        }
    }

    fn render(&mut self, frames: u32, out: &mut StereoBuf) {
        self.render_outputs(frames, std::slice::from_mut(out), &BlockInfo::default());
    }

    fn render_outputs(&mut self, frames: u32, outs: &mut [StereoBuf], info: &BlockInfo) {
        let n = frames.min(self.core.max_frames);
        let events = self.events.get_mut(..self.count).unwrap_or(&mut []);
        sort_by_frame(events);
        let status = self.core.run(n, &mut [], &mut [], events, info);
        self.count = 0;
        for (k, o) in outs.iter_mut().enumerate() {
            if status == ProcessStatus::Error {
                o.clear(frames);
            } else {
                self.core.write_port(k, o, n as usize);
            }
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
