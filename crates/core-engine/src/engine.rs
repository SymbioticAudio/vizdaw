//! The control handle (non-RT): compiles documents, sends commands to the
//! renderer and reads what it publishes.

use crate::backend::MidiPortInfo;
use crate::error::{BackendError, EngineError};
use crate::render::{
    CMD_CAPACITY, Counters, EngineCmd, Faults, RETIRE_CAPACITY, RenderConfig, Renderer, Shared,
    Sink,
};
use crate::schedule::{
    BuiltinRoute, CompileOptions, CompileReport, MAX_TARGETS, Schedule, TargetIdx, TargetKind,
    TargetTable, TempoTable, compile,
};
use crate::transport::PlayState;
use core_model::document::Document;
use core_model::ids::SeqId;
use core_model::time::Tick;
use rtrb::{Producer, RingBuffer};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::JoinHandle;
use std::time::Duration;

/// Creates sinks for new targets (control thread).
pub trait SinkFactory: Send {
    /// MIDI destinations that exist now.
    fn ports(&self) -> Vec<MidiPortInfo>;
    /// Creates the sink of target `target` (the index the engine gives
    /// it) at the stream's rate and block size.
    fn make(
        &mut self,
        target: TargetIdx,
        kind: &TargetKind,
        sample_rate: f64,
        max_frames: u32,
    ) -> Result<Sink, BackendError>;
}

/// Engine settings.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EngineConfig {
    /// Stream sample rate.
    pub sample_rate: f64,
    /// Largest block the renderer accepts.
    pub max_frames: u32,
    /// Added to MIDI-out timestamps (CLI `--midi-offset-ms`).
    pub midi_offset_ns: i64,
    /// Where `builtin:synth` destinations play.
    pub route: BuiltinRoute,
}

impl EngineConfig {
    /// Defaults: 4096-frame blocks, no MIDI offset, built-in routing.
    pub fn new(sample_rate: f64) -> EngineConfig {
        EngineConfig {
            sample_rate,
            max_frames: crate::backend::MAX_FRAMES,
            midi_offset_ns: 0,
            route: BuiltinRoute::Builtin,
        }
    }
}

/// The control handle.
pub struct Engine {
    cfg: EngineConfig,
    cmds: Producer<EngineCmd>,
    shared: Arc<Shared>,
    targets: TargetTable,
    factory: Box<dyn SinkFactory>,
    tempo: Option<TempoTable>,
    length_frames: u64,
    loop_range: Option<(u64, u64)>,
    /// The default loop (bar 1 to the end) is on: loads re-send it.
    default_loop: bool,
    collector_stop: Arc<AtomicBool>,
    collector_paused: Arc<AtomicBool>,
    collector: Option<JoinHandle<()>>,
}

impl std::fmt::Debug for Engine {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Engine")
            .field("cfg", &self.cfg)
            .field("targets", &self.targets)
            .finish_non_exhaustive()
    }
}

impl Engine {
    /// Creates the control handle and its renderer, and starts the collector
    /// thread that frees retired schedules every 10 ms. Fails when that
    /// thread cannot be started (ruling O24): without it retired schedules
    /// would never be freed.
    pub fn new(
        cfg: EngineConfig,
        factory: Box<dyn SinkFactory>,
    ) -> Result<(Engine, Renderer), EngineError> {
        Engine::with_spawner(cfg, factory, |b, f| b.spawn(f))
    }

