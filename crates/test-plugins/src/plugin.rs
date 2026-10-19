//! The one plugin type behind every [`Kind`]: its behaviour is chosen at
//! creation from the plugin id.

use crate::{BANK_PARAMS, Kind, PARAM_BANK_FIRST, PARAM_GAIN, PARAM_LATENCY, PARAM_WIGGLE};
use clack_extensions::audio_ports::{
    AudioPortFlags, AudioPortInfo, AudioPortInfoWriter, AudioPortType, PluginAudioPorts,
    PluginAudioPortsImpl,
};
use clack_extensions::latency::{PluginLatency, PluginLatencyImpl};
use clack_extensions::note_ports::{
    NoteDialect, NoteDialects, NotePortInfo, NotePortInfoWriter, PluginNotePorts,
    PluginNotePortsImpl,
};
use clack_extensions::params::{
    ParamDisplayWriter, ParamInfo, ParamInfoFlags, ParamInfoWriter, PluginAudioProcessorParams,
    PluginMainThreadParams, PluginParams,
};
use clack_extensions::state::{PluginState, PluginStateImpl};
use clack_plugin::events::event_types::{
    ParamGestureBeginEvent, ParamGestureEndEvent, ParamValueEvent,
};
use clack_plugin::events::spaces::CoreEventSpace;
use clack_plugin::prelude::*;
use clack_plugin::stream::{InputStream, OutputStream};
use std::ffi::CStr;
use std::fmt::Write as _;
use std::io::{Read, Write};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};

/// Largest latency the gain plugin accepts (frames).
const MAX_LATENCY: u32 = 48_000;

pub struct TestPlugin;

impl Plugin for TestPlugin {
    type AudioProcessor<'a> = Processor<'a>;
    type Shared<'a> = Shared<'a>;
    type MainThread<'a> = MainThread<'a>;

    fn declare_extensions(builder: &mut PluginExtensions<Self>, shared: Option<&Shared<'_>>) {
        let kind = shared.map(|s| s.kind);
        builder.register::<PluginAudioPorts>();
        if kind.is_none_or(|k| k == Kind::Gain) {
            builder
                .register::<PluginParams>()
                .register::<PluginState>()
                .register::<PluginLatency>();
        }
        if kind.is_none_or(|k| k == Kind::Synth4) {
            builder.register::<PluginNotePorts>();
        }
        if kind == Some(Kind::Bank) {
            builder.register::<PluginParams>();
        }
    }
}

/// State every thread may touch.
pub struct Shared<'a> {
    kind: Kind,
    host: HostSharedHandle<'a>,
    gain: AtomicU64,
    /// Latency asked for by the latency parameter.
    want_latency: AtomicU32,
    /// Latency in effect since the last activation.
    latency: AtomicU32,
    /// A Wiggle value waiting to become a gesture (f64 bits).
    wiggle: AtomicU64,
    wiggle_pending: AtomicBool,
    /// The bank plugin's parameter values (f64 bits).
    bank: [AtomicU64; BANK_PARAMS as usize],
}

/// The bank slot of parameter `id`.
fn bank_index(id: u32) -> Option<usize> {
    id.checked_sub(PARAM_BANK_FIRST)
        .filter(|i| *i < BANK_PARAMS)
        .map(|i| i as usize)
}

