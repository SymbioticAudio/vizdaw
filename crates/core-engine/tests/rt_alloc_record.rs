//! Recording never allocates on the render path (plan 5c Task 5; spec
//! §3, §12.3 "`rt-check` covers … the input ring"): the counting
//! allocator is armed only around `OfflineBackend::block`, while a record
//! tap captures fake input through loop wraps, a full ring, tap swaps, an
//! xrun, a stop and the tap's removal; then a return-latency measurement
//! runs, is replaced and is removed mid-note, and one finds the fake
//! synth's onsets.

use core_engine::backend::AudioBackend;
use core_engine::engine::{Engine, EngineConfig};
use core_engine::fixtures::{Builder, Q};
use core_engine::input::{InputSpec, MidiIn, MidiInputSpec, MonitorRoute, port_key};
use core_engine::measure::PINGS;
use core_engine::midi1::Midi1Msg;
use core_engine::offline::{FAKE_SYNTH_PORT, FakeInput, FakeSynth, OfflineBackend, OfflineSinks};
use core_engine::record::RecordRequest;
use core_engine::rt_check::{self, CountingAlloc};
use core_model::command::{Command, LaneCmd, RoutingCmd};
use core_model::ids::AnyId;
use core_model::lane::MonitorMode;

#[global_allocator]
static ALLOC: CountingAlloc = CountingAlloc;

fn armed_block(b: &mut OfflineBackend, frames: u32) {
    rt_check::arm();
    b.block(frames);
    rt_check::disarm();
}

