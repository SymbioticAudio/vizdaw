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

fn drain(
    h: &mut core_engine::gesture::GestureHandles,
) -> (
    Vec<core_engine::record::Mark>,
    Vec<core_engine::gesture::Stamped>,
) {
    let mut marks = Vec::new();
    while let Ok(m) = h.marks.pop() {
        marks.push(m);
    }
    let mut stamps = Vec::new();
    while let Ok(s) = h.stamps.pop() {
        stamps.push(s);
    }
    (marks, stamps)
}

/// Ruling 7: a gesture with no host time is stamped at the run frame of
/// the block that took it; the roll, a wrap and the Stop are marked as
/// the record tap marks them; nothing is stamped while stopped; the
/// status says when the tap has rolled (Ruling 24).
#[test]
fn stamps_carry_the_run_frame_of_the_block_that_took_them() {
    let mut r = rig();
    let mut h = r.engine.start_gestures().unwrap();
    let d = fader(r.synth);
    r.send(d, GesturePhase::Touch(0.5));
    r.blocks(1);
    assert!(drain(&mut h).1.is_empty(), "stopped: nothing is stamped");
    assert!(!h.status.rolled(), "not rolled while stopped");
    let lp = 16 * u64::from(BLOCK);
    r.engine.set_loop_range(Some((0, lp))).unwrap();
    r.engine.play().unwrap();
    r.blocks(10);
    assert!(h.status.rolled(), "rolled once a block played");
    r.send(d, GesturePhase::Value(0.6));
    r.blocks(1);
    r.send(d, GesturePhase::Glide(100));
    r.blocks(10);
    r.engine.stop().unwrap();
    r.blocks(1);
    r.send(d, GesturePhase::Touch(0.1));
    r.blocks(1);
    let (marks, stamps) = drain(&mut h);
    // The Play is applied at the block after the call, so the roll is the
    // first block that played: run 0 plays frame 0.
    assert_eq!((marks[0].run, marks[0].frame, marks[0].wrap), (0, 0, false));
    assert!(
        marks.iter().any(|m| m.wrap && m.run == lp && m.frame == 0),
        "{marks:?}"
    );
    assert!(!marks.last().unwrap().wrap, "the Stop is marked last");
    let runs: Vec<(i64, GesturePhase)> = stamps.iter().map(|s| (s.run, s.phase)).collect();
    assert_eq!(
        runs,
        vec![
            (10 * i64::from(BLOCK), GesturePhase::Value(0.6)),
            (11 * i64::from(BLOCK), GesturePhase::Glide(100)),
        ]
    );
    assert!(stamps.iter().all(|s| s.dest == d && s.tag == 0));
    assert_eq!(h.status.run(), 21 * u64::from(BLOCK));
    r.engine.stop_gestures().unwrap();
    r.blocks(1);
    assert!(h.status.closed());
}

/// The latent session of the arrival tests below: the synth's strip
/// carries an insert of latency `A` (its fader acts `A` frames behind the
/// renderer) and an idle bus one of `2 A`, so the graph latency is `2 A`
/// and the synth's audio waits `A` more on its way to the master.
const A: u64 = 1004;

/// The synth's fader ramps from 0 dB at frame `at` to −20 dB 100 frames
/// later (`at` a multiple of 25 frames: a whole tick at 120 BPM).
fn latent_doc(at: Option<u64>) -> (Document, SeqId, StripId) {
    let mut b = Builder::new();
    let d = b.dest("Synth", "builtin:synth", 0).unwrap();
    let (seq, notes) = b.sequence("S", "Notes", Some(d)).unwrap();
    b.notes(seq, notes, &[(0, 60, 0x8000, 64 * Q)]).unwrap();
    let synth = b.strip_of(StripKind::BuiltinSynth).unwrap();
    b.insert(synth, TestInserts::plugin(&format!("latency={A}")))
        .unwrap();
    let bus = b.bus("Latent").unwrap();
    b.insert(bus, TestInserts::plugin(&format!("latency={}", 2 * A)))
        .unwrap();
    if let Some(at) = at {
        assert_eq!(at % 25, 0, "a whole tick");
        let auto = b.lane(seq, "Auto", None).unwrap();
        let tick = |f: u64| (f as i64) * Q / 24_000;
        for (f, db) in [(at, 0.0), (at + 100, -20.0)] {
            b.event(
                seq,
                auto,
                tick(f),
                EventBody::Automation {
                    target: AutomationTarget::StripFader(synth),
                    value: fader_norm(db),
                },
            )
            .unwrap();
        }
    }
    (b.finish(), seq, synth)
}

