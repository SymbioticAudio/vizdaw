//! Arpeggiator golden tests (plan 5g-1 Task 3; spec §6.8, §12.3;
//! Rulings 9–11): every mode, rates with dotted and triplet, octaves,
//! gate, velocity, latch, synced to the transport and free-running, and
//! the held-note capacity. 120 BPM at 48 kHz: a 1/16 step is 6000 frames.

use core_engine::input_fx::{At, ChainClock, InputChain, LiveMsg};
use core_engine::schedule::TempoTable;
use core_model::input_fx::{Arp, ArpMode, Feel, InputFx, InputFxKind, NoteValue};
use core_model::tempo::TempoMap;
use core_model::time::Tick;

const SR: f64 = 48_000.0;
const BLOCK: u64 = 256;

fn arp(f: impl FnOnce(&mut Arp)) -> InputChain {
    let mut a = Arp::default();
    f(&mut a);
    InputChain::new(&[InputFx::new(InputFxKind::Arpeggiator(a))])
}

/// Runs `c` for `frames` frames in 256-frame blocks (the last one short,
/// so the run ends at `frames`): `script` events at their frames, then a
/// tick. Playing: the transport runs from schedule
/// frame `from` at 120 BPM; stopped: free-running at 120 BPM. Outputs as
/// `(frame, "on 60 v100" | "off 60")`.
fn run(
    c: &mut InputChain,
    script: &[(u64, LiveMsg)],
    frames: u64,
    playing: bool,
    from: u64,
) -> Vec<(u64, String)> {
    let table = TempoTable::build(&TempoMap::default(), Tick(0), SR);
    let mut out = Vec::new();
    let mut b = 0;
    while b < frames {
        let n = BLOCK.min(frames - b);
        for (f, m) in script.iter().filter(|(f, _)| (b..b + n).contains(f)) {
            let k = (f - b) as u32;
            c.process(
                *m,
                At {
                    block: k,
                    record: i64::from(k),
                },
                &mut |a, m| out.push((b + u64::from(a.block), text(m))),
            );
        }
        let clock = ChainClock {
            frames: n as u32,
            sample_rate: SR,
            bpm: 120.0,
            transport: playing.then_some((&table, from + b)),
            record_lead: 0,
        };
        c.tick(&clock, &mut |a, m| {
            out.push((b + u64::from(a.block), text(m)))
        });
        b += n;
    }
    out
}

fn text(m: LiveMsg) -> String {
    match m {
        LiveMsg::NoteOn {
            pitch, velocity, ..
        } => format!("on {pitch} v{}", velocity >> 9),
        LiveMsg::NoteOff { pitch, .. } => format!("off {pitch}"),
        other => format!("{other:?}"),
    }
}

fn ons(out: &[(u64, String)]) -> Vec<(u64, u8)> {
    out.iter()
        .filter_map(|(f, t)| {
            t.strip_prefix("on ")
                .map(|r| (*f, r.split(' ').next().unwrap().parse().unwrap()))
        })
        .collect()
}

fn pitches(out: &[(u64, String)]) -> Vec<u8> {
    ons(out).into_iter().map(|(_, p)| p).collect()
}

fn press(at: u64, ps: &[u8]) -> Vec<(u64, LiveMsg)> {
    ps.iter().map(|p| (at, LiveMsg::on(*p, 100))).collect()
}

#[test]
fn up_synced_steps_on_the_sixteenths_with_half_gates() {
    let mut c = arp(|_| {});
    let out = run(&mut c, &press(0, &[60, 64, 67]), 24_000, true, 0);
    assert_eq!(
        out,
        vec![
            (0, "on 60 v100".into()),
            (3000, "off 60".into()),
            (6000, "on 64 v100".into()),
            (9000, "off 64".into()),
            (12_000, "on 67 v100".into()),
            (15_000, "off 67".into()),
            (18_000, "on 60 v100".into()),
            (21_000, "off 60".into()),
        ]
    );
}

