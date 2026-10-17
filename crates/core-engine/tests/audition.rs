//! Editor audition (spec §3.2): a note straight to a destination's target.

use core_engine::BackendError;
use core_engine::backend::{BlockTime, InstrumentNode, MidiPortInfo, StereoBuf};
use core_engine::engine::{Engine, EngineConfig, SinkFactory};
use core_engine::fixtures::{Builder, Q};
use core_engine::render::{Renderer, Sink};
use core_engine::schedule::{TargetIdx, TargetKind};
use core_model::document::Document;
use core_model::ids::{DestId, SeqId};
use std::sync::{Arc, Mutex};

type Seen = Arc<Mutex<Vec<(u64, Vec<u8>)>>>;

struct Node {
    seen: Seen,
    rendered: u64,
}

impl InstrumentNode for Node {
    fn midi(&mut self, offset: u32, msg: &[u8]) {
        self.seen
            .lock()
            .unwrap()
            .push((self.rendered + u64::from(offset), msg.to_vec()));
    }
    fn render(&mut self, frames: u32, out: &mut StereoBuf) {
        out.left[..frames as usize].fill(0.0);
        out.right[..frames as usize].fill(0.0);
        self.rendered += u64::from(frames);
    }
    fn reset(&mut self) {}
}

#[derive(Default, Clone)]
struct Factory {
    synth: Seen,
}

impl SinkFactory for Factory {
    fn ports(&self) -> Vec<MidiPortInfo> {
        vec![]
    }
    fn make(
        &mut self,
        _target: TargetIdx,
        _kind: &TargetKind,
        _sr: f64,
        _max: u32,
    ) -> Result<Sink, BackendError> {
        Ok(Sink::Instrument(Box::new(Node {
            seen: self.synth.clone(),
            rendered: 0,
        })))
    }
}

/// "Synth" (channel index 2) is used by the sequence; "Virtual" plays on
/// the virtual source, a target this sequence never loads.
fn doc() -> (Document, SeqId, DestId, DestId) {
    let mut b = Builder::new();
    let synth = b.dest("Synth", "builtin:synth", 2).unwrap();
    let unused = b.dest("Virtual", "virtual:out", 5).unwrap();
    let (seq, lane) = b.sequence("S", "L", Some(synth)).unwrap();
    b.notes(seq, lane, &[(0, 60, 0x8000, Q)]).unwrap();
    (b.finish(), seq, synth, unused)
}

fn blocks(r: &mut Renderer, n: usize, ns: &mut u64) {
    let mut l = vec![0.0f32; 256];
    let mut rr = vec![0.0f32; 256];
    for _ in 0..n {
        let mut chans: [&mut [f32]; 2] = [&mut l, &mut rr];
        r.process(
            &mut chans,
            256,
            BlockTime {
                host_time_ns: *ns,
                sample_rate: 48_000.0,
            },
        );
        *ns += 256 * 1_000_000_000 / 48_000;
    }
}

#[test]
fn an_audition_note_sounds_and_stop_releases_it() {
    let (doc, seq, synth, _) = doc();
    let f = Factory::default();
    let (mut engine, mut r) =
        Engine::new(EngineConfig::new(48_000.0), Box::new(f.clone())).unwrap();
    engine.load(&doc, seq).unwrap();
    let mut ns = 0;
    blocks(&mut r, 1, &mut ns);
    assert!(engine.audition(&doc, synth, 64, 0x8000, true).unwrap());
    blocks(&mut r, 1, &mut ns);
    engine.stop().unwrap();
    blocks(&mut r, 1, &mut ns);
    let seen = f.synth.lock().unwrap().clone();
    assert!(seen.contains(&(256, vec![0x92, 64, 64])), "{seen:?}");
    assert!(
        seen.contains(&(512, vec![0x82, 64, 0])),
        "stop releases: {seen:?}"
    );
}

#[test]
fn an_audition_note_off_ends_the_note() {
    let (doc, seq, synth, _) = doc();
    let f = Factory::default();
    let (mut engine, mut r) =
        Engine::new(EngineConfig::new(48_000.0), Box::new(f.clone())).unwrap();
    engine.load(&doc, seq).unwrap();
    let mut ns = 0;
    engine.audition(&doc, synth, 64, 0x8000, true).unwrap();
    blocks(&mut r, 1, &mut ns);
    engine.audition(&doc, synth, 64, 0x2000, false).unwrap();
    blocks(&mut r, 1, &mut ns);
    engine.stop().unwrap();
    blocks(&mut r, 1, &mut ns);
    let seen = f.synth.lock().unwrap().clone();
    let offs = seen
        .iter()
        .filter(|(_, b)| b.len() == 3 && b[0] == 0x82 && b[1] == 64)
        .count();
    assert_eq!(offs, 1, "released once, not again at stop: {seen:?}");
}

#[test]
fn destinations_without_a_loaded_target_are_not_auditioned() {
    let (doc, seq, _, unused) = doc();
    let (mut engine, _r) =
        Engine::new(EngineConfig::new(48_000.0), Box::new(Factory::default())).unwrap();
    engine.load(&doc, seq).unwrap();
    assert!(!engine.audition(&doc, unused, 64, 0x8000, true).unwrap());
    assert!(
        !engine
            .audition(&doc, DestId(9_999), 64, 0x8000, true)
            .unwrap()
    );
}