impl<'a> Shared<'a> {
    pub fn new(kind: Kind, host: HostSharedHandle<'a>) -> Shared<'a> {
        Shared {
            kind,
            host,
            gain: AtomicU64::new(1.0f64.to_bits()),
            want_latency: AtomicU32::new(0),
            latency: AtomicU32::new(0),
            wiggle: AtomicU64::new(0),
            wiggle_pending: AtomicBool::new(false),
            bank: std::array::from_fn(|_| AtomicU64::new(0)),
        }
    }

    fn gain(&self) -> f64 {
        f64::from_bits(self.gain.load(Ordering::Relaxed))
    }

    fn set_gain(&self, v: f64) {
        self.gain
            .store(v.clamp(0.0, 2.0).to_bits(), Ordering::Relaxed);
    }

    /// Applies one parameter change; `active` says whether a latency change
    /// needs a restart.
    fn set_param(&self, id: u32, value: f64, active: bool) {
        match id {
            PARAM_GAIN => self.set_gain(value),
            PARAM_LATENCY => {
                let l = value.round().clamp(0.0, f64::from(MAX_LATENCY)) as u32;
                self.want_latency.store(l, Ordering::Relaxed);
                if !active {
                    self.latency.store(l, Ordering::Relaxed);
                } else if l != self.latency.load(Ordering::Relaxed) {
                    self.host.request_restart();
                }
            }
            PARAM_WIGGLE => {
                self.wiggle.store(value.to_bits(), Ordering::Relaxed);
                self.wiggle_pending.store(true, Ordering::Release);
            }
            id => {
                if let Some(slot) = bank_index(id).and_then(|i| self.bank.get(i)) {
                    slot.store(value.clamp(0.0, 1.0).to_bits(), Ordering::Relaxed);
                }
            }
        }
    }

    fn param_value(&self, id: u32) -> Option<f64> {
        match id {
            PARAM_GAIN => Some(self.gain()),
            PARAM_LATENCY => Some(f64::from(self.want_latency.load(Ordering::Relaxed))),
            PARAM_WIGGLE => Some(f64::from_bits(self.wiggle.load(Ordering::Relaxed))),
            id => bank_index(id)
                .and_then(|i| self.bank.get(i))
                .map(|v| f64::from_bits(v.load(Ordering::Relaxed))),
        }
    }

    /// Reads parameter events; returns true when Wiggle asks for a gesture.
    fn read_params(&self, events: &InputEvents, active: bool) {
        for e in events {
            if let Some(CoreEventSpace::ParamValue(p)) = e.as_core_event() {
                if let Some(id) = p.param_id() {
                    self.set_param(id.get(), p.value(), active);
                }
            }
        }
    }

    /// Emits the gesture a pending Wiggle asked for.
    fn write_gesture(&self, out: &mut OutputEvents) {
        if !self.wiggle_pending.swap(false, Ordering::Acquire) {
            return;
        }
        let v = f64::from_bits(self.wiggle.load(Ordering::Relaxed));
        self.set_gain(v);
        let id = ClapId::new(PARAM_GAIN);
        let _ = out.try_push(ParamGestureBeginEvent::new(0, id));
        let _ = out.try_push(ParamValueEvent::new(0, id, Pckn::match_all(), self.gain()));
        let _ = out.try_push(ParamGestureEndEvent::new(0, id));
    }
}

impl<'a> PluginShared<'a> for Shared<'a> {}

pub struct MainThread<'a> {
    shared: &'a Shared<'a>,
}

impl<'a> MainThread<'a> {
    pub fn new(_host: HostMainThreadHandle<'a>, shared: &'a Shared<'a>) -> MainThread<'a> {
        MainThread { shared }
    }
}

impl<'a> PluginMainThread<'a, Shared<'a>> for MainThread<'a> {}

fn port(id: u32, name: &[u8], main: bool) -> AudioPortInfo<'_> {
    AudioPortInfo {
        id: ClapId::new(id),
        name,
        channel_count: 2,
        flags: if main {
            AudioPortFlags::IS_MAIN
        } else {
            AudioPortFlags::empty()
        },
        port_type: Some(AudioPortType::STEREO),
        in_place_pair: None,
    }
}

impl PluginAudioPortsImpl for MainThread<'_> {
    fn count(&self, is_input: bool) -> u32 {
        match (self.shared.kind, is_input) {
            (Kind::Synth4, true) => 0,
            (Kind::Synth4, false) => 4,
            (Kind::Ducker, true) => 2,
            _ => 1,
        }
    }

    fn get(&self, index: u32, is_input: bool, writer: &mut AudioPortInfoWriter) {
        let info = match (self.shared.kind, is_input, index) {
            (Kind::Synth4, false, 0) => port(0, b"Out 1", true),
            (Kind::Synth4, false, 1) => port(1, b"Out 2", false),
            (Kind::Synth4, false, 2) => port(2, b"Out 3", false),
            (Kind::Synth4, false, 3) => port(3, b"Out 4", false),
            (Kind::Synth4, _, _) => return,
            (Kind::Ducker, true, 1) => port(1, b"Sidechain", false),
            (_, true, 0) => port(0, b"In", true),
            (_, false, 0) => port(0, b"Out", true),
            _ => return,
        };
        writer.set(&info);
    }
}

