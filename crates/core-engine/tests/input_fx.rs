//! The input effect chain's stateless effects and its output stage (plan
//! 5g-1 Task 2; spec §6.8, §12.3; Rulings 4–8, 12).

use core_engine::input_fx::{
    At, CENTER, Expr, InputChain, LiveMsg, MAX_FANOUT, NoteExpr, RecordQuantize, up7, up32,
};
use core_engine::midi1::Midi1Msg;
use core_model::input_fx::{InputFx, InputFxKind, VelocityFx};
use core_model::time::Tick;

fn chain(fx: Vec<InputFxKind>) -> InputChain {
    InputChain::new(&fx.into_iter().map(InputFx::new).collect::<Vec<_>>())
}

fn transpose(s: i8) -> InputFxKind {
    InputFxKind::Transpose { semitones: s }
}

fn chord(i: &[i8]) -> InputFxKind {
    InputFxKind::Chord {
        intervals: i.to_vec(),
    }
}

/// What `c` outputs for `msgs`, as text: "on 60 v100", "off 60",
/// "pn 60 pressure 0x…", "ch b0 01 40".
fn feed(c: &mut InputChain, msgs: &[LiveMsg]) -> Vec<String> {
    let mut out = Vec::new();
    for m in msgs {
        c.process(*m, At::NOW, &mut |_, m| out.push(text(m)));
    }
    out
}

fn text(m: LiveMsg) -> String {
    match m {
        LiveMsg::NoteOn {
            pitch, velocity, ..
        } => format!("on {pitch} v{}", velocity >> 9),
        LiveMsg::NoteOff { pitch, .. } => format!("off {pitch}"),
        LiveMsg::PerNote { pitch, expr, value } => format!("pn {pitch} {expr:?} {value:#x}"),
        LiveMsg::Channel(m) => format!(
            "ch {:02x} {:02x} {:02x}",
            m.bytes[0], m.bytes[1], m.bytes[2]
        ),
    }
}

fn on_(pitch: u8, v: u8) -> LiveMsg {
    LiveMsg::on(pitch, v)
}

fn off_(pitch: u8) -> LiveMsg {
    LiveMsg::off(pitch)
}

#[test]
fn midi1_maps_to_live_messages_and_back() {
    let on = LiveMsg::from_midi1(Midi1Msg::three(0x93, 60, 100));
    assert_eq!(
        on,
        LiveMsg::NoteOn {
            pitch: 60,
            velocity: up7(100),
            expr: NoteExpr::default()
        }
    );
    assert_eq!(up7(100), 0xC924);
    assert_eq!(up7(127), 0xFFFF);
    assert_eq!(up7(64), 0x8000);
    assert_eq!(on.to_midi1(5), Some(Midi1Msg::three(0x95, 60, 100)));
    assert_eq!(
        LiveMsg::from_midi1(Midi1Msg::three(0x90, 60, 0)),
        LiveMsg::NoteOff {
            pitch: 60,
            velocity: 0
        }
    );
    let off = LiveMsg::from_midi1(Midi1Msg::three(0x80, 60, 64));
    assert_eq!(off.to_midi1(0), Some(Midi1Msg::three(0x80, 60, 64)));
    let poly = LiveMsg::from_midi1(Midi1Msg::three(0xA1, 61, 64));
    assert_eq!(
        poly,
        LiveMsg::PerNote {
            pitch: 61,
            expr: Expr::Pressure,
            value: up32(64, 7)
        }
    );
    assert_eq!(poly.to_midi1(2), Some(Midi1Msg::three(0xA2, 61, 64)));
    let cc = LiveMsg::from_midi1(Midi1Msg::three(0xB4, 1, 64));
    assert_eq!(cc.to_midi1(9), Some(Midi1Msg::three(0xB9, 1, 64)));
    // Per-note bend and timbre have no MIDI 1.0 form (Ruling 4).
    assert_eq!(
        LiveMsg::PerNote {
            pitch: 60,
            expr: Expr::Bend,
            value: CENTER
        }
        .to_midi1(0),
        None
    );
    assert_eq!(
        LiveMsg::PerNote {
            pitch: 60,
            expr: Expr::Timbre,
            value: CENTER
        }
        .to_midi1(0),
        None
    );
    for v in 0..=127u8 {
        assert_eq!((up7(v) >> 9) as u8, v);
        assert_eq!((up32(u32::from(v), 7) >> 25) as u8, v);
    }
}

