//! Live gestures (plan 5e-1 Tasks 5–6; sub-project 5 spec §7.1, §7.2): a
//! touched target's value overrides its automation at once, glides back,
//! holds until a wrap, a Locate or Stop, trims in dB; a touched plugin
//! parameter gets no automation and gets it again once released; the
//! gesture tap stamps run frames, marks and says when it rolled.

use core_engine::backend::AudioBackend;
use core_engine::backend::StereoBuf;
use core_engine::engine::{Engine, EngineConfig};
use core_engine::fixtures::{Builder, Q};
use core_engine::gesture::{
    GESTURE_CAPACITY, GESTURES_PER_BLOCK, GestureMsg, GesturePhase, MAX_LIVE,
};
use core_engine::graph::{EffectNode, InsertFactory, ProcessStatus, TestInserts};
use core_engine::offline::{OfflineBackend, OfflineSinks};
use core_engine::schedule::{AutoDest, StripParam};
use core_model::automation::{AutomationTarget, fader_norm};
use core_model::document::Document;
use core_model::event::EventBody;
use core_model::ids::{PluginInstanceId, SeqId, StripId};
use core_model::mixer::{ParamId, PluginSlot, StripKind};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

const BLOCK: u32 = 512;

/// A plugin whose last automated value is kept in `.0` (`f64` bits; NaN
/// none yet) and whose automation calls are counted in `.1`.
struct Rec(Arc<AtomicU64>, Arc<AtomicU64>, Box<dyn EffectNode>);

impl EffectNode for Rec {
    fn latency(&self) -> u32 {
        self.2.latency()
    }
    fn process(
        &mut self,
        frames: u32,
        io: &mut StereoBuf,
        sc: Option<&StereoBuf>,
    ) -> ProcessStatus {
        self.2.process(frames, io, sc)
    }
    fn reset(&mut self) {
        self.2.reset();
    }
    fn automate(&mut self, _frame: u32, _param: ParamId, v: f64) -> bool {
        self.0.store(v.to_bits(), Ordering::Relaxed);
        self.1.fetch_add(1, Ordering::Relaxed);
        true
    }
}

struct RecInserts(TestInserts, Arc<AtomicU64>, Arc<AtomicU64>);

impl InsertFactory for RecInserts {
    fn make(
        &mut self,
        slot: &PluginSlot,
        sr: f64,
        max: u32,
    ) -> Result<Box<dyn EffectNode>, String> {
        Ok(Box::new(Rec(
            Arc::clone(&self.1),
            Arc::clone(&self.2),
            self.0.make(slot, sr, max)?,
        )))
    }
}

struct Rig {
    engine: Engine,
    out: OfflineBackend,
    synth: StripId,
    bus: StripId,
    insert: PluginInstanceId,
    last: Arc<AtomicU64>,
    sent: Arc<AtomicU64>,
}

fn rig() -> Rig {
    rig_with(false)
}

/// The built-in synth plays a long note; its fader is automated at −12 dB
/// from 0; a bus without automation; an insert on the bus whose parameter
/// 0 ramps 0 → 1 over 16 beats, or (`flat`) stays at 0.3 (one point at 0).
fn rig_with(flat: bool) -> Rig {
    let mut b = Builder::new();
    let d = b.dest("Synth", "builtin:synth", 0).unwrap();
    let (seq, notes) = b.sequence("S", "Notes", Some(d)).unwrap();
    b.notes(seq, notes, &[(0, 60, 0x8000, 64 * Q)]).unwrap();
    let synth = b.strip_of(StripKind::BuiltinSynth).unwrap();
    let bus = b.bus("Bus").unwrap();
    let insert = b.insert(bus, TestInserts::plugin("gain=1")).unwrap();
    let auto = b.lane(seq, "Auto", None).unwrap();
    let fader = AutomationTarget::StripFader(synth);
    let param = AutomationTarget::PluginParam(insert, ParamId(0));
    b.event(
        seq,
        auto,
        0,
        EventBody::Automation {
            target: fader,
            value: fader_norm(-12.0),
        },
    )
    .unwrap();
    if flat {
        b.event(
            seq,
            auto,
            0,
            EventBody::Automation {
                target: param,
                value: 0.3,
            },
        )
        .unwrap();
    } else {
        b.event(
            seq,
            auto,
            0,
            EventBody::Automation {
                target: param,
                value: 0.0,
            },
        )
        .unwrap();
        b.event(
            seq,
            auto,
            16 * Q,
            EventBody::Automation {
                target: param,
                value: 1.0,
            },
        )
        .unwrap();
    }
    let doc = b.finish();
    rig_of(&doc, seq, synth, bus, insert)
}