impl PluginNotePortsImpl for MainThread<'_> {
    fn count(&self, is_input: bool) -> u32 {
        u32::from(is_input && self.shared.kind == Kind::Synth4)
    }

    fn get(&self, index: u32, is_input: bool, writer: &mut NotePortInfoWriter) {
        if is_input && index == 0 {
            writer.set(&NotePortInfo {
                id: ClapId::new(0),
                name: b"Notes",
                supported_dialects: NoteDialects::CLAP | NoteDialects::MIDI,
                preferred_dialect: Some(NoteDialect::Clap),
            });
        }
    }
}

impl PluginMainThreadParams for MainThread<'_> {
    fn count(&self) -> u32 {
        match self.shared.kind {
            Kind::Bank => BANK_PARAMS,
            _ => 3,
        }
    }

    fn get_info(&self, index: u32, info: &mut ParamInfoWriter) {
        if self.shared.kind == Kind::Bank {
            if index < BANK_PARAMS {
                info.set(&ParamInfo {
                    id: ClapId::new(PARAM_BANK_FIRST + index),
                    flags: ParamInfoFlags::IS_AUTOMATABLE,
                    cookie: Default::default(),
                    name: b"Bank",
                    module: b"",
                    min_value: 0.0,
                    max_value: 1.0,
                    default_value: 0.0,
                });
            }
            return;
        }
        let (id, name, max, def, flags): (u32, &[u8], f64, f64, ParamInfoFlags) = match index {
            0 => (
                PARAM_GAIN,
                b"Gain",
                2.0,
                1.0,
                ParamInfoFlags::IS_AUTOMATABLE,
            ),
            1 => (
                PARAM_LATENCY,
                b"Latency",
                f64::from(MAX_LATENCY),
                0.0,
                ParamInfoFlags::IS_STEPPED,
            ),
            2 => (PARAM_WIGGLE, b"Wiggle", 2.0, 0.0, ParamInfoFlags::empty()),
            _ => return,
        };
        info.set(&ParamInfo {
            id: ClapId::new(id),
            flags,
            cookie: Default::default(),
            name,
            module: b"",
            min_value: 0.0,
            max_value: max,
            default_value: def,
        });
    }

    fn get_value(&self, id: ClapId) -> Option<f64> {
        self.shared.param_value(id.get())
    }

    fn value_to_text(
        &self,
        _id: ClapId,
        value: f64,
        writer: &mut ParamDisplayWriter,
    ) -> std::fmt::Result {
        write!(writer, "{value:.3}")
    }

    fn text_to_value(&self, _id: ClapId, text: &CStr) -> Option<f64> {
        text.to_str().ok()?.trim().parse().ok()
    }

    fn flush(&self, input: &InputEvents, output: &mut OutputEvents) {
        self.shared.read_params(input, false);
        self.shared.write_gesture(output);
    }
}

impl PluginStateImpl for MainThread<'_> {
    fn save(&self, output: &mut OutputStream) -> Result<(), PluginError> {
        let mut bytes = Vec::with_capacity(16);
        bytes.extend_from_slice(b"VZTG");
        bytes.extend_from_slice(&self.shared.gain().to_le_bytes());
        bytes.extend_from_slice(
            &self
                .shared
                .want_latency
                .load(Ordering::Relaxed)
                .to_le_bytes(),
        );
        output.write_all(&bytes)?;
        Ok(())
    }

    fn load(&self, input: &mut InputStream) -> Result<(), PluginError> {
        let mut bytes = Vec::new();
        input.read_to_end(&mut bytes)?;
        if bytes.len() != 16 || !bytes.starts_with(b"VZTG") {
            return Err(PluginError::Message("not a Test Gain state"));
        }
        let mut g = [0u8; 8];
        g.copy_from_slice(&bytes[4..12]);
        let mut l = [0u8; 4];
        l.copy_from_slice(&bytes[12..16]);
        self.shared.set_gain(f64::from_le_bytes(g));
        self.shared
            .set_param(PARAM_LATENCY, f64::from(u32::from_le_bytes(l)), false);
        Ok(())
    }
}

