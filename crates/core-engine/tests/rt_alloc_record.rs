//! Recording never allocates on the render path (plan 5c Task 5; spec
//! §3, §12.3 "`rt-check` covers … the input ring"): the counting
//! allocator is armed only around `OfflineBackend::block`, while a record
//! tap captures fake input through loop wraps, a full ring, tap swaps, a
//! graph whose latency really changes and changes back (plan 5c-2 Task 6),
//! an xrun, a stop and the tap's removal; then a return-latency
//! measurement runs with the transport stopped, is replaced and is removed
//! mid-note, and the fake synth answers the part (plan 5c-2).

use core_engine::backend::AudioBackend;
use core_engine::engine::{Engine, EngineConfig};
use core_engine::fixtures::{Builder, Q};
use core_engine::graph::TestInserts;
use core_engine::input::{InputSpec, MidiIn, MidiInputSpec, MonitorRoute, port_key};
use core_engine::measure::PINGS;
use core_engine::midi1::Midi1Msg;
use core_engine::offline::{FAKE_SYNTH_PORT, FakeInput, FakeSynth, OfflineBackend, OfflineSinks};
use core_engine::record::RecordRequest;
use core_engine::rt_check::{self, CountingAlloc};
use core_media::caf::{CafWriter, WriteMode};
use core_media::stream::{DiskMode, MediaFile};
use core_model::command::{Command, LaneCmd, RoutingCmd};
use core_model::event::Fade;
use core_model::format::BitDepth;
use core_model::ids::AnyId;
use core_model::lane::MonitorMode;
use core_model::mixer::StripKind;

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
    // An audio lane "Mic" plays a region on the same output; a latent bus
    // (not the master) makes the audio path lead, and the monitor route
    // names "Mic": its region falls silent in the record range (plan 5d-1).
    let mic = bld.lane(seq, "Mic", None).unwrap();
    bld.run(Command::Lane(LaneCmd::SetOutput {
        seq,
        lane: mic,
        output: Some(path),
    }))
    .unwrap();
    let media = bld.media("Media/a.caf", 120_000, 2).unwrap();
    bld.region(
        seq,
        mic,
        0,
        media,
        0,
        120_000,
        0.0,
        Fade::default(),
        Fade::default(),
    )
    .unwrap();
    let latent_bus = bld.bus("Latent").unwrap();
    bld.insert(latent_bus, TestInserts::plugin("latency=64"))
        .unwrap();
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("a.caf");
    let mut w = CafWriter::create(&file, 48_000, 2, BitDepth::Float32, WriteMode::Normal).unwrap();
    w.write(&vec![0.25f32; 2 * 120_000]).unwrap();
    w.finish().unwrap();
    let doc = bld.finish();
    // The same document with a latent insert on the master: loaded
    // mid-recording and back, the graph latency really changes (5c final
    // review judgement 1).
    let mut lb = Builder::from_doc(doc.clone());
    let master = lb.strip_of(StripKind::Master).unwrap();
    lb.insert(master, TestInserts::plugin("latency=480"))
        .unwrap();
    let latent = lb.finish();
    let sinks = OfflineSinks::new(1 << 20).with_tone();
    let log = sinks.log();
    let mut cfg = EngineConfig::new(48_000.0);
    cfg.disk = DiskMode::Manual;
    let (mut engine, renderer) = Engine::new(cfg, Box::new(sinks)).unwrap();
    engine.set_media(vec![MediaFile {
        id: media,
        path: file,
    }]);
    engine.set_insert_factory(Box::new(TestInserts::new()));
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
            lane: mic,
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
    // The graph latencies the marks carried (read between blocks).
    // in order, consecutive repeats folded.
    let mut latencies: Vec<u64> = Vec::new();
    for i in 0..3_000u32 {
        engine.streamer().service();
        armed_block(&mut b, 1 + (i * 37) % 1024);
        // Drained (unarmed) except between blocks 1000 and 1500: the ring
        // fills and blocks are lost.
        if !(1000..1500).contains(&i) {
            while h.ring.pop(&mut buf).is_some() {}
            while let Ok(m) = h.marks.pop() {
                if latencies.last() != Some(&m.latency) {
                    latencies.push(m.latency);
                }
            }
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
        if i == 1200 {
            // A latent plugin comes in while recording: the graph swap
            // changes the latency and the tap marks it (final review I1).
            engine.load(&latent, seq).unwrap();
        }
        if i == 1500 {
            // Routes change (a reload, of the document without the latent
            // insert: the latency falls back): thru notes end, the old
            // routes retire.
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
    assert!(
        // The latent bus's 64 frames are the baseline.
        latencies.starts_with(&[64, 544, 64]),
        "the marks carried the latency change and its reversal: {latencies:?}"
    );
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
        .start_measure(&doc, synth, vec![0, 1], 0)
        .unwrap()
        .unwrap();
    for i in 0..400u32 {
        armed_block(&mut b, 1 + (i * 37) % 1024);
        if i % 100 == 0 {
            log.drain();
        }
    }
    assert!(m.done(), "the measurement ran to its end");
    let m = engine
        .start_measure(&doc, synth, vec![2], 0)
        .unwrap()
        .unwrap();
    for _ in 0..12 {
        armed_block(&mut b, 512);
    }
    assert!(m.sent(0).is_some());
    engine.stop_measure().unwrap();
    armed_block(&mut b, 512);
    assert!(m.closed());
    assert_eq!(rt_check::count() - before, 0, "allocations while measuring");
    drop(b.stop().unwrap());
    // Plan 5c-2: a measurement pings the fake synth with the transport
    // stopped and watches its answer; it is replaced, then removed with a
    // note sounding, while the part plays through the synth.
    let mut sb = Builder::new();
    let fake = sb.dest(FAKE_SYNTH_PORT, FAKE_SYNTH_PORT, 0).unwrap();
    let (fseq, flane) = sb.sequence("Synth", "Part", Some(fake)).unwrap();
    sb.notes(fseq, flane, &[(0, 60, 0x8000, Q), (2 * Q, 64, 0x8000, Q)])
        .unwrap();
    let sdoc = sb.finish();
    let fsynth = FakeSynth::new(25.0);
    let ssinks = OfflineSinks::new(1 << 16).with_fake_synth(&fsynth);
    let slog = ssinks.log();
    let (mut se, sr) = Engine::new(EngineConfig::new(48_000.0), Box::new(ssinks)).unwrap();
    se.set_return_dests(vec![fake]);
    se.load(&sdoc, fseq).unwrap();
    let mut sbk = OfflineBackend::new(48_000.0, 512)
        .with_input(FakeInput::Signals)
        .with_synth(&fsynth);
    sbk.start(sr).unwrap();
    sbk.reserve(4096);
    let status = se.start_measure(&sdoc, fake, vec![2], 0).unwrap().unwrap();
    let before = rt_check::count();
    for _ in 0..400 {
        armed_block(&mut sbk, 512);
        if status.done() {
            break;
        }
    }
    assert!(status.done(), "the measurement finished");
    assert!(
        (0..PINGS).all(|i| status.onset(i).is_some()),
        "the synth answered every note"
    );
    se.play().unwrap();
    let again = se.start_measure(&sdoc, fake, vec![2], 0).unwrap().unwrap();
    // Its first note goes out after the 100 ms lead and sounds 100 ms:
    // removed 15 blocks in, the note is sounding.
    for _ in 0..15 {
        armed_block(&mut sbk, 512);
    }
    assert!(
        again.sent(0).is_some(),
        "a note was sounding at the removal"
    );
    se.stop_measure().unwrap();
    armed_block(&mut sbk, 512);
    assert!(status.closed() && again.closed());
    for _ in 0..100 {
        armed_block(&mut sbk, 512);
    }
    // The part's second note (pitch 64; the pings are pitch 60) reached
    // the fake synth.
    assert!(
        slog.drain()
            .iter()
            .any(|r| r.bytes().first().is_some_and(|s| s & 0xF0 == 0x90)
                && r.bytes().get(1) == Some(&64)),
        "the part's notes reached the fake synth"
    );
    assert_eq!(
        rt_check::count() - before,
        0,
        "allocations while measuring and while the fake synth plays"
    );
    drop(sbk.stop().unwrap());
}
