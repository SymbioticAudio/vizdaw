//! Drives `Renderer::process` directly with test sinks (the offline backend
//! comes later); each sink records `(block, offset or ns, bytes)`.

use core_engine::BackendError;
use core_engine::EngineError;
use core_engine::backend::{BlockTime, InstrumentNode, MidiOut, MidiPortInfo, StereoBuf};
use core_engine::engine::{Engine, EngineConfig, SinkFactory, find_sequence};
use core_engine::fixtures::{Builder, Q};
use core_engine::render::{Faults, Renderer, Sink};
use core_engine::schedule::{BuiltinRoute, TargetKind};
use core_engine::transport::PlayState;
use core_model::document::Document;
use core_model::ids::SeqId;
use std::sync::{Arc, Mutex};

type Seen = Arc<Mutex<Vec<(u64, Vec<u8>)>>>;

/// Records `(block_start + offset, bytes)` and renders a constant level.
struct Node {
    seen: Seen,
    rendered: u64,
    level: f32,
}

impl InstrumentNode for Node {
    fn midi(&mut self, offset: u32, msg: &[u8]) {
        self.seen
            .lock()
            .unwrap()
            .push((self.rendered + u64::from(offset), msg.to_vec()));
    }
    fn render(&mut self, frames: u32, out: &mut StereoBuf) {
        out.left[..frames as usize].fill(self.level);
        out.right[..frames as usize].fill(-self.level);
        self.rendered += u64::from(frames);
    }
    fn reset(&mut self) {}
}

/// Records `(host_time_ns, bytes)`.
struct Out {
    seen: Seen,
}

impl MidiOut for Out {
    fn send(&mut self, host_time_ns: u64, msg: &[u8]) -> bool {
        self.seen.lock().unwrap().push((host_time_ns, msg.to_vec()));
        true
    }
}

#[derive(Default, Clone)]
struct Factory {
    synth: Seen,
    midi: Seen,
    level: f32,
}

impl SinkFactory for Factory {
    fn ports(&self) -> Vec<MidiPortInfo> {
        vec![]
    }
    fn make(&mut self, kind: &TargetKind, _sr: f64, _max: u32) -> Result<Sink, BackendError> {
        Ok(match kind {
            TargetKind::BuiltinSynth => Sink::Instrument(Box::new(Node {
                seen: self.synth.clone(),
                rendered: 0,
                level: self.level,
            })),
            _ => Sink::Midi(Box::new(Out {
                seen: self.midi.clone(),
            })),
        })
    }
}

fn doc() -> (Document, SeqId) {
    let mut b = Builder::new();
    let d = b.dest("Synth", "builtin:synth", 0).unwrap();
    let (seq, lane) = b.sequence("S", "L", Some(d)).unwrap();
    b.notes(seq, lane, &[(0, 60, 0x8000, Q), (Q, 62, 0x8000, 2 * Q)])
        .unwrap();
    (b.finish(), seq)
}

/// Renders `n` blocks of `frames`; returns the last block's left channel.
fn blocks(r: &mut Renderer, n: usize, frames: u32, start_ns: &mut u64) -> Vec<f32> {
    let mut l = vec![0.0f32; frames as usize];
    let mut rr = vec![0.0f32; frames as usize];
    for _ in 0..n {
        let mut chans: [&mut [f32]; 2] = [&mut l, &mut rr];
        r.process(
            &mut chans,
            frames,
            BlockTime {
                host_time_ns: *start_ns,
                sample_rate: 48_000.0,
            },
        );
        *start_ns += u64::from(frames) * 1_000_000_000 / 48_000;
    }
    l
}

#[test]
fn events_land_at_their_offsets_and_stop_releases() {
    let (doc, seq) = doc();
    let f = Factory {
        level: 0.25,
        ..Factory::default()
    };
    let (mut engine, mut r) = Engine::new(EngineConfig::new(48_000.0), Box::new(f.clone()));
    let report = engine.load(&doc, seq).unwrap();
    assert_eq!(report.events, 2);
    engine.play().unwrap();
    let mut ns = 0;
    let out = blocks(&mut r, 50, 1000, &mut ns); // 50 000 frames
    assert_eq!(out[0], 0.25, "instrument output is mixed into channel 0");
    assert_eq!(engine.state(), PlayState::Playing);
    assert_eq!(engine.position_frames(), 50_000);
    engine.stop().unwrap();
    blocks(&mut r, 1, 1000, &mut ns);
    assert_eq!(engine.state(), PlayState::Stopped);
    let seen = f.synth.lock().unwrap().clone();
    assert_eq!(
        seen,
        vec![
            (0, vec![0x90, 60, 64]),
            (24_000, vec![0x80, 60, 0]),
            (24_000, vec![0x90, 62, 64]),
            (50_000, vec![0x80, 62, 0]), // cut by stop
        ]
    );
}