/// One test, so no other test thread allocates while the global counter
/// is compared.
#[test]
fn recording_never_allocates() {
    let mut bld = Builder::new();
    let synth = bld.dest("Synth", "builtin:synth", 0).unwrap();
    let (seq, lane) = bld.sequence("Bar", "Notes", Some(synth)).unwrap();
    bld.notes(seq, lane, &[(0, 60, 0x8000, Q), (2 * Q, 67, 0x8000, Q)])
        .unwrap();
    let r = bld
        .run(Command::Routing(RoutingCmd::AddOutput {
            name: "Audio 1".into(),
            reference: "audio:1-2".into(),
        }))
        .unwrap();
    let path = r
        .created
        .iter()
        .find_map(|id| match id {
            AnyId::Output(o) => Some(*o),
            _ => None,
        })
        .unwrap();
    bld.run(Command::Lane(LaneCmd::SetOutput {
        seq,
        lane,
        output: Some(path),
    }))
    .unwrap();
    let doc = bld.finish();
    let sinks = OfflineSinks::new(1 << 20).with_tone();
    let log = sinks.log();
    let (mut engine, renderer) = Engine::new(EngineConfig::new(48_000.0), Box::new(sinks)).unwrap();
    // A MIDI lane with thru and recording, and a monitored input.
    let inputs = InputSpec {
        midi: vec![MidiInputSpec {
            lane,
            port: None,
            channel: None,
            dest: Some(synth),
            thru: true,
            armed: true,
        }],
        monitor: vec![MonitorRoute {
            path,
            left: 0,
            right: 1,
            mode: MonitorMode::Input,
            armed: true,
        }],
    };
    engine.set_inputs(inputs.clone());
    engine.load(&doc, seq).unwrap();
    let midi = engine.midi_sender();
    let mut b = OfflineBackend::new(48_000.0, 512).with_input(FakeInput::Loopback(1000));
    b.start(renderer).unwrap();
    b.reserve(4096);
    engine.set_loop_range(Some((0, 30_000))).unwrap();
    let mut h = engine
        .start_recording(&RecordRequest {
            channels: vec![0, 1, 2],
            range: Some((1000, 20_000)),
            ring_frames: 48_000,
        })
        .unwrap();
    engine.play().unwrap();
    let before = rt_check::count();
    let mut buf = Vec::with_capacity(1 << 20);
    for i in 0..3_000u32 {
        armed_block(&mut b, 1 + (i * 37) % 1024);
        // Drained (unarmed) except between blocks 1000 and 1500: the ring
        // fills and blocks are lost.
        if !(1000..1500).contains(&i) {
            while h.ring.pop(&mut buf).is_some() {}
            while h.marks.pop().is_ok() {}
        }
        if i % 100 == 0 {
            log.drain();
            while h.midi.pop().is_ok() {}
        }
        if i % 7 == 0 {
            let on = i % 14 == 0;
            let msg = Midi1Msg::three(if on { 0x90 } else { 0x80 }, 60 + (i % 12) as u8, 90);
            midi.send(MidiIn {
                port: port_key("Keys"),
                host_ns: 0,
                msg,
            });
        }
        if i == 1500 {
            // Routes change (a reload): thru notes end, the old routes
            // retire.
            let mut changed = inputs.clone();
            changed.monitor[0].mode = MonitorMode::Off;
            engine.set_inputs(changed);
            engine.load(&doc, seq).unwrap();
        }
        if i == 2500 {
            // A jump while recording is marked (review fix M1).
            engine.locate_frame(5000).unwrap();
        }
        if i == 2000 {
            // A new tap replaces the old one while playing.
            h = engine
                .start_recording(&RecordRequest {
                    channels: vec![2],
                    range: None,
                    ring_frames: 4800,
                })
                .unwrap();
        }
    }
    assert!(h.status.run() > 0);
    assert_eq!(rt_check::count() - before, 0, "allocations while recording");
    // An xrun while recording, then the tap goes.
    armed_block(&mut b, 8192);
    // The transport stops (the tap marks it, pre-flight M2), then the tap
    // goes: the removed tap is retired, never freed here.
    engine.stop().unwrap();
    armed_block(&mut b, 512);
    // Playing again resumes the tap (a resume mark and a fresh delay).
    engine.play().unwrap();
    armed_block(&mut b, 512);
    engine.stop_recording().unwrap();
    armed_block(&mut b, 512);
    assert!(h.status.closed());
    assert_eq!(
        rt_check::count() - before,
        0,
        "allocations on xrun and stop"
    );
    // A return-latency measurement (plan 5c-2 Task 5) with the transport
    // stopped: installed, its five notes sent and watched for, then
    // removed mid-note on a second run (its note-off sent, the tap
    // retired).
    engine.stop().unwrap();
    let m = engine
        .start_measure(&doc, synth, vec![0, 1])
        .unwrap()
        .unwrap();
    for i in 0..400u32 {
        armed_block(&mut b, 1 + (i * 37) % 1024);
        if i % 100 == 0 {
            log.drain();
        }
    }
    assert!(m.done(), "the measurement ran to its end");
    let m = engine.start_measure(&doc, synth, vec![2]).unwrap().unwrap();
    for _ in 0..12 {
        armed_block(&mut b, 512);
    }
    assert!(m.sent(0).is_some());
    engine.stop_measure().unwrap();
    armed_block(&mut b, 512);
    assert!(m.closed());
    assert_eq!(rt_check::count() - before, 0, "allocations while measuring");
    drop(b.stop().unwrap());
    // The built-in synth's test tone peaks at -40 dBFS, never above it, so
    // it gives no onset: the fake synth answers instead, so the onset
    // branch runs while armed (fix round 1).
    let mut fb = Builder::new();
    let fake = fb.dest(FAKE_SYNTH_PORT, FAKE_SYNTH_PORT, 0).unwrap();
    let (fseq, _) = fb.sequence("S", "Part", Some(fake)).unwrap();
    let fdoc = fb.finish();
    let fsynth = FakeSynth::new(25.0);
    let (mut fe, fr) = Engine::new(
        EngineConfig::new(48_000.0),
        Box::new(OfflineSinks::new(1 << 16).with_fake_synth(&fsynth)),
    )
    .unwrap();
    fe.set_return_dests(vec![fake]);
    fe.load(&fdoc, fseq).unwrap();
    let mut fbk = OfflineBackend::new(48_000.0, 512)
        .with_input(FakeInput::Signals)
        .with_synth(&fsynth);
    fbk.start(fr).unwrap();
    let m = fe.start_measure(&fdoc, fake, vec![2]).unwrap().unwrap();
    let before = rt_check::count();
    for _ in 0..300 {
        armed_block(&mut fbk, 512);
    }
    assert!(m.done());
    assert!(
        (0..PINGS).all(|i| m.onset(i).is_some()),
        "every note answered"
    );
    assert_eq!(rt_check::count() - before, 0, "allocations finding onsets");
    drop(fbk.stop().unwrap());
}
