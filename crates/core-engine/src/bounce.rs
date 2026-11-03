//! The offline bounce (plan 5f-2 Task 2; spec §9.1; Rulings 4, 5, 8, 13;
//! pre-flight C1, I3, I8): renders a range of the loaded sequence with a
//! dedicated [`Engine`] and [`OfflineBackend`] on the calling thread, and
//! hands the mix and the stems, read from the renderer's per-run stem taps
//! and aligned by their arrivals in the PDC plan, to a sink, followed by a
//! tail that rings out to −96 dBFS.
//!
//! Nothing here runs on a real-time thread: `bounce` drives
//! [`OfflineBackend::block`] itself and reads the taps between blocks.

use crate::backend::{MidiOut, MidiPortInfo};
use crate::engine::{Engine, SinkFactory};
use crate::error::{BackendError, EngineError};
use crate::offline::OfflineBackend;
use crate::render::Sink;
use crate::schedule::{TargetIdx, TargetKind};
use crate::transport::PlayState;
use core_model::ids::StripId;
use core_model::mixer::{Mixer, Output};
use core_model::time::Tick;
use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, Ordering};

/// −96 dBFS (Ruling 4).
pub const TAIL_THRESHOLD: f32 = 1.584_893_2e-5;

/// The quiet run that ends a tail.
pub const TAIL_QUIET_SECONDS: f64 = 0.5;

/// Renderer frames a bounce waits for the transport to roll before it
/// gives up (the Play never arrived), in seconds.
const ROLL_WAIT_SECONDS: f64 = 10.0;

/// A stem gated as heard (Add to project, Ruling 5): `strip`'s output times
/// the audibility of every strip in `chain` (the strip, each bus its main
/// output feeds in turn, then master).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HeardStem {
    /// The strip whose post-fader output is the stem.
    pub strip: StripId,
    /// The strip, each bus its main output feeds in turn, then master.
    pub chain: Vec<StripId>,
}

impl HeardStem {
    /// `strip` and its main-output chain in `mixer` (cycle-safe: at most
    /// one visit per strip; an unknown strip's chain is itself and master).
    pub fn of(mixer: &Mixer, strip: StripId) -> HeardStem {
        let master = mixer.master();
        let mut chain = vec![strip];
        let mut at = strip;
        while at != master {
            let next = match mixer.strip(at).map(|s| s.output) {
                Ok(Output::Bus(b)) => b,
                Ok(Output::Master) | Err(_) => master,
            };
            if chain.contains(&next) {
                break;
            }
            chain.push(next);
            at = next;
        }
        if !chain.contains(&master) {
            chain.push(master);
        }
        HeardStem { strip, chain }
    }
}

/// What to bounce.
#[derive(Debug, Clone, PartialEq)]
pub struct BounceSpec {
    /// Render the mix (source 0 when set).
    pub mix: bool,
    /// Strips whose post-fader output is a stem, before mute and solo
    /// (export to files, Ruling 5), in order (after the mix).
    pub stems: Vec<StripId>,
    /// Stems gated as heard (Add to project, Ruling 5), in order (after `stems`).
    pub heard_stems: Vec<HeardStem>,
    /// The range's start.
    pub start: Tick,
    /// The range's end (exclusive).
    pub end: Tick,
    /// The longest tail, in frames.
    pub tail_cap: u64,
}

/// What a bounce rendered.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BounceStats {
    /// Frames every source's sink received (range + tail).
    pub frames: u64,
    /// The range's frames.
    pub range: u64,
    /// The tail's frames.
    pub tail: u64,
    /// Stream underruns while rendering (`Engine::faults` counters); 0 in Manual mode unless a read failed.
    pub underruns: u64,
    /// Non-finite samples silenced (Ruling 13).
    pub nonfinite: u64,
}

