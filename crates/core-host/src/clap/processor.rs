//! A CLAP plugin's real-time half: translates a [`ProcessCtx`] into a CLAP
//! `process` call. Every buffer is allocated in `new`. // RT

use super::handlers::{VizHost, audio_call};
use crate::info::{PluginInfo, PortRole};
use crate::instance::AudioProcessor;
use crate::process::{MAX_EVENTS, MAX_PARAM_EVENTS, ParamEvent, ProcessCtx};
use clack_host::events::event_types::{
    MidiEvent as ClapMidi, NoteOffEvent, NoteOnEvent, ParamValueEvent, TransportEvent,
    TransportFlags,
};
use clack_host::events::io::{OutputEventBuffer, TryPushError};
use clack_host::events::spaces::CoreEventSpace;
use clack_host::events::{EventFlags, EventHeader, Match, UnknownEvent};
use clack_host::prelude::*;
use clack_host::utils::{BeatTime, SecondsTime};
use core_engine::graph::ProcessStatus;
use core_model::mixer::ParamId;

/// Collects the plugin's parameter events into the block's `ParamOut`
/// without allocating; other output events are dropped.
struct GestureOut<'a>(&'a mut crate::process::ParamOut);

impl OutputEventBuffer for GestureOut<'_> {
    fn try_push(&mut self, event: &UnknownEvent) -> Result<(), TryPushError> {
        let id = |c: Option<ClapId>| c.map(|c| ParamId(u64::from(c.get())));
        let e = match event.as_core_event() {
            Some(CoreEventSpace::ParamGestureBegin(g)) => id(g.param_id()).map(ParamEvent::Begin),
            Some(CoreEventSpace::ParamGestureEnd(g)) => id(g.param_id()).map(ParamEvent::End),
            Some(CoreEventSpace::ParamValue(v)) => {
                id(v.param_id()).map(|p| ParamEvent::Value(p, v.value()))
            }
            _ => None,
        };
        match e {
            Some(e) if !self.0.push(e) => Err(TryPushError::new()),
            _ => Ok(()),
        }
    }
}

/// A port's channels: the caller's, then scratch for those it lacks.
type Chans<'a> =
    std::iter::Chain<std::slice::IterMut<'a, Box<[f32]>>, std::slice::IterMut<'a, Box<[f32]>>>;

/// A port's input channels, each cut to the block length.
struct InChans<'a> {
    it: Chans<'a>,
    n: usize,
}

impl<'a> Iterator for InChans<'a> {
    type Item = InputChannel<'a, f32>;
    fn next(&mut self) -> Option<Self::Item> {
        let b = self.it.next()?;
        let len = self.n.min(b.len());
        Some(InputChannel::variable(b.get_mut(..len).unwrap_or(&mut [])))
    }
}

/// A port's output channels, each cut to the block length.
struct OutChans<'a> {
    it: Chans<'a>,
    n: usize,
}

impl<'a> Iterator for OutChans<'a> {
    type Item = &'a mut [f32];
    fn next(&mut self) -> Option<Self::Item> {
        let b = self.it.next()?;
        let len = self.n.min(b.len());
        Some(b.get_mut(..len).unwrap_or(&mut []))
    }
}

/// Splits a flat channel list into consecutive ports, each with exactly
/// its declared channel count: the list's channels first, then scratch
/// channels for the ones the list lacks. // RT
struct Split<'a, 'c> {
    rest: &'a mut [Box<[f32]>],
    pad: &'a mut [Box<[f32]>],
    counts: std::slice::Iter<'c, u16>,
}

impl<'a> Iterator for Split<'a, '_> {
    type Item = Chans<'a>;
    fn next(&mut self) -> Option<Self::Item> {
        let want = usize::from(*self.counts.next()?);
        let c = want.min(self.rest.len());
        let (head, tail) = std::mem::take(&mut self.rest).split_at_mut(c);
        self.rest = tail;
        let missing = (want - c).min(self.pad.len());
        let (extra, pad_tail) = std::mem::take(&mut self.pad).split_at_mut(missing);
        self.pad = pad_tail;
        Some(head.iter_mut().chain(extra.iter_mut()))
    }
    fn size_hint(&self) -> (usize, Option<usize>) {
        self.counts.size_hint()
    }
}

impl ExactSizeIterator for Split<'_, '_> {}

type NoF64In = std::array::IntoIter<InputChannel<'static, f64>, 0>;
type NoF64Out = std::array::IntoIter<&'static mut [f64], 0>;