impl PluginLatencyImpl for MainThread<'_> {
    fn get(&self) -> u32 {
        self.shared.latency.load(Ordering::Relaxed)
    }
}

/// One voice of the synth.
#[derive(Clone, Copy, Default)]
struct Voice {
    key: Option<u8>,
    phase: f32,
}

pub struct Processor<'a> {
    shared: &'a Shared<'a>,
    rate: f32,
    /// Gain's delay lines (one per channel) and write position.
    delay: [Vec<f32>; 2],
    pos: usize,
    /// Copies of the inputs (main L/R, sidechain L/R).
    scratch: [Vec<f32>; 4],
    voices: [Voice; 4],
}

impl<'a> PluginAudioProcessor<'a, Shared<'a>, MainThread<'a>> for Processor<'a> {
    fn activate(
        _host: HostAudioProcessorHandle<'a>,
        _main: &MainThread<'a>,
        shared: &'a Shared<'a>,
        cfg: PluginAudioConfiguration,
    ) -> Result<Self, PluginError> {
        match shared.kind {
            Kind::Crasher => {
                // SAFETY: this is undefined behaviour by design: a deliberate
                // null write that raises SIGSEGV, for the scanner's crash
                // tests.
                unsafe { std::ptr::null_mut::<u8>().write_volatile(1) };
            }
            Kind::Hanger => loop {
                std::thread::sleep(std::time::Duration::from_secs(1));
            },
            _ => {}
        }
        let lat = shared.want_latency.load(Ordering::Relaxed);
        shared.latency.store(lat, Ordering::Relaxed);
        let max = cfg.max_frames_count as usize;
        Ok(Processor {
            shared,
            rate: cfg.sample_rate as f32,
            delay: [vec![0.0; lat as usize], vec![0.0; lat as usize]],
            pos: 0,
            scratch: [
                vec![0.0; max],
                vec![0.0; max],
                vec![0.0; max],
                vec![0.0; max],
            ],
            voices: [Voice::default(); 4],
        })
    }

    fn process(
        &mut self,
        _process: Process,
        mut audio: Audio,
        events: Events,
    ) -> Result<ProcessStatus, PluginError> {
        let n = audio.frames_count() as usize;
        if self.shared.kind == Kind::Synth4 {
            return self.synth(&mut audio, events, n);
        }
        self.shared.read_params(events.input, true);
        self.shared.write_gesture(events.output);
        // Copy the inputs first (the host may process in place).
        for (port, base) in [(0usize, 0usize), (1, 2)] {
            for ch in 0..2 {
                let dst = &mut self.scratch[base + ch][..n];
                match audio
                    .input_port(port)
                    .and_then(|p| p.channels().ok())
                    .and_then(|c| c.into_f32())
                    .and_then(|c| c.channel(ch as u32))
                {
                    Some(src) => dst.copy_from_slice(&src[..n]),
                    None => dst.fill(0.0),
                }
            }
        }
        let factor = match self.shared.kind {
            Kind::Gain => self.shared.gain() as f32,
            Kind::Ducker => {
                let peak = self.scratch[2][..n]
                    .iter()
                    .chain(&self.scratch[3][..n])
                    .fold(0.0f32, |m, x| m.max(x.abs()));
                1.0 - peak.min(1.0)
            }
            _ => 1.0,
        };
        let Some(mut out) = audio.output_port(0) else {
            return Ok(ProcessStatus::Continue);
        };
        let mut out = out
            .channels()?
            .into_f32()
            .ok_or(PluginError::Message("f32 only"))?;
        let lat = self.delay[0].len();
        for ch in 0..2usize {
            let Some(o) = out.channel_mut(ch as u32) else {
                continue;
            };
            let mut pos = self.pos;
            for (i, x) in o[..n].iter_mut().enumerate() {
                let v = self.scratch[ch][i] * factor;
                if lat == 0 {
                    *x = v;
                } else {
                    *x = self.delay[ch][pos];
                    self.delay[ch][pos] = v;
                    pos = (pos + 1) % lat;
                }
            }
            if self.shared.kind == Kind::Nan {
                if let Some(first) = o.first_mut() {
                    *first = f32::NAN;
                }
            }
        }
        if lat > 0 {
            self.pos = (self.pos + n) % lat;
        }
        Ok(ProcessStatus::Continue)
    }

    fn reset(&mut self) {
        for d in &mut self.delay {
            d.fill(0.0);
        }
        self.voices = [Voice::default(); 4];
    }
}