    /// [`Engine::new`] with the thread spawn injected (tests make it fail).
    fn with_spawner(
        cfg: EngineConfig,
        factory: Box<dyn SinkFactory>,
        spawn: impl FnOnce(
            std::thread::Builder,
            Box<dyn FnOnce() + Send + 'static>,
        ) -> std::io::Result<JoinHandle<()>>,
    ) -> Result<(Engine, Renderer), EngineError> {
        let (cmd_tx, cmd_rx) = RingBuffer::<EngineCmd>::new(CMD_CAPACITY);
        let (retire_tx, mut retire_rx) = RingBuffer::<Box<Schedule>>::new(RETIRE_CAPACITY);
        let shared = Arc::new(Shared::default());
        let collector_stop = Arc::new(AtomicBool::new(false));
        let collector_paused = Arc::new(AtomicBool::new(false));
        let stop = Arc::clone(&collector_stop);
        let paused = Arc::clone(&collector_paused);
        const COLLECTOR: &str = "vizdaw-collector";
        let collector = spawn(
            std::thread::Builder::new().name(COLLECTOR.into()),
            Box::new(move || {
                loop {
                    while !paused.load(Ordering::Relaxed) {
                        let Ok(old) = retire_rx.pop() else {
                            break;
                        };
                        drop(old);
                    }
                    if stop.load(Ordering::Relaxed) {
                        break;
                    }
                    std::thread::sleep(Duration::from_millis(10));
                }
            }),
        )
        .map_err(|e| EngineError::Thread {
            name: COLLECTOR,
            reason: e.to_string(),
        })?;
        let renderer = Renderer::new(
            RenderConfig {
                sample_rate: cfg.sample_rate,
                max_frames: cfg.max_frames,
                midi_offset_ns: cfg.midi_offset_ns,
            },
            cmd_rx,
            retire_tx,
            Arc::clone(&shared),
        );
        let engine = Engine {
            cfg,
            cmds: cmd_tx,
            shared,
            targets: TargetTable::new(),
            factory,
            tempo: None,
            length_frames: 0,
            loop_range: None,
            default_loop: false,
            collector_stop,
            collector_paused,
            collector: Some(collector),
        };
        Ok((engine, renderer))
    }

    /// Pauses or resumes freeing retired schedules (a diagnostic hook:
    /// tests use it to fill the retire ring).
    pub fn pause_collector(&self, paused: bool) {
        self.collector_paused.store(paused, Ordering::Relaxed);
    }

    /// Settings.
    pub fn config(&self) -> EngineConfig {
        self.cfg
    }

    /// Targets created so far.
    pub fn targets(&self) -> &TargetTable {
        &self.targets
    }

    /// Compiles `seq` and sends the schedule (and sinks for new targets) to
    /// the renderer. Call again after every committed edit.
    pub fn load(&mut self, doc: &Document, seq: SeqId) -> Result<CompileReport, EngineError> {
        // Room for every possible new sink plus the schedule and the loop,
        // so no push below can fail after targets were interned.
        if self.cmds.slots() < MAX_TARGETS + 2 {
            return Err(EngineError::Busy);
        }
        let opts = CompileOptions {
            sample_rate: self.cfg.sample_rate,
            route: self.cfg.route,
            ports: self.factory.ports(),
        };
        let (sr, max) = (self.cfg.sample_rate, self.cfg.max_frames);
        let before = self.targets.len();
        let mut new_sinks: Vec<(TargetIdx, Sink)> = Vec::new();
        let targets = &mut self.targets;
        let factory = &mut self.factory;
        let mut resolve = |kind: &TargetKind| -> Result<Result<TargetIdx, String>, EngineError> {
            if let Some(i) = targets.find(kind) {
                return Ok(Ok(i));
            }
            if targets.len() >= MAX_TARGETS {
                return Err(EngineError::TooManyTargets);
            }
            // `intern` appends, so the new target gets the next index.
            let next = u8::try_from(targets.len()).map_err(|_| EngineError::TooManyTargets)?;
            match factory.make(TargetIdx(next), kind, sr, max) {
                Ok(sink) => {
                    let i = targets.intern(kind.clone())?;
                    new_sinks.push((i, sink));
                    Ok(Ok(i))
                }
                Err(e) => Ok(Err(e.to_string())),
            }
        };
        let compiled = compile(doc, seq, &opts, &mut resolve);
        let (schedule, report) = match compiled {
            Ok(c) => c,
            Err(e) => {
                // Targets interned by this load have no sinks yet; forget
                // them (their sinks drop with `new_sinks`).
                self.targets.truncate(before);
                return Err(e);
            }
        };
        for (i, sink) in new_sinks {
            self.push(EngineCmd::AddSink(i, sink))?;
        }
        self.tempo = Some(schedule.tempo.clone());
        self.length_frames = schedule.length_frames;
        self.loop_range = schedule.loop_range;
        self.push(EngineCmd::SetSchedule(Box::new(schedule)))?;
        if self.default_loop {
            // The length may have changed; the mapped old range would not
            // follow it.
            self.push(EngineCmd::SetLoop(self.loop_range))?;
        }
        Ok(report)
    }