/// What a render of `doc` gave: the master's left channel, and the
/// gesture tap's marks and stamps.
type Heard = (
    Vec<f32>,
    Vec<core_engine::record::Mark>,
    Vec<core_engine::gesture::Stamped>,
);

/// `blocks` blocks of `doc` from frame 0 (looping `lp` when given), the
/// synth's fader touched at −20 dB before block `touch` when given.
fn render_latent(
    doc: &Document,
    seq: SeqId,
    synth: StripId,
    lp: Option<u64>,
    touch: Option<usize>,
    blocks: usize,
) -> Heard {
    let (mut engine, renderer) = Engine::new(
        EngineConfig::new(48_000.0),
        Box::new(OfflineSinks::new(1 << 16).with_tone()),
    )
    .unwrap();
    engine.set_insert_factory(Box::new(TestInserts::new()));
    engine.load(doc, seq).unwrap();
    let mut out = OfflineBackend::new(48_000.0, BLOCK);
    out.start(renderer).unwrap();
    out.reserve(4096);
    let mut h = engine.start_gestures().unwrap();
    if lp.is_some() {
        engine.set_loop_range(lp.map(|e| (0, e))).unwrap();
    }
    engine.play().unwrap();
    let mut left = Vec::new();
    for k in 0..blocks {
        if touch == Some(k) {
            assert!(engine.gesture_sender().send(GestureMsg {
                dest: fader(synth),
                phase: GesturePhase::Touch(fader_norm(-20.0)),
                host_ns: 0,
                tag: 0,
            }));
        }
        out.block(BLOCK);
        left.extend_from_slice(out.output(BLOCK).0);
    }
    let (marks, stamps) = drain(&mut h);
    (left, marks, stamps)
}

/// Where the placement (the record tap's mapping, plan 5c) puts a stamp:
/// the timeline heard at run `u` is `S(u − L)`, `S` mapping output runs
/// to schedule frames through the marks.
fn placed(marks: &[core_engine::record::Mark], u: i64) -> i64 {
    let m = marks
        .iter()
        .rev()
        .find(|m| (m.run as i64) <= u - m.latency as i64)
        .unwrap_or(&marks[0]);
    m.frame as i64 + (u - m.latency as i64 - m.run as i64)
}

/// The first frame where `a` and `b` differ.
fn first_change(a: &[f32], b: &[f32]) -> usize {
    a.iter().zip(b).position(|(x, y)| x != y).unwrap()
}