fn rig_of(
    doc: &Document,
    seq: SeqId,
    synth: StripId,
    bus: StripId,
    insert: PluginInstanceId,
) -> Rig {
    let cfg = EngineConfig::new(48_000.0);
    let (mut engine, renderer) =
        Engine::new(cfg, Box::new(OfflineSinks::new(1 << 16).with_tone())).unwrap();
    let last = Arc::new(AtomicU64::new(f64::NAN.to_bits()));
    let sent = Arc::new(AtomicU64::new(0));
    engine.set_insert_factory(Box::new(RecInserts(
        TestInserts::new(),
        Arc::clone(&last),
        Arc::clone(&sent),
    )));
    engine.load(doc, seq).unwrap();
    let mut out = OfflineBackend::new(48_000.0, BLOCK);
    out.start(renderer).unwrap();
    out.reserve(4096);
    Rig {
        engine,
        out,
        synth,
        bus,
        insert,
        last,
        sent,
    }
}

impl Rig {
    fn blocks(&mut self, n: usize) {
        for _ in 0..n {
            self.out.block(BLOCK);
        }
    }
    fn send(&self, dest: AutoDest, phase: GesturePhase) {
        assert!(self.engine.gesture_sender().send(GestureMsg {
            dest,
            phase,
            host_ns: 0,
            tag: 0
        }));
    }
    fn fader_db(&self, s: StripId) -> Option<f64> {
        self.engine.overrides()?.fader_db(s)
    }
    fn last(&self) -> f64 {
        f64::from_bits(self.last.load(Ordering::Relaxed))
    }
    fn sent(&self) -> u64 {
        self.sent.load(Ordering::Relaxed)
    }
}

fn near(a: Option<f64>, b: f64) -> bool {
    a.is_some_and(|a| (a - b).abs() < 1e-6)
}

fn fader(s: StripId) -> AutoDest {
    AutoDest::Strip(s, StripParam::Fader)
}

/// Spec §7.2: while touched the live value wins over the automation; on
/// release (Read) it snaps back.
#[test]
fn a_touched_fader_wins_over_its_automation_and_snaps_back() {
    let mut r = rig();
    r.engine.play().unwrap();
    r.blocks(4);
    assert!(
        near(r.fader_db(r.synth), -12.0),
        "{:?}",
        r.fader_db(r.synth)
    );
    r.send(fader(r.synth), GesturePhase::Touch(fader_norm(-3.0)));
    r.blocks(1);
    assert!(near(r.fader_db(r.synth), -3.0), "{:?}", r.fader_db(r.synth));
    r.send(fader(r.synth), GesturePhase::Value(fader_norm(-6.0)));
    r.blocks(1);
    assert!(near(r.fader_db(r.synth), -6.0));
    r.send(fader(r.synth), GesturePhase::Release);
    r.blocks(1);
    assert!(near(r.fader_db(r.synth), -12.0));
}