#[test]
fn every_mode_has_its_order() {
    let held = press(0, &[67, 60, 64]);
    let mode = |m: ArpMode, octaves: u8, n: u64| {
        let mut c = arp(|a| {
            a.mode = m;
            a.octaves = octaves;
        });
        pitches(&run(&mut c, &held, n * 6000, true, 0))
    };
    assert_eq!(mode(ArpMode::Up, 1, 4), vec![60, 64, 67, 60]);
    assert_eq!(mode(ArpMode::Down, 1, 4), vec![67, 64, 60, 67]);
    assert_eq!(mode(ArpMode::UpDown, 1, 6), vec![60, 64, 67, 64, 60, 64]);
    assert_eq!(mode(ArpMode::DownUp, 1, 6), vec![67, 64, 60, 64, 67, 64]);
    assert_eq!(mode(ArpMode::AsPlayed, 1, 4), vec![67, 60, 64, 67]);
    assert_eq!(mode(ArpMode::Up, 2, 7), vec![60, 64, 67, 72, 76, 79, 60]);
    assert_eq!(mode(ArpMode::Down, 2, 3), vec![79, 76, 72]);
    assert_eq!(mode(ArpMode::AsPlayed, 2, 6), vec![67, 60, 64, 79, 72, 76]);
    // Octaves past 127 are dropped from the pattern.
    let mut c = arp(|a| a.octaves = 4);
    assert_eq!(
        pitches(&run(&mut c, &press(0, &[100]), 3 * 6000, true, 0)),
        vec![100, 112, 124]
    );
    // Chord: every held note at each step.
    let mut c = arp(|a| a.mode = ArpMode::Chord);
    let o = ons(&run(&mut c, &held, 12_000, true, 0));
    assert_eq!(
        o,
        vec![
            (0, 60),
            (0, 64),
            (0, 67),
            (6000, 60),
            (6000, 64),
            (6000, 67)
        ]
    );
}

#[test]
fn random_is_seeded_and_draws_from_the_pattern() {
    let held = press(0, &[60, 64, 67]);
    let draw = || {
        let mut c = arp(|a| a.mode = ArpMode::Random);
        pitches(&run(&mut c, &held, 64 * 6000, true, 0))
    };
    let a = draw();
    assert_eq!(a, draw(), "the same seed draws the same notes");
    assert_eq!(a.len(), 64);
    for p in [60, 64, 67] {
        assert!(a.contains(&p), "{a:?}");
    }
    assert!(a.iter().all(|p| [60, 64, 67].contains(p)));
}

#[test]
fn rates_include_dotted_and_triplet() {
    let step = |division, feel| {
        let mut c = arp(|a| a.rate = NoteValue { division, feel });
        let o = ons(&run(&mut c, &press(0, &[60]), 40_000, true, 0));
        o[1].0 - o[0].0
    };
    assert_eq!(step(16, Feel::Straight), 6000);
    assert_eq!(step(8, Feel::Triplet), 8000);
    assert_eq!(step(8, Feel::Dotted), 18_000);
    assert_eq!(step(32, Feel::Straight), 3000);
    assert_eq!(step(64, Feel::Triplet), 1000);
}

#[test]
fn a_synced_arp_waits_for_the_next_step() {
    let mut c = arp(|_| {});
    let o = ons(&run(&mut c, &press(1000, &[60, 64]), 13_000, true, 0));
    assert_eq!(o, vec![(6000, 60), (12_000, 64)]);
}

#[test]
fn a_locate_moves_the_synced_grid() {
    // The transport starts at schedule frame 4000: the next 1/16 is at
    // schedule frame 6000, 2000 frames into the run.
    let mut c = arp(|_| {});
    let o = ons(&run(&mut c, &press(0, &[60]), 9000, true, 4000));
    assert_eq!(o, vec![(2000, 60), (8000, 60)]);
}

/// Ruling 10: a locate, a wind or a loop wrap while keys are held
/// re-anchors the grid: no burst of missed steps, no silence.
#[test]
fn a_jump_while_held_neither_bursts_nor_stalls() {
    let frames = |o: Vec<(u64, u8)>| o.into_iter().map(|(f, _)| f).collect::<Vec<_>>();
    let mut c = arp(|_| {});
    let o = ons(&run(&mut c, &press(0, &[60, 64]), 23_000, true, 0));
    assert_eq!(frames(o), vec![0, 6000, 12_000, 18_000]);
    // Back to schedule frame 5000: the next sixteenth is 1000 frames in.
    let back = ons(&run(&mut c, &[], 12_000, true, 5000));
    assert_eq!(frames(back), vec![1000, 7000]);
    // Forward 50 steps and a half: nothing at offset 0, then the grid.
    let fwd = ons(&run(&mut c, &[], 12_000, true, 50 * 6000 + 3000));
    assert_eq!(frames(fwd), vec![3000, 9000]);
}