/// Why a bounce stopped early.
#[derive(Debug, thiserror::Error)]
pub enum BounceError {
    /// The cancel flag was set.
    #[error("cancelled")]
    Cancelled,
    /// The engine refused a command (or the backend has no renderer).
    #[error("{0}")]
    Engine(#[from] EngineError),
    /// The sink failed.
    #[error("{0}")]
    Sink(String),
    /// The range holds no frames.
    #[error("the range is empty")]
    Empty,
}

/// One source being bounced.
struct Source {
    /// Its tap in the block's `stem_taps` (`None`: not tapped, silent).
    tap: Option<usize>,
    /// Renderer frames before its timeline frame 0 (its arrival).
    skip: u64,
    /// The taps gating it, each with the arrival its gate acts at, and a
    /// cursor into that tap's gate history.
    gates: Vec<(Option<usize>, u64, usize)>,
    /// Its timeline frames not processed yet, from the next one on.
    queue: VecDeque<(f32, f32)>,
    /// Quiet frames after the last loud one, not final yet.
    held: (Vec<f32>, Vec<f32>),
    /// Final frames to hand on after this block.
    out: (Vec<f32>, Vec<f32>),
}

/// The audibility of a gate history at renderer frame `r`, moving `cur`
/// forward (frames are read in order). Not audible before its first entry.
fn gate_at(h: &[(u64, bool)], cur: &mut usize, r: u64) -> bool {
    while h.get(*cur + 1).is_some_and(|x| x.0 <= r) {
        *cur += 1;
    }
    h.get(*cur).is_some_and(|x| x.0 <= r && x.1)
}

/// Adds `(frame, on)` to a gate history when it changes it.
fn push_gate(h: &mut Vec<(u64, bool)>, frame: u64, on: bool) {
    match h.last_mut() {
        Some(last) if last.1 == on => {}
        Some(last) if last.0 == frame => last.1 = on,
        _ => h.push((frame, on)),
    }
}

/// A finite sample, or 0 (counted).
fn finite(x: f32, nonfinite: &mut u64) -> f32 {
    if x.is_finite() {
        x
    } else {
        *nonfinite += 1;
        0.0
    }
}

/// Renders `spec` with `engine`, whose renderer `out` runs, handing each source's
/// stereo frames to `sink(source, left, right)` in order. The sink receives
/// exactly `BounceStats::frames` frames per source (Ruling 4). Sets the
/// renderer's stem taps before the first block and clears them at the end.
// The sink's signature is the plan's interface.
#[allow(clippy::type_complexity)]
pub fn bounce(
    engine: &mut Engine,
    out: &mut OfflineBackend,
    spec: &BounceSpec,
    block: u32,
    sink: &mut dyn FnMut(usize, &[f32], &[f32]) -> Result<(), String>,
    progress: &mut dyn FnMut(f32),
    cancel: &AtomicBool,
) -> Result<BounceStats, BounceError> {
    if spec.end <= spec.start {
        return Err(BounceError::Empty);
    }
    let range = engine
        .tick_frame(spec.end)
        .saturating_sub(engine.tick_frame(spec.start));
    if range == 0 {
        return Err(BounceError::Empty);
    }
    let block = block.clamp(1, engine.config().max_frames.max(1));
    out.reserve(block);
    let mut strips: Vec<StripId> = spec.stems.clone();
    for h in &spec.heard_stems {
        strips.push(h.strip);
        strips.extend(h.chain.iter().copied());
    }
    // Master is always tapped, also for the mix alone.
    out.renderer_mut()
        .ok_or(EngineError::Backend(BackendError::NoDevice))?
        .tap_stems(&strips);
    let result = render(engine, out, spec, block, range, sink, progress, cancel);
    if let Some(r) = out.renderer_mut() {
        r.set_stem_taps(&[]);
    }
    // Leave the dedicated engine stopped, so the next bounce on it starts
    // from Stopped; a failed Stop changes nothing.
    let _ = engine.stop();
    result
}

/// [`bounce`] once the taps are set.
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn render(
    engine: &mut Engine,
    out: &mut OfflineBackend,
    spec: &BounceSpec,
    block: u32,
    range: u64,
    sink: &mut dyn FnMut(usize, &[f32], &[f32]) -> Result<(), String>,
    progress: &mut dyn FnMut(f32),
    cancel: &AtomicBool,
) -> Result<BounceStats, BounceError> {
    engine.locate_tick(spec.start)?;
    engine.set_loop(false)?;
    engine.play_until(spec.end)?;
    let rate = engine.config().sample_rate.max(1.0);
    let quiet_need = (TAIL_QUIET_SECONDS * rate).round() as u64;
    let roll_wait = (ROLL_WAIT_SECONDS * rate).round() as u64;
    let underruns = engine.streamer().counters().underruns;
    let total = range.saturating_add(spec.tail_cap).max(1) as f64;
    let blk = u64::from(block);

    let mut sources: Vec<Source> = Vec::new();
    // Per tap (by its index in `stem_taps`), its gate changes in absolute
    // renderer frames.
    let mut history: Vec<Vec<(u64, bool)>> = Vec::new();
    // The renderer frames counted from the first playing block, and the
    // frames waited before it.
    let (mut started, mut n, mut waited) = (false, 0u64, 0u64);
    // The latest renderer frame any source reads ahead of its timeline frame.
    let mut ahead = 0u64;
    let mut next_t = 0u64;
    let (mut quiet, mut last_loud, mut handed, mut nonfinite) = (0u64, 0u64, 0u64, 0u64);
    let mut frame: Vec<(f32, f32)> = Vec::new();
    let (mut resolved, mut done) = (false, false);
    while !done {
        if cancel.load(Ordering::Relaxed) {
            return Err(BounceError::Cancelled);
        }
        engine.streamer().service();
        out.block(block);
        if !started {
            // Playing, or already in its tail: a range shorter than a block
            // ends inside the first played block.
            if engine.state() == PlayState::Stopped {
                waited = waited.saturating_add(blk);
                if waited > roll_wait {
                    // The Play never reached the renderer.
                    return Err(BounceError::Engine(EngineError::Busy));
                }
                continue;
            }
            started = true;
        }
        let Some(r) = out.renderer_mut() else {
            return Err(BounceError::Engine(EngineError::Backend(
                BackendError::NoDevice,
            )));
        };
        let taps = r.stem_taps();
        if !resolved {
            resolved = true;
            // The sources, resolved once against the first playing block's
            // taps and arrivals (the graph does not change in a bounce).
            let find = |s: StripId| taps.iter().position(|t| t.strip == s);
            let arrival = |k: Option<usize>| k.and_then(|k| taps.get(k)).map_or(0, |t| t.arrival);
            let gate_arrival =
                |k: Option<usize>| k.and_then(|k| taps.get(k)).map_or(0, |t| t.gate_arrival);
            let held = (quiet_need as usize).saturating_add(block as usize);
            let mut add = |tap: Option<usize>, gates: Vec<Option<usize>>| {
                sources.push(Source {
                    tap,
                    skip: arrival(tap),
                    gates: gates.into_iter().map(|g| (g, gate_arrival(g), 0)).collect(),
                    queue: VecDeque::new(),
                    held: (Vec::with_capacity(held), Vec::with_capacity(held)),
                    out: (Vec::new(), Vec::new()),
                });
            };
            if spec.mix {
                // Master's tap is first.
                add(Some(0), vec![Some(0)]);
            }
            for s in &spec.stems {
                add(find(*s), Vec::new());
            }
            for h in &spec.heard_stems {
                add(find(h.strip), h.chain.iter().map(|s| find(*s)).collect());
            }
            ahead = sources
                .iter()
                .flat_map(|s| std::iter::once(s.skip).chain(s.gates.iter().map(|g| g.1)))
                .max()
                .unwrap_or(0);
            let room = (ahead as usize).saturating_add(2 * block as usize);
            for s in &mut sources {
                s.queue.reserve(room);
                s.held.0.reserve(ahead as usize);
                s.held.1.reserve(ahead as usize);
            }
            history = vec![Vec::new(); taps.len()];
            frame = vec![(0.0, 0.0); sources.len()];
        }
        // This block's gate changes; a block with no rendered run is not
        // audible from its start.
        for (h, t) in history.iter_mut().zip(&taps) {
            if t.gates.first().is_none_or(|g| g.0 > 0) {
                push_gate(h, n, false);
            }
            for g in t.gates {
                push_gate(h, n + u64::from(g.0), g.1);
            }
        }
        // This block's samples, at each source's timeline frames.
        for s in &mut sources {
            let tap = s.tap.and_then(|k| taps.get(k));
            for i in 0..blk {
                let at = n + i;
                if at < s.skip {
                    continue;
                }
                let x = tap.map_or((0.0, 0.0), |t| {
                    let i = i as usize;
                    (
                        t.left.get(i).copied().unwrap_or(0.0),
                        t.right.get(i).copied().unwrap_or(0.0),
                    )
                });
                s.queue.push_back(x);
            }
        }
        n += blk;
        // Every frame whose samples and gates are known, all sources at once.
        while !done && next_t.saturating_add(ahead) < n {
            let t = next_t;
            let mut loud = false;
            for (s, f) in sources.iter_mut().zip(frame.iter_mut()) {
                let (l, r) = s.queue.pop_front().unwrap_or((0.0, 0.0));
                let (l, r) = (finite(l, &mut nonfinite), finite(r, &mut nonfinite));
                let mut on = true;
                for (g, at, cur) in &mut s.gates {
                    let h = g
                        .and_then(|k| history.get(k))
                        .map_or(&[][..], Vec::as_slice);
                    on &= gate_at(h, cur, t + *at);
                }
                *f = if on { (l, r) } else { (0.0, 0.0) };
                loud |= f.0.abs() >= TAIL_THRESHOLD || f.1.abs() >= TAIL_THRESHOLD;
            }
            if t < range {
                for (s, f) in sources.iter_mut().zip(&frame) {
                    s.out.0.push(f.0);
                    s.out.1.push(f.1);
                }
            } else if loud {
                for (s, f) in sources.iter_mut().zip(&frame) {
                    s.out.0.append(&mut s.held.0);
                    s.out.1.append(&mut s.held.1);
                    s.out.0.push(f.0);
                    s.out.1.push(f.1);
                }
                last_loud = t + 1 - range;
                quiet = 0;
            } else {
                for (s, f) in sources.iter_mut().zip(&frame) {
                    s.held.0.push(f.0);
                    s.held.1.push(f.1);
                }
                quiet += 1;
            }
            next_t += 1;
            if next_t >= range && (next_t - range >= spec.tail_cap || quiet >= quiet_need) {
                done = true;
            }
        }
        let mut handed_now = 0;
        for (k, s) in sources.iter_mut().enumerate() {
            if !s.out.0.is_empty() {
                handed_now = s.out.0.len() as u64;
                sink(k, &s.out.0, &s.out.1).map_err(BounceError::Sink)?;
                s.out.0.clear();
                s.out.1.clear();
            }
        }
        handed += handed_now;
        progress((handed as f64 / total).min(1.0) as f32);
    }
    progress(1.0);
    let tail = last_loud.min(spec.tail_cap);
    Ok(BounceStats {
        frames: range + tail,
        range,
        tail,
        underruns: engine
            .streamer()
            .counters()
            .underruns
            .saturating_sub(underruns),
        nonfinite,
    })
}

/// A MIDI sink that drops every message (Ruling 8).
#[derive(Debug)]
struct Dropped;

impl MidiOut for Dropped {
    // RT
    fn send(&mut self, _: u64, _: &[u8]) -> bool {
        true
    }
}

/// Wraps a sink factory so that no MIDI leaves a bounce (Ruling 8): MIDI
/// destinations and the virtual source get a sink that drops every message.
#[derive(Debug)]
pub struct SilentMidi<F>(pub F);

impl<F: SinkFactory> SinkFactory for SilentMidi<F> {
    fn ports(&self) -> Vec<MidiPortInfo> {
        self.0.ports()
    }

    fn make(
        &mut self,
        target: TargetIdx,
        kind: &TargetKind,
        sample_rate: f64,
        max_frames: u32,
    ) -> Result<Sink, BackendError> {
        match kind {
            TargetKind::MidiDestination(_) | TargetKind::MidiVirtualSource => {
                Ok(Sink::Midi(Box::new(Dropped)))
            }
            _ => self.0.make(target, kind, sample_rate, max_frames),
        }
    }
}