/// Task 5 review M3: a live value applies at the strip in the run being
/// processed, whose lane reads `A` frames behind the transport; the stamp
/// maps the gesture to that frame, so the automation written from it
/// plays back exactly where the touch was heard (not `L − A` early).
#[test]
fn a_written_gesture_plays_back_where_it_was_heard_on_a_latent_strip() {
    let (doc, seq, synth) = latent_doc(None);
    let blocks = 40;
    let k = 17;
    let (dry, _, _) = render_latent(&doc, seq, synth, None, None, blocks);
    let (live, marks, stamps) = render_latent(&doc, seq, synth, None, Some(k), blocks);
    assert_eq!(marks[0].latency, 2 * A, "the graph latency");
    assert_eq!(stamps.len(), 1, "{stamps:?}");
    let block = (k as u64 * u64::from(BLOCK)) as i64;
    assert_eq!(
        stamps[0].run,
        block + A as i64,
        "heard when the strip's change reaches the master"
    );
    let at = placed(&marks, stamps[0].run);
    assert_eq!(at, block - A as i64, "the frame the strip's lane read");
    let (doc2, seq2, synth2) = latent_doc(Some(at as u64));
    let (played, _, _) = render_latent(&doc2, seq2, synth2, None, None, blocks);
    let heard = first_change(&dry, &live);
    assert_eq!(heard as i64, block + A as i64 + 1, "the live touch's ramp");
    assert_eq!(
        first_change(&dry, &played),
        heard,
        "the written automation is heard where the touch was"
    );
    // Both stand at −20 dB once their ramps are done.
    let from = heard + 600;
    for (i, (x, y)) in live[from..].iter().zip(&played[from..]).enumerate() {
        assert!((x - y).abs() < 1e-6, "frame {}: {x} {y}", from + i);
    }
}

/// Task 4's loop seam: just after a wrap, a strip `A` frames behind still
/// renders the end of the previous pass; a gesture taken there is
/// stamped into that pass, at the frame the lane read, and its written
/// automation is heard where the touch was.
#[test]
fn a_gesture_just_after_a_wrap_maps_to_the_pass_its_strip_rendered() {
    let (doc, seq, synth) = latent_doc(None);
    let lp = 16 * u64::from(BLOCK);
    let blocks = 24;
    let k = 17;
    let (dry, _, _) = render_latent(&doc, seq, synth, Some(lp), None, blocks);
    let (live, marks, stamps) = render_latent(&doc, seq, synth, Some(lp), Some(k), blocks);
    assert!(marks.iter().any(|m| m.wrap && m.run == lp), "{marks:?}");
    assert_eq!(stamps.len(), 1, "{stamps:?}");
    let block = k as i64 * i64::from(BLOCK);
    let pos = block - lp as i64;
    assert!(pos < A as i64, "the strip still reads before the seam");
    let at = placed(&marks, stamps[0].run);
    assert_eq!(
        at,
        lp as i64 - (A as i64 - pos),
        "the previous pass's end, where the lane read"
    );
    let (doc2, seq2, synth2) = latent_doc(Some(at as u64));
    let (played, _, _) = render_latent(&doc2, seq2, synth2, Some(lp), None, blocks);
    let heard = first_change(&dry, &live);
    assert_eq!(heard as i64, block + A as i64 + 1, "the live touch's ramp");
    assert_eq!(
        first_change(&dry, &played),
        heard,
        "the written automation is heard where the touch was"
    );
}

/// The built-in synth plays through its strip at a static 0 dB; with
/// `lane`, its fader is automated at −12 dB from 0.
fn plain_doc(lane: bool) -> (Document, SeqId, StripId, core_model::ids::DestId) {
    let mut b = Builder::new();
    let d = b.dest("Synth", "builtin:synth", 0).unwrap();
    let (seq, notes) = b.sequence("S", "Notes", Some(d)).unwrap();
    b.notes(seq, notes, &[(0, 60, 0x8000, 64 * Q)]).unwrap();
    let synth = b.strip_of(StripKind::BuiltinSynth).unwrap();
    let auto = b.lane(seq, "Auto", None).unwrap();
    if lane {
        b.event(
            seq,
            auto,
            0,
            EventBody::Automation {
                target: AutomationTarget::StripFader(synth),
                value: fader_norm(-12.0),
            },
        )
        .unwrap();
    }
    (b.finish(), seq, synth, d)
}