#[test]
fn free_running_starts_at_once_at_the_project_tempo() {
    let mut c = arp(|_| {});
    let o = ons(&run(&mut c, &press(1000, &[60, 64]), 14_000, false, 0));
    assert_eq!(o, vec![(1000, 60), (7000, 64), (13_000, 60)]);
}

#[test]
fn gate_and_fixed_velocity() {
    let mut c = arp(|a| {
        a.gate = 25;
        a.velocity = Some(90);
    });
    let out = run(&mut c, &press(0, &[60]), 6000, true, 0);
    assert_eq!(out, vec![(0, "on 60 v90".into()), (1500, "off 60".into())]);
}

#[test]
fn a_long_gate_on_one_note_ends_before_it_retriggers() {
    let mut c = arp(|a| a.gate = 150);
    let out = run(&mut c, &press(0, &[60]), 12_001, true, 0);
    assert_eq!(
        out,
        vec![
            (0, "on 60 v100".into()),
            (6000, "off 60".into()),
            (6000, "on 60 v100".into()),
            (12_000, "off 60".into()),
            (12_000, "on 60 v100".into()),
        ]
    );
}

#[test]
fn releasing_stops_the_arp_and_latch_keeps_it() {
    let mut script = press(0, &[60, 64]);
    script.push((7000, LiveMsg::off(60)));
    script.push((7000, LiveMsg::off(64)));
    let mut c = arp(|_| {});
    assert_eq!(
        pitches(&run(&mut c, &script, 30_000, true, 0)),
        vec![60, 64]
    );
    let mut c = arp(|a| a.latch = true);
    // Latched: it goes on; a new key with none down starts a new set.
    script.push((24_100, LiveMsg::on(67, 100)));
    // Steps at 30 000, 36 000 and 42 000 play 67; the run ends at 42 001.
    let p = pitches(&run(&mut c, &script, 42_001, true, 0));
    assert_eq!(p, vec![60, 64, 60, 64, 60, 67, 67, 67]);
}

#[test]
fn per_note_messages_of_held_notes_are_dropped_and_channel_messages_pass() {
    let mut c = arp(|_| {});
    let script = vec![
        (0, LiveMsg::on(60, 100)),
        (
            10,
            LiveMsg::PerNote {
                pitch: 60,
                expr: core_engine::input_fx::Expr::Pressure,
                value: 9,
            },
        ),
        (
            20,
            LiveMsg::Channel(core_engine::midi1::Midi1Msg::three(0xB0, 64, 127)),
        ),
    ];
    let out = run(&mut c, &script, 256, true, 0);
    assert!(out.iter().all(|(_, t)| !t.contains("PerNote")), "{out:?}");
    assert!(
        out.iter().any(|(f, t)| *f == 20 && t.contains("Channel")),
        "{out:?}"
    );
}

#[test]
fn the_33rd_held_note_is_dropped_and_counted() {
    let mut c = arp(|a| a.mode = ArpMode::Chord);
    let keys: Vec<u8> = (30..63).collect();
    assert_eq!(keys.len(), 33);
    let o = ons(&run(&mut c, &press(0, &keys), 256, true, 0));
    assert_eq!(o.len(), 32);
    assert!(
        !o.iter().any(|(_, p)| *p == 62),
        "the newest is the one dropped"
    );
    assert_eq!(c.take_dropped(), 1);
}

#[test]
fn generated_notes_carry_the_record_lead() {
    let table = TempoTable::build(&TempoMap::default(), Tick(0), SR);
    let mut c = arp(|_| {});
    c.process(
        LiveMsg::on(60, 100),
        At {
            block: 0,
            record: -40,
        },
        &mut |_, _| {},
    );
    let mut got = Vec::new();
    let clock = ChainClock {
        frames: 256,
        sample_rate: SR,
        bpm: 120.0,
        transport: Some((&table, 0)),
        record_lead: 64,
    };
    c.tick(&clock, &mut |a, m| got.push((a, text(m))));
    assert_eq!(
        got,
        vec![(
            At {
                block: 0,
                record: 64
            },
            "on 60 v100".to_string()
        )]
    );
}
