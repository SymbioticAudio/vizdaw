use core_engine::backend::AudioBackend;
use core_engine::engine::{Engine, EngineConfig, find_sequence};
use core_engine::fixtures::{BAR, Builder, Q, demo_document};
use core_engine::offline::{EventLog, LogRecord, OfflineBackend, OfflineSinks};
use core_engine::render::Faults;
use core_engine::schedule::{BuiltinRoute, CompileReport};
use core_engine::transport::PlayState;
use core_model::document::{DestKind, Document};
use core_model::event::EventBody;
use core_model::ids::SeqId;
use proptest::prelude::*;

const SR: f64 = 48_000.0;

struct Run {
    engine: Engine,
    backend: OfflineBackend,
    log: EventLog,
    report: CompileReport,
}

fn start_with(
    doc: &Document,
    seq: SeqId,
    block: u32,
    cfg: EngineConfig,
    sinks: OfflineSinks,
) -> Run {
    let log = sinks.log();
    let (mut engine, renderer) = Engine::new(cfg, Box::new(sinks)).unwrap();
    let report = engine.load(doc, seq).unwrap();
    let mut backend = OfflineBackend::new(SR, block);
    backend.start(renderer).unwrap();
    Run {
        engine,
        backend,
        log,
        report,
    }
}

fn start(doc: &Document, seq: SeqId, block: u32) -> Run {
    start_with(
        doc,
        seq,
        block,
        EngineConfig::new(SR),
        OfflineSinks::new(100_000),
    )
}

impl Run {
    /// Plays until the end bit is set (tail included); returns the log.
    fn play_to_end(&mut self) -> String {
        self.engine.play().unwrap();
        for _ in 0..100_000 {
            self.backend.run_frames(512);
            if self.engine.take_ended() {
                return self.log.drain_text();
            }
        }
        panic!("never ended");
    }
}

fn one_bar() -> (Document, SeqId) {
    let mut b = Builder::new();
    let synth = b.dest("Synth", "builtin:synth", 0).unwrap();
    let (seq, lane) = b.sequence("Bar", "Notes", Some(synth)).unwrap();
    b.notes(
        seq,
        lane,
        &[
            (0, 60, 0x8000, Q / 2),
            (Q, 62, 0x8000, Q / 2),
            (2 * Q, 64, 0x8000, Q / 2),
            (3 * Q, 65, 0x8000, Q / 2),
        ],
    )
    .unwrap();
    (b.finish(), seq)
}

/// Short, stable summary of a long log: counts per status and channel,
/// first and last lines, and an FNV-1a hash of the whole text.
fn digest(log: &str) -> String {
    let mut counts = std::collections::BTreeMap::<String, usize>::new();
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for b in log.bytes() {
        hash ^= u64::from(b);
        hash = hash.wrapping_mul(0x0100_0000_01b3);
    }
    for line in log.lines() {
        let status = line.split(' ').nth(2).unwrap_or("?");
        *counts.entry(status.to_string()).or_default() += 1;
    }
    let lines: Vec<&str> = log.lines().collect();
    let mut s = format!("lines {} hash {hash:016x}\n", lines.len());
    for (k, v) in counts {
        s.push_str(&format!("{k}: {v}\n"));
    }
    s.push_str("first:\n");
    for l in lines.iter().take(8) {
        s.push_str(&format!("  {l}\n"));
    }
    s.push_str("last:\n");
    for l in lines.iter().rev().take(4).rev() {
        s.push_str(&format!("  {l}\n"));
    }
    s
}