impl<'a> PluginAudioProcessorParams for Processor<'a> {
    fn flush(&mut self, input: &InputEvents, output: &mut OutputEvents) {
        self.shared.read_params(input, true);
        self.shared.write_gesture(output);
    }
}

impl Processor<'_> {
    /// Saw voices: channel k plays on output k; events are sample-exact.
    fn synth(
        &mut self,
        audio: &mut Audio,
        events: Events,
        n: usize,
    ) -> Result<ProcessStatus, PluginError> {
        let mut voices = self.voices;
        let rate = self.rate.max(1.0);
        // Render each output in turn, replaying the events per output.
        for (k, slot) in voices.iter_mut().enumerate() {
            let mut v = *slot;
            let mut buf = std::mem::take(&mut self.scratch[0]);
            let mut start = 0usize;
            let mut evs = events.input.iter().peekable();
            loop {
                let end = evs
                    .peek()
                    .map(|e| (e.header().time() as usize).min(n))
                    .unwrap_or(n);
                for x in &mut buf[start..end] {
                    *x = match v.key {
                        Some(key) => {
                            let f = 440.0 * 2f32.powf((f32::from(key) - 69.0) / 12.0);
                            v.phase = (v.phase + f / rate).fract();
                            0.25 * (2.0 * v.phase - 1.0)
                        }
                        None => 0.0,
                    };
                }
                start = end;
                let Some(e) = evs.next() else {
                    break;
                };
                let (chan, key, on) = match e.as_core_event() {
                    Some(CoreEventSpace::NoteOn(x)) => (
                        x.pckn().channel.into_specific(),
                        x.pckn().key.into_specific(),
                        true,
                    ),
                    Some(CoreEventSpace::NoteOff(x)) => (
                        x.pckn().channel.into_specific(),
                        x.pckn().key.into_specific(),
                        false,
                    ),
                    Some(CoreEventSpace::Midi(m)) => {
                        let [s, d1, d2] = m.data();
                        let on = s & 0xF0 == 0x90 && d2 > 0;
                        let off = s & 0xF0 == 0x80 || (s & 0xF0 == 0x90 && d2 == 0);
                        if !on && !off {
                            continue;
                        }
                        (Some(u16::from(s & 0x0F)), Some(u16::from(d1)), on)
                    }
                    _ => continue,
                };
                let (Some(chan), Some(key)) = (chan, key) else {
                    continue;
                };
                if usize::from(chan) % 4 != k {
                    continue;
                }
                let key = key.min(127) as u8;
                if on {
                    v = Voice {
                        key: Some(key),
                        phase: 0.0,
                    };
                } else if v.key == Some(key) {
                    v.key = None;
                }
            }
            *slot = v;
            if let Some(mut port) = audio.output_port(k) {
                if let Some(mut out) = port.channels()?.into_f32() {
                    for ch in 0..2 {
                        if let Some(o) = out.channel_mut(ch) {
                            o[..n].copy_from_slice(&buf[..n]);
                        }
                    }
                }
            }
            self.scratch[0] = buf;
        }
        self.voices = voices;
        Ok(ProcessStatus::Continue)
    }
}