/// Plan 5e-1 Ruling 10: Touch's release glides back over the return time;
/// a glide of 0 frames is a release.
#[test]
fn a_glide_returns_to_the_automation_over_its_frames() {
    let mut r = rig();
    r.engine.play().unwrap();
    r.blocks(2);
    r.send(fader(r.synth), GesturePhase::Touch(fader_norm(-3.0)));
    r.blocks(1);
    r.send(fader(r.synth), GesturePhase::Glide(8 * BLOCK));
    r.blocks(2);
    let mid = r.fader_db(r.synth).unwrap();
    assert!(mid < -3.0 - 1e-6 && mid > -12.0 + 1e-6, "{mid}");
    r.blocks(8);
    assert!(near(r.fader_db(r.synth), -12.0));
    r.send(fader(r.synth), GesturePhase::Touch(fader_norm(0.0)));
    r.send(fader(r.synth), GesturePhase::Glide(0));
    r.blocks(1);
    assert!(near(r.fader_db(r.synth), -12.0), "a 0-frame glide releases");
}

/// Ruling 10: Latch and Write hold until the next loop wrap or Stop.
#[test]
fn held_values_last_until_a_wrap_or_a_stop() {
    let mut r = rig();
    r.engine
        .set_loop_range(Some((0, 16 * u64::from(BLOCK))))
        .unwrap();
    r.engine.play().unwrap();
    r.blocks(2);
    r.send(fader(r.synth), GesturePhase::Touch(fader_norm(-3.0)));
    r.send(fader(r.synth), GesturePhase::Hold);
    r.blocks(4);
    assert!(near(r.fader_db(r.synth), -3.0), "held after the release");
    r.blocks(12);
    assert!(near(r.fader_db(r.synth), -12.0), "the wrap ended the hold");
    r.send(fader(r.synth), GesturePhase::Touch(fader_norm(-3.0)));
    r.send(fader(r.synth), GesturePhase::Hold);
    r.blocks(1);
    r.engine.stop().unwrap();
    r.blocks(2);
    r.engine.locate_frame(0).unwrap();
    r.engine.play().unwrap();
    r.blocks(2);
    assert!(near(r.fader_db(r.synth), -12.0), "the Stop ended the hold");
}

/// Spec §7.2: a gesture while stopped is heard at once; on a control no
/// automation drives, its release gives the static value back (no
/// override left).
#[test]
fn a_touch_while_stopped_is_heard_at_once() {
    let mut r = rig();
    r.blocks(2);
    assert_eq!(r.fader_db(r.bus), None);
    r.send(fader(r.bus), GesturePhase::Touch(fader_norm(-3.0)));
    r.blocks(1);
    assert!(near(r.fader_db(r.bus), -3.0));
    // The snapshot in automation's units, as the session reads it for a
    // touch with no value (Ruling 26).
    let snap = r.engine.overrides().unwrap();
    assert!(
        near(snap.value(fader(r.bus)), fader_norm(-3.0)),
        "{:?}",
        snap.value(fader(r.bus))
    );
    assert_eq!(snap.value(AutoDest::Plugin(r.insert, ParamId(0))), None);
    r.send(fader(r.bus), GesturePhase::Release);
    r.blocks(1);
    assert_eq!(r.fader_db(r.bus), None);
}

/// Ruling 11: Trim plays automation plus the offset, in dB for a fader.
#[test]
fn an_offset_trims_the_automation_in_db() {
    let mut r = rig();
    r.engine.play().unwrap();
    r.blocks(2);
    r.send(fader(r.synth), GesturePhase::Touch(fader_norm(-12.0)));
    r.send(fader(r.synth), GesturePhase::Offset(4.0));
    r.blocks(1);
    assert!(near(r.fader_db(r.synth), -8.0), "{:?}", r.fader_db(r.synth));
}