/// Final review m2 (Task 5 re-review N2): a Trim offset held while
/// stopped on a fader no lane drives plays over the static value; when a
/// lane appears under it (still stopped) it keeps playing over the static
/// value, instead of over its own output (the offset counted twice).
#[test]
fn a_held_trim_keeps_its_level_when_a_lane_appears_while_stopped() {
    let (doc, seq, synth, _) = plain_doc(false);
    let mut r = rig_of(&doc, seq, synth, synth, PluginInstanceId(0));
    r.blocks(2);
    r.send(fader(synth), GesturePhase::Offset(4.0));
    r.blocks(2);
    assert!(near(r.fader_db(synth), 4.0), "{:?}", r.fader_db(synth));
    let (with_lane, _, _, _) = plain_doc(true);
    r.engine.load(&with_lane, seq).unwrap();
    r.blocks(4);
    assert!(
        near(r.fader_db(synth), 4.0),
        "the offset counts once: {:?}",
        r.fader_db(synth)
    );
}

/// Final review m2 (Task 5 re-review N2): a touch right after a Locate
/// ended a hold, in the same block, starts from the held value still
/// heard, not from the static value under it (no dip). A noise insert on
/// a bus no lane drives sounds while stopped.
#[test]
fn a_touch_right_after_a_hold_ends_starts_from_what_is_heard() {
    let mut b = Builder::new();
    let d = b.dest("Synth", "builtin:synth", 0).unwrap();
    let (seq, _) = b.sequence("S", "Notes", Some(d)).unwrap();
    let bus = b.bus("Bus").unwrap();
    let insert = b.insert(bus, TestInserts::plugin("noise=7")).unwrap();
    let doc = b.finish();
    let mut r = rig_of(&doc, seq, bus, bus, insert);
    r.blocks(4);
    r.send(fader(bus), GesturePhase::Touch(fader_norm(-20.0)));
    r.send(fader(bus), GesturePhase::Hold);
    r.blocks(3);
    let (a, b) = halves(&r);
    assert!(a > 0.0 && a < 1.5 * b && b < 1.5 * a, "held: {a} {b}");
    r.engine.locate_frame(0).unwrap();
    r.send(fader(bus), GesturePhase::Touch(fader_norm(-20.0)));
    r.blocks(1);
    let (a, b) = halves(&r);
    assert!(a < 1.5 * b, "no dip: {a} {b}");
}

/// Final review m3 (controller ruling 1 on the Task 5 forward): released
/// while stopped, a fader its lane drives stays at the value the user
/// left (the session makes it the static value) until Play, instead of
/// returning to the automation value heard when it was touched.
#[test]
fn a_release_while_stopped_keeps_the_value_left() {
    let mut r = rig();
    r.engine.play().unwrap();
    r.blocks(4);
    r.engine.stop().unwrap();
    r.blocks(2);
    assert!(
        near(r.fader_db(r.synth), -12.0),
        "{:?}",
        r.fader_db(r.synth)
    );
    r.send(fader(r.synth), GesturePhase::Touch(fader_norm(-3.0)));
    r.blocks(1);
    assert!(near(r.fader_db(r.synth), -3.0));
    r.send(fader(r.synth), GesturePhase::Release);
    r.blocks(3);
    assert!(
        near(r.fader_db(r.synth), -3.0),
        "stays while stopped: {:?}",
        r.fader_db(r.synth)
    );
    r.engine.play().unwrap();
    r.blocks(2);
    assert!(near(r.fader_db(r.synth), -12.0), "the automation at Play");
}

/// Task 5 re-review N3: a Trim offset taken while stopped, on a fader its
/// lane drove before the Stop, plays over the automation value still
/// heard (the lane reads nothing while stopped), not over the static
/// value.
#[test]
fn a_trim_while_stopped_plays_over_the_heard_automation() {
    let mut r = rig();
    r.engine.play().unwrap();
    r.blocks(4);
    r.engine.stop().unwrap();
    r.blocks(2);
    r.send(fader(r.synth), GesturePhase::Offset(4.0));
    r.blocks(4);
    assert!(near(r.fader_db(r.synth), -8.0), "{:?}", r.fader_db(r.synth));
}