#[test]
fn midi_outs_get_host_times() {
    let (doc, seq) = doc();
    let f = Factory::default();
    let mut cfg = EngineConfig::new(48_000.0);
    cfg.route = BuiltinRoute::Virtual;
    cfg.midi_offset_ns = 1_000;
    let (mut engine, mut r) = Engine::new(cfg, Box::new(f.clone()));
    engine.load(&doc, seq).unwrap();
    engine.play().unwrap();
    let mut ns = 5_000_000_000;
    blocks(&mut r, 30, 1000, &mut ns);
    let seen = f.midi.lock().unwrap().clone();
    // Frame 24000 is block 24 offset 0: 5 s + 24 × 1000 frames × 20833 ns.
    assert_eq!(seen[0], (5_000_000_000 + 1_000, vec![0x90, 60, 64]));
    assert_eq!(
        seen[1],
        (5_000_000_000 + 24 * 20_833_333 + 1_000, vec![0x80, 60, 0])
    );
}

#[test]
fn clipping_and_xruns_are_faults_that_clear_on_read() {
    let (doc, seq) = doc();
    let f = Factory {
        level: 2.0,
        ..Factory::default()
    };
    let (mut engine, mut r) = Engine::new(EngineConfig::new(48_000.0), Box::new(f));
    engine.load(&doc, seq).unwrap();
    let mut ns = 0;
    let out = blocks(&mut r, 1, 64, &mut ns);
    assert_eq!(out[0], 1.0, "hard clamp at +1.0");
    assert!(engine.faults().contains(Faults::CLIPPED));
    assert!(engine.faults().is_empty());
    blocks(&mut r, 1, 5000, &mut ns);
    let f = engine.faults();
    assert!(f.contains(Faults::XRUN));
    assert_eq!(f.names(), vec!["xrun"]);
    assert_eq!(engine.counters().xruns, 1);
}

#[test]
fn non_finite_samples_are_silenced_and_flagged() {
    for level in [f32::NAN, f32::INFINITY] {
        let (doc, seq) = doc();
        let f = Factory {
            level,
            ..Factory::default()
        };
        let (mut engine, mut r) = Engine::new(EngineConfig::new(48_000.0), Box::new(f));
        engine.load(&doc, seq).unwrap();
        let mut l = vec![0.5f32; 64];
        let mut rr = vec![0.5f32; 64];
        let mut chans: [&mut [f32]; 2] = [&mut l, &mut rr];
        r.process(
            &mut chans,
            64,
            core_engine::backend::BlockTime {
                host_time_ns: 0,
                sample_rate: 48_000.0,
            },
        );
        // Left gets `level`, right gets `-level`.
        assert!(l.iter().all(|x| *x == 0.0), "left {level}: {:?}", &l[..4]);
        assert!(
            rr.iter().all(|x| *x == 0.0),
            "right {level}: {:?}",
            &rr[..4]
        );
        assert!(engine.faults().contains(Faults::CLIPPED), "{level}");
    }
}

#[test]
fn loop_and_locate_by_tick() {
    let (doc, seq) = doc();
    let f = Factory::default();
    let (mut engine, mut r) = Engine::new(EngineConfig::new(48_000.0), Box::new(f.clone()));
    engine.load(&doc, seq).unwrap();
    assert_eq!(engine.length_frames(), 96_000);
    engine.set_loop(true).unwrap();
    engine.locate_tick(core_model::time::Tick(3 * Q)).unwrap();
    engine.play().unwrap();
    let mut ns = 0;
    blocks(&mut r, 30, 1000, &mut ns); // 72000 → 96000 wraps to 0 → 6000
    assert_eq!(engine.position_frames(), 6_000);
    assert_eq!(engine.position_tick(), core_model::time::Tick(3_840));
    let ons = f
        .synth
        .lock()
        .unwrap()
        .iter()
        .filter(|(_, b)| b[0] == 0x90)
        .count();
    assert_eq!(ons, 1, "note 60 replayed after the wrap");
}

#[test]
fn engine_errors() {
    let (doc, _) = doc();
    assert_eq!(
        find_sequence(&doc, "Nope").unwrap_err(),
        EngineError::UnknownSequence("Nope".into())
    );
    let (mut engine, _r) = Engine::new(EngineConfig::new(48_000.0), Box::new(Factory::default()));
    assert!(matches!(
        engine.load(&doc, SeqId(99)),
        Err(EngineError::Model(_))
    ));
    // The command ring holds 64 commands; nothing drains it here.
    let mut busy = false;
    for _ in 0..100 {
        if engine.play() == Err(EngineError::Busy) {
            busy = true;
            break;
        }
    }
    assert!(busy);
}
