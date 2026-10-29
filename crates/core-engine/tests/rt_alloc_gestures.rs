//! Gestures never allocate on the render path (plan 5e-1 Tasks 5–6; spec
//! §3, §12.3 "`rt-check` covers … gesture overrides"): the counting
//! allocator is armed only around `OfflineBackend::block` while every
//! gesture phase arrives (strip, send and plugin targets, a full table, a
//! full queue, glides, holds, offsets, released plugin lanes), across loop
//! wraps, a Locate, a Stop and a graph whose latency changes (so arrivals
//! change), and (Task 6) while the gesture tap is installed, stamps and is
//! replaced and removed.

use core_engine::backend::AudioBackend;
use core_engine::engine::{Engine, EngineConfig};
use core_engine::fixtures::{Builder, Q};
use core_engine::gesture::{GESTURE_CAPACITY, GestureMsg, GesturePhase, MAX_LIVE};
use core_engine::graph::TestInserts;
use core_engine::offline::{OfflineBackend, OfflineSinks};
use core_engine::rt_check::{self, CountingAlloc};
use core_engine::schedule::{AutoDest, StripParam};
use core_model::automation::{AutomationTarget, SendIndex, fader_norm};
use core_model::command::MixerCmd;
use core_model::event::EventBody;
use core_model::ids::PluginInstanceId;
use core_model::mixer::{AuxSend, ParamId, StripKind};

#[global_allocator]
static ALLOC: CountingAlloc = CountingAlloc;

fn armed_block(b: &mut OfflineBackend, frames: u32) {
    rt_check::arm();
    b.block(frames);
    rt_check::disarm();
}

#[test]
fn gestures_never_allocate() {
    let mut bld = Builder::new();
    let synth_dest = bld.dest("Synth", "builtin:synth", 0).unwrap();
    let (seq, notes) = bld.sequence("S", "Notes", Some(synth_dest)).unwrap();
    bld.notes(seq, notes, &[(0, 60, 0x8000, 64 * Q)]).unwrap();
    let synth = bld.strip_of(StripKind::BuiltinSynth).unwrap();
    let bus = bld.bus("Bus").unwrap();
    bld.mixer(MixerCmd::AddSend {
        strip: synth,
        send: AuxSend::new(bus),
    })
    .unwrap();
    let insert = bld.insert(bus, TestInserts::plugin("latency=64")).unwrap();
    let auto = bld.lane(seq, "Auto", None).unwrap();
    for (t, target, v) in [
        (0, AutomationTarget::StripFader(synth), fader_norm(-12.0)),
        (8 * Q, AutomationTarget::StripFader(synth), fader_norm(0.0)),
        (0, AutomationTarget::SendLevel(synth, SendIndex(0)), 0.5),
        (0, AutomationTarget::PluginParam(insert, ParamId(0)), 0.0),
        (
            8 * Q,
            AutomationTarget::PluginParam(insert, ParamId(0)),
            1.0,
        ),
        (Q, AutomationTarget::StripMute(bus), 0.0),
    ] {
        bld.event(seq, auto, t, EventBody::Automation { target, value: v })
            .unwrap();
    }
    let doc = bld.finish();
    let mut lb = Builder::from_doc(doc.clone());
    let master = lb.strip_of(StripKind::Master).unwrap();
    lb.insert(master, TestInserts::plugin("latency=480"))
        .unwrap();
    let latent = lb.finish();
    let (mut engine, renderer) = Engine::new(
        EngineConfig::new(48_000.0),
        Box::new(OfflineSinks::new(1 << 20).with_tone()),
    )
    .unwrap();
    engine.set_insert_factory(Box::new(TestInserts::new()));
    engine.load(&doc, seq).unwrap();
    let tx = engine.gesture_sender();
    let mut b = OfflineBackend::new(48_000.0, 512);
    b.start(renderer).unwrap();
    b.reserve(1 << 16);
    let send = |dest: AutoDest, phase: GesturePhase| {
        let _ = tx.send(GestureMsg {
            dest,
            phase,
            host_ns: 0,
            tag: 1,
        });
    };
    let dests = [
        AutoDest::Strip(synth, StripParam::Fader),
        AutoDest::Strip(bus, StripParam::Pan),
        AutoDest::Strip(bus, StripParam::Mute),
        AutoDest::Send(synth, 0),
        AutoDest::Plugin(insert, ParamId(0)),
    ];
    // Warm up outside the count.
    engine.set_loop_range(Some((0, 4 * 24_000))).unwrap();
    engine.play().unwrap();
    for _ in 0..8 {
        b.block(512);
    }
    let mut handles = engine.start_gestures().unwrap();
    for _ in 0..2 {
        b.block(512);
    }
    let before = rt_check::count();
    for round in 0..6u32 {
        if round == 1 {
            // A full queue (pre-flight M11): sent outside the count; what
            // does not fit is dropped. Drained 256 a block.
            for _ in 0..GESTURE_CAPACITY + 16 {
                send(dests[0], GesturePhase::Value(0.5));
            }
            for _ in 0..20 {
                armed_block(&mut b, 512);
            }
        }
        if round == 3 {
            engine.locate_frame(0).unwrap();
        }
        for (k, d) in dests.iter().enumerate() {
            let v = f64::from(round * 7 + k as u32 % 10) / 70.0;
            send(*d, GesturePhase::Touch(v));
            send(*d, GesturePhase::Value(v / 2.0));
            send(*d, GesturePhase::Offset(1.5));
            send(
                *d,
                match round % 4 {
                    0 => GesturePhase::Release,
                    1 => GesturePhase::Glide(3000),
                    2 => GesturePhase::Hold,
                    _ => GesturePhase::Glide(0),
                },
            );
            armed_block(&mut b, 512);
        }
        // A full table.
        for k in 0..(MAX_LIVE as u64 + 4) {
            send(
                AutoDest::Plugin(PluginInstanceId(900), ParamId(k)),
                GesturePhase::Touch(0.5),
            );
        }
        for _ in 0..40 {
            armed_block(&mut b, 512);
        }
        for k in 0..(MAX_LIVE as u64 + 4) {
            send(
                AutoDest::Plugin(PluginInstanceId(900), ParamId(k)),
                GesturePhase::Release,
            );
        }
        if round == 2 {
            engine.load(&latent, seq).unwrap();
        }
        if round == 4 {
            engine.load(&doc, seq).unwrap();
            engine.stop().unwrap();
            armed_block(&mut b, 512);
            send(dests[0], GesturePhase::Touch(0.3));
            armed_block(&mut b, 512);
            engine.play().unwrap();
            // A tap replaced while playing.
            handles = engine.start_gestures().unwrap();
        }
        for _ in 0..30 {
            armed_block(&mut b, 512);
        }
    }
    engine.stop_gestures().unwrap();
    armed_block(&mut b, 512);
    // Draining is the control side's, outside the count.
    while handles.stamps.pop().is_ok() {}
    assert_eq!(rt_check::count(), before, "the render path allocated");
}