    fn push(&mut self, cmd: EngineCmd) -> Result<(), EngineError> {
        self.cmds.push(cmd).map_err(|_| EngineError::Busy)
    }

    /// Starts playback (from the start when at or past the end).
    pub fn play(&mut self) -> Result<(), EngineError> {
        self.push(EngineCmd::Play)
    }

    /// Stops playback; sounding notes get note-offs.
    pub fn stop(&mut self) -> Result<(), EngineError> {
        self.push(EngineCmd::Stop)
    }

    /// Moves to a schedule frame.
    pub fn locate_frame(&mut self, frame: u64) -> Result<(), EngineError> {
        self.push(EngineCmd::Locate(frame))
    }

    /// Moves to a tick of the loaded sequence.
    pub fn locate_tick(&mut self, tick: Tick) -> Result<(), EngineError> {
        let frame = self.tempo.as_ref().map_or(0, |t| t.tick_to_frame(tick));
        self.locate_frame(frame)
    }

    /// Loops bar 1 to the end of the loaded sequence, or stops looping.
    pub fn set_loop(&mut self, on: bool) -> Result<(), EngineError> {
        let range = if on { self.loop_range } else { None };
        self.push(EngineCmd::SetLoop(range))?;
        self.default_loop = on;
        Ok(())
    }

    /// Loops an explicit frame range, or stops looping.
    pub fn set_loop_range(&mut self, range: Option<(u64, u64)>) -> Result<(), EngineError> {
        self.push(EngineCmd::SetLoop(range))?;
        self.default_loop = false;
        Ok(())
    }

    /// Schedule frame of the transport.
    pub fn position_frames(&self) -> u64 {
        self.shared.position()
    }

    /// Tick of the transport, through the last loaded schedule's tempo table.
    pub fn position_tick(&self) -> Tick {
        let f = self.shared.position();
        self.tempo
            .as_ref()
            .map_or(Tick::ZERO, |t| t.frame_to_tick(f))
    }

    /// Frame of the loaded sequence's end.
    pub fn length_frames(&self) -> u64 {
        self.length_frames
    }

    /// Transport state.
    pub fn state(&self) -> PlayState {
        self.shared.state()
    }

    /// Reads and clears the fault bits.
    pub fn faults(&self) -> Faults {
        self.shared.take_faults()
    }

    /// Current counters.
    pub fn counters(&self) -> Counters {
        self.shared.counters()
    }

    /// True once after playback reached the end and the tail finished.
    pub fn take_ended(&self) -> bool {
        self.shared.take_ended()
    }
}

impl Drop for Engine {
    fn drop(&mut self) {
        self.collector_stop.store(true, Ordering::Relaxed);
        if let Some(h) = self.collector.take() {
            let _ = h.join();
        }
    }
}

/// Finds a sequence by name.
pub fn find_sequence(doc: &Document, name: &str) -> Result<SeqId, EngineError> {
    doc.sequences()
        .values()
        .find(|s| s.name == name)
        .map(|s| s.id)
        .ok_or_else(|| EngineError::UnknownSequence(name.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::offline::OfflineSinks;

    #[test]
    fn new_fails_when_the_collector_thread_cannot_start() {
        let r = Engine::with_spawner(
            EngineConfig::new(48_000.0),
            Box::new(OfflineSinks::new(4)),
            |_, _| Err(std::io::Error::other("no threads")),
        );
        match r {
            Err(EngineError::Thread { name, .. }) => assert_eq!(name, "vizdaw-collector"),
            other => panic!("expected a thread error, got {other:?}"),
        }
    }

    #[test]
    fn new_succeeds_normally() {
        let (engine, _r) =
            Engine::new(EngineConfig::new(48_000.0), Box::new(OfflineSinks::new(4))).unwrap();
        assert!(engine.targets().is_empty());
    }
}