#[test]
fn an_empty_chain_passes_input_unchanged() {
    let mut c = InputChain::default();
    assert!(c.config().is_empty());
    let cc = LiveMsg::Channel(Midi1Msg::three(0xB0, 64, 127));
    assert_eq!(
        feed(&mut c, &[on_(60, 100), cc, off_(60)]),
        vec!["on 60 v100", "ch b0 40 7f", "off 60"]
    );
}

#[test]
fn transpose_moves_notes_and_their_per_note_messages() {
    let mut c = chain(vec![transpose(12)]);
    let pn = LiveMsg::PerNote {
        pitch: 60,
        expr: Expr::Pressure,
        value: 5,
    };
    assert_eq!(
        feed(&mut c, &[on_(60, 100), pn, off_(60)]),
        vec!["on 72 v100", "pn 72 Pressure 0x5", "off 72"]
    );
}

#[test]
fn out_of_range_pitches_are_dropped_not_folded() {
    let mut c = chain(vec![transpose(12)]);
    assert!(feed(&mut c, &[on_(120, 100), off_(120)]).is_empty());
    let mut c = chain(vec![transpose(-48)]);
    assert!(feed(&mut c, &[on_(40, 100), off_(40)]).is_empty());
    let mut c = chain(vec![chord(&[12, -12])]);
    assert_eq!(
        feed(&mut c, &[on_(120, 100)]),
        vec!["on 120 v100", "on 108 v100"]
    );
}

#[test]
fn chord_adds_its_intervals_and_ends_them() {
    let mut c = chain(vec![chord(&[4, 7])]);
    assert_eq!(
        feed(&mut c, &[on_(60, 90), off_(60)]),
        vec![
            "on 60 v90",
            "on 64 v90",
            "on 67 v90",
            "off 60",
            "off 64",
            "off 67"
        ]
    );
}

#[test]
fn chain_order_matters_where_a_drop_applies() {
    let mut a = chain(vec![transpose(10), chord(&[-12])]);
    let mut b = chain(vec![chord(&[-12]), transpose(10)]);
    assert!(feed(&mut a, &[on_(120, 100)]).is_empty());
    assert_eq!(feed(&mut b, &[on_(120, 100)]), vec!["on 118 v100"]);
    // Velocity clamps are order-dependent too.
    let fixed = |v| {
        InputFxKind::Velocity(VelocityFx {
            fixed: Some(v),
            ..VelocityFx::default()
        })
    };
    let half = InputFxKind::Velocity(VelocityFx {
        scale: 50,
        ..VelocityFx::default()
    });
    assert_eq!(
        feed(&mut chain(vec![fixed(100), half.clone()]), &[on_(60, 10)]),
        vec!["on 60 v50"]
    );
    assert_eq!(
        feed(&mut chain(vec![half, fixed(100)]), &[on_(60, 10)]),
        vec!["on 60 v100"]
    );
}

#[test]
fn velocity_scales_offsets_clamps_or_fixes() {
    let v =
        |f: VelocityFx, vel: u8| feed(&mut chain(vec![InputFxKind::Velocity(f)]), &[on_(60, vel)]);
    let d = VelocityFx::default();
    assert_eq!(v(VelocityFx { scale: 50, ..d }, 100), vec!["on 60 v50"]);
    assert_eq!(v(VelocityFx { offset: 10, ..d }, 100), vec!["on 60 v110"]);
    assert_eq!(
        v(VelocityFx { offset: -127, ..d }, 100),
        vec!["on 60 v1"],
        "never 0: a note-on stays one"
    );
    assert_eq!(v(VelocityFx { max: 100, ..d }, 120), vec!["on 60 v100"]);
    assert_eq!(v(VelocityFx { min: 20, ..d }, 1), vec!["on 60 v20"]);
    assert_eq!(v(VelocityFx { scale: 400, ..d }, 100), vec!["on 60 v127"]);
    assert_eq!(
        v(
            VelocityFx {
                scale: 0,
                min: 30,
                ..d
            },
            100
        ),
        vec!["on 60 v30"]
    );
    assert_eq!(
        v(
            VelocityFx {
                fixed: Some(64),
                ..d
            },
            3
        ),
        vec!["on 60 v64"]
    );
    // Note-offs are untouched.
    let mut c = chain(vec![InputFxKind::Velocity(VelocityFx {
        fixed: Some(64),
        ..d
    })]);
    assert_eq!(
        feed(
            &mut c,
            &[
                on_(60, 3),
                LiveMsg::NoteOff {
                    pitch: 60,
                    velocity: up7(99)
                }
            ]
        )
        .len(),
        2
    );
}

#[test]
fn a_bypassed_effect_passes_its_input() {
    let mut c = InputChain::new(&[InputFx {
        kind: transpose(12),
        bypass: true,
    }]);
    assert_eq!(feed(&mut c, &[on_(60, 100)]), vec!["on 60 v100"]);
}