/// The real-time half of a [`super::ClapInstance`].
pub struct ClapProcessor {
    proc: Option<PluginAudioProcessor<VizHost>>,
    in_ports: AudioPorts,
    out_ports: AudioPorts,
    /// Channels per input port, and which port is main / sidechain.
    in_counts: Box<[u16]>,
    main_in: Option<usize>,
    sidechain_in: Option<usize>,
    /// Every input port's channels, port after port.
    inputs: Box<[Box<[f32]>]>,
    out_counts: Box<[u16]>,
    /// Stand-ins for output channels the context does not offer.
    out_pad: Box<[Box<[f32]>]>,
    events: EventBuffer,
    clap_notes: bool,
    steady: u64,
    max_frames: u32,
}

impl std::fmt::Debug for ClapProcessor {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ClapProcessor").finish_non_exhaustive()
    }
}

impl ClapProcessor {
    pub(crate) fn new(
        proc: PluginAudioProcessor<VizHost>,
        info: &PluginInfo,
        max_frames: u32,
        clap_notes: bool,
    ) -> ClapProcessor {
        let in_counts: Box<[u16]> = info.inputs.iter().map(|p| p.channels).collect();
        let out_counts: Box<[u16]> = info.outputs.iter().map(|p| p.channels).collect();
        let in_total: usize = in_counts.iter().map(|c| usize::from(*c)).sum();
        let out_total: usize = out_counts.iter().map(|c| usize::from(*c)).sum();
        ClapProcessor {
            proc: Some(proc),
            in_ports: AudioPorts::with_capacity(in_total, in_counts.len()),
            out_ports: AudioPorts::with_capacity(out_total, out_counts.len()),
            main_in: info.inputs.iter().position(|p| p.role == PortRole::Main),
            sidechain_in: info
                .inputs
                .iter()
                .position(|p| p.role == PortRole::Sidechain),
            inputs: (0..in_total)
                .map(|_| vec![0.0f32; max_frames as usize].into_boxed_slice())
                .collect(),
            in_counts,
            out_counts,
            out_pad: (0..out_total)
                .map(|_| vec![0.0f32; max_frames as usize].into_boxed_slice())
                .collect(),
            events: EventBuffer::with_capacity(MAX_EVENTS + MAX_PARAM_EVENTS),
            clap_notes,
            steady: 0,
            max_frames,
        }
    }

    /// First channel index of input port `port`.
    fn in_start(&self, port: usize) -> usize {
        self.in_counts
            .iter()
            .take(port)
            .map(|c| usize::from(*c))
            .sum()
    }

    /// Copies the context's main and sidechain channels into the plugin's
    /// input ports; other ports get silence. // RT
    fn fill_inputs(&mut self, ctx: &ProcessCtx<'_>, n: usize) {
        for ch in self.inputs.iter_mut() {
            if let Some(c) = ch.get_mut(..n) {
                c.fill(0.0);
            }
        }
        for (port, src) in [
            (self.main_in, ctx.input),
            (self.sidechain_in, ctx.sidechain),
        ] {
            let Some(port) = port else {
                continue;
            };
            let start = self.in_start(port);
            let count = usize::from(self.in_counts.get(port).copied().unwrap_or(0));
            for k in 0..count {
                // A mono source fills every channel of a stereo port.
                let s = src.get(k).or_else(|| src.first());
                if let (Some(d), Some(s)) = (self.inputs.get_mut(start + k), s) {
                    if let (Some(d), Some(s)) = (d.get_mut(..n), s.get(..n)) {
                        d.copy_from_slice(s);
                    }
                }
            }
        }
    }