#[test]
fn demo_plays_to_the_end() {
    let doc = demo_document().unwrap();
    let song = find_sequence(&doc, "Song").unwrap();
    let mut run = start(&doc, song, 512);
    assert!(run.report.unroutable.is_empty());
    assert!(run.report.warnings.is_empty(), "{:?}", run.report.warnings);
    let log = run.play_to_end();
    assert_eq!(run.engine.state(), PlayState::Stopped);
    assert!(run.engine.faults().is_empty());
    insta::assert_snapshot!(digest(&log), @r"
lines 335 hash 1feb33c1bfd60916
80: 36
81: 32
89: 96
90: 36
91: 32
99: 96
b0: 1
c0: 1
c1: 2
c9: 1
e0: 2
first:
  0 t0 c9 00
  0 t0 c1 21
  0 t0 c0 00
  0 t0 b0 07 64
  0 t0 99 2a 48
  0 t0 99 24 60
  0 t0 91 24 50
  0 t0 90 3c 38
last:
  1149589 t0 e0 00 40
  1176950 t0 80 37 00
  1176950 t0 80 3b 00
  1176950 t0 80 3e 00
");
}

#[test]
fn one_bar_at_120_bpm() {
    let (doc, seq) = one_bar();
    let mut run = start(&doc, seq, 512);
    insta::assert_snapshot!(run.play_to_end(), @r"
0 t0 90 3c 40
12000 t0 80 3c 00
24000 t0 90 3e 40
36000 t0 80 3e 00
48000 t0 90 40 40
60000 t0 80 40 00
72000 t0 90 41 40
84000 t0 80 41 00
");
}

#[test]
fn nested_reference_with_transpose_and_loop_count() {
    let mut b = Builder::new();
    let synth = b.dest("Synth", "builtin:synth", 0).unwrap();
    let (inner, il) = b.sequence("Inner", "L", Some(synth)).unwrap();
    b.notes(inner, il, &[(0, 60, 0x8000, Q), (2 * Q, 64, 0x8000, Q)])
        .unwrap();
    let (mid, ml) = b.sequence("Mid", "Refs", None).unwrap();
    b.reference(mid, ml, 0, inner, 2, 2).unwrap();
    let (top, tl) = b.sequence("Top", "Refs", None).unwrap();
    b.reference(top, tl, 0, mid, 1, 0).unwrap();
    b.reference(top, tl, 2 * BAR, inner, 1, -12).unwrap();
    let doc = b.finish();
    let mut run = start(&doc, top, 512);
    insta::assert_snapshot!(run.play_to_end(), @r"
0 t0 90 3e 40
24000 t0 80 3e 00
48000 t0 90 42 40
72000 t0 80 42 00
96000 t0 90 3e 40
120000 t0 80 3e 00
144000 t0 90 42 40
168000 t0 80 42 00
192000 t0 90 30 40
216000 t0 80 30 00
240000 t0 90 34 40
264000 t0 80 34 00
");
}

#[test]
fn loop_wraps_three_times_with_note_offs() {
    let mut b = Builder::new();
    let synth = b.dest("Synth", "builtin:synth", 0).unwrap();
    let (seq, lane) = b.sequence("Loop", "L", Some(synth)).unwrap();
    // The second note lasts to the bar end, so it is still on at the wrap.
    b.notes(seq, lane, &[(0, 60, 0x8000, Q), (2 * Q, 67, 0x8000, 2 * Q)])
        .unwrap();
    let doc = b.finish();
    let mut run = start(&doc, seq, 500);
    run.engine.set_loop(true).unwrap();
    run.engine.play().unwrap();
    // Three wraps at 96000, 192000 and 288000, then stop at 300000.
    run.backend.run_frames(300_000);
    run.engine.stop().unwrap();
    run.backend.run_frames(500);
    assert_eq!(run.engine.state(), PlayState::Stopped);
    insta::assert_snapshot!(run.log.drain_text(), @r"
0 t0 90 3c 40
24000 t0 80 3c 00
48000 t0 90 43 40
96000 t0 80 43 00
96000 t0 e0 00 40
96000 t0 b0 79 00
96000 t0 90 3c 40
120000 t0 80 3c 00
144000 t0 90 43 40
192000 t0 80 43 00
192000 t0 e0 00 40
192000 t0 b0 79 00
192000 t0 90 3c 40
216000 t0 80 3c 00
240000 t0 90 43 40
288000 t0 80 43 00
288000 t0 e0 00 40
288000 t0 b0 79 00
288000 t0 90 3c 40
300000 t0 80 3c 00
300000 t0 e0 00 40
300000 t0 b0 79 00
");
}

#[test]
fn program_and_bank_change() {
    let mut b = Builder::new();
    let synth = b.dest("Synth", "builtin:synth", 5).unwrap();
    let (seq, lane) = b.sequence("Patch", "L", Some(synth)).unwrap();
    b.event(
        seq,
        lane,
        0,
        EventBody::Program {
            program: 48,
            bank: Some((121, 1)),
        },
    )
    .unwrap();
    b.notes(seq, lane, &[(0, 60, 0x8000, Q)]).unwrap();
    b.event(
        seq,
        lane,
        2 * Q,
        EventBody::Program {
            program: 0,
            bank: None,
        },
    )
    .unwrap();
    b.notes(seq, lane, &[(2 * Q, 62, 0x8000, Q)]).unwrap();
    let doc = b.finish();
    let mut run = start(&doc, seq, 512);
    insta::assert_snapshot!(run.play_to_end(), @r"
0 t0 b5 00 79
0 t0 b5 20 01
0 t0 c5 30
0 t0 95 3c 40
24000 t0 85 3c 00
48000 t0 c5 00
48000 t0 95 3e 40
72000 t0 85 3e 00
");
}

#[test]
fn unroutable_plugin_is_reported_and_absent() {
    let mut b = Builder::new();
    let synth = b.dest("Synth", "builtin:synth", 0).unwrap();
    let pad = b
        .dest_kind(
            "Pad",
            DestKind::PluginInstrument {
                plugin_ref: "clap:pad".into(),
            },
            None,
        )
        .unwrap();
    let (seq, lane) = b.sequence("S", "Synth", Some(synth)).unwrap();
    b.notes(seq, lane, &[(0, 60, 0x8000, Q)]).unwrap();
    let pl = b.lane(seq, "Pad", Some(pad)).unwrap();
    b.notes(seq, pl, &[(0, 72, 0x8000, Q), (Q, 74, 0x8000, Q)])
        .unwrap();
    let doc = b.finish();
    let mut run = start(&doc, seq, 512);
    assert_eq!(run.report.unroutable.len(), 2);
    assert_eq!(run.report.warnings, vec![
        "2 event(s) not played: Pad: plugin instrument \"clap:pad\" cannot play until plugin hosting exists".to_string()
    ]);
    insta::assert_snapshot!(run.play_to_end(), @r"
0 t0 90 3c 40
24000 t0 80 3c 00
");
}

#[test]
fn stop_and_locate_send_note_offs() {
    let mut b = Builder::new();
    let synth = b.dest("Synth", "builtin:synth", 0).unwrap();
    let (seq, lane) = b.sequence("S", "L", Some(synth)).unwrap();
    b.notes(seq, lane, &[(0, 60, 0x8000, 4 * Q)]).unwrap();
    b.event(
        seq,
        lane,
        0,
        EventBody::Cc {
            index: 64,
            value: 0xFFFF_FFFF,
        },
    )
    .unwrap();
    let doc = b.finish();
    let mut run = start(&doc, seq, 100);
    run.engine.play().unwrap();
    run.backend.run_frames(1_000);
    run.engine.stop().unwrap();
    run.backend.run_frames(100);
    run.engine.locate_frame(0).unwrap();
    run.engine.play().unwrap();
    run.backend.run_frames(1_000);
    run.engine
        .locate_tick(core_model::time::Tick(2 * Q))
        .unwrap();
    run.backend.run_frames(100);
    assert_eq!(run.engine.position_frames(), 48_100);
    assert_eq!(
        run.engine.position_tick(),
        core_model::time::Tick(2 * Q + 64)
    );
    insta::assert_snapshot!(run.log.drain_text(), @r"
0 t0 b0 40 7f
0 t0 90 3c 40
1000 t0 80 3c 00
1000 t0 b0 40 00
1000 t0 e0 00 40
1000 t0 b0 79 00
1100 t0 e0 00 40
1100 t0 b0 79 00
1100 t0 b0 40 7f
1100 t0 90 3c 40
2100 t0 80 3c 00
2100 t0 b0 40 00
2100 t0 e0 00 40
2100 t0 b0 79 00
");
}

#[test]
fn schedule_swap_mid_play_keeps_position_and_cuts_notes() {
    let build = |bpm: u32, second: u8| {
        let mut b = Builder::new();
        let synth = b.dest("Synth", "builtin:synth", 0).unwrap();
        let (seq, lane) = b.sequence("S", "L", Some(synth)).unwrap();
        b.notes(
            seq,
            lane,
            &[(0, 60, 0x8000, BAR), (2 * Q, second, 0x8000, Q)],
        )
        .unwrap();
        b.tempo(seq, &[(0, bpm, false)]).unwrap();
        (b.finish(), seq)
    };
    let (old, seq) = build(120, 62);
    let mut run = start(&old, seq, 512);
    run.engine.play().unwrap();
    run.backend.run_frames(512 * 59);
    // Swap at device frame 30208: tick 30208 × 15360 / 24000 ≈ 19333 at
    // 120 BPM, which is frame 60416 at 60 BPM.
    assert_eq!(run.backend.frames(), 30_208);
    assert_eq!(run.engine.position_tick().0, 19_333);
    let (new, seq2) = build(60, 67);
    run.engine.load(&new, seq2).unwrap();
    run.backend.run_frames(512 * 100);
    assert_eq!(run.engine.position_frames(), 60_416 + 51_200);
    let log: Vec<String> = run.log.drain().iter().map(LogRecord::line).collect();
    assert_eq!(
        log,
        vec![
            // Old schedule only before the swap.
            "0 t0 90 3c 40".to_string(),
            // The sounding note is cut at the swap.
            "30208 t0 80 3c 00".to_string(),
            // Then the old schedule's channels are reset (ruling O23).
            "30208 t0 e0 00 40".to_string(),
            "30208 t0 b0 79 00".to_string(),
            // New schedule from the same tick: beat 3 (tick 30720) is frame
            // 96000 at 60 BPM, i.e. 96000 − 60416 frames after the swap.
            "65792 t0 90 43 40".to_string(),
        ]
    );
}

#[test]
fn full_midi_queue_raises_a_fault() {
    let (doc, seq) = one_bar();
    let mut sinks = OfflineSinks::new(16);
    sinks.midi_accepts = false;
    let mut cfg = EngineConfig::new(SR);
    cfg.route = BuiltinRoute::Virtual;
    let mut run = start_with(&doc, seq, 512, cfg, sinks);
    run.engine.play().unwrap();
    run.backend.run_frames(48_000);
    assert!(run.engine.faults().contains(Faults::MIDI_DROPPED));
    assert_eq!(run.engine.counters().midi_dropped, 4);
    assert!(run.engine.faults().is_empty(), "faults clear on read");
}

#[test]
fn oversized_block_is_silence_and_an_xrun() {
    let (doc, seq) = one_bar();
    let mut cfg = EngineConfig::new(SR);
    cfg.max_frames = 256;
    let mut run = start_with(&doc, seq, 512, cfg, OfflineSinks::new(100));
    run.engine.play().unwrap();
    run.backend.run_blocks([512]);
    assert!(run.engine.faults().contains(Faults::XRUN));
    assert_eq!(run.engine.position_frames(), 0);
    run.backend.run_blocks([256]);
    assert_eq!(run.engine.position_frames(), 256);
}

#[test]
fn swap_waits_while_the_retire_ring_is_full() {
    let (doc, seq) = one_bar();
    let mut run = start(&doc, seq, 64);
    run.engine.pause_collector(true);
    run.backend.run_blocks([64]);
    for _ in 0..8 {
        run.engine.load(&doc, seq).unwrap();
        run.backend.run_blocks([64]);
    }
    assert!(run.engine.faults().is_empty());
    run.engine.load(&doc, seq).unwrap();
    run.backend.run_blocks([64]);
    assert!(run.engine.faults().contains(Faults::SWAP_DEFERRED));
    assert_eq!(run.engine.counters().swaps_deferred, 1);
    run.engine.pause_collector(false);
    std::thread::sleep(std::time::Duration::from_millis(50));
    run.backend.run_blocks([64]);
    assert!(run.engine.faults().is_empty());
}

fn arb_doc() -> impl Strategy<Value = Vec<(i64, u8, i64, bool)>> {
    prop::collection::vec((0i64..BAR, 30u8..90, 1i64..BAR, any::<bool>()), 1..24)
}

fn render_with_blocks(doc: &Document, seq: SeqId, sizes: &[u32], looped: bool) -> String {
    let mut run = start(doc, seq, 512);
    run.engine.set_loop(looped).unwrap();
    run.engine.play().unwrap();
    let total = 2 * 96_000 + 48_000 + 7;
    let mut done = 0u64;
    let mut i = 0usize;
    while done < total {
        let n = sizes[i % sizes.len()].min((total - done) as u32);
        run.backend.run_blocks([n]);
        done += u64::from(n);
        i += 1;
    }
    run.log.drain_text()
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(12))]

    #[test]
    fn logs_do_not_depend_on_block_size(
        notes in arb_doc(),
        base in prop::sample::select(vec![1u32, 7, 64, 512, 4096]),
        jitter in prop::collection::vec(0u32..4096, 1..16),
        looped in any::<bool>(),
    ) {
        let mut b = Builder::new();
        let synth = b.dest("Synth", "builtin:synth", 0).unwrap();
        let (seq, lane) = b.sequence("P", "L", Some(synth)).unwrap();
        for (t, p, d, cc) in &notes {
            b.notes(seq, lane, &[(*t, *p, 0x8000, *d)]).unwrap();
            if *cc {
                b.event(seq, lane, *t, EventBody::Cc { index: 1, value: u32::from(*p) << 25 }).unwrap();
            }
        }
        let doc = b.finish();
        let reference = render_with_blocks(&doc, seq, &[512], looped);
        let sizes: Vec<u32> = jitter.iter().map(|j| 1 + j % base).collect();
        prop_assert_eq!(render_with_blocks(&doc, seq, &sizes, looped), reference);
    }
}

/// Log lines (`frame tN bytes`) of channel-voice status `status` (high
/// nibble and channel), in log order.
fn with_status<'a>(log: &'a [String], status: &str) -> Vec<&'a String> {
    log.iter()
        .filter(|l| l.split(' ').nth(2) == Some(status))
        .collect()
}

fn index_of(log: &[String], line: &str) -> usize {
    log.iter()
        .position(|l| l == line)
        .unwrap_or_else(|| panic!("{line:?} not in log:\n{}", log.join("\n")))
}

#[test]
fn locate_into_bar_3_chases_patch_and_volume() {
    let doc = demo_document().unwrap();
    let song = find_sequence(&doc, "Song").unwrap();
    let mut run = start(&doc, song, 512);
    run.engine
        .locate_tick(core_model::time::Tick(2 * BAR))
        .unwrap();
    run.engine.play().unwrap();
    run.backend.run_frames(48_000);
    let log: Vec<String> = run.log.drain().iter().map(LogRecord::line).collect();
    // Resets on every used channel (0, 1, 9) at the locate.
    for ch in ["0", "1", "9"] {
        index_of(&log, &format!("0 t0 e{ch} 00 40"));
        index_of(&log, &format!("0 t0 b{ch} 79 00"));
    }
    // Bass (channel 1): program 33 before its first note.
    let pc = index_of(&log, "0 t0 c1 21");
    let first_bass = log.iter().position(|l| l.contains(" t0 91 ")).unwrap();
    assert!(pc < first_bass, "{}", log.join("\n"));
    assert!(index_of(&log, "0 t0 b1 79 00") < pc);
    // Chords (channel 0): program 0 and CC7 100 before the first chord.
    let first_chord = log.iter().position(|l| l.contains(" t0 90 ")).unwrap();
    assert!(index_of(&log, "0 t0 c0 00") < first_chord);
    assert!(index_of(&log, "0 t0 b0 07 64") < first_chord);
    // Drums keep their default patch.
    assert!(index_of(&log, "0 t0 c9 00") < log.iter().position(|l| l.contains(" t0 99 ")).unwrap());
    // Nothing was chased twice.
    assert_eq!(with_status(&log, "c1").len(), 1);
}

#[test]
fn stop_inside_the_bar_12_bend_recentres_it() {
    let doc = demo_document().unwrap();
    let song = find_sequence(&doc, "Song").unwrap();
    let mut run = start(&doc, song, 512);
    run.engine
        .locate_tick(core_model::time::Tick(11 * BAR))
        .unwrap();
    run.engine.play().unwrap();
    // Play past the bend up (beat 3) but not to the recentre (beat 4).
    while run.engine.position_tick().0 < 11 * BAR + 2 * Q + Q / 2 {
        run.backend.run_frames(512);
    }
    run.engine.stop().unwrap();
    run.backend.run_frames(512);
    run.engine.locate_frame(0).unwrap();
    run.engine.play().unwrap();
    run.backend.run_frames(512);
    let log: Vec<String> = run.log.drain().iter().map(LogRecord::line).collect();
    let bends = with_status(&log, "e0");
    let up = bends
        .iter()
        .position(|l| l.ends_with("e0 00 48"))
        .unwrap_or_else(|| panic!("no bend up:\n{}", log.join("\n")));
    assert!(
        bends.iter().skip(up + 1).any(|l| l.ends_with("e0 00 40")),
        "no recentre after the stop:\n{}",
        log.join("\n")
    );
}

#[test]
fn loop_wrap_resends_the_chased_state() {
    let doc = demo_document().unwrap();
    let song = find_sequence(&doc, "Song").unwrap();
    let mut run = start(&doc, song, 512);
    // Bars 3–4 at 120 BPM: frames 192000..384000.
    run.engine.set_loop_range(Some((192_000, 384_000))).unwrap();
    run.engine.locate_frame(192_000).unwrap();
    run.engine.play().unwrap();
    run.backend.run_frames(192_000 + 4_800);
    let log: Vec<String> = run.log.drain().iter().map(LogRecord::line).collect();
    for at in [0, 192_000] {
        let reset = index_of(&log, &format!("{at} t0 b1 79 00"));
        let pc = index_of(&log, &format!("{at} t0 c1 21"));
        assert!(reset < pc);
        index_of(&log, &format!("{at} t0 e0 00 40"));
        index_of(&log, &format!("{at} t0 c0 00"));
        index_of(&log, &format!("{at} t0 b0 07 64"));
    }
}