#[test]
fn a_pitch_two_sources_reach_sounds_once_and_ends_once() {
    let mut c = chain(vec![chord(&[12])]);
    assert_eq!(
        feed(&mut c, &[on_(60, 100), on_(72, 100), off_(60), off_(72)]),
        vec![
            "on 60 v100",
            "on 72 v100",
            "on 84 v100",
            "off 60",
            "off 72",
            "off 84"
        ]
    );
    // An off for a note the chain never started is dropped (Ruling 6).
    assert!(feed(&mut c, &[off_(50)]).is_empty());
    // Counts saturate at 255: many hits end on the last off counted.
    let mut c = InputChain::default();
    let mut many = vec![on_(60, 100); 300];
    many.extend(vec![off_(60); 300]);
    assert_eq!(feed(&mut c, &many), vec!["on 60 v100", "off 60"]);
}

#[test]
fn flush_ends_every_sounding_note_once() {
    let mut c = chain(vec![chord(&[4, 7])]);
    feed(&mut c, &[on_(60, 100)]);
    let mut out = Vec::new();
    let at = At {
        block: 7,
        record: -3,
    };
    c.flush(at, &mut |a, m| out.push((a, text(m))));
    assert_eq!(
        out,
        vec![
            (at, "off 60".to_string()),
            (at, "off 64".to_string()),
            (at, "off 67".to_string())
        ]
    );
    let mut again = Vec::new();
    c.flush(At::NOW, &mut |_, m| again.push(text(m)));
    assert!(again.is_empty(), "flushed once");
    assert!(
        feed(&mut c, &[off_(60)]).is_empty(),
        "the late key-up is dropped"
    );
}

#[test]
fn fan_out_is_capped_and_counted() {
    let six = chord(&[1, 2, 3, 4, 5, 6]);
    let mut c = chain(vec![six.clone(), six.clone(), six.clone(), six]);
    let got = feed(&mut c, &[on_(30, 100)]);
    assert!(got.len() <= MAX_FANOUT, "{}", got.len());
    assert_eq!(c.take_dropped(), 1, "one truncated event");
    assert_eq!(c.take_dropped(), 0, "taken once");
}

/// Ruling 7: the cap bounds the work, not only the outputs. 7^8 leaves are
/// never visited; the note-off ends exactly the notes the cap let start.
#[test]
fn eight_chained_chords_stop_at_the_cap() {
    let mut c = chain(vec![chord(&[1, 2, 3, 4, 5, 6]); 8]);
    let got = feed(&mut c, &[on_(30, 100)]);
    assert!(!got.is_empty() && got.len() <= MAX_FANOUT, "{}", got.len());
    assert_eq!(c.take_dropped(), 1);
    let visits = c.take_visits();
    assert!(
        visits <= (8 * 7 * MAX_FANOUT) as u64,
        "{visits} stage visits"
    );
    let offs = feed(&mut c, &[off_(30)]);
    assert_eq!(
        offs.len(),
        got.len(),
        "every note it started ends: {offs:?}"
    );
    assert!(offs.iter().all(|t| t.starts_with("off ")));
}

#[test]
fn carry_takes_state_only_between_equal_chains() {
    let mut old = chain(vec![chord(&[4])]);
    feed(&mut old, &[on_(60, 100)]);
    let mut same = chain(vec![chord(&[4])]);
    assert_eq!(same, old);
    same.carry_from(&mut old);
    assert_eq!(
        feed(&mut same, &[off_(60)]),
        vec!["off 60", "off 64"],
        "it knew the notes"
    );
    let mut other = chain(vec![chord(&[3])]);
    assert_ne!(other, old);
    other.carry_from(&mut old);
    assert!(
        feed(&mut other, &[off_(60)]).is_empty(),
        "a different chain starts fresh"
    );
}

#[test]
fn quantize_is_the_first_active_input_quantize() {
    let q = |grid, strength, bypass| InputFx {
        kind: InputFxKind::InputQuantize {
            grid: Tick(grid),
            strength,
        },
        bypass,
    };
    assert_eq!(
        InputChain::new(&[q(3840, 80, false)]).quantize(),
        Some(RecordQuantize {
            grid: 3840,
            strength: 80
        })
    );
    assert_eq!(InputChain::new(&[q(3840, 80, true)]).quantize(), None);
    // Input Quantize never changes what plays (Ruling 12).
    let mut c = InputChain::new(&[q(3840, 100, false)]);
    assert_eq!(feed(&mut c, &[on_(60, 100)]), vec!["on 60 v100"]);
}