    /// Fills the input event list from the context, merged by frame. // RT
    fn fill_events(&mut self, ctx: &ProcessCtx<'_>) {
        self.events.clear();
        let mut params = ctx.params_in.iter().take(MAX_PARAM_EVENTS).peekable();
        let mut midi = ctx.events.iter().take(MAX_EVENTS).peekable();
        loop {
            let take_param = match (params.peek(), midi.peek()) {
                (Some(p), Some(m)) => p.frame <= m.frame,
                (Some(_), None) => true,
                (None, Some(_)) => false,
                (None, None) => break,
            };
            if take_param {
                let Some(p) = params.next() else {
                    break;
                };
                let Some(id) = u32::try_from(p.id.0).ok().and_then(ClapId::from_raw) else {
                    continue;
                };
                self.events.push(&ParamValueEvent::new(
                    p.frame,
                    id,
                    Pckn::match_all(),
                    p.value,
                ));
                continue;
            }
            let Some(m) = midi.next() else {
                break;
            };
            let [status, d1, d2] = m.bytes;
            let ch = u16::from(status & 0x0F);
            let pckn = Pckn::new(0u16, ch, u16::from(d1 & 0x7F), Match::<u32>::All);
            match status & 0xF0 {
                0x90 if self.clap_notes && d2 > 0 => {
                    self.events
                        .push(&NoteOnEvent::new(m.frame, pckn, f64::from(d2) / 127.0))
                }
                0x80 | 0x90 if self.clap_notes => {
                    self.events
                        .push(&NoteOffEvent::new(m.frame, pckn, f64::from(d2) / 127.0))
                }
                _ => self.events.push(&ClapMidi::new(m.frame, 0, m.bytes)),
            }
        }
    }
}

fn transport(ctx: &ProcessCtx<'_>) -> TransportEvent {
    let t = ctx.transport;
    let mut flags = TransportFlags::HAS_TEMPO | TransportFlags::HAS_BEATS_TIMELINE;
    if t.playing {
        flags |= TransportFlags::IS_PLAYING;
    }
    let seconds = t.beats * 60.0 / t.tempo_bpm.max(1.0);
    TransportEvent {
        header: EventHeader::new_core(0, EventFlags::empty()),
        flags,
        song_pos_beats: BeatTime::from_float(t.beats),
        song_pos_seconds: SecondsTime::from_float(seconds),
        tempo: t.tempo_bpm,
        tempo_inc: 0.0,
        loop_start_beats: BeatTime::from_float(0.0),
        loop_end_beats: BeatTime::from_float(0.0),
        loop_start_seconds: SecondsTime::from_float(0.0),
        loop_end_seconds: SecondsTime::from_float(0.0),
        bar_start: BeatTime::from_float((t.beats / 4.0).floor() * 4.0),
        bar_number: (t.beats / 4.0).floor() as i32,
        time_signature_numerator: 4,
        time_signature_denominator: 4,
    }
}

impl AudioProcessor for ClapProcessor {
    fn process(&mut self, ctx: &mut ProcessCtx<'_>) -> ProcessStatus {
        let n = ctx.frames.min(self.max_frames) as usize;
        self.fill_inputs(ctx, n);
        self.fill_events(ctx);
        let t = transport(ctx);
        let Some(proc) = self.proc.as_mut() else {
            return ProcessStatus::Error;
        };
        let steady = self.steady;
        let ins = Split {
            rest: &mut self.inputs,
            pad: &mut [],
            counts: self.in_counts.iter(),
        }
        .map(|chans| AudioPortBuffer {
            latency: 0,
            channels: AudioPortBufferType::<_, NoF64In>::F32(InChans { it: chans, n }),
        });
        let input = self.in_ports.with_input_buffers(ins);
        let outs = Split {
            rest: ctx.outputs,
            pad: &mut self.out_pad,
            counts: self.out_counts.iter(),
        }
        .map(|chans| AudioPortBuffer {
            latency: 0,
            channels: AudioPortBufferType::<_, NoF64Out>::F32(OutChans { it: chans, n }),
        });
        let mut output = self.out_ports.with_output_buffers(outs);
        let events_in = self.events.as_input();
        let mut gestures = GestureOut(ctx.params_out);
        let mut events_out = OutputEvents::from_buffer(&mut gestures);
        let result = audio_call(|| {
            let started = proc.ensure_processing_started().ok()?;
            started
                .process(
                    &input,
                    &mut output,
                    &events_in,
                    &mut events_out,
                    Some(steady),
                    Some(&t),
                )
                .ok()
        });
        self.steady = self.steady.wrapping_add(n as u64);
        match result {
            Some(_) => ProcessStatus::Ok,
            None => ProcessStatus::Error,
        }
    }

    fn reset(&mut self) {
        if let Some(p) = self.proc.as_mut() {
            audio_call(|| p.reset());
        }
        self.steady = 0;
    }
}

impl Drop for ClapProcessor {
    fn drop(&mut self) {
        if let Some(p) = self.proc.take() {
            // `stop_processing` is an audio-thread call.
            audio_call(move || drop(p.into_stopped()));
        }
    }
}