/// Ruling 6: a touched plugin parameter gets no automation (the plugin
/// keeps the user's value); released, its automation is sent again.
#[test]
fn plugin_lanes_pause_while_touched() {
    let mut r = rig();
    let dest = AutoDest::Plugin(r.insert, ParamId(0));
    r.engine.play().unwrap();
    r.blocks(8);
    let before = r.last();
    assert!(before > 0.0 && before < 1.0, "{before}");
    r.send(dest, GesturePhase::Touch(0.9));
    r.blocks(1);
    let touched = r.last();
    r.blocks(16);
    assert_eq!(
        r.last().to_bits(),
        touched.to_bits(),
        "no automation while touched"
    );
    r.send(dest, GesturePhase::Release);
    r.blocks(2);
    assert!(r.last() > touched, "the ramp is sent again");
}

/// Pre-flight I10, Ruling 10: a released plugin parameter whose lane is
/// flat is sent its automation again (the renderer had not sent it while
/// it equalled the last value sent).
#[test]
fn a_released_plugin_parameter_gets_flat_automation_again() {
    let mut r = rig_with(true);
    let dest = AutoDest::Plugin(r.insert, ParamId(0));
    r.engine.play().unwrap();
    r.blocks(4);
    assert!((r.last() - 0.3).abs() < 1e-9, "{}", r.last());
    r.send(dest, GesturePhase::Touch(0.9));
    r.blocks(4);
    let during = r.sent();
    r.send(dest, GesturePhase::Release);
    r.blocks(2);
    assert!(r.sent() > during, "sent again after the release");
    assert!((r.last() - 0.3).abs() < 1e-9, "{}", r.last());
    // More plugin entries end in one block than the renderer keeps (64):
    // the real one ends last, after 64 others, and is still reset.
    for k in 0..64u64 {
        r.send(
            AutoDest::Plugin(r.insert, ParamId(100 + k)),
            GesturePhase::Touch(0.5),
        );
    }
    r.send(dest, GesturePhase::Touch(0.9));
    r.blocks(2);
    let during = r.sent();
    for k in 0..64u64 {
        r.send(
            AutoDest::Plugin(r.insert, ParamId(100 + k)),
            GesturePhase::Release,
        );
    }
    r.send(dest, GesturePhase::Release);
    r.blocks(2);
    assert!(r.sent() > during, "the overflow resets every plugin lane");
}

/// Ruling 6 (pre-flight I3): a sender waits for another sender instead of
/// dropping; only a full queue refuses, and that is counted.
#[test]
fn a_full_queue_refuses_and_counts() {
    let r = rig();
    let m = GestureMsg {
        dest: fader(r.bus),
        phase: GesturePhase::Value(0.5),
        host_ns: 0,
        tag: 0,
    };
    let (tx, tx2) = (r.engine.gesture_sender(), r.engine.gesture_sender());
    // Two threads send at once into a queue nothing drains (no block runs).
    let other = std::thread::spawn(move || (0..GESTURE_CAPACITY / 2).all(|_| tx2.send(m)));
    let mine = (0..GESTURE_CAPACITY / 2).all(|_| tx.send(m));
    assert!(
        other.join().unwrap() && mine,
        "no send is refused while the queue has room"
    );
    assert_eq!(r.engine.gestures_dropped(), 0);
    assert!(!tx.send(m), "the queue is full");
    assert_eq!(r.engine.gestures_dropped(), 1);
}

/// Ruling 23: a Locate ends held values, as a wrap does.
#[test]
fn a_locate_ends_held_values() {
    let mut r = rig();
    r.engine.play().unwrap();
    r.blocks(2);
    r.send(fader(r.synth), GesturePhase::Touch(fader_norm(-3.0)));
    r.send(fader(r.synth), GesturePhase::Hold);
    r.blocks(2);
    assert!(near(r.fader_db(r.synth), -3.0), "held");
    r.engine.locate_frame(0).unwrap();
    r.blocks(2);
    assert!(
        near(r.fader_db(r.synth), -12.0),
        "the Locate ended the hold"
    );
}

/// Ruling 6: at most `MAX_LIVE` live targets; the next touch is dropped
/// and counted, and the others still work.
#[test]
fn a_full_live_table_drops_and_counts() {
    let mut r = rig();
    for k in 0..=MAX_LIVE as u64 {
        r.send(
            AutoDest::Plugin(r.insert, ParamId(100 + k)),
            GesturePhase::Touch(0.5),
        );
    }
    // 256 messages are taken per block.
    r.blocks(MAX_LIVE / GESTURES_PER_BLOCK + 2);
    assert_eq!(r.engine.gestures_dropped(), 1);
    r.send(fader(r.bus), GesturePhase::Touch(fader_norm(-3.0)));
    r.blocks(1);
    assert_eq!(r.fader_db(r.bus), None, "the table is still full");
    r.send(
        AutoDest::Plugin(r.insert, ParamId(100)),
        GesturePhase::Release,
    );
    r.send(fader(r.bus), GesturePhase::Touch(fader_norm(-3.0)));
    r.blocks(1);
    assert!(near(r.fader_db(r.bus), -3.0), "a freed entry is reused");
}

/// Task 5 review I1: a Trim held over a Stop keeps playing its offset over
/// the automation that was heard, not over the mixer's static value.
#[test]
fn a_trim_held_over_a_stop_keeps_its_level() {
    let mut r = rig();
    r.engine.play().unwrap();
    r.blocks(2);
    r.send(fader(r.synth), GesturePhase::Offset(4.0));
    r.blocks(1);
    assert!(near(r.fader_db(r.synth), -8.0), "{:?}", r.fader_db(r.synth));
    r.engine.stop().unwrap();
    r.blocks(2);
    assert!(
        near(r.fader_db(r.synth), -8.0),
        "held over the Stop: {:?}",
        r.fader_db(r.synth)
    );
}

/// The peaks of the first and second halves of the last block.
fn halves(r: &Rig) -> (f32, f32) {
    let (l, _) = r.out.output(BLOCK);
    let h = l.len() / 2;
    let peak = |s: &[f32]| s.iter().fold(0f32, |m, x| m.max(x.abs()));
    (peak(&l[..h]), peak(&l[h..]))
}

/// Task 5 review I2: riding a fader ramps each new value over a run, as
/// automation does, instead of stepping at the run's start.
#[test]
fn riding_a_fader_ramps_each_value() {
    let mut r = rig();
    r.engine.play().unwrap();
    r.blocks(8);
    r.send(fader(r.synth), GesturePhase::Touch(fader_norm(-12.0)));
    r.blocks(2);
    let (a, b) = halves(&r);
    assert!(
        a > 0.0 && (a / b - 1.0).abs() < 0.05,
        "a steady tone: {a} {b}"
    );
    for db in [-6.0, 0.0] {
        r.send(fader(r.synth), GesturePhase::Value(fader_norm(db)));
        r.blocks(1);
        let (a, b) = halves(&r);
        assert!(
            a < 0.85 * b,
            "the move to {db} dB ramps over the block: {a} {b}"
        );
    }
    r.blocks(1);
    let (a, b) = halves(&r);
    assert!((a / b - 1.0).abs() < 0.05, "settled: {a} {b}");
}

/// Controller ruling on Task 5 fix round 1: a release returns to the
/// automation over one run (at once to the ear) instead of stepping.
#[test]
fn a_release_ramps_back_over_a_run() {
    let mut r = rig();
    r.engine.play().unwrap();
    r.blocks(8);
    r.send(fader(r.synth), GesturePhase::Touch(fader_norm(0.0)));
    r.blocks(2);
    r.send(fader(r.synth), GesturePhase::Release);
    r.blocks(1);
    let (a, b) = halves(&r);
    assert!(
        a > 1.5 * b,
        "the return to -12 dB ramps over the block: {a} {b}"
    );
    assert!(
        near(r.fader_db(r.synth), -12.0),
        "{:?}",
        r.fader_db(r.synth)
    );
    r.blocks(1);
    let (a, b) = halves(&r);
    assert!((a / b - 1.0).abs() < 0.05, "settled: {a} {b}");
}
